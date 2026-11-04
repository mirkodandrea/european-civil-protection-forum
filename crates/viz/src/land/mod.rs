//! The landscape of the desktop game `propagator_abm`, drawn for a territory
//! that carries one (`scenario::land`, e.g. Isola di Roccavento): an adaptive
//! terrain mesh on the 5 m heights with the roads graded into it, the ground
//! painted from the 5 m land cover, the canopy on the ground, instanced
//! Blender plants that catch as the front reaches them, grass and shrubs
//! round the camera, farmland parcels, and the fire's scar and glowing band
//! drawn by the terrain's own shader.
//!
//! The modules are the source game's, ported as they are; the few things
//! they read from the source's world come from small stand-ins here:
//!
//! - [`sim::Sim`]: the source's simulation, as far as drawing goes, rebuilt
//!   from each [`crate::Snapshot`]. Only the **actual** fire (cells reached by
//!   at least `cutoff` of the ensemble's runs, codes `BURNING`/`BURNT`) burns
//!   plants and scars the ground; the halo's probability travels apart
//!   ([`sim::Fire::halo`]) and never ignites anything. A rollback is a
//!   restart: what had burnt grows back.
//! - [`quality::Graphics`]: the source's graphics level, from the TTX one.
//! - [`visual_scale`]: the source's diorama proportions, all 1 (true scale).
//!
//! The sea, the coast's rocks, the landmarks, the boats, the land on the
//! horizon and the god rays are the source's too; so are the buildings,
//! extruded from their traced footprints with the town kit's landmarks and
//! open spaces, coloured by the exercise's building states
//! ([`buildings::damage`]).
//!
//! Without a landscape the TTX's own terrain (`crate::terrain`) is drawn.

// The ported modules keep the source's API (lab hooks, unit meshes, setters
// the TTX does not call) so they can be re-synced with it.
#![allow(dead_code)]

pub mod boats;
pub mod buildings;
pub mod coast;
pub mod field;
pub mod fields;
pub mod godrays;
pub mod horizon;
pub mod landmarks;
pub mod sea;
pub mod ground_mask;
pub mod models;
pub mod near_cover;
pub mod roads;
pub mod terrain_material;
pub mod terrain_mesh;
pub mod terrain_tin;
pub mod vegetation;

use bevy::prelude::*;

use crate::{Scn, Snapshot};

/// The scenario types the ported modules read, under the source's names.
pub mod sc {
    pub use scenario::land::{grade, Building, Cell, Cover, CoverClass, Land as Scenario, Pos, World};
}

/// The source's diorama proportions. The TTX draws true scale: all 1.
pub mod visual_scale {
    pub const BUILDING_FOOTPRINT: f32 = 1.0;
    pub const BUILDING_HEIGHT: f32 = 1.0;
    pub const CAR: f32 = 1.0;
    pub const BOAT: f32 = 1.0;
    pub const TREE: f32 = 1.0;
    pub const SHRUB: f32 = 1.0;
}

/// Stand-ins for the source's camera markers.
pub mod camera {
    /// The camera the near cover follows: the TTX's one 3D camera.
    pub type OrbitCamera = bevy::prelude::Camera3d;
    /// Never spawned here (the source's picture-in-picture views).
    #[derive(bevy::prelude::Component)]
    pub struct AuxiliaryCamera;
}

pub mod materials {
    pub use super::terrain_material::TerrainMaterial;
    pub use crate::materials::{structure, surface_alpha, Finish, StructureMaterial};
}

pub mod quality {
    //! The source's graphics level (`quality::Graphics`), from the TTX's.
    use bevy::prelude::*;

    /// The browser build (WebGL2).
    pub const WEBGL2: bool = cfg!(target_arch = "wasm32");

    #[derive(Resource, Clone, Copy, Debug)]
    pub struct Graphics {
        /// Share of the authored plant densities that is planted.
        pub vegetation: f32,
        /// Distance (m) beyond which plants are drawn as proxies; 0: proxies only.
        pub lod_m: f32,
        pub terrain_shadows: bool,
        /// Sun shadows at all (the plants cast them when on).
        pub shadows: bool,
        /// Terrain samples per mesh vertex of the finest level.
        pub terrain_stride: usize,
        /// Radius (m) of the grass drawn round the camera; 0: none.
        pub cover_m: f32,
        /// Share of the plants built that are shown.
        pub veg_keep: f32,
        /// The stand-ins are lumps (the browser's).
        pub lobes: bool,
        /// The highest level: god rays.
        pub alta: bool,
    }

