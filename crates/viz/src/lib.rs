//! The big-screen renderer.
//!
//! Reads the shared [`Exercise`] (never writes it): each frame it `try_lock`s
//! the mutex and, when the exercise `generation` has moved, copies what it
//! draws into a [`Snapshot`] resource. A slow request on the server side
//! therefore never stalls a frame, and the renderer can never change the
//! exercise.
//!
//! Turns are simulated live: while the EXCON resolves a turn the server
//! advances the world one simulated minute at a time (optionally paced), and
//! every step bumps `generation`, so this renderer shows the fire, the
//! buildings and the counters as they change.
//!
//! Vegetation, landmarks and articulated unit bakes come from `propagator_abm`
//! under EUPL-1.2.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use exercise::Exercise;

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
mod objects;
mod quality;
mod sky;
mod sky_light;
mod snapshot;
mod terrain;

pub use frame::Frame;

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

/// Something worth looking at, derived from the exercise log (see `camera`).
#[derive(Clone, Debug)]
pub struct Cue {
    pub pos: scenario::Pos,
    /// Size of what to frame, metres.
    pub radius_m: f32,
    /// How long to hold the shot, seconds.
    pub hold_s: f32,
    pub label: String,
}

/// Everything the renderer draws, copied from the exercise.
#[derive(Resource, Default)]
pub struct Snapshot {
    pub deployments: Vec<(sim::DeploymentTask, u32)>,
    pub traffic: Vec<(scenario::Pos, u32, bool)>,
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
    /// Ensemble-mean fireline intensity (kW/m) and fraction of runs reached, per cell.
    pub intensity: Vec<f32>,
    pub probability: Vec<f32>,
    pub buildings: Vec<BuildingDraw>,
    pub roads: Vec<(bool, bool)>,
    pub road_segments: Vec<(usize, scenario::Pos, scenario::Pos, bool, bool)>,
    pub lines: Vec<(scenario::Pos, scenario::Pos, f32)>,
    pub drops: Vec<(scenario::Pos, f32)>,
    pub warned: Vec<String>,
    pub last_turn: Option<exercise::TurnWindow>,
    /// `(turn, from_s, to_s)` while a turn is being simulated.
    pub resolving: Option<(u32, i64, i64)>,
    pub feed: Vec<String>,
    pub llm_model: String,
    pub ensemble: Option<fire::FireOptions>,
    /// Camera cues, append-only.
    pub cues: Vec<Cue>,
    log_seen: u64,
}

/// The scenario, shared by all systems.
#[derive(Resource, Clone)]
pub struct Scn(pub Arc<scenario::Scenario>);

/// Open the window and run until it is closed.
pub fn run(ex: Arc<Mutex<Exercise>>, port: u16, llm_model: String) {
    let scn = ex
        .lock()
        .map(|e| e.world.scn.clone())
        .expect("exercise lock");
    let quality = quality::Quality::from_env();
    let capture = std::env::var("TTX_SNAPSHOT").is_ok();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
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
                    }),
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
        .insert_resource(if quality.name == "low" {
            Msaa::Off
        } else {
            Msaa::Sample4
        })
        .insert_resource(bevy::pbr::DirectionalLightShadowMap {
            size: if quality.name == "high" { 4096 } else { 2048 },
        })
        .insert_resource(quality)
        .insert_resource(ClearColor(camera::SKY))
        // Sky-tinted fill light; the sun does the modelling.
        .insert_resource(AmbientLight {
            color: Color::srgb(0.70, 0.78, 0.95),
            brightness: 140.0,
        })
        .insert_resource(Shared(ex))
        .insert_resource(ServerPort(port))
        .insert_resource(Scn(scn))
        .insert_resource(Snapshot {
            llm_model,
            ..default()
        })
        .init_resource::<fire_field::FirePresentation>()
        .init_resource::<Frame>()
        .add_systems(
            Startup,
            (
                camera::setup,
                terrain::setup,
                objects::setup,
                deployments::setup,
            ),
        )
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
                deployments::update.after(frame::update),
                camera::control.after(frame::update),
                camera::fire_lights.after(frame::update),
                hud::draw.after(frame::update).after(fire_field::toggle),
                screenshot,
                record,
                snapshot::hold.after(camera::control).after(sync),
                snapshot::capture.after(snapshot::hold),
                terrain::lod,
            ),
        )
        .run();
}

