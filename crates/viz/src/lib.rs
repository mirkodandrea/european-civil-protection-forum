//! The big-screen renderer.
//!
//! Reads the shared [`Exercise`] (never writes it): each frame it `try_lock`s
//! the mutex and, when the exercise `generation` has moved, copies what it
//! draws into a [`Snapshot`] resource. A slow request on the server side
//! therefore never stalls a frame, and the renderer can never change the
//! exercise.
//!
//! When a turn is resolved the renderer **replays** it: a display clock runs
//! from the turn's start to its end over [`REPLAY_SECONDS`], revealing fire
//! cells by their arrival time, so the audience watches the half hour that
//! just passed.
//!
//! Vegetation meshes (pine, oak, bush) are the Blender bakes from
//! `propagator_abm` (`assets/models/meshes.json`).

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use exercise::Exercise;

mod camera;
mod frame;
mod hud;
mod models;
mod objects;
mod terrain;

pub use frame::Frame;

/// Wall-clock seconds a resolved turn takes to replay on screen.
pub const REPLAY_SECONDS: f32 = 10.0;

#[derive(Resource, Clone)]
pub struct Shared(pub Arc<Mutex<Exercise>>);

#[derive(Resource)]
pub struct ServerPort(pub u16);

#[derive(Clone, Default)]
pub struct BuildingDraw {
    /// 0 intact, 1 damaged, 2 destroyed
    pub damage: u8,
    pub threatened: bool,
    pub evacuating: bool,
    pub empty_evacuated: bool,
}

/// Everything the renderer draws, copied from the exercise.
#[derive(Resource, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub turn: u32,
    pub phase: String,
    pub time_s: i64,
    pub clock: String,
    pub weather: Option<scenario::WeatherSpec>,
    pub stats: Option<sim::Stats>,
    /// Final per-cell code (see `sim::digest::cell_code`).
    pub cells: Vec<u8>,
    pub arrival: Vec<i32>,
    pub buildings: Vec<BuildingDraw>,
    pub roads: Vec<(bool, bool)>,
    pub lines: Vec<(scenario::Pos, scenario::Pos, f32)>,
    pub drops: Vec<(scenario::Pos, f32)>,
    pub warned: Vec<String>,
    pub last_turn: Option<exercise::TurnWindow>,
    pub feed: Vec<String>,
    pub llm_model: String,
}

/// The turn replay: which simulated second the screen currently shows.
#[derive(Resource, Default)]
pub struct Replay {
    pub turn: Option<u32>,
    pub from_s: i64,
    pub to_s: i64,
    pub elapsed: f32,
}

impl Replay {
    pub fn display_time(&self, now_s: i64) -> i64 {
        if self.turn.is_none() || self.elapsed >= REPLAY_SECONDS {
            return now_s;
        }
        let f = (self.elapsed / REPLAY_SECONDS).clamp(0.0, 1.0) as f64;
        self.from_s + ((self.to_s - self.from_s) as f64 * f) as i64
    }
    pub fn active(&self) -> bool {
        self.turn.is_some() && self.elapsed < REPLAY_SECONDS
    }
}

/// The scenario, shared by all systems.
#[derive(Resource, Clone)]
pub struct Scn(pub Arc<scenario::Scenario>);

/// Open the window and run until it is closed.
pub fn run(ex: Arc<Mutex<Exercise>>, port: u16, llm_model: String) {
    let scn = ex.lock().map(|e| e.world.scn.clone()).expect("exercise lock");
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "PPRD East TTX — simulator".into(),
                        resolution: (1600., 900.).into(),
                        ..default()
                    }),
                    ..default()
                })
                // The app installs its own tracing subscriber.
                .disable::<bevy::log::LogPlugin>(),
        )
        .add_plugins(EguiPlugin)
        .insert_resource(ClearColor(Color::srgb(0.62, 0.74, 0.86)))
        .insert_resource(AmbientLight { color: Color::WHITE, brightness: 350.0 })
        .insert_resource(Shared(ex))
        .insert_resource(ServerPort(port))
        .insert_resource(Scn(scn))
        .insert_resource(Snapshot { llm_model, ..default() })
        .init_resource::<Replay>()
        .init_resource::<Frame>()
        .add_systems(Startup, (camera::setup, terrain::setup, objects::setup))
        .add_systems(
            Update,
            (
                sync,
                frame::update.after(sync),
                terrain::update.after(frame::update),
                objects::update.after(frame::update),
                camera::control,
                hud::draw.after(frame::update),
                screenshot,
                record,
            ),
        )
        .run();
}

