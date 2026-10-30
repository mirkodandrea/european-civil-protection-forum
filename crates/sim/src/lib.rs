//! The world: fire + buildings + roads + resources, changed only by
//! [`Command`]s and by the passage of simulated time.
//!
//! - [`World::validate_batch`] is deterministic and side-effect free: it tells
//!   the EXCON whether a proposal is feasible (authority, resources, places).
//! - [`World::apply`] executes one command.
//! - [`World::advance`] moves simulated time in 60 s sub-steps: fire spread,
//!   building exposure and damage, evacuations completing, roads cut by fire,
//!   reinforcements arriving. It returns the [`SimEvent`]s that happened.
//! - [`World::snapshot`] / [`World::restore`] roll the whole world back.
//!
//! No people are modelled individually: buildings carry occupant counts.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Result;
use fire::{CellFire, FireSim, FireSnapshot};
use scenario::{normalise, BuildingKind, Pos, Scenario};
use serde::Serialize;

pub mod command;
pub mod digest;
pub mod drone;
pub mod hazard;
pub mod impact;
pub mod network;
pub mod traffic;
pub mod travel;
pub use drone::{MonitorSample, MonitorState, SampleKind};
pub use travel::{DeploymentTask, TaskStatus, TravelSetting};

pub use command::{catalogue, catalogue_for, missing_params, Area, Command, Damage, Loc, ALL_KINDS, MASTER_ONLY};
pub use fire::FireOptions;

/// Sub-step of [`World::advance`], seconds. Buildings, evacuations, roads and
/// arrivals update every simulated second; the fire core is event-driven, so
/// its result does not depend on this.
pub const SUBSTEP_S: i64 = 1;
/// Metres of fire line one ground crew can cut in an assigned task.
pub const LINE_M_PER_CREW: f32 = 600.0;
pub const DEFAULT_LINE_WIDTH_M: f32 = 40.0;
pub const DROP_RADIUS_M: f32 = 60.0;
/// Litres per square metre of one drop: +36 moisture points, past extinction for a while.
pub const DROP_LITRES_PER_M2: f32 = 1.2;
pub const BUS_SEATS: u32 = 50;
/// Minutes an evacuation takes (unwarned / warned). Transport-dependent facilities add 10.
pub const EVAC_MIN: i64 = 20;
pub const EVAC_WARNED_MIN: i64 = 10;
/// Weighted exposure seconds for damage / destruction.
pub const DAMAGE_EXPOSURE_S: f32 = 300.0;
pub const DESTROY_EXPOSURE_S: f32 = 900.0;
/// A building is "threatened" with fire burning within this distance.
pub const THREAT_RADIUS_M: f32 = 300.0;
/// Buildings sit on non-burnable plots, so the fire "reaches" a building when
/// a confirmed burning cell is at the edge of its plot: within this distance.
pub const CONTACT_RADIUS_M: f32 = 50.0;

/// Distance (building → cell centre) within which a confirmed burning cell
/// counts as fire that has **reached** the building: [`CONTACT_RADIUS_M`], or
/// the building's cell and its eight neighbours on coarse grids. Only fire in
/// contact damages a building; anything farther (distant fire, the ensemble
/// halo, modelled radiant/ember loads) can only make it *threatened*.
pub fn contact_radius_m(cellsize: f32) -> f32 {
    (cellsize * 1.5).max(CONTACT_RADIUS_M)
}
/// Radius used by an `Extinguish` on a named point (a place without its own
/// `radius_m`, a facility, a road) — not a zone or a sector.
pub const PLACE_EXTINGUISH_RADIUS_M: f32 = 250.0;

/// The ground an `Extinguish` area covers, in world metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Footprint {
    /// `[x0, y0, x1, y1]`: a whole zone of the map grid, or a sector.
    Rect([f32; 4]),
    /// Centre and radius: a circle, or a named place.
    Disc(Pos, f32),
}

impl Footprint {
    pub fn area_ha(&self) -> f64 {
        match *self {
            Footprint::Rect(b) => ((b[2] - b[0]) as f64 * (b[3] - b[1]) as f64) / 10_000.0,
            Footprint::Disc(_, r) => std::f64::consts::PI * (r as f64).powi(2) / 10_000.0,
        }
    }
    pub fn centre(&self) -> Pos {
        match *self {
            Footprint::Rect(b) => [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5],
            Footprint::Disc(c, _) => c,
        }
    }
    /// Half the diagonal of a rectangle, the radius of a disc.
    pub fn reach_m(&self) -> f32 {
        match *self {
            Footprint::Rect(b) => 0.5 * ((b[2] - b[0]).powi(2) + (b[3] - b[1]).powi(2)).sqrt(),
            Footprint::Disc(_, r) => r,
        }
    }
}
/// Most vertices an `ExtinguishArea` polygon may have.
pub const MAX_POLYGON_VERTICES: usize = 200;

/// Area of a simple polygon (shoelace), square metres.
pub fn polygon_area_m2(poly: &[Pos]) -> f64 {
    let n = poly.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64
        })
        .sum();
    twice.abs() / 2.0
}

/// Mean of a polygon's vertices (a label point, not the area centroid).
pub fn polygon_centroid(poly: &[Pos]) -> Pos {
    let n = poly.len().max(1) as f32;
    let (x, y) = poly.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    [x / n, y / n]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Master,
    Role(String),
}

