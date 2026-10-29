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

/// One state-changing request, kept so a reload can play the session again.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub role: String,
    pub url: String,
    pub body: Value,
    /// Simulated seconds when the request was made.
    pub t_s: i64,
    #[serde(skip)]
    seq: u64,
    /// Still running when false; such a request is not part of the saved session.
    #[serde(skip)]
    done: bool,
}

thread_local! {
    static JOURNAL: std::cell::RefCell<(u64, Vec<JournalEntry>)> = const { std::cell::RefCell::new((0, Vec::new())) };
    static TAPE: Arc<llm::cassette::Cassette> = Arc::new(llm::cassette::Cassette::memory([]));
}

/// The requests applied so far, oldest first.
pub fn journal() -> Vec<JournalEntry> {
    JOURNAL.with(|j| j.borrow().1.iter().filter(|e| e.done).cloned().collect())
}

/// The AI answers received so far as `(key, answer)`. Replays use them instead
/// of calling the model again.
pub fn tape_entries() -> Vec<(String, String)> {
    TAPE.with(|t| t.entries())
}

/// Preload AI answers (before replaying a saved session).
pub fn load_tape(entries: Vec<(String, String)>) {
    TAPE.with(|t| {
        for (key, answer) in entries {
            t.put(&key, "restored", &answer);
        }
    });
}

fn tape() -> Arc<llm::cassette::Cassette> {
    TAPE.with(Arc::clone)
}

/// The browser keeps its journal in `localStorage` (a few MB). A PDF above this
/// many base64 characters is replaced there by a minimal valid stub, so a
/// replay still produces the same exercise but the document itself is gone.
const JOURNAL_PDF_CHARS: usize = 512 * 1024;
const STUB_PDF: &str = "%PDF-1.4\n% document not kept in the browser save\n";

fn journal_body(mut body: Value) -> Value {
    use base64::Engine;
    if let Some(a) = body.get_mut("attachment").filter(|a| a.is_object()) {
        if a["data"].as_str().is_some_and(|d| d.len() > JOURNAL_PDF_CHARS) {
            a["data"] = Value::String(
                base64::engine::general_purpose::STANDARD.encode(STUB_PDF),
            );
        }
    }
    body
}

fn journaled(url: &str) -> bool {
    !url.contains("/presence") && !url.contains("/api/master/scenarios")
}

/// Play `entries` into a fresh exercise, reaching simulated time `end_t_s`
/// with the clock paused. Fails at the first request that is no longer accepted,
/// since the state would then differ from the saved one.
pub async fn replay(s: AppState, entries: Vec<JournalEntry>, end_t_s: i64) -> Result<(), String> {
    use std::sync::atomic::Ordering::Relaxed;
    flow::REPLAYING.store(true, Relaxed);
    let outcome = async {
        for (i, e) in entries.iter().enumerate() {
            flow::advance_to(&s, e.t_s).await;
            dispatch(s.clone(), &e.role, &e.url, Some(e.body.clone()))
                .await
                .map_err(|err| format!("passo {} ({}): {}", i + 1, e.url, err.1))?;
            flow::settle(&s).await;
        }
        flow::advance_to(&s, end_t_s).await;
        Ok(())
    }
    .await;
    {
        let mut ex = s.lock();
        if ex.clock_control.running {
            let ratio = ex.clock_control.ratio;
            let _ = ex.configure_clock(false, ratio, "Session restored; clock paused");
        }
    }
    flow::REPLAYING.store(false, Relaxed);
    if outcome.is_ok() {
        JOURNAL.with(|j| {
            let mut j = j.borrow_mut();
            j.1 = entries;
            let n = j.1.len() as u64;
            for (i, e) in j.1.iter_mut().enumerate() {
                e.seq = i as u64;
                e.done = true;
            }
            j.0 = n;
        });
    }
    outcome
}

pub fn new(ex: Exercise, scenarios: Option<Arc<library::Scenarios>>) -> AppState {
    let access = Access::local(ex.role_ids());
    let state = AppState {
        ex: Arc::new(Mutex::new(ex)),
        llm: Llm::off()
            .with_shared_cassette(tape())
            .with_runtime_config(),
        access: Arc::new(access),
        view_frames: Default::default(),
        assessments: Default::default(),
        forecasts: Default::default(),
        forecast_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        situation: Default::default(),
        presence: Default::default(),
        text_cache: Default::default(),
        scenarios,
    };
    tokio::spawn(flow::run_clock(state.clone()));
    tokio::spawn(flow::run_excon_translator(state.clone()));
    state
}

