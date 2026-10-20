//! Camera, lighting and the cinematic "director".
//!
//! Manual control: left-drag rotate, right-drag pan, scroll zoom, `F` focus on
//! the fire, `R` reset, `A` toggle the director.
//!
//! **The director** (on with `TTX_ORBIT=1` / `TTX_CINEMATIC=1`, or `A`) frames
//! shots from what happens in the exercise ([`crate::Snapshot::cues`]):
//! an inject that names a place, a player's proposal, an approved order or a
//! Director command each get a short shot of their location (a slow push-in
//! with a little drift). While a turn is being simulated it follows the fire:
//! the camera tracks the burning front and widens as the fire grows, orbiting
//! slowly. Between events it holds a wide establishing orbit. Every parameter
//! is eased, so cuts are glides; any manual input hands control back to the
//! user for 15 s.
//!
//! **Lighting**: a clock-driven sun with cascaded shadows, image-based sky
//! light, distance fog in the sky colour, HDR with bloom (the additive flames
//! glow), and a pool of flickering point lights placed on the hottest parts
//! of the fire so it lights the terrain around it.

use bevy::core_pipeline::bloom::BloomSettings;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::pbr::{CascadeShadowConfigBuilder, FogFalloff, FogSettings};
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::frame::{to_bevy, Frame};
use crate::{Cue, Scn, Snapshot};

pub const SKY: Color = Color::srgb(0.66, 0.76, 0.86);
const FIRE_LIGHTS: usize = 8;

#[derive(Resource)]
pub struct Orbit {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub auto: bool,
}

/// A framing the camera eases towards.
#[derive(Clone, Copy, Debug)]
struct Shot {
    focus: Vec3,
    dist: f32,
    pitch: f32,
    /// Yaw drift, rad/s.
    drift: f32,
}

#[derive(Resource, Default)]
pub struct Director {
    queue: Vec<(Cue, f32)>,
    current: Option<(Shot, f32, f32)>, // shot, started at, duration
    manual_until: f32,
    seen_cues: usize,
    fire_focus: Option<Vec3>,
    fire_dist: f32,
    /// Spring velocities of focus, distance and pitch (critically damped smoothing).
    v_focus: Vec3,
    v_dist: f32,
    v_pitch: f32,
    /// Extra pitch when terrain would block the view (raised quickly, released slowly).
    pitch_boost: f32,
}

/// Critically damped spring towards `target` (Unity-style SmoothDamp):
/// continuous velocity, no overshoot, so a jumping target never jerks the camera.
fn smooth_damp(current: f32, target: f32, vel: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let omega = 2.0 / smooth_time.max(1e-3);
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    target + (change + temp) * exp
}

fn smooth_damp3(current: Vec3, target: Vec3, vel: &mut Vec3, smooth_time: f32, dt: f32) -> Vec3 {
    let (mut vx, mut vy, mut vz) = (vel.x, vel.y, vel.z);
    let out = Vec3::new(
        smooth_damp(current.x, target.x, &mut vx, smooth_time, dt),
        smooth_damp(current.y, target.y, &mut vy, smooth_time, dt),
        smooth_damp(current.z, target.z, &mut vz, smooth_time, dt),
    );
    *vel = Vec3::new(vx, vy, vz);
    out
}

#[derive(Component)]
pub struct FireLight(usize);

fn home(scn: &scenario::Scenario) -> Orbit {
    let auto = std::env::var("TTX_SNAPSHOT").is_err()
        && ["TTX_ORBIT", "TTX_CINEMATIC"]
            .iter()
            .any(|k| std::env::var(k).is_ok_and(|v| v == "1"));
    // TTX_CAM="x,y,distance,yaw,pitch" (world metres, radians) sets the start view.
    let v: Vec<f32> = std::env::var("TTX_CAM")
        .ok()
        .map(|s| s.split(',').filter_map(|t| t.trim().parse().ok()).collect())
        .unwrap_or_default();
    if v.len() == 5 && v.iter().all(|v| v.is_finite()) {
        return Orbit {
            focus: to_bevy(scn, [v[0], v[1]], 0.0),
            dist: v[2].clamp(150.0, 15000.0),
            yaw: v[3],
            pitch: v[4].clamp(0.12, 1.45),
            auto,
        };
    }
    Orbit {
        focus: to_bevy(scn, [2600.0, 2250.0], 0.0),
        yaw: 0.6,
        pitch: 0.62,
        dist: 2600.0,
        auto,
    }
}

