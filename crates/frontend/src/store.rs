//! Browser database of the standalone app (`localStorage`): the session in
//! progress, the scenario library and a one-shot notice.
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const SESSION_KEY: &str = "ttx.standalone.session";
pub const PACKS_KEY: &str = "ttx.standalone.packs";
pub const ACTIVE_KEY: &str = "ttx.standalone.active";
pub const API_KEY: &str = "ttx.standalone.openrouter-key";
const NOTICE_KEY: &str = "ttx.standalone.notice";
const AUTOSTART_KEY: &str = "ttx.standalone.autostart";
const FORMAT: u32 = 2;

/// Everything needed to play a session again from the start: the scenario it
/// began with, the requests applied since, and the AI answers they received.
#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    pub v: u32,
    pub pack: scenario::ScenarioPack,
    #[serde(default)]
    pub territory_fingerprint: String,
    pub journal: Vec<server::browser::JournalEntry>,
    pub tape: Vec<(String, String)>,
    /// Simulated seconds reached when saved.
    pub end_t_s: i64,
    /// Participants chose their language before the start (journal holds
    /// their choices). Sessions saved before this existed kept the
    /// scenario's languages and replay that way.
    #[serde(default)]
    pub language_choice: bool,
}

thread_local! {
    static PENDING: RefCell<Option<SavedSession>> = const { RefCell::new(None) };
    static RESTORING: Cell<bool> = const { Cell::new(false) };
    static LAST_SAVED: Cell<(usize, usize, i64)> = const { Cell::new((0, 0, 0)) };
    static FROZEN: Cell<bool> = const { Cell::new(false) };
    static SAVE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Scenario chosen in the launcher when the browser blocks storage: the
    /// exercise still starts (nothing is persisted).
    static CHOSEN: RefCell<Option<String>> = const { RefCell::new(None) };
    static LANGUAGE_CHOICE: Cell<bool> = const { Cell::new(true) };
}

/// Whether the running session asks participants for their language
/// (see [`SavedSession::language_choice`]).
pub fn set_language_choice(on: bool) {
    LANGUAGE_CHOICE.with(|c| c.set(on));
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    storage()
        .ok_or("archiviazione del browser non disponibile")?
        .set_item(key, value)
        .map_err(|_| "spazio del browser esaurito o archiviazione bloccata".to_string())
}

pub fn remove(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}

/// Stored session, if present and readable. An unreadable one is dropped.
pub fn load_session() -> Option<SavedSession> {
    let text = get(SESSION_KEY)?;
    match serde_json::from_str::<SavedSession>(&text) {
        Ok(s)
            if s.v == FORMAT
                && authoring_base(&s.pack.base).is_ok()
                && (s.territory_fingerprint.is_empty()
                    || territory_fingerprint(&s.pack.base) == s.territory_fingerprint) =>
        {
            Some(s)
        }
        _ => {
            remove(SESSION_KEY);
            set_notice("La sessione salvata non è compatibile con il formato o il territorio attuale ed è stata scartata.");
            None
        }
    }
}

/// Scenarios the user saved in this browser.
fn saved_packs() -> BTreeMap<String, scenario::ScenarioPack> {
    get(PACKS_KEY)
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn store_packs(packs: &BTreeMap<String, scenario::ScenarioPack>) -> Result<(), String> {
    set(
        PACKS_KEY,
        &serde_json::to_string(packs).map_err(|e| e.to_string())?,
    )
}

/// Start the next exercise from `id`: forget the session, remember the choice,
/// reload straight into the exercise (the choice was made in the app).
pub fn activate(id: &str) -> Result<(), String> {
    set(ACTIVE_KEY, id)?;
    if let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
        let _ = s.set_item(AUTOSTART_KEY, "1");
    }
    restart(&[SESSION_KEY]);
    Ok(())
}

/// Chosen in the launcher: the next exercise starts from `id`, without the saved session.
pub fn choose(id: &str) -> Result<(), String> {
    if storage().is_none() {
        CHOSEN.with(|c| *c.borrow_mut() = Some(id.to_string()));
        return Ok(());
    }
    set(ACTIVE_KEY, id)?;
    remove(SESSION_KEY);
    Ok(())
}

/// Once after [`activate`]: skip the launcher.
pub fn take_autostart() -> bool {
    let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else {
        return false;
    };
    let set = s.get_item(AUTOSTART_KEY).ok().flatten().is_some();
    let _ = s.remove_item(AUTOSTART_KEY);
    set
}

