//! Local transport for the main-branch UI; all rules remain in existing controllers.
use super::*;
pub struct State<T>(pub T);
pub struct Extension<T>(pub T);
pub struct Path<T>(pub T);
pub struct Query<T>(pub T);
pub struct Json<T>(pub T);
impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
pub struct StatusCode;
impl StatusCode {
    pub const BAD_REQUEST: Self = Self;
    pub const CONFLICT: Self = Self;
}
pub fn new(ex: Exercise) -> AppState {
    let access = Access::new(ex.role_ids());
    let state = AppState {
        ex: Arc::new(Mutex::new(ex)),
        llm: Llm::off().with_runtime_config(),
        access: Arc::new(access),
        assessments: Default::default(),
    };
    tokio::spawn(flow::run_clock(state.clone()));
    state
}
fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ApiError> {
    serde_json::from_value(v).map_err(|e| bad(e.to_string()))
}
pub async fn request(
    s: AppState,
    role: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    dispatch(s, role, url, body)
        .await
        .map(|j| j.0)
        .map_err(|e| e.1)
}
async fn dispatch(s: AppState, role: &str, url: &str, body: Option<Value>) -> ApiResult {
    let principal = match role {
        "master" => Principal::Master,
        "evaluator" => Principal::Evaluator {
            id: "evaluator-1".into(),
        },
        r if s.access.players.contains_key(r) => Principal::Player { role: r.into() },
        _ => return Err(bad("unknown role")),
    };
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if !access::allows(&principal, path, body.is_none()) {
        return Err(bad("role does not authorize this endpoint"));
    }
    let segments: Vec<_> = path.split('/').collect();
    let method = if body.is_some() { "post" } else { "get" };
    match (method, segments.as_slice()) {
        ("get", ["", "api", "access"]) => get_access(Extension(principal.clone())).await,
        ("get", ["", "api", "master", "access"]) => get_access_links(State(s.clone())).await,
        ("post", ["", "api", "master", "knowledge"]) => {
            post_knowledge(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "references"]) => {
            post_references(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "resource-limits"]) => {
            post_resource_limits(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "clock"]) => {
            post_clock(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "phase"]) => {
            post_phase(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "schedules"]) => {
            post_schedule(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "schedules", id, "cancel"]) => {
            post_cancel_schedule(
                State(s.clone()),
                Path(id.parse().map_err(|_| bad("invalid id"))?),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "proposals", id, "execute"]) => {
            post_execute(
                State(s.clone()),
                Path(id.parse().map_err(|_| bad("invalid id"))?),
            )
            .await
        }
        ("get", ["", "api", "evaluator"]) => {
            get_evaluator(
                State(s.clone()),
                Query(serde_urlencoded::from_str(query).map_err(|e| bad(e.to_string()))?),
            )
            .await
        }
        ("get", ["", "api", "evaluator", "assessment"]) => {
            assessment::get(
                State(s.clone()),
                Query(serde_urlencoded::from_str(query).map_err(|e| bad(e.to_string()))?),
            )
            .await
        }
        ("get", ["", "api", "static"]) => {
            get_static(State(s.clone()), Extension(principal.clone())).await
        }
        ("get", ["", "api", "state"]) => get_state(State(s.clone())).await,
        ("get", ["", "api", "master"]) => get_master(State(s.clone())).await,
        ("get", ["", "api", "player", role]) => {
            get_player(State(s.clone()), Path(role.to_string())).await
        }
        ("post", ["", "api", "player", role, "reply"]) => {
            post_reply(
                State(s.clone()),
                Path(role.to_string()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "player", role, "language"]) => {
            post_language(
                State(s.clone()),
                Path(role.to_string()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "start"]) => post_start(State(s.clone())).await,
        ("post", ["", "api", "master", "inject"]) => {
            post_inject(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "msel", id, "send"]) => {
            post_msel_send(State(s.clone()), Path(id.to_string())).await
        }
        ("post", ["", "api", "master", "prompt"]) => {
            post_prompt(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "proposals", id, "decide"]) => {
            post_decide(
                State(s.clone()),
                Path(id.parse().map_err(|_| bad("invalid id"))?),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "command"]) => {
            post_command(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "preview"]) => {
            post_preview(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "preview", "apply"]) => {
            post_preview_apply(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "correct-intervention"]) => {
            post_correction(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "design"]) => {
            post_design(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "path"]) => {
            post_path(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "resolve"]) => {
            post_resolve(
                State(s.clone()),
                body.clone().map(decode).transpose()?.map(Json),
            )
            .await
        }
        ("post", ["", "api", "master", "rollback"]) => {
            post_rollback(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("post", ["", "api", "master", "end"]) => post_end(State(s.clone())).await,
        ("post", ["", "api", "master", "settings"]) => {
            post_settings(
                State(s.clone()),
                Json(decode(body.clone().ok_or_else(|| bad("body required"))?)?),
            )
            .await
        }
        ("get", ["", "api", "catalogue"]) => get_catalogue(State(s.clone())).await,
        ("get", ["", "api", "debrief"]) => get_debrief(State(s.clone())).await,
        _ => Err(bad("unknown operation")),
    }
}