pub fn setup(mut commands: Commands, scn: Res<Scn>, quality: Res<crate::quality::Quality>) {
    let o = home(&scn.0);
    let mut camera = commands.spawn((
        Camera3dBundle {
            camera: Camera {
                hdr: true,
                ..default()
            },
            tonemapping: Tonemapping::TonyMcMapface,
            color_grading: {
                use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
                let mut g = ColorGrading::with_identical_sections(
                    ColorGradingGlobal {
                        post_saturation: 1.05,
                        ..default()
                    },
                    ColorGradingSection::default(),
                );
                g.midtones.contrast = 1.1;
                g
            },
            projection: PerspectiveProjection {
                far: 40000.0,
                ..default()
            }
            .into(),
            transform: place(&o),
            ..default()
        },
        BloomSettings {
            intensity: quality.bloom,
            ..BloomSettings::NATURAL
        },
        FogSettings {
            color: SKY,
            // Warm light scattering towards the sun.
            directional_light_color: Color::srgba(1.0, 0.86, 0.66, 0.35),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::from_visibility_colors(
                22000.0,
                Color::srgb(0.62, 0.68, 0.78),
                Color::srgb(0.95, 0.80, 0.62),
            ),
        },
    ));
    if quality.name == "low" {
        camera.insert(bevy::core_pipeline::smaa::SmaaSettings {
            preset: bevy::core_pipeline::smaa::SmaaPreset::Medium,
        });
    }
    // The procedural sky updates sunlight from the scenario clock.
    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight {
                illuminance: 12_000.0,
                color: Color::srgb(1.0, 0.9, 0.76),
                shadows_enabled: quality.shadows,
                ..default()
            },
            transform: Transform::from_xyz(-1.4, 0.55, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
            cascade_shadow_config: CascadeShadowConfigBuilder {
                num_cascades: 4,
                minimum_distance: 0.1,
                maximum_distance: 6000.0,
                first_cascade_far_bound: 300.0,
                overlap_proportion: 0.2,
            }
            .into(),
            ..default()
        },
        crate::sky::Sun,
    ));
    for k in 0..quality.fire_lights.min(FIRE_LIGHTS) {
        commands.spawn((
            PointLightBundle {
                point_light: PointLight {
                    color: Color::srgb(1.0, 0.5, 0.16),
                    intensity: 0.0,
                    range: 1000.0,
                    radius: 30.0,
                    shadows_enabled: false,
                    ..default()
                },
                transform: Transform::from_xyz(0.0, -1000.0, 0.0),
                ..default()
            },
            FireLight(k),
        ));
    }
    commands.insert_resource(o);
    commands.insert_resource(Director::default());
}

pub(super) fn place(o: &Orbit) -> Transform {
    let dir = Vec3::new(
        o.yaw.sin() * o.pitch.cos(),
        o.pitch.sin(),
        o.yaw.cos() * o.pitch.cos(),
    );
    Transform::from_translation(o.focus + dir * o.dist).looking_at(o.focus, Vec3::Y)
}

/// Centroid and radius (m) of the fire area (burning and burnt cells), if
/// any. The whole scar grows steadily, so it is a calm camera target; the
/// flickering front alone would make the camera twitch.
pub fn fire_extent(scn: &scenario::Scenario, cells: &[u8]) -> Option<(scenario::Pos, f32)> {
    let cols = scn.grid.cols;
    let pts: Vec<scenario::Pos> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == 1 || **c == 2)
        .map(|(i, _)| {
            scn.cell_centre(scenario::Cell {
                row: i / cols,
                col: i % cols,
            })
        })
        .collect();
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as f32;
    let c = [
        pts.iter().map(|p| p[0]).sum::<f32>() / n,
        pts.iter().map(|p| p[1]).sum::<f32>() / n,
    ];
    let r = pts
        .iter()
        .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt())
        .fold(0.0, f32::max);
    Some((c, r.max(100.0)))
}

/// Framing for a cue: a push-in on its location, sized to what it covers.
fn shot_for(scn: &scenario::Scenario, cue: &Cue, pitch: f32) -> Shot {
    Shot {
        focus: to_bevy(scn, cue.pos, 0.0),
        dist: (cue.radius_m * 3.2).clamp(650.0, 2600.0),
        pitch: pitch.clamp(0.42, 0.7),
        drift: 0.035,
    }
}

fn ease(current: f32, target: f32, dt: f32, tau: f32) -> f32 {
    current + (target - current) * (1.0 - (-dt / tau).exp())
}

