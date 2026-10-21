//! PBR terrain and vegetation adapted from propagator_abm. Static geometry
//! reads exact-cell terrain state and per-plant fire textures from snapshots;
//! rendering never changes the model or reloads plant vertices when fire moves.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use scenario::{fuel, Cell, Scenario};

use crate::frame::{to_bevy, Frame};
use crate::materials::{FoliageMaterial, TerrainMaterial, ATTRIBUTE_PLANT};
use crate::models::plant;
use crate::Scn;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::texture::ImageSampler;

const CHUNK: usize = 25;

#[derive(Resource)]
pub struct TerrainHandles {
    material: Handle<TerrainMaterial>,
    pub(crate) state: Handle<Image>,
    pub road_mask: Handle<Image>,
}
#[derive(Component)]
pub struct VegChunk {
    state: Handle<Image>,
    material: Handle<FoliageMaterial>,
    /// One model cell per plant; vertices and indices remain immutable.
    cells: Vec<usize>,
    points: Vec<scenario::Pos>,
    centre: Vec3,
}

pub fn setup(
    mut commands: Commands,
    scn: Res<Scn>,
    quality: Res<crate::quality::Quality>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
    mut foliage: ResMut<Assets<FoliageMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let s = &scn.0;
    let g = s.grid;
    // Visual DEM nodes may be finer than the model raster. Fire state is
    // sampled separately from the authoritative cell texture in the shader.
    let stride = if quality.name == "low" {
        4
    } else if quality.name == "projector" {
        2
    } else {
        1
    };
    let (rows, cols) = s
        .render_terrain
        .as_ref()
        .map(|t| (t.rows, t.cols))
        .unwrap_or((g.rows, g.cols));
    let sample = |n: usize| {
        let mut v: Vec<usize> = (0..n)
            .step_by(if s.render_terrain.is_some() {
                stride
            } else {
                1
            })
            .collect();
        if v.last() != Some(&(n - 1)) {
            v.push(n - 1);
        }
        v
    };
    let rr = sample(rows);
    let cc = sample(cols);
    let (mut pos, mut base) = (Vec::<[f32; 3]>::new(), Vec::<[f32; 4]>::new());
    for &row in &rr {
        for &col in &cc {
            let at = if let Some(t) = &s.render_terrain {
                [
                    col as f32 * t.posting_m,
                    t.world_size_m[1] - row as f32 * t.posting_m,
                ]
            } else {
                s.cell_centre(Cell { row, col })
            };
            let cell = s
                .cell_of([
                    at[0].clamp(0.0, g.width_m() - 0.01),
                    at[1].clamp(0.0, g.height_m() - 0.01),
                ])
                .unwrap();
            pos.push(to_bevy(s, at, 0.0).to_array());
            let _ = cell;
            base.push([1., 1., 1., 1.]);
        }
    }
    let mut idx = Vec::new();
    for row in 0..rr.len() - 1 {
        for col in 0..cc.len() - 1 {
            let i = (row * cc.len() + col) as u32;
            let (r, d, dr) = (i + 1, i + cc.len() as u32, i + cc.len() as u32 + 1);
            idx.extend_from_slice(&[i, d, r, r, d, dr]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, base.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; base.len()]);
    mesh.insert_indices(Indices::U32(idx));
    mesh.compute_smooth_normals();
    let terrain = meshes.add(mesh);
    let (road_mask, grass_map, ground_colour, ground_normal) = ground_images(s, &mut images);
    let state = images.add(state_image(g.cols as u32, g.rows as u32));
    let material = mats.add(TerrainMaterial::new(
        Vec4::new(1. / g.width_m(), 1. / g.height_m(), 1., 0.),
        s.render_terrain
            .as_ref()
            .map(|t| {
                Vec4::new(
                    1. / (t.posting_m * t.cols as f32),
                    1. / (t.posting_m * t.rows as f32),
                    0.5,
                    0.5,
                )
            })
            .unwrap_or(Vec4::new(1. / g.width_m(), 1. / g.height_m(), 0.5, 0.5)),
        road_mask.clone(),
        grass_map,
        ground_colour,
        ground_normal,
        state.clone(),
    ));
    commands.spawn(MaterialMeshBundle {
        mesh: terrain,
        material: material.clone(),
        ..default()
    });
    // Vegetation and the fire state use the latest source-game GPU pipeline.
    let mut rng = ChaCha8Rng::seed_from_u64(11);
    let chunks_per_row = g.cols.div_ceil(CHUNK);
    for cr in 0..g.rows.div_ceil(CHUNK) {
        for cc in 0..chunks_per_row {
            let (mut p, mut n, mut col, mut ix) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let (mut plant_ids, mut plant_cells, mut plant_points) =
                (Vec::new(), Vec::new(), Vec::new());
            for row in cr * CHUNK..((cr + 1) * CHUNK).min(g.rows) {
                for c in cc * CHUNK..((cc + 1) * CHUNK).min(g.cols) {
                    let cell = Cell { row, col: c };
                    let (name, density, height) = match visual_fuel(s, s.fuel_at(cell)) {
                        fuel::CONIFERS => ("pine", 2.8, 14.0),
                        fuel::BROADLEAVES | fuel::NON_FIRE_PRONE_FOREST => ("oak", 2.5, 11.0),
                        fuel::SHRUBS => ("bush", 4.5, 3.0),
                        fuel::AGRO_FORESTRY => ("olive", 0.7, 8.0),
                        _ => continue,
                    };
                    let expected = density * quality.vegetation * (g.cellsize / 20.).powi(2);
                    let count = expected.floor() as usize
                        + usize::from(rng.gen::<f32>() < expected.fract());
                    for _ in 0..count {
                        let centre = s.cell_centre(cell);
                        let at = [
                            centre[0] + rng.gen_range(-g.cellsize * 0.48..g.cellsize * 0.48),
                            centre[1] + rng.gen_range(-g.cellsize * 0.48..g.cellsize * 0.48),
                        ];
                        // Keep road surfaces readable; visual thinning only.
                        if s.roads.iter().any(|r| {
                            r.points.windows(2).any(|w| {
                                let a = Vec2::from_array(w[0]);
                                let b = Vec2::from_array(w[1]);
                                let d = b - a;
                                let t = ((Vec2::from_array(at) - a).dot(d)
                                    / d.length_squared().max(1.0))
                                .clamp(0.0, 1.0);
                                Vec2::from_array(at).distance(a + d * t) < 13.0
                            })
                        }) {
                            continue;
                        }
                        let origin = to_bevy(s, at, -0.3);
                        let scale = height * rng.gen_range(0.75..1.3);
                        let spread = rng.gen_range(0.65..1.0);
                        let rot = Quat::from_rotation_y(rng.gen_range(0.0..std::f32::consts::TAU));
                        let variety = plant(name, rng.gen::<f32>());
                        let ci = plant_cells.len() as u32;
                        plant_cells.push(g.index(cell));
                        plant_points.push(at);
                        for (part, m) in [(0, &variety.live), (2, &variety.remains)] {
                            let off = p.len() as u32;
                            for (k, v) in m.positions.iter().enumerate() {
                                let local = rot
                                    * (Vec3::from(*v)
                                        * Vec3::new(scale * spread, scale, scale * spread));
                                p.push((origin + local).to_array());
                                n.push((rot * Vec3::from(m.normals[k])).to_array());
                                let c = m.colors[k];
                                let flag = if part == 2 {
                                    variety.debris[k]
                                } else {
                                    !variety.wood[k]
                                };
                                col.push([c[0], c[1], c[2], if flag { 1. } else { 0. }]);
                                plant_ids.push(ci | if part == 2 { 0x80000000 } else { 0 });
                            }
                            ix.extend(m.indices.iter().map(|i| i + off));
                        }
                    }
                }
            }
            if p.is_empty() {
                continue;
            }
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; p.len()]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            mesh.insert_attribute(ATTRIBUTE_PLANT, plant_ids);
            mesh.insert_indices(Indices::U32(ix));
            let h = meshes.add(mesh);
            let state = images.add(state_image(256, plant_cells.len().div_ceil(256) as u32));
            let material = foliage.add(FoliageMaterial::new(state.clone()));
            commands.spawn((
                MaterialMeshBundle {
                    mesh: h,
                    material: material.clone(),
                    ..default()
                },
                VegChunk {
                    state,
                    material,
                    cells: plant_cells,
                    points: plant_points,
                    centre: to_bevy(
                        s,
                        [
                            ((cc * CHUNK) as f32 + CHUNK as f32 * 0.5) * g.cellsize,
                            g.height_m() - ((cr * CHUNK) as f32 + CHUNK as f32 * 0.5) * g.cellsize,
                        ],
                        0.0,
                    ),
                },
            ));
        }
    }

    commands.insert_resource(TerrainHandles {
        material,
        state,
        road_mask,
    });
}

