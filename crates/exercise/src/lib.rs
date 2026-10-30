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
pub mod action_suggestions;
pub mod alert_brief;
pub mod files;
pub use files::{FileRef, PdfUpload};
pub mod evaluation;
pub mod interventions;
pub mod knowledge;
mod monitoring;
pub mod conversations;
pub mod narrative;
pub mod persona;
pub mod response;
pub mod reply_review;
pub use response::{Addressee, ResponseKind};
pub mod summaries;
pub mod threads;
pub mod planned;
pub mod agenda;
pub mod actions;
pub mod queue;
pub mod situation;
pub mod suggestions;
pub mod progress;
pub use progress::Progress;

/// Minutes an evacuation takes (see `sim::EVAC_MIN`); the lead time of warnings.
pub const EVAC_LEAD_MIN: i64 = sim::EVAC_MIN;
pub use knowledge::{BuildingObs, FireCell, KnowledgeEvent, KnowledgeSource, MapObservation, WarningObs};

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
    /// Copy for the EXCON console, in `excon_language` (see [`Exercise::excon_pending`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_excon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excon_lang: Option<String>,
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
    /// Who exercise control spoke as, when EXCON wrote this (see [`persona`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
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
    /// EXCON took note of a response that carried nothing to execute (no
    /// actions, communications or drafts). Terminal: the EXCON note is the answer.
    Acknowledged,
}

/// What the EXCON made of a proposal, as players and the EXCON lists show it.
/// It is orthogonal to [`ProposalStatus`], which also tracks execution: an
/// `Applied` proposal can still be `PartiallyApproved`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    /// Not decided yet (being interpreted or awaiting the EXCON).
    #[default]
    InReview,
    /// A question to the player is open (asked by EXCON or by the AI).
    ClarificationRequested,
    Approved,
    /// Some actions approved, the others rejected (see [`ApprovalRecord::rejected_actions`]).
    PartiallyApproved,
    Rejected,
    /// Nothing to execute: the EXCON took note (see [`ProposalStatus::Acknowledged`]).
    Acknowledged,
    /// Chat without operational content; nothing to decide.
    Informational,
    /// Replaced by a newer reply before any decision.
    Superseded,
    /// Undone by a rollback before any decision.
    Withdrawn,
}

/// The EXCON rejects one action of a proposal it otherwise approves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRejection {
    /// Index into the reviewed actions (the EXCON's edit, else the proposal's commands).
    pub index: usize,
    /// Reason for the player (optional).
    #[serde(default, alias = "reason")]
    pub note: Option<String>,
}

