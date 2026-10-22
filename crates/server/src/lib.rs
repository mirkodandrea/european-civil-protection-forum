//! HTTP + WebSocket server: the EXCON and player web UIs, and a JSON API
//! that also lets scripts drive the exercise directly ("by code").
//!
//! Shared state is one `Arc<Mutex<Exercise>>` (also read by the Bevy
//! renderer). Handlers hold the lock only for synchronous work; LLM calls run
//! in spawned tasks (see [`flow`]). Clients open `/ws` and receive the
//! exercise `generation` whenever anything changes, then re-fetch.
//!
//! Route definitions and request types live in this crate.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use web_time::{Duration, Instant};

use access::{Access, Principal};
use exercise::{Decision, Exercise, ProposalStatus};
#[cfg(not(target_arch = "wasm32"))]
use futures::{SinkExt, StreamExt};
use llm::Llm;
#[cfg(not(target_arch = "wasm32"))]
use rocket::http::Status;
#[cfg(not(target_arch = "wasm32"))]
use rocket::response::{self, Responder};
#[cfg(not(target_arch = "wasm32"))]
use rocket::serde::json::Json;
#[cfg(not(target_arch = "wasm32"))]
use rocket::{Build, Request, Rocket};
#[cfg(not(target_arch = "wasm32"))]
use rocket_ws::Message as WsMessage;
use serde::Deserialize;
use serde_json::{json, Value};
use sim::Command;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::watch;

#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(target_arch = "wasm32")]
mod browser_runtime;
#[cfg(target_arch = "wasm32")]
use browser::{Json, Status};
#[cfg(target_arch = "wasm32")]
use browser_runtime as tokio;
pub mod access;
mod assessment;
mod decision_drafts;
pub mod flow;
mod forecast;

