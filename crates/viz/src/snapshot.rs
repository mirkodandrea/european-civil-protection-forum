//! Invisible, reproducible captures adapted from the source kiosk/snap.rs.
//! Batch specifications are driven by scripts/snapshot.py through real TTX
//! approvals. Each state gets a fresh process; PNG callbacks complete before
//! exit. Optional benchmarks freeze animation after settling and measure actual
//! wall intervals and source CPU/render/GPU probes, never the fixed sim timestep.
use bevy::prelude::*;
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};

pub struct SnapshotPlugin;
impl Plugin for SnapshotPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("TTX_SNAPSHOT").is_some() && bench_s().is_some() {
            app.add_plugins(crate::bench::BenchPlugin);
        }
        if std::env::var_os("TTX_SNAPSHOT").is_some() {
            app.add_systems(PostUpdate, redirect_camera);
            #[cfg(not(target_arch = "wasm32"))]
            app.add_plugins(SnapPacingPlugin);
        }
    }
}
/// Since Bevy 0.20 the screenshot of a hidden window comes out black (seen on
/// macOS, propagator_abm 2026-10-21): with `TTX_SNAPSHOT` the camera draws
/// into an image of the window's size instead, and that image is saved.
#[derive(Resource)]
pub struct SnapTarget(pub Handle<Image>);

fn redirect_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cams: Query<Entity, Added<Camera3d>>,
) {
    let Ok(w) = windows.single() else { return };
    for e in &cams {
        let image = Image::new_target_texture(
            w.physical_width(),
            w.physical_height(),
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            None,
        );
        let handle = images.add(image);
        commands.entity(e).insert(bevy::camera::RenderTarget::Image(
            bevy::camera::ImageRenderTarget {
                handle: handle.clone(),
                scale_factor: w.scale_factor(),
            },
        ));
        commands.insert_resource(SnapTarget(handle));
    }
}

/// Frames the CPU may queue ahead of the GPU, as with a swap chain.
#[cfg(not(target_arch = "wasm32"))]
const FRAMES_IN_FLIGHT: usize = 2;

/// The frame pacing a window would give. Drawing into an image there is no
/// swap-chain texture to wait for, so the CPU runs ahead of the GPU until
/// Metal's command-buffer budget is full (seconds of latency, frame times
/// 20–30 % worse: measured in propagator_abm). Each frame waits, before its
/// views are prepared, for the frame `FRAMES_IN_FLIGHT` back.
#[cfg(not(target_arch = "wasm32"))]
struct SnapPacingPlugin;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default)]
struct InFlight(std::collections::VecDeque<wgpu::SubmissionIndex>);

#[cfg(not(target_arch = "wasm32"))]
impl Plugin for SnapPacingPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        use bevy::render::renderer::{render_system, RenderDevice, RenderQueue};
        use bevy::render::{Render, RenderApp, RenderSystems};
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render
            .init_resource::<InFlight>()
            .add_systems(Render, (|mut f: ResMut<InFlight>, q: Res<RenderQueue>| {
                f.0.push_back(q.submit(std::iter::empty()));
            }).in_set(RenderSystems::Render).after(render_system))
            .add_systems(Render, (|mut f: ResMut<InFlight>, d: Res<RenderDevice>| {
                while f.0.len() > FRAMES_IN_FLIGHT {
                    let i = f.0.pop_front().expect("not empty");
                    let _ = d.poll(wgpu::PollType::Wait { submission_index: Some(i), timeout: None });
                }
            }).in_set(RenderSystems::CreateViews));
    }
}

