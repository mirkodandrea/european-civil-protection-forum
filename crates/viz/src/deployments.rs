//! Models depict approved deployments at the simulator's authoritative position.
use crate::frame::to_bevy;
use crate::materials::{structure, Finish, StructureMaterial};
use crate::models::{self, Rig};
use crate::{Scn, Snapshot};
use bevy::prelude::*;
use scenario::Pos;
use view::TaskStatus;
use std::collections::HashMap;
#[derive(Component)]
pub struct DeploymentMarker(pub u64);
#[derive(Component)]
pub struct TrafficMarker {
    index: usize,
    bus: bool,
}
#[derive(Component)]
pub struct UnitPart {
    task: u64,
    rig: Rig,
}
#[derive(Resource)]
pub struct DeploymentPalette {
    models: HashMap<&'static str, (Handle<Mesh>, Vec<(Handle<Mesh>, Rig)>)>,
    material: Handle<StructureMaterial>,
    kit_material: Handle<StructureMaterial>,
}
impl DeploymentPalette {
    /// Body mesh and material of a model (`"tanker"`, `"car"`, `"bus"`…), for
    /// the effects that borrow a vehicle without being a deployment.
    pub fn body(&self, name: &str) -> (Handle<Mesh>, Handle<StructureMaterial>) {
        let material = if matches!(name, "engine" | "tanker" | "firefighter" | "drone") {
            &self.material
        } else {
            &self.kit_material
        };
        (self.models[name].0.clone(), material.clone())
    }
}
/// Altitude a monitoring drone flies at, metres above the ground (a symbol
/// altitude, like the tankers': at 50 m real it would vanish in the canopy).
const DRONE_ALT_M: f32 = 90.0;
/// Ground speed of the drawn orbit over the monitored area, metres per real second.
const DRONE_ORBIT_MPS: f32 = 28.0;
/// Beacons and headlamps brighten after dark (`structure.wgsl`, lamps read the
/// night factor from the material's emissive red).
pub fn night_vehicles(
    sky: Res<crate::sky::SkyEnv>,
    pal: Option<Res<DeploymentPalette>>,
    mut mats: ResMut<Assets<StructureMaterial>>,
    mut last: Local<f32>,
) {
    let Some(pal) = pal else { return };
    let night = (crate::night::night_factor(&sky) * 50.0).round() / 50.0;
    if night == *last {
        return;
    }
    *last = night;
    for handle in [&pal.material, &pal.kit_material] {
        if let Some(m) = mats.get_mut(handle) {
            m.base.emissive = LinearRgba::new(night, 0.0, 0.0, 0.0);
        }
    }
}
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StructureMaterial>>,
) {
    let mut models = HashMap::new();
    for name in ["engine", "tanker", "firefighter", "drone"] {
        let body = meshes.add(models::mesh(&models::units()[name].model, Vec3::ZERO));
        let parts = models::unit_parts(name)
            .into_iter()
            .map(|(m, r)| (meshes.add(m), r))
            .collect();
        models.insert(name, (body, parts));
    }
    for (key, name) in [
        ("bus", "car_bus"),
        ("police", "car_suv"),
        ("car", "car_hatch"),
    ] {
        let model = models::urban_model(name).expect("vehicle kit");
        models.insert(
            key,
            (meshes.add(models::mesh(model, Vec3::ZERO)), Vec::new()),
        );
    }
    commands.insert_resource(DeploymentPalette {
        models,
        kit_material: mats.add(structure(
            StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.65,
                ..default()
            },
            Finish::None,
        )),
        material: mats.add(structure(
            StandardMaterial {
                base_color: Color::WHITE,
                // Enable Bevy's clearcoat pipeline variant; per-surface WGSL
                // then controls it on paint and disables it on rubber/cloth.
                clearcoat: 1.0,
                perceptual_roughness: 0.65,
                ..default()
            },
            Finish::None,
        )),
    });
}
fn is_drone(scn: &Scn, task: &view::Deployment) -> bool {
    task.action == "MonitorArea"
        || task.resource == "drones"
        || scn.0.drone_spec(&task.resource).is_some()
}
fn model_for(scn: &Scn, task: &view::Deployment) -> (&'static str, f32, f32) {
    if is_drone(scn, task) {
        return ("drone", 7.0, DRONE_ALT_M);
    }
    match task.resource.as_str() {
        "air_tankers" => ("tanker", 2.8, 120.),
        "ground_crews" => ("firefighter", 4., 0.2),
        "buses" => ("bus", 3., 0.2),
        "police_patrols" => ("police", 3.5, 0.2),
        _ => ("engine", 3.5, 0.2),
    }
}
fn status_colour(s: TaskStatus) -> Color {
    match s {
        TaskStatus::Working => Color::srgb(0.1, 0.9, 1.),
        TaskStatus::Completed => Color::srgb(0.2, 1., 0.35),
        TaskStatus::Failed => Color::srgb(1., 0.1, 0.15),
        _ => Color::srgb(1., 0.65, 0.1),
    }
}
pub fn update(
    mut commands: Commands,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    time: Res<Time>,
    palette: Res<DeploymentPalette>,
    mut markers: Query<
        (Entity, &DeploymentMarker, &mut Transform),
        (Without<UnitPart>, Without<TrafficMarker>),
    >,
    mut parts: Query<
        (&UnitPart, &mut Transform),
        (Without<DeploymentMarker>, Without<TrafficMarker>),
    >,
    mut traffic: Query<
        (Entity, &TrafficMarker, &mut Transform),
        (Without<DeploymentMarker>, Without<UnitPart>),
    >,
    mut gizmos: Gizmos,
) {
    let visible = snap
        .deployments
        .iter()
        .filter(|task| task.qty > 0 && task.status != TaskStatus::Cancelled)
        .collect::<Vec<_>>();
    let real_s = time.elapsed_seconds();
    for (entity, marker, mut transform) in &mut markers {
        if let Some(task) = visible.iter().find(|t| t.id == marker.0) {
            set(&scn, task, snap.time_s, real_s, &mut transform);
        } else {
            commands.entity(entity).despawn_recursive();
        }
    }
    for (part, mut tf) in &mut parts {
        if let Some(task) = visible.iter().find(|t| t.id == part.task) {
            let moving = task.position(snap.time_s) != task.position(snap.time_s + 1);
            let work = task.status == TaskStatus::Working;
            let t = snap.time_s as f32;
            // Propellers and beacons run on real time: tied to the simulated
            // clock they would strobe at speed and freeze between turns.
            let a = match part.rig.kind.as_str() {
                "wheel" if moving => t * 2.,
                "prop" if moving || work => real_s * 40.,
                "swing" if moving => (t * 3. + part.rig.phase).sin() * 0.4,
                "tool" if work => (real_s * 5. + part.rig.phase).sin() * 0.6,
                _ => 0.,
            };
            tf.rotation = Quat::from_axis_angle(part.rig.axis.normalize_or_zero(), a);
            if part.rig.kind == "blink" {
                let active = moving || work;
                let lit = active && ((real_s * 3.).floor() as i32 + part.rig.phase as i32) % 2 == 0;
                tf.scale = Vec3::splat(if lit { 1. } else { 0. });
            }
        }
    }
    let show_marks = std::env::var("TTX_SNAPSHOT_MARKS").as_deref() != Ok("0");
    for (entity, marker, mut tf) in &mut traffic {
        if let Some(view::Vehicle { pos, people, .. }) = snap
            .traffic
            .get(marker.index)
            .filter(|v| (v.people > 4) == marker.bus)
        {
            let next = to_bevy(&scn.0, *pos, 0.3);
            let d = next - tf.translation;
            if d.length_squared() > 0.01 {
                tf.rotation = Quat::from_rotation_y(d.x.atan2(d.z));
            }
            tf.translation = next;
            let _ = people;
        } else {
            commands.entity(entity).despawn_recursive();
        }
    }
    for (index, view::Vehicle { pos, people, at_risk }) in snap.traffic.iter().enumerate() {
        let bus = *people > 4;
        if !traffic
            .iter()
            .any(|(_, m, _)| m.index == index && m.bus == bus)
        {
            commands.spawn((
                TrafficMarker { index, bus },
                MaterialMeshBundle {
                    mesh: palette.models[if bus { "bus" } else { "car" }].0.clone(),
                    material: palette.kit_material.clone(),
                    transform: Transform::from_translation(to_bevy(&scn.0, *pos, 0.3))
                        .with_scale(Vec3::splat(2.)),
                    ..default()
                },
            ));
        }
        if show_marks {
            gizmos.cuboid(
                Transform::from_translation(to_bevy(&scn.0, *pos, 5.)).with_scale(Vec3::new(
                    7.,
                    5.,
                    if bus { 18. } else { 10. },
                )),
                if *at_risk {
                    Color::srgb(1., 0.1, 0.1)
                } else {
                    Color::srgb(0.2, 0.8, 1.)
                },
            );
        }
    }
    for task in visible {
        let (name, _, lift) = model_for(&scn, task);
        let airborne = matches!(name, "tanker" | "drone");
        let mut transform = Transform::default();
        set(&scn, task, snap.time_s, real_s, &mut transform);
        if !markers.iter().any(|(_, m, _)| m.0 == task.id) {
            let (body, parts) = &palette.models[name];
            let material = if matches!(name, "engine" | "tanker" | "firefighter" | "drone") {
                palette.material.clone()
            } else {
                palette.kit_material.clone()
            };
            commands
                .spawn((
                    DeploymentMarker(task.id),
                    MaterialMeshBundle {
                        mesh: body.clone(),
                        material,
                        transform,
                        ..default()
                    },
                ))
                .with_children(|parent| {
                    for (mesh, rig) in parts {
                        parent.spawn((
                            UnitPart {
                                task: task.id,
                                rig: rig.clone(),
                            },
                            MaterialMeshBundle {
                                mesh: mesh.clone(),
                                material: palette.material.clone(),
                                transform: Transform::from_translation(rig.pivot),
                                ..default()
                            },
                        ));
                    }
                });
        }
        if show_marks && name == "drone" && task.status == TaskStatus::Working {
            scan_marks(&scn, task, transform.translation, real_s, &mut gizmos);
        }
        if show_marks {
            let p = to_bevy(&scn.0, task.position(snap.time_s), lift + 2.);
            gizmos.linestrip(
                (0..=32).map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 32.;
                    p + Vec3::new(a.cos() * 21., 0., a.sin() * 21.)
                }),
                status_colour(task.status),
            );
        }
        if std::env::var("TTX_SNAPSHOT_MARKS").as_deref() != Ok("0")
            && matches!(
                task.status,
                TaskStatus::EnRoute | TaskStatus::Refilling | TaskStatus::Withdrawing
            )
        {
            let lift = if airborne { lift } else { 12.0 };
            let points = task
                .route
                .as_ref()
                .cloned()
                // Only aircraft have a direct route; a missing road path is not
                // evidence of a traversable straight line.
                .unwrap_or_else(|| {
                    if airborne {
                        vec![task.origin, task.target]
                    } else {
                        Vec::new()
                    }
                });
            // A destination is a hollow diamond, distinct from the actual vehicle.
            let p = to_bevy(&scn.0, task.target, lift + 25.0);
            gizmos.linestrip(
                [
                    p + Vec3::X * 22.0,
                    p + Vec3::Z * 22.0,
                    p - Vec3::X * 22.0,
                    p - Vec3::Z * 22.0,
                    p + Vec3::X * 22.0,
                ],
                Color::srgb(1.0, 0.75, 0.3),
            );
            gizmos.linestrip(
                points.into_iter().map(|p| to_bevy(&scn.0, p, lift + 25.0)),
                Color::srgba(1.0, 0.65, 0.1, 0.5),
            );
        }
    }
}
/// Where a drone on station circles: the centre and a radius that keeps the
/// orbit inside the monitored zones.
fn orbit(scn: &Scn, task: &view::Deployment) -> Option<(Pos, f32)> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for z in &task.zones {
        let r = scn.0.zone_bounds(z)?;
        b = [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])];
    }
    if task.zones.is_empty() {
        return Some((task.target, 160.0));
    }
    let half = ((b[2] - b[0]).min(b[3] - b[1]) * 0.5).max(1.0);
    Some(([(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5], (half * 0.55).clamp(80.0, 320.0)))
}

