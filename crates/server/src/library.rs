//! The scenario library: the scenarios an EXCON can pick, edit and save.
//!
//! A scenario is a [`ScenarioPack`] (small, editable) over a base territory
//! (large, read-only). Where packs are stored depends on the host: files in
//! the server, `localStorage` in the browser; both implement
//! [`ScenarioLibrary`]. The routes below are the same in both builds.
use super::*;
use scenario::{pack::Vocabulary, Scenario, ScenarioPack};

/// One entry of the library, as listed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub base: String,
    /// Shipped with the app or a bare territory: it cannot be overwritten.
    pub builtin: bool,
}

/// Where packs are kept.
pub trait ScenarioLibrary: Send + Sync {
    /// Every pack, bare base territories included (as unedited packs).
    fn list(&self) -> Vec<PackInfo>;
    fn load(&self, id: &str) -> Option<ScenarioPack>;
    /// Ids of the base territories.
    fn bases(&self) -> Vec<String>;
    fn save(&self, pack: &ScenarioPack) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
    /// The pack the next new exercise starts from.
    fn active(&self) -> Option<String>;
    fn set_active(&self, id: &str) -> Result<(), String>;
}

pub type BaseLoader = Arc<dyn Fn(&str) -> anyhow::Result<Arc<Scenario>> + Send + Sync>;
pub type Factory = Arc<dyn Fn(&ScenarioPack) -> anyhow::Result<Exercise> + Send + Sync>;

/// Everything the routes need to work with scenarios.
pub struct Scenarios {
    pub library: Arc<dyn ScenarioLibrary>,
    pub base: BaseLoader,
    /// Builds a fresh exercise from a pack.
    pub build: Factory,
    /// Stands for the session in the saved file: a different setup (base file,
    /// seed, ensemble) must not replay an old session.
    pub fingerprint: Arc<dyn Fn(&ScenarioPack) -> String + Send + Sync>,
    /// Pack used when nothing else is chosen.
    pub default_id: String,
    /// Pack the running exercise was built from.
    pub running: Mutex<Option<ScenarioPack>>,
}

impl Scenarios {
    /// The pack the next exercise starts from: the chosen one, else the default.
    pub fn next_pack(&self) -> Option<ScenarioPack> {
        self.library
            .active()
            .and_then(|id| self.library.load(&id))
            .or_else(|| self.library.load(&self.default_id))
    }
    pub fn running_pack(&self) -> Option<ScenarioPack> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn set_running(&self, pack: &ScenarioPack) {
        *self.running.lock().unwrap_or_else(|p| p.into_inner()) = Some(pack.clone());
    }
}

/// Command kinds a role may be given.
fn role_actions() -> Vec<&'static str> {
    sim::ALL_KINDS
        .iter()
        .copied()
        .filter(|k| !sim::MASTER_ONLY.contains(k))
        .collect()
}

fn scenarios(s: &AppState) -> Result<Arc<Scenarios>, ApiError> {
    s.scenarios
        .clone()
        .ok_or_else(|| bad("the scenario library is not available"))
}

/// Everything wrong with `pack`, all at once; empty when it can be saved.
pub fn problems(sc: &Scenarios, pack: &ScenarioPack) -> Vec<String> {
    let mut out = Vec::new();
    if !scenario::pack::valid_id(&pack.id) {
        out.push("Identificativo scenario non valido (a-z, 0-9, _ -, max 64)".to_string());
    }
    if pack.name.trim().is_empty() {
        out.push("Il nome dello scenario è obbligatorio".into());
    }
    let base = match (sc.base)(&pack.base) {
        Ok(b) => b,
        Err(e) => {
            out.push(format!("Territorio «{}» non disponibile: {e}", pack.base));
            return out;
        }
    };
    let actions = role_actions();
    out.extend(base.edit_problems(&pack.edits, &Vocabulary { role_actions: &actions }));
    if out.is_empty() {
        if let Err(e) = pack.build((*base).clone()) {
            out.push(e.to_string());
        }
    }
    out
}

