//! The turn-based exercise engine.
//!
//! Owns the [`World`] and everything around it that is not simulation:
//! turns and phases, injects, per-role inboxes, proposals and the EXCON's
//! decisions, per-turn snapshots for rollback, and the event log.
//!
//! It is synchronous and knows nothing about LLMs or HTTP: the server runs
//! the LLM workflows and feeds their results in ([`Exercise::attach_parse`],
//! [`Exercise::add_message`]). That keeps the whole turn loop testable
//! headless and makes "LLM down" a normal path — the EXCON can type
//! commands for any proposal.
//!
//! ## Turn loop
//!
//! `Briefing` → [`Exercise::start`] → `Orders` (injects go out, players reply,
//! proposals are parsed and decided) → [`Exercise::resolve_turn`] (approved
//! player actions applied, simulated time advanced, snapshot) → `Orders` of
//! the next turn … → [`Exercise::end`] → `Debrief`.
//!
//! EXCON actions (direct commands, approved EXCON prompts) apply at once;
//! approved player actions apply together at resolution.

pub mod limits;
pub use limits::ResourceLimits;
pub mod chat_policy;
pub mod timing;
pub use timing::{ClockControl, ExercisePhase, PhaseEntry, ScheduleStatus, ScheduledInject};
pub mod design;
pub mod drafts;
pub mod evaluation;
pub mod interventions;
pub mod knowledge;
pub mod summaries;
pub mod threads;
pub mod planned;
pub mod agenda;
pub mod actions;
pub use knowledge::{BuildingObs, FireCell, KnowledgeEvent, KnowledgeSource, MapObservation};

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::{Actor, Command, SimEvent, Verdict, World, WorldSnapshot};

pub const DEFAULT_TURN_MINUTES: u32 = scenario::DEFAULT_TURN_MINUTES;
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
    Chat,
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
    pub original: Option<String>,
    pub language_src: Option<String>,
    pub translation_failed: bool,
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
    /// Chat interpreted without an operational action.
    Informational,
    /// Waiting for the LLM.
    Parsing,
    /// Waiting for the EXCON.
    Pending,
    /// The LLM asked the player a question; the EXCON may still act on it.
    NeedsClarification,
    /// Approved; applies at turn resolution.
    Approved,
    Rejected,
    Applied,
    /// Approved, but could not be executed at resolution.
    Failed,
    /// Undone by a rollback.
    RolledBack,
    /// Replaced by the player's answer to the EXCON's clarification question.
    Superseded,
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
    pub resource_limits: Option<ResourceLimits>,
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
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
        Self {
            basis: DecisionBasis {
                kind: BasisKind::OperationalJudgment,
                reference: reference.into(),
            },
            attachments: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.basis.reference.trim().is_empty() || self.basis.reference.chars().count() > 2000 {
            bail!("decision basis requires a reference or rationale (up to 2000 characters)");
        }
        if self.attachments.len() > 20 {
            bail!("at most 20 evidence links are allowed");
        }
        for attachment in &self.attachments {
            if attachment.title.trim().is_empty()
                || attachment.title.chars().count() > 200
                || attachment.url.chars().count() > 4000
            {
                bail!("evidence link needs a title (up to 200 characters) and URL (up to 4000 characters)");
            }
            let url = url::Url::parse(&attachment.url)?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                bail!("evidence links must use HTTP or HTTPS");
            }
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
    pub resource_limit_draft: Option<Value>,
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
    /// Connected messages the submitting player could see, frozen at submission.
    pub thread: Vec<threads::ThreadMessage>,
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
    /// Question the EXCON put to the proposing player (status `NeedsClarification`).
    #[serde(default)]
    pub clarification_request: Option<String>,
    /// EXCON closed the clarification thread this proposal ends (see [`threads`]).
    #[serde(default)]
    pub clarification_closed: bool,
    /// Earlier proposal this one replaces (the player answered its clarification question).
    #[serde(default)]
    pub supersedes: Option<u64>,
    /// Newer proposal that replaced this one (status `superseded`).
    #[serde(default)]
    pub superseded_by: Option<u64>,
    pub results: Vec<String>,
    /// Injects drafted by the LLM for a EXCON prompt; sent when approved.
    pub inject_drafts: Vec<Value>,
}

/// What the LLM made of a reply, in exercise terms.
#[derive(Debug, Clone, Default)]
pub struct ParsedInput {
    pub resource_limit_draft: Option<Value>,
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
    /// Replaces the proposal's commands (the EXCON's edit).
    pub commands: Option<Vec<Command>>,
    pub note: Option<String>,
}

/// A clarification the EXCON asked of a player, to be delivered.
#[derive(Debug, Clone, Serialize)]
pub struct ClarificationRequest {
    pub proposal: u64,
    pub role: String,
    pub text_en: String,
}

/// Facts about a decision, for the feedback message to the player.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionOutcome {
    pub proposal: u64,
    /// Player to inform (`None` for EXCON proposals).
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
    /// An intermediate advance: the turn stays open when it ends.
    pub partial: bool,
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

