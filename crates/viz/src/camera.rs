//! Orbit camera: left-drag rotate, right-drag pan, scroll zoom,
//! `A` auto-orbit (presentation mode), `F` focus on the fire, `R` reset.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::frame::{to_bevy, Frame};
use crate::Scn;

#[derive(Resource)]
pub struct Orbit {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub auto: bool,
}

fn home(scn: &scenario::Scenario) -> Orbit {
    Orbit { focus: to_bevy(scn, [2600.0, 2250.0], 0.0), yaw: 0.6, pitch: 0.62, dist: 2600.0, auto: false }
}

pub fn setup(mut commands: Commands, scn: Res<Scn>) {
    let o = home(&scn.0);
    commands.spawn(Camera3dBundle { transform: place(&o), ..default() });
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight { illuminance: 9000.0, shadows_enabled: false, ..default() },
        transform: Transform::from_xyz(-1.0, 2.0, 1.2).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });
    commands.insert_resource(o);
}

fn place(o: &Orbit) -> Transform {
    let dir = Vec3::new(o.yaw.sin() * o.pitch.cos(), o.pitch.sin(), o.yaw.cos() * o.pitch.cos());
    Transform::from_translation(o.focus + dir * o.dist).looking_at(o.focus, Vec3::Y)
}

#[allow(clippy::too_many_arguments)]
pub fn control(
    mut o: ResMut<Orbit>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
    mut egui: EguiContexts,
    scn: Res<Scn>,
    frame: Res<Frame>,
    time: Res<Time>,
) {
    let over_ui = egui.ctx_mut().wants_pointer_input();
    let delta: Vec2 = motion.read().map(|m| m.delta).sum();
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if !over_ui {
        if buttons.pressed(MouseButton::Left) {
            o.yaw -= delta.x * 0.005;
            o.pitch = (o.pitch + delta.y * 0.004).clamp(0.12, 1.45);
            o.auto = false;
        }
        if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle) {
            let right = Vec3::new(o.yaw.cos(), 0.0, -o.yaw.sin());
            let fwd = Vec3::new(-o.yaw.sin(), 0.0, -o.yaw.cos());
            let k = o.dist * 0.0015;
            o.focus += (-right * delta.x + fwd * delta.y) * k;
        }
        if scroll != 0.0 {
            o.dist = (o.dist * (1.0 - scroll * 0.08)).clamp(150.0, 7000.0);
        }
    }
    if keys.just_pressed(KeyCode::KeyA) {
        o.auto = !o.auto;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        *o = home(&scn.0);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        let s = &scn.0;
        let burning: Vec<scenario::Pos> = frame
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == 1)
            .map(|(i, _)| s.cell_centre(scenario::Cell { row: i / s.grid.cols, col: i % s.grid.cols }))
            .collect();
        if !burning.is_empty() {
            let n = burning.len() as f32;
            let c = [burning.iter().map(|p| p[0]).sum::<f32>() / n, burning.iter().map(|p| p[1]).sum::<f32>() / n];
            o.focus = to_bevy(s, c, 0.0);
            o.dist = o.dist.min(1500.0);
        }
    }
    if o.auto {
        o.yaw += time.delta_seconds() * 0.05;
    }
    if let Ok(mut t) = cam.get_single_mut() {
        *t = place(&o);
    }
}
