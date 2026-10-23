//! In-process dispatcher for the standalone WASM app. Calls the same controllers
//! as HTTP, without starting or contacting any server.
use super::*;
use std::collections::BTreeMap;

pub struct Json<T>(pub T);
impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
#[allow(non_upper_case_globals)]
#[derive(Clone, Copy)]
pub struct Status;
#[allow(non_upper_case_globals)]
impl Status {
    pub const BadRequest: Self = Self;
    pub const Conflict: Self = Self;
    pub const TooManyRequests: Self = Self;
}
fn decode<T: serde::de::DeserializeOwned>(body: Value) -> Result<Json<T>, ApiError> {
    serde_json::from_value(body)
        .map(Json)
        .map_err(|e| bad(e.to_string()))
}

pub fn new(ex: Exercise) -> AppState {
    let access = Access::local(ex.role_ids());
    let state = AppState {
        ex: Arc::new(Mutex::new(ex)),
        llm: Llm::off().with_runtime_config(),
        access: Arc::new(access),
        assessments: Default::default(),
        forecasts: Default::default(),
        forecast_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        presence: Default::default(),
    };
    tokio::spawn(flow::run_clock(state.clone()));
    state
}

/// Reconfiguration affects later requests; credentials never enter Exercise/logs.
pub fn configure(state: &mut AppState, key: &str, model: &str, translator: &str) {
    let configured = if key.trim().is_empty() {
        Llm::off()
    } else {
        Llm::openrouter(
            key.trim().into(),
            model.trim().into(),
            llm::DEFAULT_BASE_URL.into(),
        )
        .with_providers(vec![])
        .with_translation_model(translator.trim())
    };
    state.llm.configure_runtime(configured);
}

fn route<'a>(pattern: &str, path: &'a str) -> Option<BTreeMap<String, &'a str>> {
    let a: Vec<_> = pattern.split('/').collect();
    let b: Vec<_> = path.split('/').collect();
    if a.len() != b.len() {
        return None;
    }
    let mut captures = BTreeMap::new();
    for (a, b) in a.into_iter().zip(b) {
        if a.starts_with('<') && a.ends_with('>') {
            captures.insert(a[1..a.len() - 1].into(), b);
        } else if a != b {
            return None;
        }
    }
    Some(captures)
}