/// What the editor needs to offer choices: the territory's names.
fn reference(scn: &Scenario) -> Value {
    json!({
        "grid": {"width_m": scn.grid.width_m(), "height_m": scn.grid.height_m()},
        "zones": scn.zones.names(),
        "zone_grid": {"cols": scn.zones.cols, "rows": scn.zones.rows, "size_m": scn.zones.size_m},
        "zone_centres": scn.zones.names().iter().filter_map(|z| scn.zone_centre(z).map(|c| (z.clone(), c))).collect::<std::collections::BTreeMap<_, _>>(),
        "facilities": scn.buildings.iter().filter_map(|b| b.name.clone()).collect::<Vec<_>>(),
        "buildings": scn.buildings.iter().map(|b| json!([b.id, b.pos[0].round(), b.pos[1].round(), b.occupants, b.name, format!("{:?}", b.kind), b.shelter_capacity])).collect::<Vec<_>>(),
        "sectors": scn.sectors.iter().map(|s| json!({"id": s.id, "name": s.name})).collect::<Vec<_>>(),
        "actions": role_actions(),
        "master_only": sim::MASTER_ONLY,
    })
}

// ---------------------------------------------------------------- reads

#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master/scenarios"))]
pub(crate) async fn get_scenarios(_principal: Principal, s: AppState) -> ApiResult {
    let sc = scenarios(&s)?;
    let active = sc.library.active();
    let running = sc.running_pack().map(|p| p.id);
    Ok(Json(json!({
        "packs": sc.library.list(),
        "bases": sc.library.bases(),
        "active": active.or_else(|| Some(sc.default_id.clone())),
        "running": running,
        "default": sc.default_id,
    })))
}

/// One pack with every editable value filled in (what the base would give for
/// what the pack leaves out), the problems it has now, and the territory's names.
#[cfg_attr(not(target_arch = "wasm32"), rocket::get("/api/master/scenarios/<id>"))]
pub(crate) async fn get_scenario(_principal: Principal, s: AppState, id: String) -> ApiResult {
    let sc = scenarios(&s)?;
    let mut pack = sc.library.load(&id).ok_or_else(|| bad(format!("scenario {id} not found")))?;
    let base = (sc.base)(&pack.base).map_err(|e| bad(e.to_string()))?;
    let built = pack.build((*base).clone())?;
    if pack.name.trim().is_empty() || pack.name == pack.id {
        pack.name = built.name.clone();
    }
    if pack.description.is_empty() {
        pack.description = built.description.clone();
    }
    pack.edits = built.edits();
    let builtin = sc.library.list().iter().any(|p| p.id == id && p.builtin);
    Ok(Json(json!({
        "pack": pack,
        "builtin": builtin,
        "reference": reference(&built),
    })))
}

// ---------------------------------------------------------------- writes