/// One action the EXCON did not approve, kept with the decision.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RejectedAction {
    /// Position in the reviewed actions.
    pub index: usize,
    pub command: Command,
    pub description: String,
    pub note: Option<String>,
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
    /// Persona the decision note and feedback are written as.
    pub persona: Option<String>,
    pub edited: bool,
    pub proposed_commands: Vec<Command>,
    /// The approved actions (what executes); empty for a rejection.
    pub effective_commands: Vec<Command>,
    /// Actions the EXCON did not approve, with their reasons.
    pub rejected_actions: Vec<RejectedAction>,
    /// Approved, partially approved, rejected or acknowledged.
    pub review: Review,
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
    /// Laws and regulations.
    Law,
    OperationalJudgment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionBasis {
    pub kind: BasisKind,
    /// Procedure/plan reference or the player's operational rationale.
    pub reference: String,
    /// Why the player took this decision, in their own words (a reply to
    /// the inject's sender requires it; older sessions have none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
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
                rationale: None,
            },
            attachments: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.basis.reference.trim().is_empty() || self.basis.reference.chars().count() > 2000 {
            bail!("decision basis requires a reference or rationale (up to 2000 characters)");
        }
        if self.basis.rationale.as_ref().is_some_and(|r| r.chars().count() > 2000) {
            bail!("the rationale is limited to 2000 characters");
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

impl Proposal {
    /// A narrative initiative with no simulator actions: never executed.
    pub fn pure_narrative(&self) -> bool {
        self.narrative && !self.carries_actions
    }

    /// The EXCON's verdict when decided, else where the proposal stands.
    pub fn review_state(&self) -> Review {
        if let Some(d) = &self.decision {
            return d.review;
        }
        match self.status {
            ProposalStatus::NeedsClarification => Review::ClarificationRequested,
            ProposalStatus::Informational => Review::Informational,
            ProposalStatus::Superseded => Review::Superseded,
            ProposalStatus::RolledBack => Review::Withdrawn,
            _ => Review::InReview,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    /// Explicit EXCON-adjudicated initiative with no simulated commands.
    pub narrative: bool,
    /// The response also declared simulator actions (`narrative` + `commands`).
    /// A narrative without this flag is a pure narrative initiative.
    pub carries_actions: bool,
    /// The EXCON added simulator actions to a narrative-only response while
    /// reviewing it (e.g. from AI suggestions); `carries_actions` follows them.
    pub excon_actions: bool,
    /// Linked conversation (see [`conversations`]) the response came out of.
    /// It is the only way a player who never received the inject can answer it.
    pub via_conversation: Option<u64>,
    /// A plain communication (see [`response::ResponseKind`]): stored in the
    /// inject's history, never reviewed or executed (status `Informational`).
    pub communication: bool,
    /// Who the response was addressed to: the fictitious sender EXCON plays,
    /// or real players (delivered through a linked conversation).
    pub addressed_to: response::Addressee,
    pub narrative_outcomes: Vec<narrative::NarrativeOutcome>,
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
    /// Derived from `status` and `decision` (see [`Proposal::review_state`]); kept current by every transition.
    pub review: Review,
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
    /// Who EXCON speaks as in the note and the feedback (see [`persona`]).
    pub persona: Option<String>,
    /// With `approve`: actions not approved (partial approval). With a
    /// rejection: optional per-action reasons (every action is rejected).
    pub rejected_actions: Vec<ActionRejection>,
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
    /// Persona the feedback is delivered as.
    pub persona: Option<String>,
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
    /// Player conversations about an inject (see [`conversations`]).
    pub linked_conversations: Vec<conversations::LinkedConversation>,
    pub training_design: design::TrainingDesign,
    pub current_path: Option<String>,
    pub path_history: Vec<Value>,
    intervention: Option<interventions::Intervention>,
    pub resource_limits: ResourceLimits,
    pub exercise_phase: ExercisePhase,
    pub phase_history: Vec<PhaseEntry>,
    pub clock_control: ClockControl,
    /// Language of the EXCON console (players have their own in `Player`).
    pub excon_language: String,
    pub scheduled_injects: Vec<ScheduledInject>,
    /// Actions run (or not) for each inject delivery (see [`actions`]).
    pub action_runs: BTreeMap<String, actions::ActionRun>,
    /// MSEL injects EXCON decided not to send, with the simulated time of the
    /// decision. Skipped injects never block advancing to the next inject.
    pub skipped_injects: BTreeMap<String, i64>,
    /// Pre-actions EXCON switched off before they ran (`inject:action` → time).
    pub disabled_actions: BTreeMap<String, i64>,
    /// Situation suggestions the EXCON dismissed, sent or planned (id -> mark).
    pub suggestion_marks: BTreeMap<String, suggestions::SuggestionMark>,
    /// EXCON review of player replies: AI action suggestions and questions (see [`reply_review`]).
    pub review: reply_review::Board,
    /// Bumped whenever the world is restored (the fire is reseeded), so a preview made before is not reused.
    pub(crate) restore_epoch: u32,
    pub world: World,
    pub turn: u32,
    pub phase: Phase,
    pub turn_minutes: u32,
    /// Simulated time at which the open turn started.
    pub turn_start_s: i64,
    /// Intermediate advances made inside the open turn.
    pub substeps: Vec<SubStep>,
    /// Wall-clock instant (ms) of the first start; kept across rollbacks (see [`progress`]).
    pub started_wall_ms: Option<u128>,
    /// Wall-clock instant the exercise ended.
    pub ended_wall_ms: Option<u128>,
    /// Wall-clock instant the open turn began.
    pub turn_wall_ms: Option<u128>,
    /// While a saved session is replayed: the instant the request being
    /// replayed was originally made, used instead of the wall clock for the
    /// instants above.
    pub replay_wall_ms: Option<u128>,
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
    /// PDF documents attached to player responses (see [`files`]).
    pub(crate) files: files::FileStore,
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
            linked_conversations: Vec::new(),
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
            excon_language: "en".into(),
            scheduled_injects: Vec::new(),
            action_runs: BTreeMap::new(),
            skipped_injects: BTreeMap::new(),
            disabled_actions: BTreeMap::new(),
            suggestion_marks: BTreeMap::new(),
            review: Default::default(),
            restore_epoch: 0,
            world,
            turn: 0,
            phase: Phase::Briefing,
            turn_minutes,
            turn_start_s: 0,
            substeps: Vec::new(),
            started_wall_ms: None,
            ended_wall_ms: None,
            turn_wall_ms: None,
            replay_wall_ms: None,
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
            files: Default::default(),
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

    /// Every participant confirms a language before the exercise starts: the
    /// scenario's languages become proposals (multi-device server). Without
    /// this, as in the standalone app, the scenario's languages hold.
    pub fn with_language_choice(mut self) -> Exercise {
        for p in self.players.values_mut() {
            p.language_confirmed = false;
        }
        self
    }

    /// Roles whose participant has not confirmed a language yet.
    pub fn pending_languages(&self) -> Vec<String> {
        self.players
            .values()
            .filter(|p| !p.language_confirmed)
            .map(|p| p.role.clone())
            .collect()
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

    /// The LLM gazetteer, with the map names in the players' languages as aliases.
    pub fn gazetteer(&self) -> Value {
        let mut languages: Vec<String> = self.players.values().map(|p| p.language.clone()).collect();
        languages.sort();
        languages.dedup();
        self.world.gazetteer_for(&languages)
    }

    pub fn language_of(&self, role: &str) -> String {
        if role == MASTER {
            return self.excon_language.clone();
        }
        self.players
            .get(role)
            .map(|p| p.language.clone())
            .unwrap_or_else(|| "en".into())
    }

    /// Player messages to the EXCON that still lack a copy in the EXCON
    /// language: `(id, source text, source language)`. The source is the
    /// player's original, so the translation never goes through English twice.
    pub fn excon_pending(&self) -> Vec<(u64, String, String)> {
        self.messages
            .iter()
            .filter(|m| {
                m.to == MASTER
                    && m.from != MASTER
                    && matches!(m.kind, MsgKind::Reply | MsgKind::Clarification)
                    && m.excon_lang.as_deref() != Some(self.excon_language.as_str())
            })
            .map(|m| match (&m.original, &m.language_src) {
                (Some(o), Some(l)) => (m.id, o.clone(), l.clone()),
                _ => (m.id, m.text_en.clone(), "en".into()),
            })
            .collect()
    }

    pub fn set_excon_text(&mut self, id: u64, lang: &str, text: String) {
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
            m.text_excon = Some(text);
            m.excon_lang = Some(lang.into());
        }
    }

    pub fn set_language(&mut self, role: &str, lang: &str) -> Result<()> {
        if ![
            "en", "it", "fr", "de", "es", "hy", "ka", "uk", "ro", "az", "ru", "el", "pt",
        ]
        .contains(&lang)
        {
            bail!("choose a supported language");
        }
        if role == MASTER {
            self.excon_language = lang.to_string();
            self.record("language_set", role, json!({"language": lang}));
            return Ok(());
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
        let now = self.now_wall_ms();
        self.started_wall_ms.get_or_insert(now);
        self.turn_wall_ms = Some(now);
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
        // EXCON actions queued during the pause (and inject actions due now)
        // set the scene first; then the approved proposals.
        self.run_approved_actions();
        let mut applied = Vec::new();
        let approved: Vec<u64> = self
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved && !p.pure_narrative())
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
            self.absorb_monitoring();
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
            self.absorb_monitoring();
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
        self.turn_wall_ms = Some(self.now_wall_ms());
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
        self.rollback_narrative_outcomes(turn);
        self.rollback_reviews(turn);
        self.turn_summaries.retain(|s| s.turn < turn);
        self.world.restore(&snap)?;
        self.restore_epoch += 1;
        self.forget_actions_since_restore();
        self.sync_review_queue();
        self.forget_monitoring_since_restore();
        self.snapshots.retain(|(t, _)| *t <= turn);
        let undone: Vec<u64> = self
            .proposals
            .iter()
            .filter(|p| {
                p.turn >= turn
                    && p.status != ProposalStatus::Rejected
                    && p.status != ProposalStatus::Acknowledged
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
        self.turn_wall_ms = Some(self.now_wall_ms());
        self.substeps.clear();
        self.last_turn = None;
        self.record("rollback", MASTER, json!({"to_turn": turn}));
        for r in self.role_ids() {
            let text = format!(
                "The exercise has been rolled back to the start of turn {turn} ({}).",
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
        self.phase = Phase::Debrief;
        self.ended_wall_ms.get_or_insert(self.now_wall_ms());
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
            text_excon: None,
            excon_lang: None,
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
            persona: None,
        };
        self.record("message", from, json!(&m));
        // What a player receives becomes context of their conversation with the AI.
        // (Clarifications are the AI's own words: they are already in it.)
        if self.players.contains_key(to) && kind != MsgKind::Clarification && kind != MsgKind::Reply
        {
            let what = match kind {
                MsgKind::Inject => "INJECT",
                MsgKind::Feedback => "FEEDBACK",
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
        for a in &actions.custom {
            self.world.scn.check_action(a)?;
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
        self.register_inject_actions(delivery_id, inject_id, actions, first, sender, &to);
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
        if self.clarification_target(role, reply_to).is_some() {
            bail!("this answers an open question: send it as a text answer, or create a new action without replying to the question");
        }
        let pid = self.open_chat_proposal(role, text, inject_id, Some(reply_to), Some(evidence))?;
        self.proposal_mut(pid)?.thread = thread;
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
        basis: BasisKind,
    ) -> Result<u64> {
        self.validate_chat(role, MASTER, text, true)?;
        if commands.is_empty() || commands.len() > 32 {
            bail!("provide 1–32 commands");
        }
        self.player_response(
            role,
            response::PlayerResponse {
                text: text.into(),
                commands,
                reply_to,
                kind: response::ResponseKind::Decision,
                basis: Some(basis),
                ..Default::default()
            },
        )
    }

    /// The EXCON-questioned proposal of `role` that a reply to message `parent`
    /// answers: `parent` must be EXCON's question about a proposal still waiting for it.
    pub fn clarification_target(&self, role: &str, parent: u64) -> Option<u64> {
        let m = self.messages.iter().find(|m| m.id == parent)?;
        if m.kind != MsgKind::Clarification || m.to != role {
            return None;
        }
        m.proposal
            .and_then(|pid| self.proposal(pid))
            .filter(|p| {
                p.status == ProposalStatus::NeedsClarification
                    && p.clarification_request.is_some()
                    && !p.clarification_closed
                    && matches!(&p.source, Source::Player(r) if r == role)
            })
            .map(|p| p.id)
    }

    /// A player's text answer to an EXCON question. It is a plain message to the
    /// EXCON, never interpreted: the questioned proposal keeps its commands and
    /// returns to `Pending` for the EXCON to approve, edit, reject or question again.
    /// `text_en` is the English working copy (the translation of `text`).
    pub fn answer_clarification(
        &mut self,
        role: &str,
        text: &str,
        reply_to: u64,
        text_en: &str,
        translation_failed: bool,
    ) -> Result<(u64, u64)> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for answers");
        }
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        if text.trim().is_empty() {
            bail!("empty answer");
        }
        let parent = self.validate_action_reference(role, reply_to)?;
        let inject_id = parent.inject_id;
        let Some(pid) = self.clarification_target(role, reply_to) else {
            bail!("there is no open question to answer");
        };
        let language = self.language_of(role);
        let id = self.add_message_in_thread(
            role,
            MASTER,
            MsgKind::Reply,
            None,
            text,
            text_en,
            Some(pid),
            inject_id.map(|root| (root, Some(reply_to), vec![MASTER.into()])),
            Some(reply_to),
        );
        let message = self.messages.last_mut().expect("recorded answer");
        message.original = Some(text.into());
        message.language_src = Some(language);
        message.translation_failed = translation_failed;
        let recorded = message.clone();
        self.record("reply_source", role, json!(recorded));
        self.transition(
            pid,
            ProposalStatus::Pending,
            role,
            "Player answered the EXCON question; awaiting EXCON",
        )?;
        self.record(
            "clarification_answered",
            role,
            json!({"proposal": pid, "message": id, "reply_to": reply_to}),
        );
        self.converse(
            role,
            "user",
            format!("[{}] PLAYER ANSWER to the clarification:\n{text_en}", self.world.clock()),
        );
        Ok((id, pid))
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
            narrative: false,
            carries_actions: false,
            excon_actions: false,
            via_conversation: None,
            communication: false,
            addressed_to: Default::default(),
            narrative_outcomes: Vec::new(),
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
            review: Review::InReview,
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
            narrative: false,
            carries_actions: false,
            excon_actions: false,
            via_conversation: None,
            communication: false,
            addressed_to: Default::default(),
            narrative_outcomes: Vec::new(),
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
            review: Review::InReview,
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
        p.review = p.review_state();
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

    /// `commands` as an order of `role`, approved and executed now: the same
    /// path as a player reply that EXCON interprets, approves and executes
    /// (visual fixtures, the simulator bench). Returns the proposal and its
    /// per-action results.
    pub fn order_as_role(
        &mut self,
        role: &str,
        commands: Vec<Value>,
        note: &str,
    ) -> Result<(u64, Vec<AppliedAction>)> {
        let message = self.send_inject(
            None,
            note,
            "EXCON",
            &[role.into()],
            note,
            &BTreeMap::new(),
        )?[0];
        let proposal = self.player_reply(
            role,
            note,
            message,
            DecisionEvidence::judgment(note),
        )?;
        self.attach_parse(
            proposal,
            ParsedInput {
                resource_limit_draft: None,
                communication_drafts: vec![],
                language: "en".into(),
                translation_en: note.into(),
                actions: commands,
                clarification: None,
                summary: note.into(),
                inject_drafts: vec![],
            },
        )?;
        // An infeasible action would block the approval with a generic
        // message: name the actions and their reasons instead.
        let refused: Vec<String> = self
            .proposal(proposal)
            .map(|p| {
                p.commands
                    .iter()
                    .zip(&p.verdicts)
                    .filter_map(|(c, v)| match v {
                        Verdict::Rejected { reason } => Some(format!("{}: {reason}", c.describe())),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !refused.is_empty() {
            // Closed, so it holds no resources against later orders.
            let _ = self.decide(proposal, Decision::default());
            bail!("{}", refused.join("; "));
        }
        self.decide(
            proposal,
            Decision {
                approve: true,
                ..Default::default()
            },
        )?;
        Ok((proposal, self.execute_approved(proposal)?))
    }

    fn execute_proposal(&mut self, pid: u64) -> Result<Vec<AppliedAction>> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?
            .clone();
        if p.pure_narrative() {
            bail!("narrative initiatives require an EXCON narrative outcome, not physical execution");
        }
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
            self.absorb_monitoring();
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
        // Only the EXCON review (revalidate/decide) reaches a narrative proposal
        // here: the actions it adds turn the initiative into a response that
        // carries actions; removing all of them again restores it.
        if let Some((pure, excon)) = self.proposal(pid).filter(|p| p.narrative).map(|p| (p.pure_narrative(), p.excon_actions)) {
            let added = !commands.is_empty();
            if pure && added {
                let p = self.proposal_mut(pid)?;
                p.carries_actions = true;
                p.excon_actions = true;
                self.record("narrative_actions_added", MASTER, json!({"proposal": pid, "commands": commands}));
            } else if excon && !added {
                let p = self.proposal_mut(pid)?;
                p.carries_actions = false;
                p.excon_actions = false;
            }
        }
        let actor = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?
            .source
            .actor();
        let prior = self.prior(pid);
        // A place of another kind than the action expects (a building given
        // as a zone) is adapted when unambiguous; the verdict says so.
        let (commands, adapted): (Vec<Command>, Vec<Option<String>>) =
            commands.iter().map(|c| self.world.reconcile_command(c)).unzip();
        let mut verdicts = self.world.validate_batch(&prior, &commands, &actor);
        self.check_limit_batch(pid, &commands, &actor, &mut verdicts);
        for (v, note) in verdicts.iter_mut().zip(adapted) {
            if let (Verdict::Ok { notes }, Some(n)) = (v, note) {
                notes.insert(0, n);
            }
        }
        let p = self.proposal_mut(pid)?;
        p.descriptions = commands.iter().map(|c| c.describe()).collect();
        p.commands = commands;
        p.verdicts = verdicts;
        Ok(())
    }

    /// Attach the LLM's interpretation. Returns the clarification question
    /// (in the player's language) if the player must be asked one.
    pub fn attach_parse(&mut self, pid: u64, parsed: ParsedInput) -> Result<Option<String>> {
        if self.proposal(pid).is_some_and(|p| p.narrative) {
            bail!("narrative initiatives are not interpreted as commands");
        }
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
        if p.narrative && (communications.as_ref().is_some_and(|v| !v.is_empty()) || injects.as_ref().is_some_and(|v| !v.is_empty())) {
            bail!("narrative initiatives cannot contain executable communication or inject drafts");
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
        self.deliver_clarification_as(pid, text, text_en, None)
    }

    /// Same, spoken as `persona` (default: the inject's sender, then the scenario default).
    pub fn deliver_clarification_as(
        &mut self,
        pid: u64,
        text: &str,
        text_en: &str,
        persona: Option<&str>,
    ) -> Result<u64> {
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
        let inject = self.proposal(pid).and_then(|p| p.inject_id);
        let persona = self.persona_for(persona, inject);
        let who = self.persona_name(persona.as_deref(), "en");
        self.set_message_persona(id, persona);
        // The AI conversation must know what was asked.
        self.converse(
            &role,
            "user",
            format!("[{}] CLARIFICATION REQUEST from {who}:\n{text_en}", self.world.clock()),
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
    ///
    /// * `approve` with no [`Decision::rejected_actions`]: every action is approved.
    /// * `approve` with some rejected actions: **partial approval**. Only the
    ///   other actions are kept (and executed); the rejected ones are stored
    ///   in [`ApprovalRecord::rejected_actions`] with their reasons, and the
    ///   player is told which is which. Rejecting every action of a proposal
    ///   that carries nothing else is a rejection.
    /// * `approve` on a proposal with nothing to execute (no actions,
    ///   communications, drafts or limits, not a narrative initiative): the
    ///   EXCON **acknowledges** it ([`ProposalStatus::Acknowledged`]).
    /// * no `approve`: the whole proposal is rejected; per-action reasons, if
    ///   any, are kept and reported.
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
        // The actions the EXCON reviewed: its edit, else the interpretation.
        let reviewed = d.commands.clone().unwrap_or_else(|| proposed_commands.clone());
        let clean = |n: &Option<String>| n.as_ref().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        let mut reasons: BTreeMap<usize, Option<String>> = BTreeMap::new();
        for r in &d.rejected_actions {
            if r.index >= reviewed.len() {
                bail!(
                    "action {} does not exist: proposal {pid} has {} action(s)",
                    r.index + 1,
                    reviewed.len()
                );
            }
            if reasons.insert(r.index, clean(&r.note)).is_some() {
                bail!("action {} is rejected twice", r.index + 1);
            }
        }
        let rejected_action = |i: usize, c: &Command, note: Option<String>| RejectedAction {
            index: i,
            command: c.clone(),
            description: c.describe(),
            note,
        };
        let rejected: Vec<RejectedAction> = reviewed
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match reasons.get(&i) {
                Some(note) => Some(rejected_action(i, c, note.clone())),
                // A whole rejection rejects every action, with or without a reason.
                None if !d.approve => Some(rejected_action(i, c, None)),
                None => None,
            })
            .collect();
        let kept: Vec<Command> = reviewed
            .iter()
            .enumerate()
            .filter(|(i, _)| !reasons.contains_key(i))
            .map(|(_, c)| c.clone())
            .collect();
        let before = self.proposal(pid).unwrap();
        let carries_other = before.narrative
            || !before.inject_drafts.is_empty()
            || !before.communication_drafts.is_empty()
            || before.resource_limit_draft.is_some();
        // Rejecting every action of a proposal with nothing else left is a rejection.
        let approve = d.approve && (!kept.is_empty() || carries_other || rejected.is_empty());
        let acknowledge = approve && kept.is_empty() && rejected.is_empty() && !carries_other;
        let partial = approve && !rejected.is_empty();
        // Re-check feasibility now (resources may have changed since parsing),
        // only for what is approved.
        self.set_commands(pid, if approve { kept.clone() } else { reviewed.clone() })?;
        let p = self.proposal(pid).unwrap().clone();
        let notify = match &p.source {
            Source::Player(r) => Some(r.clone()),
            Source::Master => None,
        };
        let note = clean(&d.note);
        let persona = self.persona_for(d.persona.as_deref(), p.inject_id);
        let who = self.persona_name(persona.as_deref(), "en");
        if approve
            && matches!(p.source, Source::Master)
            && p.summary.contains("UNPLANNED DIVERGENCE")
            && note.is_none()
        {
            bail!("unplanned training divergence requires explicit EXCON rationale");
        }
        if approve {
            self.edit_communications(
                pid,
                Some(p.communication_drafts.clone()),
                Some(p.inject_drafts.clone()),
            )?;
            if p.verdicts.iter().any(|v| !v.is_ok()) {
                bail!("proposal is not feasible; edit or reject the infeasible actions before approval");
            }
            if d.commands.is_none() && !p.invalid.is_empty() {
                bail!("proposal contains invalid actions; edit it before approval");
            }
        }
        let effective_limits = if approve {
            p.resource_limit_draft
                .as_ref()
                .map(|v| serde_json::from_value::<ResourceLimits>(v.clone()))
                .transpose()?
        } else {
            None
        };
        if approve {
            if let Some(limits) = &effective_limits {
                self.validate_limits_for_phase(limits, self.exercise_phase)?;
            }
        }
        let review = if !approve {
            Review::Rejected
        } else if acknowledge {
            Review::Acknowledged
        } else if partial {
            Review::PartiallyApproved
        } else {
            Review::Approved
        };
        self.proposal_mut(pid)?.decision = Some(ApprovalRecord {
            resource_limits: effective_limits,
            approve,
            actor: MASTER.into(),
            turn: self.turn,
            t_s: self.world.time_s(),
            wall_ms: wall_ms(),
            note: note.clone(),
            persona: persona.clone(),
            edited: d.commands.is_some() && reviewed != proposed_commands,
            proposed_commands,
            effective_commands: if approve { p.commands.clone() } else { Vec::new() },
            rejected_actions: rejected.clone(),
            review,
        });
        let reason = |r: &RejectedAction| {
            r.note
                .as_ref()
                .map(|n| format!(" Reason: {n}"))
                .unwrap_or_default()
        };
        let mut facts = Vec::new();
        if !approve {
            self.transition(
                pid,
                ProposalStatus::Rejected,
                MASTER,
                note.as_deref().unwrap_or("Rejected by EXCON"),
            )?;
            facts.push(format!(
                "Your orders were NOT approved by {who}: {}",
                if p.descriptions.is_empty() {
                    p.summary.clone()
                } else {
                    p.descriptions.join("; ")
                }
            ));
            for r in rejected.iter().filter(|r| r.note.is_some()) {
                facts.push(format!("NOT APPROVED: {}.{}", r.description, reason(r)));
            }
        } else if acknowledge {
            self.transition(
                pid,
                ProposalStatus::Acknowledged,
                MASTER,
                note.as_deref().unwrap_or("Acknowledged by EXCON"),
            )?;
            facts.push(format!(
                "Your message was acknowledged by {who}. It contained no orders to execute."
            ));
        } else {
            self.transition(
                pid,
                ProposalStatus::Approved,
                MASTER,
                note.as_deref().unwrap_or(if partial {
                    "Partially approved by EXCON"
                } else {
                    "Approved by EXCON"
                }),
            )?;
            if partial {
                facts.push(format!(
                    "{who} approved {} of the {} actions of your orders; the others will NOT be executed.",
                    kept.len(),
                    reviewed.len()
                ));
            }
            if p.pure_narrative() {
                facts.push(format!("Narrative initiative approved by {who}. Its outcome is pending; no simulated action was executed."));
            } else if matches!(p.source, Source::Master) || p.commands.is_empty() {
                self.execute_proposal(pid)?;
                facts.extend(self.proposal(pid).unwrap().results.clone());
                if p.commands.is_empty()
                    && (!p.communication_drafts.is_empty() || !p.inject_drafts.is_empty())
                {
                    facts.push(
                        format!("APPROVED: the reviewed communication will be delivered by {who}."),
                    );
                }
            } else {
                for (desc, v) in p.descriptions.iter().zip(&p.verdicts) {
                    let notes = match v {
                        Verdict::Ok { notes } => notes.join("; "),
                        _ => String::new(),
                    };
                    facts.push(format!(
                        "APPROVED: {desc}{}. It awaits execution or turn resolution.",
                        if notes.is_empty() {
                            String::new()
                        } else {
                            format!(" ({notes})")
                        }
                    ));
                }
            }
            for r in &rejected {
                facts.push(format!(
                    "NOT APPROVED: {}. It will not be executed.{}",
                    r.description,
                    reason(r)
                ));
            }
        }
        if approve && p.narrative && p.carries_actions {
            facts.push("The narrative part of your response was approved; its outcome is pending and is not simulated.".into());
        }
        if let Some(n) = &note {
            facts.push(format!("Note from {who}: {n}"));
        }
        self.proposal_mut(pid)?.master_note = note;
        // Decided directly: its AI review (if any) is over too.
        self.close_review_after_decision(pid);
        let facts_en = facts.join("\n");
        self.record("proposal_decided", MASTER, json!({"proposal": pid, "inject_id": p.inject_id, "review": review, "decision": self.proposal(pid).unwrap().decision, "facts": facts_en}));
        Ok(DecisionOutcome {
            proposal: pid,
            notify,
            persona,
            facts_en,
        })
    }

    /// A command by code (scripts, fixtures, tests): validated as EXCON and
    /// applied now. The EXCON console goes through
    /// [`Self::apply_or_queue_excon`], which waits for play while orders are open.
    pub fn direct_command(&mut self, cmd: &Command) -> Result<Vec<String>> {
        self.direct_command_logged(cmd, json!({"origin": "direct_excon_intervention"}))
            .map(|(notes, _)| notes)
    }

    /// Same, with `origin` merged into the log entry (e.g. the narrative
    /// outcome the intervention belongs to). Returns the notes and the `seq`
    /// of the `master_command` log entry.
    pub fn direct_command_logged(
        &mut self,
        cmd: &Command,
        origin: Value,
    ) -> Result<(Vec<String>, u64)> {
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
        self.absorb_monitoring();
        if r.is_ok() && matches!(cmd, Command::AdvanceTime { .. }) {
            self.clock_control.revision += 1;
        }
        let mut entry = json!({"command": cmd, "result": match &r { Ok(n) => json!({"ok": n}), Err(e) => json!({"error": e}) }});
        if let (Some(map), Some(extra)) = (entry.as_object_mut(), origin.as_object()) {
            map.extend(extra.clone());
        }
        self.record("master_command", MASTER, entry);
        let seq = self.log.last().unwrap().seq;
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
        r.map(|notes| (notes, seq)).map_err(|e| anyhow::anyhow!(e))
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
            "linked_conversations": self.master_linked_conversations(),
            "log": self.log,
        })
    }
}
