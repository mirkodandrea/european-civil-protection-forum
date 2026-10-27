//! The big-screen renderer.
//!
//! Knows the exercise only through a [`view::ViewSource`]: each frame it polls
//! for a newer [`view::ViewState`] (never blocking) and, when one arrives,
//! copies what it draws into a [`Snapshot`] resource. The model can live in
//! this process, in the browser's WASM, or behind HTTP on another machine, and
//! the renderer can never change it: it has no dependency on the simulator or
//! the exercise.
//!
//! Turns are simulated live: while the EXCON resolves a turn the model
//! advances the world one simulated minute at a time (optionally paced), and
//! every step is a new view state, so this renderer shows the fire, the
//! buildings and the counters as they change.
//!
//! Vegetation, landmarks and articulated unit bakes come from `propagator_abm`
//! under EUPL-1.2.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use view::{
    BuildingDraw, Cue, Deployment, Ensemble, FireLine, Have, RoadSegment, Stats, TurnWindow,
    Vehicle, ViewSource,
};

pub mod editor;
mod architecture;
mod bench;
mod camera;
mod deployments;
mod fire_field;
mod fire_shader;
mod fire_textures;
mod flames;
mod frame;
mod hud;
mod materials;
mod models;
mod night;
mod objects;
mod quality;
mod quality_live;
mod sky;
mod sky_light;
mod snapshot;
mod terrain;

pub use frame::Frame;

#[derive(bevy::ecs::schedule::ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct SceneSetup;

/// What the HUD says about the surroundings of the viewer.
#[derive(Resource, Clone, Default)]
pub struct Info {
    /// The AI model the exercise uses ("off" when none).
    pub llm_model: String,
    /// Port of the exercise web server, when this process hosts it.
    pub port: Option<u16>,
    /// Where the state comes from, when it is not this process.
    pub remote: Option<String>,
}

/// Everything the renderer draws, copied from the view state.
#[derive(Resource, Default)]
pub struct Snapshot {
    pub deployments: Vec<Deployment>,
    pub traffic: Vec<Vehicle>,
    pub generation: u64,
    pub turn: u32,
    pub phase: String,
    pub time_s: i64,
    pub clock: String,
    pub weather: Option<scenario::WeatherSpec>,
    pub stats: Option<Stats>,
    pub ensemble: Option<Ensemble>,
    /// Final per-cell code (see [`view::cell`]).
    pub cells: Vec<u8>,
    pub arrival: Vec<i32>,
    /// Ensemble-mean fireline intensity (kW/m) and fraction of runs reached, per cell.
    pub intensity: Vec<f32>,
    pub probability: Vec<f32>,
    pub buildings: Vec<BuildingDraw>,
    pub roads: Vec<(bool, bool)>,
    pub road_segments: Vec<RoadSegment>,
    pub lines: Vec<FireLine>,
    pub drops: Vec<view::Drop>,
    pub warned: Vec<String>,
    pub last_turn: Option<TurnWindow>,
    /// While a turn is being simulated.
    pub resolving: Option<TurnWindow>,
    /// EXCON free-running clock between turns (distinct from `resolving`).
    pub clock_running: bool,
    pub feed: Vec<String>,
    /// Camera cues, append-only.
    pub cues: Vec<Cue>,
    cue_seen: u64,
}

/// The scenario, shared by all systems.
#[derive(Resource, Clone)]
pub struct Scn(pub Arc<scenario::Scenario>);

/// Open the window and run until it is closed.
///
/// The renderer knows the exercise only through `source`: in this process
/// (`server::view::ExerciseSource`) or on another machine.
pub fn run(source: Box<dyn ViewSource>, info: Info) {
    run_internal(source, info, None);
}

