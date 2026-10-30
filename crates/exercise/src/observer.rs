//! Observer dashboard: read-only counts of what the players did, per turn and
//! cumulated, overall and per role, plus the exercise's pace (injects sent
//! against the MSEL, wall time since the start, where the scenario stands).
//!
//! Everything is derived from records the exercise already keeps (proposals,
//! linked conversations, messages, the review board, the log); nothing here
//! changes the exercise. Turns already resolved are final; the open turn is
//! reported too, flagged `closed: false`, so the UI can mark it provisional.
use crate::{reply_review::ItemState, BasisKind, Exercise, MsgKind, Phase, Source, MASTER};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::AddAssign;

/// What the responses declared they rest on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BasisCounts {
    pub plan: u32,
    pub procedure: u32,
    pub law: u32,
    pub judgment: u32,
    /// Responses with no basis (communications).
    pub none: u32,
}

impl BasisCounts {
    fn add(&mut self, kind: Option<BasisKind>) {
        match kind {
            Some(BasisKind::Plan) => self.plan += 1,
            Some(BasisKind::Procedure) => self.procedure += 1,
            Some(BasisKind::Law) => self.law += 1,
            Some(BasisKind::OperationalJudgment) => self.judgment += 1,
            None => self.none += 1,
        }
    }
}

impl AddAssign for BasisCounts {
    fn add_assign(&mut self, o: Self) {
        self.plan += o.plan;
        self.procedure += o.procedure;
        self.law += o.law;
        self.judgment += o.judgment;
        self.none += o.none;
    }
}

/// Activity counts of one role, or of everyone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Activity {
    /// Inject copies received.
    pub received: u32,
    /// Responses to an inject's sender (the players' decisions and reports).
    pub responses: u32,
    /// Messages between players in the conversations linked to an inject.
    pub interactions: u32,
    /// Linked conversations opened.
    pub conversations: u32,
    /// Answers to EXCON questions about a reply.
    pub answers: u32,
    /// What the responses rest on.
    pub basis: BasisCounts,
    /// Catalogue actions the AI read in the role's words.
    pub suggested: u32,
    /// Of those, executed by the EXCON.
    pub executed: u32,
}

impl Activity {
    /// Everything the role wrote: responses, coordination and answers.
    pub fn total(&self) -> u32 {
        self.responses + self.interactions + self.answers
    }
}

impl AddAssign<&Activity> for Activity {
    fn add_assign(&mut self, o: &Activity) {
        self.received += o.received;
        self.responses += o.responses;
        self.interactions += o.interactions;
        self.conversations += o.conversations;
        self.answers += o.answers;
        self.basis += o.basis;
        self.suggested += o.suggested;
        self.executed += o.executed;
    }
}

/// Counts of one turn or cumulated up to it: overall and per role.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Block {
    pub overall: Activity,
    pub roles: BTreeMap<String, Activity>,
    /// MSEL injects first sent (in this turn, or up to it).
    pub injects_sent: u32,
    /// Custom (non-MSEL) injects sent.
    pub custom_injects: u32,
}