impl Actor {
    pub fn label(&self) -> &str {
        match self {
            Actor::Master => "master",
            Actor::Role(r) => r,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Ok { notes: Vec<String> },
    Rejected { reason: String },
}

impl Verdict {
    pub fn is_ok(&self) -> bool {
        matches!(self, Verdict::Ok { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SimEvent {
    pub t_s: i64,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Pool {
    pub total: u32,
    pub used: u32,
}

impl Pool {
    pub fn available(&self) -> u32 {
        self.total.saturating_sub(self.used)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildingState {
    pub id: u32,
    pub damage: Damage,
    pub threatened: bool,
    pub people_inside: u32,
    pub evacuated: u32,
    /// People leaving, and when they will be out.
    pub evac_pending: u32,
    pub evac_done_at: Option<i64>,
    pub evac_destination: Option<u32>,
    /// People inside when the building was damaged or destroyed.
    pub at_risk: u32,
    pub exposure_s: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoadState {
    pub closed: bool,
    pub cut_by_fire: bool,
    /// None uses legacy whole-road closure; Some stores stable directed link ids.
    /// Closures persist until scenario reset/rollback; no automatic reopening assumption.
    pub fire_closed_links: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineRec {
    pub from: Pos,
    pub to: Pos,
    pub width_m: f32,
    pub by: String,
    pub t_s: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DropRec {
    pub at: Pos,
    pub radius_m: f32,
    pub by: String,
    pub t_s: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Arrival {
    pub role: String,
    pub resource: String,
    pub qty: u32,
    pub at_s: i64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceAllocation {
    pub id: u64,
    pub role: String,
    pub resource: String,
    pub qty: u32,
    pub returned: u32,
    pub command: Command,
    pub deployed_at_s: i64,
    pub last_return_at_s: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceEvent {
    pub kind: String,
    pub allocation: Option<u64>,
    pub role: String,
    pub resource: String,
    pub qty: i64,
    pub t_s: i64,
}

/// Everything that changes, except the fire (which snapshots itself).
#[derive(Debug, Clone)]
pub struct Dyn {
    pub hazard: hazard::HazardState,
    pub traffic: traffic::Traffic,
    pub travel_settings: BTreeMap<String, BTreeMap<String, TravelSetting>>,
    pub tasks: Vec<DeploymentTask>,
    pub buildings: Vec<BuildingState>,
    pub roads: BTreeMap<String, RoadState>,
    pub lines: Vec<LineRec>,
    pub drops: Vec<DropRec>,
    /// Open shelters: building id -> people sheltered.
    pub shelters: BTreeMap<u32, u32>,
    pub warned: BTreeSet<String>,
    pub pools: BTreeMap<String, BTreeMap<String, Pool>>,
    pub allocations: Vec<ResourceAllocation>,
    pub resource_events: Vec<ResourceEvent>,
    pub arrivals: Vec<Arrival>,
    pub events: Vec<SimEvent>,
    /// Drones on station: their update schedule, by task id.
    pub monitors: BTreeMap<u64, MonitorState>,
    /// Drone observations not yet taken up by the exercise layer.
    pub monitor_samples: Vec<MonitorSample>,
}

#[derive(Clone)]
pub struct WorldSnapshot {
    fire: FireSnapshot,
    d: Dyn,
    time_s: i64,
}

impl WorldSnapshot {
    pub fn resource_pools(&self) -> &BTreeMap<String, BTreeMap<String, Pool>> {
        &self.d.pools
    }
}

/// Precomputed geometry: which fire cells matter to each building and road.
#[derive(Clone)]
struct Cache {
    /// Per building: (cell index, weight) within [`contact_radius_m`].
    near: Vec<Vec<(usize, f32)>>,
    roads: Vec<(String, Vec<usize>)>,
    /// Building ids of the borghi's gathering areas, resolved once.
    gathering: BTreeSet<u32>,
}

#[derive(Clone)]
pub struct World {
    pub scn: Arc<Scenario>,
    pub seed: u64,
    pub fire: FireSim,
    pub d: Dyn,
    cache: Cache,
    pub network: network::Network,
    next_allocation_id: u64,
}

impl World {
    /// A world at t = 0, with the scenario's opening fire lit (default ensemble).
    pub fn new(scn: Arc<Scenario>, seed: u64) -> Result<World> {
        World::with_options(scn, seed, fire::FireOptions::default())
    }

    pub fn with_options(
        scn: Arc<Scenario>,
        seed: u64,
        options: fire::FireOptions,
    ) -> Result<World> {
        let fire = FireSim::with_options(&scn, seed, options)?;
        let buildings = scn
            .buildings
            .iter()
            .map(|b| BuildingState {
                id: b.id,
                damage: Damage::Intact,
                threatened: false,
                people_inside: b.occupants,
                evacuated: 0,
                evac_pending: 0,
                evac_done_at: None,
                evac_destination: None,
                at_risk: 0,
                exposure_s: 0.0,
            })
            .collect();
        let roads = scn
            .roads
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    RoadState {
                        closed: false,
                        cut_by_fire: false,
                        fire_closed_links: scn.operations.segment_fire_closures.then(Vec::new),
                    },
                )
            })
            .collect();
        let pools = scn
            .roles
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    r.resources
                        .iter()
                        .map(|(k, v)| (k.clone(), Pool { total: *v, used: 0 }))
                        .collect(),
                )
            })
            .collect();
        let origin = scn
            .staging_base
            .as_deref()
            .and_then(|name| scn.resolve_place(name))
            .unwrap_or([scn.grid.width_m() * 0.5, scn.grid.height_m() * 0.5]);
        let travel_settings = scn
            .roles
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    scn.resource_types
                        .iter()
                        .map(|resource| {
                            let speed_mps = resource.speed();
                            (resource.id.clone(), TravelSetting { origin, speed_mps })
                        })
                        .collect(),
                )
            })
            .collect();
        let cache = Cache::build(&scn, &fire);
        let network = network::Network::build(&scn);
        let mut world = World {
            scn,
            seed,
            fire,
            d: Dyn {
                hazard: Default::default(),
                traffic: Default::default(),
                travel_settings,
                tasks: Vec::new(),
                buildings,
                roads,
                lines: Vec::new(),
                drops: Vec::new(),
                shelters: BTreeMap::new(),
                warned: BTreeSet::new(),
                pools,
                allocations: Vec::new(),
                resource_events: Vec::new(),
                arrivals: Vec::new(),
                events: Vec::new(),
                monitors: BTreeMap::new(),
                monitor_samples: Vec::new(),
            },
            cache,
            network,
            next_allocation_id: 1,
        };
        world.d.hazard.updated_s = -10;
        world.refresh_hazard();
        Ok(world)
    }

    pub fn time_s(&self) -> i64 {
        self.fire.time_s()
    }

    pub fn clock(&self) -> String {
        scenario::format_clock(self.scn.start_clock_s + self.time_s())
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            fire: self.fire.snapshot(),
            d: self.d.clone(),
            time_s: self.time_s(),
        }
    }

    pub fn restore(&mut self, s: &WorldSnapshot) -> Result<()> {
        let reseed = self.seed.wrapping_add(s.time_s as u64).wrapping_add(1);
        self.fire.restore(&s.fire, reseed)?;
        self.d = s.d.clone();
        Ok(())
    }

    /// Opening a turn preserves actual deployments and physical state.
    /// Resources become available only through an explicit ReturnResource command.
    pub fn new_turn(&mut self) {}

    fn resource_event(
        &mut self,
        kind: &str,
        allocation: Option<u64>,
        role: &str,
        resource: &str,
        qty: i64,
    ) {
        self.d.resource_events.push(ResourceEvent {
            kind: kind.into(),
            allocation,
            role: role.into(),
            resource: resource.into(),
            qty,
            t_s: self.time_s(),
        });
    }

    /// Requested quantities, independent of available units. Invalid places/actions yield no demand.
    pub fn resource_requirements(&self, cmd: &Command, actor: &Actor) -> BTreeMap<String, u32> {
        let Actor::Role(role) = actor else {
            return BTreeMap::new();
        };
        let mut pools = self.d.pools.clone();
        pools.insert(
            role.clone(),
            self.scn
                .resource_types
                .iter()
                .map(|r| {
                    (
                        r.id.clone(),
                        Pool {
                            total: u32::MAX,
                            used: 0,
                        },
                    )
                })
                .collect(),
        );
        if self.check_inner(cmd, actor, &mut pools).is_err() {
            return BTreeMap::new();
        }
        pools
            .get(role)
            .into_iter()
            .flat_map(|p| {
                p.iter()
                    .filter(|(_, p)| p.used > 0)
                    .map(|(resource, p)| (resource.clone(), p.used))
            })
            .collect()
    }

    fn event(&mut self, kind: &str, text: String, zone: Option<String>) -> SimEvent {
        let e = SimEvent {
            t_s: self.time_s(),
            kind: kind.into(),
            text,
            zone,
        };
        self.d.events.push(e.clone());
        e
    }

    // ------------------------------------------------------------------ lookups

    /// Resolved geometry and units for EXCON previews. Areas are nominal;
    /// affected cells are clipped to map/burnable fuel and shown by preview results.
    pub fn command_geometry(&self, cmd: &Command) -> serde_json::Value {
        use serde_json::json;
        match cmd {
            Command::Ignite { at, radius_m } => {
                let r = radius_m.unwrap_or(80.0);
                json!({"centre_m":self.resolve_loc(at).ok(),"radius_m":r,"nominal_area_ha":std::f64::consts::PI*(r as f64).powi(2)/10000.0})
            }
            Command::Extinguish { area } => match self.extinguish_footprint(area) {
                Ok(f @ Footprint::Rect(b)) => {
                    json!({"bounds_m":b,"nominal_area_ha":f.area_ha(),"effect":"puts out the fire inside; fuel and vegetation unchanged, unburnt ground can burn again later"})
                }
                Ok(f @ Footprint::Disc(c, r)) => {
                    json!({"centre_m":c,"radius_m":r,"nominal_area_ha":f.area_ha(),"effect":"puts out the fire inside; fuel and vegetation unchanged, unburnt ground can burn again later"})
                }
                Err(_) => serde_json::Value::Null,
            },
            Command::ExtinguishArea { polygon } => match self.resolve_polygon(polygon) {
                Ok(poly) => {
                    json!({"polygon_m":poly,"nominal_area_ha":polygon_area_m2(&poly)/10000.0,"effect":"puts out the fire inside; fuel and vegetation unchanged, unburnt ground can burn again later"})
                }
                Err(_) => serde_json::Value::Null,
            },
            Command::CutFireLine {
                from,
                to,
                along,
                width_m,
            } => match self.line_endpoints(from, to, along) {
                Ok((a, b)) => {
                    let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                    json!({"from_m":a,"to_m":b,"length_m":length,"width_m":width_m.unwrap_or(if self.scn.operations.progressive_suppression {6.0}else{40.0}),"nominal_area_ha":length*width_m.unwrap_or(if self.scn.operations.progressive_suppression {6.0}else{40.0})/10000.0})
                }
                Err(_) => serde_json::Value::Null,
            },
            Command::WaterDrop { at } => {
                json!({"centre_m":self.resolve_loc(at).ok(),"radius_m":60,"effect":"temporary wetting of unburnt fuel; does not extinguish burning cells"})
            }
            Command::SetWeather { .. } => {
                json!({"wind_direction":"degrees FROM","wind_speed":"km/h","fuel_moisture":"percent","spread":"future simulated time, stochastic ensemble; no instant perimeter change"})
            }
            _ => serde_json::Value::Null,
        }
    }

    /// The ground an `Extinguish` covers: a zone code (`"F4"`) is the whole
    /// zone; a sector name its bounds; a gazetteer place its own `radius_m`
    /// (else [`PLACE_EXTINGUISH_RADIUS_M`]); any other named point (facility,
    /// road) a [`PLACE_EXTINGUISH_RADIUS_M`] disc; a circle itself.
    pub fn extinguish_footprint(&self, area: &Area) -> Result<Footprint, String> {
        match area {
            Area::Named(name) => {
                if let Some(code) = self.zone_code(name) {
                    if let Some(b) = self.scn.zone_bounds(&code) {
                        return Ok(Footprint::Rect(b));
                    }
                }
                let key = normalise(name);
                if let Some(p) = self.scn.places.iter().find(|p| {
                    normalise(&p.name) == key || p.aliases.iter().any(|a| normalise(a) == key)
                }) {
                    let r = p.radius_m.filter(|r| r.is_finite() && *r > 0.0);
                    return Ok(Footprint::Disc(p.pos, r.unwrap_or(PLACE_EXTINGUISH_RADIUS_M)));
                }
                if let Some(s) = self
                    .scn
                    .sectors
                    .iter()
                    .find(|s| normalise(&s.id) == key || normalise(&s.name) == key)
                {
                    return Ok(Footprint::Rect(s.bounds));
                }
                self.scn
                    .resolve_place(name)
                    .map(|p| Footprint::Disc(p, PLACE_EXTINGUISH_RADIUS_M))
                    .ok_or_else(|| format!("unknown area \"{name}\"{}", self.did_you_mean(name)))
            }
            Area::Circle { at, radius_m } => {
                let p = self.resolve_loc(at)?;
                if !radius_m.is_finite() || *radius_m <= 0.0 {
                    return Err("radius must be positive".into());
                }
                Ok(Footprint::Disc(p, *radius_m))
            }
        }
    }

    /// The vertices of an `ExtinguishArea` polygon in world metres: at least
    /// three, on the map, enclosing a non-zero area.
    pub fn resolve_polygon(&self, polygon: &[Loc]) -> Result<Vec<Pos>, String> {
        if polygon.len() < 3 {
            return Err("an area needs at least 3 vertices".into());
        }
        if polygon.len() > MAX_POLYGON_VERTICES {
            return Err(format!("an area has at most {MAX_POLYGON_VERTICES} vertices"));
        }
        let poly = polygon
            .iter()
            .map(|l| self.resolve_loc(l))
            .collect::<Result<Vec<_>, _>>()?;
        if polygon_area_m2(&poly) < 1.0 {
            return Err("the area's vertices enclose no surface".into());
        }
        Ok(poly)
    }

    pub fn resolve_loc(&self, loc: &Loc) -> Result<Pos, String> {
        match loc {
            Loc::Point { x, y } => {
                let (w, h) = (self.scn.grid.width_m(), self.scn.grid.height_m());
                if !x.is_finite() || !y.is_finite() || *x < 0.0 || *y < 0.0 || *x >= w || *y >= h {
                    return Err(format!("point ({x:.0}, {y:.0}) is outside the map"));
                }
                Ok([*x, *y])
            }
            Loc::Named(name) => self
                .scn
                .resolve_place(name)
                .ok_or_else(|| format!("unknown place \"{name}\"{}", self.did_you_mean(name))),
        }
    }

    /// A building by facility name, `#id`, or bare id.
    pub fn find_building(&self, key: &str) -> Option<usize> {
        let k = key.trim().trim_start_matches('#');
        if let Ok(id) = k.parse::<u32>() {
            return self.scn.buildings.iter().position(|b| b.id == id);
        }
        let n = normalise(key);
        self.scn
            .buildings
            .iter()
            .position(|b| b.name.as_deref().map(normalise).as_deref() == Some(n.as_str()))
    }

    pub fn find_road(&self, key: &str) -> Option<String> {
        let n = normalise(key);
        self.scn
            .roads
            .iter()
            .find(|r| normalise(&r.id) == n || normalise(&r.name) == n)
            .map(|r| r.id.clone())
    }

    pub fn zone_code(&self, key: &str) -> Option<String> {
        self.scn
            .zones
            .parse(key)
            .map(|(c, r)| self.scn.zones.name(c, r))
    }

    /// The zone code of a named place or sector (never a building or road).
    fn place_zone(&self, name: &str) -> Option<String> {
        let key = scenario::normalise(name);
        if key.is_empty() {
            return None;
        }
        let pos = self
            .scn
            .places
            .iter()
            .find(|p| {
                scenario::normalise(&p.name) == key
                    || p.aliases.iter().any(|a| scenario::normalise(a) == key)
            })
            .map(|p| p.pos)
            .or_else(|| {
                self.scn
                    .sectors
                    .iter()
                    .find(|s| scenario::normalise(&s.id) == key || scenario::normalise(&s.name) == key)
                    .map(|s| s.centre())
            })?;
        self.scn.zone_of(pos)
    }

    /// Up to three known names (buildings, places, sectors) close to `key`:
    /// one contains the other, or they share a word of three letters or more.
    pub fn place_suggestions(&self, key: &str) -> Vec<String> {
        let k = scenario::normalise(key);
        let words: Vec<String> = key
            .split(|c: char| !c.is_alphanumeric())
            .map(scenario::normalise)
            .filter(|w| w.len() >= 3)
            .collect();
        if k.is_empty() {
            return Vec::new();
        }
        let names = self
            .scn
            .buildings
            .iter()
            .filter_map(|b| b.name.clone())
            .chain(self.scn.places.iter().map(|p| p.name.clone()))
            .chain(self.scn.sectors.iter().map(|s| s.name.clone()));
        let mut scored: Vec<(usize, String)> = Vec::new();
        for n in names {
            let nn = scenario::normalise(&n);
            let mut score = 0;
            if nn.contains(&k) || (nn.len() >= 3 && k.contains(&nn)) {
                score += 10;
            }
            score += words.iter().filter(|w| nn.contains(w.as_str())).count();
            if score > 0 && !scored.iter().any(|(_, x)| *x == n) {
                scored.push((score, n));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(3).map(|(_, n)| n).collect()
    }

    /// `" (did you mean «A», «B»?)"` for a name that matched nothing, or empty.
    fn did_you_mean(&self, key: &str) -> String {
        let s = self.place_suggestions(key);
        if s.is_empty() {
            String::new()
        } else {
            format!(
                " (did you mean {}?)",
                s.iter().map(|n| format!("«{n}»")).collect::<Vec<_>>().join(", ")
            )
        }
    }

    fn unknown_building(&self, key: &str) -> String {
        if let Some(code) = self.zone_code(key) {
            return format!("\"{key}\" is a zone ({code}), not a building: use Evacuate zone, or name a building{}", self.did_you_mean(key));
        }
        format!("unknown building \"{key}\"{}", self.did_you_mean(key))
    }

    fn example_zone(&self) -> String {
        self.scn.zones.names().first().cloned().unwrap_or_else(|| "F4".into())
    }

    /// Error for a zone argument that is not a zone code, saying what it is
    /// instead (a building, a named place) and what to use.
    fn unknown_zone(&self, key: &str, building_action: Option<&str>) -> String {
        if let Some(i) = self.find_building(key) {
            let name = self.scn.buildings[i]
                .name
                .clone()
                .unwrap_or_else(|| format!("#{}", self.scn.buildings[i].id));
            let zone = self.scn.zone_of(self.scn.buildings[i].pos);
            return match (building_action, zone) {
                (Some(a), _) => format!(
                    "\"{key}\" is a building, not a zone: use {a} for «{name}», or a zone code like {}",
                    self.example_zone()
                ),
                (None, Some(z)) => format!("\"{key}\" is a building, not a zone: «{name}» lies in zone {z}"),
                (None, None) => format!("\"{key}\" is a building, not a zone code like {}", self.example_zone()),
            };
        }
        format!(
            "unknown zone \"{key}\": a zone is a grid code like {}{}",
            self.example_zone(),
            self.did_you_mean(key)
        )
    }

    /// Adapt a command whose place is of another kind than its type expects,
    /// when the intent is unambiguous: an evacuate-zone or warning aimed at a
    /// building or named place, or an evacuate-building aimed at a zone code.
    /// Returns the command to use and a sentence telling what was changed.
    pub fn reconcile_command(&self, cmd: &Command) -> (Command, Option<String>) {
        match cmd {
            Command::EvacuateZone { zone, destination } if self.zone_code(zone).is_none() => {
                if let Some(d) = self.scn.district(zone) {
                    return (
                        Command::EvacuateDistrict {
                            district: d.name.clone(),
                            destination: destination.clone(),
                        },
                        Some(format!("«{zone}» is a borgo: treated as Evacuate borgo «{}»", d.name)),
                    );
                }
                if let Some(i) = self.find_building(zone) {
                    let name = self.scn.buildings[i]
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", self.scn.buildings[i].id));
                    return (
                        Command::EvacuateBuilding {
                            building: name.clone(),
                            destination: destination.clone(),
                        },
                        Some(format!("«{zone}» is a building, not a zone: treated as Evacuate building «{name}»")),
                    );
                }
                if let Some(code) = self.place_zone(zone) {
                    return (
                        Command::EvacuateZone { zone: code.clone(), destination: destination.clone() },
                        Some(format!("«{zone}» is a named place: treated as zone {code}, where it lies")),
                    );
                }
            }
            Command::IssueWarning { zone } if self.zone_code(zone).is_none() => {
                let code = self
                    .find_building(zone)
                    .map(|i| self.scn.buildings[i].pos)
                    .and_then(|p| self.scn.zone_of(p))
                    .or_else(|| self.place_zone(zone));
                if let Some(code) = code {
                    return (
                        Command::IssueWarning { zone: code.clone() },
                        Some(format!("«{zone}» is not a zone code: warning zone {code}, where it lies")),
                    );
                }
            }
            Command::EvacuateBuilding { building, destination }
                if self.find_building(building).is_none() && self.scn.district(building).is_some() =>
            {
                let name = self.scn.district(building).map(|d| d.name.clone()).unwrap_or_default();
                return (
                    Command::EvacuateDistrict { district: name.clone(), destination: destination.clone() },
                    Some(format!("«{building}» is a borgo, not a building: treated as Evacuate borgo «{name}»")),
                );
            }
            Command::EvacuateBuilding { building, destination }
                if self.find_building(building).is_none() && self.zone_code(building).is_some() =>
            {
                let code = self.zone_code(building).unwrap_or_default();
                return (
                    Command::EvacuateZone { zone: code.clone(), destination: destination.clone() },
                    Some(format!("«{building}» is a zone, not a building: treated as Evacuate zone {code}")),
                );
            }
            _ => {}
        }
        (cmd.clone(), None)
    }

    fn buildings_in_zone(&self, zone: &str) -> Vec<usize> {
        let b = self.scn.zone_bounds(zone).unwrap_or([0.0; 4]);
        self.scn
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, x)| {
                x.pos[0] >= b[0] && x.pos[0] < b[2] && x.pos[1] >= b[1] && x.pos[1] < b[3]
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn line_endpoints(
        &self,
        from: &Option<Loc>,
        to: &Option<Loc>,
        along: &Option<String>,
    ) -> Result<(Pos, Pos), String> {
        if let Some(code) = along {
            return self
                .scn
                .zone_edge(code)
                .ok_or_else(|| format!("\"{code}\" is not a zone edge (use e.g. F3:S)"));
        }
        match (from, to) {
            (Some(f), Some(t)) => Ok((self.resolve_loc(f)?, self.resolve_loc(t)?)),
            _ => Err("a fire line needs \"from\" and \"to\", or \"along\" a zone edge".into()),
        }
    }

    fn offer(&self, resource: &str) -> Option<&scenario::ReinforcementOffer> {
        self.scn
            .reinforcements
            .iter()
            .find(|o| o.resource == resource)
    }

    fn requested_so_far(&self, resource: &str) -> u32 {
        self.d
            .arrivals
            .iter()
            .filter(|a| a.resource == resource)
            .map(|a| a.qty)
            .sum()
    }

    // --------------------------------------------------------------- validation

    /// Validate a batch as if applied in order by `actor`, after `prior`
    /// (already-approved, not yet applied) commands have consumed resources.
    pub fn validate_batch(
        &self,
        prior: &[(Actor, Command)],
        cmds: &[Command],
        actor: &Actor,
    ) -> Vec<Verdict> {
        let mut pools = self.d.pools.clone();
        let mut requested: BTreeMap<String, u32> = BTreeMap::new();
        let mut returned: BTreeMap<u64, u32> = BTreeMap::new();
        let mut evacuation_reserved = BTreeSet::new();
        let mut shelter_reserved: BTreeMap<u32, u32> = BTreeMap::new();
        let mut check = |cmd: &Command, actor: &Actor| {
            let targets = self.evacuation_targets(cmd);
            if matches!(actor, Actor::Role(_))
                && targets.iter().any(|i| evacuation_reserved.contains(i))
            {
                return Verdict::Rejected {
                    reason: "evacuation targets reserved by another approved action".into(),
                };
            }
            let destination = match cmd {
                Command::EvacuateZone { destination, .. }
                | Command::EvacuateBuilding { destination, .. }
                | Command::EvacuateDistrict { destination, .. } => {
                    destination.as_deref().and_then(|d| self.find_building(d))
                }
                _ => None,
            };
            let demand: u32 = targets
                .iter()
                .map(|i| {
                    self.d.buildings[*i]
                        .people_inside
                        .saturating_sub(self.d.buildings[*i].evac_pending)
                })
                .sum();
            if self.scn.operations.traffic {
                if let Some(dest) = destination {
                    let b = &self.scn.buildings[dest];
                    let reserved: u32 = self
                        .d
                        .traffic
                        .vehicles
                        .iter()
                        .filter(|v| !v.completed && v.destination == Some(b.id))
                        .map(|v| v.people)
                        .sum();
                    let used = self.d.shelters.get(&b.id).copied().unwrap_or(0);
                    if demand
                        > b.shelter_capacity
                            .saturating_sub(used)
                            .saturating_sub(reserved)
                            .saturating_sub(shelter_reserved.get(&b.id).copied().unwrap_or(0))
                    {
                        return Verdict::Rejected {
                            reason:
                                "destination capacity reserved by existing or approved evacuations"
                                    .into(),
                        };
                    }
                }
            }
            match cmd {
                Command::RequestReinforcement { resource, qty } => {
                    if let Some(offer) = self.offer(resource) {
                        let prior = requested.get(resource).copied().unwrap_or(0);
                        let left = offer
                            .max_qty
                            .saturating_sub(self.requested_so_far(resource))
                            .saturating_sub(prior);
                        if *qty > left {
                            return Verdict::Rejected {
                                reason: format!(
                                    "only {left} more {resource} available after approved requests"
                                ),
                            };
                        }
                    }
                }
                Command::ReturnResource { allocation, qty } => {
                    if let Some(a) = self.d.allocations.iter().find(|a| a.id == *allocation) {
                        let prior = returned.get(allocation).copied().unwrap_or(0);
                        if *qty > a.qty.saturating_sub(a.returned).saturating_sub(prior) {
                            return Verdict::Rejected {
                                reason: "return quantity exceeds remaining allocation".into(),
                            };
                        }
                    }
                }
                _ => {}
            }
            let verdict = self.check(cmd, actor, &mut pools);
            if verdict.is_ok() {
                if matches!(actor, Actor::Role(_)) {
                    evacuation_reserved.extend(targets);
                }
                if let Some(dest) = destination {
                    if self.scn.operations.traffic {
                        *shelter_reserved
                            .entry(self.scn.buildings[dest].id)
                            .or_default() += demand;
                    }
                }
                match cmd {
                    Command::RequestReinforcement { resource, qty } => {
                        *requested.entry(resource.clone()).or_default() += *qty
                    }
                    Command::ReturnResource { allocation, qty } => {
                        *returned.entry(*allocation).or_default() += *qty
                    }
                    _ => {}
                }
            }
            verdict
        };
        for (actor, cmd) in prior {
            let _ = check(cmd, actor);
        }
        cmds.iter().map(|cmd| check(cmd, actor)).collect()
    }

    pub fn validate(&self, cmd: &Command, actor: &Actor) -> Verdict {
        self.validate_batch(&[], std::slice::from_ref(cmd), actor)
            .remove(0)
    }

    /// Check one command, consuming from `pools` if it passes.
    fn check(
        &self,
        cmd: &Command,
        actor: &Actor,
        pools: &mut BTreeMap<String, BTreeMap<String, Pool>>,
    ) -> Verdict {
        match self.check_inner(cmd, actor, pools) {
            Ok(notes) => Verdict::Ok { notes },
            Err(reason) => Verdict::Rejected { reason },
        }
    }

    fn check_inner(
        &self,
        cmd: &Command,
        actor: &Actor,
        pools: &mut BTreeMap<String, BTreeMap<String, Pool>>,
    ) -> Result<Vec<String>, String> {
        // EXCON tasking a drone acts through the owner's drone.
        let owner;
        let actor = match (actor, cmd) {
            (Actor::Master, Command::MonitorArea { role, .. }) => {
                owner = Actor::Role(self.drone_owner(role.as_deref())?);
                &owner
            }
            _ => actor,
        };
        let role = match actor {
            Actor::Master => None,
            Actor::Role(id) => {
                let r = self
                    .scn
                    .role(id)
                    .ok_or_else(|| format!("unknown role {id}"))?;
                if cmd.is_master_only() || !r.allowed_actions.iter().any(|a| a == cmd.kind()) {
                    return Err(format!(
                        "{} is not authorised to {}",
                        r.name,
                        cmd.describe().to_lowercase()
                    ));
                }
                Some(r)
            }
        };
        // Consume `n` units of `res` from the acting role (the EXCON is unconstrained).
        let mut take = |res: &str, n: u32, what: &str| -> Result<(), String> {
            let Some(r) = role else { return Ok(()) };
            let pool = pools.get_mut(&r.id).and_then(|p| p.get_mut(res));
            let avail = pool.as_ref().map(|p| p.available()).unwrap_or(0);
            if avail < n {
                return Err(format!(
                    "{what} needs {n} {res}, only {avail} currently available"
                ));
            }
            if let Some(p) = pool {
                p.used += n;
            }
            Ok(())
        };
        if matches!(actor, Actor::Role(_)) {
            let targets = self.evacuation_targets(cmd);
            if !targets.is_empty()
                && self.d.tasks.iter().any(|task| {
                    matches!(task.status, TaskStatus::EnRoute | TaskStatus::Working)
                        && self
                            .evacuation_targets(&task.command)
                            .iter()
                            .any(|i| targets.contains(i))
                })
            {
                return Err("evacuation targets already assigned to a deployed task".into());
            }
        }
        let mut notes = Vec::new();
        if let Actor::Role(id) = actor {
            if self.scn.operations.road_routing
                && !matches!(cmd, Command::WaterDrop { .. } | Command::MonitorArea { .. })
            {
                if let Some(target) = self.travel_target(cmd) {
                    let resource = match cmd {
                        Command::CutFireLine { .. } | Command::Reconnoitre { .. } => {
                            "ground_crews"
                        }
                        Command::DeployEngine { .. } => "engines",
                        Command::CloseRoad { .. } => "police_patrols",
                        _ => "buses",
                    };
                    if let Some(setting) =
                        self.d.travel_settings.get(id).and_then(|r| r.get(resource))
                    {
                        let route = self
                            .network
                            .route(
                                setting.origin,
                                target,
                                &self.d.roads,
                                false,
                                resource == "ground_crews",
                            )
                            .ok_or("no reachable route from staging to task")?;
                        if resource == "engines"
                            && route
                                .legs
                                .last()
                                .is_some_and(|l| l.road.is_none() && l.length_m > self.engine_hose_reach_m())
                        {
                            return Err(
                                format!("engine target exceeds {:.0} m hose reach from a drivable road",self.engine_hose_reach_m()),
                            );
                        }
                    }
                }
            }
        }
        match cmd {
            Command::CutFireLine {
                from,
                to,
                along,
                width_m,
            } => {
                let (a, b) = self.line_endpoints(from, to, along)?;
                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                if len < 20.0 {
                    return Err("fire line is too short".into());
                }
                if let Some(w) = width_m {
                    let min = if self.scn.operations.progressive_suppression {
                        2.0
                    } else {
                        10.0
                    };
                    if !(min..=200.0).contains(w) {
                        return Err("fire line width must be 10–200 m".into());
                    }
                }
                let crews = (len / LINE_M_PER_CREW).ceil() as u32;
                take("ground_crews", crews, &format!("a {len:.0} m fire line"))?;
                notes.push(format!("{len:.0} m line, {crews} crew(s)"));
            }
            Command::DeployEngine { at } => {
                if matches!(actor, Actor::Master) {
                    return Err(
                        "DeployEngine needs a role deployment and physical allocation".into(),
                    );
                }
                if !self.scn.operations.progressive_suppression {
                    return Err("DeployEngine requires the operational model".into());
                }
                self.resolve_loc(at)?;
                take("engines", 1, "a road engine")?;
            }
            Command::WaterDrop { at } => {
                self.resolve_loc(at)?;
                take("air_tankers", 1, "a water drop")?;
            }
            Command::MonitorArea { zones, .. } => {
                let (resource, spec) = role
                    .and_then(|r| self.drone_of(r))
                    .ok_or("no monitoring drone is available to this role")?;
                let covered = self.checked_monitor_zones(zones, &spec)?;
                self.check_drone_wind(&spec)?;
                take(&resource, 1, "a monitoring drone")?;
                notes.push(format!("drone over {}", covered.join(", ")));
            }
            Command::Reconnoitre { zone, .. } => {
                let code = self.recon_zone(zone)?;
                take("ground_crews", 1, "a ground reconnaissance")?;
                notes.push(format!("1 crew reconnoitres {code}"));
            }
            Command::EvacuateZone { zone, destination } => {
                let code = self
                    .zone_code(zone)
                    .ok_or_else(|| self.unknown_zone(zone, Some("Evacuate building")))?;
                let idx: Vec<usize> = self
                    .buildings_in_zone(&code)
                    .into_iter()
                    .filter(|&i| self.d.buildings[i].people_inside > 0)
                    .collect();
                if idx.is_empty() {
                    return Err(format!("no occupied buildings in zone {code}"));
                }
                self.check_destination(destination, &mut notes)?;
                self.check_evacuation_capacity(&idx, destination)?;
                self.check_transport(&idx, role.is_some(), &mut take, &mut notes)?;
                let people: u32 = idx.iter().map(|&i| self.d.buildings[i].people_inside).sum();
                notes.push(format!("{} buildings, {people} people", idx.len()));
            }
            Command::EvacuateBuilding {
                building,
                destination,
            } => {
                let i = self
                    .find_building(building)
                    .ok_or_else(|| self.unknown_building(building))?;
                if self.d.buildings[i].people_inside == 0 {
                    return Err(format!("{building} is already empty"));
                }
                self.check_destination(destination, &mut notes)?;
                self.check_evacuation_capacity(&[i], destination)?;
                self.check_transport(&[i], role.is_some(), &mut take, &mut notes)?;
            }
            Command::EvacuateDistrict {
                district,
                destination,
            } => {
                let idx = self.district_targets(district)?;
                if idx.is_empty() {
                    return Err(format!("borgo {district} has nobody left to evacuate"));
                }
                self.check_destination(destination, &mut notes)?;
                self.check_evacuation_capacity(&idx, destination)?;
                self.check_transport(&idx, role.is_some(), &mut take, &mut notes)?;
                let people: u32 = idx.iter().map(|&i| self.d.buildings[i].people_inside).sum();
                notes.push(format!("{} homes, {people} people", idx.len()));
            }
            Command::IssueWarning { zone } => {
                self.zone_code(zone)
                    .ok_or_else(|| self.unknown_zone(zone, None))?;
            }
            Command::CloseRoad { road } => {
                let id = self
                    .find_road(road)
                    .ok_or_else(|| format!("unknown road \"{road}\""))?;
                if self.d.roads[&id].closed {
                    return Err(format!("{road} is already closed"));
                }
                take("police_patrols", 1, "closing a road")?;
            }
            Command::ReopenRoad { road } => {
                self.find_road(road)
                    .ok_or_else(|| format!("unknown road \"{road}\""))?;
            }
            Command::OpenShelter { building } => {
                let i = self
                    .find_building(building)
                    .ok_or_else(|| self.unknown_building(building))?;
                let b = &self.scn.buildings[i];
                if b.shelter_capacity == 0 {
                    return Err(format!("{building} cannot be used as a shelter"));
                }
                if self.d.buildings[i].damage != Damage::Intact {
                    return Err(format!("{building} is damaged"));
                }
            }
            Command::RequestReinforcement { resource, qty } => {
                let o = self
                    .offer(resource)
                    .ok_or_else(|| format!("no reinforcement offer for \"{resource}\""))?;
                let left = o.max_qty.saturating_sub(self.requested_so_far(resource));
                if *qty == 0 || *qty > left {
                    return Err(format!(
                        "{} can provide at most {left} more {resource}",
                        o.source
                    ));
                }
                notes.push(format!(
                    "{} arrive in {} min from {}",
                    qty, o.delay_min, o.source
                ));
            }
            Command::Ignite { at, radius_m } => {
                self.resolve_loc(at)?;
                if radius_m.is_some_and(|r| {
                    !r.is_finite()
                        || r <= 0.0
                        || r > self.scn.grid.width_m().max(self.scn.grid.height_m())
                }) {
                    return Err(
                        "ignition radius must be finite, positive and no larger than map extent"
                            .into(),
                    );
                }
            }
            Command::Extinguish { area } => {
                self.extinguish_footprint(area)?;
            }
            Command::ExtinguishArea { polygon } => {
                self.resolve_polygon(polygon)?;
            }
            Command::SetWeather {
                wind_speed_kmh,
                moisture_pct,
                ..
            } => {
                if wind_speed_kmh.is_some_and(|v| !(0.0..=150.0).contains(&v))
                    || moisture_pct.is_some_and(|v| !(0.0..=100.0).contains(&v))
                {
                    return Err("weather value out of range".into());
                }
            }
            Command::SetBuildingStatus { building, .. } => {
                if self.find_building(building).is_none() && self.zone_code(building).is_none() {
                    return Err(format!("unknown building or zone \"{building}\""));
                }
            }
            Command::SetDeploymentTravel {
                role,
                resource,
                at,
                speed_mps,
            } => {
                self.resolve_loc(at)?;
                if !self
                    .d
                    .travel_settings
                    .get(role)
                    .is_some_and(|r| r.contains_key(resource))
                    || !speed_mps.is_finite()
                    || !(0.1..=300.0).contains(speed_mps)
                {
                    return Err("unknown role/resource or travel speed outside 0.1–300 m/s".into());
                }
            }
            Command::ReturnResource { allocation, qty } => {
                let a = self
                    .d
                    .allocations
                    .iter()
                    .find(|a| a.id == *allocation)
                    .ok_or_else(|| format!("unknown allocation {allocation}"))?;
                if let Some(task) = self
                    .d
                    .tasks
                    .iter()
                    .find(|task| task.allocations.contains(allocation))
                {
                    if task.status == TaskStatus::Working
                        && (!self.scn.operations.progressive_suppression
                            || !matches!(
                                task.command,
                                Command::CutFireLine { .. } | Command::DeployEngine { .. }
                            ))
                        && !matches!(task.command, Command::MonitorArea { .. })
                    {
                        return Err("units are performing the task; wait for completion".into());
                    }
                    if task.status == TaskStatus::EnRoute
                        && !self.scn.operations.progressive_suppression
                        && *qty != a.qty.saturating_sub(a.returned)
                    {
                        return Err("return all en-route units to cancel dispatch; partial cancellation is unsupported".into());
                    }
                }
                if *qty == 0 || *qty > a.qty.saturating_sub(a.returned) {
                    return Err("return quantity exceeds active allocation".into());
                }
                let p = pools
                    .get_mut(&a.role)
                    .and_then(|p| p.get_mut(&a.resource))
                    .ok_or("allocation pool missing")?;
                if p.used < *qty {
                    return Err("return exceeds deployed units".into());
                }
                p.used -= *qty;
            }
            Command::AdjustResource {
                role,
                resource,
                delta,
            } => {
                if self.scn.role(role).is_none() {
                    return Err(format!("unknown role {role}"));
                }
                if !self.scn.resource_types.iter().any(|r| &r.id == resource) {
                    return Err(format!("unknown resource {resource}"));
                }
                let p = pools
                    .get(role)
                    .and_then(|p| p.get(resource))
                    .copied()
                    .unwrap_or(Pool { total: 0, used: 0 });
                let total = p.total as i64 + *delta as i64;
                if total < p.used as i64 || total > u32::MAX as i64 {
                    return Err("availability cannot fall below deployed units or overflow".into());
                }
            }
            Command::AdvanceTime { minutes } => {
                if *minutes == 0 || *minutes > 60 * 24 * 14 {
                    return Err("minutes must be 1..20160".into());
                }
            }
        }
        Ok(notes)
    }

    /// A destination must be able to host evacuees. It need not be open yet:
    /// evacuees are counted in when they arrive, if it has been opened by then.
    fn check_destination(
        &self,
        destination: &Option<String>,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        if let Some(d) = destination {
            let i = self
                .find_building(d)
                .ok_or_else(|| format!("unknown destination \"{d}\""))?;
            if self.scn.buildings[i].shelter_capacity == 0 {
                return Err(format!("{d} cannot host evacuees"));
            }
            notes.push(format!("to {d}"));
        }
        Ok(())
    }

    fn check_evacuation_capacity(
        &self,
        idx: &[usize],
        destination: &Option<String>,
    ) -> Result<(), String> {
        if !self.scn.operations.traffic {
            return Ok(());
        }
        let Some(name) = destination else {
            return Ok(());
        };
        let dest = self.find_building(name).ok_or("unknown destination")?;
        let id = self.scn.buildings[dest].id;
        let need: u32 = idx
            .iter()
            .map(|i| {
                self.d.buildings[*i]
                    .people_inside
                    .saturating_sub(self.d.buildings[*i].evac_pending)
            })
            .sum();
        let reserved: u32 = self
            .d
            .traffic
            .vehicles
            .iter()
            .filter(|v| !v.completed && v.destination == Some(id))
            .map(|v| v.people)
            .sum();
        let used = self.d.shelters.get(&id).copied().unwrap_or(0);
        if need
            > self.scn.buildings[dest]
                .shelter_capacity
                .saturating_sub(used)
                .saturating_sub(reserved)
        {
            return Err("destination has insufficient unreserved shelter capacity".into());
        }
        Ok(())
    }
    fn check_transport(
        &self,
        idx: &[usize],
        constrained: bool,
        take: &mut impl FnMut(&str, u32, &str) -> Result<(), String>,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        let seats: u32 = idx
            .iter()
            .filter(|&&i| self.scn.buildings[i].kind.needs_transport())
            .map(|&i| self.d.buildings[i].people_inside)
            .sum();
        if seats == 0 || !constrained {
            return Ok(());
        }
        let buses = seats.div_ceil(BUS_SEATS);
        take(
            "buses",
            buses,
            &format!("moving {seats} people without cars"),
        )?;
        notes.push(format!("{buses} bus(es) for {seats} people"));
        Ok(())
    }

    // ------------------------------------------------------------------ apply

    /// Execute a command. Validates first; resources are consumed from the
    /// acting role. Returns human-readable notes.
    pub fn apply(&mut self, cmd: &Command, actor: &Actor) -> Result<Vec<String>, String> {
        let owner;
        let actor = match (actor, cmd) {
            (Actor::Master, Command::MonitorArea { role, .. }) => {
                owner = Actor::Role(self.drone_owner(role.as_deref())?);
                &owner
            }
            _ => actor,
        };
        let before_pools = self.d.pools.clone();
        let mut pools = before_pools.clone();
        let notes = self.check_inner(cmd, actor, &mut pools)?;
        self.d.pools = pools;
        let t = self.time_s();
        let deployed: Vec<(String, String, u32)> = self
            .d
            .pools
            .iter()
            .flat_map(|(role, pools)| {
                pools.iter().filter_map(|(resource, p)| {
                    let previous = before_pools
                        .get(role)
                        .and_then(|p| p.get(resource))
                        .map(|p| p.used)
                        .unwrap_or(0);
                    (p.used > previous).then(|| (role.clone(), resource.clone(), p.used - previous))
                })
            })
            .collect();
        let mut allocation_ids = Vec::new();
        for (role, resource, qty) in deployed {
            let id = self.next_allocation_id;
            self.next_allocation_id += 1;
            allocation_ids.push(id);
            self.d.allocations.push(ResourceAllocation {
                id,
                role: role.clone(),
                resource: resource.clone(),
                qty,
                returned: 0,
                command: cmd.clone(),
                deployed_at_s: t,
                last_return_at_s: None,
            });
            self.resource_event("deployed", Some(id), &role, &resource, qty as i64);
        }
        if let Actor::Role(role) = actor {
            if !allocation_ids.is_empty() {
                if let Some(detail) = self.dispatch(cmd, role, allocation_ids) {
                    let mut out = notes;
                    out.push(detail);
                    return Ok(out);
                }
            }
        }
        let mut out = notes;
        out.extend(self.apply_effect(cmd, actor)?);
        Ok(out)
    }

    fn apply_effect(&mut self, cmd: &Command, actor: &Actor) -> Result<Vec<String>, String> {
        let by = actor.label().to_string();
        let t = self.time_s();
        let mut out = Vec::new();
        match cmd {
            Command::DeployEngine { .. } => {
                out.push("Engine deployment requires allocated role resources".into());
            }
            Command::MonitorArea { .. } => {
                out.push("Drone monitoring requires an allocated drone".into());
            }
            Command::Reconnoitre { zone, role } => {
                // A crew on arrival reports for its role; EXCON names the role it informs.
                let owner = match (actor, role) {
                    (Actor::Role(id), _) => id.clone(),
                    (Actor::Master, Some(id)) if self.scn.role(id).is_some() => id.clone(),
                    (Actor::Master, Some(id)) => return Err(format!("unknown role {id}")),
                    (Actor::Master, None) => {
                        return Err("name the role that receives the reconnaissance".into())
                    }
                };
                let code = self.recon_zone(zone)?;
                self.record_reconnaissance(&owner, &code);
                out.push(format!("reconnaissance of {code} reported to {owner}"));
                self.event(
                    "reconnaissance",
                    format!("{owner}: ground reconnaissance reports on {code}"),
                    Some(code),
                );
            }
            Command::CutFireLine {
                from,
                to,
                along,
                width_m,
            } => {
                let (a, b) = self.line_endpoints(from, to, along)?;
                let w = width_m.unwrap_or(if self.scn.operations.progressive_suppression {
                    6.0
                } else {
                    DEFAULT_LINE_WIDTH_M
                });
                let n = self.fire.fire_line(a, b, w);
                self.d.lines.push(LineRec {
                    from: a,
                    to: b,
                    width_m: w,
                    by: by.clone(),
                    t_s: t,
                });
                out.push(format!("{n} cells of fuel cleared"));
                self.event(
                    "fire_line",
                    format!("{by} cut a fire line: {}", cmd.describe()),
                    self.scn.zone_of(a),
                );
            }
            Command::WaterDrop { at } => {
                let p = self.resolve_loc(at)?;
                let n = self.fire.water_drop(p, DROP_RADIUS_M, DROP_LITRES_PER_M2);
                self.d.drops.push(DropRec {
                    at: p,
                    radius_m: DROP_RADIUS_M,
                    by: by.clone(),
                    t_s: t,
                });
                out.push(format!("{n} cells wetted"));
                self.event(
                    "water_drop",
                    format!("{by}: {}", cmd.describe()),
                    self.scn.zone_of(p),
                );
            }
            Command::EvacuateZone { zone, destination } => {
                let code = self.zone_code(zone).unwrap();
                let idx = self.buildings_in_zone(&code);
                let n = self.start_evacuation(
                    &idx,
                    destination.as_deref(),
                    matches!(actor, Actor::Role(_)),
                );
                out.push(format!("{n} people leaving"));
                self.event(
                    "evacuation_ordered",
                    format!("{by} ordered evacuation of zone {code} ({n} people)"),
                    Some(code),
                );
            }
            Command::EvacuateBuilding {
                building,
                destination,
            } => {
                let i = self.find_building(building).unwrap();
                let n = self.start_evacuation(
                    &[i],
                    destination.as_deref(),
                    matches!(actor, Actor::Role(_)),
                );
                out.push(format!("{n} people leaving"));
                let z = self.scn.zone_of(self.scn.buildings[i].pos);
                self.event(
                    "evacuation_ordered",
                    format!("{by} ordered evacuation of {building} ({n} people)"),
                    z,
                );
            }
            Command::EvacuateDistrict {
                district,
                destination,
            } => {
                let d = self.scn.district(district).unwrap().clone();
                let n = self.start_district_evacuation(
                    &d,
                    destination.as_deref(),
                    matches!(actor, Actor::Role(_)),
                );
                out.push(format!("{n} people leaving"));
                let z = self
                    .scn
                    .places
                    .iter()
                    .find(|p| p.name == d.name)
                    .and_then(|p| self.scn.zone_of(p.pos));
                self.event(
                    "evacuation_ordered",
                    format!("{by} ordered evacuation of borgo {} ({n} people)", d.name),
                    z,
                );
            }
            Command::IssueWarning { zone } => {
                let code = self.zone_code(zone).unwrap();
                self.d.warned.insert(code.clone());
                self.event(
                    "warning",
                    format!("{by} issued a warning to zone {code}"),
                    Some(code),
                );
            }
            Command::CloseRoad { road } => {
                let id = self.find_road(road).unwrap();
                self.d.roads.get_mut(&id).unwrap().closed = true;
                self.event(
                    "road_closed",
                    format!("{by} closed {id} to civilian traffic"),
                    None,
                );
            }
            Command::ReopenRoad { road } => {
                let id = self.find_road(road).unwrap();
                self.d.roads.get_mut(&id).unwrap().closed = false;
                self.event("road_reopened", format!("{by} reopened {id}"), None);
            }
            Command::OpenShelter { building } => {
                let i = self.find_building(building).unwrap();
                let id = self.scn.buildings[i].id;
                self.d.shelters.entry(id).or_insert(0);
                self.event(
                    "shelter_opened",
                    format!("{by} opened {building} as a shelter"),
                    self.scn.zone_of(self.scn.buildings[i].pos),
                );
            }
            Command::RequestReinforcement { resource, qty } => {
                let o = self.offer(resource).unwrap().clone();
                self.d.arrivals.push(Arrival {
                    role: o.assigned_to.clone(),
                    resource: resource.clone(),
                    qty: *qty,
                    at_s: t + o.delay_min as i64 * 60,
                    source: o.source.clone(),
                });
                self.resource_event("requested", None, &o.assigned_to, resource, *qty as i64);
                self.event(
                    "reinforcement_requested",
                    format!(
                        "{by} requested {qty} {resource} from {} (ETA {} min)",
                        o.source, o.delay_min
                    ),
                    None,
                );
            }
            Command::Ignite { at, radius_m } => {
                let p = self.resolve_loc(at)?;
                let n = self
                    .fire
                    .ignite(p, radius_m.unwrap_or(80.0))
                    .map_err(|e| e.to_string())?;
                out.push(format!("{n} cells lit"));
                self.event(
                    "ignition",
                    format!(
                        "New fire at {}",
                        cmd.describe().trim_start_matches("Ignite fire at ")
                    ),
                    self.scn.zone_of(p),
                );
            }
            Command::Extinguish { area } => {
                // Same as a drawn area: every realization, fuel unchanged.
                let foot = self.extinguish_footprint(area)?;
                let n = match foot {
                    Footprint::Rect(b) => self.fire.extinguish_rect(b),
                    Footprint::Disc(p, r) => self.fire.extinguish(p, r),
                }
                .map_err(|e| e.to_string())?;
                out.push(format!("{n} burning cells put out; fuel unchanged"));
                self.event(
                    "extinguished",
                    format!(
                        "Fire put out: {} ({:.1} ha, {n} burning cells)",
                        cmd.describe(),
                        foot.area_ha()
                    ),
                    self.scn.zone_of(foot.centre()),
                );
            }
            Command::ExtinguishArea { polygon } => {
                let poly = self.resolve_polygon(polygon)?;
                let cells = self.fire.cells_in_polygon(&poly);
                let n = self
                    .fire
                    .extinguish_area(&cells)
                    .map_err(|e| e.to_string())?;
                out.push(format!("{n} burning cells put out; fuel unchanged"));
                let centre = polygon_centroid(&poly);
                self.event(
                    "extinguished",
                    format!(
                        "Fire put out over {:.1} ha drawn by {by} ({n} burning cells)",
                        polygon_area_m2(&poly) / 10_000.0
                    ),
                    self.scn.zone_of(centre),
                );
            }
            Command::SetWeather {
                wind_dir_deg,
                wind_speed_kmh,
                moisture_pct,
            } => {
                let mut w = self.fire.weather();
                if let Some(v) = wind_dir_deg {
                    w.wind_dir_deg = v.rem_euclid(360.0);
                }
                if let Some(v) = wind_speed_kmh {
                    w.wind_speed_kmh = *v;
                }
                if let Some(v) = moisture_pct {
                    w.moisture_pct = *v;
                }
                self.fire.set_weather(w).map_err(|e| e.to_string())?;
                self.event(
                    "weather",
                    format!(
                        "Weather now: wind from {:.0}° at {:.0} km/h, fuel moisture {:.0}%",
                        w.wind_dir_deg, w.wind_speed_kmh, w.moisture_pct
                    ),
                    None,
                );
            }
            Command::SetBuildingStatus {
                building,
                damage,
                evacuated,
            } => {
                let idx = match self.find_building(building) {
                    Some(i) => vec![i],
                    None => self.buildings_in_zone(&self.zone_code(building).unwrap()),
                };
                for i in idx {
                    if evacuated.is_some() && self.scn.operations.traffic {
                        let mut in_transit = 0;
                        for v in self
                            .d
                            .traffic
                            .vehicles
                            .iter_mut()
                            .filter(|v| v.building == i && !v.completed)
                        {
                            if v.departed {
                                in_transit += v.people;
                            }
                            v.completed = true;
                            v.detail = "EXCON building override".into();
                            for link in self.d.traffic.links.values_mut() {
                                link.queue.retain(|id| *id != v.id);
                            }
                        }
                        self.d.buildings[i].people_inside += in_transit;
                        self.d.buildings[i].evac_pending = 0;
                        self.d.buildings[i].evac_done_at = None;
                    }
                    let b = &mut self.d.buildings[i];
                    if let Some(d) = damage {
                        b.damage = *d;
                        b.exposure_s = match d {
                            Damage::Intact => 0.0,
                            Damage::Damaged => DAMAGE_EXPOSURE_S,
                            Damage::Destroyed => DESTROY_EXPOSURE_S,
                        };
                    }
                    match evacuated {
                        Some(true) => {
                            b.evacuated += b.people_inside;
                            b.people_inside = 0;
                            b.evac_pending = 0;
                            b.evac_done_at = None;
                        }
                        Some(false) => {
                            b.people_inside += b.evacuated;
                            b.evacuated = 0;
                        }
                        None => {}
                    }
                }
                self.event(
                    "building_status",
                    format!("EXCON set {}", cmd.describe().trim_start_matches("Set ")),
                    None,
                );
            }
            Command::SetDeploymentTravel {
                role,
                resource,
                at,
                speed_mps,
            } => {
                let origin = self.resolve_loc(at)?;
                self.d.travel_settings.get_mut(role).unwrap().insert(
                    resource.clone(),
                    TravelSetting {
                        origin,
                        speed_mps: *speed_mps,
                    },
                );
                self.event("deployment_travel_configured", cmd.describe(), None);
            }
            Command::ReturnResource { allocation, qty } => {
                let remaining = self
                    .d
                    .allocations
                    .iter()
                    .find(|a| a.id == *allocation)
                    .map(|a| a.qty - a.returned)
                    .unwrap_or(0);
                if *qty == remaining {
                    self.drone_recalled(*allocation);
                    if let Some(task) = self.d.tasks.iter_mut().find(|task| {
                        task.allocations.contains(allocation)
                            && !matches!(
                                task.status,
                                TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed
                            )
                    }) {
                        task.status = TaskStatus::Cancelled;
                        task.completed_s = Some(t);
                        task.detail =
                            "EXCON returned the allocation; accumulated effects persist".into();
                    }
                }
                let a = self
                    .d
                    .allocations
                    .iter_mut()
                    .find(|a| a.id == *allocation)
                    .unwrap();
                a.returned += *qty;
                a.last_return_at_s = Some(t);
                let (role, resource) = (a.role.clone(), a.resource.clone());
                self.resource_event("returned", Some(*allocation), &role, &resource, *qty as i64);
                self.event(
                    "resources_returned",
                    format!("{qty} {resource} returned to {role} from allocation #{allocation}"),
                    None,
                );
            }
            Command::AdjustResource {
                role,
                resource,
                delta,
            } => {
                let p = self
                    .d
                    .pools
                    .entry(role.clone())
                    .or_default()
                    .entry(resource.clone())
                    .or_insert(Pool { total: 0, used: 0 });
                p.total = (p.total as i64 + *delta as i64).max(0) as u32;
                self.resource_event("adjusted", None, role, resource, *delta as i64);
                self.event(
                    "resources",
                    format!("{role} {resource} adjusted by {delta:+}"),
                    None,
                );
            }
            Command::AdvanceTime { minutes } => {
                let ev = self
                    .advance(*minutes as i64 * 60)
                    .map_err(|e| e.to_string())?;
                out.push(format!("{} events", ev.len()));
            }
        }
        Ok(out)
    }

    fn start_evacuation(
        &mut self,
        idx: &[usize],
        destination: Option<&str>,
        constrained: bool,
    ) -> u32 {
        let t = self.time_s();
        let dest = destination
            .and_then(|d| self.find_building(d))
            .map(|i| self.scn.buildings[i].id);
        let mut total = 0;
        for &i in idx {
            let kind = self.scn.buildings[i].kind;
            let zone = self
                .scn
                .zone_of(self.scn.buildings[i].pos)
                .unwrap_or_default();
            let warned = self.d.warned.contains(&zone);
            let b = &mut self.d.buildings[i];
            let leaving = b.people_inside.saturating_sub(b.evac_pending);
            if leaving == 0 {
                continue;
            }
            let mut minutes = if warned { EVAC_WARNED_MIN } else { EVAC_MIN };
            if kind.needs_transport() && constrained {
                minutes += 10;
            }
            b.evac_pending += leaving;
            b.evac_done_at = Some(
                b.evac_done_at
                    .unwrap_or(t + minutes * 60)
                    .min(t + minutes * 60),
            );
            b.evac_destination = dest.or(b.evac_destination);
            if self.scn.operations.traffic {
                let ready = b.evac_done_at.unwrap();
                let destination = b.evac_destination;
                self.schedule_traffic(i, leaving, ready, destination);
            }
            total += leaving;
        }
        total
    }

    /// Occupied homes of a borgo not already leaving.
    pub(crate) fn district_targets(&self, key: &str) -> Result<Vec<usize>, String> {
        let d = self.scn.district(key).ok_or_else(|| {
            let known: Vec<&str> = self.scn.districts.iter().map(|d| d.name.as_str()).collect();
            if known.is_empty() {
                format!("this scenario has no borghi (unknown \"{key}\")")
            } else {
                format!("unknown borgo \"{key}\" (borghi: {})", known.join(", "))
            }
        })?;
        Ok(d.buildings
            .iter()
            .filter_map(|id| self.scn.buildings.iter().position(|b| b.id == *id))
            .filter(|&i| {
                let b = &self.d.buildings[i];
                b.people_inside > b.evac_pending
            })
            .collect())
    }

    /// Building id of a borgo's gathering area.
    fn gathering_of(&self, d: &scenario::District) -> Option<u32> {
        let name = d.gathering.as_deref()?;
        self.scn
            .buildings
            .iter()
            .find(|b| b.name.as_deref() == Some(name))
            .map(|b| b.id)
    }

    /// Gathering areas of every borgo: usable without `OpenShelter`.
    pub(crate) fn is_gathering(&self, id: u32) -> bool {
        self.cache.gathering.contains(&id)
    }

    /// The borgo's pace (`Scenario::district_evacuation`): everyone prepares,
    /// then homes leave in order of distance from the departure point (the
    /// road point nearest the homes' centre), at most `per_min` people a minute.
    fn start_district_evacuation(
        &mut self,
        d: &scenario::District,
        destination: Option<&str>,
        constrained: bool,
    ) -> u32 {
        let t = self.time_s();
        let pace = self.scn.district_evacuation;
        let Ok(mut idx) = self.district_targets(&d.id) else {
            return 0;
        };
        let n = idx.len().max(1) as f32;
        let centre = idx.iter().fold([0.0, 0.0], |c, &i| {
            let p = self.scn.buildings[i].pos;
            [c[0] + p[0] / n, c[1] + p[1] / n]
        });
        let departure = self.network.nearest_point(centre).unwrap_or(centre);
        let dist = |i: usize| network::distance(self.scn.buildings[i].pos, departure);
        idx.sort_by(|&a, &b| dist(a).total_cmp(&dist(b)));
        let dest = destination
            .and_then(|d| self.find_building(d))
            .map(|i| self.scn.buildings[i].id)
            .or_else(|| self.gathering_of(d));
        let mut gone = 0.0_f32;
        let mut total = 0;
        for i in idx {
            let mut ready = t + i64::from(pace.prep_min) * 60 + (gone / pace.per_min * 60.0).round() as i64;
            if self.scn.buildings[i].kind.needs_transport() && constrained {
                ready += 10 * 60;
            }
            let b = &mut self.d.buildings[i];
            let leaving = b.people_inside.saturating_sub(b.evac_pending);
            b.evac_pending += leaving;
            b.evac_done_at = Some(b.evac_done_at.map_or(ready, |r| r.min(ready)));
            b.evac_destination = dest.or(b.evac_destination);
            if self.scn.operations.traffic {
                let first = self.d.traffic.vehicles.len();
                self.schedule_traffic(i, leaving, ready, dest);
                if destination.is_none() {
                    for v in &mut self.d.traffic.vehicles[first..] {
                        v.gathering = true;
                    }
                }
            }
            gone += leaving as f32;
            total += leaving;
        }
        total
    }

    // ------------------------------------------------------------------ time

    /// Advance simulated time, returning the events that happened.
    pub fn advance(&mut self, seconds: i64) -> Result<Vec<SimEvent>> {
        let first = self.d.events.len();
        let mut left = seconds;
        while left > 0 {
            let dt = left.min(SUBSTEP_S);
            self.fire.advance(dt)?;
            self.refresh_hazard();
            self.step_buildings(dt as f32);
            self.step_roads();
            self.step_arrivals();
            self.step_tasks();
            self.step_traffic();
            left -= dt;
        }
        Ok(self.d.events[first..].to_vec())
    }

    fn step_buildings(&mut self, dt: f32) {
        let t = self.time_s();
        let gathering = self.cache.gathering.clone();
        let state = self.fire.state();
        let burning: Vec<Pos> = state
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == CellFire::Burning)
            .map(|(i, _)| {
                self.scn.cell_centre(scenario::Cell {
                    row: i / self.scn.grid.cols,
                    col: i % self.scn.grid.cols,
                })
            })
            .collect();
        let mut events = Vec::new();
        for (i, b) in self.d.buildings.iter_mut().enumerate() {
            let spec = &self.scn.buildings[i];
            // Damage needs confirmed fire in contact with the building (a
            // burning cell of the thresholded ensemble at the edge of its
            // plot). Distant fire, the uncertainty halo and far radiant/ember
            // loads only make it threatened, so the map, the impact report and
            // the AI never report damage the fire has not reached.
            let legacy_w = self.cache.near[i]
                .iter()
                .filter(|(c, _)| state[*c] == CellFire::Burning)
                .map(|(_, w)| *w)
                .fold(0.0, f32::max);
            let w = if legacy_w <= 0.0 {
                0.0
            } else if self.scn.operations.intensity_exposure {
                let load = &self.d.hazard.loads[i];
                (load.radiant + load.ember / 3.0).min(1.0)
            } else {
                legacy_w
            };
            let was = b.damage;
            if w > 0.0 && b.damage != Damage::Destroyed {
                b.exposure_s += w * dt;
                if b.exposure_s >= DESTROY_EXPOSURE_S {
                    b.damage = Damage::Destroyed;
                } else if b.exposure_s >= DAMAGE_EXPOSURE_S {
                    b.damage = Damage::Damaged;
                }
            }
            let r2 = THREAT_RADIUS_M * THREAT_RADIUS_M;
            let threatened = burning
                .iter()
                .any(|p| (p[0] - spec.pos[0]).powi(2) + (p[1] - spec.pos[1]).powi(2) < r2);
            if threatened && !b.threatened && spec.name.is_some() {
                events.push((
                    "threatened",
                    format!(
                        "Fire within {THREAT_RADIUS_M:.0} m of {}",
                        spec.name.as_deref().unwrap()
                    ),
                    spec.pos,
                ));
            }
            b.threatened = threatened;
            if b.damage != was {
                let label = spec
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("house #{}", spec.id));
                let stayed = b.people_inside;
                if stayed > 0 && b.at_risk == 0 {
                    b.at_risk = stayed;
                }
                let risk = if stayed > 0 {
                    format!(" — {stayed} people still inside")
                } else {
                    String::new()
                };
                let kind = if b.damage == Damage::Destroyed {
                    "building_destroyed"
                } else {
                    "building_damaged"
                };
                events.push((
                    kind,
                    format!(
                        "{label} {}{risk}",
                        if b.damage == Damage::Destroyed {
                            "destroyed by fire"
                        } else {
                            "damaged by fire"
                        }
                    ),
                    spec.pos,
                ));
            }
            // Evacuations completing.
            if let Some(done) = b.evac_done_at {
                if t >= done && !self.scn.operations.traffic {
                    let moved = b.evac_pending.min(b.people_inside);
                    b.people_inside -= moved;
                    b.evacuated += moved;
                    b.evac_pending = 0;
                    b.evac_done_at = None;
                    if let Some(dest) = b.evac_destination {
                        if let Some(n) = self.d.shelters.get_mut(&dest) {
                            *n += moved;
                        } else if gathering.contains(&dest) {
                            self.d.shelters.insert(dest, moved);
                        }
                    }
                    if spec.name.is_some() {
                        events.push((
                            "evacuated",
                            format!(
                                "{} evacuated ({moved} people)",
                                spec.name.as_deref().unwrap()
                            ),
                            spec.pos,
                        ));
                    }
                }
            }
        }
        for (kind, text, pos) in events {
            let z = self.scn.zone_of(pos);
            self.event(kind, text, z);
        }
    }

    fn step_roads(&mut self) {
        if self.scn.operations.road_routing && self.d.hazard.updated_s != self.time_s() {
            return;
        }
        if self.scn.operations.segment_fire_closures {
            let mut cuts = Vec::new();
            for segment in self.network.segments() {
                let n = (network::distance(segment.from, segment.to)
                    / (self.scn.grid.cellsize * 0.5))
                    .ceil()
                    .max(1.0) as usize;
                let unsafe_segment = (0..=n).any(|i| {
                    let p = network::interpolate(segment.from, segment.to, i as f32 / n as f32);
                    self.d.hazard.at(p).danger >= 0.55
                        || self.scn.cell_of(p).is_some_and(|c| {
                            self.fire.state()[self.scn.grid.index(c)] == CellFire::Burning
                        })
                });
                if unsafe_segment {
                    let state = self.d.roads.get_mut(&segment.road).unwrap();
                    let links = state.fire_closed_links.get_or_insert_with(Vec::new);
                    if !links.contains(&segment.links[0]) {
                        links.extend(segment.links);
                        links.sort_unstable();
                        state.cut_by_fire = true;
                        cuts.push(segment);
                    }
                }
            }
            for segment in cuts {
                self.event(
                    "road_segment_cut",
                    format!("{} links {:?} cut by fire", segment.road, segment.links),
                    self.scn.zone_of(segment.from),
                );
            }
            return;
        }
        let state = self.fire.state();
        let mut cut = Vec::new();
        for (id, cells) in &self.cache.roads {
            let rs = &self.d.roads[id];
            if !rs.cut_by_fire
                && cells.iter().any(|&c| {
                    state[c] == CellFire::Burning
                        || (self.scn.operations.road_routing
                            && self
                                .d
                                .hazard
                                .at(self.scn.cell_centre(scenario::Cell {
                                    row: c / self.scn.grid.cols,
                                    col: c % self.scn.grid.cols,
                                }))
                                .danger
                                >= 0.55)
                })
            {
                cut.push(id.clone());
            }
        }
        for id in cut {
            self.d.roads.get_mut(&id).unwrap().cut_by_fire = true;
            let name = self.scn.road(&id).map(|r| r.name.clone()).unwrap_or(id);
            self.event("road_cut", format!("{name} is cut by the fire"), None);
        }
    }

    fn step_arrivals(&mut self) {
        let t = self.time_s();
        let due: Vec<Arrival> = self
            .d
            .arrivals
            .iter()
            .filter(|a| a.at_s <= t && a.at_s > t - SUBSTEP_S)
            .cloned()
            .collect();
        for a in due {
            let p = self
                .d
                .pools
                .entry(a.role.clone())
                .or_default()
                .entry(a.resource.clone())
                .or_insert(Pool { total: 0, used: 0 });
            p.total += a.qty;
            self.resource_event("arrived", None, &a.role, &a.resource, a.qty as i64);
            self.event(
                "reinforcement_arrived",
                format!(
                    "{} {} from {} arrived for {}",
                    a.qty, a.resource, a.source, a.role
                ),
                None,
            );
        }
    }

    // ------------------------------------------------------------------ stats

    pub fn stats(&self) -> Stats {
        let b = &self.d.buildings;
        Stats {
            time_s: self.time_s(),
            clock: self.clock(),
            burned_ha: self.fire.burned_area_ha(),
            burning_cells: self.fire.burning_cells(),
            damaged: b.iter().filter(|x| x.damage == Damage::Damaged).count(),
            destroyed: b.iter().filter(|x| x.damage == Damage::Destroyed).count(),
            threatened: b.iter().filter(|x| x.threatened).count(),
            evacuated_people: b.iter().map(|x| x.evacuated).sum(),
            people_at_risk: b.iter().map(|x| x.at_risk).sum(),
            sheltered: self.d.shelters.values().sum(),
            line_km: self
                .d
                .lines
                .iter()
                .map(|l| ((l.to[0] - l.from[0]).powi(2) + (l.to[1] - l.from[1]).powi(2)).sqrt())
                .sum::<f32>()
                / 1000.0
                + 0.0,
            drops: self.d.drops.len(),
        }
    }

    /// Zones with burning cells.
    pub fn burning_zones(&self) -> Vec<String> {
        let cols = self.scn.grid.cols;
        let mut set = BTreeSet::new();
        for (i, s) in self.fire.state().iter().enumerate() {
            if *s == CellFire::Burning {
                if let Some(z) = self.scn.zone_of(self.scn.cell_centre(scenario::Cell {
                    row: i / cols,
                    col: i % cols,
                })) {
                    set.insert(z);
                }
            }
        }
        set.into_iter().collect()
    }
}

impl WorldSnapshot {
    pub fn time_s(&self) -> i64 {
        self.time_s
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub time_s: i64,
    pub clock: String,
    pub burned_ha: f64,
    pub burning_cells: usize,
    pub damaged: usize,
    pub destroyed: usize,
    pub threatened: usize,
    pub evacuated_people: u32,
    pub people_at_risk: u32,
    pub sheltered: u32,
    pub line_km: f32,
    pub drops: usize,
}

impl Cache {
    fn build(scn: &Scenario, fire: &FireSim) -> Cache {
        let contact = contact_radius_m(scn.grid.cellsize);
        let near = scn
            .buildings
            .iter()
            .map(|b| {
                fire.cells_in_radius(b.pos, contact)
                    .into_iter()
                    .map(|c| {
                        let q = scn.cell_centre(c);
                        let d = ((q[0] - b.pos[0]).powi(2) + (q[1] - b.pos[1]).powi(2)).sqrt();
                        let w = if d <= 30.0 { 1.0 } else { 0.5 };
                        (scn.grid.index(c), w)
                    })
                    .collect()
            })
            .collect();
        let roads = scn
            .roads
            .iter()
            .map(|r| {
                let mut cells: Vec<usize> = r
                    .points
                    .windows(2)
                    .flat_map(|w| fire.cells_along(w[0], w[1], 10.0))
                    .map(|c| scn.grid.index(c))
                    .collect();
                cells.sort_unstable();
                cells.dedup();
                (r.id.clone(), cells)
            })
            .collect();
        let gathering = scn
            .districts
            .iter()
            .filter_map(|d| d.gathering.as_deref())
            .filter_map(|g| scn.buildings.iter().find(|b| b.name.as_deref() == Some(g)))
            .map(|b| b.id)
            .collect();
        Cache {
            near,
            roads,
            gathering,
        }
    }
}

/// True if a building kind should be listed individually in digests.
pub fn is_facility(kind: BuildingKind) -> bool {
    kind != BuildingKind::House
}
