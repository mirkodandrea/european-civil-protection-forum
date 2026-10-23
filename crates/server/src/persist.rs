//! Server-side storage of the session and of the inject library.
//!
//! A session is saved as a journal: the state-changing requests that were
//! accepted, who made them, the AI answers they received and the simulated
//! time reached. On startup the journal is played again through the real
//! routes (a local Rocket client) into a fresh exercise, answering from the
//! saved AI answers and with the wall clock stood down, like the browser does.
//! The inject library is a plain file read when a new exercise is built.
use super::*;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{ContentType, Header};
use rocket::local::asynchronous::Client;
use rocket::{Data, Response};
use scenario::InjectTemplate;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FORMAT: u32 = 1;
const SESSION_FILE: &str = "session.json";
const INJECTS_FILE: &str = "injects.json";
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// Builds a fresh exercise, optionally with replacement injects.
pub type Factory = Arc<dyn Fn(Option<Vec<InjectTemplate>>) -> anyhow::Result<Exercise> + Send + Sync>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub principal: Principal,
    pub uri: String,
    pub body: String,
    pub t_s: i64,
    #[serde(skip)]
    seq: u64,
    #[serde(skip)]
    done: bool,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    pub v: u32,
    /// Scenario, seed and ensemble options: a session only fits the same setup.
    pub fingerprint: String,
    pub msel: Vec<InjectTemplate>,
    pub journal: Vec<JournalEntry>,
    pub tape: Vec<(String, String)>,
    pub end_t_s: i64,
}

#[derive(Default)]
struct Inner {
    next: u64,
    journal: Vec<JournalEntry>,
    saved: (usize, usize, i64),
    error: Option<String>,
}