#[allow(clippy::too_many_arguments)]
pub fn control(
    mut o: ResMut<Orbit>,
    mut dir: ResMut<Director>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
    mut egui: EguiContexts,
    scn: Res<Scn>,
    frame: Res<Frame>,
    snap: Res<Snapshot>,
    time: Res<Time>,
) {
    let s = &scn.0;
    let now = time.elapsed_seconds();
    let dt = time.delta_seconds().min(0.1);
    let over_ui = egui.ctx_mut().wants_pointer_input();
    let delta: Vec2 = motion.read().map(|m| m.delta).sum();
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    let mut manual = false;
    if !over_ui {
        if buttons.pressed(MouseButton::Left) && delta != Vec2::ZERO {
            o.yaw -= delta.x * 0.005;
            o.pitch = (o.pitch + delta.y * 0.004).clamp(0.12, 1.45);
            manual = true;
        }
        if (buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle))
            && delta != Vec2::ZERO
        {
            let right = Vec3::new(o.yaw.cos(), 0.0, -o.yaw.sin());
            let fwd = Vec3::new(-o.yaw.sin(), 0.0, -o.yaw.cos());
            let k = o.dist * 0.0015;
            o.focus += (-right * delta.x + fwd * delta.y) * k;
            manual = true;
        }
        if scroll != 0.0 {
            o.dist = (o.dist * (1.0 - scroll * 0.08)).clamp(150.0, 7000.0);
            manual = true;
        }
    }
    if manual {
        dir.manual_until = now + 15.0;
    }
    if !egui.ctx_mut().wants_keyboard_input() && keys.just_pressed(KeyCode::KeyA) {
        o.auto = !o.auto;
    }
    if !egui.ctx_mut().wants_keyboard_input()
        && (keys.just_pressed(KeyCode::KeyR) || keys.just_pressed(KeyCode::Home))
    {
        *o = Orbit {
            auto: o.auto,
            ..home(s)
        };
    }
    let fire = fire_extent(s, &frame.cells);
    if !egui.ctx_mut().wants_keyboard_input() && keys.just_pressed(KeyCode::KeyF) {
        if let Some((c, r)) = fire {
            o.focus = to_bevy(s, c, 0.0);
            o.dist = (r * 3.0).clamp(600.0, 3000.0);
            dir.manual_until = now + 15.0;
        }
    }

    // New cues from the exercise, queued (the latest few only: never lag behind).
    if snap.cues.len() > dir.seen_cues {
        for c in &snap.cues[dir.seen_cues..] {
            dir.queue.push((c.clone(), now));
        }
        dir.seen_cues = snap.cues.len();
        let n = dir.queue.len();
        if n > 3 {
            dir.queue.drain(0..n - 3);
        }
    }

    if o.auto && now >= dir.manual_until {
        let resolving = snap.resolving.is_some();
        // Fire tracking target, smoothed so a flickering front does not shake the camera.
        if let Some((c, r)) = fire {
            let target = to_bevy(s, c, 0.0);
            dir.fire_focus = Some(
                dir.fire_focus
                    .map_or(target, |f| f.lerp(target, (dt / 4.0).min(1.0))),
            );
            dir.fire_dist = ease(
                dir.fire_dist.max(1100.0),
                (r * 3.0).clamp(1100.0, 3800.0),
                dt,
                5.0,
            );
        }
        // Pick the shot.
        let shot_done = dir.current.is_none_or(|(_, t0, d)| now - t0 > d);
        let shot = if resolving {
            // Between turns the fire is the story.
            dir.current = None;
            dir.queue.clear();
            Shot {
                focus: dir.fire_focus.unwrap_or(o.focus),
                dist: dir.fire_dist.max(1100.0),
                pitch: 0.72,
                drift: 0.06,
            }
        } else {
            if shot_done {
                dir.current = None;
                if !dir.queue.is_empty() {
                    let (cue, _) = dir.queue.remove(0);
                    let sh = shot_for(s, &cue, o.pitch);
                    dir.current = Some((sh, now, cue.hold_s));
                }
            }
            match dir.current {
                Some((mut sh, t0, d)) => {
                    // Slow push-in across the shot.
                    let k = ((now - t0) / d).clamp(0.0, 1.0);
                    sh.dist *= 1.18 - 0.18 * k;
                    sh
                }
                // Establishing: wide, around the fire (or the valley), slow orbit.
                None => Shot {
                    focus: dir
                        .fire_focus
                        .map_or(to_bevy(s, [2600.0, 2250.0], 0.0), |f| {
                            f.lerp(to_bevy(s, [2400.0, 2000.0], 0.0), 0.35)
                        }),
                    dist: 2500.0,
                    pitch: 0.6,
                    drift: 0.03,
                },
            }
        };
        let smooth = if resolving { 2.4 } else { 1.6 };
        let (mut vf, mut vd, mut vp) = (dir.v_focus, dir.v_dist, dir.v_pitch);
        o.focus = smooth_damp3(o.focus, shot.focus, &mut vf, smooth, dt);
        o.dist = smooth_damp(o.dist, shot.dist, &mut vd, smooth * 1.3, dt);
        o.pitch = smooth_damp(
            o.pitch,
            (shot.pitch + dir.pitch_boost).min(1.3),
            &mut vp,
            smooth * 1.6,
            dt,
        );
        (dir.v_focus, dir.v_dist, dir.v_pitch) = (vf, vd, vp);
        o.yaw += shot.drift * dt;
    }
    // Never let a hill come between the camera and what it looks at: sample
    // the terrain along the sight line and raise the camera if needed.
    if o.auto && now >= dir.manual_until {
        let t = place(&o);
        let clearance = (0..=8)
            .map(|k| {
                let p = t.translation.lerp(o.focus, k as f32 / 10.0);
                let ground = crate::frame::height_at(s, crate::frame::to_world(s, p))
                    * crate::frame::EXAGGERATION;
                p.y - ground
            })
            .fold(f32::MAX, f32::min);
        // Hysteresis: raise when too close, release only once well clear.
        if clearance < 160.0 {
            dir.pitch_boost = (dir.pitch_boost + dt * 0.25).min(0.6);
        } else if clearance > 400.0 {
            dir.pitch_boost = (dir.pitch_boost - dt * 0.04).max(0.0);
        }
    }
    // Keep manual and scripted views above terrain too.
    let candidate = place(&o).translation;
    let ground = crate::frame::height_at(s, crate::frame::to_world(s, candidate))
        * crate::frame::EXAGGERATION;
    if candidate.y < ground + 30.0 {
        o.pitch = ((ground + 30.0 - o.focus.y) / o.dist)
            .clamp(0.0, 0.99)
            .asin()
            .max(o.pitch);
    }
    if let Ok(mut t) = cam.get_single_mut() {
        *t = place(&o);
    }
}