#[derive(Clone)]
pub struct AppState {
    pub ex: Arc<Mutex<Exercise>>,
    pub llm: Llm,
    pub access: Arc<Access>,
    #[cfg(not(target_arch = "wasm32"))]
    generation: watch::Receiver<u64>,
    assessments: Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, Value>>>,
    forecasts: Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, forecast::Draft>>>,
    forecast_slot: Arc<tokio::sync::Semaphore>,
    presence: Arc<Mutex<std::collections::BTreeMap<String, (String, Instant)>>>,
}

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, Exercise> {
        self.ex.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Build the Rocket application. Spawns the generation watcher, so call it
/// inside a Tokio runtime.
#[cfg(not(target_arch = "wasm32"))]
pub fn router(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: Option<PathBuf>,
) -> (Rocket<Build>, AppState) {
    let access = Access::new(ex.lock().unwrap_or_else(|p| p.into_inner()).role_ids());
    router_with_access(ex, llm, web_dir, access)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn router_with_access(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: Option<PathBuf>,
    access: Access,
) -> (Rocket<Build>, AppState) {
    let (tx, rx) = watch::channel(0u64);
    let watched = ex.clone();
    tokio::spawn(async move {
        let mut last = 0;
        loop {
            let g = watched.lock().map(|e| e.generation).unwrap_or(last);
            if g != last {
                last = g;
                if tx.send(g).is_err() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    let state = AppState {
        ex,
        llm,
        generation: rx,
        access: Arc::new(access),
        assessments: Default::default(),
        forecasts: Default::default(),
        forecast_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        presence: Default::default(),
    };
    tokio::spawn(flow::run_clock(state.clone()));
    let config = rocket::Config::figment()
        .merge(("log_level", "off"))
        .merge(("limits.json", 2 * 1024 * 1024));
    let mut app = rocket::custom(config)
        .manage(state.clone())
        .mount(
            "/",
            rocket::routes![
                health,
                get_access,
                get_access_links,
                post_knowledge,
                forecast::compute,
                forecast::publish,
                post_references,
                post_resource_limits,
                post_clock,
                post_phase,
                post_schedule,
                post_cancel_schedule,
                post_execute,
                get_evaluator,
                assessment::get,
                get_static,
                get_state,
                get_master,
                get_player,
                post_presence,
                get_presence,
                decision_drafts::prepare,
                decision_drafts::submit,
                post_player_chat,
                post_master_chat,
                post_reply,
                post_language,
                post_start,
                post_inject,
                post_msel_send,
                post_prompt,
                post_decide,
                post_command,
                post_preview,
                post_preview_apply,
                post_correction,
                post_design,
                post_path,
                post_resolve,
                post_rollback,
                post_end,
                post_settings,
                get_catalogue,
                get_debrief,
                ws
            ],
        )
        .register("/", rocket::catchers![api_catcher]);
    if let Some(dir) = web_dir {
        app = app.mount("/", rocket::fs::FileServer::from(dir));
    }
    (app, state)
}

/// Serve until the process exits.
#[cfg(not(target_arch = "wasm32"))]
pub async fn serve(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: PathBuf,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let (app, state) = router(ex, llm, Some(web_dir));
    println!(
        "EXCON access: http://localhost:{}/play.html?token={}",
        addr.port(),
        state.access.master
    );
    tracing::info!(
        "web UI on http://{addr}; distribute individual player links from the EXCON console"
    );
    let config = app
        .figment()
        .clone()
        .merge(("address", addr.ip()))
        .merge(("port", addr.port()));
    app.configure(config).launch().await?;
    Ok(())
}

// ---------------------------------------------------------------- errors

pub struct ApiError(Status, String);

#[cfg(not(target_arch = "wasm32"))]
impl<'r> Responder<'r, 'static> for ApiError {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        (self.0, Json(json!({"error": self.1}))).respond_to(request)
    }
}

// Guard/data failures retain the JSON error envelope used by browser clients.
#[cfg(not(target_arch = "wasm32"))]
#[rocket::catch(default)]
async fn api_catcher(mut status: Status, request: &Request<'_>) -> (Status, Json<Value>) {
    // Preserve authorization errors for unsupported methods on protected URLs.
    // Rocket otherwise returns 404 before invoking a route's request guards.
    let path = request.uri().path();
    if status == Status::NotFound && (path.starts_with("/api/") || path == "/ws") {
        if let rocket::request::Outcome::Error((denied, ())) = request.guard::<Principal>().await {
            status = denied;
        }
    }
    let message = match status.code {
        401 => "access key required",
        403 => "access key does not authorize this endpoint",
        _ => status.reason().unwrap_or("request failed"),
    };
    (status, Json(json!({"error": message})))
}

#[cfg(not(target_arch = "wasm32"))]
#[rocket::get("/api/health")]
fn health() -> Json<Value> {
    Json(json!({"ok": true}))
}

#[cfg(not(target_arch = "wasm32"))]
#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for AppState {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> rocket::request::Outcome<Self, ()> {
        match request.rocket().state::<Self>() {
            Some(state) => rocket::request::Outcome::Success(state.clone()),
            None => rocket::request::Outcome::Error((Status::InternalServerError, ())),
        }
    }
}

/// Decode API filters without coupling the headless Exercise types to HTTP.
pub(crate) struct EvidenceQuery(pub exercise::evaluation::EvidenceFilter);

#[cfg(not(target_arch = "wasm32"))]
#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for EvidenceQuery {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> rocket::request::Outcome<Self, ()> {
        let query = request.uri().query().map(|q| q.as_str()).unwrap_or("");
        match serde_urlencoded::from_str(query) {
            Ok(filter) => rocket::request::Outcome::Success(Self(filter)),
            Err(_) => rocket::request::Outcome::Error((Status::BadRequest, ())),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(Status::BadRequest, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(Status::BadRequest, msg.into())
}

// ---------------------------------------------------------------- reads

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/static"))]
async fn get_static(s: AppState, principal: Principal) -> ApiResult {
    let ex = s.lock();
    if let Principal::Player { role } = principal {
        return Ok(Json(ex.player_static(&role)?));
    }
    let mut v = ex.world.static_view();
    v["msel"] = json!(ex.world.scn.msel);
    Ok(Json(v))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/access"))]
async fn get_access(principal: Principal) -> ApiResult {
    Ok(Json(json!(principal)))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master/access"))]
async fn get_access_links(_principal: Principal, s: AppState) -> ApiResult {
    Ok(Json(
        json!({"players": s.access.players.iter().map(|(role, token)| json!({"role":role,"url":format!("/player.html?role={role}&token={token}")})).collect::<Vec<_>>(), "evaluators":s.access.evaluators.iter().map(|(id, token)| json!({"id":id,"url":format!("/evaluator.html?evaluator={id}&token={token}")})).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
struct KnowledgeBody {
    role: String,
    source: String,
    #[serde(default)]
    observation: Option<exercise::MapObservation>,
    #[serde(default)]
    current_perimeter: bool,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/knowledge", data = "<b>")
)]
async fn post_knowledge(_principal: Principal, s: AppState, b: Json<KnowledgeBody>) -> ApiResult {
    let Json(b) = b;
    let observation = {
        let ex = s.lock();
        if !ex.players.contains_key(&b.role)
            || b.source.trim().is_empty()
            || b.source.chars().count() > 2000
        {
            return Err(bad("known role and information source required"));
        }
        if b.current_perimeter && b.observation.is_some() {
            return Err(bad("choose an observation or current_perimeter"));
        }
        let observation = if b.current_perimeter {
            ex.current_perimeter()
        } else {
            b.observation.ok_or_else(|| bad("observation required"))?
        };
        ex.validate_observation(&observation)?;
        observation
    };
    let text = Exercise::map_information_text(&b.source);
    let translations = flow::translations(&s, std::slice::from_ref(&b.role), &text).await;
    let id = s.lock().acquire_map(
        &b.role,
        &b.source,
        observation,
        translations.get(&b.role).map(String::as_str),
    )?;
    Ok(Json(json!({"message":id})))
}

#[derive(Deserialize)]
struct ReferencesBody {
    references: Vec<exercise::DecisionEvidence>,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/references", data = "<b>")
)]
async fn post_references(_principal: Principal, s: AppState, b: Json<ReferencesBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().set_reference_materials(b.references)?;
    Ok(Json(json!({"ok":true})))
}
#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/evaluator"))]
async fn get_evaluator(
    _principal: Principal,
    s: AppState,
    filter: crate::EvidenceQuery,
) -> ApiResult {
    let crate::EvidenceQuery(filter) = filter;
    Ok(Json(s.lock().evaluator_view(&filter)?))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/state"))]
async fn get_state(_principal: Principal, s: AppState) -> ApiResult {
    let ex = s.lock();
    Ok(Json(json!({
        "wall_ms": web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d|d.as_millis()).unwrap_or(0),
        "exercise_phase":ex.exercise_phase,
        "phase_history":ex.phase_history,
        "resource_limits":ex.resource_limits,
        "clock_control":ex.clock_control,
        "scheduled_injects":ex.scheduled_injects,
        "turn": ex.turn,
        "phase": ex.phase,
        "turn_minutes": ex.turn_minutes,
        "speed": ex.speed,
        "resolving": ex.resolving.as_ref().map(|r| json!({"turn": r.turn, "from_s": r.from_s, "to_s": r.to_s, "minutes": r.minutes})),
        "generation": ex.generation,
        "last_turn": ex.last_turn,
        "players": ex.players,
        "sent_injects": ex.sent_injects,
        "llm": {"available": s.llm.is_available(), "model": s.llm.model(), "translation_model":s.llm.model_for("translate")},
        "world": ex.world.view(),
        "resource_accounting": ex.resource_accounting(None),
    })))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master"))]
async fn get_master(_principal: Principal, s: AppState) -> ApiResult {
    let ex = s.lock();
    let tail = ex.log.len().saturating_sub(300);
    // Peer-to-peer chat is private to its two participants. Operational
    // proposals remain visible to EXCON through the separate review queue.
    let messages = ex
        .messages
        .iter()
        .filter(|m| {
            !(m.kind == exercise::MsgKind::Chat
                && m.from != exercise::MASTER
                && m.to != exercise::MASTER)
        })
        .collect::<Vec<_>>();
    let log = ex.log[tail..]
        .iter()
        .filter(|entry| {
            !matches!(entry.kind.as_str(), "message" | "chat_source")
                || !(entry.data["kind"] == "chat"
                    && entry.data["from"]
                        .as_str()
                        .is_some_and(|from| from != exercise::MASTER)
                    && entry.data["to"]
                        .as_str()
                        .is_some_and(|to| to != exercise::MASTER))
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "proposals": ex.proposals,
        "messages": messages,
        "log": log,
        "training_design":ex.design_view(),
        "correction":ex.correction_view(),
        "digest": ex.world.digest(None),
        "conversations": ex.conversations,
    })))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/player/<role>"))]
async fn get_player(_principal: Principal, s: AppState, role: String) -> ApiResult {
    let ex = s.lock();
    let player = ex
        .players
        .get(&role)
        .cloned()
        .ok_or_else(|| bad(format!("unknown role {role}")))?;
    let proposals: Vec<Value> = ex
        .proposals
        .iter()
        .filter(|p| p.source.label() == role)
        .map(|p| json!({"id": p.id, "turn": p.turn, "status": p.status, "original": p.original, "inject_id": p.inject_id, "reply_to": p.reply_to, "declaration_kind": p.declaration_kind, "history": p.history, "decision": p.decision.as_ref().map(|d| json!({"approve":d.approve, "note":d.note, "t_s":d.t_s})), "executions": p.executions, "evidence": p.evidence}))
        .collect();
    Ok(Json(json!({
        "map": ex.player_map(&role)?,
        "knowledge_events": ex.knowledge_events.iter().filter(|e| e.role == role).collect::<Vec<_>>(),
        "exercise_phase":ex.exercise_phase,
        "clock_control":ex.clock_control,
        "player": player,
        "role": ex.world.scn.role(&role),
        "turn": ex.turn,
        "phase": ex.phase,
        "clock": ex.world.clock(),
        "time_s":ex.world.time_s(),
        "inbox": ex.inbox(&role),
        "drafts": ex.player_drafts(&role),
        "resources": ex.world.d.pools.get(&role),
        "resource_accounting": ex.resource_accounting(Some(&role))[&role],
        "deployments":ex.world.d.tasks.iter().filter(|t|t.role==role).map(|t|json!({"task":t,"position_m":t.position(ex.world.time_s()),"active_qty":ex.world.task_active_qty(t)})).collect::<Vec<_>>(),
        "allocations": ex.world.d.allocations.iter().filter(|a| a.role == role).collect::<Vec<_>>(),
        "proposals": proposals,
    })))
}

#[derive(Deserialize)]
struct PresenceBody {
    to: String,
    typing: bool,
}

// Ephemeral typing hints: deliberately kept outside Exercise so snapshots and
// the exercise record contain no transient UI state.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/presence", data = "<body>")
)]
async fn post_presence(
    principal: Principal,
    s: AppState,
    role: String,
    body: Json<PresenceBody>,
) -> ApiResult {
    if principal != (Principal::Player { role: role.clone() }) {
        return Err(bad("presence role does not match access key"));
    }
    if !s.access.players.contains_key(&body.to) || body.to == role {
        return Err(bad("unknown conversation recipient"));
    }
    let mut presence = s.presence.lock().unwrap_or_else(|p| p.into_inner());
    presence.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(5));
    if body.typing {
        presence.insert(role, (body.to.clone(), Instant::now()));
    } else {
        presence.remove(&role);
    }
    Ok(Json(json!({"ok":true})))
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::get("/api/player/<role>/presence?<peer>")
)]
async fn get_presence(principal: Principal, s: AppState, role: String, peer: String) -> ApiResult {
    if principal != (Principal::Player { role: role.clone() }) {
        return Err(bad("presence role does not match access key"));
    }
    let presence = s.presence.lock().unwrap_or_else(|p| p.into_inner());
    let typing = presence
        .get(&peer)
        .is_some_and(|(to, at)| to == &role && at.elapsed() < Duration::from_secs(5));
    Ok(Json(json!({"typing":typing,"from":peer})))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/catalogue"))]
async fn get_catalogue(_principal: Principal, s: AppState) -> ApiResult {
    let ex = s.lock();
    let all: Vec<String> = sim::ALL_KINDS.iter().map(|k| k.to_string()).collect();
    Ok(Json(
        json!({"kinds": sim::ALL_KINDS, "master_only": sim::MASTER_ONLY, "doc": sim::catalogue(&all), "roles": ex.world.scn.roles}),
    ))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/debrief"))]
async fn get_debrief(_principal: Principal, s: AppState) -> ApiResult {
    Ok(Json(s.lock().debrief()))
}

#[derive(Deserialize)]
struct ChatBody {
    to: String,
    text: String,
    #[serde(default)]
    interpret: bool,
}

async fn deliver_chat(s: AppState, from: String, b: ChatBody) -> ApiResult {
    let (language, turn, revision) = {
        let ex = s.lock();
        ex.validate_chat(&from, &b.to, &b.text, b.interpret)?;
        (ex.language_of(&from), ex.turn, ex.clock_control.revision)
    };
    let mut failed = false;
    let text_en = if language == "en" || from == exercise::MASTER {
        b.text.clone()
    } else {
        match s.llm.translate_to_english(&b.text, &language).await {
            Ok(text) => text,
            Err(_) => {
                failed = true;
                b.text.clone()
            }
        }
    };
    let target_language = s.lock().language_of(&b.to);
    let translated = if failed || target_language == "en" {
        text_en.clone()
    } else {
        match s.llm.translate(&text_en, &target_language).await {
            Ok(text) => text,
            Err(_) => {
                failed = true;
                text_en.clone()
            }
        }
    };
    let (message, proposal) = {
        let mut ex = s.lock();
        // A rollback/turn change during translation must not submit an obsolete order.
        if ex.turn != turn || ex.clock_control.revision != revision {
            return Err(bad("exercise changed during translation; send again"));
        }
        ex.chat(
            &from,
            &b.to,
            &b.text,
            &text_en,
            &translated,
            failed,
            b.interpret,
        )?
    };
    if let Some(pid) = proposal {
        tokio::spawn(flow::process_reply(s, pid));
    }
    Ok(Json(
        json!({"message":message,"proposal":proposal,"translation_failed":failed}),
    ))
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/chat", data = "<b>")
)]
async fn post_player_chat(
    _principal: Principal,
    s: AppState,
    role: String,
    b: Json<ChatBody>,
) -> ApiResult {
    let Json(b) = b;
    if b.interpret {
        return Err(bad(
            "prepare and confirm the decision privately before sending it",
        ));
    }
    deliver_chat(s, role, b).await
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/chat", data = "<b>")
)]
async fn post_master_chat(_principal: Principal, s: AppState, b: Json<ChatBody>) -> ApiResult {
    let Json(b) = b;
    deliver_chat(s, exercise::MASTER.into(), b).await
}

// ---------------------------------------------------------------- players

#[derive(Deserialize)]
struct TextBody {
    text: String,
}

#[derive(Deserialize)]
struct ReplyBody {
    evidence: exercise::DecisionEvidence,
    text: String,
    reply_to: u64,
    #[serde(default)]
    declaration_kind: exercise::DeclarationKind,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/reply", data = "<b>")
)]
async fn post_reply(
    _principal: Principal,
    s: AppState,
    role: String,
    b: Json<ReplyBody>,
) -> ApiResult {
    let Json(b) = b;
    let pid = {
        let mut ex = s.lock();
        let pid = ex.player_reply(&role, &b.text, b.reply_to, b.evidence)?;
        ex.set_declaration_kind(pid, b.declaration_kind)?;
        pid
    };
    tokio::spawn(flow::process_reply(s.clone(), pid));
    Ok(Json(json!({"proposal": pid})))
}

#[derive(Deserialize)]
struct LangBody {
    language: String,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/language", data = "<b>")
)]
async fn post_language(
    _principal: Principal,
    s: AppState,
    role: String,
    b: Json<LangBody>,
) -> ApiResult {
    let Json(b) = b;
    s.lock().set_language(&role, &b.language)?;
    Ok(Json(json!({"ok": true})))
}

// ---------------------------------------------------------------- master

#[derive(Deserialize)]
struct ClockBody {
    running: bool,
    ratio: f64,
    #[serde(default)]
    reason: String,
}
#[derive(Deserialize)]
struct LimitsBody {
    limits: exercise::ResourceLimits,
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/resource-limits", data = "<b>")
)]
async fn post_resource_limits(
    _principal: Principal,
    s: AppState,
    b: Json<LimitsBody>,
) -> ApiResult {
    let Json(b) = b;
    s.lock().set_resource_limits(b.limits, &b.reason, None)?;
    Ok(Json(json!({"ok":true})))
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/clock", data = "<b>")
)]
async fn post_clock(_principal: Principal, s: AppState, b: Json<ClockBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().configure_clock(b.running, b.ratio, &b.reason)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct PhaseBody {
    phase: exercise::ExercisePhase,
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/phase", data = "<b>")
)]
async fn post_phase(_principal: Principal, s: AppState, b: Json<PhaseBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().set_exercise_phase(b.phase, &b.reason)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct ScheduleBody {
    msel_id: String,
    #[serde(default)]
    at_s: Option<i64>,
    #[serde(default)]
    at_clock: Option<String>,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/schedules", data = "<b>")
)]
async fn post_schedule(_principal: Principal, s: AppState, b: Json<ScheduleBody>) -> ApiResult {
    let Json(b) = b;
    let mut ex = s.lock();
    let due = match (b.at_s, b.at_clock) {
        (Some(s), None) => s,
        (None, Some(clock)) => {
            let parts = clock
                .split(':')
                .map(str::parse::<i64>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| bad("at_clock must be HH:MM or HH:MM:SS on Day 1"))?;
            if !(2..=3).contains(&parts.len())
                || !(0..=23).contains(&parts[0])
                || !(0..=59).contains(&parts[1])
                || parts.get(2).is_some_and(|s| !(0..=59).contains(s))
            {
                return Err(bad("at_clock must be HH:MM or HH:MM:SS on Day 1"));
            }
            parts[0] * 3600 + parts[1] * 60 + parts.get(2).copied().unwrap_or(0)
                - ex.world.scn.start_clock_s
        }
        _ => return Err(bad("provide exactly one of at_s or at_clock")),
    };
    let id = ex.schedule_inject(&b.msel_id, due)?;
    Ok(Json(json!({"schedule":id})))
}
#[derive(Deserialize)]
struct ReasonBody {
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/schedules/<id>/cancel", data = "<b>")
)]
async fn post_cancel_schedule(
    _principal: Principal,
    s: AppState,
    id: u64,
    b: Json<ReasonBody>,
) -> ApiResult {
    let Json(b) = b;
    s.lock().cancel_schedule(id, &b.reason)?;
    Ok(Json(json!({"ok":true})))
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/execute")
)]
async fn post_execute(_principal: Principal, s: AppState, id: u64) -> ApiResult {
    let results = s.lock().execute_approved(id)?;
    let state = s.clone();
    tokio::spawn(async move {
        flow::send_drafts(&state, id).await;
    });
    Ok(Json(json!({"results":results})))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/start"))]
async fn post_start(_principal: Principal, s: AppState) -> ApiResult {
    s.lock().start()?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct InjectBody {
    #[serde(default)]
    observation: Option<exercise::MapObservation>,
    title: String,
    #[serde(default = "default_sender")]
    sender: String,
    recipients: Vec<String>,
    text: String,
}

fn default_sender() -> String {
    "EXCON".into()
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/inject", data = "<b>")
)]
async fn post_inject(_principal: Principal, s: AppState, b: Json<InjectBody>) -> ApiResult {
    let Json(b) = b;
    if let Some(o) = &b.observation {
        s.lock().validate_observation(o)?;
    }
    let ids = flow::send_inject(&s, None, &b.title, &b.sender, &b.recipients, &b.text).await?;
    if let Some(o) = b.observation {
        let mut ex = s.lock();
        for id in &ids {
            ex.attach_map_observation(*id, o.clone(), exercise::KnowledgeSource::Delivered)?;
        }
    }
    Ok(Json(json!({"messages": ids})))
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/msel/<id>/send")
)]
async fn post_msel_send(_principal: Principal, s: AppState, id: String) -> ApiResult {
    let t = s
        .lock()
        .world
        .scn
        .msel
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .ok_or_else(|| bad(format!("unknown inject {id}")))?;
    let ids =
        flow::send_inject(&s, Some(&t.id), &t.title, &t.sender, &t.recipients, &t.text).await?;
    Ok(Json(json!({"messages": ids})))
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/prompt", data = "<b>")
)]
async fn post_prompt(_principal: Principal, s: AppState, b: Json<TextBody>) -> ApiResult {
    let Json(b) = b;
    if b.text.trim().is_empty() {
        return Err(bad("empty prompt"));
    }
    let pid = s.lock().master_prompt(&b.text);
    tokio::spawn(flow::process_master_prompt(s.clone(), pid));
    Ok(Json(json!({"proposal": pid})))
}

