//! Inject actions: world changes that belong to an inject. Post-actions
//! (`offset_s >= 0`) run because the inject is sent: at once as EXCON commands
//! for offset 0, otherwise recorded as approved and applied `offset_s` after the
//! send, during the following resolution. Pre-actions (`offset_s < 0`) run by
//! themselves `-offset_s` before the inject's planned time while time is
//! scanned, unless EXCON disabled them or skipped the inject; an inject sent
//! early runs its pending pre-actions at send time. Each run is one
//! [`ActionRun`], so sending the same inject to more roles never runs its
//! actions twice.
use crate::{Exercise, MASTER};
use anyhow::Result;
use crate::knowledge::{KnowledgeSource, MapObservation};
use scenario::{InjectAction, RevealSpec};
use serde::Serialize;
use serde_json::{json, Value};
use sim::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    /// Approved with a delay, waiting for its instant.
    Approved,
    Done,
    Failed,
    /// Left unchecked when the inject was sent.
    NotRun,
    /// A queued EXCON action withdrawn before its instant.
    Cancelled,
}

/// Which actions run when an inject is sent.
#[derive(Debug, Clone, Default)]
pub struct SendActions {
    /// Ids of the MSEL inject's own actions to run; `None` runs all of them
    /// (disabled pre-actions stay off).
    pub run: Option<Vec<String>>,
    /// Actions of a custom inject (all of them run).
    pub custom: Vec<InjectAction>,
}

impl SendActions {
    /// Run every action of the MSEL inject.
    pub fn all() -> Self {
        Self::default()
    }

    /// Run none of them.
    pub fn none() -> Self {
        Self { run: Some(Vec::new()), custom: Vec::new() }
    }
}

/// The decision taken on one action of one delivery and what came of it.
/// Dropped by a rollback to a time before the decision.
#[derive(Debug, Clone, Serialize)]
pub struct ActionRun {
    /// Delivery (send) the action ran for.
    pub delivery: u64,
    pub action: String,
    /// MSEL inject it belongs to; `None` for a custom inject.
    pub inject: Option<String>,
    pub title: String,
    pub command: Value,
    /// Map knowledge disclosed instead of a world change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<RevealSpec>,
    /// Sender of the inject and roles it went to (what a reveal needs).
    pub sender: String,
    pub roles: Vec<String>,
    pub offset_s: i64,
    pub status: ActionStatus,
    /// Simulated time when the inject was sent.
    pub decided_s: i64,
    /// Instant at which an approved action is applied.
    pub run_at_s: Option<i64>,
    pub executed_s: Option<i64>,
    pub result: Option<String>,
    /// Set for an EXCON action queued on its own (not part of an inject).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<crate::queue::Queued>,
}

fn run_key(delivery: u64, action: &str) -> String {
    format!("{delivery}:{action}")
}

fn disabled_key(inject: &str, action: &str) -> String {
    format!("{inject}:{action}")
}