/// Complete authored territories, shared by the 3D editor and exercise library.
const TERRITORIES_KEY: &str = "ttx.standalone.territories";
pub fn authored_territories() -> BTreeMap<String, scenario::Scenario> {
    let Some(raw) = get(TERRITORIES_KEY) else {
        return BTreeMap::new();
    };
    let all: BTreeMap<String, scenario::Scenario> =
        serde_json::from_str(&raw).unwrap_or_default();
    // Retired keys are ignored on load; rewrite once so the stored copy drops them.
    if !all.is_empty() && retired_keys(&raw) {
        if let Ok(clean) = serde_json::to_string(&all) {
            let _ = set(TERRITORIES_KEY, &clean);
        }
    }
    all
}
fn retired_keys(raw: &str) -> bool {
    raw.contains("\"communication_sites\"") || raw.contains("\"communications\"")
}
pub fn authoring_base(id: &str) -> Result<scenario::Scenario, String> {
    if let Some(s) = authored_territories().remove(id) {
        return Ok(s);
    }
    scenario::embedded::load(id).map_err(|e| e.to_string())
}
pub fn save_territory(s: &scenario::Scenario) -> Result<(), String> {
    scenario::editor::validate(s).map_err(|e| e.to_string())?;
    if embedded_id(&s.id)
        || embedded_packs().iter().any(|p| p.id == s.id)
        || saved_packs().contains_key(&s.id)
    {
        return Err(
            "Identificativo già usato da un territorio incluso o un pack: clona con un nuovo ID"
                .into(),
        );
    }
    let mut all = authored_territories();
    all.insert(s.id.clone(), s.clone());
    set(
        TERRITORIES_KEY,
        &serde_json::to_string(&all).map_err(|e| e.to_string())?,
    )
}
fn territory_fingerprint(id: &str) -> String {
    if let Some(s) = authored_territories().remove(id) {
        let json = serde_json::to_string(&s).unwrap_or_default();
        let hash = json.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        return format!("{id}:{hash:016x}");
    }
    scenario::embedded::fingerprint(id).unwrap_or_default()
}

/// Lazy per-territory cache. Selecting a pack always loads its declared base.
fn embedded_base(id: &str) -> Result<Arc<scenario::Scenario>, String> {
    if let Some(s) = authored_territories().remove(id) {
        return Ok(Arc::new(s));
    }
    thread_local! { static BASES: RefCell<BTreeMap<String, Arc<scenario::Scenario>>> = const { RefCell::new(BTreeMap::new()) }; }
    BASES.with(|bases| {
        if let Some(base) = bases.borrow().get(id) {
            return Ok(base.clone());
        }
        let base = Arc::new(scenario::embedded::load(id).map_err(|e| e.to_string())?);
        bases.borrow_mut().insert(id.into(), base.clone());
        Ok(base)
    })
}
fn embedded_packs() -> Vec<scenario::ScenarioPack> {
    scenario::embedded::packs()
}
fn embedded_id(id: &str) -> bool {
    scenario::embedded::territory(id).is_ok()
}

/// The scenario library in `localStorage`, over what is embedded in the app.
pub struct BrowserLibrary;

impl server::library::ScenarioLibrary for BrowserLibrary {
    fn bases(&self) -> Vec<String> {
        scenario::embedded::TERRITORIES
            .iter()
            .map(|t| t.id.into())
            .chain(authored_territories().into_keys())
            .collect()
    }
    fn list(&self) -> Vec<server::library::PackInfo> {
        use server::library::PackInfo;
        let info = |p: &scenario::ScenarioPack, builtin| PackInfo {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            base: p.base.clone(),
            builtin,
        };
        let mut out = scenario::embedded::TERRITORIES
            .iter()
            .map(|t| {
                let mut p = scenario::ScenarioPack::plain(t.id, t.name, t.id);
                p.description = t.description.into();
                info(&p, true)
            })
            .collect::<Vec<_>>();
        out.extend(authored_territories().values().map(|s| {
            let mut p = scenario::ScenarioPack::plain(&s.id, &s.name, &s.id);
            p.description = s.description.clone();
            info(&p, true)
        }));
        out.extend(embedded_packs().iter().map(|p| info(p, true)));
        out.extend(saved_packs().values().map(|p| info(p, false)));
        out
    }
    fn load(&self, id: &str) -> Option<scenario::ScenarioPack> {
        if let Some(s) = authored_territories().remove(id) {
            return Some(scenario::ScenarioPack::plain(&s.id, &s.name, &s.id));
        }
        if let Ok(t) = scenario::embedded::territory(id) {
            let mut pack = scenario::ScenarioPack::plain(t.id, t.name, t.id);
            pack.description = t.description.into();
            return Some(pack);
        }
        saved_packs()
            .remove(id)
            .or_else(|| embedded_packs().into_iter().find(|p| p.id == id))
    }
    fn save(&self, pack: &scenario::ScenarioPack) -> Result<(), String> {
        if embedded_id(&pack.id)
            || authored_territories().contains_key(&pack.id)
            || embedded_packs().iter().any(|p| p.id == pack.id)
        {
            return Err(format!("«{}» è uno scenario incluso nell’app", pack.id));
        }
        let mut packs = saved_packs();
        packs.insert(pack.id.clone(), pack.clone());
        store_packs(&packs)
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        let mut packs = saved_packs();
        packs.remove(id).ok_or("scenario non trovato")?;
        store_packs(&packs)?;
        if get(ACTIVE_KEY).as_deref() == Some(id) {
            remove(ACTIVE_KEY);
        }
        Ok(())
    }
    fn active(&self) -> Option<String> {
        get(ACTIVE_KEY)
            .or_else(|| CHOSEN.with(|c| c.borrow().clone()))
            .filter(|id| self.load(id).is_some())
    }
    fn set_active(&self, id: &str) -> Result<(), String> {
        set(ACTIVE_KEY, id)
    }
}

