//! The turn-based exercise engine.
//!
//! Owns the [`World`] and everything around it that is not simulation:
//! turns and phases, injects, per-role inboxes, proposals and the Master's
//! decisions, per-turn snapshots for rollback, and the event log.
//!
//! It is synchronous and knows nothing about LLMs or HTTP: the server runs
//! the LLM workflows and feeds their results in ([`Exercise::attach_parse`],
//! [`Exercise::add_message`]). That keeps the whole turn loop testable
//! headless and makes "LLM down" a normal path — the Master can type
//! commands for any proposal.
//!
//! ## Turn loop
//!
//! `Briefing` → [`Exercise::start`] → `Orders` (injects go out, players reply,
//! proposals are parsed and decided) → [`Exercise::resolve_turn`] (approved
//! player actions applied, simulated time advanced, snapshot) → `Orders` of
//! the next turn … → [`Exercise::end`] → `Debrief`.
//!
//! Master actions (direct commands, approved Master prompts) apply at once;
//! approved player actions apply together at resolution.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sim::{Actor, Command, SimEvent, Verdict, World, WorldSnapshot};

pub const DEFAULT_TURN_MINUTES: u32 = 30;
/// Conversation turns kept per role (oldest dropped first).
pub const CONVERSATION_MAX: usize = 60;
/// Resolution granularity: the world is published after every simulated second.
pub const STEP_S: i64 = 1;
pub const MASTER: &str = "master";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Briefing,
    Orders,
    /// A turn is being simulated, minute by minute (see [`Exercise::begin_resolution`]).
    Resolving,
    Debrief,
}

#[derive(Debug, Clone, Serialize)]
pub struct Player {
    pub role: String,
    pub name: String,
    pub language: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MsgKind {
    Inject,
    Reply,
    Feedback,
    Clarification,
    Situation,
    RoleMessage,
    System,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: u64,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
    pub from: String,
    /// Role id, or `"master"`.
    pub to: String,
    pub kind: MsgKind,
    pub title: Option<String>,
    /// In the recipient's language.
    pub text: String,
    /// In English (working language).
    pub text_en: String,
    pub proposal: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Waiting for the LLM.
    Parsing,
    /// Waiting for the Master.
    Pending,
    /// The LLM asked the player a question; the Master may still act on it.
    NeedsClarification,
    /// Approved; applies at turn resolution.
    Approved,
    Rejected,
    Applied,
    /// Approved, but could not be executed at resolution.
    Failed,
    /// Undone by a rollback.
    RolledBack,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "role", rename_all = "snake_case")]
pub enum Source {
    Player(String),
    Master,
}