impl Exercise {
    /// Record, and for delay 0 execute, the actions of an inject just sent.
    /// MSEL actions are decided on the first send only.
    pub(crate) fn register_inject_actions(
        &mut self,
        delivery: u64,
        inject_id: Option<&str>,
        selection: &SendActions,
        first: bool,
        sender: &str,
        roles: &[String],
    ) {
        let mut items: Vec<(InjectAction, bool)> = Vec::new();
        match inject_id {
            Some(id) => {
                if !first {
                    return;
                }
                if let Some(t) = self.world.scn.msel.iter().find(|t| t.id == id).cloned() {
                    for a in t.actions {
                        // Already ran by itself during the scan.
                        if self.action_has_run(id, &a.id) {
                            continue;
                        }
                        let off = a.offset_s < 0
                            && self.disabled_actions.contains_key(&disabled_key(id, &a.id));
                        let run = !off && selection.run.as_ref().is_none_or(|ids| ids.contains(&a.id));
                        items.push((a, run));
                    }
                }
            }
            None => items.extend(selection.custom.iter().cloned().map(|a| (a, true))),
        }
        let now = self.world.time_s();
        for (a, run) in items {
            let key = run_key(delivery, &a.id);
            // Pre-actions of an inject sent early run now.
            let delay = a.offset_s.max(0);
            let (status, run_at_s) = match run {
                false => (ActionStatus::NotRun, None),
                true => (ActionStatus::Approved, Some(now + delay)),
            };
            self.action_runs.insert(
                key.clone(),
                ActionRun {
                    delivery,
                    action: a.id.clone(),
                    inject: inject_id.map(String::from),
                    title: a.title.clone(),
                    command: a.command.clone(),
                    reveal: a.reveal.clone(),
                    sender: sender.to_string(),
                    roles: roles.to_vec(),
                    offset_s: a.offset_s,
                    status,
                    decided_s: now,
                    run_at_s,
                    executed_s: None,
                    result: None,
                    queued: None,
                },
            );
            self.record(
                "timeline_action_decided",
                MASTER,
                json!({"action": a.id, "delivery": delivery, "inject": inject_id,
                    "choice": if run { "run" } else { "not_run" }, "run_at_s": run_at_s}),
            );
        }
        // Immediate actions apply now, before the send returns: they are what
        // the delivered message describes (e.g. the fire it reports, which its
        // reveal then estimates). Standalone EXCON actions wait for play
        // instead ([`crate::queue`]).
        self.run_approved_actions();
    }

    /// Every action run with its current state, chronological by decision.
    pub fn inject_action_runs(&self) -> Vec<Value> {
        let start = self.world.scn.start_clock_s;
        let mut rows: Vec<&ActionRun> = self.action_runs.values().filter(|r| r.queued.is_none()).collect();
        rows.sort_by_key(|r| (r.decided_s, r.delivery));
        rows.into_iter()
            .map(|r| {
                json!({
                    "id": r.action, "delivery": r.delivery, "inject": r.inject, "title": r.title,
                    "command": r.command, "reveal": r.reveal, "summary": Self::describe_run(&r.command, r.reveal.as_ref()),
                    "offset_s": r.offset_s, "status": r.status,
                    "run_at_s": r.run_at_s, "run_at_clock": r.run_at_s.map(|t| scenario::format_clock(start + t)),
                    "executed_s": r.executed_s, "result": r.result,
                })
            })
            .collect()
    }

    /// Readable summary of an action: its command or its reveal.
    pub(crate) fn describe_run(command: &Value, reveal: Option<&RevealSpec>) -> String {
        match reveal {
            Some(r) => Self::describe_reveal(r),
            None => Self::describe_command(command),
        }
    }

    fn describe_reveal(r: &RevealSpec) -> String {
        use scenario::{RevealZones, ZoneRule};
        match r {
            RevealSpec::Exact { zones, layers, .. } => {
                let z = match zones {
                    RevealZones::Rule(ZoneRule::Burning) => "burning zones".to_string(),
                    RevealZones::Rule(ZoneRule::BurningAndAdjacent) => "burning zones and neighbours".to_string(),
                    RevealZones::List(l) => l.join(", "),
                };
                format!("Reveal on the map (exact): {z}, layers {}", layers.join(", "))
            }
            RevealSpec::Approximate { position_error_m, area_error_frac, .. } => format!(
                "Reveal on the map (approximate): unverified fire estimate, position error up to {position_error_m:.0} m, area ±{:.0}%",
                area_error_frac * 100.0
            ),
        }
    }

    fn describe_command(command: &Value) -> String {
        serde_json::from_value::<Command>(command.clone())
            .map(|c| c.describe())
            .unwrap_or_else(|e| format!("invalid command: {e}"))
    }

    /// Whether the action of this MSEL inject already has a run (or a recorded
    /// decision not to run).
    fn action_has_run(&self, inject: &str, action: &str) -> bool {
        self.action_runs
            .values()
            .any(|r| r.inject.as_deref() == Some(inject) && r.action == action)
    }