/// Refresh only tiny state textures. Reset/rollback is derived from the
/// snapshot, with no persistent burn latch or randomised replacement geometry.
pub fn update(
    frame: Res<Frame>,
    style: Res<crate::fire_field::FirePresentation>,
    scn: Res<Scn>,
    snap: Res<crate::Snapshot>,
    h: Option<Res<TerrainHandles>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut foliage: ResMut<Assets<FoliageMaterial>>,
    chunks: Query<&VegChunk>,
    mut seen: Local<Option<(u64, i64, bool)>>,
) {
    let Some(h) = h else { return };
    let now = frame.display_s as f32;
    if let Some(m) = terrain.get_mut(&h.material) {
        m.set_time(now);
        m.set_continuous_fire(style.continuous);
    }
    if frame.cells.is_empty() {
        return;
    }
    for chunk in &chunks {
        if let Some(m) = foliage.get_mut(&chunk.material) {
            m.fire = Vec4::new(0., now, 240., 0.);
        }
    }
    if *seen == Some((snap.generation, frame.display_s, style.continuous)) {
        return;
    }
    *seen = Some((snap.generation, frame.display_s, style.continuous));
    if let Some(img) = images.get_mut(&h.state) {
        for (i, code) in frame.cells.iter().enumerate() {
            let arrival = snap
                .arrival
                .get(i)
                .copied()
                .filter(|a| *a != i32::MIN)
                .unwrap_or(snap.time_s as i32) as f32;
            put_state(&mut img.data, i, *code as f32, arrival);
        }
    }
    for chunk in &chunks {
        if let Some(m) = foliage.get_mut(&chunk.material) {
            m.fire = Vec4::new(0., now, 240., 0.);
        }
        if let Some(img) = images.get_mut(&chunk.state) {
            for (j, &i) in chunk.cells.iter().enumerate() {
                let arrival = snap
                    .arrival
                    .get(i)
                    .copied()
                    .filter(|a| *a != i32::MIN)
                    .unwrap_or(snap.time_s as i32) as f32;
                let (code, arrival) = if style.continuous {
                    let field = crate::fire_field::FireField {
                        snap: &snap,
                        grid: scn.0.grid,
                    };
                    let p = chunk.points[j];
                    let sample = field.sample(p);
                    if frame.cells[i] == 3 {
                        (4, arrival)
                    } else if sample.coverage > 0.5 && sample.arrival.is_some_and(|a| a <= now) {
                        let a = sample.arrival.unwrap();
                        (plant_state(1, now - a), a)
                    } else {
                        (0, arrival)
                    }
                } else {
                    (plant_state(frame.cells[i], now - arrival), arrival)
                };
                put_state(&mut img.data, j, code as f32, arrival);
            }
        }
    }
}
fn plant_state(code: u8, age: f32) -> u8 {
    match code {
        1 if age < 240. => 1,
        1 | 2 => 2,
        3 => 4,
        _ => 0,
    }
}
fn put_state(data: &mut [u8], i: usize, code: f32, arrival: f32) {
    data[i * 8..i * 8 + 4].copy_from_slice(&code.to_le_bytes());
    data[i * 8 + 4..i * 8 + 8].copy_from_slice(&arrival.to_le_bytes());
}
fn state_image(w: u32, h: u32) -> Image {
    Image::new_fill(
        Extent3d {
            width: w,
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rg32Float,
        RenderAssetUsages::default(),
    )
}
fn ground_images(
    s: &Scenario,
    images: &mut Assets<Image>,
) -> (Handle<Image>, Handle<Image>, Handle<Image>, Handle<Image>) {
    let g = s.grid;
    let mut grass = Vec::new();
    let mut colours = Vec::new();
    let mut normals = Vec::new();
    for row in 0..g.rows {
        for col in 0..g.cols {
            let f = visual_fuel(s, s.fuel_at(Cell { row, col }));
            let meadow = matches!(f, fuel::GRASSLAND | fuel::SHRUBS | fuel::AGRO_FORESTRY);
            grass.extend_from_slice(&[if meadow { 220 } else { 30 }, 0, 0, 255]);
        }
    }
    // Normals are sampled at the supplied visual DEM resolution, independently
    // of mesh LOD. Fine ridges retain their light even on the projector mesh.
    let (rows, cols) = s
        .render_terrain
        .as_ref()
        .map(|t| (t.rows, t.cols))
        .unwrap_or((g.rows, g.cols));
    for row in 0..rows {
        for col in 0..cols {
            let at = s
                .render_terrain
                .as_ref()
                .map(|t| {
                    [
                        col as f32 * t.posting_m,
                        t.world_size_m[1] - row as f32 * t.posting_m,
                    ]
                })
                .unwrap_or_else(|| s.cell_centre(Cell { row, col }));
            let cell = s
                .cell_of([
                    at[0].clamp(0., g.width_m() - 0.01),
                    at[1].clamp(0., g.height_m() - 0.01),
                ])
                .unwrap();
            let f = visual_fuel(s, s.fuel_at(cell));
            let soil = match f {
                fuel::NON_VEGETATED => [0.56, 0.53, 0.46],
                fuel::CONIFERS => [0.29, 0.31, 0.21],
                fuel::BROADLEAVES => [0.37, 0.37, 0.25],
                _ => [0.53, 0.49, 0.34],
            };
            let h = crate::frame::height_at(s, at);
            let k = 0.94 + 0.06 * (h * 0.037 + at[0] * 0.003).sin();
            colours.extend(soil.map(|v| (v * k * 255.).clamp(0., 255.) as u8));
            colours.push(255);
            let d = s.render_terrain.as_ref().map(|t| t.posting_m).unwrap_or(5.);
            let ht = |x, y| crate::frame::height_at(s, [x, y]) * crate::frame::EXAGGERATION;
            let n = Vec3::new(
                (ht(at[0] - d, at[1]) - ht(at[0] + d, at[1])) / (2. * d),
                1.,
                (ht(at[0], at[1] + d) - ht(at[0], at[1] - d)) / (2. * d),
            )
            .normalize();
            normals.extend(n.to_array().map(|v| ((v * 0.5 + 0.5) * 255.) as u8));
            normals.push(255);
        }
    }
    let make = |w, h, data, format| {
        let mut i = Image::new(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            format,
            RenderAssetUsages::default(),
        );
        i.sampler = ImageSampler::linear();
        i
    };
    let grass = images.add(make(
        g.cols as u32,
        g.rows as u32,
        grass,
        TextureFormat::Rgba8Unorm,
    ));
    let colour = images.add(make(
        cols as u32,
        rows as u32,
        colours,
        TextureFormat::Rgba8UnormSrgb,
    ));
    let normal = images.add(make(
        cols as u32,
        rows as u32,
        normals,
        TextureFormat::Rgba8Unorm,
    ));
    // Signed distances to the actual network. Rasterise only each segment's
    // expanded bounds, avoiding a road-network scan for every terrain pixel.
    let w = (g.width_m() / 4.).ceil().clamp(1., 2048.) as usize;
    let h = (g.height_m() / 4.).ceil().clamp(1., 2048.) as usize;
    let dx = g.width_m() / w as f32;
    let dy = g.height_m() / h as f32;
    let mut dist = vec![[12f32; 2]; w * h];
    for road in &s.roads {
        let channel = if road.drivable { 0 } else { 1 };
        let half = if !road.drivable {
            1.5
        } else if matches!(road.class.as_str(), "primary" | "secondary" | "trunk") {
            5.5
        } else {
            3.5
        };
        for seg in road.points.windows(2) {
            let a = Vec2::new(seg[0][0], g.height_m() - seg[0][1]);
            let b = Vec2::new(seg[1][0], g.height_m() - seg[1][1]);
            let lo = a.min(b) - Vec2::splat(half + 12.);
            let hi = a.max(b) + Vec2::splat(half + 12.);
            for y in ((lo.y / dy).floor().max(0.) as usize)
                ..=((hi.y / dy).ceil().max(0.) as usize).min(h - 1)
            {
                for x in ((lo.x / dx).floor().max(0.) as usize)
                    ..=((hi.x / dx).ceil().max(0.) as usize).min(w - 1)
                {
                    let p = Vec2::new((x as f32 + 0.5) * dx, (y as f32 + 0.5) * dy);
                    let v = b - a;
                    let t = ((p - a).dot(v) / v.length_squared().max(0.01)).clamp(0., 1.);
                    let d = p.distance(a + t * v) - half;
                    dist[y * w + x][channel] = dist[y * w + x][channel].min(d);
                }
            }
        }
    }
    let data = dist
        .iter()
        .flat_map(|d| {
            [
                ((d[0] / 12. * 0.5 + 0.5).clamp(0., 1.) * 255.) as u8,
                ((d[1] / 12. * 0.5 + 0.5).clamp(0., 1.) * 255.) as u8,
                0,
                0,
            ]
        })
        .collect();
    let mask = images.add(make(w as u32, h as u32, data, TextureFormat::Rgba8Unorm));
    (mask, grass, colour, normal)
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_state_distinguishes_actual_fire_halo_clearing_and_rollback() {
        assert_eq!(super::plant_state(5, 10.), 0); // uncertainty is not ignition
        assert_eq!(super::plant_state(1, 0.), 1);
        assert_eq!(super::plant_state(1, 239.), 1);
        assert_eq!(super::plant_state(1, 240.), 2);
        assert_eq!(super::plant_state(2, 1200.), 2);
        assert_eq!(super::plant_state(3, 0.), 4); // removed fuel hides every layer
        assert_eq!(super::plant_state(0, 0.), 0); // rollback has no burn latch
    }
    #[test]
    fn state_texture_uses_two_f32_values_per_plant_without_losing_ignition_time() {
        let mut img = super::state_image(256, 2);
        super::put_state(&mut img.data, 257, 1., 3721.);
        assert_eq!(
            f32::from_le_bytes(img.data[257 * 8..257 * 8 + 4].try_into().unwrap()),
            1.
        );
        assert_eq!(
            f32::from_le_bytes(img.data[257 * 8 + 4..258 * 8].try_into().unwrap()),
            3721.
        );
    }
}

/// EU12 ids overlap legacy ids; rendering must interpret the scenario's table.
fn visual_fuel(s: &Scenario, id: i32) -> i32 {
    if s.fuel_definitions.is_empty() {
        return id;
    }
    let Some(f) = s.fuel_definitions.iter().find(|f| f.id == id) else {
        return fuel::NON_VEGETATED;
    };
    if !f.burn {
        fuel::NON_VEGETATED
    } else if f.name.contains("conifer") {
        fuel::CONIFERS
    } else if f.name.contains("broadleav") {
        fuel::BROADLEAVES
    } else if f.name.contains("shrub") {
        fuel::SHRUBS
    } else {
        fuel::GRASSLAND
    }
}

/// Chunk visibility is presentation-only; ground fuel/fire remains visible at every distance.
pub fn lod(
    quality: Res<crate::quality::Quality>,
    cam: Query<&Transform, With<Camera3d>>,
    mut chunks: Query<(&VegChunk, &mut Visibility)>,
) {
    let Ok(cam) = cam.get_single() else { return };
    for (chunk, mut visible) in &mut chunks {
        *visible = if cam.translation.distance(chunk.centre) < quality.vegetation_distance {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