#[derive(Deserialize)]
pub struct PackBody {
    pack: ScenarioPack,
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/scenarios/validate", data = "<b>"))]
pub(crate) async fn post_validate(_principal: Principal, s: AppState, b: Json<PackBody>) -> ApiResult {
    let Json(b) = b;
    let sc = scenarios(&s)?;
    Ok(Json(json!({"problems": problems(&sc, &b.pack)})))
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/scenarios/save", data = "<b>"))]
pub(crate) async fn post_save(_principal: Principal, s: AppState, b: Json<PackBody>) -> ApiResult {
    let Json(b) = b;
    let sc = scenarios(&s)?;
    let pack = b.pack;
    if sc.library.list().iter().any(|p| p.id == pack.id && p.builtin) {
        return Err(bad("questo scenario è incluso nell’app e non si sovrascrive: duplicalo con un altro identificativo"));
    }
    let found = problems(&sc, &pack);
    if !found.is_empty() {
        return Err(bad(found.join("\n")));
    }
    sc.library.save(&pack).map_err(bad)?;
    Ok(Json(json!({"saved": pack.id, "packs": sc.library.list()})))
}

#[derive(Deserialize)]
pub struct IdBody {
    id: String,
}

#[cfg_attr(not(target_arch = "wasm32"), rocket::post("/api/master/scenarios/delete", data = "<b>"))]
pub(crate) async fn post_delete_scenario(_principal: Principal, s: AppState, b: Json<IdBody>) -> ApiResult {
    let Json(b) = b;
    let sc = scenarios(&s)?;
    if sc.library.list().iter().any(|p| p.id == b.id && p.builtin) {
        return Err(bad("uno scenario incluso nell’app non si elimina"));
    }
    if sc.running_pack().is_some_and(|p| p.id == b.id) {
        return Err(bad("lo scenario è in uso: avviane un altro prima di eliminarlo"));
    }
    sc.library.delete(&b.id).map_err(bad)?;
    Ok(Json(json!({"deleted": b.id, "packs": sc.library.list()})))
}

/// Start a new exercise from the pack: the session in progress is dropped.
#[cfg(not(target_arch = "wasm32"))]
#[rocket::post("/api/master/scenarios/activate", data = "<b>")]
pub(crate) async fn post_activate(_principal: Principal, s: AppState, b: Json<IdBody>) -> ApiResult {
    let Json(b) = b;
    let sc = scenarios(&s)?;
    let pack = sc.library.load(&b.id).ok_or_else(|| bad(format!("scenario {} not found", b.id)))?;
    let found = problems(&sc, &pack);
    if !found.is_empty() {
        return Err(bad(found.join("\n")));
    }
    persist::reset(&s, &pack).await.map_err(bad)?;
    sc.library.set_active(&b.id).map_err(bad)?;
    Ok(Json(json!({"running": b.id})))
}

// ---------------------------------------------------------------- files

/// Packs as `<id>.pack.json` next to the base territories `<id>.json`.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileLibrary {
    dir: PathBuf,
    /// Where the chosen pack is remembered across restarts.
    state_dir: Option<PathBuf>,
    chosen: Mutex<Option<String>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileLibrary {
    pub fn new(dir: PathBuf, state_dir: Option<PathBuf>) -> Self {
        let chosen = state_dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join(persist::ACTIVE_FILE)).ok())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        Self { dir, state_dir, chosen: Mutex::new(chosen) }
    }

    fn pack_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.pack.json"))
    }

    fn base_pack(&self, id: &str) -> Option<ScenarioPack> {
        #[derive(Deserialize)]
        struct Metadata { name: String, #[serde(default)] description: String }
        let file = std::fs::File::open(self.dir.join(format!("{id}.json"))).ok()?;
        let meta: Metadata = serde_json::from_reader(std::io::BufReader::new(file)).ok()?;
        let mut pack = ScenarioPack::plain(id, &meta.name, id);
        pack.description = meta.description;
        Some(pack)
    }

    fn files(&self) -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let name = path.file_name()?.to_str()?.to_string();
                Some((name, path))
            })
            .collect();
        out.sort();
        out
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ScenarioLibrary for FileLibrary {
    fn bases(&self) -> Vec<String> {
        self.files()
            .into_iter()
            .filter(|(n, _)| {
                n.ends_with(".json") && !n.ends_with(".pack.json") && !n.ends_with(".manifest.json")
            })
            .map(|(n, _)| n.trim_end_matches(".json").to_string())
            .collect()
    }

    fn list(&self) -> Vec<PackInfo> {
        let mut out: Vec<PackInfo> = self
            .bases()
            .into_iter()
            .filter_map(|b| self.base_pack(&b).map(|p| PackInfo {
                id: b.clone(), name: p.name, description: p.description, base: b, builtin: true,
            }))
            .collect();
        for (name, path) in self.files() {
            let Some(id) = name.strip_suffix(".pack.json") else { continue };
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<ScenarioPack>(&t).map_err(|e| e.to_string()))
            {
                Ok(p) => out.push(PackInfo {
                    id: id.to_string(),
                    name: p.name,
                    description: p.description,
                    base: p.base,
                    builtin: false,
                }),
                Err(e) => tracing::warn!("scenario pack {} is not readable: {e}", path.display()),
            }
        }
        out
    }

    fn load(&self, id: &str) -> Option<ScenarioPack> {
        if !scenario::pack::valid_id(id) {
            return None;
        }
        if let Ok(text) = std::fs::read_to_string(self.pack_path(id)) {
            let mut pack: ScenarioPack = serde_json::from_str(&text).ok()?;
            pack.id = id.to_string();
            return Some(pack);
        }
        self.bases().contains(&id.to_string()).then(|| self.base_pack(id)).flatten()
    }

    fn save(&self, pack: &ScenarioPack) -> Result<(), String> {
        if !scenario::pack::valid_id(&pack.id) {
            return Err("identificativo non valido".into());
        }
        if self.bases().contains(&pack.id) {
            return Err(format!("«{}» è il nome di un territorio: scegli un altro identificativo", pack.id));
        }
        let text = serde_json::to_string_pretty(pack).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.pack_path(&pack.id);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        if !scenario::pack::valid_id(id) {
            return Err("identificativo non valido".into());
        }
        std::fs::remove_file(self.pack_path(id)).map_err(|e| e.to_string())?;
        let mut chosen = self.chosen.lock().unwrap_or_else(|p| p.into_inner());
        if chosen.as_deref() == Some(id) {
            *chosen = None;
            if let Some(d) = &self.state_dir {
                let _ = std::fs::remove_file(d.join(persist::ACTIVE_FILE));
            }
        }
        Ok(())
    }

    fn active(&self) -> Option<String> {
        self.chosen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .filter(|id| self.load(id).is_some())
    }

    fn set_active(&self, id: &str) -> Result<(), String> {
        *self.chosen.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.to_string());
        if let Some(d) = &self.state_dir {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            std::fs::write(d.join(persist::ACTIVE_FILE), id).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Loads `<dir>/<id>.json`, keeping the last territory in memory: they are
/// big, and editing a scenario asks for the same one over and over.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_base_loader(dir: PathBuf) -> BaseLoader {
    type Cached = (String, std::time::SystemTime, Arc<Scenario>);
    let cache: Mutex<Option<Cached>> = Mutex::new(None);
    Arc::new(move |id: &str| {
        anyhow::ensure!(scenario::pack::valid_id(id), "invalid territory id {id:?}");
        let path = dir.join(format!("{id}.json"));
        let modified = std::fs::metadata(&path)?.modified()?;
        let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((cid, ctime, scn)) = cache.as_ref() {
            if cid == id && *ctime == modified {
                return Ok(scn.clone());
            }
        }
        let scn = Arc::new(Scenario::load(&path)?);
        *cache = Some((id.to_string(), modified, scn.clone()));
        Ok(scn)
    })
}

