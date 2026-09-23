//! Buildings (status-coloured blocks), roads, fire lines, water drops and the
//! zone grid, all draped on the terrain.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use scenario::{BuildingKind, Pos, Scenario};

use crate::frame::{to_bevy, EXAGGERATION};
use crate::{Scn, Snapshot};

#[derive(Component)]
pub struct BuildingTag(usize);

#[derive(Component)]
pub struct RoadTag(usize);

#[derive(Component)]
pub struct LineLayer;

#[derive(Resource)]
pub struct Palette {
    house: Handle<StandardMaterial>,
    facility: Handle<StandardMaterial>,
    threatened: Handle<StandardMaterial>,
    evacuating: Handle<StandardMaterial>,
    evacuated: Handle<StandardMaterial>,
    damaged: Handle<StandardMaterial>,
    destroyed: Handle<StandardMaterial>,
    road: Handle<StandardMaterial>,
    road_closed: Handle<StandardMaterial>,
    road_cut: Handle<StandardMaterial>,
    line: Handle<StandardMaterial>,
}

/// Show the zone grid (toggle with G).
#[derive(Resource)]
pub struct ShowGrid(pub bool);

pub fn setup(mut commands: Commands, scn: Res<Scn>, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let s = &scn.0;
    let mut solid = |r, g, b| mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: 0.8, ..default() });
    let pal = Palette {
        house: solid(0.86, 0.83, 0.77),
        facility: solid(0.55, 0.62, 0.80),
        threatened: solid(1.0, 0.78, 0.2),
        evacuating: solid(0.2, 0.85, 1.0),
        evacuated: solid(0.25, 0.45, 1.0),
        damaged: solid(1.0, 0.42, 0.0),
        destroyed: solid(0.07, 0.07, 0.07),
        road: solid(0.28, 0.28, 0.30),
        road_closed: solid(1.0, 0.55, 0.0),
        road_cut: solid(0.85, 0.08, 0.08),
        line: mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.9, 0.1), unlit: true, ..default() }),
    };
    for (i, b) in s.buildings.iter().enumerate() {
        let height = match b.kind {
            BuildingKind::House => 7.0,
            BuildingKind::FuelStation => 5.0,
            _ => 12.0,
        } * EXAGGERATION;
        let base = to_bevy(s, b.pos, 0.0);
        commands.spawn((
            PbrBundle {
                mesh: meshes.add(Cuboid::new(b.size[0], height, b.size[1])),
                material: if b.name.is_some() { pal.facility.clone() } else { pal.house.clone() },
                transform: Transform::from_translation(base + Vec3::Y * (height * 0.5 - 1.0)),
                ..default()
            },
            BuildingTag(i),
        ));
    }
    for (i, r) in s.roads.iter().enumerate() {
        commands.spawn((PbrBundle { mesh: meshes.add(ribbon(s, &r.points, 9.0, 1.2)), material: pal.road.clone(), ..default() }, RoadTag(i)));
    }
    commands.insert_resource(pal);
    commands.insert_resource(ShowGrid(true));
}

