//! HTTP + WebSocket server: the Master and player web UIs, and a JSON API
//! that also lets scripts drive the exercise directly ("by code").
//!
//! Shared state is one `Arc<Mutex<Exercise>>` (also read by the Bevy
//! renderer). Handlers hold the lock only for synchronous work; LLM calls run
//! in spawned tasks (see [`flow`]). Clients open `/ws` and receive the
//! exercise `generation` whenever anything changes, then re-fetch.
//!
//! See `docs/api.md` for the endpoint reference.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use access::{Access, Principal};
use exercise::{Decision, Exercise, ProposalStatus};
use llm::Llm;
use serde::Deserialize;
use serde_json::{json, Value};
use sim::Command;
use tokio::sync::watch;

pub mod flow;
pub mod access;
mod assessment;

#[derive(Clone)]
pub struct AppState {
    pub ex: Arc<Mutex<Exercise>>,
    pub llm: Llm,
    pub access: Arc<Access>,
    generation: watch::Receiver<u64>,
    assessments: Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, Value>>>,
}

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, Exercise> {
        self.ex.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Build the application router. Spawns the generation watcher, so call it
/// inside a Tokio runtime.
pub fn router(ex: Arc<Mutex<Exercise>>, llm: Llm, web_dir: Option<PathBuf>) -> (Router, AppState) {
    let access = Access::new(ex.lock().unwrap_or_else(|p| p.into_inner()).role_ids());
    router_with_access(ex, llm, web_dir, access)
}

pub fn router_with_access(ex: Arc<Mutex<Exercise>>, llm: Llm, web_dir: Option<PathBuf>, access: Access) -> (Router, AppState) {
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
    let state = AppState { ex, llm, generation: rx, access: Arc::new(access), assessments: Default::default() };
    tokio::spawn(flow::run_clock(state.clone()));
    let api = Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok":true})) }))
        .route("/api/access", get(get_access))
        .route("/api/master/access", get(get_access_links))
        .route("/api/master/knowledge", post(post_knowledge))
        .route("/api/master/references", post(post_references))
        .route("/api/master/resource-limits", post(post_resource_limits))
        .route("/api/master/clock", post(post_clock))
        .route("/api/master/phase", post(post_phase))
        .route("/api/master/schedules", post(post_schedule))
        .route("/api/master/schedules/:id/cancel", post(post_cancel_schedule))
        .route("/api/master/proposals/:id/execute", post(post_execute))
        .route("/api/evaluator", get(get_evaluator))
        .route("/api/evaluator/assessment", get(assessment::get))
        .route("/api/static", get(get_static))
        .route("/api/state", get(get_state))
        .route("/api/master", get(get_master))
        .route("/api/player/:role", get(get_player))
        .route("/api/player/:role/reply", post(post_reply))
        .route("/api/player/:role/language", post(post_language))
        .route("/api/master/start", post(post_start))
        .route("/api/master/inject", post(post_inject))
        .route("/api/master/msel/:id/send", post(post_msel_send))
        .route("/api/master/prompt", post(post_prompt))
        .route("/api/master/proposals/:id/decide", post(post_decide))
        .route("/api/master/command", post(post_command))
        .route("/api/master/resolve", post(post_resolve))
        .route("/api/master/rollback", post(post_rollback))
        .route("/api/master/end", post(post_end))
        .route("/api/master/settings", post(post_settings))
        .route("/api/catalogue", get(get_catalogue))
        .route("/api/debrief", get(get_debrief))
        .route("/ws", get(ws))
        .layer(axum::middleware::from_fn_with_state(state.clone(), access::authorize))
        .with_state(state.clone());
    let app = match web_dir {
        Some(dir) => api.fallback_service(tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true)),
        None => api,
    };
    (app, state)
}

