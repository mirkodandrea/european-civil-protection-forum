//! Terrain mesh (one vertex per fire cell, coloured by fuel and fire state),
//! vegetation (Blender pines, oaks and bushes merged into chunk meshes that
//! char as the fire passes) and the glowing fire overlay.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use scenario::{fuel, Cell, Scenario};

use crate::frame::{to_bevy, Frame};
use crate::models::model;
use crate::Scn;

const CHUNK: usize = 25;

#[derive(Resource)]
pub struct TerrainHandles {
    terrain: Handle<Mesh>,
    base: Vec<[f32; 4]>,
}

#[derive(Component)]
pub struct VegChunk {
    mesh: Handle<Mesh>,
    /// Per vertex: cell index and base colour.
    cell: Vec<u32>,
    base: Vec<[f32; 4]>,
    chunk: usize,
}

fn fuel_rgb(code: i32) -> [f32; 3] {
    match code {
        fuel::BROADLEAVES => [0.33, 0.45, 0.22],
        fuel::SHRUBS => [0.50, 0.53, 0.28],
        fuel::NON_VEGETATED => [0.74, 0.72, 0.68],
        fuel::GRASSLAND => [0.70, 0.68, 0.38],
        fuel::CONIFERS => [0.20, 0.34, 0.18],
        fuel::AGRO_FORESTRY => [0.62, 0.58, 0.32],
        _ => [0.42, 0.55, 0.36],
    }
}

/// Burnt ground: charcoal to grey ash, varying per cell so a scar is not a flat slab.
pub fn scar_colour(i: usize) -> [f32; 4] {
    // splitmix64: neighbouring cells get unrelated values (no row stripes).
    let mut z = (i as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let h = ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32;
    // Linear colour: 0.01-0.05 is charcoal to dark ash once displayed.
    let k = h * h;
    [0.012 + 0.04 * k, 0.011 + 0.036 * k, 0.010 + 0.032 * k, 1.0]
}

/// Terrain colour for a cell given its display code.
fn cell_colour(i: usize, base: [f32; 4], code: u8) -> [f32; 4] {
    match code {
        1 => [1.0, 0.32, 0.05, 1.0],
        2 => scar_colour(i),
        3 => [0.52, 0.40, 0.24, 1.0],
        4 => [base[0] * 0.7, base[1] * 0.8, (base[2] + 0.35).min(1.0), 1.0],
        // Uncertainty halo: reached by some runs, below the cutoff.
        5 => [base[0] * 0.6 + 0.4, base[1] * 0.75 + 0.1, base[2] * 0.6, 1.0],
        _ => base,
    }
}

fn veg_colour(base: [f32; 4], code: u8) -> [f32; 4] {
    match code {
        1 => [1.0, 0.45, 0.08, 1.0],
        2 => [0.07, 0.06, 0.05, 1.0],
        _ => base,
    }
}

pub fn setup(mut commands: Commands, scn: Res<Scn>, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let s = &scn.0;
    let g = s.grid;
    // Terrain.
    let mut pos = Vec::with_capacity(g.len());
    let mut base = Vec::with_capacity(g.len());
    for row in 0..g.rows {
        for col in 0..g.cols {
            let c = Cell { row, col };
            pos.push(to_bevy(s, s.cell_centre(c), 0.0).to_array());
            let [r, gg, b] = fuel_rgb(s.fuel_at(c));
            // Slight shading noise from elevation keeps large patches from looking flat.
            let k = 0.94 + 0.06 * ((s.elevation_at(c) * 0.37).sin());
            base.push([r * k, gg * k, b * k, 1.0]);
        }
    }
    let mut idx = Vec::new();
    for row in 0..g.rows - 1 {
        for col in 0..g.cols - 1 {
            let i = (row * g.cols + col) as u32;
            let (r, d, dr) = (i + 1, i + g.cols as u32, i + g.cols as u32 + 1);
            idx.extend_from_slice(&[i, d, r, r, d, dr]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, base.clone());
    mesh.insert_indices(Indices::U32(idx));
    mesh.compute_smooth_normals();
    let terrain = meshes.add(mesh);
    commands.spawn(PbrBundle {
        mesh: terrain.clone(),
        material: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() }),
        ..default()
    });

    // Vegetation, merged per chunk.
    let veg_mat = mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    let mut rng = ChaCha8Rng::seed_from_u64(11);
    let chunks_per_row = g.cols.div_ceil(CHUNK);
    for cr in 0..g.rows.div_ceil(CHUNK) {
        for cc in 0..chunks_per_row {
            let (mut p, mut n, mut col, mut ix, mut cells) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for row in cr * CHUNK..((cr + 1) * CHUNK).min(g.rows) {
                for c in cc * CHUNK..((cc + 1) * CHUNK).min(g.cols) {
                    let cell = Cell { row, col: c };
                    let (name, density, height) = match s.fuel_at(cell) {
                        fuel::CONIFERS => ("pine", 0.55, 14.0),
                        fuel::BROADLEAVES | fuel::NON_FIRE_PRONE_FOREST => ("oak", 0.45, 11.0),
                        fuel::SHRUBS => ("bush", 0.35, 3.0),
                        fuel::AGRO_FORESTRY => ("oak", 0.08, 8.0),
                        _ => continue,
                    };
                    if rng.gen::<f32>() > density {
                        continue;
                    }
                    let centre = s.cell_centre(cell);
                    let at = [centre[0] + rng.gen_range(-8.0..8.0), centre[1] + rng.gen_range(-8.0..8.0)];
                    let origin = to_bevy(s, at, -0.3);
                    let scale = height * rng.gen_range(0.75..1.3);
                    let rot = Quat::from_rotation_y(rng.gen_range(0.0..std::f32::consts::TAU));
                    let m = model(name);
                    let off = p.len() as u32;
                    let ci = g.index(cell) as u32;
                    for (k, v) in m.positions.iter().enumerate() {
                        let local = rot * (Vec3::from(*v) * Vec3::new(scale * 0.8, scale, scale * 0.8));
                        p.push((origin + local).to_array());
                        n.push((rot * Vec3::from(m.normals[k])).to_array());
                        col.push(m.colors[k]);
                        cells.push(ci);
                    }
                    ix.extend(m.indices.iter().map(|i| i + off));
                }
            }
            if p.is_empty() {
                continue;
            }
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col.clone());
            mesh.insert_indices(Indices::U32(ix));
            let h = meshes.add(mesh);
            commands.spawn((
                PbrBundle { mesh: h.clone(), material: veg_mat.clone(), ..default() },
                VegChunk { mesh: h, cell: cells, base: col, chunk: cr * chunks_per_row + cc },
            ));
        }
    }

    commands.insert_resource(TerrainHandles { terrain, base });
}

