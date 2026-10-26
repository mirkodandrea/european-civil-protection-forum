//! Models depict approved deployments at the simulator's authoritative position.
use crate::frame::to_bevy;
use crate::materials::{structure, Finish, StructureMaterial};
use crate::models::{self, Rig};
use crate::{Scn, Snapshot};
use bevy::prelude::*;
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
    for name in ["engine", "tanker", "firefighter"] {
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
fn model_for(resource: &str) -> (&'static str, f32, f32) {
    match resource {
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
    for (entity, marker, mut transform) in &mut markers {
        if let Some(task) = visible.iter().find(|t| t.id == marker.0) {
            set(&scn, task, snap.time_s, &mut transform);
        } else {
            commands.entity(entity).despawn_recursive();
        }
    }
    for (part, mut tf) in &mut parts {
        if let Some(task) = visible.iter().find(|t| t.id == part.task) {
            let moving = task.position(snap.time_s) != task.position(snap.time_s + 1);
            let work = task.status == TaskStatus::Working;
            let t = snap.time_s as f32;
            let a = match part.rig.kind.as_str() {
                "wheel" if moving => t * 2.,
                "prop" if moving || work => t * 24.,
                "swing" if moving => (t * 3. + part.rig.phase).sin() * 0.4,
                "tool" if work => (t * 2. + part.rig.phase).sin() * 0.25,
                _ => 0.,
            };
            tf.rotation = Quat::from_axis_angle(part.rig.axis.normalize_or_zero(), a);
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
        let (name, _, lift) = model_for(&task.resource);
        let airborne = name == "tanker";
        let mut transform = Transform::default();
        set(&scn, task, snap.time_s, &mut transform);
        if !markers.iter().any(|(_, m, _)| m.0 == task.id) {
            let (body, parts) = &palette.models[name];
            let material = if matches!(name, "engine" | "tanker" | "firefighter") {
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
            let lift = if airborne { 120.0 } else { 12.0 };
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
fn set(scn: &Scn, task: &view::Deployment, t: i64, transform: &mut Transform) {
    let (_, scale, lift) = model_for(&task.resource);
    transform.translation = to_bevy(&scn.0, task.position(t), lift);
    transform.scale = Vec3::splat(scale);
    let direction =
        to_bevy(&scn.0, task.position(t + 1), lift) - to_bevy(&scn.0, task.position(t), lift);
    if direction.length_squared() > 0.01 {
        transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
    }
}