pub struct Persist {
    dir: PathBuf,
    fingerprint: String,
    /// Injects of the scenario file, before any library override.
    pub default_msel: Vec<InjectTemplate>,
    factory: Factory,
    pub tape: Arc<llm::cassette::Cassette>,
    inner: Mutex<Inner>,
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Same rules as the editor: unique ids, texts present, known recipients.
pub fn validate_injects(list: &[InjectTemplate], roles: &[String]) -> Result<(), String> {
    for (n, i) in list.iter().enumerate() {
        let at = |what: &str| format!("Inject {}: {what}", n + 1);
        if i.id.trim().is_empty() || i.title.trim().is_empty() || i.text.trim().is_empty() {
            return Err(at("identifier, title and text are required"));
        }
        if list[..n].iter().any(|o| o.id == i.id) {
            return Err(at(&format!("duplicate identifier {}", i.id)));
        }
        if i.recipients.is_empty() {
            return Err(at("at least one recipient (or \"all\") is required"));
        }
        if let Some(bad) = i.recipients.iter().find(|r| *r != "all" && !roles.contains(r)) {
            return Err(at(&format!("unknown recipient {bad}")));
        }
    }
    Ok(())
}

impl Persist {
    pub fn new(
        dir: PathBuf,
        fingerprint: String,
        default_msel: Vec<InjectTemplate>,
        factory: Factory,
    ) -> Arc<Persist> {
        Arc::new(Persist {
            dir,
            fingerprint,
            default_msel,
            factory,
            tape: Arc::new(llm::cassette::Cassette::memory([])),
            inner: Mutex::new(Inner::default()),
        })
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The saved session, if it exists and fits this setup. One that does not
    /// is deleted: replaying it would give a different exercise.
    pub fn load_session(&self) -> Option<SavedSession> {
        let path = self.dir.join(SESSION_FILE);
        let text = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<SavedSession>(&text) {
            Ok(s) if s.v == FORMAT && s.fingerprint == self.fingerprint => Some(s),
            _ => {
                tracing::warn!("saved session {} does not fit this setup: discarded", path.display());
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    pub fn load_injects(&self) -> Option<Vec<InjectTemplate>> {
        serde_json::from_str(&std::fs::read_to_string(self.dir.join(INJECTS_FILE)).ok()?).ok()
    }

    pub fn save_injects(&self, list: &[InjectTemplate]) -> Result<(), String> {
        let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
        write_atomic(&self.dir.join(INJECTS_FILE), &text).map_err(|e| e.to_string())
    }

    pub fn build(&self, msel: Option<Vec<InjectTemplate>>) -> anyhow::Result<Exercise> {
        (self.factory)(msel)
    }

    fn begin(&self, principal: Principal, uri: String, body: String, t_s: i64) -> u64 {
        let mut inner = self.inner();
        inner.next += 1;
        let seq = inner.next;
        inner.journal.push(JournalEntry { principal, uri, body, t_s, seq, done: false });
        seq
    }

    fn finish(&self, seq: u64, ok: bool) {
        let mut inner = self.inner();
        if ok {
            if let Some(e) = inner.journal.iter_mut().find(|e| e.seq == seq) {
                e.done = true;
            }
        } else {
            inner.journal.retain(|e| e.seq != seq);
        }
    }

    /// Write the session if it changed since the last write (`force` skips the check).
    pub fn autosave(&self, state: &AppState, force: bool) {
        if flow::REPLAYING.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let (msel, end_t_s) = {
            let ex = state.lock();
            (ex.world.scn.msel.clone(), ex.world.time_s())
        };
        let tape = self.tape.entries();
        let mut inner = self.inner();
        let journal: Vec<JournalEntry> = inner.journal.iter().filter(|e| e.done).cloned().collect();
        let signature = (journal.len(), tape.len(), end_t_s);
        if !force && inner.saved == signature {
            return;
        }
        inner.saved = signature;
        let path = self.dir.join(SESSION_FILE);
        if journal.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        let session = SavedSession {
            v: FORMAT,
            fingerprint: self.fingerprint.clone(),
            msel,
            journal,
            tape,
            end_t_s,
        };
        inner.error = serde_json::to_string(&session)
            .map_err(|e| e.to_string())
            .and_then(|text| write_atomic(&path, &text).map_err(|e| e.to_string()))
            .err();
        if let Some(e) = &inner.error {
            tracing::warn!("session not saved: {e}");
        }
    }

    fn summary(&self) -> Option<Value> {
        let path = self.dir.join(SESSION_FILE);
        let bytes = std::fs::metadata(&path).ok()?.len();
        let steps = self.inner().journal.iter().filter(|e| e.done).count();
        Some(json!({"steps": steps, "bytes": bytes}))
    }
}

/// Replace the exercise by a fresh one, forgetting the journal and AI answers.
pub async fn reset(state: &AppState, msel: Option<Vec<InjectTemplate>>) -> Result<(), String> {
    let persist = state.persist.clone().ok_or("storage is not enabled")?;
    if state.lock().phase == exercise::Phase::Resolving {
        return Err("a turn is being resolved; wait for it to finish".into());
    }
    let mut fresh = persist.build(msel).map_err(|e| e.to_string())?;
    {
        let mut ex = state.lock();
        fresh.generation = ex.generation + 1;
        *ex = fresh;
    }
    state.assessments.lock().await.clear();
    state.forecasts.lock().await.clear();
    persist.tape.clear();
    {
        let mut inner = persist.inner();
        inner.journal.clear();
        inner.saved = (0, 0, 0);
    }
    let _ = std::fs::remove_file(persist.dir.join(SESSION_FILE));
    Ok(())
}

/// Play a saved session into the (fresh) exercise through `rocket`, a second
/// instance over the same state. Fails at the first request that is no longer
/// accepted; the caller then starts clean.
pub async fn restore(
    state: &AppState,
    rocket: rocket::Rocket<rocket::Build>,
    session: SavedSession,
) -> Result<(), String> {
    use std::sync::atomic::Ordering::Relaxed;
    let persist = state.persist.clone().ok_or("storage is not enabled")?;
    for (key, answer) in &session.tape {
        persist.tape.put(key, "restored", answer);
    }
    flow::REPLAYING.store(true, Relaxed);
    let outcome = async {
        let client = Client::untracked(rocket).await.map_err(|e| e.to_string())?;
        for (i, e) in session.journal.iter().enumerate() {
            flow::advance_to(state, e.t_s).await;
            let token = match &e.principal {
                Principal::Master => Some(state.access.master.clone()),
                Principal::Player { role } => state.access.players.get(role).cloned(),
                Principal::Evaluator { id } => state.access.evaluators.get(id).cloned(),
            }
            .ok_or_else(|| format!("step {}: unknown participant", i + 1))?;
            let response = client
                .post(e.uri.clone())
                .header(Header::new("Authorization", format!("Bearer {token}")))
                .header(ContentType::JSON)
                .body(e.body.clone())
                .dispatch()
                .await;
            if !response.status().class().is_success() {
                let status = response.status();
                let text = response.into_string().await.unwrap_or_default();
                return Err(format!("step {} ({}): {status} {text}", i + 1, e.uri));
            }
            flow::settle(state).await;
        }
        flow::advance_to(state, session.end_t_s).await;
        Ok(())
    }
    .await;
    {
        let mut ex = state.lock();
        if ex.clock_control.running {
            let ratio = ex.clock_control.ratio;
            let _ = ex.configure_clock(false, ratio, "Session restored; clock paused");
        }
    }
    flow::REPLAYING.store(false, Relaxed);
    if outcome.is_ok() {
        let mut inner = persist.inner();
        inner.journal = session.journal;
        for (i, e) in inner.journal.iter_mut().enumerate() {
            e.seq = i as u64;
            e.done = true;
        }
        inner.next = inner.journal.len() as u64;
    }
    outcome
}

// ------------------------------------------------------------ journal fairing

struct Recorded(Option<u64>);

pub struct JournalFairing;

fn journaled(path: &str) -> bool {
    path.starts_with("/api/")
        && !path.contains("/presence")
        && !path.starts_with("/api/master/storage")
}

#[rocket::async_trait]
impl Fairing for JournalFairing {
    fn info(&self) -> Info {
        Info { name: "Session journal", kind: Kind::Request | Kind::Response }
    }

    async fn on_request(&self, req: &mut rocket::Request<'_>, data: &mut Data<'_>) {
        if req.method() != rocket::http::Method::Post || !journaled(req.uri().path().as_str()) {
            return;
        }
        let Some(state) = req.rocket().state::<AppState>() else { return };
        let Some(persist) = state.persist.clone() else { return };
        let principal = req
            .headers()
            .get_one("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .and_then(|t| state.access.principal(t));
        let Some(principal) = principal else { return };
        let bytes = data.peek(BODY_LIMIT + 1).await;
        if bytes.len() > BODY_LIMIT {
            return;
        }
        let body = String::from_utf8_lossy(bytes).into_owned();
        let uri = req.uri().to_string();
        let t_s = state.lock().world.time_s();
        let seq = persist.begin(principal, uri, body, t_s);
        let _ = req.local_cache(|| Recorded(Some(seq)));
    }

    async fn on_response<'r>(&self, req: &'r rocket::Request<'_>, res: &mut Response<'r>) {
        let Recorded(seq) = req.local_cache(|| Recorded(None));
        let (Some(seq), Some(state)) = (seq, req.rocket().state::<AppState>()) else { return };
        let Some(persist) = state.persist.clone() else { return };
        persist.finish(*seq, res.status().class().is_success());
        persist.autosave(state, false);
    }
}

// ------------------------------------------------------------------ endpoints

fn storage_info(s: &AppState) -> Value {
    let Some(persist) = s.persist.clone() else {
        return json!({"enabled": false});
    };
    json!({
        "enabled": true,
        "roles": s.lock().role_ids(),
        "session": persist.summary(),
        "library": persist.load_injects(),
        "defaults": persist.default_msel,
        "running": s.lock().world.scn.msel,
        "error": persist.inner().error,
    })
}

#[rocket::get("/api/master/storage")]
pub(crate) async fn get_storage(_principal: Principal, s: AppState) -> ApiResult {
    Ok(Json(storage_info(&s)))
}

#[derive(Deserialize)]
pub(crate) struct InjectsBody {
    injects: Vec<InjectTemplate>,
    #[serde(default)]
    new_session: bool,
}

#[rocket::post("/api/master/storage/injects", data = "<b>")]
pub(crate) async fn post_injects(_principal: Principal, s: AppState, b: Json<InjectsBody>) -> ApiResult {
    let Json(b) = b;
    let persist = s.persist.clone().ok_or_else(|| bad("storage is not enabled"))?;
    let roles = s.lock().role_ids();
    validate_injects(&b.injects, &roles).map_err(bad)?;
    persist.save_injects(&b.injects).map_err(bad)?;
    if b.new_session {
        reset(&s, Some(b.injects)).await.map_err(bad)?;
    }
    Ok(Json(storage_info(&s)))
}

#[derive(Deserialize)]
pub(crate) struct DeleteBody {
    /// `session`, `injects` or `all`.
    target: String,
}

#[rocket::post("/api/master/storage/delete", data = "<b>")]
pub(crate) async fn post_delete(_principal: Principal, s: AppState, b: Json<DeleteBody>) -> ApiResult {
    let Json(b) = b;
    let persist = s.persist.clone().ok_or_else(|| bad("storage is not enabled"))?;
    let drop_library = || {
        let _ = std::fs::remove_file(persist.dir.join(INJECTS_FILE));
    };
    match b.target.as_str() {
        "injects" => drop_library(),
        "session" => reset(&s, persist.load_injects()).await.map_err(bad)?,
        "all" => {
            drop_library();
            reset(&s, None).await.map_err(bad)?;
        }
        other => return Err(bad(format!("unknown target {other}"))),
    }
    Ok(Json(storage_info(&s)))
}