fn run_internal(mut source: Box<dyn ViewSource>, info: Info, editor: Option<editor::Editor>) {
    let scn = match source.scenario() {
        Ok(scn) => scn,
        Err(e) => {
            eprintln!("cannot load the scenario: {e}");
            return;
        }
    };
    let quality = quality::Quality::from_env();
    let capture = std::env::var("TTX_SNAPSHOT").is_ok();
    #[allow(unused_mut)] // mutated only on wasm32
    let mut primary_window = Window {
        title: "PPRD East TTX — simulator".into(),
        visible: !capture,
        focused: !capture,
        resolution: snapshot::resolution(),
        present_mode: if snapshot::bench_s().is_some() {
            bevy::window::PresentMode::AutoNoVsync
        } else {
            bevy::window::PresentMode::AutoVsync
        },
        ..default()
    };
    #[cfg(target_arch = "wasm32")]
    {
        primary_window.canvas = Some("#standalone-3d".into());
        primary_window.fit_canvas_to_parent = true;
    }
    let mut app = App::new();
    if let Some(editor) = editor {
        app.insert_resource(editor);
    }
    app
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(primary_window),
                    ..default()
                })
                // The app installs its own tracing subscriber.
                .disable::<bevy::log::LogPlugin>(),
        )
        .add_plugins(EguiPlugin)
        .add_plugins(flames::FlamesPlugin)
        .add_plugins(materials::MaterialsPlugin)
        .add_plugins(sky::SkyPlugin)
        .add_plugins(snapshot::SnapshotPlugin)
        .insert_resource(if capture {
            bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(
                1.0 / 60.0,
            ))
        } else {
            bevy::time::TimeUpdateStrategy::Automatic
        })
        .insert_resource(quality.msaa_mode())
        .insert_resource(bevy::pbr::DirectionalLightShadowMap {
            size: quality.shadow_map,
        })
        .insert_resource(quality)
        .init_resource::<quality::Rebuild>()
        .insert_resource(ClearColor(camera::SKY))
        // Sky-tinted fill light; the sun does the modelling.
        .insert_resource(AmbientLight {
            color: Color::srgb(0.70, 0.78, 0.95),
            brightness: 140.0,
        })
        .insert_resource(Source(Mutex::new(source)))
        .insert_resource(info)
        .insert_resource(Scn(scn))
        .init_resource::<Snapshot>()
        .init_resource::<fire_field::FirePresentation>()
        .init_resource::<Frame>()
        .add_systems(
            Startup,
            (
                camera::setup,
                terrain::setup,
                objects::setup,
                night::setup,
                deployments::setup,
            ),
        )
        .add_systems(
            SceneSetup,
            (
                camera::setup,
                terrain::setup,
                objects::setup,
                night::setup,
                deployments::setup,
                flames::setup.after(terrain::setup),
            ),
        )
        .add_systems(First, replace_scene)
        .add_systems(PreUpdate, editor::paste_input
            .after(bevy_egui::EguiSet::ProcessInput)
            .before(bevy_egui::EguiSet::BeginFrame))
        .add_systems(
            Update,
            (
                sync,
                frame::update.after(sync),
                fire_field::toggle,
                terrain::update
                    .after(frame::update)
                    .after(fire_field::toggle),
                objects::update.after(frame::update),
                objects::night_windows,
                night::update,
                deployments::night_vehicles,
                deployments::update.after(frame::update),
                camera::control.after(frame::update),
                camera::fire_lights.after(frame::update),
                hud::draw.after(frame::update).after(fire_field::toggle),
                editor::draw,
                editor::interact.after(editor::draw),
                screenshot,
                record,
                snapshot::hold.after(camera::control).after(sync),
                snapshot::capture.after(snapshot::hold),
                terrain::lod,
                quality_live::apply,
            ),
        )
        .run();
}

/// The source of views, polled each frame. Its `poll` never blocks.
#[derive(Resource)]
pub struct Source(Mutex<Box<dyn ViewSource>>);

/// The newest edited scenario that has been shown only through the light
/// refresh (terrain mesh), waiting for a full rebuild once the brush rests.
#[derive(Resource, Default)]
struct SceneBuild {
    pending: Option<Arc<scenario::Scenario>>,
}