/// The drawn position of a deployment: its track, except a drone on station,
/// which circles over its area in real time.
fn drawn(scn: &Scn, task: &view::Deployment, t: i64, real_s: f32) -> (Pos, Pos, f32) {
    let (name, _, lift) = model_for(scn, task);
    let (here, next) = (task.position(t), task.position(t + 1));
    if name != "drone" {
        return (here, next, lift);
    }
    if task.status == TaskStatus::Working {
        if let Some((c, r)) = orbit(scn, task) {
            let a = real_s * DRONE_ORBIT_MPS / r + task.id as f32 * 1.7;
            let at = |a: f32| [c[0] + r * a.cos(), c[1] + r * a.sin()];
            return (at(a), at(a + 0.05), lift);
        }
    }
    // Parked at its base (recharging, or waiting): on the ground, rotors still.
    let lift = if here == next && task.status != TaskStatus::EnRoute { 0.3 } else { lift };
    (here, next, lift)
}

fn set(scn: &Scn, task: &view::Deployment, t: i64, real_s: f32, transform: &mut Transform) {
    let (_, scale, _) = model_for(scn, task);
    let (here, next, lift) = drawn(scn, task, t, real_s);
    transform.translation = to_bevy(&scn.0, here, lift);
    transform.scale = Vec3::splat(scale);
    let direction = to_bevy(&scn.0, next, lift) - to_bevy(&scn.0, here, lift);
    if direction.length_squared() > 0.01 {
        transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
    }
}