#[derive(Deserialize)]
struct DecideBody {
    #[serde(default)]
    preview_generation: Option<u64>,
    #[serde(default)]
    resource_limits: Option<exercise::ResourceLimits>,
    #[serde(default)]
    communication_drafts: Option<Vec<exercise::CommunicationDraft>>,
    #[serde(default)]
    inject_drafts: Option<Vec<Value>>,
    approve: bool,
    #[serde(default)]
    commands: Option<Vec<Value>>,
    #[serde(default)]
    note: Option<String>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/decide", data = "<b>")
)]
async fn post_decide(
    _principal: Principal,
    s: AppState,
    id: u64,
    b: Json<DecideBody>,
) -> ApiResult {
    let Json(b) = b;
    let commands = match b.commands {
        Some(vals) => Some(
            vals.into_iter()
                .map(serde_json::from_value::<Command>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| bad(format!("invalid command: {e}")))?,
        ),
        None => None,
    };
    let (outcome, drafts) = {
        let mut ex = s.lock();
        if b.approve && b.preview_generation.is_some_and(|g| g != ex.generation) {
            return Err(bad("exercise changed; review a fresh physical preview"));
        }
        if let Some(limits) = b.resource_limits {
            ex.edit_resource_limit_draft(id, limits)?;
        }
        if b.communication_drafts.is_some() || b.inject_drafts.is_some() {
            ex.edit_communications(id, b.communication_drafts, b.inject_drafts)?;
        }
        let outcome = ex.decide(
            id,
            Decision {
                approve: b.approve,
                commands,
                note: b.note,
            },
        )?;
        let p = ex.proposal(id).unwrap();
        let drafts = b.approve && p.status == ProposalStatus::Applied;
        (outcome, drafts)
    };
    if let Some(role) = outcome.notify.clone() {
        tokio::spawn(flow::send_feedback(
            s.clone(),
            role,
            id,
            outcome.facts_en.clone(),
        ));
    }
    if drafts {
        let st = s.clone();
        tokio::spawn(async move { flow::send_drafts(&st, id).await });
    }
    Ok(Json(json!(outcome)))
}

