//! Night lights: street lamps along the drivable roads of the villages, each
//! with a warm pool of light on the road. Lit windows are in `objects.rs`
//! (`night_windows`) and `shaders/structure.wgsl`. Presentation only.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;

use crate::frame::to_bevy;
use crate::sky::SkyEnv;
use crate::Scn;

/// Metres between lamps along a road.
const SPACING_M: f32 = 38.0;
/// Lamps only where people live: within this distance of a building.
const VILLAGE_M: f32 = 90.0;
/// Pole height and pool radius, metres.
const POLE_M: f32 = 6.5;
const POOL_M: f32 = 16.0;

/// 0 by day, 1 in the dark: lights come on as the sun drops below ~6 degrees
/// and are at full strength by -4.
pub fn night_factor(sky: &SkyEnv) -> f32 {
    ((6.0 - sky.el_deg) / 10.0).clamp(0.0, 1.0)
}

#[derive(Resource)]
pub struct Lamps {
    head: Handle<StandardMaterial>,
    pool: Handle<StandardMaterial>,
}

fn push_box(m: &mut (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>), c: Vec3, h: Vec3) {
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let (mut du, mut dv) = (Vec3::ZERO, Vec3::ZERO);
            du[u] = h[u];
            dv[v] = h[v];
            let o = m.0.len() as u32;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                m.0.push((c + n * h[axis] + du * a + dv * b).to_array());
                m.1.push(n.to_array());
            }
            // Winding so the face points along `n`.
            if sign * (du.cross(dv)[axis]) > 0.0 {
                m.2.extend([o, o + 1, o + 2, o, o + 2, o + 3]);
            } else {
                m.2.extend([o, o + 2, o + 1, o, o + 3, o + 2]);
            }
        }
    }
}

fn mesh_of(m: (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>)) -> Mesh {
    let n = m.0.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, m.0);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.1);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; n]);
    mesh.insert_indices(Indices::U32(m.2));
    mesh
}

pub fn setup(
    mut commands: Commands,
    scn: Res<Scn>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let s = &scn.0;
    let near_village = |p: [f32; 2]| {
        s.buildings.iter().any(|b| {
            (b.pos[0] - p[0]).powi(2) + (b.pos[1] - p[1]).powi(2) < VILLAGE_M * VILLAGE_M
        })
    };
    let mut poles = (Vec::new(), Vec::new(), Vec::new());
    let mut heads = (Vec::new(), Vec::new(), Vec::new());
    let (mut pool_p, mut pool_c, mut pool_ix) = (Vec::new(), Vec::new(), Vec::new());
    let mut count = 0usize;
    for road in s.roads.iter().filter(|r| r.drivable) {
        let half = if matches!(road.class.as_str(), "primary" | "secondary" | "trunk") {
            5.5
        } else {
            3.5
        };
        let mut carry = SPACING_M * 0.5;
        for w in road.points.windows(2) {
            let (a, b) = (Vec2::from_array(w[0]), Vec2::from_array(w[1]));
            let len = a.distance(b);
            if len < 1.0 {
                continue;
            }
            let dir = (b - a) / len;
            let side = Vec2::new(-dir.y, dir.x);
            let mut d = carry;
            while d < len {
                let on_road = a + dir * d;
                let at = on_road + side * (half + 1.5) * if count % 2 == 0 { 1.0 } else { -1.0 };
                if near_village(on_road.to_array()) {
                    let base = to_bevy(s, at.to_array(), 0.0);
                    push_box(&mut poles, base + Vec3::Y * POLE_M * 0.5, Vec3::new(0.12, POLE_M * 0.5, 0.12));
                    push_box(&mut heads, base + Vec3::Y * (POLE_M + 0.15), Vec3::new(0.55, 0.15, 0.55));
                    // Pool: a fan draped on the ground, bright in the middle.
                    let o = pool_p.len() as u32;
                    pool_p.push(to_bevy(s, on_road.to_array(), 0.45).to_array());
                    pool_c.push([1.0, 1.0, 1.0, 1.0]);
                    const N: u32 = 14;
                    for k in 0..N {
                        let t = k as f32 / N as f32 * std::f32::consts::TAU;
                        let q = on_road + Vec2::new(t.cos(), t.sin()) * POOL_M;
                        pool_p.push(to_bevy(s, q.to_array(), 0.45).to_array());
                        pool_c.push([1.0, 1.0, 1.0, 0.0]);
                        pool_ix.extend([o, o + 1 + k, o + 1 + (k + 1) % N]);
                    }
                    count += 1;
                }
                d += SPACING_M;
            }
            carry = d - len;
        }
    }
    if count == 0 {
        return;
    }
    let pole = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.12, 0.13),
        perceptual_roughness: 0.6,
        metallic: 0.6,
        ..default()
    });
    let head = mats.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2),
        emissive: LinearRgba::BLACK,
        ..default()
    });
    let pool = mats.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.72, 0.38, 0.0),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    let mut pool_mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    pool_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pool_p);
    pool_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, pool_c);
    pool_mesh.insert_indices(Indices::U32(pool_ix));
    commands.spawn(PbrBundle {
        mesh: meshes.add(mesh_of(poles)),
        material: pole,
        ..default()
    });
    commands.spawn(PbrBundle {
        mesh: meshes.add(mesh_of(heads)),
        material: head.clone(),
        ..default()
    });
    commands.spawn(PbrBundle {
        mesh: meshes.add(pool_mesh),
        material: pool.clone(),
        ..default()
    });
    commands.insert_resource(Lamps { head, pool });
}

pub fn update(
    sky: Res<SkyEnv>,
    lamps: Option<Res<Lamps>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut last: Local<f32>,
) {
    let Some(lamps) = lamps else { return };
    let night = (night_factor(&sky) * 50.0).round() / 50.0;
    if night == *last {
        return;
    }
    *last = night;
    if let Some(m) = mats.get_mut(&lamps.head) {
        m.emissive = LinearRgba::new(9.0, 6.0, 2.6, 0.0) * night;
    }
    if let Some(m) = mats.get_mut(&lamps.pool) {
        m.base_color = Color::srgba(1.0, 0.72, 0.38, 0.24 * night);
    }
}