/// Serve until the process exits.
pub async fn serve(ex: Arc<Mutex<Exercise>>, llm: Llm, web_dir: PathBuf, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let (app, state) = router(ex, llm, Some(web_dir));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Master access: http://localhost:{}/play.html?token={}", addr.port(), state.access.master);
    tracing::info!("web UI on http://{addr}; distribute individual player links from the Master console");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------- errors

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

// ---------------------------------------------------------------- reads

async fn get_static(State(s): State<AppState>, Extension(principal): Extension<Principal>) -> ApiResult {
    let ex = s.lock();
    if let Principal::Player { role } = principal { return Ok(Json(ex.player_static(&role)?)); }
    let mut v = ex.world.static_view();
    v["msel"] = json!(ex.world.scn.msel);
    Ok(Json(v))
}

async fn get_access(Extension(principal): Extension<Principal>) -> ApiResult { Ok(Json(json!(principal))) }

async fn get_access_links(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!({"players": s.access.players.iter().map(|(role, token)| json!({"role":role,"url":format!("/player.html?role={role}&token={token}")})).collect::<Vec<_>>(), "evaluators":s.access.evaluators.iter().map(|(id, token)| json!({"id":id,"url":format!("/evaluator.html?evaluator={id}&token={token}")})).collect::<Vec<_>>()})))
}

#[derive(Deserialize)]
struct KnowledgeBody { role: String, source: String, #[serde(default)] observation: Option<exercise::MapObservation>, #[serde(default)] current_perimeter: bool }
async fn post_knowledge(State(s): State<AppState>, Json(b): Json<KnowledgeBody>) -> ApiResult {
    let observation = {
        let ex = s.lock();
        if !ex.players.contains_key(&b.role) || b.source.trim().is_empty() || b.source.chars().count() > 2000 { return Err(bad("known role and information source required")); }
        if b.current_perimeter && b.observation.is_some() { return Err(bad("choose an observation or current_perimeter")); }
        let observation = if b.current_perimeter { ex.current_perimeter() } else { b.observation.ok_or_else(|| bad("observation required"))? };
        ex.validate_observation(&observation)?;
        observation
    };
    let text = Exercise::map_information_text(&b.source);
    let translations = flow::translations(&s, std::slice::from_ref(&b.role), &text).await;
    let id = s.lock().acquire_map(&b.role, &b.source, observation, translations.get(&b.role).map(String::as_str))?;
    Ok(Json(json!({"message":id})))
}

#[derive(Deserialize)]
struct ReferencesBody { references: Vec<exercise::DecisionEvidence> }
async fn post_references(State(s): State<AppState>, Json(b): Json<ReferencesBody>) -> ApiResult {
    s.lock().set_reference_materials(b.references)?;
    Ok(Json(json!({"ok":true})))
}
async fn get_evaluator(State(s): State<AppState>, Query(filter): Query<exercise::evaluation::EvidenceFilter>) -> ApiResult {
    Ok(Json(s.lock().evaluator_view(&filter)?))
}

async fn get_state(State(s): State<AppState>) -> ApiResult {
    let ex = s.lock();
    Ok(Json(json!({
        "wall_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_millis()).unwrap_or(0),
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
        "llm": {"available": s.llm.is_available(), "model": s.llm.model()},
        "world": ex.world.view(),
        "resource_accounting": ex.resource_accounting(None),
    })))
}

async fn get_master(State(s): State<AppState>) -> ApiResult {
    let ex = s.lock();
    let tail = ex.log.len().saturating_sub(300);
    Ok(Json(json!({
        "proposals": ex.proposals,
        "messages": ex.messages,
        "log": &ex.log[tail..],
        "digest": ex.world.digest(None),
        "conversations": ex.conversations,
    })))
}

async fn get_player(State(s): State<AppState>, Path(role): Path<String>) -> ApiResult {
    let ex = s.lock();
    let player = ex.players.get(&role).cloned().ok_or_else(|| bad(format!("unknown role {role}")))?;
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
        "inbox": ex.inbox(&role),
        "resources": ex.world.d.pools.get(&role),
        "resource_accounting": ex.resource_accounting(Some(&role))[&role],
        "allocations": ex.world.d.allocations.iter().filter(|a| a.role == role).collect::<Vec<_>>(),
        "proposals": proposals,
    })))
}

async fn get_catalogue(State(s): State<AppState>) -> ApiResult {
    let ex = s.lock();
    let all: Vec<String> = sim::ALL_KINDS.iter().map(|k| k.to_string()).collect();
    Ok(Json(json!({"kinds": sim::ALL_KINDS, "master_only": sim::MASTER_ONLY, "doc": sim::catalogue(&all), "roles": ex.world.scn.roles})))
}

async fn get_debrief(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.lock().debrief()))
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