#[derive(Deserialize)]
struct PreviewBody {
    commands: Vec<Command>,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/preview", data = "<b>")
)]
async fn post_preview(_principal: Principal, s: AppState, b: Json<PreviewBody>) -> ApiResult {
    let Json(b) = b;
    Ok(Json(s.lock().preview_commands(&b.commands)?))
}
#[derive(Deserialize)]
struct CorrectionBody {
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/correct-intervention", data = "<b>")
)]
async fn post_correction(_principal: Principal, s: AppState, b: Json<CorrectionBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().correct_intervention(&b.reason)?;
    Ok(Json(json!({"ok":true})))
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/design", data = "<b>")
)]
async fn post_design(
    _principal: Principal,
    s: AppState,
    b: Json<exercise::design::TrainingDesign>,
) -> ApiResult {
    let Json(b) = b;
    s.lock().configure_design(b)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct PathBody {
    point: String,
    path: String,
    reason: String,
    #[serde(default)]
    adaptation: bool,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/path", data = "<b>")
)]
async fn post_path(_principal: Principal, s: AppState, b: Json<PathBody>) -> ApiResult {
    let Json(b) = b;
    s.lock()
        .approve_path(&b.point, &b.path, &b.reason, b.adaptation)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct PreviewApplyBody {
    generation: u64,
    commands: Vec<Command>,
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/preview/apply", data = "<b>")
)]
async fn post_preview_apply(
    _principal: Principal,
    s: AppState,
    b: Json<PreviewApplyBody>,
) -> ApiResult {
    let Json(b) = b;
    let mut ex = s.lock();
    if ex.generation != b.generation {
        return Err(bad("exercise changed; refresh the preview"));
    }
    if ex.clock_control.running || ex.phase == exercise::Phase::Resolving {
        return Err(bad(
            "pause simulation before applying a reviewed intervention",
        ));
    }
    if b.reason.trim().is_empty()
        || b.reason.len() > 2000
        || b.commands.is_empty()
        || b.commands.len() > 20
    {
        return Err(bad("intervention requires rationale and 1–20 commands"));
    }
    let mut results = Vec::new();
    for cmd in b.commands {
        let r = ex.direct_command(&cmd);
        results.push(match r {
            Ok(n) => json!({"command":cmd.describe(),"ok":true,"notes":n}),
            Err(e) => json!({"command":cmd.describe(),"ok":false,"error":e.to_string()}),
        });
    }
    // Rationale becomes part of the same correctable intervention generation.
    ex.record(
        "physical_preview_approved",
        exercise::MASTER,
        json!({"reason":b.reason,"preview_generation":b.generation,"results":results}),
    );
    ex.refresh_correction_generation();
    Ok(Json(json!({"results":results})))
}

