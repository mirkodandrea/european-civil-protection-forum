//! Detailed PBR structures, operational roads, fire lines, water drops and the
//! zone grid, all draped on the terrain.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use scenario::{BuildingKind, Pos, Scenario};

use crate::frame::{to_bevy, EXAGGERATION};
use crate::materials::{structure, Finish, StructureMaterial};
use crate::night::night_factor;
use crate::sky::SkyEnv;
use crate::{Scn, Snapshot};

#[derive(Component)]
pub struct BuildingTag(usize);

#[derive(Component)]
pub struct RoadTag;
#[derive(Component)]
pub struct RoadStatusLayer;

#[derive(Component)]
pub struct LineLayer;

#[derive(Resource)]
pub struct Palette {
    house: Handle<StructureMaterial>,
    facility: Handle<StructureMaterial>,
    threatened: Handle<StructureMaterial>,
    evacuating: Handle<StructureMaterial>,
    evacuated: Handle<StructureMaterial>,
    damaged: Handle<StructureMaterial>,
    destroyed: Handle<StructureMaterial>,
    road_closed: Handle<StandardMaterial>,
    road_cut: Handle<StandardMaterial>,
    line: Handle<StandardMaterial>,
}

/// Lit windows after dark. Each occupied state has its own light (warm
/// homes, amber when threatened, cold white-blue while evacuating); empty and
/// damaged buildings stay dark, so the lights themselves say who is still
/// home. The shader (`structure.wgsl`, glass) picks which panes are on.
pub fn night_windows(
    sky: Res<SkyEnv>,
    pal: Option<Res<Palette>>,
    mut mats: ResMut<Assets<StructureMaterial>>,
    mut last: Local<f32>,
) {
    let Some(pal) = pal else { return };
    let night = (night_factor(&sky) * 50.0).round() / 50.0;
    if night == *last {
        return;
    }
    *last = night;
    for (handle, rgb) in [
        (&pal.house, [2.6, 1.7, 0.7]),
        (&pal.facility, [2.8, 2.2, 1.2]),
        (&pal.threatened, [3.4, 1.5, 0.4]),
        (&pal.evacuating, [1.6, 2.6, 3.4]),
    ] {
        if let Some(m) = mats.get_mut(handle) {
            m.base.emissive = LinearRgba::new(rgb[0], rgb[1], rgb[2], 0.0) * night;
        }
    }
}

/// Show the zone grid (toggle with G).
#[derive(Resource)]
pub struct ShowGrid(pub bool);