/// A drone on station: the monitored zones outlined, and a sensor cone from
/// the aircraft to a sweeping footprint on the ground.
fn scan_marks(scn: &Scn, task: &view::Deployment, at: Vec3, real_s: f32, gizmos: &mut Gizmos) {
    let s = &scn.0;
    let cyan = Color::srgba(0.3, 0.95, 1.0, 0.8);
    for z in &task.zones {
        let Some(b) = s.zone_bounds(z) else { continue };
        let ring = [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]], [b[0], b[1]]];
        let mut pts = Vec::new();
        for w in ring.windows(2) {
            for k in 0..=24 {
                let f = k as f32 / 24.0;
                pts.push(to_bevy(s, [w[0][0] + (w[1][0] - w[0][0]) * f, w[0][1] + (w[1][1] - w[0][1]) * f], 8.0));
            }
        }
        gizmos.linestrip(pts, Color::srgba(0.3, 0.95, 1.0, 0.45));
    }
    let foot = crate::frame::to_world(s, at);
    let r = 55.0 + 10.0 * (real_s * 2.0).sin();
    let rim: Vec<Vec3> = (0..=24)
        .map(|k| {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            to_bevy(s, [foot[0] + r * a.cos(), foot[1] + r * a.sin()], 4.0)
        })
        .collect();
    for p in rim.iter().step_by(6) {
        gizmos.line(at - Vec3::Y * 2.0, *p, Color::srgba(0.3, 0.95, 1.0, 0.35));
    }
    gizmos.linestrip(rim, cyan);
}