/// One command or a list, applied immediately as the EXCON.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/command", data = "<body>")
)]
async fn post_command(_principal: Principal, s: AppState, body: Json<Value>) -> ApiResult {
    let Json(body) = body;
    let list = match body {
        Value::Array(a) => a,
        v => vec![v],
    };
    let mut results = Vec::new();
    for v in list {
        let cmd: Command = serde_json::from_value(v.clone())
            .map_err(|e| bad(format!("invalid command {v}: {e}")))?;
        let r = s.lock().direct_command(&cmd);
        results.push(match r {
            Ok(notes) => json!({"command": cmd.describe(), "ok": true, "notes": notes}),
            Err(e) => json!({"command": cmd.describe(), "ok": false, "error": e.to_string()}),
        });
    }
    Ok(Json(json!({"results": results})))
}

#[derive(Deserialize, Default)]
struct ResolveBody {
    #[serde(default)]
    minutes: Option<u32>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/resolve", data = "<body>")
)]
async fn post_resolve(
    _principal: Principal,
    s: AppState,
    body: Option<Json<ResolveBody>>,
) -> ApiResult {
    let minutes = body.map(|b| b.0.minutes).unwrap_or_default();
    s.lock().begin_resolution(minutes)?;
    // Spawned, so the turn completes even if the client goes away; the
    // response still waits for the report.
    let report = tokio::spawn(flow::run_resolution(s.clone()))
        .await
        .map_err(|e| bad(e.to_string()))??;
    let mut events: Vec<String> = report
        .applied
        .iter()
        .map(|a| {
            format!(
                "{} ({}): {}",
                a.action,
                a.by,
                if a.ok { "done" } else { &a.detail }
            )
        })
        .collect();
    events.extend(report.events.iter().map(|e| e.text.clone()));
    let approved = s
        .lock()
        .proposals
        .iter()
        .filter(|p| p.status == ProposalStatus::Applied && !p.communications_claimed)
        .map(|p| p.id)
        .collect::<Vec<_>>();
    for pid in approved {
        let st = s.clone();
        tokio::spawn(async move {
            flow::send_drafts(&st, pid).await;
        });
    }
    tokio::spawn(flow::situation_updates(s.clone(), events));
    Ok(Json(json!(report)))
}