    impl Graphics {
        /// The source's «alta».
        pub const ALTA: Graphics = Graphics {
            vegetation: if WEBGL2 { 0.12 } else { 0.5 },
            lod_m: 1400.0,
            terrain_shadows: true,
            shadows: true,
            terrain_stride: if WEBGL2 { 4 } else { 1 },
            cover_m: if WEBGL2 { 0.0 } else { 45.0 },
            veg_keep: 1.0,
            lobes: WEBGL2,
            alta: true,
        };
        /// «media».
        pub const MEDIA: Graphics = Graphics {
            vegetation: if WEBGL2 { 0.12 } else { 0.35 },
            lod_m: 1000.0,
            cover_m: if WEBGL2 { 0.0 } else { 35.0 },
            alta: false,
            ..Graphics::ALTA
        };
        /// «bassa».
        pub const BASSA: Graphics = Graphics {
            vegetation: if WEBGL2 { 0.08 } else { 0.2 },
            lod_m: 600.0,
            cover_m: if WEBGL2 { 0.0 } else { 22.0 },
            alta: false,
            ..Graphics::ALTA
        };

        /// `high` is «alta», the projector «media», `low` «bassa». The TTX
        /// level's plant share scales the density (`TTX_GFX=veg=…`).
        pub fn for_quality(q: &crate::quality::Quality) -> Graphics {
            let (base, authored) = match q.name {
                "high" => (Graphics::ALTA, crate::quality::Quality::HIGH.vegetation),
                "low" => (Graphics::BASSA, crate::quality::Quality::LOW.vegetation),
                _ => (Graphics::MEDIA, crate::quality::Quality::PROJECTOR.vegetation),
            };
            let mut g = base;
            if authored > 0.0 {
                g.vegetation *= (q.vegetation / authored).clamp(0.0, 2.0);
            }
            g.terrain_shadows = q.shadows;
            g.shadows = q.shadows;
            if let Some(v) = std::env::var("TTX_LAND_VEG").ok().and_then(|v| v.parse::<f32>().ok()) {
                g.vegetation = v.clamp(0.0, 1.0);
            }
            g
        }
    }
}

/// The source's frame conversion (`frame::to_bevy`): map metres and an
/// elevation to Bevy, the same frame as [`crate::frame`].
pub mod frame {
    use bevy::prelude::Vec3;

    #[inline]
    pub fn to_bevy(p: super::sc::Pos, elev: f32) -> Vec3 {
        Vec3::new(p.x, elev, -p.y)
    }

    #[inline]
    pub fn to_world(v: Vec3) -> super::sc::Pos {
        super::sc::Pos { x: v.x, y: -v.z }
    }
}

pub mod sim {
    //! The source's simulation as the renderer reads it, from the TTX's
    //! snapshot.
    use std::sync::Arc;

    use bevy::prelude::*;
    use scenario::land::Land;
    use view::cell;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CellFire {
        Unburnt = 0,
        Burning = 1,
        Burnt = 2,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Weather {
        pub wind_dir_deg: f64,
        pub wind_speed_kmh: f64,
    }

    pub struct Fire {
        state: Vec<CellFire>,
        /// Arrival of the actual fire, seconds; `i32::MIN` where it has not come.
        arrival: Vec<i32>,
        intensity: Vec<f32>,
        cleared: Vec<bool>,
        weather: Weather,
        /// Cleared cells, so a new cut line redraws the plants.
        pub line_cells: usize,
        /// Share of the runs that reached each cell the actual fire has not
        /// (the halo), 0 elsewhere: drawn, never burnt.
        pub halo: Vec<f32>,
        /// Wetted by water or retardant: drawn, the model's own state.
        pub wet: Vec<bool>,
    }

    impl Fire {
        pub fn state(&self) -> &[CellFire] {
            &self.state
        }
        pub fn arrival_times(&self) -> &[i32] {
            &self.arrival
        }
        pub fn intensity(&self) -> &[f32] {
            &self.intensity
        }
        pub fn cleared(&self) -> &[bool] {
            &self.cleared
        }
        pub fn weather(&self) -> Weather {
            self.weather
        }
    }

    /// The fire went back (a rollback, a new exercise): plants grow back.
    #[derive(Message)]
    pub struct SimRestarted;

    #[derive(Resource)]
    pub struct Sim {
        pub scn: Arc<Land>,
        pub fire: Fire,
        /// Changes whenever what is drawn of the fire changes.
        pub generation: u64,
        time_s: i64,
    }

    impl Sim {
        pub fn new(scn: Arc<Land>, weather: scenario::WeatherSpec) -> Sim {
            let n = scn.world.fire_rows * scn.world.fire_cols;
            Sim {
                scn,
                fire: Fire {
                    state: vec![CellFire::Unburnt; n],
                    arrival: vec![i32::MIN; n],
                    intensity: vec![0.0; n],
                    cleared: vec![false; n],
                    weather: Weather { wind_dir_deg: weather.wind_dir_deg, wind_speed_kmh: weather.wind_speed_kmh },
                    line_cells: 0,
                    halo: vec![0.0; n],
                    wet: vec![false; n],
                },
                generation: 0,
                time_s: 0,
            }
        }

        pub fn time_s(&self) -> i64 {
            self.time_s
        }

        /// The TTX has no end of the run the renderer should know of.
        pub fn is_over(&self) -> bool {
            false
        }

        /// Take the drawn state of the fire from a snapshot: `cells` codes
        /// (`view::cell`), quantile arrivals, mean intensities, probabilities.
        /// Returns whether the fire went back (something burnt is unburnt).
        pub fn take(&mut self, time_s: i64, cells: &[u8], arrival: &[i32], intensity: &[f32], probability: &[f32], weather: Option<scenario::WeatherSpec>) -> bool {
            if cells.len() != self.fire.state.len() {
                return false;
            }
            let mut back = time_s < self.time_s;
            let f = &mut self.fire;
            let mut line = 0;
            for (i, &code) in cells.iter().enumerate() {
                let state = match code {
                    cell::BURNING => CellFire::Burning,
                    cell::BURNT => CellFire::Burnt,
                    _ => CellFire::Unburnt,
                };
                back |= state == CellFire::Unburnt && f.state[i] != CellFire::Unburnt;
                f.state[i] = state;
                f.arrival[i] = match (state, arrival.get(i).copied()) {
                    (CellFire::Unburnt, _) => i32::MIN,
                    (_, Some(a)) if a >= 0 => a,
                    _ => time_s as i32,
                };
                f.intensity[i] = if state == CellFire::Unburnt { 0.0 } else { intensity.get(i).copied().unwrap_or(0.0) };
                let cut = code == cell::CLEARED;
                back |= f.cleared[i] && !cut;
                f.cleared[i] = cut;
                line += usize::from(cut);
                f.wet[i] = code == cell::WET;
                f.halo[i] = if code == cell::HALO { probability.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) } else { 0.0 };
            }
            f.line_cells = line;
            if let Some(w) = weather {
                f.weather = Weather { wind_dir_deg: w.wind_dir_deg, wind_speed_kmh: w.wind_speed_kmh };
            }
            self.time_s = time_s;
            self.generation += 1;
            back
        }
    }
}