/// Put the fire lights on the hottest 200 m bins of the burning area.
pub fn fire_lights(
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    frame: Res<Frame>,
    time: Res<Time>,
    mut lights: Query<(&FireLight, &mut PointLight, &mut Transform)>,
) {
    let s = &scn.0;
    if frame.cells.len() != s.grid.len() || snap.intensity.len() != s.grid.len() {
        return;
    }
    let bin = 200.0;
    let nb = (s.grid.width_m() / bin).ceil() as usize;
    let mut heat = vec![0.0f32; nb * nb];
    let mut sum = vec![[0.0f32; 2]; nb * nb];
    let cols = s.grid.cols;
    for (i, c) in frame.cells.iter().enumerate() {
        if *c != 1 {
            continue;
        }
        let p = s.cell_centre(scenario::Cell {
            row: i / cols,
            col: i % cols,
        });
        let b = ((p[1] / bin) as usize).min(nb - 1) * nb + ((p[0] / bin) as usize).min(nb - 1);
        let w = snap.intensity[i].max(200.0);
        heat[b] += w;
        sum[b][0] += p[0] * w;
        sum[b][1] += p[1] * w;
    }
    let mut order: Vec<usize> = (0..heat.len()).filter(|&b| heat[b] > 0.0).collect();
    order.sort_by(|a, b| heat[*b].partial_cmp(&heat[*a]).unwrap());
    let t = time.elapsed_seconds();
    for (light, mut pl, mut tf) in lights.iter_mut() {
        match order.get(light.0) {
            Some(&b) => {
                let p = [sum[b][0] / heat[b], sum[b][1] / heat[b]];
                tf.translation = to_bevy(s, p, 25.0);
                let flicker = 0.8
                    + 0.2
                        * (t * 7.0 + light.0 as f32 * 1.7).sin()
                        * (t * 3.1 + light.0 as f32).cos();
                // Bright enough to read against daylight on the ground around the fire.
                pl.intensity = (heat[b] * 9000.0).clamp(2.0e7, 6.0e8) * flicker;
            }
            None => pl.intensity = 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_damp_converges_without_overshoot() {
        let (mut x, mut v) = (0.0f32, 0.0f32);
        let mut max = 0.0f32;
        for _ in 0..600 {
            x = smooth_damp(x, 10.0, &mut v, 1.0, 1.0 / 60.0);
            max = max.max(x);
        }
        assert!((x - 10.0).abs() < 0.01 && max <= 10.0 + 1e-3, "{x} {max}");
    }

    #[test]
    fn fire_extent_centroid_and_radius() {
        let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let mut cells = vec![0u8; scn.grid.len()];
        assert!(fire_extent(&scn, &cells).is_none());
        cells[100 * scn.grid.cols + 100] = 1;
        cells[100 * scn.grid.cols + 110] = 1;
        let (c, r) = fire_extent(&scn, &cells).unwrap();
        assert!((c[0] - 2110.0).abs() < 1.0);
        assert!(r >= 100.0);
    }
}
