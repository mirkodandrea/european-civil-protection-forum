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
    let mut r = bevy::window::WindowResolution::new(w as f32, h as f32);
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
    mut exit: EventWriter<bevy::app::AppExit>,
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
    let (Some(mut o), Ok(mut tf)) = (orbit, cams.get_single_mut()) else {
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
            .find(|(t, _)| t.id == id)
            .map(|(t, _)| t.position(snap.time_s))
    } else {
        at.split_once(':')
            .and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))
    };
    if point.is_none() && at != "home" && at != "fire" {
        eprintln!("snapshot target failed: unknown location {at}");
        exit.send(bevy::app::AppExit::error());
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
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut shots: ResMut<bevy::render::view::screenshot::ScreenshotManager>,
    costs: Option<Res<crate::bench::Costs>>,
    mut p: Local<Progress>,
    mut exit: EventWriter<bevy::app::AppExit>,
) {
    let Ok(path) = std::env::var("TTX_SNAPSHOT") else {
        return;
    };
    if let Some(done) = &p.done {
        match done.load(Ordering::Acquire) {
            1 => {
                exit.send(bevy::app::AppExit::Success);
            }
            2 => {
                exit.send(bevy::app::AppExit::error());
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
    if time.elapsed_seconds() < settle {
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
            exit.send(bevy::app::AppExit::error());
            return;
        }
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    let done = Arc::new(AtomicU8::new(0));
    p.done = Some(done.clone());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("snapshot directory failed: {e}");
            exit.send(bevy::app::AppExit::error());
            return;
        }
    }
    if let Err(e) = shots.take_screenshot(window, move |img| {
        let result = img
            .try_into_dynamic()
            .map_err(|e| format!("{e:?}"))
            .and_then(|img| img.save(&path).map_err(|e| e.to_string()));
        if let Err(e) = &result {
            eprintln!("snapshot failed: {e}");
        }
        done.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
    }) {
        eprintln!("snapshot request failed: {e}");
        exit.send(bevy::app::AppExit::error());
    }
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