// ---------------------------------------------------------------- memory

/// A library kept in memory, over a fixed set of territories. For tests and
/// hosts without storage.
pub struct MemoryLibrary {
    bases: Vec<String>,
    packs: Mutex<std::collections::BTreeMap<String, ScenarioPack>>,
    chosen: Mutex<Option<String>>,
}

impl MemoryLibrary {
    pub fn new(bases: Vec<String>) -> Self {
        Self { bases, packs: Default::default(), chosen: Default::default() }
    }
}

impl ScenarioLibrary for MemoryLibrary {
    fn bases(&self) -> Vec<String> {
        self.bases.clone()
    }
    fn list(&self) -> Vec<PackInfo> {
        let mut out: Vec<PackInfo> = self
            .bases
            .iter()
            .map(|b| PackInfo {
                id: b.clone(),
                name: b.clone(),
                description: String::new(),
                base: b.clone(),
                builtin: true,
            })
            .collect();
        out.extend(self.packs.lock().unwrap_or_else(|p| p.into_inner()).values().map(|p| PackInfo {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            base: p.base.clone(),
            builtin: false,
        }));
        out
    }
    fn load(&self, id: &str) -> Option<ScenarioPack> {
        if let Some(p) = self.packs.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
            return Some(p.clone());
        }
        self.bases.iter().any(|b| b == id).then(|| ScenarioPack::plain(id, id, id))
    }
    fn save(&self, pack: &ScenarioPack) -> Result<(), String> {
        if self.bases.contains(&pack.id) {
            return Err(format!("«{}» è il nome di un territorio", pack.id));
        }
        self.packs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(pack.id.clone(), pack.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        self.packs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| format!("scenario {id} not found"))
    }
    fn active(&self) -> Option<String> {
        self.chosen.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn set_active(&self, id: &str) -> Result<(), String> {
        *self.chosen.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.to_string());
        Ok(())
    }
}
