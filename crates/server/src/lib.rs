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
#[cfg(not(target_arch = "wasm32"))]
pub mod persist;
mod decision_drafts;
pub mod flow;
mod forecast;
pub mod library;
pub mod view;

#[derive(Clone)]
pub struct AppState {
    pub ex: Arc<Mutex<Exercise>>,
    pub llm: Llm,
    pub access: Arc<Access>,
    #[cfg(not(target_arch = "wasm32"))]
    generation: watch::Receiver<u64>,
    /// Recent view states, the bases of differential `/api/view/frame` answers.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    view_frames: Arc<Mutex<::view::wire::FrameCache>>,
    assessments: Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, Value>>>,
    forecasts: Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, forecast::Draft>>>,
    forecast_slot: Arc<tokio::sync::Semaphore>,
    presence: Arc<Mutex<std::collections::BTreeMap<String, (String, Instant)>>>,
    /// Saved session; `None` keeps everything in memory.
    #[cfg(not(target_arch = "wasm32"))]
    persist: Option<Arc<persist::Persist>>,
    /// Scenario packs the EXCON can edit and start; `None` hides the editor.
    pub scenarios: Option<Arc<library::Scenarios>>,
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
    router_inner(ex, llm, web_dir, access, None, None)
}

/// Like [`router`], with the scenario library (but no saved session).
#[cfg(not(target_arch = "wasm32"))]
pub fn router_with_scenarios(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: Option<PathBuf>,
    scenarios: Arc<library::Scenarios>,
) -> (Rocket<Build>, AppState) {
    let access = Access::new(ex.lock().unwrap_or_else(|p| p.into_inner()).role_ids());
    router_inner(ex, llm, web_dir, access, None, Some(scenarios))
}

/// Like [`router`], saving the session through `persist`, which also brings
/// the scenario library.
#[cfg(not(target_arch = "wasm32"))]
pub fn router_persistent(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: Option<PathBuf>,
    persist: Arc<persist::Persist>,
) -> (Rocket<Build>, AppState) {
    let access = Access::new(ex.lock().unwrap_or_else(|p| p.into_inner()).role_ids());
    let scenarios = Some(persist.scenarios.clone());
    router_inner(ex, llm, web_dir, access, Some(persist), scenarios)
}

#[cfg(not(target_arch = "wasm32"))]
fn router_inner(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: Option<PathBuf>,
    access: Access,
    persist: Option<Arc<persist::Persist>>,
    scenarios: Option<Arc<library::Scenarios>>,
) -> (Rocket<Build>, AppState) {
    // Answers of the AI are kept with the session, unless a demo cassette is set.
    let llm = match &persist {
        Some(p) if !llm.has_cassette() => llm.with_shared_cassette(p.tape.clone()),
        _ => llm,
    };
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
        view_frames: Default::default(),
        assessments: Default::default(),
        forecasts: Default::default(),
        forecast_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        presence: Default::default(),
        persist,
        scenarios,
    };
    tokio::spawn(flow::run_clock(state.clone()));
    if state.persist.is_some() {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Some(p) = &state.persist {
                    p.autosave(&state, false);
                }
            }
        });
    }
    let app = assemble(&state, web_dir, state.persist.is_some());
    (app, state)
}

/// The Rocket application over `state`. A second instance (no files, no
/// journal) serves the replay of a saved session.
#[cfg(not(target_arch = "wasm32"))]
pub fn assemble(state: &AppState, web_dir: Option<PathBuf>, journal: bool) -> Rocket<Build> {
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
                get_due_actions,
                get_player,
                post_presence,
                get_presence,
                decision_drafts::prepare,
                decision_drafts::submit,
                post_player_chat,
                post_master_chat,
                post_reply,
                post_player_actions,
                post_language,
                post_start,
                post_inject,
                post_msel_send,
                post_prompt,
                post_decide,
                post_clarify,
                post_close_clarification,
                post_revalidate,
                post_command,
                post_preview,
                post_preview_apply,
                post_correction,
                post_design,
                post_path,
                post_resolve,
                post_resolve_stop,
                post_rollback,
                post_end,
                post_settings,
                persist::get_storage,
                persist::post_delete,
                library::get_scenarios,
                library::get_scenario,
                library::post_validate,
                library::post_save,
                library::post_delete_scenario,
                library::post_activate,
                get_catalogue,
                get_view_scene,
                get_view_state,
                get_view_frame,
                get_view_scene_z,
                get_debrief,
                ws
            ],
        )
        .register("/", rocket::catchers![api_catcher]);
    if journal {
        app = app.attach(persist::JournalFairing);
    }
    if let Some(dir) = web_dir {
        app = app.mount("/", rocket::fs::FileServer::from(dir));
    }
    app
}