impl Source {
    pub fn actor(&self) -> Actor {
        match self {
            Source::Player(r) => Actor::Role(r.clone()),
            Source::Master => Actor::Master,
        }
    }
    pub fn label(&self) -> &str {
        match self {
            Source::Player(r) => r,
            Source::Master => MASTER,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub id: u64,
    pub turn: u32,
    pub t_s: i64,
    pub source: Source,
    pub original: String,
    pub language: String,
    pub translation_en: String,
    pub summary: String,
    pub commands: Vec<Command>,
    pub descriptions: Vec<String>,
    pub verdicts: Vec<Verdict>,
    /// Actions the LLM produced that are not valid commands.
    pub invalid: Vec<String>,
    pub clarification: Option<String>,
    pub status: ProposalStatus,
    pub master_note: Option<String>,
    pub results: Vec<String>,
    /// Injects drafted by the LLM for a Master prompt; sent when approved.
    pub inject_drafts: Vec<Value>,
}

/// What the LLM made of a reply, in exercise terms.
#[derive(Debug, Clone, Default)]
pub struct ParsedInput {
    pub language: String,
    pub translation_en: String,
    pub actions: Vec<Value>,
    pub clarification: Option<String>,
    pub summary: String,
    pub inject_drafts: Vec<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Decision {
    pub approve: bool,
    /// Replaces the proposal's commands (the Master's edit).
    pub commands: Option<Vec<Command>>,
    pub note: Option<String>,
}

/// Facts about a decision, for the feedback message to the player.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionOutcome {
    pub proposal: u64,
    /// Player to inform (`None` for Master proposals).
    pub notify: Option<String>,
    pub facts_en: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppliedAction {
    pub proposal: u64,
    pub by: String,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnReport {
    pub turn: u32,
    pub from_s: i64,
    pub to_s: i64,
    pub applied: Vec<AppliedAction>,
    pub events: Vec<SimEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub seq: u64,
    pub wall_ms: u128,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
    pub kind: String,
    pub actor: String,
    pub data: Value,
}

/// A turn being resolved: where it started, where it stops, what happened so far.
#[derive(Debug, Clone, Serialize)]
pub struct Resolution {
    pub turn: u32,
    pub minutes: u32,
    pub from_s: i64,
    pub to_s: i64,
    pub applied: Vec<AppliedAction>,
    pub events: Vec<SimEvent>,
}

/// One entry of a role's conversation with the AI ("user" = what the player
/// received or wrote, "assistant" = the AI's interpretation).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TurnWindow {
    pub turn: u32,
    pub from_s: i64,
    pub to_s: i64,
}

pub struct Exercise {
    pub world: World,
    pub turn: u32,
    pub phase: Phase,
    pub turn_minutes: u32,
    pub players: BTreeMap<String, Player>,
    pub messages: Vec<Message>,
    pub proposals: Vec<Proposal>,
    pub log: Vec<LogEntry>,
    /// Bumped on every change; UIs and the renderer poll it.
    pub generation: u64,
    /// The last resolved turn's time window (the renderer replays it).
    pub last_turn: Option<TurnWindow>,
    /// Injects of the MSEL already sent.
    pub sent_injects: Vec<String>,
    /// The turn currently being simulated, if any.
    pub resolving: Option<Resolution>,
    /// Each role's ongoing conversation with the AI (key: role id, or `"master"`):
    /// everything the player received, what they wrote, and how it was read.
    pub conversations: BTreeMap<String, Vec<ChatTurn>>,
    /// Simulated seconds per wall-clock second while resolving (60 = one
    /// simulated minute per second); 0 = as fast as possible.
    pub speed: f32,
    snapshots: Vec<(u32, WorldSnapshot)>,
    log_file: Option<PathBuf>,
    next_id: u64,
}

impl Exercise {
    pub fn new(world: World) -> Exercise {
        let players = world
            .scn
            .roles
            .iter()
            .map(|r| (r.id.clone(), Player { role: r.id.clone(), name: r.name.clone(), language: r.language.clone() }))
            .collect();
        let mut ex = Exercise {
            world,
            turn: 0,
            phase: Phase::Briefing,
            turn_minutes: DEFAULT_TURN_MINUTES,
            players,
            messages: Vec::new(),
            proposals: Vec::new(),
            log: Vec::new(),
            generation: 1,
            last_turn: None,
            sent_injects: Vec::new(),
            resolving: None,
            conversations: BTreeMap::new(),
            speed: 0.0,
            snapshots: Vec::new(),
            log_file: None,
            next_id: 1,
        };
        ex.record("exercise_created", MASTER, json!({"scenario": ex.world.scn.id, "seed": ex.world.seed}));
        ex
    }

    /// Also append every log entry as a JSON line to `path`.
    pub fn with_log_file(mut self, path: PathBuf) -> Exercise {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        self.log_file = Some(path);
        let existing = std::mem::take(&mut self.log);
        for e in existing {
            self.write_log(&e);
            self.log.push(e);
        }
        self
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn touch(&mut self) {
        self.generation += 1;
    }

    pub fn record(&mut self, kind: &str, actor: &str, data: Value) {
        let e = LogEntry {
            seq: self.log.len() as u64 + 1,
            wall_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
            turn: self.turn,
            t_s: self.world.time_s(),
            clock: self.world.clock(),
            kind: kind.into(),
            actor: actor.into(),
            data,
        };
        self.write_log(&e);
        self.log.push(e);
        self.touch();
    }

    fn write_log(&self, e: &LogEntry) {
        if let Some(p) = &self.log_file {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
            }
        }
    }

    pub fn role_ids(&self) -> Vec<String> {
        self.players.keys().cloned().collect()
    }

    pub fn language_of(&self, role: &str) -> String {
        self.players.get(role).map(|p| p.language.clone()).unwrap_or_else(|| "en".into())
    }

    pub fn set_language(&mut self, role: &str, lang: &str) -> Result<()> {
        let Some(p) = self.players.get_mut(role) else { bail!("unknown role {role}") };
        p.language = lang.to_string();
        self.record("language_set", role, json!({"language": lang}));
        Ok(())
    }

    /// Expand `"all"` and check role ids.
    pub fn recipients(&self, list: &[String]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for r in list {
            if r == "all" {
                return Ok(self.role_ids());
            }
            if !self.players.contains_key(r) {
                bail!("unknown recipient {r}");
            }
            if !out.contains(r) {
                out.push(r.clone());
            }
        }
        if out.is_empty() {
            bail!("no recipients");
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ turns

    pub fn start(&mut self) -> Result<()> {
        if self.phase != Phase::Briefing {
            bail!("exercise already started");
        }
        self.turn = 1;
        self.phase = Phase::Orders;
        self.world.new_turn();
        self.snapshots.push((1, self.world.snapshot()));
        self.record("turn_opened", MASTER, json!({"turn": 1}));
        Ok(())
    }

    /// Resolve a whole turn at once: [`Self::begin_resolution`], one-second
    /// [`Self::step_resolution`]s, [`Self::finish_resolution`]. The server runs
    /// the same three steps with the lock released between steps, so the
    /// renderer and the browsers see the turn unfold.
    pub fn resolve_turn(&mut self, minutes: Option<u32>) -> Result<TurnReport> {
        self.begin_resolution(minutes)?;
        while !self.step_resolution(STEP_S)? {}
        self.finish_resolution()
    }

    /// Start resolving the open turn: approved player actions are applied now,
    /// then the phase is `Resolving` until simulated time reaches the end.
    pub fn begin_resolution(&mut self, minutes: Option<u32>) -> Result<()> {
        if self.phase != Phase::Orders {
            bail!("no turn is open");
        }
        let minutes = minutes.unwrap_or(self.turn_minutes).clamp(1, 24 * 60);
        let from_s = self.world.time_s();
        let mut applied = Vec::new();
        for i in 0..self.proposals.len() {
            if self.proposals[i].status != ProposalStatus::Approved {
                continue;
            }
            let actor = self.proposals[i].source.actor();
            let by = self.proposals[i].source.label().to_string();
            let mut all_ok = true;
            let mut results = Vec::new();
            for cmd in self.proposals[i].commands.clone() {
                let r = self.world.apply(&cmd, &actor);
                let (ok, detail) = match r {
                    Ok(notes) => (true, notes.join("; ")),
                    Err(reason) => (false, reason),
                };
                all_ok &= ok;
                results.push(format!("{}: {}", cmd.describe(), if ok { "done" } else { "not executed" }));
                applied.push(AppliedAction { proposal: self.proposals[i].id, by: by.clone(), action: cmd.describe(), ok, detail });
            }
            let p = &mut self.proposals[i];
            p.status = if all_ok { ProposalStatus::Applied } else { ProposalStatus::Failed };
            p.results = results;
        }
        let to_s = from_s + minutes as i64 * 60;
        self.resolving = Some(Resolution { turn: self.turn, minutes, from_s, to_s, applied, events: Vec::new() });
        self.phase = Phase::Resolving;
        self.record("turn_resolving", MASTER, json!({"turn": self.turn, "minutes": minutes, "to_s": to_s}));
        Ok(())
    }

    /// Advance the resolving turn by up to `step_s` simulated seconds.
    /// Returns `true` once the end of the turn has been reached.
    pub fn step_resolution(&mut self, step_s: i64) -> Result<bool> {
        let Some(to_s) = self.resolving.as_ref().map(|r| r.to_s) else { bail!("no turn is being resolved") };
        let dt = (to_s - self.world.time_s()).min(step_s);
        if dt > 0 {
            let events = self.world.advance(dt)?;
            if let Some(r) = self.resolving.as_mut() {
                r.events.extend(events);
            }
            self.touch();
        }
        Ok(self.world.time_s() >= to_s)
    }

    /// Close the resolved turn: log it, snapshot, open the next turn.
    pub fn finish_resolution(&mut self) -> Result<TurnReport> {
        let Some(r) = self.resolving.take() else { bail!("no turn is being resolved") };
        while self.world.time_s() < r.to_s {
            // Finishing early (e.g. a caller that did not step to the end) still completes the turn.
            let dt = (r.to_s - self.world.time_s()).min(STEP_S);
            self.world.advance(dt)?;
        }
        let report = TurnReport { turn: r.turn, from_s: r.from_s, to_s: r.to_s, applied: r.applied, events: r.events };
        self.record("turn_resolved", MASTER, json!({"turn": r.turn, "minutes": r.minutes, "applied": report.applied, "events": report.events, "stats": self.world.stats()}));
        self.last_turn = Some(TurnWindow { turn: r.turn, from_s: r.from_s, to_s: r.to_s });
        self.turn += 1;
        self.phase = Phase::Orders;
        self.world.new_turn();
        self.snapshots.push((self.turn, self.world.snapshot()));
        self.record("turn_opened", MASTER, json!({"turn": self.turn}));
        Ok(report)
    }

    /// Put the world back to the start of `turn`. Proposals of that turn and
    /// later are marked rolled back; messages stay as history.
    pub fn rollback(&mut self, turn: u32) -> Result<()> {
        if self.phase == Phase::Resolving {
            bail!("a turn is being resolved; wait for it to finish");
        }
        let Some((_, snap)) = self.snapshots.iter().find(|(t, _)| *t == turn).cloned() else {
            bail!("no snapshot for turn {turn}");
        };
        self.world.restore(&snap)?;
        self.snapshots.retain(|(t, _)| *t <= turn);
        for p in &mut self.proposals {
            if p.turn >= turn && p.status != ProposalStatus::Rejected {
                p.status = ProposalStatus::RolledBack;
            }
        }
        self.turn = turn;
        self.phase = Phase::Orders;
        self.last_turn = None;
        self.record("rollback", MASTER, json!({"to_turn": turn}));
        for r in self.role_ids() {
            let text = format!("EXCON: the exercise has been rolled back to the start of turn {turn} ({}).", self.world.clock());
            self.add_message(MASTER, &r, MsgKind::System, None, &text, &text, None);
        }
        Ok(())
    }

    pub fn end(&mut self) -> Result<()> {
        if self.phase == Phase::Resolving {
            bail!("a turn is being resolved; wait for it to finish");
        }
        self.phase = Phase::Debrief;
        self.record("exercise_ended", MASTER, json!({"stats": self.world.stats()}));
        Ok(())
    }

    // --------------------------------------------------------------- messages

    /// Append to a role's conversation with the AI.
    pub fn converse(&mut self, key: &str, role: &str, content: impl Into<String>) {
        let c = self.conversations.entry(key.to_string()).or_default();
        c.push(ChatTurn { role: role.into(), content: content.into() });
        if c.len() > CONVERSATION_MAX {
            let n = c.len() - CONVERSATION_MAX;
            c.drain(0..n);
        }
    }

    /// A role's conversation so far, oldest first.
    pub fn conversation(&self, key: &str) -> Vec<ChatTurn> {
        self.conversations.get(key).cloned().unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_message(&mut self, from: &str, to: &str, kind: MsgKind, title: Option<&str>, text: &str, text_en: &str, proposal: Option<u64>) -> u64 {
        let id = self.id();
        let m = Message {
            id,
            turn: self.turn,
            t_s: self.world.time_s(),
            clock: self.world.clock(),
            from: from.into(),
            to: to.into(),
            kind,
            title: title.map(str::to_string),
            text: text.into(),
            text_en: text_en.into(),
            proposal,
        };
        self.record("message", from, json!({"to": to, "kind": kind, "title": title, "text_en": text_en, "text": text}));
        // What a player receives becomes context of their conversation with the AI.
        // (Clarifications are the AI's own words: they are already in it.)
        if self.players.contains_key(to) && kind != MsgKind::Clarification && kind != MsgKind::Reply {
            let what = match kind {
                MsgKind::Inject => "INJECT",
                MsgKind::Feedback => "EXERCISE CONTROL FEEDBACK",
                MsgKind::Situation => "SITUATION UPDATE",
                MsgKind::RoleMessage => "MESSAGE",
                _ => "NOTICE",
            };
            let head = format!("[{}] {what} from {}{}", self.world.clock(), from, title.map(|t| format!(" — {t}")).unwrap_or_default());
            self.converse(to, "user", format!("{head}:\n{text_en}"));
        }
        self.messages.push(m);
        id
    }

    /// Send an inject. `translations` maps role → text in that role's language;
    /// missing roles get the English text.
    pub fn send_inject(&mut self, inject_id: Option<&str>, title: &str, sender: &str, recipients: &[String], text_en: &str, translations: &BTreeMap<String, String>) -> Result<Vec<u64>> {
        if self.phase == Phase::Debrief {
            bail!("the exercise has ended");
        }
        let to = self.recipients(recipients)?;
        let mut ids = Vec::new();
        for r in &to {
            let text = translations.get(r).cloned().unwrap_or_else(|| text_en.to_string());
            ids.push(self.add_message(sender, r, MsgKind::Inject, Some(title), &text, text_en, None));
        }
        if let Some(id) = inject_id {
            self.sent_injects.push(id.to_string());
        }
        self.record("inject_sent", MASTER, json!({"inject": inject_id, "title": title, "sender": sender, "recipients": to}));
        Ok(ids)
    }

    pub fn inbox(&self, role: &str) -> Vec<&Message> {
        self.messages.iter().filter(|m| m.to == role || (m.from == role && m.kind == MsgKind::Reply)).collect()
    }

    // -------------------------------------------------------------- proposals

    /// A player's free-text reply: stored, and a proposal opened for parsing.
    pub fn player_reply(&mut self, role: &str, text: &str) -> Result<u64> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for replies");
        }
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        if text.trim().is_empty() {
            bail!("empty reply");
        }
        let pid = self.id();
        let lang = self.language_of(role);
        self.proposals.push(Proposal {
            id: pid,
            turn: self.turn,
            t_s: self.world.time_s(),
            source: Source::Player(role.into()),
            original: text.into(),
            language: lang,
            translation_en: String::new(),
            summary: String::new(),
            commands: Vec::new(),
            descriptions: Vec::new(),
            verdicts: Vec::new(),
            invalid: Vec::new(),
            clarification: None,
            status: ProposalStatus::Parsing,
            master_note: None,
            results: Vec::new(),
            inject_drafts: Vec::new(),
        });
        self.add_message(role, MASTER, MsgKind::Reply, None, text, text, Some(pid));
        Ok(pid)
    }

    /// A Master natural-language prompt, opened as a proposal for confirmation.
    pub fn master_prompt(&mut self, text: &str) -> u64 {
        let pid = self.id();
        self.proposals.push(Proposal {
            id: pid,
            turn: self.turn,
            t_s: self.world.time_s(),
            source: Source::Master,
            original: text.into(),
            language: "en".into(),
            translation_en: text.into(),
            summary: String::new(),
            commands: Vec::new(),
            descriptions: Vec::new(),
            verdicts: Vec::new(),
            invalid: Vec::new(),
            clarification: None,
            status: ProposalStatus::Parsing,
            master_note: None,
            results: Vec::new(),
            inject_drafts: Vec::new(),
        });
        self.record("master_prompt", MASTER, json!({"proposal": pid, "text": text}));
        pid
    }

    fn proposal_mut(&mut self, pid: u64) -> Result<&mut Proposal> {
        self.proposals.iter_mut().find(|p| p.id == pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))
    }

    pub fn proposal(&self, pid: u64) -> Option<&Proposal> {
        self.proposals.iter().find(|p| p.id == pid)
    }

    /// Approved-but-unapplied player actions, which hold resources.
    fn prior(&self, except: u64) -> Vec<(Actor, Command)> {
        self.proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved && p.id != except)
            .flat_map(|p| p.commands.iter().map(move |c| (p.source.actor(), c.clone())))
            .collect()
    }

    fn set_commands(&mut self, pid: u64, commands: Vec<Command>) -> Result<()> {
        let actor = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?.source.actor();
        let prior = self.prior(pid);
        let verdicts = self.world.validate_batch(&prior, &commands, &actor);
        let p = self.proposal_mut(pid)?;
        p.descriptions = commands.iter().map(|c| c.describe()).collect();
        p.commands = commands;
        p.verdicts = verdicts;
        Ok(())
    }

    /// Attach the LLM's interpretation. Returns the clarification question
    /// (in the player's language) if the player must be asked one.
    pub fn attach_parse(&mut self, pid: u64, parsed: ParsedInput) -> Result<Option<String>> {
        let mut commands = Vec::new();
        let mut invalid = Vec::new();
        for a in &parsed.actions {
            match serde_json::from_value::<Command>(a.clone()) {
                Ok(c) => commands.push(c),
                Err(e) => invalid.push(format!("{a} ({e})")),
            }
        }
        self.set_commands(pid, commands)?;
        let p = self.proposal_mut(pid)?;
        if !parsed.language.is_empty() {
            p.language = parsed.language.clone();
        }
        p.translation_en = parsed.translation_en.clone();
        p.summary = parsed.summary.clone();
        p.invalid = invalid;
        p.clarification = parsed.clarification.clone();
        p.inject_drafts = parsed.inject_drafts.clone();
        p.status = if p.commands.is_empty() && p.inject_drafts.is_empty() && p.clarification.is_some() { ProposalStatus::NeedsClarification } else { ProposalStatus::Pending };
        let (source, clar) = (p.source.clone(), p.clarification.clone());
        let snapshot = serde_json::to_value(&*p)?;
        self.record("proposal_parsed", source.label(), snapshot);
        if let (Source::Player(role), Some(q)) = (&source, &clar) {
            self.add_message(MASTER, role, MsgKind::Clarification, Some("Clarification needed"), q, q, Some(pid));
        }
        Ok(clar)
    }

    /// The LLM could not parse: the proposal waits for the Master, who can
    /// type the commands themselves.
    pub fn parse_failed(&mut self, pid: u64, error: &str) -> Result<()> {
        let p = self.proposal_mut(pid)?;
        p.status = ProposalStatus::Pending;
        p.summary = format!("(AI unavailable: {error}) — enter the actions manually");
        let label = p.source.label().to_string();
        self.record("proposal_parse_failed", &label, json!({"proposal": pid, "error": error}));
        Ok(())
    }

    /// The Master decides on a proposal.
    pub fn decide(&mut self, pid: u64, d: Decision) -> Result<DecisionOutcome> {
        let status = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?.status;
        if !matches!(status, ProposalStatus::Pending | ProposalStatus::NeedsClarification | ProposalStatus::Parsing) {
            bail!("proposal {pid} is already {status:?}");
        }
        // Re-check feasibility now: resources may have changed since parsing.
        let cmds = match d.commands.clone() {
            Some(c) => c,
            None => self.proposal(pid).unwrap().commands.clone(),
        };
        self.set_commands(pid, cmds)?;
        let p = self.proposal(pid).unwrap().clone();
        let notify = match &p.source {
            Source::Player(r) => Some(r.clone()),
            Source::Master => None,
        };
        let note = d.note.clone().filter(|n| !n.trim().is_empty());
        let mut facts = Vec::new();
        if !d.approve {
            self.proposal_mut(pid)?.status = ProposalStatus::Rejected;
            facts.push(format!("Your orders were NOT approved by exercise control: {}", if p.descriptions.is_empty() { p.summary.clone() } else { p.descriptions.join("; ") }));
        } else if p.commands.is_empty() && p.inject_drafts.is_empty() {
            bail!("nothing to approve: proposal {pid} has no actions (edit it to add some, or reject it)");
        } else if matches!(p.source, Source::Master) {
            let mut results = Vec::new();
            for c in &p.commands {
                match self.world.apply(c, &Actor::Master) {
                    Ok(n) => results.push(format!("{}: done{}", c.describe(), if n.is_empty() { String::new() } else { format!(" ({})", n.join("; ")) })),
                    Err(e) => results.push(format!("{}: failed ({e})", c.describe())),
                }
            }
            let pm = self.proposal_mut(pid)?;
            pm.status = ProposalStatus::Applied;
            pm.results = results.clone();
            facts.extend(results);
        } else {
            self.proposal_mut(pid)?.status = ProposalStatus::Approved;
            for (desc, v) in p.descriptions.iter().zip(&p.verdicts) {
                match v {
                    Verdict::Ok { notes } => facts.push(format!("APPROVED: {desc}{}. It will be executed at the end of the turn.", if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) })),
                    Verdict::Rejected { reason } => facts.push(format!("APPROVED BUT NOT FEASIBLE: {desc} — {reason}")),
                }
            }
        }
        if let Some(n) = &note {
            facts.push(format!("Note from exercise control: {n}"));
        }
        self.proposal_mut(pid)?.master_note = note;
        let facts_en = facts.join("\n");
        self.record("proposal_decided", MASTER, json!({"proposal": pid, "approve": d.approve, "edited": d.commands.is_some(), "facts": facts_en}));
        Ok(DecisionOutcome { proposal: pid, notify, facts_en })
    }

    /// A command from the Master GUI or a script: validated as Master and applied now.
    pub fn direct_command(&mut self, cmd: &Command) -> Result<Vec<String>> {
        let r = self.world.apply(cmd, &Actor::Master);
        self.record("master_command", MASTER, json!({"command": cmd, "result": match &r { Ok(n) => json!({"ok": n}), Err(e) => json!({"error": e}) }}));
        r.map_err(|e| anyhow::anyhow!(e))
    }

    /// The full log as a debrief timeline.
    pub fn debrief(&self) -> Value {
        json!({
            "scenario": self.world.scn.name,
            "turns": self.turn,
            "stats": self.world.stats(),
            "proposals": self.proposals,
            "messages": self.messages,
            "log": self.log,
        })
    }
}