/// Rebuild only presentation entities. The source owns the exercise and all physics.
fn replace_scene(world: &mut World) {
    let current = world.resource::<Scn>().0.clone();
    // A graphics level whose knobs are baked into the scene (terrain stride,
    // plant density) was picked: build it again from the scenario in hand.
    if world.get_resource_mut::<quality::Rebuild>().is_some_and(|mut r| std::mem::take(&mut r.0)) {
        return replace_with(world, current.clone(), current);
    }
    let polled = world
        .resource::<Source>()
        .0
        .lock()
        .ok()
        .and_then(|mut source| source.poll_scene(&current));
    // A full rebuild reconstructs terrain textures and vegetation (seconds on a
    // large territory), so while the editor brush is working only the terrain
    // mesh follows; the rebuild happens once the brush has rested.
    let now = world.get_resource::<Time>().map_or(0., |t| t.elapsed_seconds_f64());
    if world.get_resource::<editor::Editor>().is_some_and(|e| e.busy(now)) {
        if let Some(next) = polled {
            let same_grid = current.grid.rows == next.grid.rows
                && current.grid.cols == next.grid.cols
                && current.grid.cellsize == next.grid.cellsize;
            if same_grid {
                terrain::reshape(world, &next);
                world.insert_resource(Scn(next.clone()));
                world.get_resource_or_insert_with(SceneBuild::default).pending = Some(next);
                return;
            }
            world.get_resource_or_insert_with(SceneBuild::default).pending = None;
            return replace_with(world, current, next);
        }
        return;
    }
    let pending = world.get_resource_or_insert_with(SceneBuild::default).pending.take();
    let Some(next) = polled.or(pending) else {
        return;
    };
    replace_with(world, current, next);
}

fn replace_with(world: &mut World, current: Arc<scenario::Scenario>, next: Arc<scenario::Scenario>) {
    let preserve_orbit = world.contains_resource::<editor::Editor>()
        && current.grid.rows == next.grid.rows
        && current.grid.cols == next.grid.cols
        && current.grid.cellsize == next.grid.cellsize;
    let orbit = preserve_orbit
        .then(|| {
            world
                .get_resource::<camera::Orbit>()
                .map(|o| camera::Orbit {
                    focus: o.focus,
                    yaw: o.yaw,
                    pitch: o.pitch,
                    dist: o.dist,
                    auto: false,
                })
        })
        .flatten();
    // Keep the window (including its egui context) and the atmosphere. All scene
    // roots and their children belong to this renderer and are reconstructed.
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, (Without<Parent>, Without<Window>, Without<sky::SkyDome>)>()
        .iter(world)
        .collect();
    for entity in roots {
        world.entity_mut(entity).despawn_recursive();
    }
    world.insert_resource(Scn(next));
    world.insert_resource(Snapshot::default());
    world.insert_resource(Frame::default());
    world.run_schedule(SceneSetup);
    if let Some(orbit) = orbit {
        world.insert_resource(orbit);
    }
}

/// Copy the newest view into the [`Snapshot`] whenever the model has changed
/// (during a resolution: after every simulated minute).
fn sync(source: Res<Source>, scn: Res<Scn>, mut snap: ResMut<Snapshot>) {
    let have = Have {
        generation: snap.generation,
        cue: snap.cue_seen,
    };
    let Some(v) = source.0.lock().ok().and_then(|mut s| s.poll(have)) else {
        return;
    };
    if !v.scenario_id.is_empty() && v.scenario_id != scn.0.id {
        return;
    }
    snap.generation = v.generation;
    snap.turn = v.turn;
    snap.phase = v.phase;
    snap.time_s = v.time_s;
    snap.clock = v.clock;
    snap.weather = v.weather;
    snap.ensemble = v.ensemble;
    snap.stats = v.stats;
    snap.cells = v.cells;
    snap.arrival = v.arrival;
    snap.intensity = v.intensity;
    snap.probability = v.probability;
    snap.buildings = v.buildings;
    snap.roads = v.roads;
    snap.road_segments = v.road_segments;
    snap.lines = v.lines;
    snap.drops = v.drops;
    snap.traffic = v.traffic;
    snap.deployments = v.deployments;
    snap.warned = v.warned;
    snap.last_turn = v.last_turn;
    snap.resolving = v.resolving;
    snap.clock_running = v.clock_running;
    snap.feed = v.feed;
    for cue in v.cues {
        snap.cue_seen = snap.cue_seen.max(cue.seq);
        snap.cues.push(cue);
    }
}