pub async fn request(
    s: AppState,
    role: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    dispatch(s, role, url, body)
        .await
        .map(|v| v.0)
        .map_err(|e| e.1)
}
async fn dispatch(s: AppState, role: &str, url: &str, body: Option<Value>) -> ApiResult {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    // This dispatcher exists only for the all-in-one local WASM build. Its
    // role tabs are the UI's authority; there is no remote client or access
    // key boundary in this process.
    let principal = match role {
        "master" | "advanced" => Principal::Master,
        "evaluator" => Principal::Evaluator {
            id: s
                .access
                .evaluators
                .keys()
                .next()
                .cloned()
                .unwrap_or_default(),
        },
        player if s.access.players.contains_key(player) => Principal::Player {
            role: player.to_string(),
        },
        _ => return Err(bad("unknown standalone role")),
    };
    let method = if body.is_some() { "POST" } else { "GET" };
    // Match path templates before calling the shared typed handlers.
    let patterns = PATTERNS;
    let Some((_, pattern, captures)) = patterns
        .iter()
        .filter(|(m, _)| *m == method)
        .find_map(|(m, p)| route(p, path).map(|c| (*m, *p, c)))
    else {
        return Err(bad("unknown operation"));
    };
    match (method, pattern) {
        ("GET", "/api/static") => get_static(s.clone(), principal.clone()).await,
        ("GET", "/api/access") => get_access(principal.clone()).await,
        ("GET", "/api/master/access") => get_access_links(principal.clone(), s.clone()).await,
        ("POST", "/api/master/knowledge") => {
            post_knowledge(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/references") => {
            post_references(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("GET", "/api/evaluator") => {
            get_evaluator(
                principal.clone(),
                s.clone(),
                EvidenceQuery(serde_urlencoded::from_str(query).map_err(|e| bad(e.to_string()))?),
            )
            .await
        }
        ("GET", "/api/state") => get_state(principal.clone(), s.clone()).await,
        ("GET", "/api/master") => get_master(principal.clone(), s.clone()).await,
        ("GET", "/api/player/<role>") => {
            get_player(principal.clone(), s.clone(), captures["role"].to_string()).await
        }
        ("POST", "/api/player/<role>/presence") => {
            post_presence(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("GET", "/api/player/<role>/presence") => {
            let peer = serde_urlencoded::from_str::<BTreeMap<String, String>>(query)
                .map_err(|e| bad(e.to_string()))?
                .remove("peer")
                .ok_or_else(|| bad("peer required"))?;
            get_presence(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                peer,
            )
            .await
        }
        ("GET", "/api/catalogue") => get_catalogue(principal.clone(), s.clone()).await,
        ("GET", "/api/debrief") => get_debrief(principal.clone(), s.clone()).await,
        ("POST", "/api/player/<role>/chat") => {
            post_player_chat(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/chat") => {
            post_master_chat(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/player/<role>/actions") => {
            post_player_actions(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/player/<role>/reply") => {
            post_reply(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/player/<role>/language") => {
            post_language(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/resource-limits") => {
            post_resource_limits(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/clock") => {
            post_clock(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/phase") => {
            post_phase(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/schedules") => {
            post_schedule(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/schedules/<id>/cancel") => {
            post_cancel_schedule(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/proposals/<id>/execute") => {
            post_execute(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
            )
            .await
        }
        ("POST", "/api/master/start") => post_start(principal.clone(), s.clone()).await,
        ("POST", "/api/master/inject") => {
            post_inject(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/msel/<id>/send") => {
            post_msel_send(principal.clone(), s.clone(), captures["id"].to_string()).await
        }
        ("POST", "/api/master/prompt") => {
            post_prompt(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/proposals/<id>/decide") => {
            post_decide(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/preview") => {
            post_preview(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/correct-intervention") => {
            post_correction(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/design") => {
            post_design(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/path") => {
            post_path(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/preview/apply") => {
            post_preview_apply(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/command") => {
            post_command(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/resolve") => {
            post_resolve(
                principal.clone(),
                s.clone(),
                body.clone().map(decode).transpose()?,
            )
            .await
        }
        ("POST", "/api/master/rollback") => {
            post_rollback(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/end") => post_end(principal.clone(), s.clone()).await,
        ("POST", "/api/master/settings") => {
            post_settings(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/player/<role>/draft") => {
            decision_drafts::prepare(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/player/<role>/draft/submit") => {
            decision_drafts::submit(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/forecast") => {
            forecast::compute(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/forecast/publish") => {
            forecast::publish(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("GET", "/api/evaluator/assessment") => {
            assessment::get(
                principal.clone(),
                s.clone(),
                EvidenceQuery(serde_urlencoded::from_str(query).map_err(|e| bad(e.to_string()))?),
            )
            .await
        }
        _ => Err(bad("unknown operation")),
    }
}
const PATTERNS: &[(&str, &str)] = &[
    ("GET", "/api/static"),
    ("GET", "/api/access"),
    ("GET", "/api/master/access"),
    ("POST", "/api/master/knowledge"),
    ("POST", "/api/master/references"),
    ("GET", "/api/evaluator"),
    ("GET", "/api/state"),
    ("GET", "/api/master"),
    ("GET", "/api/player/<role>"),
    ("POST", "/api/player/<role>/presence"),
    ("GET", "/api/player/<role>/presence"),
    ("GET", "/api/catalogue"),
    ("GET", "/api/debrief"),
    ("POST", "/api/player/<role>/chat"),
    ("POST", "/api/master/chat"),
    ("POST", "/api/player/<role>/reply"),
    ("POST", "/api/player/<role>/actions"),
    ("POST", "/api/player/<role>/language"),
    ("POST", "/api/master/resource-limits"),
    ("POST", "/api/master/clock"),
    ("POST", "/api/master/phase"),
    ("POST", "/api/master/schedules"),
    ("POST", "/api/master/schedules/<id>/cancel"),
    ("POST", "/api/master/proposals/<id>/execute"),
    ("POST", "/api/master/start"),
    ("POST", "/api/master/inject"),
    ("POST", "/api/master/msel/<id>/send"),
    ("POST", "/api/master/prompt"),
    ("POST", "/api/master/proposals/<id>/decide"),
    ("POST", "/api/master/preview"),
    ("POST", "/api/master/correct-intervention"),
    ("POST", "/api/master/design"),
    ("POST", "/api/master/path"),
    ("POST", "/api/master/preview/apply"),
    ("POST", "/api/master/command"),
    ("POST", "/api/master/resolve"),
    ("POST", "/api/master/rollback"),
    ("POST", "/api/master/end"),
    ("POST", "/api/master/settings"),
    ("POST", "/api/player/<role>/draft"),
    ("POST", "/api/player/<role>/draft/submit"),
    ("POST", "/api/master/forecast"),
    ("POST", "/api/master/forecast/publish"),
    ("GET", "/api/evaluator/assessment"),
];