/// A flat strip along a polyline, draped on the terrain.
fn ribbon(s: &Scenario, pts: &[Pos], width: f32, lift: f32) -> Mesh {
    let mut samples: Vec<Pos> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = (len / 15.0).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f32 / n as f32;
            samples.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if let Some(l) = pts.last() {
        samples.push(*l);
    }
    let (mut p, mut n, mut ix) = (Vec::new(), Vec::new(), Vec::new());
    for (k, q) in samples.iter().enumerate() {
        let next = samples.get(k + 1).or(samples.get(k.wrapping_sub(1))).copied().unwrap_or(*q);
        let (mut dx, mut dy) = (next[0] - q[0], next[1] - q[1]);
        if k + 1 == samples.len() {
            (dx, dy) = (-dx, -dy);
        }
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / l * width * 0.5, dx / l * width * 0.5);
        p.push(to_bevy(s, [q[0] + nx, q[1] + ny], lift).to_array());
        p.push(to_bevy(s, [q[0] - nx, q[1] - ny], lift).to_array());
        n.push([0.0, 1.0, 0.0]);
        n.push([0.0, 1.0, 0.0]);
        if k + 1 < samples.len() {
            let o = (k * 2) as u32;
            ix.extend_from_slice(&[o, o + 1, o + 2, o + 1, o + 3, o + 2]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n);
    m.insert_indices(Indices::U32(ix));
    m
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    snap: Res<Snapshot>,
    scn: Res<Scn>,
    pal: Option<Res<Palette>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buildings: Query<(&BuildingTag, &mut Handle<StandardMaterial>), Without<RoadTag>>,
    mut roads: Query<(&RoadTag, &mut Handle<StandardMaterial>), Without<BuildingTag>>,
    layers: Query<Entity, With<LineLayer>>,
    mut seen: Local<(u64, usize)>,
    mut gizmos: Gizmos,
    keys: Res<ButtonInput<KeyCode>>,
    mut grid: ResMut<ShowGrid>,
) {
    let Some(pal) = pal else { return };
    let s = &scn.0;
    if keys.just_pressed(KeyCode::KeyG) {
        grid.0 = !grid.0;
    }
    // Water drops and the zone grid as gizmos (cheap, redrawn every frame).
    for (at, r) in &snap.drops {
        let pts: Vec<Vec3> = (0..=40).map(|k| {
            let a = k as f32 / 40.0 * std::f32::consts::TAU;
            to_bevy(s, [at[0] + r * a.cos(), at[1] + r * a.sin()], 4.0)
        }).collect();
        gizmos.linestrip(pts, Color::srgb(0.2, 0.55, 1.0));
    }
    if grid.0 {
        let z = s.zones;
        let warned: std::collections::HashSet<&String> = snap.warned.iter().collect();
        let (w, h) = (s.grid.width_m(), s.grid.height_m());
        for c in 0..=z.cols {
            let x = (c as f32 * z.size_m).min(w - 1.0);
            gizmos.linestrip((0..=80).map(|k| to_bevy(s, [x, k as f32 / 80.0 * h], 3.0)), Color::srgba(1.0, 1.0, 1.0, 0.35));
        }
        for r in 0..=z.rows {
            let y = (r as f32 * z.size_m).min(h - 1.0);
            gizmos.linestrip((0..=80).map(|k| to_bevy(s, [k as f32 / 80.0 * w, y], 3.0)), Color::srgba(1.0, 1.0, 1.0, 0.35));
        }
        for name in &warned {
            if let Some(b) = s.zone_bounds(name) {
                let ring = [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]], [b[0], b[1]]];
                let mut pts = Vec::new();
                for wv in ring.windows(2) {
                    for k in 0..=20 {
                        let t = k as f32 / 20.0;
                        pts.push(to_bevy(s, [wv[0][0] + (wv[1][0] - wv[0][0]) * t, wv[0][1] + (wv[1][1] - wv[0][1]) * t], 6.0));
                    }
                }
                gizmos.linestrip(pts, Color::srgb(1.0, 0.8, 0.0));
            }
        }
    }

    if snap.generation == seen.0 {
        return;
    }
    // Building materials.
    for (tag, mut mat) in buildings.iter_mut() {
        let Some(st) = snap.buildings.get(tag.0) else { continue };
        let named = s.buildings[tag.0].name.is_some();
        let want = if st.damage == 2 {
            &pal.destroyed
        } else if st.damage == 1 {
            &pal.damaged
        } else if st.evacuating {
            &pal.evacuating
        } else if st.empty_evacuated {
            &pal.evacuated
        } else if st.threatened {
            &pal.threatened
        } else if named {
            &pal.facility
        } else {
            &pal.house
        };
        if *mat != *want {
            *mat = want.clone();
        }
    }
    for (tag, mut mat) in roads.iter_mut() {
        let (closed, cut) = snap.roads.get(tag.0).copied().unwrap_or_default();
        let want = if cut { &pal.road_cut } else if closed { &pal.road_closed } else { &pal.road };
        if *mat != *want {
            *mat = want.clone();
        }
    }
    // Fire lines: rebuild when their number changes.
    if snap.lines.len() != seen.1 {
        for e in layers.iter() {
            commands.entity(e).despawn();
        }
        for (a, b, w) in &snap.lines {
            commands.spawn((PbrBundle { mesh: meshes.add(ribbon(s, &[*a, *b], *w * 0.6, 2.0)), material: pal.line.clone(), ..default() }, LineLayer));
        }
        seen.1 = snap.lines.len();
    }
    seen.0 = snap.generation;
}
