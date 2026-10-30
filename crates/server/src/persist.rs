//! Server-side storage of the session.
//!
//! A session is saved as a journal: the state-changing requests that were
//! accepted, who made them, the AI answers they received and the simulated
//! time reached. On startup the journal is played again through the real
//! routes (a local Rocket client) into a fresh exercise, answering from the
//! saved AI answers and with the wall clock stood down, like the browser does.
//! The scenario the session started from is saved with it, so a restart
//! rebuilds the same exercise even if the library has changed since.
use super::*;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{ContentType, Header};
use rocket::local::asynchronous::Client;
use rocket::{Data, Response};
use crate::library::Scenarios;
use scenario::ScenarioPack;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FORMAT: u32 = 2;
const SESSION_FILE: &str = "session.json";
pub const ACTIVE_FILE: &str = "active.txt";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub principal: Principal,
    pub uri: String,
    pub body: String,
    pub t_s: i64,
    /// Wall-clock instant (ms) of the request; replays restore the exercise timers from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(skip)]
    seq: u64,
    #[serde(skip)]
    done: bool,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    pub v: u32,
    /// Base file, seed and ensemble options: a session only fits the same setup.
    pub fingerprint: String,
    /// The scenario the exercise was built from.
    pub pack: ScenarioPack,
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
    pub scenarios: Arc<Scenarios>,
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

impl Persist {
    pub fn new(dir: PathBuf, scenarios: Arc<Scenarios>) -> Arc<Persist> {
        Arc::new(Persist {
            dir,
            scenarios,
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
            Ok(s) if s.v == FORMAT && s.fingerprint == (self.scenarios.fingerprint)(&s.pack) => Some(s),
            _ => {
                tracing::warn!("saved session {} does not fit this setup: discarded", path.display());
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    fn begin(&self, principal: Principal, uri: String, body: String, t_s: i64) -> u64 {
        let mut inner = self.inner();
        inner.next += 1;
        let seq = inner.next;
        inner.journal.push(JournalEntry { principal, uri, body, t_s, wall_ms: Some(now_wall_ms()), seq, done: false });
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
        let end_t_s = state.lock().world.time_s();
        let Some(pack) = self.scenarios.running_pack() else { return };
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
            fingerprint: (self.scenarios.fingerprint)(&pack),
            pack,
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

/// Replace the exercise by a fresh one built from `pack`, forgetting the
/// journal and the AI answers.
pub async fn reset(state: &AppState, pack: &ScenarioPack) -> Result<(), String> {
    let scenarios = state.scenarios.clone().ok_or("scenarios are not available")?;
    if state.lock().phase == exercise::Phase::Resolving {
        return Err("a turn is being resolved; wait for it to finish".into());
    }
    let mut fresh = (scenarios.build)(pack).map_err(|e| e.to_string())?;
    {
        let mut ex = state.lock();
        fresh.generation = ex.generation + 1;
        *ex = fresh;
    }
    state.access.set_roles(state.lock().role_ids());
    state.assessments.lock().await.clear();
    state.forecasts.lock().await.clear();
    situation::clear(state);
    scenarios.set_running(pack);
    if let Some(persist) = &state.persist {
        persist.tape.clear();
        {
            let mut inner = persist.inner();
            inner.journal.clear();
            inner.saved = (0, 0, 0);
        }
        let _ = std::fs::remove_file(persist.dir.join(SESSION_FILE));
    }
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
            state.lock().replay_wall_ms = e.wall_ms.map(u128::from);
            flow::advance_to(state, e.t_s).await;
            let token = match &e.principal {
                Principal::Master => Some(state.access.master.clone()),
                Principal::Player { role } => state.access.player_key(role),
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
        ex.replay_wall_ms = None;
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

/// A POST the fairing decided to journal; its body arrives with the data guard.
struct Pending {
    principal: Principal,
    uri: String,
    t_s: i64,
}

/// Journal sequence of the request, once its body has been recorded.
struct Recorded(Mutex<Option<u64>>);

pub struct JournalFairing;

fn journaled(path: &str) -> bool {
    path.starts_with("/api/")
        && !path.contains("/presence")
        && !path.starts_with("/api/master/storage")
        && !path.starts_with("/api/master/scenarios")
}

/// Record the whole body of a journaled request (called by the JSON data guard).
pub(crate) fn record_body(req: &rocket::Request<'_>, bytes: &[u8]) {
    let Some(p) = req.local_cache(|| None::<Pending>) else { return };
    let Some(state) = req.rocket().state::<AppState>() else { return };
    let Some(persist) = state.persist.clone() else { return };
    let seq = persist.begin(
        p.principal.clone(),
        p.uri.clone(),
        String::from_utf8_lossy(bytes).into_owned(),
        p.t_s,
    );
    *req.local_cache(|| Recorded(Mutex::new(None))).0.lock().unwrap() = Some(seq);
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
        let uri = req.uri().to_string();
        let t_s = state.lock().world.time_s();
        if data.peek(1).await.is_empty() {
            // No body: nothing for a data guard to read.
            let seq = persist.begin(principal, uri, String::new(), t_s);
            let _ = req.local_cache(|| Recorded(Mutex::new(Some(seq))));
        } else {
            let _ = req.local_cache(|| Some(Pending { principal, uri, t_s }));
        }
    }

    async fn on_response<'r>(&self, req: &'r rocket::Request<'_>, res: &mut Response<'r>) {
        let Some(state) = req.rocket().state::<AppState>() else { return };
        let Some(persist) = state.persist.clone() else { return };
        let mut seq = *req.local_cache(|| Recorded(Mutex::new(None))).0.lock().unwrap();
        if seq.is_none() {
            // A route that never reads its body (no data guard): it has nothing to replay.
            if let Some(p) = req.local_cache(|| None::<Pending>) {
                seq = Some(persist.begin(p.principal.clone(), p.uri.clone(), String::new(), p.t_s));
            }
        }
        let Some(seq) = seq else { return };
        persist.finish(seq, res.status().class().is_success());
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
        "session": persist.summary(),
        "running": s.scenarios.as_ref().and_then(|sc| sc.running_pack()).map(|p| p.id),
        "error": persist.inner().error,
    })
}

#[rocket::get("/api/master/storage")]
pub(crate) async fn get_storage(_principal: Principal, s: AppState) -> ApiResult {
    Ok(Json(storage_info(&s)))
}

#[derive(Deserialize)]
pub(crate) struct DeleteBody {
    /// `session`, or `all` to also forget the chosen scenario.
    target: String,
}

#[rocket::post("/api/master/storage/delete", data = "<b>")]
pub(crate) async fn post_delete(_principal: Principal, s: AppState, b: Json<DeleteBody>) -> ApiResult {
    let Json(b) = b;
    s.persist.as_ref().ok_or_else(|| bad("storage is not enabled"))?;
    let scenarios = s.scenarios.clone().ok_or_else(|| bad("scenarios are not available"))?;
    let pack = match b.target.as_str() {
        "session" => scenarios.running_pack(),
        "all" => {
            scenarios.library.set_active(&scenarios.default_id).map_err(bad)?;
            scenarios.library.load(&scenarios.default_id)
        }
        other => return Err(bad(format!("unknown target {other}"))),
    }
    .ok_or_else(|| bad("no scenario to restart from"))?;
    reset(&s, &pack).await.map_err(bad)?;
    Ok(Json(storage_info(&s)))
}