/// Reconfiguration affects later requests; credentials never enter Exercise/logs.
pub fn configure(
    state: &mut AppState,
    key: &str,
    model: &str,
    translator: &str,
    providers: &[String],
) {
    let configured = if key.trim().is_empty() {
        Llm::off()
    } else {
        Llm::openrouter(
            key.trim().into(),
            model.trim().into(),
            llm::DEFAULT_BASE_URL.into(),
        )
        .with_providers(providers.to_vec())
        .with_translation_model(translator.trim())
    }
    .with_shared_cassette(tape());
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
    // Recorded at the start, so the order matches what was asked; dropped again
    // if the request fails and therefore changed nothing.
    let seq = body.clone().filter(|_| journaled(url)).map(journal_body).map(|body| {
        let t_s = s.lock().world.time_s();
        JOURNAL.with(|j| {
            let mut j = j.borrow_mut();
            j.0 += 1;
            let seq = j.0;
            j.1.push(JournalEntry {
                role: role.into(),
                url: url.into(),
                body,
                t_s,
                seq,
                done: false,
            });
            seq
        })
    });
    let result = dispatch(s, role, url, body)
        .await
        .map(|v| v.0)
        .map_err(|e| e.1);
    if let Some(seq) = seq {
        JOURNAL.with(|j| {
            let mut j = j.borrow_mut();
            if result.is_ok() {
                if let Some(e) = j.1.iter_mut().find(|e| e.seq == seq) {
                    e.done = true;
                }
            } else {
                j.1.retain(|e| e.seq != seq);
            }
        });
    }
    result
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
        player if s.access.has_player(player) => Principal::Player {
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
        ("POST", "/api/master/knowledge/refresh") => {
            post_knowledge_refresh(
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
        ("GET", "/api/attachments/<id>") => {
            get_attachment(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid document id"))?,
            )
            .await
        }
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
        ("GET", "/api/master/scenarios") => {
            library::get_scenarios(principal.clone(), s.clone()).await
        }
        ("GET", "/api/master/scenarios/<id>") => {
            library::get_scenario(principal.clone(), s.clone(), captures["id"].to_string()).await
        }
        ("POST", "/api/master/scenarios/validate") => {
            library::post_validate(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/scenarios/save") => {
            library::post_save(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/scenarios/delete") => {
            library::post_delete_scenario(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("GET", "/api/view/scene") => get_view_scene(principal.clone(), s.clone()).await,
        ("GET", "/api/view/state") => {
            let have: ::view::Have = serde_urlencoded::from_str(query).unwrap_or_default();
            get_view_state(
                principal.clone(),
                s.clone(),
                Some(have.generation),
                Some(have.cue),
            )
            .await
        }
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
        ("POST", "/api/master/proposals/<id>/narrative-outcome") => narrative::outcome(principal.clone(), s.clone(), captures["id"].parse().map_err(|_| bad("invalid proposal id"))?, decode(body.clone().ok_or_else(|| bad("request body required"))?)?).await,
        ("POST", "/api/player/<role>/conversations") => conversations::start(principal.clone(), s.clone(), captures["role"].into(), decode(body.clone().ok_or_else(|| bad("request body required"))?)?).await,
        ("POST", "/api/player/<role>/conversations/<id>/messages") => conversations::post_message(principal.clone(), s.clone(), captures["role"].into(), captures["id"].parse().map_err(|_| bad("invalid conversation id"))?, decode(body.clone().ok_or_else(|| bad("request body required"))?)?).await,
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
        ("POST", "/api/master/start") => {
            post_start(principal.clone(), s.clone(), body.clone().map(decode).transpose()?).await
        }
        ("POST", "/api/master/inject") => {
            post_inject(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/msel/<id>/send") => {
            let b = match body.clone() {
                Some(v) if !v.is_null() => Some(decode(v)?),
                _ => None,
            };
            post_msel_send(principal.clone(), s.clone(), captures["id"].to_string(), b).await
        }
        ("POST", "/api/master/msel/<id>/skip") => {
            post_msel_skip(principal.clone(), s.clone(), captures["id"].to_string()).await
        }
        ("POST", "/api/master/msel/<id>/restore") => {
            post_msel_restore(principal.clone(), s.clone(), captures["id"].to_string()).await
        }
        ("POST", "/api/master/msel/<id>/actions/<action>/enabled") => {
            post_action_enabled(
                principal.clone(),
                s.clone(),
                captures["id"].to_string(),
                captures["action"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
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
        ("POST", "/api/master/proposals/<id>/clarify") => {
            post_clarify(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/clarifications/<id>/close") => {
            post_close_clarification(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
            )
            .await
        }
        ("POST", "/api/master/proposals/<id>/revalidate") => {
            post_revalidate(
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
        ("POST", "/api/master/resolve/stop") => {
            post_resolve_stop(principal.clone(), s.clone()).await
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
        ("POST", "/api/player/<role>/suggest-actions") => {
            action_suggestions::player(
                principal.clone(),
                s.clone(),
                captures["role"].to_string(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/proposals/<id>/suggest-actions") => {
            action_suggestions::excon(
                principal.clone(),
                s.clone(),
                captures["id"].parse().map_err(|_| bad("invalid id"))?,
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
        ("GET", "/api/master/impact") => situation::get_impact(principal.clone(), s.clone()).await,
        ("GET", "/api/master/situation/preview") => {
            let have = serde_urlencoded::from_str::<BTreeMap<String, String>>(query)
                .map_err(|e| bad(e.to_string()))?
                .remove("have")
                .and_then(|h| h.parse().ok());
            situation::get_preview(principal.clone(), s.clone(), have).await
        }
        ("GET", "/api/master/situation/suggestions") => {
            situation::get_suggestions(principal.clone(), s.clone()).await
        }
        ("POST", "/api/master/situation/mark") => {
            situation::post_mark(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/situation/plan") => {
            situation::post_plan(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/situation/rewrite") => {
            situation::post_rewrite(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/situation/draft") => {
            situation::post_draft(
                principal.clone(),
                s.clone(),
                decode(body.clone().ok_or_else(|| bad("request body required"))?)?,
            )
            .await
        }
        ("POST", "/api/master/situation/brief") => {
            situation::post_brief(
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
    ("POST", "/api/master/knowledge/refresh"),
    ("POST", "/api/master/references"),
    ("GET", "/api/evaluator"),
    ("GET", "/api/state"),
    ("GET", "/api/master"),
    ("GET", "/api/attachments/<id>"),
    ("GET", "/api/player/<role>"),
    ("POST", "/api/player/<role>/presence"),
    ("GET", "/api/player/<role>/presence"),
    ("GET", "/api/catalogue"),
    ("GET", "/api/master/scenarios"),
    ("GET", "/api/master/scenarios/<id>"),
    ("POST", "/api/master/scenarios/validate"),
    ("POST", "/api/master/scenarios/save"),
    ("POST", "/api/master/scenarios/delete"),
    ("GET", "/api/view/scene"),
    ("GET", "/api/view/state"),
    ("GET", "/api/debrief"),
    ("POST", "/api/player/<role>/chat"),
    ("POST", "/api/master/chat"),
    ("POST", "/api/player/<role>/reply"),
    ("POST", "/api/player/<role>/actions"),
    ("POST", "/api/player/<role>/conversations"),
    ("POST", "/api/player/<role>/conversations/<id>/messages"),
    ("POST", "/api/master/proposals/<id>/narrative-outcome"),
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
    ("POST", "/api/master/msel/<id>/skip"),
    ("POST", "/api/master/msel/<id>/restore"),
    ("POST", "/api/master/msel/<id>/actions/<action>/enabled"),
    ("POST", "/api/master/prompt"),
    ("POST", "/api/master/proposals/<id>/decide"),
    ("POST", "/api/master/proposals/<id>/clarify"),
    ("POST", "/api/master/clarifications/<id>/close"),
    ("POST", "/api/master/proposals/<id>/revalidate"),
    ("POST", "/api/master/preview"),
    ("POST", "/api/master/correct-intervention"),
    ("POST", "/api/master/design"),
    ("POST", "/api/master/path"),
    ("POST", "/api/master/preview/apply"),
    ("POST", "/api/master/command"),
    ("POST", "/api/master/resolve"),
    ("POST", "/api/master/resolve/stop"),
    ("POST", "/api/master/rollback"),
    ("POST", "/api/master/end"),
    ("POST", "/api/master/settings"),
    ("POST", "/api/player/<role>/suggest-actions"),
    ("POST", "/api/master/proposals/<id>/suggest-actions"),
    ("POST", "/api/master/forecast"),
    ("POST", "/api/master/forecast/publish"),
    ("GET", "/api/master/impact"),
    ("GET", "/api/master/situation/preview"),
    ("GET", "/api/master/situation/suggestions"),
    ("POST", "/api/master/situation/mark"),
    ("POST", "/api/master/situation/plan"),
    ("POST", "/api/master/situation/rewrite"),
    ("POST", "/api/master/situation/draft"),
    ("POST", "/api/master/situation/brief"),
    ("GET", "/api/evaluator/assessment"),
];
