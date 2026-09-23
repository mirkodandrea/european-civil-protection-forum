//! Terrain mesh (one vertex per fire cell, coloured by fuel and fire state),
//! vegetation (Blender pines, oaks and bushes merged into chunk meshes that
//! char as the fire passes) and the glowing fire overlay.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use scenario::{fuel, Cell, Scenario};

use crate::frame::{to_bevy, Frame, EXAGGERATION};
use crate::models::model;
use crate::Scn;

const CHUNK: usize = 25;

#[derive(Resource)]
pub struct TerrainHandles {
    terrain: Handle<Mesh>,
    base: Vec<[f32; 4]>,
    fire_mesh: Handle<Mesh>,
    fire_mat: Handle<StandardMaterial>,
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

/// Terrain colour for a cell given its display code.
fn cell_colour(base: [f32; 4], code: u8) -> [f32; 4] {
    match code {
        1 => [1.0, 0.32, 0.05, 1.0],
        2 => [0.10, 0.09, 0.08, 1.0],
        3 => [0.52, 0.40, 0.24, 1.0],
        4 => [base[0] * 0.7, base[1] * 0.8, (base[2] + 0.35).min(1.0), 1.0],
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

    // Fire overlay: unlit, additive quads over burning cells, rebuilt on change.
    let fire_mesh = meshes.add(empty_mesh());
    let fire_mat = mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.55, 0.15), unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, ..default() });
    commands.spawn(PbrBundle { mesh: fire_mesh.clone(), material: fire_mat.clone(), ..default() });
    commands.insert_resource(TerrainHandles { terrain, base, fire_mesh, fire_mat });
}

fn empty_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    m.insert_indices(Indices::U32(Vec::new()));
    m
}

fn chunk_of(s: &Scenario, i: usize) -> usize {
    let g = s.grid;
    (i / g.cols / CHUNK) * g.cols.div_ceil(CHUNK) + (i % g.cols) / CHUNK
}

pub fn update(
    frame: Res<Frame>,
    scn: Res<Scn>,
    h: Option<Res<TerrainHandles>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    chunks: Query<&VegChunk>,
    time: Res<Time>,
) {
    let Some(h) = h else { return };
    let s = &scn.0;
    if frame.cells.is_empty() {
        return;
    }
    // Flicker the fire glow every frame.
    if let Some(m) = mats.get_mut(&h.fire_mat) {
        let t = time.elapsed_seconds();
        let f = 0.8 + 0.2 * (t * 9.0).sin() * (t * 5.3).cos();
        m.base_color = Color::srgb(1.0 * f, 0.5 * f, 0.12 * f);
    }
    if !frame.all_changed && frame.changed.is_empty() {
        return;
    }
    // Terrain colours.
    if let Some(mesh) = meshes.get_mut(&h.terrain) {
        let colours: Vec<[f32; 4]> = h.base.iter().zip(&frame.cells).map(|(b, c)| cell_colour(*b, *c)).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    // Vegetation: only chunks with changed cells.
    let dirty: std::collections::HashSet<usize> = if frame.all_changed { chunks.iter().map(|c| c.chunk).collect() } else { frame.changed.iter().map(|&i| chunk_of(s, i)).collect() };
    for ch in chunks.iter().filter(|c| dirty.contains(&c.chunk)) {
        if let Some(mesh) = meshes.get_mut(&ch.mesh) {
            let colours: Vec<[f32; 4]> = ch.cell.iter().zip(&ch.base).map(|(ci, b)| veg_colour(*b, frame.cells[*ci as usize])).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        }
    }
    // Fire overlay quads.
    let (mut p, mut col, mut ix) = (Vec::new(), Vec::new(), Vec::new());
    let half = s.grid.cellsize * 0.6;
    for (i, &c) in frame.cells.iter().enumerate() {
        if c != 1 {
            continue;
        }
        let cell = Cell { row: i / s.grid.cols, col: i % s.grid.cols };
        let ctr = s.cell_centre(cell);
        let lift = 3.0 + 4.0 * ((i as f32 * 12.9898).sin() * 0.5 + 0.5);
        let off = p.len() as u32;
        for (dx, dy) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
            p.push(to_bevy(s, [ctr[0] + dx, ctr[1] + dy], lift).to_array());
            col.push([1.0, 0.7, 0.3, 1.0]);
        }
        // A vertical flame card too, so the fire reads from a low camera.
        let base = to_bevy(s, ctr, 0.0);
        let top = base + Vec3::Y * (10.0 + 10.0 * ((i as f32 * 78.233).sin() * 0.5 + 0.5)) * EXAGGERATION;
        for v in [base - Vec3::X * half, base + Vec3::X * half, top + Vec3::X * half * 0.3, top - Vec3::X * half * 0.3] {
            p.push(v.to_array());
        }
        col.extend_from_slice(&[[1.0, 0.6, 0.2, 1.0], [1.0, 0.6, 0.2, 1.0], [0.6, 0.15, 0.02, 1.0], [0.6, 0.15, 0.02, 1.0]]);
        for q in [off, off + 4] {
            ix.extend_from_slice(&[q, q + 1, q + 2, q, q + 2, q + 3]);
        }
    }
    if let Some(mesh) = meshes.get_mut(&h.fire_mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(Indices::U32(ix));
    }
}