/// Copy the exercise into the [`Snapshot`] whenever it changed (during a
/// resolution: after every simulated minute).
fn sync(shared: Res<Shared>, mut snap: ResMut<Snapshot>) {
    let Ok(ex) = shared.0.try_lock() else { return };
    if ex.generation == snap.generation {
        return;
    }
    let w = &ex.world;
    snap.traffic =
        w.d.traffic
            .vehicles
            .iter()
            .filter(|v| v.departed && !v.completed)
            .map(|v| (w.traffic_position(v, w.time_s()), v.people, v.at_risk))
            .collect();
    snap.deployments =
        w.d.tasks
            .iter()
            .map(|task| (task.clone(), w.task_active_qty(task)))
            .collect();
    snap.generation = ex.generation;
    snap.turn = ex.turn;
    snap.phase = format!("{:?}", ex.phase).to_lowercase();
    snap.time_s = w.time_s();
    snap.clock = w.clock();
    snap.weather = Some(w.fire.weather());
    snap.ensemble = Some(w.fire.options());
    snap.stats = Some(w.stats());
    let cleared = w.fire.cleared();
    let wet = w.fire.added_moisture();
    let prob = w.fire.probability();
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
            fire::CellFire::Unburnt if prob[i] > 0.0 => 5,
            fire::CellFire::Unburnt => 0,
        })
        .collect();
    snap.arrival = w.fire.arrival_times().to_vec();
    snap.intensity = w.fire.intensity().to_vec();
    snap.probability = w.fire.probability().to_vec();
    snap.buildings =
        w.d.buildings
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
    snap.roads = w
        .scn
        .roads
        .iter()
        .map(|r| {
            w.d.roads
                .get(&r.id)
                .map(|s| (s.closed, s.cut_by_fire))
                .unwrap_or_default()
        })
        .collect();
    snap.road_segments = w
        .network
        .segments()
        .into_iter()
        .filter_map(|segment| {
            let i = w.scn.roads.iter().position(|r| r.id == segment.road)?;
            let st = w.d.roads.get(&segment.road)?;
            let cut = st
                .fire_closed_links
                .as_ref()
                .map_or(st.cut_by_fire, |links| {
                    segment.links.iter().any(|id| links.contains(id))
                });
            Some((i, segment.from, segment.to, st.closed, cut))
        })
        .collect();
    snap.lines =
        w.d.lines
            .iter()
            .map(|l| (l.from, l.to, l.width_m))
            .collect();
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
    snap.last_turn = ex.last_turn;
    // Camera cues from new log entries.
    let first = snap.log_seen;
    for e in ex.log.iter().filter(|e| e.seq > first) {
        if let Some(c) = cue_for(&ex, e) {
            snap.cues.push(c);
        }
        snap.log_seen = e.seq;
    }
    snap.resolving = ex.resolving.as_ref().map(|r| (r.turn, r.from_s, r.to_s));
}

/// Where an order acts, and how big an area it covers.
fn command_place(w: &sim::World, cmd: &sim::Command) -> Option<(scenario::Pos, f32)> {
    use sim::{Area, Command as C};
    let zone = |z: &str| {
        w.zone_code(z)
            .and_then(|c| w.scn.zone_centre(&c))
            .map(|p| (p, 300.0))
    };
    let building = |b: &str| w.find_building(b).map(|i| (w.scn.buildings[i].pos, 160.0));
    match cmd {
        C::CutFireLine {
            from, to, along, ..
        } => {
            let (a, b) = match along {
                Some(code) => w.scn.zone_edge(code)?,
                None => (
                    w.resolve_loc(from.as_ref()?).ok()?,
                    w.resolve_loc(to.as_ref()?).ok()?,
                ),
            };
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            Some(([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], len / 2.0 + 80.0))
        }
        C::DeployEngine { at } | C::WaterDrop { at } | C::Ignite { at, .. } => {
            w.resolve_loc(at).ok().map(|p| (p, 180.0))
        }
        C::EvacuateZone { zone: z, .. } | C::IssueWarning { zone: z } => zone(z),
        C::EvacuateBuilding { building: b, .. } | C::OpenShelter { building: b } => building(b),
        C::SetBuildingStatus { building: b, .. } => building(b).or_else(|| zone(b)),
        C::CloseRoad { road } | C::ReopenRoad { road } => w
            .find_road(road)
            .and_then(|id| w.scn.road(&id))
            .and_then(|r| r.points.get(r.points.len() / 2).copied())
            .map(|p| (p, 350.0)),
        C::Extinguish { area } => match area {
            Area::Named(n) => zone(n).or_else(|| w.scn.resolve_place(n).map(|p| (p, 250.0))),
            Area::Circle { at, radius_m } => w.resolve_loc(at).ok().map(|p| (p, *radius_m)),
        },
        _ => None,
    }
}