/// Whether landscapes are drawn at all (`TTX_LAND=0`: the TTX's own
/// terrain everywhere, for comparisons).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("TTX_LAND").is_ok_and(|v| v == "0"))
}

/// Whether the landscape is drawn for this scenario.
pub fn active(scn: &scenario::Scenario) -> bool {
    enabled() && scn.land_ref().is_some()
}

/// The landscape's own build schedule, run whenever the scene is (re)built
/// (from `Startup` and the TTX's `SceneSetup`, see [`run_scene`]).
#[derive(bevy::ecs::schedule::ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LandScene;

/// The source modules' spawns (`OnEnter(Playing)` there) go in this schedule.
pub const ON_SCENE: LandScene = LandScene;

/// Stand-ins in place, terrain, plants, grass and fields built.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LandPrepare;

/// The source modules' spawns, after [`LandPrepare`], only with a landscape.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LandSpawn;

pub struct LandPlugin;

impl Plugin for LandPlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(LandScene);
        app.add_message::<sim::SimRestarted>()
            .configure_sets(LandScene, LandSpawn.after(LandPrepare).run_if(resource_exists::<sim::Sim>))
            .add_systems(LandScene, setup_systems().in_set(LandPrepare))
            .add_plugins((terrain_material::TerrainPlugin, near_cover::NearCoverPlugin, vegetation::PlantRenderPlugin))
            .add_systems(LandScene, buildings::spawn.in_set(LandSpawn))
            .add_systems(Update, buildings::damage.after(crate::frame::update).run_if(resource_exists::<buildings::Buildings>))
            .add_plugins((sea::SeaPlugin, coast::CoastPlugin, horizon::HorizonPlugin, boats::BoatsPlugin, landmarks::LandmarksPlugin, godrays::GodRaysPlugin))
            .add_systems(
                Update,
                (
                    sync,
                    field_fire_map,
                    vegetation::burn,
                    vegetation::clock,
                    near_cover::update,
                )
                    .chain()
                    .after(crate::frame::update)
                    .run_if(resource_exists::<sim::Sim>),
            );
    }
}

/// Build the landscape of the scenario in hand (nothing without one).
pub fn run_scene(world: &mut World) {
    world.run_schedule(LandScene);
}

/// The scene's landscape systems, in order.
fn setup_systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (
        prepare,
        build_terrain.run_if(resource_exists::<sim::Sim>),
        vegetation::spawn.run_if(resource_exists::<sim::Sim>),
        near_cover::spawn.run_if(resource_exists::<sim::Sim>),
        build_fields.run_if(resource_exists::<sim::Sim>),
    )
        .chain()
}