async fn post_reply(State(s): State<AppState>, Path(role): Path<String>, Json(b): Json<ReplyBody>) -> ApiResult {
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

async fn post_language(State(s): State<AppState>, Path(role): Path<String>, Json(b): Json<LangBody>) -> ApiResult {
    s.lock().set_language(&role, &b.language)?;
    Ok(Json(json!({"ok": true})))
}

// ---------------------------------------------------------------- master

#[derive(Deserialize)]
struct ClockBody { running:bool, ratio:f64, #[serde(default)] reason:String }
#[derive(Deserialize)]
struct LimitsBody {limits:exercise::ResourceLimits,reason:String}
async fn post_resource_limits(State(s):State<AppState>,Json(b):Json<LimitsBody>)->ApiResult {
    s.lock().set_resource_limits(b.limits,&b.reason,None)?;Ok(Json(json!({"ok":true})))
}
async fn post_clock(State(s):State<AppState>,Json(b):Json<ClockBody>)->ApiResult {
    s.lock().configure_clock(b.running,b.ratio,&b.reason)?;Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct PhaseBody { phase:exercise::ExercisePhase, reason:String }
async fn post_phase(State(s):State<AppState>,Json(b):Json<PhaseBody>)->ApiResult {
    s.lock().set_exercise_phase(b.phase,&b.reason)?;Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct ScheduleBody { msel_id:String, #[serde(default)] at_s:Option<i64>, #[serde(default)] at_clock:Option<String> }
async fn post_schedule(State(s):State<AppState>,Json(b):Json<ScheduleBody>)->ApiResult {
    let mut ex=s.lock();
    let due=match (b.at_s,b.at_clock) {
        (Some(s),None)=>s,
        (None,Some(clock))=>{
            let parts=clock.split(':').map(str::parse::<i64>).collect::<Result<Vec<_>,_>>().map_err(|_|bad("at_clock must be HH:MM or HH:MM:SS on Day 1"))?;
            if !(2..=3).contains(&parts.len()) || !(0..=23).contains(&parts[0]) || !(0..=59).contains(&parts[1]) || parts.get(2).is_some_and(|s|!(0..=59).contains(s)){return Err(bad("at_clock must be HH:MM or HH:MM:SS on Day 1"));}
            parts[0]*3600+parts[1]*60+parts.get(2).copied().unwrap_or(0)-ex.world.scn.start_clock_s
        }
        _=>return Err(bad("provide exactly one of at_s or at_clock")),
    };
    let id=ex.schedule_inject(&b.msel_id,due)?;Ok(Json(json!({"schedule":id})))
}
#[derive(Deserialize)]
struct ReasonBody { reason:String }
async fn post_cancel_schedule(State(s):State<AppState>,Path(id):Path<u64>,Json(b):Json<ReasonBody>)->ApiResult {
    s.lock().cancel_schedule(id,&b.reason)?;Ok(Json(json!({"ok":true})))
}
async fn post_execute(State(s):State<AppState>,Path(id):Path<u64>)->ApiResult {
    let results=s.lock().execute_approved(id)?;
    let state=s.clone();tokio::spawn(async move {flow::send_drafts(&state,id).await;});
    Ok(Json(json!({"results":results})))
}

async fn post_start(State(s): State<AppState>) -> ApiResult {
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

async fn post_inject(State(s): State<AppState>, Json(b): Json<InjectBody>) -> ApiResult {
    if let Some(o) = &b.observation { s.lock().validate_observation(o)?; }
    let ids = flow::send_inject(&s, None, &b.title, &b.sender, &b.recipients, &b.text).await?;
    if let Some(o) = b.observation { let mut ex = s.lock(); for id in &ids { ex.attach_map_observation(*id, o.clone(), exercise::KnowledgeSource::Delivered)?; } }
    Ok(Json(json!({"messages": ids})))
}

async fn post_msel_send(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let t = s.lock().world.scn.msel.iter().find(|i| i.id == id).cloned().ok_or_else(|| bad(format!("unknown inject {id}")))?;
    let ids = flow::send_inject(&s, Some(&t.id), &t.title, &t.sender, &t.recipients, &t.text).await?;
    Ok(Json(json!({"messages": ids})))
}

async fn post_prompt(State(s): State<AppState>, Json(b): Json<TextBody>) -> ApiResult {
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
    resource_limits:Option<exercise::ResourceLimits>,
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

async fn post_decide(State(s): State<AppState>, Path(id): Path<u64>, Json(b): Json<DecideBody>) -> ApiResult {
    let commands = match b.commands {
        Some(vals) => Some(vals.into_iter().map(serde_json::from_value::<Command>).collect::<Result<Vec<_>, _>>().map_err(|e| bad(format!("invalid command: {e}")))?),
        None => None,
    };
    let (outcome, drafts) = {
        let mut ex = s.lock();
        if let Some(limits)=b.resource_limits {ex.edit_resource_limit_draft(id,limits)?;}
        if b.communication_drafts.is_some() || b.inject_drafts.is_some() { ex.edit_communications(id, b.communication_drafts, b.inject_drafts)?; }
        let outcome = ex.decide(id, Decision { approve: b.approve, commands, note: b.note })?;
        let p = ex.proposal(id).unwrap();
        let drafts = b.approve && p.status == ProposalStatus::Applied;
        (outcome, drafts)
    };
    if let Some(role) = outcome.notify.clone() {
        tokio::spawn(flow::send_feedback(s.clone(), role, id, outcome.facts_en.clone()));
    }
    if drafts {
        let st = s.clone();
        tokio::spawn(async move { flow::send_drafts(&st, id).await });
    }
    Ok(Json(json!(outcome)))
}

/// One command or a list, applied immediately as the Master.
async fn post_command(State(s): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let list = match body {
        Value::Array(a) => a,
        v => vec![v],
    };
    let mut results = Vec::new();
    for v in list {
        let cmd: Command = serde_json::from_value(v.clone()).map_err(|e| bad(format!("invalid command {v}: {e}")))?;
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

async fn post_resolve(State(s): State<AppState>, body: Option<Json<ResolveBody>>) -> ApiResult {
    let minutes = body.map(|b| b.0.minutes).unwrap_or_default();
    s.lock().begin_resolution(minutes)?;
    // Spawned, so the turn completes even if the client goes away; the
    // response still waits for the report.
    let report = tokio::spawn(flow::run_resolution(s.clone())).await.map_err(|e| bad(e.to_string()))??;
    let mut events: Vec<String> = report.applied.iter().map(|a| format!("{} ({}): {}", a.action, a.by, if a.ok { "done" } else { &a.detail })).collect();
    events.extend(report.events.iter().map(|e| e.text.clone()));
    let approved = s.lock().proposals.iter().filter(|p| p.status == ProposalStatus::Applied && !p.communications_claimed).map(|p| p.id).collect::<Vec<_>>();
    for pid in approved { let st = s.clone(); tokio::spawn(async move { flow::send_drafts(&st, pid).await; }); }
    tokio::spawn(flow::situation_updates(s.clone(), events));
    Ok(Json(json!(report)))
}

#[derive(Deserialize)]
struct RollbackBody {
    turn: u32,
}

async fn post_rollback(State(s): State<AppState>, Json(b): Json<RollbackBody>) -> ApiResult {
    s.lock().rollback(b.turn)?;
    Ok(Json(json!({"ok": true})))
}

async fn post_end(State(s): State<AppState>) -> ApiResult {
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

async fn post_settings(State(s): State<AppState>, Json(b): Json<SettingsBody>) -> ApiResult {
    let mut ex = s.lock();
    if let Some(m) = b.turn_minutes {
        let m = m.clamp(1, 24 * 60);
        ex.turn_minutes = m;
        ex.record("settings", exercise::MASTER, json!({"turn_minutes": m}));
    }
    if let Some(v) = b.speed {
        let v = if v.is_finite() { v.clamp(0.0, 86_400.0) } else { 0.0 };
        ex.speed = v;
        ex.record("settings", exercise::MASTER, json!({"speed": v}));
    }
    Ok(Json(json!({"turn_minutes": ex.turn_minutes, "speed": ex.speed})))
}

// ---------------------------------------------------------------- ws

#[derive(Deserialize)]
struct WsQuery {
    #[allow(dead_code)]
    role: Option<String>,
}

async fn ws(State(s): State<AppState>, Query(_q): Query<WsQuery>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(move |socket| ws_loop(socket, s.generation.clone()))
}

async fn ws_loop(mut socket: WebSocket, mut rx: watch::Receiver<u64>) {
    let g = *rx.borrow();
    if socket.send(WsMessage::Text(json!({"generation": g}).to_string())).await.is_err() {
        return;
    }
    while rx.changed().await.is_ok() {
        let g = *rx.borrow();
        if socket.send(WsMessage::Text(json!({"generation": g}).to_string())).await.is_err() {
            return;
        }
    }
}