/// The first place a text names: a facility, a gazetteer place (or alias), or a zone code.
fn named_place(w: &sim::World, text: &str) -> Option<(scenario::Pos, f32)> {
    let lower = text.to_lowercase();
    let mut best: Option<(usize, scenario::Pos, f32)> = None;
    let mut consider = |name: &str, pos: scenario::Pos, r: f32| {
        if name.len() < 3 {
            return;
        }
        if let Some(i) = lower.find(&name.to_lowercase()) {
            if best.is_none_or(|(j, _, _)| i < j) {
                best = Some((i, pos, r));
            }
        }
    };
    for b in &w.scn.buildings {
        if let Some(n) = &b.name {
            consider(n, b.pos, 170.0);
        }
    }
    for p in &w.scn.places {
        consider(&p.name, p.pos, 300.0);
        for a in &p.aliases {
            consider(a, p.pos, 300.0);
        }
    }
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        if token.len() == 2 {
            if let (Some(c), Some(pos)) = (w.zone_code(token), text.find(token)) {
                if best.is_none_or(|(j, _, _)| pos < j) {
                    best = w.scn.zone_centre(&c).map(|p| (pos, p, 300.0));
                }
            }
        }
    }
    best.map(|(_, p, r)| (p, r))
}

fn cue_for(ex: &Exercise, e: &exercise::LogEntry) -> Option<Cue> {
    let w = &ex.world;
    let d = &e.data;
    let cmds_cue = |cmds: &[sim::Command], label: String, hold: f32| {
        cmds.iter()
            .find_map(|c| command_place(w, c))
            .map(|(pos, radius_m)| Cue {
                pos,
                radius_m,
                hold_s: hold,
                label,
            })
    };
    match e.kind.as_str() {
        "inject_sent" => {
            let title = d["title"].as_str().unwrap_or_default();
            let text = ex
                .messages
                .iter()
                .rev()
                .find(|m| m.title.as_deref() == Some(title))
                .map(|m| m.text_en.clone())
                .unwrap_or_default();
            let (pos, radius_m) = named_place(w, &format!("{title} {text}"))?;
            Some(Cue {
                pos,
                radius_m,
                hold_s: 6.0,
                label: format!("inject: {title}"),
            })
        }
        "proposal_parsed" => {
            let cmds: Vec<sim::Command> = serde_json::from_value(d["commands"].clone()).ok()?;
            cmds_cue(&cmds, format!("proposal from {}", e.actor), 5.0)
        }
        "proposal_decided" if d["approve"].as_bool() == Some(true) => {
            let p = ex.proposal(d["proposal"].as_u64()?)?;
            cmds_cue(&p.commands, "approved".into(), 5.0)
        }
        "master_command" => {
            let c: sim::Command = serde_json::from_value(d["command"].clone()).ok()?;
            cmds_cue(std::slice::from_ref(&c), c.describe(), 6.0)
        }
        _ => None,
    }
}

/// What the audience sees in the HUD feed.
fn feed_line(e: &exercise::LogEntry) -> Option<String> {
    let d = &e.data;
    let s = match e.kind.as_str() {
        "inject_sent" => format!(
            "INJECT  {} -> {}",
            d["title"].as_str()?,
            d["recipients"]
                .as_array()?
                .iter()
                .filter_map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "proposal_decided" if d["approve"].as_bool()? => format!(
            "APPROVED  {}",
            d["facts"]
                .as_str()?
                .lines()
                .next()?
                .trim_start_matches("APPROVED: ")
        ),
        "proposal_decided" => "REJECTED  a proposal".to_string(),
        "master_command" => format!(
            "EXCON  {}",
            serde_json::from_value::<sim::Command>(d["command"].clone())
                .ok()?
                .describe()
        ),
        "turn_resolving" => format!("TURN {} SIMULATING  ({} min)", d["turn"], d["minutes"]),
        "turn_resolved" => format!("TURN {} RESOLVED  ({} min)", d["turn"], d["minutes"]),
        // The English summary, not the raw reply: the HUD font has no Armenian, Georgian… glyphs.
        "proposal_parsed" if e.actor != exercise::MASTER => format!(
            "ORDERS  {}: {}",
            e.actor,
            truncate(d["summary"].as_str().filter(|s| !s.is_empty())?, 90)
        ),
        "rollback" => format!("ROLLBACK to turn {}", d["to_turn"]),
        _ => return None,
    };
    Some(format!(
        "{}  {}",
        &e.clock[e.clock.len().saturating_sub(5)..],
        s
    ))
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