/// Put the stand-ins in place for the scenario in hand, or take them away.
fn prepare(mut commands: Commands, scn: Res<Scn>, q: Res<crate::quality::Quality>) {
    commands.remove_resource::<vegetation::Vegetation>();
    commands.remove_resource::<near_cover::NearCover>();
    commands.remove_resource::<buildings::Buildings>();
    match scn.0.land().filter(|_| active(&scn.0)) {
        Some(land) => {
            commands.insert_resource(sim::Sim::new(land, scn.0.weather));
            commands.insert_resource(quality::Graphics::for_quality(&q));
        }
        None => {
            commands.remove_resource::<sim::Sim>();
        }
    }
}

fn build_terrain(
    sim: Res<sim::Sim>,
    gfx: Res<quality::Graphics>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<terrain_material::TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    terrain_mesh::build(&sim.scn, &mut commands, &mut meshes, &mut materials, &mut images, &gfx);
}

fn build_fields(sim: Res<sim::Sim>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    fields::build(&sim.scn, &mut commands, &mut meshes, &mut materials);
}

/// The drawn fire from the newest snapshot.
fn sync(snap: Res<Snapshot>, mut sim: ResMut<sim::Sim>, mut restarted: MessageWriter<sim::SimRestarted>, mut seen: Local<Option<(u64, i64)>>) {
    let key = (snap.generation, snap.time_s);
    if *seen == Some(key) || snap.cells.is_empty() {
        return;
    }
    *seen = Some(key);
    if sim.take(snap.time_s, &snap.cells, &snap.arrival, &snap.intensity, &snap.probability, snap.weather) {
        restarted.write(sim::SimRestarted);
    }
}

/// The scar and the glow on the ground: the source's fire map.
fn field_fire_map(sim: Res<sim::Sim>, time: Res<Time>, mut state: Local<FireMapState>, mut images: ResMut<Assets<Image>>) {
    state.since += time.delta_secs();
    if Some(sim.generation) == state.written || state.since < 0.15 {
        return;
    }
    let Some(mut image) = images.get_mut(&ground_mask::GRASS_MAP) else { return };
    let Some(data) = image.data.as_mut() else { return };
    state.written = Some(sim.generation);
    state.since = 0.0;
    let w = sim.scn.world;
    let now = sim.time_s() as f32;
    let (st, arrival, cleared) = (sim.fire.state(), sim.fire.arrival_times(), sim.fire.cleared());
    let f16 = terrain_mesh::f16_bits;
    for r in 0..w.fire_rows {
        for c in 0..w.fire_cols {
            let i = r * w.fire_cols + c;
            let burnt = st[i] != sim::CellFire::Unburnt;
            // half floats: ages past ~16 h all read as long cold
            let since = match (burnt, arrival[i]) {
                (false, _) => 0.0,
                (true, i32::MIN) => -60_000.0,
                (true, t) => (t as f32 - now).clamp(-60_000.0, 0.0),
            };
            let at = ((w.fire_rows - 1 - r) * w.fire_cols + c) * 8;
            if at + 8 > data.len() {
                return;
            }
            data[at + 2..at + 4].copy_from_slice(&f16(f32::from(burnt)).to_le_bytes());
            data[at + 4..at + 6].copy_from_slice(&f16(since).to_le_bytes());
            data[at + 6..at + 8].copy_from_slice(&f16(f32::from(cleared[i])).to_le_bytes());
        }
    }
    drop(image);
    // the ensemble's halo and wetted ground: drawn, never burnt (`ground_mask::FIRE_EXTRA`)
    let Some(mut image) = images.get_mut(&ground_mask::FIRE_EXTRA) else { return };
    let Some(data) = image.data.as_mut() else { return };
    for r in 0..w.fire_rows {
        for c in 0..w.fire_cols {
            let i = r * w.fire_cols + c;
            let at = ((w.fire_rows - 1 - r) * w.fire_cols + c) * 4;
            if at + 4 > data.len() {
                return;
            }
            data[at] = if sim.fire.halo[i] > 0.0 { 255 } else { 0 };
            data[at + 1] = if sim.fire.wet[i] { 255 } else { 0 };
        }
    }
}

#[derive(Default)]
struct FireMapState {
    since: f32,
    written: Option<u64>,
}

/// The shipped Roccavento territory's landscape, for the tests.
#[cfg(test)]
pub(crate) fn test_land() -> Option<std::sync::Arc<scenario::land::Land>> {
    static LAND: std::sync::OnceLock<Option<std::sync::Arc<scenario::land::Land>>> = std::sync::OnceLock::new();
    LAND.get_or_init(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenarios/roccavento_island.json");
        let s: scenario::Scenario = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        s.land()
    })
    .clone()
}
