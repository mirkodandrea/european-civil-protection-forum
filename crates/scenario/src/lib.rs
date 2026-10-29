//! The scenario package: everything the exercise needs to know about a place
//! before the first turn — terrain and fuel for the fire model, buildings,
//! roads, sectors, a multilingual gazetteer, the player roles with their
//! resources, reinforcement offers and the MSEL.
//!
//! ## Coordinate frame
//!
//! One metric **world frame**: origin at the south-west corner of the grid,
//! `+x` east, `+y` north, metres. Rasters are stored row-major with **row 0 at
//! the north edge** (the fire core's convention: a north wind, `wind_dir = 0`,
//! drives fire toward increasing rows). [`Scenario::cell_of`] and
//! [`Scenario::cell_centre`] are the only conversions; nothing else should
//! need to know the raster spacing.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(feature = "embedded")]
pub mod embedded;
pub mod factory;
pub mod glossary;
pub mod editor;
pub mod pack;
pub mod synthetic;
pub use pack::{RunOptions, ScenarioEdits, ScenarioPack};

/// Fuel codes of the fire core's legacy 7-class table.
pub mod fuel {
    pub const BROADLEAVES: i32 = 1;
    pub const SHRUBS: i32 = 2;
    /// Non-burnable: settlements, water, bare rock, cleared fire lines.
    pub const NON_VEGETATED: i32 = 3;
    pub const GRASSLAND: i32 = 4;
    pub const CONIFERS: i32 = 5;
    pub const AGRO_FORESTRY: i32 = 6;
    pub const NON_FIRE_PRONE_FOREST: i32 = 7;

    pub fn name(code: i32) -> &'static str {
        match code {
            BROADLEAVES => "broadleaves",
            SHRUBS => "shrubs",
            NON_VEGETATED => "non-vegetated",
            GRASSLAND => "grassland",
            CONIFERS => "conifers",
            AGRO_FORESTRY => "agro-forestry",
            NON_FIRE_PRONE_FOREST => "non-fire-prone forest",
            _ => "unknown",
        }
    }
}

fn yes() -> bool {
    true
}
fn default_road_class() -> String {
    "residential".into()
}

/// Opt-in operational assumptions. Old scenario packages retain calibrated rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Operations {
    pub road_routing: bool,
    pub progressive_suppression: bool,
    pub traffic: bool,
    pub intensity_exposure: bool,
    pub spotting: bool,
    pub segment_fire_closures: bool,
    pub terrain_travel: bool,
    pub emergency_congestion: bool,
    pub local_autonomy: bool,
}
impl Operations {
    pub fn enhanced() -> Self {
        Self {
            road_routing: true,
            progressive_suppression: true,
            traffic: true,
            intensity_exposure: true,
            spotting: false,
            segment_fire_closures: true,
            terrain_travel: true,
            emergency_congestion: true,
            local_autonomy: true,
        }
    }
}