/// A fresh Rocket over `state`, for replaying a saved session.
#[cfg(not(target_arch = "wasm32"))]
pub fn replay_rocket(state: &AppState) -> Rocket<Build> {
    assemble(state, None, false)
}

/// Serve until the process exits.
#[cfg(not(target_arch = "wasm32"))]
pub async fn serve(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: PathBuf,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    serve_with(ex, llm, web_dir, addr, None, None).await
}

/// [`serve`] with storage: a saved `session` is replayed before listening;
/// if it can no longer be replayed it is dropped and the exercise starts clean.
#[cfg(not(target_arch = "wasm32"))]
pub async fn serve_with(
    ex: Arc<Mutex<Exercise>>,
    llm: Llm,
    web_dir: PathBuf,
    addr: std::net::SocketAddr,
    storage: Option<(Arc<persist::Persist>, Option<persist::SavedSession>)>,
    scenarios: Option<Arc<library::Scenarios>>,
) -> anyhow::Result<()> {
    let (app, _state) = match storage {
        None => match scenarios {
            Some(sc) => router_with_scenarios(ex, llm, Some(web_dir), sc),
            None => router(ex, llm, Some(web_dir)),
        },
        Some((persist, session)) => {
            let (app, state) = router_persistent(ex, llm, Some(web_dir), persist.clone());
            if let Some(session) = session {
                let steps = session.journal.len();
                tracing::info!("restoring the saved session ({steps} steps)");
                if let Err(e) = persist::restore(&state, replay_rocket(&state), session).await {
                    tracing::warn!("saved session cannot be restored and was discarded: {e}");
                    let pack = persist
                        .scenarios
                        .running_pack()
                        .ok_or_else(|| anyhow::anyhow!("no scenario to restart from"))?;
                    persist::reset(&state, &pack)
                        .await
                        .map_err(anyhow::Error::msg)?;
                }
            }
            (app, state)
        }
    };
    println!(
        "EXCON console: http://localhost:{}/play.html (no access keys: keep the port on a trusted network)",
        addr.port()
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
    v["excon_roles"] = json!(ex.world.scn.excon_roles);
    Ok(Json(v))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/access"))]
async fn get_access(principal: Principal) -> ApiResult {
    Ok(Json(json!(principal)))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master/access"))]
async fn get_access_links(_principal: Principal, s: AppState) -> ApiResult {
    Ok(Json(
        json!({"players": s.access.player_ids().iter().map(|role| json!({"role":role,"url":format!("/player.html?role={role}")})).collect::<Vec<_>>(), "evaluators":s.access.evaluators.keys().map(|id| json!({"id":id,"url":format!("/evaluator.html?evaluator={id}")})).collect::<Vec<_>>()}),
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
    /// Zones to disclose, with `layers` among fire/buildings/roads/lines/units (default: fire).
    #[serde(default)]
    zones: Vec<String>,
    #[serde(default)]
    layers: Vec<String>,
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
        if [b.current_perimeter, b.observation.is_some(), !b.zones.is_empty()]
            .into_iter()
            .filter(|x| *x)
            .count()
            > 1
        {
            return Err(bad("choose an observation, current_perimeter or zones"));
        }
        let observation = if !b.zones.is_empty() {
            let layers = if b.layers.is_empty() {
                vec!["fire".to_string()]
            } else {
                b.layers.clone()
            };
            ex.zone_observation(&b.zones, &layers)?
        } else if b.current_perimeter {
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
        "resolving": ex.resolving.as_ref().map(|r| json!({"turn": r.turn, "from_s": r.from_s, "to_s": r.to_s, "minutes": r.minutes, "partial": r.partial})),
        "turn_window": {"start_s": ex.turn_start_s, "planned_end_s": ex.planned_end_s(), "now_s": ex.world.time_s(), "substeps": ex.substeps},
        "generation": ex.generation,
        "last_turn": ex.last_turn,
        "players": ex.players,
        "sent_injects": ex.sent_injects,
        "planned_injects": ex.planned_injects(),
        "llm": {"available": s.llm.is_available(), "model": s.llm.model(), "translation_model":s.llm.model_for("translate")},
        "world": ex.world.view(),
        "resource_accounting": ex.resource_accounting(None),
    })))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master"))]
async fn get_master(_principal: Principal, s: AppState) -> ApiResult {
    let ex = s.lock();
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
    let log = ex
        .log
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
        "planned_injects": ex.planned_injects(),
        "timeline_actions": ex.timeline_actions(),
        "agenda": ex.turn_agenda(),
        "inject_responses": ex.inject_responses(),
        "clarification_threads": ex.clarification_threads(),
        "sent_injects": ex.sent_injects,
        "messages": messages,
        "log": log,
        "training_design":ex.design_view(),
        "correction":ex.correction_view(),
        "digest": ex.world.digest(None),
        "conversations": std::collections::BTreeMap::from([(exercise::MASTER, ex.conversation(exercise::MASTER))]),
        "role_knowledge": ex.role_knowledge(),
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
        .map(|p| {
            let thread = p.thread.iter().filter_map(|entry| {
                let message = ex.messages.iter().find(|m| m.id == entry.id)?;
                if message.from != role && message.to != role { return None; }
                let mut visible = entry.clone();
                visible.text = if message.from == role {
                    message.original.as_ref().unwrap_or(&message.text).clone()
                } else { message.text.clone() };
                Some(visible)
            }).collect::<Vec<_>>();
            json!({"id": p.id, "turn": p.turn, "t_s":p.t_s, "status": p.status, "original": p.original, "inject_id": p.inject_id, "reply_to": p.reply_to, "thread":thread, "declaration_kind": p.declaration_kind, "history": p.history, "clarification_request": p.clarification_request, "decision": p.decision.as_ref().map(|d| json!({"approve":d.approve, "note":d.note, "t_s":d.t_s})), "executions": p.executions, "evidence": p.evidence})
        })
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
        "turn_summaries": ex.player_turn_summaries(&role),
        "resources": ex.world.d.pools.get(&role),
        "resource_accounting": ex.resource_accounting(Some(&role))[&role],
        "deployments":ex.world.d.tasks.iter().filter(|t|t.role==role).map(|t|json!({"task":t,"position_m":t.position(ex.world.time_s()),"active_qty":ex.world.task_active_qty(t)})).collect::<Vec<_>>(),
        "allocations": ex.world.d.allocations.iter().filter(|a| a.role == role).collect::<Vec<_>>(),
        "proposals": proposals,
        "clarification_threads": ex.player_clarification_threads(&role),
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
    if !s.access.has_player(&body.to) || body.to == role {
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

/// The static world for a viewer (fetched once). EXCON only, like the rest of
/// the view: it shows the ground truth.
#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/view/scene"))]
async fn get_view_scene(_principal: Principal, s: AppState) -> ApiResult {
    let scn = s.lock().world.scn.clone();
    Ok(Json(
        serde_json::to_value(&*scn).map_err(|e| bad(e.to_string()))?,
    ))
}

/// The newest view state, or `null` when the viewer already has
/// `generation` (and every cue up to `cue`). See [`view`].
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::get("/api/view/state?<generation>&<cue>")
)]
async fn get_view_state(
    _principal: Principal,
    s: AppState,
    generation: Option<u64>,
    cue: Option<u64>,
) -> ApiResult {
    let have = ::view::Have {
        generation: generation.unwrap_or(0),
        cue: cue.unwrap_or(0),
    };
    let state = view::extract(&s.lock(), have);
    Ok(Json(match state {
        Some(state) => serde_json::to_value(state).map_err(|e| bad(e.to_string()))?,
        None => Value::Null,
    }))
}

/// `/api/view/scene`, zlib-deflated (the scenario is megabytes of JSON).
#[cfg(not(target_arch = "wasm32"))]
#[rocket::get("/api/view/scene.z")]
async fn get_view_scene_z(
    _principal: Principal,
    s: AppState,
) -> Result<(rocket::http::ContentType, Vec<u8>), ApiError> {
    let scn = s.lock().world.scn.clone();
    let json = serde_json::to_vec(&*scn).map_err(|e| bad(e.to_string()))?;
    Ok((rocket::http::ContentType::Binary, ::view::wire::compress(&json)))
}

/// The compact form of `/api/view/state`: only what changed since the viewer's
/// `generation` when that is still known, zlib-compressed (see
/// [`view::wire`]). `204` when the viewer is up to date.
#[cfg(not(target_arch = "wasm32"))]
#[rocket::get("/api/view/frame?<generation>&<cue>")]
async fn get_view_frame(
    _principal: Principal,
    s: AppState,
    generation: Option<u64>,
    cue: Option<u64>,
) -> (Status, (rocket::http::ContentType, Vec<u8>)) {
    let have = ::view::Have {
        generation: generation.unwrap_or(0),
        cue: cue.unwrap_or(0),
    };
    let binary = rocket::http::ContentType::Binary;
    // The lock is held only for the synchronous extraction.
    let Some(new) = view::extract(&s.lock(), have) else {
        return (Status::NoContent, (binary, Vec::new()));
    };
    let frame = s
        .view_frames
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .frame(new, have.generation);
    (Status::Ok, (binary, ::view::wire::encode(&frame)))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/catalogue"))]
async fn get_catalogue(_principal: Principal, s: AppState) -> ApiResult {
    let ex = s.lock();
    let all: Vec<String> = sim::ALL_KINDS.iter().map(|k| k.to_string()).collect();
    Ok(Json(
        json!({"kinds": sim::ALL_KINDS, "master_only": sim::MASTER_ONLY, "doc": sim::catalogue_for(&ex.world.scn, &all), "roles": ex.world.scn.roles}),
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
    #[serde(default)]
    reply_to: Option<u64>,
}

async fn deliver_chat(s: AppState, from: String, b: ChatBody) -> ApiResult {
    let (language, turn, revision) = {
        let ex = s.lock();
        ex.validate_chat_reply(&from, &b.to, &b.text, b.interpret, b.reply_to)?;
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
    let translated = if target_language == language {
        // Peer messages between participants who share a language are literal
        // copies. The English working copy above remains available for logs/AI.
        b.text.clone()
    } else if failed || target_language == "en" {
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
        ex.chat_reply(
            &from,
            &b.to,
            &b.text,
            &text_en,
            &translated,
            failed,
            b.interpret,
            b.reply_to,
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
    if b.to == exercise::MASTER {
        return Err(bad("player chat is only available between players; submit an operational decision for EXCON review"));
    }
    if b.interpret {
        return Err(bad(
            "prepare and confirm the decision privately before sending it",
        ));
    }
    if b.reply_to.is_none() {
        return Err(bad(
            "Select a previous chat message or an inject you received before sending peer chat",
        ));
    }
    deliver_chat(s, role, b).await
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/chat", data = "<b>")
)]
async fn post_master_chat(_principal: Principal, _s: AppState, b: Json<ChatBody>) -> ApiResult {
    let _ = b;
    Err(bad(
        "EXCON communicates through injects; chat is only available between players",
    ))
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
    // An answer to an EXCON question is a message to the EXCON: translated, never interpreted.
    let answering = {
        let ex = s.lock();
        ex.clarification_target(&role, b.reply_to)
            .map(|_| (ex.language_of(&role), ex.turn, ex.clock_control.revision))
    };
    if let Some((language, turn, revision)) = answering {
        let mut failed = false;
        let text_en = if language == "en" || b.text.trim().is_empty() {
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
        let mut ex = s.lock();
        if ex.turn != turn || ex.clock_control.revision != revision {
            return Err(bad("exercise changed during translation; send again"));
        }
        let (message, proposal) =
            ex.answer_clarification(&role, &b.text, b.reply_to, &text_en, failed)?;
        return Ok(Json(
            json!({"message": message, "proposal": proposal, "answered": true, "translation_failed": failed}),
        ));
    }
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
struct PlayerActionsBody {
    text: String,
    commands: Vec<Command>,
    reply_to: Option<u64>,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/actions", data = "<b>")
)]
async fn post_player_actions(
    _principal: Principal,
    s: AppState,
    role: String,
    b: Json<PlayerActionsBody>,
) -> ApiResult {
    let Json(b) = b;
    let pid = s
        .lock()
        .player_actions(&role, &b.text, b.commands, b.reply_to)?;
    Ok(Json(json!({"proposal":pid})))
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

#[derive(Deserialize, Default)]
struct MselSendBody {
    #[serde(default)]
    recipients: Option<Vec<String>>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/msel/<id>/send", data = "<b>")
)]
async fn post_msel_send(
    _principal: Principal,
    s: AppState,
    id: String,
    b: Option<Json<MselSendBody>>,
) -> ApiResult {
    let t = s
        .lock()
        .world
        .scn
        .msel
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .ok_or_else(|| bad(format!("unknown inject {id}")))?;
    // Default to the planned recipients; roles already served are skipped.
    let recipients = match b.and_then(|Json(b)| b.recipients) {
        Some(r) if !r.is_empty() => r,
        _ => t.recipients.clone(),
    };
    let ids = flow::send_inject(&s, Some(&t.id), &t.title, &t.sender, &recipients, &t.text).await?;
    let ex = s.lock();
    Ok(Json(json!({"messages": ids, "sent_to": ex.inject_sent_to.get(&t.id)})))
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
struct ClarifyBody {
    note: String,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/clarify", data = "<b>")
)]
async fn post_clarify(
    _principal: Principal,
    s: AppState,
    id: u64,
    b: Json<ClarifyBody>,
) -> ApiResult {
    let Json(b) = b;
    let req = s.lock().request_clarification(id, &b.note)?;
    tokio::spawn(flow::send_clarification(
        s.clone(),
        req.role.clone(),
        id,
        req.text_en.clone(),
    ));
    Ok(Json(json!(req)))
}

/// EXCON closes a clarification thread (`id` = originating proposal) and goes
/// back to deciding on its latest proposal.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/clarifications/<id>/close")
)]
async fn post_close_clarification(_principal: Principal, s: AppState, id: u64) -> ApiResult {
    let mut ex = s.lock();
    ex.close_clarification(id)?;
    Ok(Json(json!(ex.clarification_thread(id))))
}

#[derive(Deserialize)]
struct RevalidateBody {
    #[serde(default)]
    commands: Option<Vec<Value>>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/revalidate", data = "<b>")
)]
async fn post_revalidate(
    _principal: Principal,
    s: AppState,
    id: u64,
    b: Json<RevalidateBody>,
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
    let mut ex = s.lock();
    ex.revalidate_proposal(id, commands)?;
    Ok(Json(json!(ex.proposal(id))))
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
    /// Decision for each scripted timeline action due in the turn.
    #[serde(default)]
    actions: std::collections::BTreeMap<String, exercise::actions::ActionChoice>,
    /// Intermediate advance: run for `minutes` (or `until`) and keep the turn open.
    #[serde(default)]
    substep: bool,
    /// Run up to `"next_event"` (the next pending inject or action of the
    /// turn) or `"turn_end"` (the planned end) instead of for `minutes`.
    #[serde(default)]
    until: Option<String>,
}

#[derive(Deserialize, Default)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct DueQuery {
    #[serde(default)]
    minutes: Option<u32>,
    #[serde(default)]
    to_s: Option<i64>,
}

/// Scripted timeline actions that fall inside the next turn: EXCON decides
/// on each of them when starting the turn.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::get("/api/master/due-actions?<minutes>&<to_s>")
)]
async fn get_due_actions(
    _principal: Principal,
    s: AppState,
    minutes: Option<u32>,
    to_s: Option<i64>,
) -> ApiResult {
    let ex = s.lock();
    let due = match to_s {
        Some(t) => ex.due_actions_until(t),
        None => ex.due_actions(minutes),
    };
    Ok(Json(json!({"turn": ex.turn, "due": due})))
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
    let body = body.map(|b| b.0).unwrap_or_default();
    let minutes = body.minutes;
    {
        let mut ex = s.lock();
        if body.substep || body.until.is_some() {
            let to_s = match body.until.as_deref() {
                Some("next_event") => ex.turn_agenda()["next_event_s"]
                    .as_i64()
                    .ok_or_else(|| bad("no further event in this turn"))?,
                Some("turn_end") => ex.planned_end_s(),
                Some(other) => return Err(bad(format!("unknown target {other}"))),
                None => ex.world.time_s() + minutes.unwrap_or(1).clamp(1, 24 * 60) as i64 * 60,
            };
            ex.begin_resolution_until(to_s, body.substep, &body.actions)?;
        } else {
            ex.begin_resolution_with(minutes, &body.actions)?;
        }
    }
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

/// End the running resolution at the current simulated second; the turn stays
/// open. The `post_resolve` request still waiting for its report returns.
#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/resolve/stop"))]
async fn post_resolve_stop(_principal: Principal, s: AppState) -> ApiResult {
    let at_s = s.lock().stop_resolution()?;
    Ok(Json(json!({"ok": true, "at_s": at_s})))
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