/// An intermediate advance inside an open turn (the turn is not closed).
#[derive(Debug, Clone, Serialize)]
pub struct SubStep {
    pub from_s: i64,
    pub to_s: i64,
    pub applied: usize,
    pub events: usize,
    pub wall_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TurnWindow {
    pub turn: u32,
    pub from_s: i64,
    pub to_s: i64,
}

pub struct Exercise {
    pub(crate) turn_summaries: Vec<summaries::TurnSummary>,
    player_drafts: BTreeMap<u64, drafts::PlayerDraft>,
    pub training_design: design::TrainingDesign,
    pub current_path: Option<String>,
    pub path_history: Vec<Value>,
    intervention: Option<interventions::Intervention>,
    pub resource_limits: ResourceLimits,
    pub exercise_phase: ExercisePhase,
    pub phase_history: Vec<PhaseEntry>,
    pub clock_control: ClockControl,
    pub scheduled_injects: Vec<ScheduledInject>,
    /// Actions run (or not) for each inject delivery (see [`actions`]).
    pub action_runs: BTreeMap<String, actions::ActionRun>,
    /// MSEL injects EXCON decided not to send, with the simulated time of the
    /// decision. Skipped injects never block advancing to the next inject.
    pub skipped_injects: BTreeMap<String, i64>,
    /// Pre-actions EXCON switched off before they ran (`inject:action` → time).
    pub disabled_actions: BTreeMap<String, i64>,
    pub world: World,
    pub turn: u32,
    pub phase: Phase,
    pub turn_minutes: u32,
    /// Simulated time at which the open turn started.
    pub turn_start_s: i64,
    /// Intermediate advances made inside the open turn.
    pub substeps: Vec<SubStep>,
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
    /// MSEL inject id -> roles that received it.
    pub inject_sent_to: BTreeMap<String, Vec<String>>,
    /// Delivery id (`Message::inject_id`) -> MSEL inject id.
    pub delivery_msel: BTreeMap<u64, String>,
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
            .map(|r| {
                (
                    r.id.clone(),
                    Player {
                        role: r.id.clone(),
                        name: r.name.clone(),
                        language: r.language.clone(),
                        language_confirmed: true,
                    },
                )
            })
            .collect();
        let turn_minutes = world.scn.turn_minutes.clamp(1, 24 * 60);
        let mut ex = Exercise {
            turn_summaries: Vec::new(),
            player_drafts: BTreeMap::new(),
            training_design: Default::default(),
            current_path: None,
            path_history: Vec::new(),
            intervention: None,
            resource_limits: ResourceLimits::default(),
            exercise_phase: ExercisePhase::EarlyWarning,
            phase_history: vec![PhaseEntry {
                phase: ExercisePhase::EarlyWarning,
                turn: 0,
                t_s: world.time_s(),
                wall_ms: wall_ms(),
                reason: "Initial training phase".into(),
            }],
            clock_control: ClockControl::default(),
            scheduled_injects: Vec::new(),
            action_runs: BTreeMap::new(),
            skipped_injects: BTreeMap::new(),
            disabled_actions: BTreeMap::new(),
            world,
            turn: 0,
            phase: Phase::Briefing,
            turn_minutes,
            turn_start_s: 0,
            substeps: Vec::new(),
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
            inject_sent_to: BTreeMap::new(),
            delivery_msel: BTreeMap::new(),
            resolving: None,
            conversations: BTreeMap::new(),
            speed: 0.0,
            snapshots: Vec::new(),
            log_file: None,
            next_id: 1,
        };
        ex.record(
            "exercise_created",
            MASTER,
            json!({"scenario": ex.world.scn.id, "seed": ex.world.seed}),
        );
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
            exercise_phase: self.exercise_phase,
            phase: self.phase,
            seq: self.log.len() as u64 + 1,
            wall_ms: web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
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
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
            }
        }
    }

    pub fn role_ids(&self) -> Vec<String> {
        self.players.keys().cloned().collect()
    }

    pub fn language_of(&self, role: &str) -> String {
        self.players
            .get(role)
            .map(|p| p.language.clone())
            .unwrap_or_else(|| "en".into())
    }

    pub fn set_language(&mut self, role: &str, lang: &str) -> Result<()> {
        if ![
            "en", "it", "fr", "de", "es", "hy", "ka", "uk", "ro", "az", "ru", "el", "pt",
        ]
        .contains(&lang)
        {
            bail!("choose a supported language");
        }
        let Some(p) = self.players.get_mut(role) else {
            bail!("unknown role {role}")
        };
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
        self.turn_start_s = self.world.time_s();
        self.substeps.clear();
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
    /// `minutes` counts from now; without it the turn runs to its planned end
    /// (after any intermediate advances), or one minute if that has already
    /// been passed. Approved inject actions apply at their instants.
    pub fn begin_resolution(&mut self, minutes: Option<u32>) -> Result<()> {
        let now = self.world.time_s();
        let to_s = match minutes {
            Some(m) => now + m.clamp(1, 24 * 60) as i64 * 60,
            None => self.planned_end_s().max(now + 60),
        };
        self.begin_resolution_until(to_s, false)
    }

    /// Resolve up to the simulated instant `to_s`. With `partial` the turn
    /// stays open afterwards (an intermediate advance); otherwise it is closed.
    pub fn begin_resolution_until(
        &mut self,
        to_s: i64,
        partial: bool,
    ) -> Result<()> {
        if self.phase != Phase::Orders {
            bail!("no turn is open");
        }
        let from_s = self.world.time_s();
        if to_s <= from_s {
            bail!("the target time must be after the current time");
        }
        let to_s = to_s.min(from_s + 24 * 3600);
        let minutes = ((to_s - from_s + 59) / 60) as u32;
        self.stop_clock("Turn resolution uses its own duration and playback speed");
        let mut applied = Vec::new();
        let approved: Vec<u64> = self
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved)
            .map(|p| p.id)
            .collect();
        for pid in approved {
            applied.extend(self.execute_proposal(pid)?);
        }
        self.resolving = Some(Resolution {
            turn: self.turn,
            partial,
            minutes,
            from_s,
            to_s,
            applied,
            events: Vec::new(),
        });
        self.phase = Phase::Resolving;
        self.record(
            "turn_resolving",
            MASTER,
            json!({"turn": self.turn, "minutes": minutes, "to_s": to_s, "partial": partial}),
        );
        Ok(())
    }

    /// Advance the resolving turn by up to `step_s` simulated seconds.
    /// Returns `true` once the end of the turn has been reached.
    pub fn step_resolution(&mut self, step_s: i64) -> Result<bool> {
        let Some(to_s) = self.resolving.as_ref().map(|r| r.to_s) else {
            bail!("no turn is being resolved")
        };
        self.run_approved_actions();
        let mut dt = (to_s - self.world.time_s()).min(step_s);
        if let Some(next) = self.next_action_in() {
            dt = dt.min(next);
        }
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

    /// Ask the running resolution to end at the current simulated second. It
    /// becomes an intermediate advance: the turn stays open (even when it was
    /// closing the turn) and the next advance continues from here. Approved
    /// scripted actions planned later stay approved. Returns the stop instant.
    pub fn stop_resolution(&mut self) -> Result<i64> {
        let now = self.world.time_s();
        let Some(r) = self.resolving.as_mut() else {
            bail!("no turn is being resolved")
        };
        r.to_s = now;
        r.partial = true;
        r.minutes = ((now - r.from_s + 59) / 60).max(0) as u32;
        self.record("resolution_stopped", MASTER, json!({"turn": self.turn, "at_s": now}));
        Ok(now)
    }

    /// Close the resolved turn: log it, snapshot, open the next turn.
    pub fn finish_resolution(&mut self) -> Result<TurnReport> {
        let Some(r) = self.resolving.take() else {
            bail!("no turn is being resolved")
        };
        while self.world.time_s() < r.to_s {
            // Finishing early (e.g. a caller that did not step to the end) still completes the turn.
            self.run_approved_actions();
            let mut dt = (r.to_s - self.world.time_s()).min(STEP_S);
            if let Some(next) = self.next_action_in() {
                dt = dt.min(next);
            }
            let resource_event_count = self.world.d.resource_events.len();
            self.world.advance(dt)?;
            self.record_resource_events(resource_event_count, MASTER);
        }
        let report = TurnReport {
            turn: r.turn,
            from_s: r.from_s,
            to_s: r.to_s,
            applied: r.applied,
            events: r.events,
        };
        if r.partial {
            self.substeps.push(SubStep {
                from_s: r.from_s,
                to_s: r.to_s,
                applied: report.applied.len(),
                events: report.events.len(),
                wall_ms: wall_ms(),
            });
            self.record("turn_substep", MASTER, json!({"turn": r.turn, "from_s": r.from_s, "to_s": r.to_s, "applied": report.applied, "events": report.events, "stats": self.world.stats()}));
            self.last_turn = Some(TurnWindow {
                turn: r.turn,
                from_s: r.from_s,
                to_s: r.to_s,
            });
            self.phase = Phase::Orders;
            return Ok(report);
        }
        self.record("turn_resolved", MASTER, json!({"turn": r.turn, "minutes": r.minutes, "applied": report.applied, "events": report.events, "stats": self.world.stats()}));
        self.last_turn = Some(TurnWindow {
            turn: r.turn,
            from_s: r.from_s,
            to_s: r.to_s,
        });
        self.prepare_turn_summaries(r.turn);
        self.turn += 1;
        self.phase = Phase::Orders;
        self.turn_start_s = self.world.time_s();
        self.substeps.clear();
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
        self.player_drafts.clear();
        self.turn_summaries.retain(|s| s.turn < turn);
        self.world.restore(&snap)?;
        self.forget_actions_since_restore();
        self.snapshots.retain(|(t, _)| *t <= turn);
        let undone: Vec<u64> = self
            .proposals
            .iter()
            .filter(|p| {
                p.turn >= turn
                    && p.status != ProposalStatus::Rejected
                    && p.status != ProposalStatus::RolledBack
            })
            .map(|p| p.id)
            .collect();
        for pid in undone {
            self.transition(
                pid,
                ProposalStatus::RolledBack,
                MASTER,
                "Undone by explicit rollback",
            )?;
        }
        self.turn = turn;
        self.phase = Phase::Orders;
        self.turn_start_s = self.world.time_s();
        self.substeps.clear();
        self.last_turn = None;
        self.record("rollback", MASTER, json!({"to_turn": turn}));
        for r in self.role_ids() {
            let text = format!(
                "EXCON: the exercise has been rolled back to the start of turn {turn} ({}).",
                self.world.clock()
            );
            self.add_message(MASTER, &r, MsgKind::System, None, &text, &text, None);
        }
        Ok(())
    }

    pub fn end(&mut self) -> Result<()> {
        if self.phase == Phase::Resolving {
            bail!("a turn is being resolved; wait for it to finish");
        }
        self.stop_clock("Exercise ended");
        self.player_drafts.clear();
        self.phase = Phase::Debrief;
        self.record(
            "exercise_ended",
            MASTER,
            json!({"stats": self.world.stats()}),
        );
        Ok(())
    }

    // --------------------------------------------------------------- messages

    /// Append to a role's conversation with the AI.
    pub fn converse(&mut self, key: &str, role: &str, content: impl Into<String>) {
        let c = self.conversations.entry(key.to_string()).or_default();
        c.push(ChatTurn {
            role: role.into(),
            content: content.into(),
        });
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
    pub fn add_message(
        &mut self,
        from: &str,
        to: &str,
        kind: MsgKind,
        title: Option<&str>,
        text: &str,
        text_en: &str,
        proposal: Option<u64>,
    ) -> u64 {
        let thread = proposal
            .and_then(|pid| self.proposal(pid))
            .and_then(|p| p.inject_id.map(|id| (id, p.reply_to, vec![to.into()])));
        self.add_message_in_thread(from, to, kind, title, text, text_en, proposal, thread, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_message_in_thread(
        &mut self,
        from: &str,
        to: &str,
        kind: MsgKind,
        title: Option<&str>,
        text: &str,
        text_en: &str,
        proposal: Option<u64>,
        thread: Option<(u64, Option<u64>, Vec<String>)>,
        message_reply_to: Option<u64>,
    ) -> u64 {
        let id = self.id();
        let (inject_id, thread_reply_to, recipients) = match thread {
            Some((root, parent, recipients)) => (Some(root), parent, recipients),
            None => (None, None, vec![to.into()]),
        };
        let reply_to = message_reply_to.or(thread_reply_to);
        let m = Message {
            original: None,
            language_src: None,
            translation_failed: false,
            inject_id,
            reply_to,
            recipients,
            wall_ms: web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
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
        if self.players.contains_key(to) && kind != MsgKind::Clarification && kind != MsgKind::Reply
        {
            let what = match kind {
                MsgKind::Inject => "INJECT",
                MsgKind::Feedback => "EXCON FEEDBACK",
                MsgKind::Situation => "SITUATION UPDATE",
                MsgKind::RoleMessage => "MESSAGE",
                _ => "NOTICE",
            };
            let reference = reply_to
                .map(|id| format!(" · reply to message {id}"))
                .unwrap_or_default();
            let head = format!(
                "[{}] {what} from {}{}{}",
                self.world.clock(),
                from,
                title.map(|t| format!(" — {t}")).unwrap_or_default(),
                reference
            );
            self.converse(to, "user", format!("{head}:\n{text_en}"));
        }
        self.messages.push(m);
        id
    }

    /// Send an inject without running any of its actions (see
    /// [`Self::send_inject_with`]).
    pub fn send_inject(
        &mut self,
        inject_id: Option<&str>,
        title: &str,
        sender: &str,
        recipients: &[String],
        text_en: &str,
        translations: &BTreeMap<String, String>,
    ) -> Result<Vec<u64>> {
        self.send_inject_with(
            inject_id,
            title,
            sender,
            recipients,
            text_en,
            translations,
            &actions::SendActions::none(),
        )
    }

    /// Send an inject. `translations` maps role → text in that role's language;
    /// missing roles get the English text. `actions` says which of the
    /// inject's actions run: the MSEL inject's own ones on its first send, or
    /// the custom inject's (see [`actions`]).
    #[allow(clippy::too_many_arguments)]
    pub fn send_inject_with(
        &mut self,
        inject_id: Option<&str>,
        title: &str,
        sender: &str,
        recipients: &[String],
        text_en: &str,
        translations: &BTreeMap<String, String>,
        actions: &actions::SendActions,
    ) -> Result<Vec<u64>> {
        if self.phase == Phase::Debrief {
            bail!("the exercise has ended");
        }
        if inject_id.is_none() && actions.custom.iter().any(|a| a.offset_s < 0) {
            bail!("a custom inject has no planned time: its actions cannot come before it");
        }
        let mut to = self.recipients(recipients)?;
        // Roles that already received this MSEL inject are skipped silently.
        if let Some(id) = inject_id {
            if let Some(done) = self.inject_sent_to.get(id) {
                to.retain(|r| !done.contains(r));
            }
        }
        if to.is_empty() {
            return Ok(Vec::new());
        }
        let first = inject_id
            .is_none_or(|id| self.inject_sent_to.get(id).is_none_or(|v| v.is_empty()));
        let delivery_id = self.id();
        let mut ids = Vec::new();
        for r in &to {
            let text = translations
                .get(r)
                .cloned()
                .unwrap_or_else(|| text_en.to_string());
            let mid = self.add_message_in_thread(
                sender,
                r,
                MsgKind::Inject,
                Some(title),
                &text,
                text_en,
                None,
                Some((delivery_id, None, to.clone())),
                None,
            );
            ids.push(mid);
        }
        if let Some(id) = inject_id {
            if !self.sent_injects.iter().any(|s| s == id) {
                self.sent_injects.push(id.to_string());
            }
            self.inject_sent_to.entry(id.to_string()).or_default().extend(to.iter().cloned());
            self.delivery_msel.insert(delivery_id, id.to_string());
            // Sending a skipped inject takes it off the skipped list.
            self.skipped_injects.remove(id);
        }
        self.record("inject_sent", MASTER, json!({"inject_id": delivery_id, "msel_id": inject_id, "title": title, "sender": sender, "recipients": to, "messages": ids}));
        self.register_inject_actions(delivery_id, inject_id, actions, first);
        Ok(ids)
    }

    pub fn inbox(&self, role: &str) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| m.to == role || m.from == role)
            .collect()
    }

    // -------------------------------------------------------------- proposals

    /// A player's free-text reply: stored, and a proposal opened for parsing.
    pub fn player_reply(
        &mut self,
        role: &str,
        text: &str,
        reply_to: u64,
        evidence: DecisionEvidence,
    ) -> Result<u64> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for replies");
        }
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        if text.trim().is_empty() {
            bail!("empty reply");
        }
        evidence.validate()?;
        let parent = self.validate_action_reference(role, reply_to)?;
        let inject_id = parent.inject_id;
        let thread = self.thread_evidence(role, reply_to)?;
        let old = self.clarification_target(role, reply_to);
        let pid = self.open_chat_proposal(role, text, inject_id, Some(reply_to), Some(evidence))?;
        self.proposal_mut(pid)?.thread = thread;
        if let Some(old) = old {
            self.proposal_mut(pid)?.supersedes = Some(old);
            self.proposal_mut(old)?.superseded_by = Some(pid);
            self.transition(
                old,
                ProposalStatus::Superseded,
                role,
                &format!("Superseded by the player's answer (proposal {pid})"),
            )?;
            self.record("proposal_superseded", role, json!({"proposal": old, "superseded_by": pid}));
        }
        self.add_message_in_thread(
            role,
            MASTER,
            MsgKind::Reply,
            None,
            text,
            text,
            Some(pid),
            inject_id.map(|root| (root, Some(reply_to), vec![MASTER.into()])),
            Some(reply_to),
        );
        Ok(pid)
    }

    /// Structured player input enters the same review queue without requiring AI.
    pub fn player_actions(
        &mut self,
        role: &str,
        text: &str,
        commands: Vec<Command>,
        reply_to: Option<u64>,
    ) -> Result<u64> {
        self.validate_chat(role, MASTER, text, true)?;
        if commands.is_empty() || commands.len() > 32 {
            bail!("provide 1–32 commands");
        }
        let allowed = &self.world.scn.role(role).unwrap().allowed_actions;
        if commands
            .iter()
            .any(|c| !allowed.iter().any(|kind| kind == c.kind()))
        {
            bail!("action outside this role's authority");
        }
        let pid = if let Some(parent) = reply_to {
            self.player_reply(
                role,
                text,
                parent,
                DecisionEvidence::judgment("Structured operational decision"),
            )?
        } else {
            self.open_chat_proposal(role, text, None, None, None)?
        };
        self.attach_parse(
            pid,
            ParsedInput {
                resource_limit_draft: None,
                communication_drafts: Vec::new(),
                language: self.language_of(role),
                translation_en: text.into(),
                actions: commands
                    .iter()
                    .map(|c| serde_json::to_value(c).expect("command serialization"))
                    .collect(),
                clarification: None,
                summary: text.into(),
                inject_drafts: Vec::new(),
            },
        )?;
        Ok(pid)
    }

    /// The EXCON-questioned proposal of `role` that a reply to message `parent` answers.
    fn clarification_target(&self, role: &str, parent: u64) -> Option<u64> {
        let live = |p: &Proposal| {
            p.status == ProposalStatus::NeedsClarification
                && p.clarification_request.is_some()
                && matches!(&p.source, Source::Player(r) if r == role)
        };
        let m = self.messages.iter().find(|m| m.id == parent)?;
        if m.kind == MsgKind::Clarification {
            if let Some(p) = m.proposal.and_then(|pid| self.proposal(pid)).filter(|p| live(p)) {
                return Some(p.id);
            }
        }
        // Replying to the original inject again also answers the open question.
        self.proposals
            .iter()
            .filter(|p| live(p) && p.reply_to == Some(parent))
            .map(|p| p.id)
            .max()
    }

    fn open_chat_proposal(
        &mut self,
        role: &str,
        text: &str,
        inject_id: Option<u64>,
        reply_to: Option<u64>,
        evidence: Option<DecisionEvidence>,
    ) -> Result<u64> {
        let pid = self.id();
        let lang = self.language_of(role);
        self.proposals.push(Proposal {
            resource_limit_draft: None,
            evidence,
            communication_drafts: Vec::new(),
            communications_claimed: false,
            delivered_communications: Vec::new(),
            declaration_kind: DeclarationKind::ProposedDecision,
            history: Vec::new(),
            decision: None,
            executions: Vec::new(),
            inject_id,
            reply_to,
            thread: Vec::new(),
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
            clarification_request: None,
            clarification_closed: false,
            supersedes: None,
            superseded_by: None,
            results: Vec::new(),
            inject_drafts: Vec::new(),
        });
        self.transition(
            pid,
            ProposalStatus::Parsing,
            role,
            "Declaration recorded; awaiting interpretation",
        )?;
        self.record("proposal_declared", role, json!(self.proposal(pid)));
        Ok(pid)
    }

    /// Conversations are evidence; only separately reviewed proposals affect physics.
    pub fn chat(
        &mut self,
        from: &str,
        to: &str,
        original: &str,
        text_en: &str,
        translated: &str,
        failed: bool,
        interpret: bool,
    ) -> Result<(u64, Option<u64>)> {
        self.chat_reply(
            from, to, original, text_en, translated, failed, interpret, None,
        )
    }

    /// Record a translated chat message with an optional reference to a prior
    /// message in the same peer conversation.
    pub fn chat_reply(
        &mut self,
        from: &str,
        to: &str,
        original: &str,
        text_en: &str,
        translated: &str,
        failed: bool,
        interpret: bool,
        reply_to: Option<u64>,
    ) -> Result<(u64, Option<u64>)> {
        self.validate_chat_reply(from, to, original, interpret, reply_to)?;
        let language = self.language_of(from);
        let proposal = if interpret {
            Some(self.open_chat_proposal(from, original, None, None, None)?)
        } else {
            None
        };
        let thread = reply_to
            .and_then(|id| self.messages.iter().find(|m| m.id == id))
            .and_then(|m| m.inject_id.map(|root| (root, Some(m.id), vec![to.into()])));
        let id = self.add_message_in_thread(
            from,
            to,
            MsgKind::Chat,
            None,
            translated,
            text_en,
            proposal,
            thread,
            reply_to,
        );
        let message = self.messages.last_mut().unwrap();
        message.original = Some(original.into());
        message.language_src = Some(language);
        message.translation_failed = failed;
        message.reply_to = reply_to;
        let recorded = message.clone();
        self.record("chat_source", from, json!(recorded));
        if from != MASTER && !interpret {
            let reference = reply_to
                .map(|id| format!(" (reply to message {id})"))
                .unwrap_or_default();
            self.converse(from, "user", format!("CHAT to {to}{reference}: {text_en}"));
        }
        Ok((id, proposal))
    }

    /// Perform all chat checks needed before any AI translation request.
    pub fn validate_chat_reply(
        &self,
        from: &str,
        to: &str,
        text: &str,
        interpret: bool,
        reply_to: Option<u64>,
    ) -> Result<()> {
        self.validate_chat(from, to, text, interpret)?;
        if from != MASTER && to != MASTER {
            chat_policy::validate_player_chat(text, &self.language_of(from))?;
        }
        if let Some(id) = reply_to {
            if from == MASTER || to == MASTER {
                bail!("reply_to is available only for player-to-player chat");
            }
            let parent = self.messages.iter().find(|m| m.id == id).ok_or_else(|| {
                anyhow::anyhow!("reply_to does not identify a chat message in this conversation")
            })?;
            let same_pair = parent.kind == MsgKind::Chat
                && ((parent.from == from && parent.to == to)
                    || (parent.from == to && parent.to == from));
            let own_inject =
                parent.kind == MsgKind::Inject && parent.to == from && parent.inject_id.is_some();
            if !same_pair && !own_inject {
                bail!("reply_to does not identify a chat message in this conversation");
            }
        }
        Ok(())
    }

    pub fn validate_chat(&self, from: &str, to: &str, text: &str, interpret: bool) -> Result<()> {
        if self.phase == Phase::Debrief {
            bail!("the exercise has ended");
        }
        if from == to
            || (from != MASTER && !self.players.contains_key(from))
            || (to != MASTER && !self.players.contains_key(to))
        {
            bail!("invalid chat participants");
        }
        if text.trim().is_empty() || text.chars().count() > 8000 {
            bail!("message must contain 1–8000 characters");
        }
        if interpret && (from == MASTER || self.phase != Phase::Orders) {
            bail!("operational interpretation requires a player and an open turn");
        }
        Ok(())
    }

    /// A EXCON natural-language prompt, opened as a proposal for confirmation.
    pub fn master_prompt(&mut self, text: &str) -> u64 {
        let pid = self.id();
        self.proposals.push(Proposal {
            resource_limit_draft: None,
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
            thread: Vec::new(),
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
            clarification_request: None,
            clarification_closed: false,
            supersedes: None,
            superseded_by: None,
            results: Vec::new(),
            inject_drafts: Vec::new(),
        });
        self.transition(
            pid,
            ProposalStatus::Parsing,
            MASTER,
            "EXCON suggestion requested",
        )
        .expect("new proposal");
        self.record(
            "master_prompt",
            MASTER,
            json!({"proposal": pid, "text": text}),
        );
        pid
    }

    fn transition(
        &mut self,
        pid: u64,
        status: ProposalStatus,
        actor: &str,
        note: &str,
    ) -> Result<()> {
        let change = StatusChange {
            status,
            turn: self.turn,
            t_s: self.world.time_s(),
            wall_ms: wall_ms(),
            actor: actor.into(),
            note: note.into(),
        };
        let p = self.proposal_mut(pid)?;
        p.status = status;
        p.history.push(change.clone());
        self.record(
            "proposal_status",
            actor,
            json!({"proposal": pid, "change": change}),
        );
        Ok(())
    }

    /// Set the explicitly selected declaration type before parsing starts.
    pub fn set_declaration_kind(&mut self, pid: u64, kind: DeclarationKind) -> Result<()> {
        let p = self.proposal_mut(pid)?;
        if p.status != ProposalStatus::Parsing {
            bail!("declaration already interpreted");
        }
        p.declaration_kind = kind;
        let actor = p.source.label().to_string();
        self.record(
            "declaration_kind",
            &actor,
            json!({"proposal": pid, "kind": kind}),
        );
        Ok(())
    }

    pub fn execute_approved(&mut self, pid: u64) -> Result<Vec<AppliedAction>> {
        if self.phase != Phase::Orders {
            bail!("execution requires an open turn");
        }
        self.execute_proposal(pid)
    }

    fn execute_proposal(&mut self, pid: u64) -> Result<Vec<AppliedAction>> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?
            .clone();
        if p.status != ProposalStatus::Approved
            || p.decision.as_ref().map(|d| d.approve) != Some(true)
        {
            bail!("proposal has no effective EXCON approval");
        }
        if let Some(draft) = &p.resource_limit_draft {
            self.set_resource_limits(
                serde_json::from_value(draft.clone())?,
                p.decision
                    .as_ref()
                    .and_then(|d| d.note.as_deref())
                    .unwrap_or("Approved EXCON resource-limit recommendation"),
                Some(pid),
            )?;
        }
        let origin = if matches!(p.source, Source::Master) {
            "approved_master_suggestion"
        } else {
            "approved_player_decision"
        };
        let mut applied = Vec::new();
        let mut executions = Vec::new();
        for cmd in &p.commands {
            let allocation_count = self.world.d.allocations.len();
            let resource_event_count = self.world.d.resource_events.len();
            let mut verdicts = self.world.validate_batch(
                &self.prior(pid),
                std::slice::from_ref(cmd),
                &p.source.actor(),
            );
            self.check_limit_batch(
                pid,
                std::slice::from_ref(cmd),
                &p.source.actor(),
                &mut verdicts,
            );
            let result = if let Verdict::Rejected { reason } = &verdicts[0] {
                Err(reason.clone())
            } else {
                self.world.apply(cmd, &p.source.actor())
            };
            let (ok, detail) = match result {
                Ok(notes) => (true, notes.join("; ")),
                Err(reason) => (false, reason),
            };
            self.record_resource_events(resource_event_count, p.source.label());
            if ok && matches!(cmd, Command::AdvanceTime { .. }) {
                self.clock_control.revision += 1;
            }
            let execution = ExecutionRecord {
                allocations: self.world.d.allocations[allocation_count..]
                    .iter()
                    .map(|a| a.id)
                    .collect(),
                command: cmd.clone(),
                ok,
                detail: detail.clone(),
                t_s: self.world.time_s(),
                wall_ms: wall_ms(),
                origin: origin.into(),
            };
            self.record(
                "action_executed",
                MASTER,
                json!({"proposal": pid, "inject_id": p.inject_id, "execution": execution}),
            );
            executions.push(execution);
            applied.push(AppliedAction {
                proposal: pid,
                by: p.source.label().into(),
                action: cmd.describe(),
                ok,
                detail,
            });
        }
        let all_ok = applied.iter().all(|a| a.ok);
        let pm = self.proposal_mut(pid)?;
        pm.results = applied
            .iter()
            .map(|a| {
                format!(
                    "{}: {} ({})",
                    a.action,
                    if a.ok {
                        "executed / dispatched"
                    } else {
                        "not executed"
                    },
                    a.detail
                )
            })
            .collect();
        if p.resource_limit_draft.is_some() {
            pm.results
                .push("EXCON-approved resource limits applied".into());
        }
        pm.executions = executions;
        self.transition(
            pid,
            if all_ok {
                ProposalStatus::Applied
            } else {
                ProposalStatus::Failed
            },
            MASTER,
            "Execution outcomes recorded",
        )?;
        Ok(applied)
    }

    fn proposal_mut(&mut self, pid: u64) -> Result<&mut Proposal> {
        self.proposals
            .iter_mut()
            .find(|p| p.id == pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))
    }

    pub fn proposal(&self, pid: u64) -> Option<&Proposal> {
        self.proposals.iter().find(|p| p.id == pid)
    }

    /// Approved-but-unapplied player actions, which hold resources.
    fn prior(&self, except: u64) -> Vec<(Actor, Command)> {
        self.proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved && p.id != except)
            .flat_map(|p| {
                p.commands
                    .iter()
                    .map(move |c| (p.source.actor(), c.clone()))
            })
            .collect()
    }

    fn set_commands(&mut self, pid: u64, commands: Vec<Command>) -> Result<()> {
        let actor = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?
            .source
            .actor();
        let prior = self.prior(pid);
        let mut verdicts = self.world.validate_batch(&prior, &commands, &actor);
        self.check_limit_batch(pid, &commands, &actor, &mut verdicts);
        let p = self.proposal_mut(pid)?;
        p.descriptions = commands.iter().map(|c| c.describe()).collect();
        p.commands = commands;
        p.verdicts = verdicts;
        Ok(())
    }

    /// Attach the LLM's interpretation. Returns the clarification question
    /// (in the player's language) if the player must be asked one.
    pub fn attach_parse(&mut self, pid: u64, parsed: ParsedInput) -> Result<Option<String>> {
        if self.proposal(pid).map(|p| p.status) != Some(ProposalStatus::Parsing) {
            bail!("proposal is no longer awaiting interpretation");
        }
        let mut commands = Vec::new();
        let mut invalid = Vec::new();
        for a in &parsed.actions {
            match serde_json::from_value::<Command>(a.clone()) {
                Ok(c) => commands.push(c),
                Err(e) => invalid.push(format!("{a} ({e})")),
            }
        }
        if parsed.resource_limit_draft.is_some()
            && !matches!(self.proposal(pid).unwrap().source, Source::Master)
        {
            bail!("players cannot draft operational limits");
        }
        if let Some(draft) = &parsed.resource_limit_draft {
            if let Err(e) = serde_json::from_value::<ResourceLimits>(draft.clone())
                .map_err(anyhow::Error::from)
                .and_then(|limits| self.validate_limits_for_phase(&limits, self.exercise_phase))
            {
                invalid.push(format!("resource limits: {e}"));
            }
        }
        self.set_commands(pid, commands)?;
        let p = self.proposal_mut(pid)?;
        p.resource_limit_draft = parsed.resource_limit_draft;
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
            p.communication_drafts.push(CommunicationDraft {
                to: role.clone(),
                from: MASTER.into(),
                kind: MsgKind::Clarification,
                title: "Clarification needed".into(),
                text_en: q.clone(),
            });
        }
        let chat_only = p.inject_id.is_none()
            && matches!(p.source, Source::Player(_))
            && p.commands.is_empty()
            && p.invalid.is_empty()
            && p.communication_drafts.is_empty()
            && p.clarification.is_none();
        let status = if chat_only {
            ProposalStatus::Informational
        } else if p.commands.is_empty() && p.inject_drafts.is_empty() && p.clarification.is_some() {
            ProposalStatus::NeedsClarification
        } else {
            ProposalStatus::Pending
        };
        let (source, clar) = (p.source.clone(), p.clarification.clone());
        self.transition(
            pid,
            status,
            source.label(),
            "Interpretation stored; awaiting EXCON",
        )?;
        self.record(
            "proposal_parsed",
            source.label(),
            serde_json::to_value(self.proposal(pid))?,
        );
        Ok(clar)
    }

    /// The LLM could not parse: the proposal waits for the EXCON, who can
    /// type the commands themselves.
    pub fn parse_failed(&mut self, pid: u64, error: &str) -> Result<()> {
        if self.proposal(pid).map(|p| p.status) != Some(ProposalStatus::Parsing) {
            bail!("proposal is no longer awaiting interpretation");
        }
        let p = self.proposal_mut(pid)?;
        p.summary = format!("(AI unavailable: {error}) — enter the actions manually");
        let label = p.source.label().to_string();
        self.transition(
            pid,
            ProposalStatus::Pending,
            &label,
            "Manual interpretation required",
        )?;
        self.record(
            "proposal_parse_failed",
            &label,
            json!({"proposal": pid, "error": error}),
        );
        Ok(())
    }

    /// Deliver an approved communication once; translation may only preserve its meaning.
    pub fn deliver_approved_communication(
        &mut self,
        pid: u64,
        index: usize,
        translated: &str,
    ) -> Result<u64> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if self.phase == Phase::Debrief
            || p.status != ProposalStatus::Applied
            || p.decision.as_ref().map(|d| d.approve) != Some(true)
        {
            bail!("communication has no effective EXCON approval");
        }
        if p.delivered_communications.contains(&index) {
            bail!("communication already delivered");
        }
        let draft = p
            .communication_drafts
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("unknown communication draft"))?
            .clone();
        let mid = self.add_message(
            &draft.from,
            &draft.to,
            draft.kind,
            Some(&draft.title),
            translated,
            &draft.text_en,
            Some(pid),
        );
        let p = self.proposal_mut(pid)?;
        p.delivered_communications.push(index);
        p.results.push(format!(
            "Communication to {} delivered as message #{mid}",
            draft.to
        ));
        self.record(
            "approved_communication_delivered",
            MASTER,
            json!({"proposal": pid, "draft": index, "message": mid}),
        );
        Ok(mid)
    }

    /// Review/edit operational communications while the proposal is still pending.
    pub fn edit_communications(
        &mut self,
        pid: u64,
        communications: Option<Vec<CommunicationDraft>>,
        injects: Option<Vec<Value>>,
    ) -> Result<()> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if !matches!(
            p.status,
            ProposalStatus::Parsing | ProposalStatus::Pending | ProposalStatus::NeedsClarification
        ) {
            bail!("proposal already decided");
        }
        if let Some(drafts) = &communications {
            for d in drafts {
                if !self.players.contains_key(&d.to)
                    || !matches!(d.kind, MsgKind::RoleMessage | MsgKind::Clarification)
                    || d.text_en.trim().is_empty()
                {
                    bail!("invalid communication draft");
                }
                if d.from != MASTER && d.from != p.source.label() {
                    bail!("draft sender must match the proposal origin or EXCON");
                }
            }
        }
        if let Some(drafts) = &injects {
            if !matches!(p.source, Source::Master) && !drafts.is_empty() {
                bail!("players cannot draft scenario injects");
            }
            for d in drafts {
                if d["text"].as_str().unwrap_or("").trim().is_empty() {
                    bail!("inject draft needs text");
                }
                let to: Vec<String> = serde_json::from_value(d["recipients"].clone())?;
                self.recipients(&to)?;
            }
        }
        let before = json!({"communications": p.communication_drafts, "injects": p.inject_drafts});
        let p = self.proposal_mut(pid)?;
        if let Some(d) = communications {
            p.communication_drafts = d;
        }
        if let Some(d) = injects {
            p.inject_drafts = d;
        }
        let after = json!({"communications": p.communication_drafts, "injects": p.inject_drafts});
        self.record(
            "communications_reviewed",
            MASTER,
            json!({"proposal": pid, "before": before, "after": after}),
        );
        Ok(())
    }

    /// The EXCON asks the proposing player a question instead of deciding.
    /// The proposal stays open (`NeedsClarification`) and can still be
    /// approved or rejected later. Delivery of the question to the player is
    /// done by [`Exercise::deliver_clarification`] (after translation).
    pub fn request_clarification(&mut self, pid: u64, note: &str) -> Result<ClarificationRequest> {
        if self.phase != Phase::Orders {
            bail!("clarification requests require an open exercise turn");
        }
        let note = note.trim();
        if note.is_empty() {
            bail!("a clarification question is required");
        }
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        let Source::Player(role) = p.source.clone() else {
            bail!("only a player proposal can be sent back for clarification");
        };
        if !matches!(p.status, ProposalStatus::Pending | ProposalStatus::NeedsClarification) {
            bail!("proposal {pid} is already {:?}", p.status);
        }
        let p = self.proposal_mut(pid)?;
        p.clarification_request = Some(note.to_string());
        p.clarification_closed = false;
        self.transition(
            pid,
            ProposalStatus::NeedsClarification,
            MASTER,
            &format!("Clarification requested: {note}"),
        )?;
        self.record(
            "clarification_requested",
            MASTER,
            json!({"proposal": pid, "role": role, "note": note}),
        );
        Ok(ClarificationRequest { proposal: pid, role, text_en: note.to_string() })
    }

    /// Put the EXCON's question into the player's thread.
    pub fn deliver_clarification(&mut self, pid: u64, text: &str, text_en: &str) -> Result<u64> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        let Source::Player(role) = p.source.clone() else {
            bail!("only a player proposal can be clarified");
        };
        if p.status != ProposalStatus::NeedsClarification {
            bail!("proposal {pid} no longer awaits clarification");
        }
        let id = self.add_message(
            MASTER,
            &role,
            MsgKind::Clarification,
            Some("Clarification requested"),
            text,
            text_en,
            Some(pid),
        );
        // The AI conversation must know what the EXCON asked.
        self.converse(
            &role,
            "user",
            format!("[{}] EXCON CLARIFICATION REQUEST:\n{text_en}", self.world.clock()),
        );
        Ok(id)
    }

    /// Re-run feasibility on an undecided proposal, optionally with edited commands,
    /// without deciding or changing its status.
    pub fn revalidate_proposal(&mut self, pid: u64, commands: Option<Vec<Command>>) -> Result<()> {
        if self.phase != Phase::Orders {
            bail!("revalidation requires an open exercise turn");
        }
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if !matches!(p.status, ProposalStatus::Pending | ProposalStatus::NeedsClarification) {
            bail!("proposal {pid} is already {:?}", p.status);
        }
        let cmds = commands.unwrap_or_else(|| p.commands.clone());
        let actor = p.source.label().to_string();
        self.set_commands(pid, cmds)?;
        self.record(
            "proposal_revalidated",
            MASTER,
            json!({"proposal": pid, "source": actor, "commands": self.proposal(pid).map(|p| p.commands.clone()), "verdicts": self.proposal(pid).map(|p| p.verdicts.clone())}),
        );
        Ok(())
    }

    /// The EXCON decides on a proposal.
    pub fn decide(&mut self, pid: u64, d: Decision) -> Result<DecisionOutcome> {
        if self.phase != Phase::Orders {
            bail!("decisions require an open exercise turn");
        }
        let status = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?
            .status;
        if !matches!(
            status,
            ProposalStatus::Pending | ProposalStatus::NeedsClarification | ProposalStatus::Parsing
        ) {
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
        if d.approve
            && matches!(p.source, Source::Master)
            && p.summary.contains("UNPLANNED DIVERGENCE")
            && note.is_none()
        {
            bail!("unplanned training divergence requires explicit EXCON rationale");
        }
        if d.approve {
            self.edit_communications(
                pid,
                Some(p.communication_drafts.clone()),
                Some(p.inject_drafts.clone()),
            )?;
            if p.commands.is_empty()
                && p.inject_drafts.is_empty()
                && p.communication_drafts.is_empty()
                && p.resource_limit_draft.is_none()
            {
                bail!("nothing to approve: proposal {pid} has no actions (edit it to add some, or reject it)");
            }
            if p.verdicts.iter().any(|v| !v.is_ok()) {
                bail!("proposal is not feasible; edit its actions or resources before approval");
            }
            if d.commands.is_none() && !p.invalid.is_empty() {
                bail!("proposal contains invalid actions; edit it before approval");
            }
        }
        let effective_limits = if d.approve {
            p.resource_limit_draft
                .as_ref()
                .map(|v| serde_json::from_value::<ResourceLimits>(v.clone()))
                .transpose()?
        } else {
            None
        };
        if d.approve {
            if let Some(limits) = &effective_limits {
                self.validate_limits_for_phase(limits, self.exercise_phase)?;
            }
        }
        self.proposal_mut(pid)?.decision = Some(ApprovalRecord {
            resource_limits: effective_limits,
            approve: d.approve,
            actor: MASTER.into(),
            turn: self.turn,
            t_s: self.world.time_s(),
            wall_ms: wall_ms(),
            note: note.clone(),
            edited: d.commands.is_some() && p.commands != proposed_commands,
            proposed_commands,
            effective_commands: p.commands.clone(),
        });
        let mut facts = Vec::new();
        if !d.approve {
            self.transition(
                pid,
                ProposalStatus::Rejected,
                MASTER,
                note.as_deref().unwrap_or("Rejected by EXCON"),
            )?;
            facts.push(format!(
                "Your orders were NOT approved by EXCON: {}",
                if p.descriptions.is_empty() {
                    p.summary.clone()
                } else {
                    p.descriptions.join("; ")
                }
            ));
        } else {
            self.transition(
                pid,
                ProposalStatus::Approved,
                MASTER,
                note.as_deref().unwrap_or("Approved by EXCON"),
            )?;
            if matches!(p.source, Source::Master) || p.commands.is_empty() {
                self.execute_proposal(pid)?;
                facts.extend(self.proposal(pid).unwrap().results.clone());
                if p.commands.is_empty()
                    && (!p.communication_drafts.is_empty() || !p.inject_drafts.is_empty())
                {
                    facts.push(
                        "APPROVED: the reviewed communication will be delivered by EXCON.".into(),
                    );
                }
            } else {
                for (desc, v) in p.descriptions.iter().zip(&p.verdicts) {
                    let notes = match v {
                        Verdict::Ok { notes } => notes.join("; "),
                        _ => String::new(),
                    };
                    facts.push(format!(
                        "APPROVED: {desc}{}. It awaits EXCON execution or turn resolution.",
                        if notes.is_empty() {
                            String::new()
                        } else {
                            format!(" ({notes})")
                        }
                    ));
                }
            }
        }
        if let Some(n) = &note {
            facts.push(format!("Note from EXCON: {n}"));
        }
        self.proposal_mut(pid)?.master_note = note;
        let facts_en = facts.join("\n");
        self.record("proposal_decided", MASTER, json!({"proposal": pid, "inject_id": p.inject_id, "decision": self.proposal(pid).unwrap().decision, "facts": facts_en}));
        Ok(DecisionOutcome {
            proposal: pid,
            notify,
            facts_en,
        })
    }

    /// A command from the EXCON GUI or a script: validated as EXCON and applied now.
    pub fn direct_command(&mut self, cmd: &Command) -> Result<Vec<String>> {
        let before = if interventions::physical(cmd) {
            Some(self.world.snapshot())
        } else {
            None
        };
        let resource_event_count = self.world.d.resource_events.len();
        let verdict = self
            .world
            .validate_batch(&self.prior(0), std::slice::from_ref(cmd), &Actor::Master)
            .remove(0);
        let r = if let Verdict::Rejected { reason } = verdict {
            Err(reason)
        } else {
            self.world.apply(cmd, &Actor::Master)
        };
        self.record_resource_events(resource_event_count, MASTER);
        if r.is_ok() && matches!(cmd, Command::AdvanceTime { .. }) {
            self.clock_control.revision += 1;
        }
        self.record("master_command", MASTER, json!({"origin": "direct_excon_intervention", "command": cmd, "result": match &r { Ok(n) => json!({"ok": n}), Err(e) => json!({"error": e}) }}));
        if r.is_ok() {
            if let Some(snapshot) = before {
                self.intervention = Some(interventions::Intervention {
                    snapshot,
                    generation: self.generation,
                    log_seq: self.log.last().unwrap().seq,
                    command: cmd.clone(),
                });
            }
        }
        r.map_err(|e| anyhow::anyhow!(e))
    }

    /// Separate unmet declarations, approved reservations and actual deployments.
    pub fn resource_accounting(&self, only_role: Option<&str>) -> Value {
        let mut roles = serde_json::Map::new();
        for (role, pools) in &self.world.d.pools {
            if only_role.is_some_and(|r| r != role) {
                continue;
            }
            let mut resources = serde_json::Map::new();
            for (resource, pool) in pools {
                let mut requested = 0u64;
                let mut committed = 0u64;
                for p in self.proposals.iter().filter(|p| {
                    p.source.label() == role
                        && matches!(
                            p.status,
                            ProposalStatus::Parsing
                                | ProposalStatus::Pending
                                | ProposalStatus::NeedsClarification
                                | ProposalStatus::Approved
                        )
                }) {
                    let qty: u64 = p
                        .commands
                        .iter()
                        .map(|c| {
                            self.world
                                .resource_requirements(c, &p.source.actor())
                                .get(resource)
                                .copied()
                                .unwrap_or(0) as u64
                        })
                        .sum();
                    if p.status == ProposalStatus::Approved {
                        committed += qty;
                    } else {
                        requested += qty;
                    }
                }
                let returned: u64 = self
                    .world
                    .d
                    .allocations
                    .iter()
                    .filter(|a| &a.role == role && &a.resource == resource)
                    .map(|a| a.returned as u64)
                    .sum();
                let external_requested: u64 = self
                    .world
                    .d
                    .arrivals
                    .iter()
                    .filter(|a| {
                        &a.role == role && &a.resource == resource && a.at_s > self.world.time_s()
                    })
                    .map(|a| a.qty as u64)
                    .sum();
                let effective_capacity =
                    self.effective_capacity(resource, self.exercise_phase, &self.resource_limits);
                let shared_used = self
                    .resource_usage_with_commitments(0)
                    .get(resource)
                    .copied()
                    .unwrap_or(0);
                let deployable = (pool.available() as u64)
                    .saturating_sub(committed)
                    .min(effective_capacity.saturating_sub(shared_used));
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
            "inject_actions":self.inject_action_runs(),
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
            "clarification_threads": self.clarification_threads(),
            "log": self.log,
        })
    }
}