/// Authored training assumptions for execution within an approved engine mission.
/// Distances are metres, rates use simulated seconds; these are not safety guidance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TacticalPolicy {
    pub version: String,
    pub sector_radius_m: f32,
    pub hose_reach_m: f32,
    pub wet_limit_pts: f32,
    pub reassess_s: i64,
    pub parking_danger: f32,
    pub work_danger: f32,
    pub share_work: bool,
    pub resume_after_withdrawal: bool,
    pub regroup_s: i64,
}
impl Default for TacticalPolicy {
    fn default() -> Self {
        Self {
            version: "ttx-local-v1".into(),
            sector_radius_m: 300.0,
            hose_reach_m: 60.0,
            wet_limit_pts: 30.0,
            reassess_s: 60,
            parking_danger: 0.10,
            work_danger: 0.35,
            share_work: true,
            resume_after_withdrawal: true,
            regroup_s: 60,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuelDefinition {
    pub id: i32,
    pub name: String,
    pub v0: f64,
    pub d0: f64,
    pub d1: f64,
    pub hhv: f64,
    pub humidity: Option<f64>,
    pub spotting: bool,
    pub prob_ign_by_embers: f64,
    pub burn: bool,
    pub spread_probability: Vec<(i32, f64)>,
}
/// A position in the world frame, metres.
pub type Pos = [f32; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GridSpec {
    pub rows: usize,
    pub cols: usize,
    /// Cell edge, metres.
    pub cellsize: f32,
}

impl GridSpec {
    pub fn width_m(&self) -> f32 {
        self.cols as f32 * self.cellsize
    }
    pub fn height_m(&self) -> f32 {
        self.rows as f32 * self.cellsize
    }
    pub fn len(&self) -> usize {
        self.rows * self.cols
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn index(&self, c: Cell) -> usize {
        c.row * self.cols + c.col
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WeatherSpec {
    /// Degrees clockwise from north the wind blows **from** (meteorological).
    pub wind_dir_deg: f64,
    pub wind_speed_kmh: f64,
    /// Dead fine fuel moisture, percent.
    pub moisture_pct: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingKind {
    House,
    School,
    Hospital,
    FuelStation,
    SportsCentre,
    TownHall,
    Industrial,
}

impl BuildingKind {
    /// Buildings whose occupants cannot self-evacuate by car and need buses.
    pub fn needs_transport(self) -> bool {
        matches!(self, BuildingKind::School | BuildingKind::Hospital)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Building {
    pub id: u32,
    pub kind: BuildingKind,
    /// Source artwork category, retained by the Factory importer. Presentation
    /// only: does not grant capacity, affect exposure, or change operational kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_kind: Option<String>,
    /// Named facilities carry a name and are addressable from the gazetteer.
    #[serde(default)]
    pub name: Option<String>,
    pub pos: Pos,
    /// Footprint, metres (x, y).
    pub size: [f32; 2],
    /// People normally inside during the exercise window (no individual agents).
    pub occupants: u32,
    pub sector: String,
    /// Can host evacuees (shelter / assembly area) once opened.
    #[serde(default)]
    pub shelter_capacity: u32,
    /// Footprint rotation, degrees counter-clockwise seen from above (0 = `size[0]`
    /// along east). Presentation only: exposure and routing use `pos`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation_deg: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Road {
    #[serde(default)]
    pub oneway: bool,
    #[serde(default = "default_road_class")]
    pub class: String,
    #[serde(default = "yes")]
    pub drivable: bool,
    pub id: String,
    pub name: String,
    pub points: Vec<Pos>,
}

/// A borgo: the homes of one locality, evacuated together by
/// `EvacuateDistrict`. Membership is explicit, not geometric: scattered
/// houses belong to their locality wherever they stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct District {
    pub id: String,
    pub name: String,
    /// Building ids.
    pub buildings: Vec<u32>,
    /// Named building where evacuees gather when the order names no
    /// destination; usable without `OpenShelter`. `None`: the nearest
    /// reachable gathering area, open shelter or map exit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gathering: Option<String>,
}

/// How a district evacuation leaves: everyone prepares for `prep_min`, then
/// at most `per_min` people depart per minute, nearest homes to the borgo's
/// departure point first.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DistrictEvacuation {
    pub per_min: f32,
    pub prep_min: u32,
}

impl Default for DistrictEvacuation {
    fn default() -> Self {
        Self {
            per_min: 6.0,
            prep_min: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sector {
    pub id: String,
    pub name: String,
    /// Axis-aligned bounds `[x0, y0, x1, y1]`, metres.
    pub bounds: [f32; 4],
}

impl Sector {
    pub fn contains(&self, p: Pos) -> bool {
        p[0] >= self.bounds[0]
            && p[0] < self.bounds[2]
            && p[1] >= self.bounds[1]
            && p[1] < self.bounds[3]
    }
    pub fn centre(&self) -> Pos {
        [
            (self.bounds[0] + self.bounds[2]) * 0.5,
            (self.bounds[1] + self.bounds[3]) * 0.5,
        ]
    }
}

/// The evacuation-zone grid: a plain map grid laid over the window, so an
/// order can say "evacuate C3". Columns are letters west → east, rows are
/// numbers north → south, like a printed operational map.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ZoneGrid {
    pub cols: usize,
    pub rows: usize,
    /// Zone edge, metres.
    pub size_m: f32,
}

impl ZoneGrid {
    pub fn name(&self, col: usize, row: usize) -> String {
        format!("{}{}", (b'A' + col as u8) as char, row + 1)
    }

    /// All zone names, row by row from the north-west corner.
    pub fn names(&self) -> Vec<String> {
        (0..self.rows)
            .flat_map(|r| (0..self.cols).map(move |c| (c, r)))
            .map(|(c, r)| self.name(c, r))
            .collect()
    }

    /// The given zones plus their eight neighbours inside the grid (sorted,
    /// no duplicates); unknown names are ignored.
    pub fn with_neighbours(&self, names: &[String]) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for (c, r) in names.iter().filter_map(|n| self.parse(n)) {
            for (dc, dr) in (-1i64..=1).flat_map(|dc| (-1i64..=1).map(move |dr| (dc, dr))) {
                let (c, r) = (c as i64 + dc, r as i64 + dr);
                if (0..self.cols as i64).contains(&c) && (0..self.rows as i64).contains(&r) {
                    out.insert(self.name(c as usize, r as usize));
                }
            }
        }
        out.into_iter().collect()
    }

    /// The zone containing `p`, if inside the grid.
    pub fn zone_of(&self, p: Pos, height_m: f32) -> Option<String> {
        if p[0] < 0.0 || p[1] < 0.0 {
            return None;
        }
        let col = (p[0] / self.size_m) as usize;
        let row = ((height_m - p[1]) / self.size_m) as usize;
        (col < self.cols && row < self.rows).then(|| self.name(col, row))
    }

    /// `(col, row)` of a zone name such as `"c3"` or `"C 3"`.
    pub fn parse(&self, name: &str) -> Option<(usize, usize)> {
        let key: String = name
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase();
        let mut chars = key.chars();
        let letter = chars.next()?;
        if !letter.is_ascii_uppercase() {
            return None;
        }
        let col = (letter as u8 - b'A') as usize;
        let row: usize = chars.as_str().parse().ok()?;
        (col < self.cols && row >= 1 && row <= self.rows).then_some((col, row - 1))
    }

    /// A zone edge as a segment: `"F4:N"` is the north edge of F4 (also `S`,
    /// `E`, `W`), west→east or north→south. Used for fire lines "by code".
    pub fn edge(&self, code: &str, height_m: f32) -> Option<(Pos, Pos)> {
        let (zone, side) = code.split_once(':')?;
        let b = self.bounds(zone, height_m)?;
        match side.trim().to_uppercase().as_str() {
            "N" => Some(([b[0], b[3]], [b[2], b[3]])),
            "S" => Some(([b[0], b[1]], [b[2], b[1]])),
            "W" => Some(([b[0], b[3]], [b[0], b[1]])),
            "E" => Some(([b[2], b[3]], [b[2], b[1]])),
            _ => None,
        }
    }

    /// `[x0, y0, x1, y1]` of a zone, metres.
    pub fn bounds(&self, name: &str, height_m: f32) -> Option<[f32; 4]> {
        let (c, r) = self.parse(name)?;
        let x0 = c as f32 * self.size_m;
        let y1 = height_m - r as f32 * self.size_m;
        Some([x0, y1 - self.size_m, x0 + self.size_m, y1])
    }
}

/// A named point, for the LLM to resolve "the ridge" or "la scuola" to a place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub pos: Pos,
    /// Optional circular area around `pos`, metres (e.g. a district or a valley).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_m: Option<f32>,
    /// Buildings (by id) and roads (by id) that belong to the place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buildings: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roads: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceType {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Straight-line travel speed from the staging base, m/s. `None` keeps the
    /// built-in default for the id (air tankers 90, buses 8, anything else 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mps: Option<f32>,
    /// Makes the resource a monitoring drone (see [`DroneSpec`]). The id
    /// `drones` is a drone with the default parameters even when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone: Option<DroneSpec>,
}

impl ResourceType {
    /// Travel speed in metres per second.
    pub fn speed(&self) -> f32 {
        self.speed_mps.unwrap_or(match self.id.as_str() {
            "air_tankers" => 90.0,
            "buses" => 8.0,
            "drones" => 20.0,
            _ => 10.0,
        })
    }
}

/// Parameters of a monitoring drone. Times are simulated seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DroneSpec {
    /// Largest area one mission may cover: the bounding box of the ordered
    /// zones of the 8×8 grid may hold at most this many zones.
    pub max_zones: u32,
    /// Time on station before the battery obliges it to return.
    pub endurance_s: i64,
    /// Time from landing at the staging base to being available again.
    pub turnaround_s: i64,
    /// Interval between map updates of the covered zones while on station.
    pub update_interval_s: i64,
    /// The drone does not fly (and is recalled) when the wind blows faster, km/h.
    /// `None`: no wind limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_limit_kmh: Option<f32>,
}

impl Default for DroneSpec {
    fn default() -> Self {
        Self {
            max_zones: 4,
            endurance_s: 2400,
            turnaround_s: 900,
            update_interval_s: 300,
            wind_limit_kmh: None,
        }
    }
}

impl DroneSpec {
    pub fn check(&self) -> Result<()> {
        anyhow::ensure!(
            (1..=64).contains(&self.max_zones)
                && (60..=86_400).contains(&self.endurance_s)
                && (0..=86_400).contains(&self.turnaround_s)
                && (1..=self.endurance_s).contains(&self.update_interval_s)
                && self.wind_limit_kmh.is_none_or(|w| w.is_finite() && w > 0.0),
            "invalid drone parameters"
        );
        Ok(())
    }
}

/// What the operational model adds to one role: units of a resource, the
/// command that uses them and, if the scenario lacks it, the resource itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalGrant {
    pub role: String,
    pub resource: String,
    pub qty: u32,
    /// Command kind the role may propose (e.g. `DeployEngine`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Resource type added when no type has `resource` as its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub define: Option<ResourceType>,
}

/// The grant older scenario packages relied on.
pub(crate) fn legacy_grants() -> Vec<OperationalGrant> {
    vec![OperationalGrant {
        role: "fire_commander".into(),
        resource: "engines".into(),
        qty: 2,
        action: Some("DeployEngine".into()),
        define: Some(ResourceType {
            id: "engines".into(),
            name: "Road engines".into(),
            description: "2,500 L training engine; 60 m hose reach; refills at staging".into(),
            speed_mps: None,
            drone: None,
        }),
    }]
}

/// Turn length when the scenario does not set one.
pub const DEFAULT_TURN_MINUTES: u32 = 30;

fn default_turn_minutes() -> u32 {
    DEFAULT_TURN_MINUTES
}

/// Place deployments start from when the scenario names none: where older
/// scenario packages staged their resources.
fn legacy_staging() -> Option<String> {
    Some("Valverde Town Hall".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    /// national / regional / local
    pub level: String,
    /// ISO 639-1 code of the player's default language.
    pub language: String,
    pub description: String,
    /// Command type names (see `sim::Command`) this role may propose.
    pub allowed_actions: Vec<String>,
    /// Resource id -> units available per turn.
    pub resources: BTreeMap<String, u32>,
}

/// External capacity a role can request; arrives after a delay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReinforcementOffer {
    pub resource: String,
    pub max_qty: u32,
    pub delay_min: u32,
    pub source: String,
    /// Role that receives the units on arrival.
    pub assigned_to: String,
}

/// One prepared inject of the Master Scenario Events List.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InjectTemplate {
    pub id: String,
    /// Suggested turn (the EXCON decides when to send).
    pub turn: u32,
    pub title: String,
    /// The simulated sender (e.g. "112 Dispatch", "ERCC").
    pub sender: String,
    /// Role ids, or `"all"`.
    pub recipients: Vec<String>,
    /// Text in the working language (English).
    pub text: String,
    pub objective: String,
    /// Incident/event name used only to group injects in the EXCON view.
    #[serde(default)]
    pub event: String,
    /// Planned instant, simulated seconds from the start of the exercise.
    /// Without it the inject is planned at the start of its suggested `turn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_s: Option<i64>,
    /// World changes that go with the inject: run by EXCON when the inject is
    /// sent, `offset_s` from it (see [`InjectAction`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<InjectAction>,
}

impl InjectTemplate {
    /// Planned instant in simulated seconds: its own `at_s`, otherwise the
    /// start of its suggested turn counted with `turn_minutes`.
    pub fn planned_s(&self, turn_minutes: u32) -> i64 {
        self.at_s
            .unwrap_or_else(|| (self.turn.max(1) as i64 - 1) * turn_minutes as i64 * 60)
    }
}

/// A scripted world change that belongs to an inject. With `offset_s >= 0` it
/// runs when EXCON sends the inject: at once as an EXCON command for 0,
/// otherwise approved to apply `offset_s` after the send, during the following
/// resolution. With `offset_s < 0` it is a pre-action: it runs by itself
/// `-offset_s` before the inject's planned time during the time scan (unless
/// EXCON disabled it or skipped the inject), or at send time if the inject is
/// sent early. Ids are unique across the scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InjectAction {
    pub id: String,
    pub title: String,
    /// An EXCON `sim::Command` as JSON; validated when the exercise loads.
    /// `null` for a `reveal` action (exactly one of the two is set).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub command: serde_json::Value,
    /// Map knowledge disclosed to the inject's recipients instead of a world
    /// change (fog of war is not world state: run by the exercise, never by
    /// `World::apply`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<RevealSpec>,
    /// Seconds after the inject is sent; negative: before its planned time
    /// (never for a `reveal`).
    #[serde(default, alias = "delay_s")]
    pub offset_s: i64,
    #[serde(default)]
    pub note: String,
}

/// Layers EXCON may disclose for a set of zones.
pub const ZONE_LAYERS: [&str; 5] = ["fire", "buildings", "roads", "lines", "units"];

/// Which zones an exact reveal covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RevealZones {
    /// Computed when the action runs.
    Rule(ZoneRule),
    List(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneRule {
    /// Zones with burning cells.
    Burning,
    /// Burning zones and their eight neighbours.
    BurningAndAdjacent,
}

/// Map knowledge an inject discloses to its recipients (`source` defaults to
/// the inject's sender).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RevealSpec {
    /// The truth in the chosen zones and layers, as of delivery.
    Exact {
        zones: RevealZones,
        #[serde(default = "default_reveal_layers")]
        layers: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    /// An unverified estimate of the actual fire (centroid, area, direction
    /// of spread) with a deliberate, deterministic error.
    Approximate {
        /// Largest position error, metres.
        #[serde(default = "default_position_error")]
        position_error_m: f32,
        /// Largest relative area error (0.25 = ±25 %).
        #[serde(default = "default_area_error")]
        area_error_frac: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
}

fn default_reveal_layers() -> Vec<String> {
    vec!["fire".into()]
}
fn default_position_error() -> f32 {
    300.0
}
fn default_area_error() -> f32 {
    0.25
}

impl RevealSpec {
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Exact { source, .. } | Self::Approximate { source, .. } => source.as_deref(),
        }
    }
}

impl Scenario {
    /// Checks one inject action: exactly one of command / reveal, a reveal
    /// never before its inject, known layers and zones.
    pub fn check_action(&self, a: &InjectAction) -> Result<()> {
        match &a.reveal {
            None => anyhow::ensure!(a.command.is_object(), "action {} command must be a JSON object", a.id),
            Some(r) => {
                anyhow::ensure!(
                    a.command.is_null(),
                    "action {} has both a command and a reveal: choose one",
                    a.id
                );
                anyhow::ensure!(a.offset_s >= 0, "reveal action {} cannot come before its inject", a.id);
                match r {
                    RevealSpec::Exact { zones, layers, .. } => {
                        anyhow::ensure!(!layers.is_empty(), "reveal action {} needs at least one layer", a.id);
                        if let Some(l) = layers.iter().find(|l| !ZONE_LAYERS.contains(&l.as_str())) {
                            anyhow::bail!("reveal action {}: unknown layer {l}", a.id);
                        }
                        if let RevealZones::List(z) = zones {
                            anyhow::ensure!(!z.is_empty(), "reveal action {} needs at least one zone", a.id);
                            if let Some(z) = z.iter().find(|z| self.zone_bounds(&z.to_uppercase()).is_none()) {
                                anyhow::bail!("reveal action {}: unknown zone {z}", a.id);
                            }
                        }
                    }
                    RevealSpec::Approximate { position_error_m, area_error_frac, .. } => anyhow::ensure!(
                        (0.0..=5000.0).contains(position_error_m) && (0.0..=1.0).contains(area_error_frac),
                        "reveal action {}: position_error_m must be 0..5000 and area_error_frac 0..1",
                        a.id
                    ),
                }
            }
        }
        Ok(())
    }
}

/// Legacy standalone timeline action, still read from old packs and converted
/// into an [`InjectAction`] on load (see [`Scenario::migrate_legacy_timeline`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineAction {
    pub id: String,
    pub title: String,
    /// Planned instant, simulated seconds from the start of the exercise.
    pub at_s: i64,
    /// An EXCON `sim::Command` as JSON; validated when the exercise loads.
    pub command: serde_json::Value,
    /// Inject this action accompanies (informational link in the timeline).
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub note: String,
}

/// Optional supplied visual heightfield, sampled at grid nodes (row 0 north).
/// Rendering only: routing, slope, fire and exposure continue using `dem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderTerrain {
    pub rows: usize,
    pub cols: usize,
    pub posting_m: f32,
    pub world_size_m: [f32; 2],
    pub elev: Vec<f32>,
}

impl RenderTerrain {
    /// Row and column ranges of the postings that can lie within `radius` of `at`.
    pub fn footprint(&self, at: Pos, radius: f32) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let span = |lo: f32, hi: f32, n: usize| {
            let a = (lo / self.posting_m).floor().max(0.) as usize;
            let b = ((hi / self.posting_m).ceil().max(0.) as usize + 1).min(n);
            a.min(n)..b
        };
        let cols = span(at[0] - radius, at[0] + radius, self.cols);
        let rows = span(
            self.world_size_m[1] - at[1] - radius,
            self.world_size_m[1] - at[1] + radius,
            self.rows,
        );
        (rows, cols)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub tactical_policy: TacticalPolicy,
    #[serde(default)]
    pub operations: Operations,
    #[serde(default)]
    pub fuel_definitions: Vec<FuelDefinition>,
    pub id: String,
    pub name: String,
    pub description: String,
    pub grid: GridSpec,
    /// Row-major fuel codes (see [`fuel`]).
    pub fuel: Vec<i32>,
    /// Row-major elevation, metres.
    pub dem: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_terrain: Option<RenderTerrain>,
    /// Exercise clock at t = 0, seconds since Day 1 00:00.
    pub start_clock_s: i64,
    /// Length of one turn in minutes: sizes the timeline bands and places
    /// injects that have no `at_s`. EXCON may still change the live length.
    #[serde(default = "default_turn_minutes")]
    pub turn_minutes: u32,
    pub weather: WeatherSpec,
    pub buildings: Vec<Building>,
    pub roads: Vec<Road>,
    pub sectors: Vec<Sector>,
    /// Borghi, for `EvacuateDistrict`. Empty: the command is unavailable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub districts: Vec<District>,
    #[serde(default)]
    pub district_evacuation: DistrictEvacuation,
    pub zones: ZoneGrid,
    pub places: Vec<Place>,
    pub resource_types: Vec<ResourceType>,
    pub roles: Vec<Role>,
    /// Parts the EXCON plays (e.g. «Centrale 112»), the senders of injects. Separate from `roles`.
    #[serde(default)]
    pub excon_roles: Vec<String>,
    /// Name under which the exercise control speaks to players when no other
    /// persona applies. Empty: a built-in neutral name ("Operations room").
    #[serde(default)]
    pub default_persona: String,
    /// Texts of this scenario in other languages: language code -> English
    /// source text -> translation (role and resource names and descriptions,
    /// inject titles, senders). Overrides [`glossary`]; missing entries stay English
    /// or are translated at runtime.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
    pub reinforcements: Vec<ReinforcementOffer>,
    pub msel: Vec<InjectTemplate>,
    /// Legacy standalone actions: read for old files, emptied by
    /// [`Scenario::migrate_legacy_timeline`] and never written back.
    #[serde(default, skip_serializing)]
    pub timeline: Vec<TimelineAction>,
    /// Named place resources travel from (the exercise can move it later).
    /// Unknown or `None`: the centre of the map.
    #[serde(default = "legacy_staging")]
    pub staging_base: Option<String>,
    /// Added to roles when the operational model is enabled.
    #[serde(default = "legacy_grants")]
    pub operational_grants: Vec<OperationalGrant>,
}

impl Scenario {
    /// English source text -> text in `lang`: built-in glossary overlaid with
    /// this scenario's `translations`. Empty for the working language.
    pub fn display_map(&self, lang: &str) -> BTreeMap<String, String> {
        if lang == "en" {
            return BTreeMap::new();
        }
        let mut map = glossary::builtin(lang);
        if let Some(own) = self.translations.get(lang) {
            map.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        map.retain(|k, v| k != v && !v.trim().is_empty());
        map
    }

    /// Scenario-authored texts players read, which need a translation.
    pub fn translatable_texts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.roles {
            out.extend([r.name.clone(), r.description.clone()]);
        }
        for r in &self.resource_types {
            out.extend([r.name.clone(), r.description.clone()]);
        }
        for m in &self.msel {
            out.extend([m.title.clone(), m.sender.clone()]);
        }
        out.retain(|t| !t.trim().is_empty());
        out.sort();
        out.dedup();
        out
    }

    /// The persona exercise control speaks as by default, in `language`.
    pub fn default_persona_name(&self, language: &str) -> String {
        let name = self.default_persona.trim();
        if !name.is_empty() {
            return name.into();
        }
        match language {
            "it" => "Sala operativa",
            _ => "Operations room",
        }
        .into()
    }

    /// Enable the enhanced training model, adding engine capability to the
    /// synthetic Fire Commander when loading an older scenario package.
    pub fn enable_operational_model(&mut self) {
        let spotting = self.operations.spotting;
        self.operations = Operations::enhanced();
        self.operations.spotting = spotting;
        for grant in self.operational_grants.clone() {
            let Some(role) = self.roles.iter_mut().find(|r| r.id == grant.role) else {
                continue;
            };
            role.resources
                .entry(grant.resource.clone())
                .or_insert(grant.qty);
            if let Some(action) = &grant.action {
                if !role.allowed_actions.contains(action) {
                    role.allowed_actions.push(action.clone());
                }
            }
            if let Some(define) = grant.define {
                if !self.resource_types.iter().any(|r| r.id == grant.resource) {
                    self.resource_types.push(define);
                }
            }
        }
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Scenario> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut scn: Scenario =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        scn.migrate_legacy_timeline()?;
        scn.check()?;
        Ok(scn)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Internal consistency: raster sizes, unique ids, references.
    pub fn check(&self) -> Result<()> {
        let p = &self.tactical_policy;
        anyhow::ensure!(
            !p.version.trim().is_empty()
                && p.version.len() <= 100
                && p.sector_radius_m.is_finite()
                && (0.0..=2000.0).contains(&p.sector_radius_m)
                && p.hose_reach_m.is_finite()
                && (1.0..=300.0).contains(&p.hose_reach_m)
                && p.wet_limit_pts.is_finite()
                && (1.0..=100.0).contains(&p.wet_limit_pts)
                && (1..=3600).contains(&p.reassess_s)
                && (0..=3600).contains(&p.regroup_s)
                && p.parking_danger.is_finite()
                && p.work_danger.is_finite()
                && (0.0..=1.0).contains(&p.parking_danger)
                && (p.parking_danger..=1.0).contains(&p.work_danger),
            "invalid tactical policy"
        );
        anyhow::ensure!(
            self.grid.rows > 0
                && self.grid.cols > 0
                && self.grid.cellsize.is_finite()
                && self.grid.cellsize > 0.0,
            "invalid grid"
        );
        if let Some(t) = &self.render_terrain {
            anyhow::ensure!(t.rows >= 2 && t.cols >= 2 && t.rows.checked_mul(t.cols).is_some_and(|n| n <= 4_194_304 && n == t.elev.len())
                && t.posting_m.is_finite() && t.posting_m > 0.0 && t.posting_m <= self.grid.cellsize
                && t.elev.iter().all(|h|h.is_finite())
                && t.world_size_m.iter().all(|v|v.is_finite() && *v>0.0)
                && (t.world_size_m[0]-self.grid.width_m()).abs()<0.1
                && (t.world_size_m[1]-self.grid.height_m()).abs()<0.1
                && ((t.cols-1) as f32*t.posting_m-t.world_size_m[0]).abs()<0.1
                && ((t.rows-1) as f32*t.posting_m-t.world_size_m[1]).abs()<0.1,
                "visual terrain must be finite, <=4M nodes, no coarser than physics and cover exactly the scenario extent");
        }
        let n = self.grid.len();
        anyhow::ensure!(self.dem.iter().all(|x| x.is_finite()), "non-finite terrain");
        for road in &self.roads {
            anyhow::ensure!(
                road.points.len() >= 2
                    && road.points.iter().all(|p| p.iter().all(|x| x.is_finite())),
                "invalid road {}",
                road.id
            );
        }
        anyhow::ensure!(
            self.fuel.len() == n,
            "fuel raster has {} cells, grid needs {n}",
            self.fuel.len()
        );
        anyhow::ensure!(
            self.dem.len() == n,
            "dem raster has {} cells, grid needs {n}",
            self.dem.len()
        );
        let building_ids = self
            .buildings
            .iter()
            .map(|b| b.id)
            .collect::<std::collections::BTreeSet<_>>();
        let road_ids = self
            .roads
            .iter()
            .map(|r| &r.id)
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            building_ids.len() == self.buildings.len() && road_ids.len() == self.roads.len(),
            "duplicate building or road id"
        );
        let mut district_keys = std::collections::BTreeSet::new();
        for d in &self.districts {
            let own: std::collections::BTreeSet<String> =
                [normalise(&d.id), normalise(&d.name)].into();
            anyhow::ensure!(
                own.iter().all(|k| district_keys.insert(k.clone())),
                "duplicate district {}",
                d.id
            );
            anyhow::ensure!(
                !d.buildings.is_empty() && d.buildings.iter().all(|id| building_ids.contains(id)),
                "district {} lists no or unknown buildings",
                d.id
            );
            if let Some(g) = &d.gathering {
                anyhow::ensure!(
                    self.buildings.iter().any(|b| b.name.as_deref() == Some(g.as_str())
                        && b.shelter_capacity > 0),
                    "district {}: gathering area «{g}» is not a building with capacity",
                    d.id
                );
            }
        }
        let e = &self.district_evacuation;
        anyhow::ensure!(
            e.per_min.is_finite() && e.per_min > 0.0 && e.prep_min <= 240,
            "invalid district evacuation pace"
        );
        for b in &self.buildings {
            anyhow::ensure!(
                self.sector(&b.sector).is_some(),
                "building {} in unknown sector {}",
                b.id,
                b.sector
            );
        }
        for r in &self.resource_types {
            if let Some(d) = self.drone_spec(&r.id) {
                d.check().with_context(|| format!("resource {}", r.id))?;
            }
        }
        for r in &self.reinforcements {
            anyhow::ensure!(
                self.role(&r.assigned_to).is_some(),
                "reinforcement assigned to unknown role {}",
                r.assigned_to
            );
        }
        for i in &self.msel {
            for rcp in &i.recipients {
                anyhow::ensure!(
                    rcp == "all" || self.role(rcp).is_some(),
                    "inject {} to unknown role {rcp}",
                    i.id
                );
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for i in &self.msel {
            for a in &i.actions {
                anyhow::ensure!(
                    !a.id.trim().is_empty() && !a.title.trim().is_empty(),
                    "action {} of inject {} needs an id and a title",
                    a.id,
                    i.id
                );
                anyhow::ensure!(seen.insert(a.id.as_str()), "duplicate action id {}", a.id);
                self.check_action(a)?;
            }
        }
        Ok(())
    }

    /// Convert legacy standalone `timeline` actions into inject actions and
    /// empty the legacy list. An action with `after` joins that inject; one
    /// without joins the latest inject planned at or before its time, or the
    /// first inject if none is. `offset_s` is the time past the inject's
    /// planned instant (negative: a pre-action). Fails when there is no inject to
    /// attach to or `after` names an unknown one.
    pub fn migrate_legacy_timeline(&mut self) -> Result<()> {
        if self.timeline.is_empty() {
            return Ok(());
        }
        let tm = self.turn_minutes;
        let planned: Vec<i64> = self.msel.iter().map(|i| i.planned_s(tm)).collect();
        let legacy = std::mem::take(&mut self.timeline);
        for a in legacy {
            let idx = match &a.after {
                Some(id) => self
                    .msel
                    .iter()
                    .position(|i| &i.id == id)
                    .with_context(|| format!("timeline action {} follows unknown inject {id}", a.id))?,
                None => {
                    let latest = (0..planned.len())
                        .filter(|n| planned[*n] <= a.at_s)
                        .max_by_key(|n| (planned[*n], *n));
                    let first = (0..planned.len()).min_by_key(|n| (planned[*n], *n));
                    latest.or(first).with_context(|| {
                        format!("timeline action {} has no inject to attach to", a.id)
                    })?
                }
            };
            self.msel[idx].actions.push(InjectAction {
                id: a.id,
                title: a.title,
                command: a.command,
                reveal: None,
                offset_s: a.at_s - planned[idx],
                note: a.note,
            });
        }
        Ok(())
    }

    pub fn role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }

    /// Drone parameters of a resource, if it is a drone.
    pub fn drone_spec(&self, resource: &str) -> Option<DroneSpec> {
        match self.resource_types.iter().find(|r| r.id == resource) {
            Some(r) => r.drone.clone().or_else(|| (resource == "drones").then(DroneSpec::default)),
            None => None,
        }
    }

    pub fn sector(&self, id: &str) -> Option<&Sector> {
        self.sectors.iter().find(|s| s.id == id)
    }

    pub fn road(&self, id: &str) -> Option<&Road> {
        self.roads.iter().find(|r| r.id == id)
    }

    pub fn sector_at(&self, p: Pos) -> Option<&Sector> {
        self.sectors.iter().find(|s| s.contains(p))
    }

    /// Evacuation zone (e.g. `"F4"`) containing `p`.
    pub fn zone_of(&self, p: Pos) -> Option<String> {
        self.zones.zone_of(p, self.grid.height_m())
    }

    pub fn zone_bounds(&self, name: &str) -> Option<[f32; 4]> {
        self.zones.bounds(name, self.grid.height_m())
    }

    pub fn zone_edge(&self, code: &str) -> Option<(Pos, Pos)> {
        self.zones.edge(code, self.grid.height_m())
    }

    pub fn zone_centre(&self, name: &str) -> Option<Pos> {
        self.zone_bounds(name)
            .map(|b| [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5])
    }

    pub fn cell_of(&self, p: Pos) -> Option<Cell> {
        let cs = self.grid.cellsize;
        if p[0] < 0.0 || p[1] < 0.0 {
            return None;
        }
        let col = (p[0] / cs) as usize;
        let from_south = (p[1] / cs) as usize;
        if col >= self.grid.cols || from_south >= self.grid.rows {
            return None;
        }
        Some(Cell {
            row: self.grid.rows - 1 - from_south,
            col,
        })
    }

    pub fn cell_centre(&self, c: Cell) -> Pos {
        let cs = self.grid.cellsize;
        [
            (c.col as f32 + 0.5) * cs,
            ((self.grid.rows - 1 - c.row) as f32 + 0.5) * cs,
        ]
    }

    pub fn fuel_at(&self, c: Cell) -> i32 {
        self.fuel[self.grid.index(c)]
    }

    /// Display name of a fuel class: the scenario table when present, the
    /// legacy names otherwise.
    pub fn fuel_name(&self, id: i32) -> String {
        if self.fuel_definitions.is_empty() {
            return fuel::name(id).into();
        }
        self.fuel_definitions
            .iter()
            .find(|f| f.id == id)
            .map_or_else(|| "unknown".into(), |f| f.name.clone())
    }

    pub fn elevation_at(&self, c: Cell) -> f32 {
        self.dem[self.grid.index(c)]
    }

    pub fn is_burnable(&self, c: Cell) -> bool {
        if self.fuel_definitions.is_empty() {
            self.fuel_at(c) != fuel::NON_VEGETATED && self.fuel_at(c) != 0
        } else {
            self.fuel_definitions
                .iter()
                .any(|f| f.id == self.fuel_at(c) && f.burn)
        }
    }

    /// Resolve a free-form name against named places, facilities, sectors and
    /// roads — case-insensitive, aliases included. Returns the world position.
    /// A borgo by id or name (also «borgo di X»), ignoring case and punctuation.
    pub fn district(&self, key: &str) -> Option<&District> {
        let key = normalise(key);
        let bare = key.strip_prefix("borgodi").unwrap_or(&key);
        self.districts
            .iter()
            .find(|d| [normalise(&d.id), normalise(&d.name)].iter().any(|k| k == &key || k == bare))
    }

    pub fn resolve_place(&self, name: &str) -> Option<Pos> {
        let key = normalise(name);
        if key.is_empty() {
            return None;
        }
        for p in &self.places {
            if normalise(&p.name) == key || p.aliases.iter().any(|a| normalise(a) == key) {
                return Some(p.pos);
            }
        }
        for b in &self.buildings {
            if b.name.as_deref().map(normalise).as_deref() == Some(key.as_str()) {
                return Some(b.pos);
            }
        }
        for s in &self.sectors {
            if normalise(&s.id) == key || normalise(&s.name) == key {
                return Some(s.centre());
            }
        }
        if let Some(p) = self.zone_centre(name) {
            return Some(p);
        }
        for r in &self.roads {
            if normalise(&r.id) == key || normalise(&r.name) == key {
                return r.points.get(r.points.len() / 2).copied();
            }
        }
        None
    }
}

/// Lower-case, alphanumerics only — so "Pineta Primary School", "pineta
/// primary-school" and "PINETA  PRIMARY SCHOOL" all match.
pub fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Format an exercise clock value (`start_clock_s + t`) as `Day N HH:MM`.
pub fn format_clock(clock_s: i64) -> String {
    let day = clock_s.div_euclid(86_400) + 1;
    let rem = clock_s.rem_euclid(86_400);
    format!("Day {day} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fuel_names_come_from_the_scenario_table_when_present() {
        let mut s = synthetic::generate(synthetic::DEFAULT_SEED);
        assert_eq!(s.fuel_name(fuel::CONIFERS), "conifers");
        s.fuel_definitions = serde_json::from_str(include_str!("../data/fuels_eu12.json")).unwrap();
        assert_eq!(s.fuel_name(12), "conifers - high susceptibility");
        assert_eq!(s.fuel_name(99), "unknown");
    }

    #[test]
    fn retired_communication_keys_are_ignored_by_old_scenarios() {
        let s = synthetic::generate(synthetic::DEFAULT_SEED);
        let mut v = serde_json::to_value(&s).unwrap();
        v["communication_sites"] =
            serde_json::json!([{"id": "mast", "pos": [100.0, 100.0], "radius_m": 500.0}]);
        v["operations"]["communications"] = serde_json::json!(true);
        let old: Scenario = serde_json::from_value(v).expect("retired keys must still load");
        assert_eq!(
            serde_json::to_value(&old.operations).unwrap(),
            serde_json::to_value(&s.operations).unwrap()
        );
        assert!(old.check().is_ok());
    }

    #[test]
    fn visual_category_is_optional_and_does_not_replace_operational_kind() {
        let legacy = serde_json::json!({"id":1,"kind":"sports_centre","name":"Assembly",
            "pos":[20.,20.],"size":[30.,30.],"occupants":0,"sector":"a","shelter_capacity":100});
        let b: super::Building = serde_json::from_value(legacy.clone()).unwrap();
        assert!(b.visual_kind.is_none());
        assert_eq!(serde_json::to_value(&b).unwrap(), legacy);
        let mut authored = legacy;
        authored["visual_kind"] = serde_json::json!("assembly");
        let b: super::Building = serde_json::from_value(authored.clone()).unwrap();
        assert_eq!(b.kind, super::BuildingKind::SportsCentre);
        assert_eq!(b.shelter_capacity, 100);
        assert!(!b.kind.needs_transport());
        assert_eq!(serde_json::to_value(&b).unwrap(), authored);
    }

    use super::*;

    #[test]
    fn supplied_visual_terrain_is_separate_and_validated() {
        let mut s = synthetic::generate(synthetic::DEFAULT_SEED);
        let original = s.dem.clone();
        s.render_terrain = Some(RenderTerrain {
            rows: 401,
            cols: 401,
            posting_m: 10.0,
            world_size_m: [4000.0, 4000.0],
            elev: vec![123.0; 401 * 401],
        });
        s.check().unwrap();
        assert_eq!(s.dem, original);
        s.render_terrain.as_mut().unwrap().world_size_m[0] = 3990.0;
        assert!(s.check().is_err());
        s.render_terrain.as_mut().unwrap().world_size_m[0] = 4000.0;
        s.render_terrain.as_mut().unwrap().elev[10] = f32::NAN;
        assert!(s.check().is_err());
    }

    fn legacy(id: &str, at_s: i64, after: Option<&str>) -> TimelineAction {
        TimelineAction {
            id: id.into(),
            title: id.into(),
            at_s,
            command: serde_json::json!({"type": "SetWeather", "wind_dir_deg": 10.0}),
            after: after.map(String::from),
            note: String::new(),
        }
    }

    #[test]
    fn legacy_timeline_actions_become_inject_actions() {
        let mut s = synthetic::generate(synthetic::DEFAULT_SEED);
        for (n, i) in s.msel.iter_mut().enumerate() {
            i.at_s = Some(1000 + n as i64 * 1000);
        }
        let (first, second) = (s.msel[0].id.clone(), s.msel[1].id.clone());
        s.timeline = vec![
            legacy("linked", 1500, Some(&second)), // after an inject: attaches to it, offset 1500 - 2000
            legacy("between", 2500, None),         // latest inject at or before: the second, +500
            legacy("before_all", 200, None),       // earlier than any: the first, -800 (pre-action)
        ];
        s.migrate_legacy_timeline().unwrap();
        assert!(s.timeline.is_empty());
        let find = |id: &str| {
            s.msel
                .iter()
                .flat_map(|i| i.actions.iter().map(move |a| (i.id.clone(), a.offset_s, a.id.clone())))
                .find(|a| a.2 == id)
                .unwrap()
        };
        assert_eq!(find("linked"), (second.clone(), -500, "linked".into()));
        assert_eq!(find("between"), (second, 500, "between".into()));
        assert_eq!(find("before_all"), (first, -800, "before_all".into()));
        s.check().unwrap();
        // The legacy field is never written back, and old files still load.
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("timeline").is_none());
        let again: Scenario = serde_json::from_value(v).unwrap();
        assert_eq!(again.msel.iter().map(|i| i.actions.len()).sum::<usize>(), 10);
    }

    #[test]
    fn legacy_timeline_needs_an_inject_and_unique_ids() {
        let mut s = synthetic::generate(synthetic::DEFAULT_SEED);
        s.timeline = vec![legacy("x", 0, Some("missing"))];
        assert!(s.migrate_legacy_timeline().is_err());
        let mut s = synthetic::generate(synthetic::DEFAULT_SEED);
        s.timeline = vec![legacy("dup", 0, None), legacy("dup", 10, None)];
        s.migrate_legacy_timeline().unwrap();
        assert!(s.check().is_err(), "duplicate action ids are refused");
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(13 * 3600 + 5 * 60), "Day 1 13:05");
        assert_eq!(format_clock(86_400 * 9 + 60), "Day 10 00:01");
    }

    #[test]
    fn zone_grid_names_and_bounds() {
        let z = ZoneGrid {
            cols: 8,
            rows: 8,
            size_m: 500.0,
        };
        assert_eq!(z.name(0, 0), "A1");
        assert_eq!(z.parse("c 3"), Some((2, 2)));
        assert_eq!(z.parse("I1"), None);
        assert_eq!(z.parse("A9"), None);
        // A1 is the north-west corner.
        assert_eq!(z.zone_of([10.0, 3990.0], 4000.0).as_deref(), Some("A1"));
        assert_eq!(z.zone_of([3990.0, 10.0], 4000.0).as_deref(), Some("H8"));
        assert_eq!(z.bounds("A1", 4000.0), Some([0.0, 3500.0, 500.0, 4000.0]));
        assert_eq!(z.names().len(), 64);
        assert_eq!(
            z.edge("A1:S", 4000.0),
            Some(([0.0, 3500.0], [500.0, 3500.0]))
        );
        assert_eq!(z.edge("A1:X", 4000.0), None);
    }

    #[test]
    fn normalise_ignores_case_and_punctuation() {
        assert_eq!(
            normalise("Pineta Primary-School"),
            normalise("pineta primary school")
        );
    }
}