#[derive(Deserialize)]
struct RollbackBody {
    turn: u32,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/rollback", data = "<b>")
)]
async fn post_rollback(_principal: Principal, s: AppState, b: Json<RollbackBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().rollback(b.turn)?;
    Ok(Json(json!({"ok": true})))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/end"))]
async fn post_end(_principal: Principal, s: AppState) -> ApiResult {
    s.lock().end()?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct SettingsBody {
    #[serde(default)]
    turn_minutes: Option<u32>,
    /// Simulated seconds per wall-clock second during resolution; 0 = maximum speed.
    #[serde(default)]
    speed: Option<f32>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/settings", data = "<b>")
)]
async fn post_settings(_principal: Principal, s: AppState, b: Json<SettingsBody>) -> ApiResult {
    let Json(b) = b;
    let mut ex = s.lock();
    if let Some(m) = b.turn_minutes {
        let m = m.clamp(1, 24 * 60);
        ex.turn_minutes = m;
        ex.record("settings", exercise::MASTER, json!({"turn_minutes": m}));
    }
    if let Some(v) = b.speed {
        let v = if v.is_finite() {
            v.clamp(0.0, 86_400.0)
        } else {
            0.0
        };
        ex.speed = v;
        ex.record("settings", exercise::MASTER, json!({"speed": v}));
    }
    Ok(Json(
        json!({"turn_minutes": ex.turn_minutes, "speed": ex.speed}),
    ))
}

// ---------------------------------------------------------------- ws

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/ws"))]
fn ws(
    _principal: Principal,
    s: AppState,
    socket: rocket_ws::WebSocket,
) -> rocket_ws::Channel<'static> {
    let mut rx = s.generation.clone();
    socket.channel(move |mut stream| Box::pin(async move {
        let generation = *rx.borrow_and_update();
        stream.send(WsMessage::Text(json!({"generation": generation}).to_string())).await?;
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() { break; }
                    let generation = *rx.borrow_and_update();
                    stream.send(WsMessage::Text(json!({"generation": generation}).to_string())).await?;
                }
                message = stream.next() => match message {
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(WsMessage::Ping(payload))) => stream.send(WsMessage::Pong(payload)).await?,
                    Some(Err(error)) => return Err(error),
                    _ => {},
                },
            }
        }
        Ok(())
    }))
}