pub fn bench_s() -> Option<f32> {
    std::env::var("TTX_SNAPSHOT_BENCH")
        .ok()?
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.)
}
pub fn resolution() -> bevy::window::WindowResolution {
    let (w, h) = std::env::var("TTX_SIZE")
        .ok()
        .and_then(|v| {
            v.split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        })
        .filter(|(w, h)| (160..=8192).contains(w) && (160..=8192).contains(h))
        .unwrap_or((1600, 900));
    let mut r = bevy::window::WindowResolution::new(w, h);
    if let Some(scale) = std::env::var("TTX_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|s| s.is_finite() && *s > 0. && *s <= 4.)
    {
        r = r.with_scale_factor_override(scale);
    }
    r
}
/// Source-style exact camera hold, applied after manual/director updates.
/// Angles in the batch interface are degrees; legacy TTX_CAM stays in radians.
pub fn hold(
    scn: Res<crate::Scn>,
    snap: Res<crate::Snapshot>,
    orbit: Option<ResMut<crate::camera::Orbit>>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    if std::env::var_os("TTX_SNAPSHOT").is_none() {
        return;
    }
    let Ok(at) = std::env::var("TTX_SNAPSHOT_AT") else {
        return;
    };
    if snap.cells.len() != scn.0.grid.len() {
        return;
    }
    let (Some(mut o), Ok(mut tf)) = (orbit, cams.single_mut()) else {
        return;
    };
    let point = if at == "home" {
        None
    } else if at == "fire" {
        crate::camera::fire_extent(&scn.0, &snap.cells).map(|(p, _)| p)
    } else if let Some(name) = at.strip_prefix("place:") {
        scn.0.resolve_place(name)
    } else if let Some(id) = at.strip_prefix("task:").and_then(|s| s.parse::<u64>().ok()) {
        snap.deployments
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.position(snap.time_s))
    } else {
        at.split_once(':')
            .and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))
    };
    if point.is_none() && at != "home" && at != "fire" {
        eprintln!("snapshot target failed: unknown location {at}");
        exit.write(bevy::app::AppExit::error());
        return;
    }
    if let Some(p) = point {
        o.focus = crate::frame::to_bevy(&scn.0, p, 0.);
    }
    let number = |key: &str| {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
    };
    if let Some(d) = number("TTX_SNAPSHOT_DISTANCE") {
        o.dist = d.max(20.);
    }
    if let Some(y) = number("TTX_SNAPSHOT_YAW") {
        o.yaw = y.to_radians();
    }
    if let Some(p) = number("TTX_SNAPSHOT_PITCH") {
        o.pitch = p.abs().to_radians().clamp(0.05, 1.553);
    }
    o.auto = false;
    *tf = crate::camera::place(&o);
    let ground = crate::frame::height_at(&scn.0, crate::frame::to_world(&scn.0, tf.translation))
        * crate::frame::EXAGGERATION;
    tf.translation.y = tf.translation.y.max(ground + 4.);
    tf.look_at(o.focus, Vec3::Y);
}
#[derive(Default)]
pub struct Progress {
    done: Option<Arc<AtomicU8>>,
    start: Option<Instant>,
    last: Option<Instant>,
    frames: Vec<f32>,
}
fn stats(values: &[f32]) -> serde_json::Value {
    let mut v: Vec<f32> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.)
        .collect();
    if v.is_empty() {
        return serde_json::Value::Null;
    }
    v.sort_by(f32::total_cmp);
    serde_json::json!({"samples":v.len(),"mean_ms":v.iter().sum::<f32>()/v.len() as f32,
        "p95_ms":v[(v.len()*95/100).min(v.len()-1)],"max_ms":v[v.len()-1]})
}
pub fn capture(
    time: Res<Time>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut commands: Commands,
    target: Option<Res<SnapTarget>>,
    costs: Option<Res<crate::bench::Costs>>,
    mut p: Local<Progress>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    let Ok(path) = std::env::var("TTX_SNAPSHOT") else {
        return;
    };
    if let Some(done) = &p.done {
        match done.load(Ordering::Acquire) {
            1 => {
                exit.write(bevy::app::AppExit::Success);
            }
            2 => {
                exit.write(bevy::app::AppExit::error());
            }
            _ => {}
        }
        return;
    }
    let settle = std::env::var("TTX_SNAPSHOT_SETTLE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|s| s.is_finite() && *s >= 0.)
        .unwrap_or(3.);
    if time.elapsed_secs() < settle {
        return;
    }
    if let Some(seconds) = bench_s() {
        let now = Instant::now();
        if p.start.is_none() {
            p.start = Some(now);
            p.last = Some(now);
            virtual_time.pause();
            if let Some(c) = &costs {
                for v in [&c.cpu, &c.gpu_pass, &c.encode, &c.render] {
                    v.lock().unwrap().clear();
                }
            }
            return;
        }
        if let Some(last) = p.last.replace(now) {
            p.frames
                .push(now.duration_since(last).as_secs_f32() * 1000.);
        }
        if p.start.unwrap().elapsed().as_secs_f32() < seconds {
            return;
        }
        let frames = stats(&p.frames);
        let mut report = serde_json::json!({"frame":frames,"animation_frozen":true,"vsync_requested_off":true,
            "note":"Hidden-window FPS can be compositor limited; GPU timestamps may be unavailable or clock limited."});
        report["fps_mean"] = report["frame"]["mean_ms"]
            .as_f64()
            .filter(|v| *v > 0.)
            .map(|v| serde_json::json!(1000. / v))
            .unwrap_or(serde_json::Value::Null);
        if let Some(c) = &costs {
            for (key, v) in [
                ("main", &c.cpu),
                ("gpu", &c.gpu_pass),
                ("encode", &c.encode),
                ("render_without_present_wait", &c.render),
            ] {
                report[key] = stats(&v.lock().unwrap());
            }
        }
        eprintln!("bench: {report}");
        let output = std::path::Path::new(&path).with_extension("bench.json");
        if let Some(parent) = output.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()) {
            eprintln!("snapshot benchmark failed: {e}");
            exit.write(bevy::app::AppExit::error());
            return;
        }
    }
    let Some(target) = target else {
        return;
    };
    let done = Arc::new(AtomicU8::new(0));
    p.done = Some(done.clone());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("snapshot directory failed: {e}");
            exit.write(bevy::app::AppExit::error());
            return;
        }
    }
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    commands
        .spawn(Screenshot::image(target.0.clone()))
        .observe(move |shot: On<ScreenshotCaptured>| {
        let result = shot
            .image
            .clone()
            .try_into_dynamic()
            .map_err(|e| format!("{e:?}"))
            .and_then(|img| img.save(&path).map_err(|e| e.to_string()));
        if let Err(e) = &result {
            eprintln!("snapshot failed: {e}");
        }
        done.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
    });
}
#[cfg(test)]
mod tests {
    #[test]
    fn absent_metrics_are_null_and_percentiles_ignore_invalid_samples() {
        assert!(super::stats(&[]).is_null());
        let s = super::stats(&[1., 2., 3., f32::NAN]);
        assert_eq!(s["samples"], 3);
        assert_eq!(s["mean_ms"], 2.);
        assert_eq!(s["p95_ms"], 3.);
    }
}