pub fn setup(
    mut commands: Commands,
    scn: Res<Scn>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut structures: ResMut<Assets<StructureMaterial>>,
) {
    let s = &scn.0;
    let mut solid = |r, g, b| {
        mats.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.8,
            ..default()
        })
    };
    let mut building_material = |r, g, b| {
        structures.add(structure(
            StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.85,
                ..default()
            },
            Finish::Town,
        ))
    };
    let pal = Palette {
        house: building_material(1.0, 1.0, 1.0),
        facility: building_material(1.0, 1.0, 1.0),
        threatened: building_material(1.0, 0.78, 0.2),
        evacuating: building_material(0.2, 0.85, 1.0),
        evacuated: building_material(0.25, 0.45, 1.0),
        damaged: building_material(0.45, 0.24, 0.13),
        destroyed: building_material(0.07, 0.07, 0.07),
        road_closed: solid(1.0, 0.55, 0.0),
        road_cut: solid(0.85, 0.08, 0.08),
        line: mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.1),
            unlit: true,
            ..default()
        }),
    };
    for (i, b) in s.buildings.iter().enumerate() {
        let height = match b.kind {
            BuildingKind::House => 7.0,
            BuildingKind::FuelStation => 5.0,
            _ => 12.0,
        } * EXAGGERATION;
        let base = to_bevy(s, b.pos, 0.0);
        let source_kind = b.visual_kind.as_deref().unwrap_or("");
        if matches!(
            source_kind,
            "plaza"
                | "square"
                | "pitch"
                | "parking"
                | "assembly"
                | "assembly_area"
                | "lido"
                | "campsite"
                | "harbour"
                | "cemetery"
        ) {
            commands.spawn((
                MaterialMeshBundle {
                    mesh: meshes.add(open_ground(s, b, source_kind)),
                    material: pal.house.clone(),
                    ..default()
                },
                BuildingTag(i),
            ));
            for (model, offset, scale) in match source_kind {
                "plaza" | "square" => vec![
                    ("fountain", [0., 0.], 1.6),
                    ("bench", [b.size[0] * 0.25, b.size[1] * 0.25], 1.8),
                ],
                "assembly" | "assembly_area" => vec![("assembly_sign", [0., b.size[1] * 0.45], 2.)],
                _ => Vec::new(),
            } {
                if let Some(m) = crate::models::urban_model(model) {
                    commands.spawn(MaterialMeshBundle {
                        mesh: meshes.add(crate::models::mesh(m, Vec3::ZERO)),
                        material: pal.house.clone(),
                        transform: Transform::from_translation(to_bevy(
                            s,
                            [b.pos[0] + offset[0], b.pos[1] + offset[1]],
                            0.6,
                        ))
                        .with_scale(Vec3::splat(scale)),
                        ..default()
                    });
                }
            }
            continue;
        }
        let source_model = match source_kind {
            "church" | "chapel" | "fire_station" | "farm" | "barn" | "silo" | "mill"
            | "water_tower" | "substation" | "lighthouse" => Some(source_kind),
            _ => None,
        };
        let landmark = source_model
            .or(match b.kind {
                BuildingKind::School => Some("school"),
                BuildingKind::TownHall => Some("townhall"),
                BuildingKind::FuelStation => Some("fuel"),
                BuildingKind::Industrial => Some("industrial"),
                _ => None,
            })
            .and_then(crate::models::urban_model);
        let (mesh, transform) = if let Some(model) = landmark {
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, model.positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, model.normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, model.colors.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; model.positions.len()]);
            mesh.insert_indices(Indices::U32(model.indices.clone()));
            let bounds = [0, 1, 2].map(|axis| {
                model
                    .positions
                    .iter()
                    .map(|p| p[axis])
                    .fold(f32::NEG_INFINITY, f32::max)
                    - model
                        .positions
                        .iter()
                        .map(|p| p[axis])
                        .fold(f32::INFINITY, f32::min)
            });
            (
                mesh,
                Transform::from_translation(base).with_scale(Vec3::new(
                    b.size[0] / bounds[0].max(1.0),
                    height / bounds[1].max(1.0),
                    b.size[1] / bounds[2].max(1.0),
                )),
            )
        } else {
            (
                crate::architecture::building(b.size, height, i, b.kind == BuildingKind::House),
                Transform::from_translation(base - Vec3::Y),
            )
        };
        commands.spawn((
            MaterialMeshBundle {
                mesh: meshes.add(mesh),
                material: if b.name.is_some() {
                    pal.facility.clone()
                } else {
                    pal.house.clone()
                },
                transform,
                ..default()
            },
            BuildingTag(i),
        ));
    }
    // The physical network is rasterised into TerrainMaterial's distance
    // mask. Operational closures remain separate local ribbons below.
    commands.insert_resource(pal);
    commands.insert_resource(ShowGrid(
        std::env::var("TTX_SNAPSHOT_MARKS").as_deref() != Ok("0"),
    ));
}