    /// Pre-actions still waiting for their instant: `(instant, inject, action)`,
    /// chronological. Injects already sent or skipped, disabled actions and
    /// actions already run are left out.
    pub(crate) fn pending_pre_actions(&self) -> Vec<(i64, String, InjectAction)> {
        let mut out = Vec::new();
        for t in &self.world.scn.msel {
            let sent = self.inject_sent_to.get(&t.id).is_some_and(|v| !v.is_empty());
            if sent || self.skipped_injects.contains_key(&t.id) {
                continue;
            }
            let planned = self.planned_due_s(&t.id, t.turn, t.at_s);
            for a in t.actions.iter().filter(|a| a.offset_s < 0) {
                if self.disabled_actions.contains_key(&disabled_key(&t.id, &a.id))
                    || self.action_has_run(&t.id, &a.id)
                {
                    continue;
                }
                out.push((planned + a.offset_s, t.id.clone(), a.clone()));
            }
        }
        out.sort_by(|a, b| (a.0, &a.1, &a.2.id).cmp(&(b.0, &b.1, &b.2.id)));
        out
    }

    /// Enable or disable a pre-action of an inject not yet sent. A disabled
    /// pre-action does not run during the scan.
    pub fn set_action_enabled(&mut self, inject: &str, action: &str, enabled: bool) -> Result<()> {
        let Some(t) = self.world.scn.msel.iter().find(|t| t.id == inject) else {
            anyhow::bail!("unknown inject {inject}");
        };
        let Some(a) = t.actions.iter().find(|a| a.id == action) else {
            anyhow::bail!("unknown action {action} of inject {inject}");
        };
        if a.offset_s >= 0 {
            anyhow::bail!("only pre-actions (before the inject) can be switched off in advance");
        }
        if self.inject_sent_to.get(inject).is_some_and(|v| !v.is_empty())
            || self.action_has_run(inject, action)
        {
            anyhow::bail!("the action has already been decided");
        }
        let key = disabled_key(inject, action);
        if enabled {
            self.disabled_actions.remove(&key);
        } else {
            self.disabled_actions.insert(key, self.world.time_s());
        }
        self.record(
            "timeline_action_toggled",
            MASTER,
            json!({"action": action, "inject": inject, "enabled": enabled}),
        );
        Ok(())
    }

    /// The actions of one MSEL inject for the EXCON views: every action of the
    /// template with the state of the run recorded for it.
    pub(crate) fn msel_actions_view(&self, inject: &scenario::InjectTemplate) -> Vec<Value> {
        let start = self.world.scn.start_clock_s;
        let planned = self.planned_due_s(&inject.id, inject.turn, inject.at_s);
        inject
            .actions
            .iter()
            .map(|a| {
                let run = self
                    .action_runs
                    .values()
                    .filter(|r| r.inject.as_deref() == Some(inject.id.as_str()) && r.action == a.id)
                    .max_by_key(|r| r.decided_s);
                let pre = a.offset_s < 0;
                let enabled = !self.disabled_actions.contains_key(&disabled_key(&inject.id, &a.id));
                let instant = pre.then(|| planned + a.offset_s);
                let status = match run {
                    Some(r) => Some(r.status),
                    None if pre && !enabled => Some(ActionStatus::NotRun),
                    None => None,
                };
                json!({
                    "id": a.id, "title": a.title, "note": a.note, "command": a.command, "reveal": a.reveal,
                    "summary": Self::describe_run(&a.command, a.reveal.as_ref()), "offset_s": a.offset_s,
                    "pre": pre, "enabled": enabled,
                    "instant_s": instant,
                    "instant_clock": instant.map(|t| scenario::format_clock(start + t)),
                    "status": status,
                    "run_at_s": run.and_then(|r| r.run_at_s),
                    "run_at_clock": run.and_then(|r| r.run_at_s).map(|t| scenario::format_clock(start + t)),
                    "executed_s": run.and_then(|r| r.executed_s),
                    "result": run.and_then(|r| r.result.clone()),
                })
            })
            .collect()
    }

