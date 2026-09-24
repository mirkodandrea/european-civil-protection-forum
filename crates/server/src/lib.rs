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
use axum::{Json, Router};
use exercise::{Decision, Exercise, ProposalStatus};
use llm::Llm;
use serde::Deserialize;
use serde_json::{json, Value};
use sim::Command;
use tokio::sync::watch;

pub mod flow;

#[derive(Clone)]
pub struct AppState {
    pub ex: Arc<Mutex<Exercise>>,
    pub llm: Llm,
    generation: watch::Receiver<u64>,
}

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, Exercise> {
        self.ex.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Build the application router. Spawns the generation watcher, so call it
/// inside a Tokio runtime.
pub fn router(ex: Arc<Mutex<Exercise>>, llm: Llm, web_dir: Option<PathBuf>) -> (Router, AppState) {
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
    let state = AppState { ex, llm, generation: rx };
    let api = Router::new()
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
        .with_state(state.clone());
    let app = match web_dir {
        Some(dir) => api.fallback_service(tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true)),
        None => api,
    };
    (app.layer(tower_http::cors::CorsLayer::permissive()), state)
}

/// Serve until the process exits.
pub async fn serve(ex: Arc<Mutex<Exercise>>, llm: Llm, web_dir: PathBuf, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let (app, _) = router(ex, llm, Some(web_dir));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("web UI on http://{addr}  (master: /master.html, players: /player.html)");
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

async fn get_static(State(s): State<AppState>) -> ApiResult {
    let ex = s.lock();
    let mut v = ex.world.static_view();
    v["msel"] = json!(ex.world.scn.msel);
    Ok(Json(v))
}

async fn get_state(State(s): State<AppState>) -> ApiResult {
    let ex = s.lock();
    Ok(Json(json!({
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
        .map(|p| json!({"id": p.id, "turn": p.turn, "status": p.status, "original": p.original}))
        .collect();
    Ok(Json(json!({
        "player": player,
        "role": ex.world.scn.role(&role),
        "turn": ex.turn,
        "phase": ex.phase,
        "clock": ex.world.clock(),
        "inbox": ex.inbox(&role),
        "resources": ex.world.d.pools.get(&role),
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

async fn post_reply(State(s): State<AppState>, Path(role): Path<String>, Json(b): Json<TextBody>) -> ApiResult {
    let pid = s.lock().player_reply(&role, &b.text)?;
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

async fn post_start(State(s): State<AppState>) -> ApiResult {
    s.lock().start()?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct InjectBody {
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
    let ids = flow::send_inject(&s, None, &b.title, &b.sender, &b.recipients, &b.text).await?;
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
        let outcome = ex.decide(id, Decision { approve: b.approve, commands, note: b.note })?;
        let p = ex.proposal(id).unwrap();
        let drafts = if b.approve && p.status == ProposalStatus::Applied { p.inject_drafts.clone() } else { Vec::new() };
        (outcome, drafts)
    };
    if let Some(role) = outcome.notify.clone() {
        tokio::spawn(flow::send_feedback(s.clone(), role, id, outcome.facts_en.clone()));
    }
    if !drafts.is_empty() {
        let st = s.clone();
        tokio::spawn(async move { flow::send_drafts(&st, &drafts).await });
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