/// `P` saves a screenshot to `runs/`; so does writing a target path into
/// `runs/shot.req` (for unattended captures — the file is consumed).
fn screenshot(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut shots: ResMut<bevy::render::view::screenshot::ScreenshotManager>,
    mut tick: Local<u32>,
) {
    if std::env::var_os("TTX_SNAPSHOT").is_some() {
        return;
    }
    *tick += 1;
    let mut target = None;
    if keys.just_pressed(KeyCode::KeyP) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        target = Some(format!("runs/screenshot-{stamp}.png"));
    } else if (*tick).is_multiple_of(30) {
        if let Ok(path) = std::fs::read_to_string("runs/shot.req") {
            let _ = std::fs::remove_file("runs/shot.req");
            target = Some(path.trim().to_string());
        }
    }
    let (Some(path), Ok(window)) = (target, windows.get_single()) else {
        return;
    };
    let _ = std::fs::create_dir_all("runs");
    if let Err(e) = shots.save_screenshot_to_disk(window, &path) {
        eprintln!("screenshot failed: {e}");
    }
}

/// The recording captures every `RECORD_EVERY`-th rendered frame (about
/// 25 fps with shadows on): a fixed cadence, so frames are evenly spaced.
pub const RECORD_EVERY: u32 = 2;

/// The recording stream: raw frames piped into an ffmpeg child that encodes
/// on the fly (hardware H.264), so there is no pile of PNGs to encode later.
struct Recorder {
    tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    writer: Option<std::thread::JoinHandle<()>>,
    sent: u64,
    dropped: u64,
}

fn start_recorder(width: u32, height: u32, bgra: bool) -> std::io::Result<Recorder> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let _ = std::fs::create_dir_all("runs/rec");
    let size = format!("{width}x{height}");
    let mut child = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            if bgra { "bgra" } else { "rgba" },
            "-s",
            &size,
        ])
        // Real capture times as timestamps: a variable-rate stream that stays
        // in sync with the browser recording whatever the frame pacing.
        .args(["-use_wallclock_as_timestamps", "1", "-i", "-"])
        .args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "12M",
            "-pix_fmt",
            "yuv420p",
            "runs/rec/sim.mkv",
        ])
        .stdin(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("ffmpeg stdin");
    // Bounded: if the encoder falls behind, frames are dropped, never the render loop.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
    let writer = std::thread::spawn(move || {
        let mut first = true;
        for frame in rx {
            if first {
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let _ = std::fs::write("runs/rec/sim_start_ms.txt", ms.to_string());
                first = false;
            }
            if stdin.write_all(&frame).is_err() {
                break;
            }
        }
        drop(stdin);
        let _ = child.wait();
    });
    Ok(Recorder {
        tx: Some(tx),
        writer: Some(writer),
        sent: 0,
        dropped: 0,
    })
}