/// Scenario library, territory loader and exercise factory of the standalone app.
pub fn scenarios() -> Arc<server::library::Scenarios> {
    Arc::new(server::library::Scenarios {
        library: Arc::new(BrowserLibrary),
        base: Arc::new(|id| embedded_base(id).map_err(anyhow::Error::msg)),
        build: Arc::new(|pack| {
            let base = embedded_base(&pack.base).map_err(anyhow::Error::msg)?;
            let mut scn = pack.build((*base).clone())?;
            scn.enable_operational_model();
            let world = sim::World::with_options(
                Arc::new(scn),
                pack.run.seed.unwrap_or(1),
                sim::FireOptions {
                    realizations: pack.run.realizations.unwrap_or(32),
                    cutoff: pack.run.cutoff.unwrap_or(0.5),
                },
            )?;
            Ok(exercise::Exercise::new(world))
        }),
        fingerprint: Arc::new(|pack| territory_fingerprint(&pack.base)),
        default_id: scenario::embedded::DEFAULT_TERRITORY.into(),
        running: std::sync::Mutex::new(None),
    })
}

/// Session found at startup and waiting to be replayed.
pub fn set_pending(session: Option<SavedSession>) {
    RESTORING.with(|r| r.set(session.is_some()));
    PENDING.with(|p| *p.borrow_mut() = session);
}

pub fn take_pending() -> Option<SavedSession> {
    PENDING.with(|p| p.borrow_mut().take())
}

pub fn restoring() -> bool {
    RESTORING.with(Cell::get)
}

/// Stop writing: the page is about to reload after deleting stored objects.
pub fn freeze() {
    FROZEN.with(|f| f.set(true));
}

pub fn finish_restore() {
    RESTORING.with(|r| r.set(false));
}

pub fn set_notice(text: &str) {
    let _ = set(NOTICE_KEY, text);
}

/// A notice left before the last reload; shown once.
pub fn take_notice() -> Option<String> {
    let text = get(NOTICE_KEY)?;
    remove(NOTICE_KEY);
    Some(text)
}

pub fn save_error() -> Option<String> {
    SAVE_ERROR.with(|e| e.borrow().clone())
}

/// Write the session when something changed since the last write (`force`
/// skips that check). `pack` is what the running exercise was built from.
pub fn autosave(state: &server::AppState, force: bool) {
    if restoring() || FROZEN.with(Cell::get) {
        return;
    }
    let journal = server::browser::journal();
    let tape = server::browser::tape_entries();
    let end_t_s = state.lock().world.time_s();
    let Some(pack) = state.scenarios.as_ref().and_then(|s| s.running_pack()) else {
        return;
    };
    let signature = (journal.len(), tape.len(), end_t_s);
    if !force && LAST_SAVED.with(Cell::get) == signature {
        return;
    }
    LAST_SAVED.with(|l| l.set(signature));
    if journal.is_empty() {
        remove(SESSION_KEY);
        return;
    }
    let session = SavedSession {
        v: FORMAT,
        territory_fingerprint: territory_fingerprint(&pack.base),
        pack,
        journal,
        tape,
        end_t_s,
        language_choice: LANGUAGE_CHOICE.with(Cell::get),
    };
    let result = serde_json::to_string(&session)
        .map_err(|e| e.to_string())
        .and_then(|text| set(SESSION_KEY, &text));
    SAVE_ERROR.with(|e| *e.borrow_mut() = result.err());
}

/// Size of the stored session in kB, and the number of steps it replays.
pub fn session_summary() -> Option<(usize, usize)> {
    let text = get(SESSION_KEY)?;
    let steps = serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("journal")?
        .as_array()?
        .len();
    Some((text.len().div_ceil(1024), steps))
}

/// Delete the given stored objects and reload into a clean start.
pub fn restart(keys: &[&str]) {
    freeze();
    for key in keys {
        remove(key);
    }
    let _ = crate::api::window().location().reload();
}

/// The settings' storage operations, on browser storage.
pub fn backend() -> crate::settings::Backend {
    use crate::settings::Op;
    crate::settings::Backend(std::rc::Rc::new(|op| {
        Box::pin(async move {
            match op {
                Op::Delete(target) => {
                    let keys: &[&str] = match target {
                        "session" => &[SESSION_KEY],
                        _ => &[SESSION_KEY, PACKS_KEY, ACTIVE_KEY, API_KEY],
                    };
                    restart(keys);
                    Ok("Eliminato".into())
                }
            }
        })
    }))
}