    /// Actions of a custom inject delivery (see [`Self::inject_action_runs`]).
    pub fn delivery_actions(&self, delivery: u64) -> Vec<Value> {
        self.inject_action_runs()
            .into_iter()
            .filter(|r| r["delivery"] == delivery)
            .collect()
    }

    /// Apply approved actions whose instant has been reached: inject actions
    /// and queued EXCON actions ([`crate::queue`]).
    pub(crate) fn run_approved_actions(&mut self) {
        let now = self.world.time_s();
        // Pre-actions whose instant has come become approved runs.
        for (instant, inject, a) in self.pending_pre_actions() {
            if instant > now {
                break;
            }
            self.action_runs.insert(
                format!("pre:{inject}:{}", a.id),
                ActionRun {
                    delivery: 0,
                    action: a.id.clone(),
                    inject: Some(inject.clone()),
                    title: a.title.clone(),
                    command: a.command.clone(),
                    reveal: a.reveal.clone(),
                    sender: String::new(),
                    roles: Vec::new(),
                    offset_s: a.offset_s,
                    status: ActionStatus::Approved,
                    decided_s: now,
                    run_at_s: Some(instant),
                    executed_s: None,
                    result: None,
                    queued: None,
                },
            );
            self.record(
                "timeline_action_decided",
                MASTER,
                json!({"action": a.id, "delivery": 0, "inject": inject, "choice": "run", "run_at_s": instant}),
            );
        }
        let mut due: Vec<(i64, String)> = self
            .action_runs
            .iter()
            .filter(|(_, r)| {
                r.status == ActionStatus::Approved && r.run_at_s.is_some_and(|t| t <= now)
            })
            .map(|(k, r)| (r.run_at_s.unwrap_or(now), k.clone()))
            .collect();
        due.sort();
        for (_, key) in due {
            let Some(run) = self.action_runs.get(&key).cloned() else { continue };
            let outcome: Result<Vec<String>> = match &run.reveal {
                Some(spec) => self.execute_reveal(&run, spec),
                None => serde_json::from_value::<Command>(run.command.clone())
                    .map_err(|e| anyhow::anyhow!("invalid command: {e}"))
                    .and_then(|cmd| match &run.queued {
                        // A reviewed reply runs as its role, with its checks.
                        Some(q) if q.origin["origin"] == "excon_review" => {
                            let item = q.origin["review_item"].as_u64().unwrap_or_default();
                            self.apply_review_item(item).and_then(|rec| {
                                if rec.ok {
                                    Ok(vec![rec.detail])
                                } else {
                                    Err(anyhow::anyhow!(rec.detail))
                                }
                            })
                        }
                        Some(q) => {
                            let mut origin = json!({"origin": "excon_queue", "queue_id": q.id});
                            if let (Some(m), Some(extra)) = (origin.as_object_mut(), q.origin.as_object()) {
                                m.insert("queued_origin".into(), Value::Object(extra.clone()));
                            }
                            self.direct_command_logged(&cmd, origin).map(|(notes, _)| notes)
                        }
                        None => self.direct_command(&cmd),
                    }),
            };
            let (status, result) = match outcome {
                Ok(notes) => (ActionStatus::Done, notes.join("; ")),
                Err(e) => (ActionStatus::Failed, e.to_string()),
            };
            if let Some(r) = self.action_runs.get_mut(&key) {
                r.status = status;
                r.executed_s = Some(now);
                r.result = Some(result.clone());
            }
            self.record(
                "timeline_action_executed",
                MASTER,
                json!({"action": run.action, "delivery": run.delivery, "inject": run.inject,
                    "status": status, "result": result, "t_s": now}),
            );
        }
    }

