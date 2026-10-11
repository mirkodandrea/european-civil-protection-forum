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

pub mod limits;
pub use limits::ResourceLimits;
pub mod timing;
pub use timing::{ExercisePhase, PhaseEntry, ClockControl, ScheduledInject, ScheduleStatus};
pub mod evaluation;
pub mod design;
pub mod interventions;
pub mod knowledge;
pub use knowledge::{FireCell, MapObservation, KnowledgeEvent, KnowledgeSource};

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::{Actor, Command, SimEvent, Verdict, World, WorldSnapshot};

pub const DEFAULT_TURN_MINUTES: u32 = 30;
/// Conversation turns kept per role (oldest dropped first).
pub const CONVERSATION_MAX: usize = 60;
/// Resolution granularity: the world is published after every simulated second.
pub const STEP_S: i64 = 1;
pub const MASTER: &str = "master";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub language_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Unique delivery event, shared by every recipient of an inject.
    pub inject_id: Option<u64>,
    pub reply_to: Option<u64>,
    pub recipients: Vec<String>,
    pub wall_ms: u128,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationKind {
    Request,
    #[default]
    ProposedDecision,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusChange {
    pub status: ProposalStatus,
    pub turn: u32,
    pub t_s: i64,
    pub wall_ms: u128,
    pub actor: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRecord {
    pub resource_limits:Option<ResourceLimits>,
    pub approve: bool,
    pub actor: String,
    pub turn: u32,
    pub t_s: i64,
    pub wall_ms: u128,
    pub note: Option<String>,
    pub edited: bool,
    pub proposed_commands: Vec<Command>,
    pub effective_commands: Vec<Command>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionRecord {
    pub allocations: Vec<u64>,
    pub command: Command,
    pub ok: bool,
    pub detail: String,
    pub t_s: i64,
    pub wall_ms: u128,
    pub origin: String,
}

fn wall_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasisKind {
    Procedure,
    Plan,
    OperationalJudgment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionBasis {
    pub kind: BasisKind,
    /// Procedure/plan reference or the player's operational rationale.
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvidence {
    pub basis: DecisionBasis,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl DecisionEvidence {
    pub fn judgment(reference: &str) -> Self {
        Self { basis: DecisionBasis { kind: BasisKind::OperationalJudgment, reference: reference.into() }, attachments: Vec::new() }
    }

    pub fn validate(&self) -> Result<()> {
        if self.basis.reference.trim().is_empty() || self.basis.reference.chars().count() > 2000 { bail!("decision basis requires a reference or rationale (up to 2000 characters)"); }
        if self.attachments.len() > 20 { bail!("at most 20 evidence links are allowed"); }
        for attachment in &self.attachments {
            if attachment.title.trim().is_empty() || attachment.title.chars().count() > 200 || attachment.url.chars().count() > 4000 { bail!("evidence link needs a title (up to 200 characters) and URL (up to 4000 characters)"); }
            let url = url::Url::parse(&attachment.url)?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() { bail!("evidence links must use HTTP or HTTPS"); }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunicationDraft {
    pub to: String,
    pub from: String,
    pub kind: MsgKind,
    pub title: String,
    pub text_en: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub resource_limit_draft:Option<Value>,
    pub evidence: Option<DecisionEvidence>,
    pub communication_drafts: Vec<CommunicationDraft>,
    pub communications_claimed: bool,
    pub delivered_communications: Vec<usize>,
    pub declaration_kind: DeclarationKind,
    pub history: Vec<StatusChange>,
    pub decision: Option<ApprovalRecord>,
    pub executions: Vec<ExecutionRecord>,
    pub inject_id: Option<u64>,
    pub reply_to: Option<u64>,
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
    pub resource_limit_draft:Option<Value>,
    pub communication_drafts: Vec<CommunicationDraft>,
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
    pub exercise_phase: ExercisePhase,
    pub phase: Phase,
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
    pub training_design: design::TrainingDesign,
    pub current_path: Option<String>,
    pub path_history: Vec<Value>,
    intervention: Option<interventions::Intervention>,
    pub resource_limits:ResourceLimits,
    pub exercise_phase: ExercisePhase,
    pub phase_history: Vec<PhaseEntry>,
    pub clock_control: ClockControl,
    pub scheduled_injects: Vec<ScheduledInject>,
    pub world: World,
    pub turn: u32,
    pub phase: Phase,
    pub turn_minutes: u32,
    pub players: BTreeMap<String, Player>,
    pub messages: Vec<Message>,
    pub proposals: Vec<Proposal>,
    pub log: Vec<LogEntry>,
    /// Append-only accounting evidence, including operations later rolled back.
    pub resource_history: Vec<sim::ResourceEvent>,
    pub knowledge_events: Vec<KnowledgeEvent>,
    pub reference_materials: Vec<DecisionEvidence>,
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
            .map(|r| (r.id.clone(), Player { role: r.id.clone(), name: r.name.clone(), language: r.language.clone(), language_confirmed: false }))
            .collect();
        let mut ex = Exercise {
            training_design: Default::default(), current_path: None, path_history: Vec::new(), intervention: None,
            resource_limits:ResourceLimits::default(),
            exercise_phase:ExercisePhase::EarlyWarning,
            phase_history:vec![PhaseEntry {phase:ExercisePhase::EarlyWarning,turn:0,t_s:world.time_s(),wall_ms:wall_ms(),reason:"Initial training phase".into()}],
            clock_control:ClockControl::default(),
            scheduled_injects:Vec::new(),
            world,
            turn: 0,
            phase: Phase::Briefing,
            turn_minutes: DEFAULT_TURN_MINUTES,
            players,
            messages: Vec::new(),
            proposals: Vec::new(),
            log: Vec::new(),
            resource_history: Vec::new(),
            knowledge_events: Vec::new(),
            reference_materials: Vec::new(),
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
            exercise_phase:self.exercise_phase,
            phase: self.phase,
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

    fn record_resource_events(&mut self, from: usize, actor: &str) {
        let events = self.world.d.resource_events[from..].to_vec();
        for event in events {
            self.record("resource_event", actor, json!(&event));
            self.resource_history.push(event);
        }
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
        if !["en", "it", "fr", "de", "es", "hy", "ka", "uk", "ro", "az", "ru", "el", "pt"].contains(&lang) { bail!("choose a supported language"); }
        let Some(p) = self.players.get_mut(role) else { bail!("unknown role {role}") };
        p.language = lang.to_string();
        p.language_confirmed = true;
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
        self.stop_clock("Turn resolution uses its own duration and playback speed");
        let minutes = minutes.unwrap_or(self.turn_minutes).clamp(1, 24 * 60);
        let from_s = self.world.time_s();
        let mut applied = Vec::new();
        let approved: Vec<u64> = self.proposals.iter().filter(|p| p.status == ProposalStatus::Approved).map(|p| p.id).collect();
        for pid in approved {
            applied.extend(self.execute_proposal(pid)?);
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
            let resource_event_count = self.world.d.resource_events.len();
            let events = self.world.advance(dt)?;
            self.record_resource_events(resource_event_count, MASTER);
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
            let resource_event_count = self.world.d.resource_events.len();
            self.world.advance(dt)?;
            self.record_resource_events(resource_event_count, MASTER);
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
        self.validate_restored_limits(snap.resource_pools())?;
        self.stop_clock("Explicit snapshot rollback");
        self.world.restore(&snap)?;
        self.snapshots.retain(|(t, _)| *t <= turn);
        let undone: Vec<u64> = self.proposals.iter().filter(|p| p.turn >= turn && p.status != ProposalStatus::Rejected && p.status != ProposalStatus::RolledBack).map(|p| p.id).collect();
        for pid in undone { self.transition(pid, ProposalStatus::RolledBack, MASTER, "Undone by explicit rollback")?; }
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
        self.stop_clock("Exercise ended");
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
        let thread = proposal.and_then(|pid| self.proposal(pid)).and_then(|p| p.inject_id.map(|id| (id, p.reply_to, vec![to.into()])));
        self.add_message_in_thread(from, to, kind, title, text, text_en, proposal, thread)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_message_in_thread(&mut self, from: &str, to: &str, kind: MsgKind, title: Option<&str>, text: &str, text_en: &str, proposal: Option<u64>, thread: Option<(u64, Option<u64>, Vec<String>)>) -> u64 {
        let id = self.id();
        let (inject_id, reply_to, recipients) = match thread {
            Some((root, parent, recipients)) => (Some(root), parent, recipients),
            None => (None, None, vec![to.into()]),
        };
        let m = Message {
            inject_id,
            reply_to,
            recipients,
            wall_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
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
        self.record("message", from, json!(&m));
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
        let delivery_id = self.id();
        let mut ids = Vec::new();
        for r in &to {
            let text = translations.get(r).cloned().unwrap_or_else(|| text_en.to_string());
            let mid = self.add_message_in_thread(sender, r, MsgKind::Inject, Some(title), &text, text_en, None, Some((delivery_id, None, to.clone())));
            ids.push(mid);
        }
        if let Some(id) = inject_id {
            self.sent_injects.push(id.to_string());
        }
        self.record("inject_sent", MASTER, json!({"inject_id": delivery_id, "msel_id": inject_id, "title": title, "sender": sender, "recipients": to, "messages": ids}));
        Ok(ids)
    }

    pub fn inbox(&self, role: &str) -> Vec<&Message> {
        self.messages.iter().filter(|m| m.to == role || (m.from == role && m.kind == MsgKind::Reply)).collect()
    }

    // -------------------------------------------------------------- proposals

    /// A player's free-text reply: stored, and a proposal opened for parsing.
    pub fn player_reply(&mut self, role: &str, text: &str, reply_to: u64, evidence: DecisionEvidence) -> Result<u64> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for replies");
        }
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        if text.trim().is_empty() {
            bail!("empty reply");
        }
        if !self.players[role].language_confirmed { bail!("select your language before submitting a declaration"); }
        evidence.validate()?;
        let parent = self.messages.iter().find(|m| m.id == reply_to && m.to == role && matches!(m.kind, MsgKind::Inject | MsgKind::Clarification))
            .ok_or_else(|| anyhow::anyhow!("reply_to must reference an inject or clarification delivered to your role"))?;
        let inject_id = parent.inject_id.ok_or_else(|| anyhow::anyhow!("message has no inject thread"))?;
        let pid = self.id();
        let lang = self.language_of(role);
        self.proposals.push(Proposal {
            resource_limit_draft:None,
            evidence: Some(evidence),
            communication_drafts: Vec::new(),
            communications_claimed: false,
            delivered_communications: Vec::new(),
            declaration_kind: DeclarationKind::ProposedDecision,
            history: Vec::new(),
            decision: None,
            executions: Vec::new(),
            inject_id: Some(inject_id),
            reply_to: Some(reply_to),
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
        self.transition(pid, ProposalStatus::Parsing, role, "Declaration recorded; awaiting interpretation")?;
        self.record("proposal_declared", role, json!(self.proposal(pid)));
        self.add_message(role, MASTER, MsgKind::Reply, None, text, text, Some(pid));
        Ok(pid)
    }

    /// A Master natural-language prompt, opened as a proposal for confirmation.
    pub fn master_prompt(&mut self, text: &str) -> u64 {
        let pid = self.id();
        self.proposals.push(Proposal {
            resource_limit_draft:None,
            evidence: None,
            communication_drafts: Vec::new(),
            communications_claimed: false,
            delivered_communications: Vec::new(),
            declaration_kind: DeclarationKind::ProposedDecision,
            history: Vec::new(),
            decision: None,
            executions: Vec::new(),
            inject_id: None,
            reply_to: None,
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
        self.transition(pid, ProposalStatus::Parsing, MASTER, "Master suggestion requested").expect("new proposal");
        self.record("master_prompt", MASTER, json!({"proposal": pid, "text": text}));
        pid
    }

    fn transition(&mut self, pid: u64, status: ProposalStatus, actor: &str, note: &str) -> Result<()> {
        let change = StatusChange { status, turn: self.turn, t_s: self.world.time_s(), wall_ms: wall_ms(), actor: actor.into(), note: note.into() };
        let p = self.proposal_mut(pid)?;
        p.status = status;
        p.history.push(change.clone());
        self.record("proposal_status", actor, json!({"proposal": pid, "change": change}));
        Ok(())
    }

    /// Set the explicitly selected declaration type before parsing starts.
    pub fn set_declaration_kind(&mut self, pid: u64, kind: DeclarationKind) -> Result<()> {
        let p = self.proposal_mut(pid)?;
        if p.status != ProposalStatus::Parsing { bail!("declaration already interpreted"); }
        p.declaration_kind = kind;
        let actor = p.source.label().to_string();
        self.record("declaration_kind", &actor, json!({"proposal": pid, "kind": kind}));
        Ok(())
    }

    pub fn execute_approved(&mut self, pid:u64) -> Result<Vec<AppliedAction>> {
        if self.phase!=Phase::Orders { bail!("execution requires an open turn"); }
        self.execute_proposal(pid)
    }

    fn execute_proposal(&mut self, pid: u64) -> Result<Vec<AppliedAction>> {
        let p = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?.clone();
        if p.status != ProposalStatus::Approved || p.decision.as_ref().map(|d| d.approve) != Some(true) {
            bail!("proposal has no effective Master approval");
        }
        if let Some(draft)=&p.resource_limit_draft {
            self.set_resource_limits(serde_json::from_value(draft.clone())?,p.decision.as_ref().and_then(|d|d.note.as_deref()).unwrap_or("Approved Master resource-limit recommendation"),Some(pid))?;
        }
        let origin = if matches!(p.source, Source::Master) { "approved_master_suggestion" } else { "approved_player_decision" };
        let mut applied = Vec::new();
        let mut executions = Vec::new();
        for cmd in &p.commands {
            let allocation_count = self.world.d.allocations.len();
            let resource_event_count = self.world.d.resource_events.len();
            let mut verdicts=self.world.validate_batch(&self.prior(pid),std::slice::from_ref(cmd),&p.source.actor());
            self.check_limit_batch(pid,std::slice::from_ref(cmd),&p.source.actor(),&mut verdicts);
            let result=if let Verdict::Rejected {reason}=&verdicts[0]{Err(reason.clone())}else{self.world.apply(cmd,&p.source.actor())};
            let (ok, detail) = match result {
                Ok(notes) => (true, notes.join("; ")),
                Err(reason) => (false, reason),
            };
            self.record_resource_events(resource_event_count, p.source.label());
            if ok && matches!(cmd, Command::AdvanceTime { .. }) { self.clock_control.revision+=1; }
            let execution = ExecutionRecord { allocations: self.world.d.allocations[allocation_count..].iter().map(|a| a.id).collect(), command: cmd.clone(), ok, detail: detail.clone(), t_s: self.world.time_s(), wall_ms: wall_ms(), origin: origin.into() };
            self.record("action_executed", MASTER, json!({"proposal": pid, "inject_id": p.inject_id, "execution": execution}));
            executions.push(execution);
            applied.push(AppliedAction { proposal: pid, by: p.source.label().into(), action: cmd.describe(), ok, detail });
        }
        let all_ok = applied.iter().all(|a| a.ok);
        let pm = self.proposal_mut(pid)?;
        pm.results = applied.iter().map(|a| format!("{}: {} ({})", a.action, if a.ok { "executed / dispatched" } else { "not executed" }, a.detail)).collect();
        if p.resource_limit_draft.is_some(){pm.results.push("Master-approved resource limits applied".into());}
        pm.executions = executions;
        self.transition(pid, if all_ok { ProposalStatus::Applied } else { ProposalStatus::Failed }, MASTER, "Execution outcomes recorded")?;
        Ok(applied)
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
        let mut verdicts = self.world.validate_batch(&prior, &commands, &actor);
        self.check_limit_batch(pid,&commands,&actor,&mut verdicts);
        let p = self.proposal_mut(pid)?;
        p.descriptions = commands.iter().map(|c| c.describe()).collect();
        p.commands = commands;
        p.verdicts = verdicts;
        Ok(())
    }

    /// Attach the LLM's interpretation. Returns the clarification question
    /// (in the player's language) if the player must be asked one.
    pub fn attach_parse(&mut self, pid: u64, parsed: ParsedInput) -> Result<Option<String>> {
        if self.proposal(pid).map(|p| p.status) != Some(ProposalStatus::Parsing) { bail!("proposal is no longer awaiting interpretation"); }
        let mut commands = Vec::new();
        let mut invalid = Vec::new();
        for a in &parsed.actions {
            match serde_json::from_value::<Command>(a.clone()) {
                Ok(c) => commands.push(c),
                Err(e) => invalid.push(format!("{a} ({e})")),
            }
        }
        if parsed.resource_limit_draft.is_some() && !matches!(self.proposal(pid).unwrap().source,Source::Master){bail!("players cannot draft operational limits");}
        if let Some(draft)=&parsed.resource_limit_draft {
            if let Err(e)=serde_json::from_value::<ResourceLimits>(draft.clone()).map_err(anyhow::Error::from).and_then(|limits|self.validate_limits_for_phase(&limits,self.exercise_phase)){invalid.push(format!("resource limits: {e}"));}
        }
        self.set_commands(pid, commands)?;
        let p = self.proposal_mut(pid)?;
        p.resource_limit_draft=parsed.resource_limit_draft;
        if !parsed.language.is_empty() {
            p.language = parsed.language.clone();
        }
        p.translation_en = parsed.translation_en.clone();
        p.summary = parsed.summary.clone();
        p.invalid = invalid;
        p.clarification = parsed.clarification.clone();
        p.inject_drafts = parsed.inject_drafts.clone();
        p.communication_drafts = parsed.communication_drafts.clone();
        if let (Source::Player(role), Some(q)) = (&p.source, &parsed.clarification) {
            p.communication_drafts.push(CommunicationDraft { to: role.clone(), from: MASTER.into(), kind: MsgKind::Clarification, title: "Clarification needed".into(), text_en: q.clone() });
        }
        let status = if p.commands.is_empty() && p.inject_drafts.is_empty() && p.clarification.is_some() { ProposalStatus::NeedsClarification } else { ProposalStatus::Pending };
        let (source, clar) = (p.source.clone(), p.clarification.clone());
        self.transition(pid, status, source.label(), "Interpretation stored; awaiting exercise control")?;
        self.record("proposal_parsed", source.label(), serde_json::to_value(self.proposal(pid))?);
        Ok(clar)
    }

    /// The LLM could not parse: the proposal waits for the Master, who can
    /// type the commands themselves.
    pub fn parse_failed(&mut self, pid: u64, error: &str) -> Result<()> {
        if self.proposal(pid).map(|p| p.status) != Some(ProposalStatus::Parsing) { bail!("proposal is no longer awaiting interpretation"); }
        let p = self.proposal_mut(pid)?;
        p.summary = format!("(AI unavailable: {error}) — enter the actions manually");
        let label = p.source.label().to_string();
        self.transition(pid, ProposalStatus::Pending, &label, "Manual interpretation required")?;
        self.record("proposal_parse_failed", &label, json!({"proposal": pid, "error": error}));
        Ok(())
    }

    /// Deliver an approved communication once; translation may only preserve its meaning.
    pub fn deliver_approved_communication(&mut self, pid: u64, index: usize, translated: &str) -> Result<u64> {
        let p = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if self.phase == Phase::Debrief || p.status != ProposalStatus::Applied || p.decision.as_ref().map(|d| d.approve) != Some(true) { bail!("communication has no effective Master approval"); }
        if p.delivered_communications.contains(&index) { bail!("communication already delivered"); }
        let draft = p.communication_drafts.get(index).ok_or_else(|| anyhow::anyhow!("unknown communication draft"))?.clone();
        let mid = self.add_message(&draft.from, &draft.to, draft.kind, Some(&draft.title), translated, &draft.text_en, Some(pid));
        let p = self.proposal_mut(pid)?;
        p.delivered_communications.push(index);
        p.results.push(format!("Communication to {} delivered as message #{mid}", draft.to));
        self.record("approved_communication_delivered", MASTER, json!({"proposal": pid, "draft": index, "message": mid}));
        Ok(mid)
    }

    /// Review/edit operational communications while the proposal is still pending.
    pub fn edit_communications(&mut self, pid: u64, communications: Option<Vec<CommunicationDraft>>, injects: Option<Vec<Value>>) -> Result<()> {
        let p = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if !matches!(p.status, ProposalStatus::Parsing | ProposalStatus::Pending | ProposalStatus::NeedsClarification) { bail!("proposal already decided"); }
        if let Some(drafts) = &communications {
            for d in drafts {
                if !self.players.contains_key(&d.to) || !matches!(d.kind, MsgKind::RoleMessage | MsgKind::Clarification) || d.text_en.trim().is_empty() { bail!("invalid communication draft"); }
                if d.from != MASTER && d.from != p.source.label() { bail!("draft sender must match the proposal origin or EXCON"); }
            }
        }
        if let Some(drafts) = &injects {
            if !matches!(p.source, Source::Master) && !drafts.is_empty() { bail!("players cannot draft scenario injects"); }
            for d in drafts {
                if d["text"].as_str().unwrap_or("").trim().is_empty() { bail!("inject draft needs text"); }
                let to: Vec<String> = serde_json::from_value(d["recipients"].clone())?;
                self.recipients(&to)?;
            }
        }
        let before = json!({"communications": p.communication_drafts, "injects": p.inject_drafts});
        let p = self.proposal_mut(pid)?;
        if let Some(d) = communications { p.communication_drafts = d; }
        if let Some(d) = injects { p.inject_drafts = d; }
        let after = json!({"communications": p.communication_drafts, "injects": p.inject_drafts});
        self.record("communications_reviewed", MASTER, json!({"proposal": pid, "before": before, "after": after}));
        Ok(())
    }

    /// The Master decides on a proposal.
    pub fn decide(&mut self, pid: u64, d: Decision) -> Result<DecisionOutcome> {
        if self.phase != Phase::Orders { bail!("decisions require an open exercise turn"); }
        let status = self.proposal(pid).ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?.status;
        if !matches!(status, ProposalStatus::Pending | ProposalStatus::NeedsClarification | ProposalStatus::Parsing) {
            bail!("proposal {pid} is already {status:?}");
        }
        let proposed_commands = self.proposal(pid).unwrap().commands.clone();
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
        if d.approve && matches!(p.source,Source::Master) && p.summary.contains("UNPLANNED DIVERGENCE") && note.is_none() {bail!("unplanned training divergence requires explicit Master rationale");}
        if d.approve {
            self.edit_communications(pid, Some(p.communication_drafts.clone()), Some(p.inject_drafts.clone()))?;
            if p.commands.is_empty() && p.inject_drafts.is_empty() && p.communication_drafts.is_empty() && p.resource_limit_draft.is_none() { bail!("nothing to approve: proposal {pid} has no actions (edit it to add some, or reject it)"); }
            if p.verdicts.iter().any(|v| !v.is_ok()) { bail!("proposal is not feasible; edit its actions or resources before approval"); }
            if d.commands.is_none() && !p.invalid.is_empty() { bail!("proposal contains invalid actions; edit it before approval"); }
        }
        let effective_limits=if d.approve {p.resource_limit_draft.as_ref().map(|v|serde_json::from_value::<ResourceLimits>(v.clone())).transpose()?}else{None};
        if d.approve {if let Some(limits)=&effective_limits{self.validate_limits_for_phase(limits,self.exercise_phase)?;}}
        self.proposal_mut(pid)?.decision = Some(ApprovalRecord {
            resource_limits:effective_limits,
            approve: d.approve, actor: MASTER.into(), turn: self.turn, t_s: self.world.time_s(), wall_ms: wall_ms(), note: note.clone(), edited: d.commands.is_some(), proposed_commands, effective_commands: p.commands.clone(),
        });
        let mut facts = Vec::new();
        if !d.approve {
            self.transition(pid, ProposalStatus::Rejected, MASTER, note.as_deref().unwrap_or("Rejected by exercise control"))?;
            facts.push(format!("Your orders were NOT approved by exercise control: {}", if p.descriptions.is_empty() { p.summary.clone() } else { p.descriptions.join("; ") }));
        } else {
            self.transition(pid, ProposalStatus::Approved, MASTER, note.as_deref().unwrap_or("Approved by exercise control"))?;
            if matches!(p.source, Source::Master) || p.commands.is_empty() {
                self.execute_proposal(pid)?;
                facts.extend(self.proposal(pid).unwrap().results.clone());
                if p.commands.is_empty() && (!p.communication_drafts.is_empty() || !p.inject_drafts.is_empty()) { facts.push("APPROVED: the reviewed communication will be delivered by exercise control.".into()); }
            } else {
                for (desc, v) in p.descriptions.iter().zip(&p.verdicts) {
                    let notes = match v { Verdict::Ok { notes } => notes.join("; "), _ => String::new() };
                    facts.push(format!("APPROVED: {desc}{}. It awaits EXCON execution or turn resolution.", if notes.is_empty() { String::new() } else { format!(" ({notes})") }));
                }
            }
        }
        if let Some(n) = &note {
            facts.push(format!("Note from exercise control: {n}"));
        }
        self.proposal_mut(pid)?.master_note = note;
        let facts_en = facts.join("\n");
        self.record("proposal_decided", MASTER, json!({"proposal": pid, "inject_id": p.inject_id, "decision": self.proposal(pid).unwrap().decision, "facts": facts_en}));
        Ok(DecisionOutcome { proposal: pid, notify, facts_en })
    }

    /// A command from the Master GUI or a script: validated as Master and applied now.
    pub fn direct_command(&mut self, cmd: &Command) -> Result<Vec<String>> {
        let before = if interventions::physical(cmd) {Some(self.world.snapshot())}else{None};
        let resource_event_count = self.world.d.resource_events.len();
        let verdict=self.world.validate_batch(&self.prior(0),std::slice::from_ref(cmd),&Actor::Master).remove(0);
        let r=if let Verdict::Rejected {reason}=verdict{Err(reason)}else{self.world.apply(cmd,&Actor::Master)};
        self.record_resource_events(resource_event_count, MASTER);
        if r.is_ok() && matches!(cmd, Command::AdvanceTime { .. }) { self.clock_control.revision+=1; }
        self.record("master_command", MASTER, json!({"origin": "direct_excon_intervention", "command": cmd, "result": match &r { Ok(n) => json!({"ok": n}), Err(e) => json!({"error": e}) }}));
        if r.is_ok() { if let Some(snapshot)=before {self.intervention=Some(interventions::Intervention {snapshot,generation:self.generation,log_seq:self.log.last().unwrap().seq,command:cmd.clone()});} }
        r.map_err(|e| anyhow::anyhow!(e))
    }

    /// Separate unmet declarations, approved reservations and actual deployments.
    pub fn resource_accounting(&self, only_role: Option<&str>) -> Value {
        let mut roles = serde_json::Map::new();
        for (role, pools) in &self.world.d.pools {
            if only_role.is_some_and(|r| r != role) { continue; }
            let mut resources = serde_json::Map::new();
            for (resource, pool) in pools {
                let mut requested = 0u64;
                let mut committed = 0u64;
                for p in self.proposals.iter().filter(|p| p.source.label() == role && matches!(p.status, ProposalStatus::Parsing | ProposalStatus::Pending | ProposalStatus::NeedsClarification | ProposalStatus::Approved)) {
                    let qty: u64 = p.commands.iter().map(|c| self.world.resource_requirements(c, &p.source.actor()).get(resource).copied().unwrap_or(0) as u64).sum();
                    if p.status == ProposalStatus::Approved { committed += qty; } else { requested += qty; }
                }
                let returned: u64 = self.world.d.allocations.iter().filter(|a| &a.role == role && &a.resource == resource).map(|a| a.returned as u64).sum();
                let external_requested: u64 = self.world.d.arrivals.iter().filter(|a| &a.role == role && &a.resource == resource && a.at_s > self.world.time_s()).map(|a| a.qty as u64).sum();
                let effective_capacity=self.effective_capacity(resource,self.exercise_phase,&self.resource_limits);
                let shared_used=self.resource_usage_with_commitments(0).get(resource).copied().unwrap_or(0);
                let deployable=(pool.available() as u64).saturating_sub(committed).min(effective_capacity.saturating_sub(shared_used));
                resources.insert(resource.clone(), json!({"inventory_available":(pool.available() as u64).saturating_sub(committed),"effective_capacity":effective_capacity,"total_limit":self.resource_limits.total.get(resource),"phase_limit":self.resource_limits.phases.get(&self.exercise_phase).and_then(|p|p.get(resource)),"total": pool.total, "requested": requested, "committed": committed, "deployed": pool.used, "available": deployable, "returned": returned, "external_requested": external_requested}));
            }
            roles.insert(role.clone(), Value::Object(resources));
        }
        Value::Object(roles)
    }

    /// The full log as a debrief timeline.
    pub fn debrief(&self) -> Value {
        json!({
            "training_design":self.design_view(),
            "resource_limits":self.resource_limits,
            "exercise_phase":self.exercise_phase,
            "phase_history":self.phase_history,
            "clock_control":self.clock_control,
            "scheduled_injects":self.scheduled_injects,
            "scenario": self.world.scn.name,
            "turns": self.turn,
            "stats": self.world.stats(),
            "resource_allocations": self.world.d.allocations,
            "resource_events": self.resource_history,
            "resource_accounting": self.resource_accounting(None),
            "physical_state": self.world.view(),
            "knowledge_events": self.knowledge_events,
            "reference_materials": self.reference_materials,
            "objectives": self.world.scn.msel.iter().map(|i| json!({"inject":i.id,"objective":i.objective})).collect::<Vec<_>>(),
            "proposals": self.proposals,
            "messages": self.messages,
            "log": self.log,
        })
    }
}