fn chunk_of(s: &Scenario, i: usize) -> usize {
    let g = s.grid;
    (i / g.cols / CHUNK) * g.cols.div_ceil(CHUNK) + (i % g.cols) / CHUNK
}

/// Ground colour of a burning cell by time since the front arrived: bright
/// flame-front orange, then a glowing ember bed, then dark red cooling to the
/// scar colour at burn-out.
pub fn burning_colour(age_s: f32) -> [f32; 4] {
    let stops: [(f32, [f32; 3]); 4] = [(0.0, [1.0, 0.45, 0.08]), (240.0, [0.85, 0.18, 0.03]), (720.0, [0.35, 0.07, 0.02]), (fire::BURNOUT_S as f32, [0.02, 0.018, 0.016])];
    let a = age_s.max(0.0);
    for w in stops.windows(2) {
        if a <= w[1].0 {
            let t = (a - w[0].0) / (w[1].0 - w[0].0);
            let c: Vec<f32> = (0..3).map(|k| w[0].1[k] + (w[1].1[k] - w[0].1[k]) * t).collect();
            return [c[0], c[1], c[2], 1.0];
        }
    }
    [0.02, 0.018, 0.016, 1.0]
}

pub fn update(
    frame: Res<Frame>,
    snap: Res<crate::Snapshot>,
    scn: Res<Scn>,
    h: Option<Res<TerrainHandles>>,
    mut meshes: ResMut<Assets<Mesh>>,
    chunks: Query<&VegChunk>,
    time: Res<Time>,
    mut last_glow: Local<f32>,
) {
    let Some(h) = h else { return };
    let s = &scn.0;
    if frame.cells.is_empty() {
        return;
    }
    // Burning ground changes colour with age even without new cells: refresh ~5x/s.
    let glow_due = time.elapsed_seconds() - *last_glow > 0.2 && frame.cells.contains(&1);
    if !frame.all_changed && frame.changed.is_empty() && !glow_due {
        return;
    }
    *last_glow = time.elapsed_seconds();
    let now = frame.display_s as f32;
    if let Some(mesh) = meshes.get_mut(&h.terrain) {
        let colours: Vec<[f32; 4]> = h
            .base
            .iter()
            .zip(&frame.cells)
            .enumerate()
            .map(|(i, (b, c))| {
                if *c == 1 && snap.arrival.get(i).is_some_and(|a| *a != i32::MIN) {
                    burning_colour(now - snap.arrival[i] as f32)
                } else {
                    cell_colour(i, *b, *c)
                }
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    if !frame.all_changed && frame.changed.is_empty() {
        return;
    }
    // Vegetation: only chunks with changed cells.
    let dirty: std::collections::HashSet<usize> = if frame.all_changed { chunks.iter().map(|c| c.chunk).collect() } else { frame.changed.iter().map(|&i| chunk_of(s, i)).collect() };
    for ch in chunks.iter().filter(|c| dirty.contains(&c.chunk)) {
        if let Some(mesh) = meshes.get_mut(&ch.mesh) {
            let colours: Vec<[f32; 4]> = ch.cell.iter().zip(&ch.base).map(|(ci, b)| veg_colour(*b, frame.cells[*ci as usize])).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn burning_ground_cools_with_age() {
        let fresh = super::burning_colour(0.0);
        let old = super::burning_colour(1000.0);
        assert!(fresh[0] + fresh[1] > old[0] + old[1]);
        assert_eq!(super::burning_colour(99_999.0)[0], 0.02);
        assert_ne!(super::scar_colour(1), super::scar_colour(2));
    }
}