impl Drop for Recorder {
    fn drop(&mut self) {
        eprintln!(
            "recording: {} frames sent to the encoder, {} dropped",
            self.sent, self.dropped
        );
        self.tx.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// Whether recording is on, and the running recorder. The flag matters:
/// screenshots still in flight when recording stops must not start a new
/// recorder (which would truncate the finished file).
type RecSlot = std::sync::Arc<std::sync::Mutex<(bool, Option<Recorder>)>>;

/// While `runs/rec/ON` exists, stream every [`RECORD_EVERY`]-th frame into
/// `runs/rec/sim.mkv` (encoded while recording) and write the wall-clock time
/// of the first frame to `runs/rec/sim_start_ms.txt`. Removing the flag
/// closes the stream.
fn record(
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut shots: ResMut<bevy::render::view::screenshot::ScreenshotManager>,
    time: Res<Time>,
    mut state: Local<(u32, bool, f32)>,
    slot: Local<RecSlot>,
) {
    if std::env::var_os("TTX_SNAPSHOT").is_some() {
        return;
    }
    let (frame_no, on, check) = &mut *state;
    let t = time.elapsed_seconds();
    if t >= *check {
        let now_on = std::path::Path::new("runs/rec/ON").exists();
        if !*on && now_on {
            if let Ok(mut g) = slot.lock() {
                g.0 = true;
            }
        }
        if *on && !now_on {
            // Stop: dropping the recorder flushes and closes ffmpeg (off the render thread).
            let rec = slot.lock().ok().and_then(|mut g| {
                g.0 = false;
                g.1.take()
            });
            std::thread::spawn(move || drop(rec));
        }
        *on = now_on;
        *check = t + 0.25;
    }
    *frame_no = frame_no.wrapping_add(1);
    if !*on || !(*frame_no).is_multiple_of(RECORD_EVERY) {
        return;
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    let slot = slot.clone();
    let _ = shots.take_screenshot(window, move |img| {
        let Ok(mut guard) = slot.lock() else { return };
        if !guard.0 {
            return;
        }
        if guard.1.is_none() {
            let bgra = matches!(
                img.texture_descriptor.format,
                bevy::render::render_resource::TextureFormat::Bgra8Unorm
                    | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb
            );
            match start_recorder(img.width(), img.height(), bgra) {
                Ok(r) => guard.1 = Some(r),
                Err(e) => {
                    eprintln!("recording: cannot start ffmpeg: {e}");
                    return;
                }
            }
        }
        if let Some(r) = guard.1.as_mut() {
            match r.tx.as_ref().map(|tx| tx.try_send(img.data)) {
                Some(Ok(())) => r.sent += 1,
                _ => r.dropped += 1,
            }
        }
    });
}

#[cfg(test)]
mod territory_tests {
    use super::*;
    struct Replacement(Option<Arc<scenario::Scenario>>);
    impl ViewSource for Replacement {
        fn scenario(&mut self) -> Result<Arc<scenario::Scenario>, String> {
            Err("unused".into())
        }
        fn poll_scene(&mut self, _: &Arc<scenario::Scenario>) -> Option<Arc<scenario::Scenario>> {
            self.0.take()
        }
        fn poll(&mut self, _: Have) -> Option<view::ViewState> {
            None
        }
    }
    #[test]
    fn scene_switch_discards_old_geometry_and_state_but_keeps_window() {
        let old = Arc::new(scenario::synthetic::generate(7));
        let mut next = (*old).clone();
        next.id = "second-map".into();
        let mut world = World::new();
        world.insert_resource(Scn(old));
        world.insert_resource(Source(Mutex::new(Box::new(Replacement(Some(Arc::new(
            next,
        )))))));
        world.insert_resource(Snapshot {
            generation: 99,
            cells: vec![1; 4],
            ..default()
        });
        world.insert_resource(Frame::default());
        world.add_schedule(bevy::ecs::schedule::Schedule::new(SceneSetup));
        let window = world.spawn(Window::default()).id();
        let sky = world.spawn(sky::SkyDome).id();
        let root = world.spawn(SpatialBundle::default()).id();
        let child = world.spawn(SpatialBundle::default()).id();
        world.entity_mut(root).add_child(child);
        replace_scene(&mut world);
        assert_eq!(world.resource::<Scn>().0.id, "second-map");
        assert_eq!(world.resource::<Snapshot>().generation, 0);
        assert!(world.resource::<Snapshot>().cells.is_empty());
        assert!(world.get_entity(root).is_none());
        assert!(world.get_entity(child).is_none());
        assert!(world.get_entity(window).is_some());
        assert!(world.get_entity(sky).is_some());
    }
}