/// Copy the exercise into the [`Snapshot`] when it changed; start a replay
/// when a new turn has been resolved.
fn sync(shared: Res<Shared>, mut snap: ResMut<Snapshot>, mut replay: ResMut<Replay>, time: Res<Time>) {
    replay.elapsed += time.delta_seconds();
    let Ok(ex) = shared.0.try_lock() else { return };
    if ex.generation == snap.generation {
        return;
    }
    let w = &ex.world;
    snap.generation = ex.generation;
    snap.turn = ex.turn;
    snap.phase = format!("{:?}", ex.phase).to_lowercase();
    snap.time_s = w.time_s();
    snap.clock = w.clock();
    snap.weather = Some(w.fire.weather());
    snap.stats = Some(w.stats());
    let cleared = w.fire.cleared();
    let wet = w.fire.added_moisture();
    snap.cells = w
        .fire
        .state()
        .iter()
        .enumerate()
        .map(|(i, s)| match s {
            fire::CellFire::Burning => 1,
            fire::CellFire::Burnt => 2,
            fire::CellFire::Unburnt if cleared[i] => 3,
            fire::CellFire::Unburnt if wet[i] > 10.0 => 4,
            fire::CellFire::Unburnt => 0,
        })
        .collect();
    snap.arrival = w.fire.arrival_times().to_vec();
    snap.buildings = w
        .d
        .buildings
        .iter()
        .map(|b| BuildingDraw {
            damage: match b.damage {
                sim::Damage::Intact => 0,
                sim::Damage::Damaged => 1,
                sim::Damage::Destroyed => 2,
            },
            threatened: b.threatened,
            evacuating: b.evac_done_at.is_some(),
            empty_evacuated: b.people_inside == 0 && b.evacuated > 0,
        })
        .collect();
    snap.roads = w.scn.roads.iter().map(|r| w.d.roads.get(&r.id).map(|s| (s.closed, s.cut_by_fire)).unwrap_or_default()).collect();
    snap.lines = w.d.lines.iter().map(|l| (l.from, l.to, l.width_m)).collect();
    snap.drops = w.d.drops.iter().map(|d| (d.at, d.radius_m)).collect();
    snap.warned = w.d.warned.iter().cloned().collect();
    snap.feed = ex
        .log
        .iter()
        .rev()
        .filter_map(feed_line)
        .take(9)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if ex.last_turn.map(|t| t.turn) != snap.last_turn.map(|t| t.turn) {
        if let Some(t) = ex.last_turn {
            *replay = Replay { turn: Some(t.turn), from_s: t.from_s, to_s: t.to_s, elapsed: 0.0 };
        }
    }
    snap.last_turn = ex.last_turn;
}

/// What the audience sees in the HUD feed.
fn feed_line(e: &exercise::LogEntry) -> Option<String> {
    let d = &e.data;
    let s = match e.kind.as_str() {
        "inject_sent" => format!("INJECT  {} -> {}", d["title"].as_str()?, d["recipients"].as_array()?.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>().join(", ")),
        "proposal_decided" if d["approve"].as_bool()? => format!("APPROVED  {}", d["facts"].as_str()?.lines().next()?.trim_start_matches("APPROVED: ")),
        "proposal_decided" => "REJECTED  a proposal".to_string(),
        "master_command" => format!("DIRECTOR  {}", serde_json::from_value::<sim::Command>(d["command"].clone()).ok()?.describe()),
        "turn_resolved" => format!("TURN {} RESOLVED  ({} min)", d["turn"], d["minutes"]),
        // The English summary, not the raw reply: the HUD font has no Armenian, Georgian… glyphs.
        "proposal_parsed" if e.actor != exercise::MASTER => format!("ORDERS  {}: {}", e.actor, truncate(d["summary"].as_str().filter(|s| !s.is_empty())?, 90)),
        "rollback" => format!("ROLLBACK to turn {}", d["to_turn"]),
        _ => return None,
    };
    Some(format!("{}  {}", &e.clock[e.clock.len().saturating_sub(5)..], s))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
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
    *tick += 1;
    let mut target = None;
    if keys.just_pressed(KeyCode::KeyP) {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        target = Some(format!("runs/screenshot-{stamp}.png"));
    } else if (*tick).is_multiple_of(30) {
        if let Ok(path) = std::fs::read_to_string("runs/shot.req") {
            let _ = std::fs::remove_file("runs/shot.req");
            target = Some(path.trim().to_string());
        }
    }
    let (Some(path), Ok(window)) = (target, windows.get_single()) else { return };
    let _ = std::fs::create_dir_all("runs");
    if let Err(e) = shots.save_screenshot_to_disk(window, &path) {
        eprintln!("screenshot failed: {e}");
    }
}

/// While `runs/rec/ON` exists, save a frame every 100 ms as
/// `runs/rec/<unix-ms>.png` (timestamped, so a variable-rate video can be
/// assembled and synchronised with other recordings).
fn record(
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut shots: ResMut<bevy::render::view::screenshot::ScreenshotManager>,
    time: Res<Time>,
    mut state: Local<(f32, bool, f32)>,
) {
    let (next, on, check) = &mut *state;
    let t = time.elapsed_seconds();
    if t >= *check {
        *on = std::path::Path::new("runs/rec/ON").exists();
        *check = t + 0.5;
    }
    if !*on || t < *next {
        return;
    }
    *next = t + 0.1;
    let Ok(window) = windows.get_single() else { return };
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let _ = shots.save_screenshot_to_disk(window, format!("runs/rec/{ms}.png"));
}