/// A flat strip along a polyline, draped on the terrain.
fn ribbon(s: &Scenario, pts: &[Pos], width: f32, lift: f32) -> Mesh {
    let mut samples: Vec<Pos> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = (len / 5.0).ceil().max(1.0) as usize;
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
        // Mitered shared vertices: incoming/outgoing normals meet without gaps.
        // Clamp acute bends to prevent long spikes at hairpins.
        let here = Vec2::from_array(*q);
        let prev = samples.get(k.wrapping_sub(1)).map(|p| Vec2::from_array(*p));
        let next = samples.get(k + 1).map(|p| Vec2::from_array(*p));
        let incoming = prev.map(|p| (here - p).normalize_or_zero());
        let outgoing = next.map(|p| (p - here).normalize_or_zero());
        let a = incoming.or(outgoing).unwrap_or(Vec2::Y);
        let b = outgoing.unwrap_or(a);
        let na = Vec2::new(-a.y, a.x);
        let nb = Vec2::new(-b.y, b.x);
        let bisector = (na + nb).normalize_or_zero();
        let offset = if bisector.length_squared() < 0.1 {
            nb * width * 0.5
        } else {
            bisector * (width * 0.5 / bisector.dot(nb).max(0.5))
        };
        let (nx, ny) = (offset.x, offset.y);
        p.push(to_bevy(s, [q[0] + nx, q[1] + ny], lift).to_array());
        p.push(to_bevy(s, [q[0] - nx, q[1] - ny], lift).to_array());
        n.push([0.0, 1.0, 0.0]);
        n.push([0.0, 1.0, 0.0]);
        if k + 1 < samples.len() {
            let o = (k * 2) as u32;
            ix.extend_from_slice(&[o, o + 1, o + 2, o + 1, o + 3, o + 2]);
        }
    }
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
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
    mut buildings: Query<(&BuildingTag, &mut Handle<StructureMaterial>), Without<RoadTag>>,
    road_layers: Query<Entity, With<RoadStatusLayer>>,
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
    let show_marks = std::env::var("TTX_SNAPSHOT_MARKS").as_deref() != Ok("0");
    for view::Drop { at, radius_m: r } in snap.drops.iter().filter(|_| show_marks) {
        let pts: Vec<Vec3> = (0..=40)
            .map(|k| {
                let a = k as f32 / 40.0 * std::f32::consts::TAU;
                to_bevy(s, [at[0] + r * a.cos(), at[1] + r * a.sin()], 4.0)
            })
            .collect();
        gizmos.linestrip(pts, Color::srgb(0.2, 0.55, 1.0));
    }
    if grid.0 {
        let z = s.zones;
        let warned: std::collections::HashSet<&String> = snap.warned.iter().collect();
        let (w, h) = (s.grid.width_m(), s.grid.height_m());
        for c in 0..=z.cols {
            let x = (c as f32 * z.size_m).min(w - 1.0);
            gizmos.linestrip(
                (0..=80).map(|k| to_bevy(s, [x, k as f32 / 80.0 * h], 3.0)),
                Color::srgba(1.0, 1.0, 1.0, 0.35),
            );
        }
        for r in 0..=z.rows {
            let y = (r as f32 * z.size_m).min(h - 1.0);
            gizmos.linestrip(
                (0..=80).map(|k| to_bevy(s, [k as f32 / 80.0 * w, y], 3.0)),
                Color::srgba(1.0, 1.0, 1.0, 0.35),
            );
        }
        for name in &warned {
            if let Some(b) = s.zone_bounds(name) {
                let ring = [
                    [b[0], b[1]],
                    [b[2], b[1]],
                    [b[2], b[3]],
                    [b[0], b[3]],
                    [b[0], b[1]],
                ];
                let mut pts = Vec::new();
                for wv in ring.windows(2) {
                    for k in 0..=20 {
                        let t = k as f32 / 20.0;
                        pts.push(to_bevy(
                            s,
                            [
                                wv[0][0] + (wv[1][0] - wv[0][0]) * t,
                                wv[0][1] + (wv[1][1] - wv[0][1]) * t,
                            ],
                            6.0,
                        ));
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
        let Some(st) = snap.buildings.get(tag.0) else {
            continue;
        };
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
    // Physical road surface stays visible. State overlays are local segments,
    // never the aggregate cut_by_fire flag of an entire named road.
    for entity in &road_layers {
        commands.entity(entity).despawn();
    }
    for view::RoadSegment { from, to, closed, cut_by_fire: cut, .. } in &snap.road_segments {
        if !show_marks || (!closed && !cut) {
            continue;
        }
        commands.spawn((
            PbrBundle {
                mesh: meshes.add(ribbon(s, &[*from, *to], 5.0, 2.0)),
                material: if *cut {
                    pal.road_cut.clone()
                } else {
                    pal.road_closed.clone()
                },
                ..default()
            },
            RoadStatusLayer,
        ));
    }
    // Fire lines: rebuild when their number changes.
    if snap.lines.len() != seen.1 {
        for e in layers.iter() {
            commands.entity(e).despawn();
        }
        for view::FireLine { from: a, to: b, width_m: w } in &snap.lines {
            commands.spawn((
                PbrBundle {
                    mesh: meshes.add(ribbon(s, &[*a, *b], *w * 0.6, 2.0)),
                    material: pal.line.clone(),
                    ..default()
                },
                LineLayer,
            ));
        }
        seen.1 = snap.lines.len();
    }
    seen.0 = snap.generation;
}

/// Authored open spaces remain open and follow the visual DEM. No decorative
/// vehicles, tents or people are added to the operational model.
fn open_ground(s: &Scenario, b: &scenario::Building, kind: &str) -> Mesh {
    let (nx, nz) = (
        (b.size[0] / 5.).ceil().max(1.) as usize,
        (b.size[1] / 5.).ceil().max(1.) as usize,
    );
    let mut pos = Vec::new();
    let mut colours = Vec::new();
    let mut ix = Vec::new();
    for z in 0..=nz {
        for x in 0..=nx {
            let u = x as f32 / nx as f32;
            let v = z as f32 / nz as f32;
            let at = [
                b.pos[0] + (u - 0.5) * b.size[0],
                b.pos[1] + (0.5 - v) * b.size[1],
            ];
            pos.push(to_bevy(s, at, 0.6).to_array());
            let c = match kind {
                "pitch" => {
                    if ((u * 10.).floor() as usize) % 2 == 0 {
                        [0.30, 0.43, 0.23]
                    } else {
                        [0.34, 0.47, 0.25]
                    }
                }
                "parking" => [0.26, 0.27, 0.28],
                "assembly" | "assembly_area" => [0.62, 0.69, 0.53],
                "cemetery" => [0.58, 0.58, 0.54],
                "lido" => [0.72, 0.66, 0.49],
                _ => [0.69, 0.65, 0.56],
            };
            let c = Color::srgb(c[0], c[1], c[2]).to_linear();
            colours.push([c.red, c.green, c.blue, 1.]);
        }
    }
    for z in 0..nz {
        for x in 0..nx {
            let i = (z * (nx + 1) + x) as u32;
            let d = (nx + 1) as u32;
            ix.extend([i, i + d, i + 1, i + 1, i + d, i + d + 1]);
        }
    }
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; pos.len()]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    m.insert_indices(Indices::U32(ix));
    m.compute_smooth_normals();
    m
}