    /// Disclose map knowledge to the recipients of the inject. Offset 0 rides
    /// the delivered messages; later, each recipient gets a Situation message.
    /// Fog of war is not world state: nothing here goes through `World::apply`.
    fn execute_reveal(&mut self, run: &ActionRun, spec: &RevealSpec) -> Result<Vec<String>> {
        let source = spec.source().map(String::from).unwrap_or_else(|| run.sender.clone());
        let inject = run.inject.clone().unwrap_or_else(|| format!("delivery-{}", run.delivery));
        let exact = match spec {
            RevealSpec::Exact { zones, layers, .. } => {
                let zones = self.reveal_zones(zones)?;
                Some((self.zone_observation(&zones, layers)?, layers.clone()))
            }
            RevealSpec::Approximate { .. } => None,
        };
        let (mut notes, mut errors) = (Vec::new(), Vec::new());
        for role in &run.roles {
            let (observation, layers) = match (&exact, spec) {
                (Some((o, l)), _) => (o.clone(), l.clone()),
                (None, RevealSpec::Approximate { position_error_m, area_error_frac, .. }) => {
                    let key = format!("{inject}:{}:{role}", run.action);
                    let estimate = self.fire_estimate(&key, *position_error_m, *area_error_frac)?;
                    (MapObservation { estimates: vec![estimate], ..Default::default() }, Vec::new())
                }
                (None, _) => unreachable!("exact reveals always carry an observation"),
            };
            let delivered = (run.offset_s == 0)
                .then(|| self.messages.iter().find(|m| m.inject_id == Some(run.delivery) && &m.to == role).map(|m| m.id))
                .flatten();
            let done = match delivered {
                Some(mid) => self
                    .attach_map_observation_named(mid, observation, KnowledgeSource::Delivered, Some(source.clone()), layers)
                    .map(|_| mid),
                None => self.acquire_map_layers(role, &source, observation, None, layers),
            };
            match done {
                Ok(mid) => notes.push(format!("{role}: message {mid}")),
                Err(e) => errors.push(format!("{role}: {e}")),
            }
        }
        if notes.is_empty() {
            anyhow::bail!("{}", if errors.is_empty() { "no recipient".to_string() } else { errors.join("; ") });
        }
        notes.extend(errors);
        Ok(notes)
    }

    /// Seconds until the next approved action, so a step can stop exactly there.
    pub(crate) fn next_action_in(&self) -> Option<i64> {
        let now = self.world.time_s();
        self.action_runs
            .values()
            .filter(|r| r.status == ActionStatus::Approved)
            .filter_map(|r| r.run_at_s)
            .chain(self.pending_pre_actions().into_iter().map(|p| p.0))
            .filter(|t| *t > now)
            .map(|t| t - now)
            .min()
    }

    /// A rollback forgets the decisions taken from the restored time onwards,
    /// and the injects EXCON chose not to send after it. Actions decided
    /// earlier that ran or were cancelled from that time onwards wait again:
    /// the restored world is from before their execution (the turn snapshot
    /// is taken before anything runs at its instant).
    pub(crate) fn forget_actions_since_restore(&mut self) {
        let t = self.world.time_s();
        self.action_runs.retain(|_, r| r.decided_s < t);
        for r in self.action_runs.values_mut() {
            if r.executed_s.is_some_and(|e| e >= t) {
                r.status = ActionStatus::Approved;
                r.executed_s = None;
                r.result = None;
            }
            if let Some(q) = r.queued.as_mut() {
                if q.cancelled_s.is_some_and(|c| c >= t) {
                    q.cancelled_s = None;
                    q.cancel_reason = None;
                    r.status = ActionStatus::Approved;
                }
            }
        }
        self.skipped_injects.retain(|_, s| *s < t);
        self.disabled_actions.retain(|_, s| *s < t);
        self.suggestion_marks.retain(|_, m| m.at_s < t);
    }
}