impl Block {
    fn role(&mut self, role: &str) -> &mut Activity {
        self.roles.entry(role.to_string()).or_default()
    }
    /// Recomputes `overall` from the roles.
    fn sum(&mut self) {
        let mut overall = Activity::default();
        for a in self.roles.values() {
            overall += a;
        }
        self.overall = overall;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnStats {
    pub turn: u32,
    /// Resolved: its counts are final.
    pub closed: bool,
    /// Simulated seconds from the scenario start.
    pub start_s: i64,
    pub end_s: Option<i64>,
    /// Wall-clock milliseconds (UNIX epoch).
    pub opened_wall_ms: Option<u128>,
    pub closed_wall_ms: Option<u128>,
    /// This turn only.
    pub turn_only: Block,
    /// Every turn up to and including this one.
    pub cumulative: Block,
}

/// One inject (an MSEL entry, or a custom inject delivery).
#[derive(Debug, Clone, Serialize)]
pub struct InjectStats {
    /// MSEL id, or `custom-<delivery>`.
    pub id: String,
    pub title: String,
    pub sender: String,
    pub msel: bool,
    /// Suggested turn in the MSEL.
    pub planned_turn: Option<u32>,
    pub planned_s: Option<i64>,
    /// Turn of the first delivery (none: not sent).
    pub sent_turn: Option<u32>,
    pub sent_wall_ms: Option<u128>,
    pub skipped: bool,
    pub recipients: Vec<String>,
    /// Wall milliseconds from the first delivery to the first response.
    pub first_response_ms: Option<u128>,
    pub overall: Activity,
    pub roles: BTreeMap<String, Activity>,
}

/// Where the scenario stands in simulated time.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioClock {
    /// Scenario start, seconds after midnight.
    pub start_clock_s: i64,
    pub now_s: i64,
    pub turn_start_s: i64,
    pub turn_end_s: i64,
    pub turn_minutes: u32,
    /// Turns the MSEL spans (or more, if play went beyond it).
    pub planned_turns: u32,
    pub planned_end_s: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoleInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub turn: u32,
    pub phase: Phase,
    pub ended: bool,
    /// Wall-clock start (turn 1 opened) and end of the exercise.
    pub started_wall_ms: Option<u128>,
    pub ended_wall_ms: Option<u128>,
    pub scenario: ScenarioClock,
    pub injects_total: u32,
    pub injects_sent: u32,
    pub injects_skipped: u32,
    /// MSEL injects whose planned time has passed.
    pub injects_due: u32,
    pub roles: Vec<RoleInfo>,
    pub turns: Vec<TurnStats>,
    pub injects: Vec<InjectStats>,
}

impl Exercise {
    /// The observer dashboard (see the module docs).
    pub fn observer_dashboard(&self) -> Dashboard {
        let roles: Vec<RoleInfo> = self
            .players
            .values()
            .map(|p| RoleInfo { id: p.role.clone(), name: p.name.clone() })
            .collect();
        let last = self.turn.max(1);
        let mut turns: BTreeMap<u32, Block> = (1..=last).map(|t| (t, Block::default())).collect();
        for b in turns.values_mut() {
            for r in &roles {
                b.role(&r.id);
            }
        }
        let mut injects: Vec<InjectStats> = Vec::new();
        // Delivery event -> row in `injects`.
        let mut row_of: BTreeMap<u64, usize> = BTreeMap::new();
        let scn = &self.world.scn;
        for t in &scn.msel {
            injects.push(InjectStats {
                id: t.id.clone(),
                title: t.title.clone(),
                sender: t.sender.clone(),
                msel: true,
                planned_turn: Some(t.turn),
                planned_s: Some(self.planned_due_s(&t.id, t.turn, t.at_s)),
                sent_turn: None,
                sent_wall_ms: None,
                skipped: self.skipped_injects.contains_key(&t.id),
                recipients: Vec::new(),
                first_response_ms: None,
                overall: Activity::default(),
                roles: BTreeMap::new(),
            });
        }
        injects.sort_by_key(|i| i.planned_s);
        let mut first_reply: BTreeMap<usize, u128> = BTreeMap::new();
        for m in &self.messages {
            match m.kind {
                MsgKind::Inject => {
                    let Some(delivery) = m.inject_id else { continue };
                    let row = match self.delivery_msel.get(&delivery) {
                        Some(id) => injects.iter().position(|i| &i.id == id),
                        None => None,
                    };
                    let row = row.unwrap_or_else(|| {
                        *row_of.entry(delivery).or_insert_with(|| {
                            injects.push(InjectStats {
                                id: format!("custom-{delivery}"),
                                title: m.title.clone().unwrap_or_default(),
                                sender: m.from.clone(),
                                msel: false,
                                planned_turn: None,
                                planned_s: None,
                                sent_turn: None,
                                sent_wall_ms: None,
                                skipped: false,
                                recipients: Vec::new(),
                                first_response_ms: None,
                                overall: Activity::default(),
                                roles: BTreeMap::new(),
                            });
                            injects.len() - 1
                        })
                    });
                    row_of.insert(delivery, row);
                    let i = &mut injects[row];
                    if i.sent_turn.is_none() {
                        i.sent_turn = Some(m.turn);
                        i.sent_wall_ms = Some(m.wall_ms);
                        let b = turns.entry(m.turn).or_default();
                        if i.msel {
                            b.injects_sent += 1;
                        } else {
                            b.custom_injects += 1;
                        }
                    }
                    if !i.recipients.contains(&m.to) {
                        i.recipients.push(m.to.clone());
                    }
                    i.roles.entry(m.to.clone()).or_default().received += 1;
                    turns.entry(m.turn).or_default().role(&m.to).received += 1;
                }
                // Answers to an EXCON question about a reply carry no proposal.
                MsgKind::Reply if m.from != MASTER && m.proposal.is_none() => {
                    turns.entry(m.turn).or_default().role(&m.from).answers += 1;
                    if let Some(&row) = m.inject_id.and_then(|d| row_of.get(&d)) {
                        injects[row].roles.entry(m.from.clone()).or_default().answers += 1;
                    }
                }
                MsgKind::Reply if m.from != MASTER => {
                    if let Some(&row) = m.inject_id.and_then(|d| row_of.get(&d)) {
                        first_reply.entry(row).or_insert(m.wall_ms);
                    }
                }
                _ => {}
            }
        }
        for p in &self.proposals {
            let Source::Player(role) = &p.source else { continue };
            // Responses to players are coordination: their conversation message counts.
            if p.inject_id.is_none() || !p.addressed_to.players.is_empty() {
                continue;
            }
            let basis = p.evidence.as_ref().map(|e| e.basis.kind);
            let a = turns.entry(p.turn).or_default().role(role);
            a.responses += 1;
            a.basis.add(basis);
            if let Some(&row) = p.inject_id.and_then(|d| row_of.get(&d)) {
                let a = injects[row].roles.entry(role.clone()).or_default();
                a.responses += 1;
                a.basis.add(basis);
            }
        }
        for c in &self.linked_conversations {
            let row = row_of.get(&c.inject_id).copied();
            turns.entry(c.turn).or_default().role(&c.created_by).conversations += 1;
            if let Some(row) = row {
                injects[row].roles.entry(c.created_by.clone()).or_default().conversations += 1;
            }
            for m in &c.messages {
                turns.entry(m.turn).or_default().role(&m.from).interactions += 1;
                if let Some(row) = row {
                    injects[row].roles.entry(m.from.clone()).or_default().interactions += 1;
                }
            }
        }
        for item in &self.review.items {
            let executed = item.state == ItemState::Executed;
            let a = turns.entry(item.turn).or_default().role(&item.source_role);
            a.suggested += 1;
            a.executed += executed as u32;
            let delivery = self.proposal(item.proposal).and_then(|p| p.inject_id);
            if let Some(&row) = delivery.and_then(|d| row_of.get(&d)) {
                let a = injects[row].roles.entry(item.source_role.clone()).or_default();
                a.suggested += 1;
                a.executed += executed as u32;
            }
        }
        for (row, at) in first_reply {
            let i = &mut injects[row];
            i.first_response_ms = i.sent_wall_ms.map(|s| at.saturating_sub(s));
        }
        for i in &mut injects {
            let mut overall = Activity::default();
            for a in i.roles.values() {
                overall += a;
            }
            i.overall = overall;
        }

        // Turn windows: turn 1 opens at the start, each resolution closes one.
        let mut opened: BTreeMap<u32, (i64, u128)> = BTreeMap::new();
        let mut closed: BTreeMap<u32, (i64, u128)> = BTreeMap::new();
        let mut ended_wall_ms = None;
        for e in &self.log {
            match e.kind.as_str() {
                "turn_opened" => {
                    opened.insert(e.data["turn"].as_u64().unwrap_or(1) as u32, (e.t_s, e.wall_ms));
                }
                "turn_resolved" => {
                    let t = e.data["turn"].as_u64().unwrap_or(e.turn as u64) as u32;
                    closed.insert(t, (e.t_s, e.wall_ms));
                    opened.insert(t + 1, (e.t_s, e.wall_ms));
                }
                "exercise_ended" => ended_wall_ms = Some(e.wall_ms),
                _ => {}
            }
        }
        // A rollback forgets later turns.
        closed.retain(|t, _| *t < self.turn);
        let ended = self.phase == Phase::Debrief;
        let now_s = self.world.time_s();
        let mut sum = Block::default();
        let turn_stats: Vec<TurnStats> = turns
            .into_iter()
            .filter(|(t, _)| *t >= 1 && *t <= last)
            .map(|(turn, mut b)| {
                b.sum();
                for (role, a) in &b.roles {
                    *sum.roles.entry(role.clone()).or_default() += a;
                }
                sum.injects_sent += b.injects_sent;
                sum.custom_injects += b.custom_injects;
                sum.sum();
                let start_s = opened.get(&turn).map(|o| o.0).unwrap_or(if turn == self.turn { self.turn_start_s } else { 0 });
                TurnStats {
                    turn,
                    // The end of the exercise freezes the open turn too.
                    closed: ended || closed.contains_key(&turn),
                    start_s,
                    end_s: closed.get(&turn).map(|c| c.0).or(ended.then_some(now_s)),
                    opened_wall_ms: opened.get(&turn).map(|o| o.1),
                    closed_wall_ms: closed.get(&turn).map(|c| c.1).or(ended_wall_ms.filter(|_| ended)),
                    turn_only: b,
                    cumulative: sum.clone(),
                }
            })
            .collect();

        let turn_s = scn.turn_minutes.max(1) as i64 * 60;
        let msel_turns = injects
            .iter()
            .filter(|i| i.msel)
            .map(|i| i.planned_turn.unwrap_or(1).max((i.planned_s.unwrap_or(0) / turn_s + 1) as u32))
            .max()
            .unwrap_or(1);
        let planned_turns = msel_turns.max(self.turn);
        let started_wall_ms = (self.phase != Phase::Briefing).then(|| opened.get(&1).map(|o| o.1)).flatten();
        let msel: Vec<&InjectStats> = injects.iter().filter(|i| i.msel).collect();
        Dashboard {
            turn: self.turn,
            phase: self.phase,
            ended,
            started_wall_ms,
            ended_wall_ms: ended_wall_ms.filter(|_| ended),
            scenario: ScenarioClock {
                start_clock_s: scn.start_clock_s,
                now_s,
                turn_start_s: self.turn_start_s,
                turn_end_s: self.planned_end_s(),
                turn_minutes: self.turn_minutes,
                planned_turns,
                planned_end_s: (planned_turns as i64 * turn_s).max(self.planned_end_s()),
            },
            injects_total: msel.len() as u32,
            injects_sent: msel.iter().filter(|i| i.sent_turn.is_some()).count() as u32,
            injects_skipped: msel.iter().filter(|i| i.skipped && i.sent_turn.is_none()).count() as u32,
            injects_due: msel
                .iter()
                .filter(|i| self.phase != Phase::Briefing && i.planned_s.is_some_and(|s| s <= now_s))
                .count() as u32,
            roles,
            turns: turn_stats,
            injects,
        }
    }
}
