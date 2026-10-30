//! Suppression and evacuation made visible: water released by the air tankers,
//! the engines' jets, wetted ground, and evacuees driving out by car and bus.
//!
//! Presentation only, derived from the view state and animated on real time.
//! The shapes follow what the model recorded (where and when a drop fell,
//! which engine is working, which buildings are being evacuated and towards
//! which shelter) without claiming its precision: a drop is a pass of a few
//! seconds whatever the simulation speed, and evacuating cars drive the road
//! network at a legible pace. When the model simulates evacuation traffic
//! itself (`ViewState::traffic_model`) its vehicles are drawn instead, by
//! [`crate::deployments`].
//!
//! The water and dust are pooled puffs: one shared sphere mesh, a handful of
//! materials, scale as the only per-puff state the renderer sees.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::{
    mesh::{Indices, PrimitiveTopology},
    render_asset::RenderAssetUsages,
};
use scenario::{Pos, Scenario};
use view::{cell, TaskStatus};

use crate::deployments::DeploymentPalette;
use crate::frame::{height_at, to_bevy, EXAGGERATION};
use crate::{Scn, Snapshot};

/// More puffs than this and new ones are skipped (the oldest still fade out).
pub(crate) const MAX_PUFFS: usize = 2400;
/// Drops older than this (simulated seconds) when first seen are not replayed,
/// so joining a running exercise does not set off its whole history.
const REPLAY_WINDOW_S: i64 = 900;
/// Real seconds of a tanker pass, and how far it flies either side of the drop.
const PASS_S: f32 = 7.0;
const PASS_HALF_M: f32 = 650.0;
const PASS_ALT_M: f32 = 95.0;
/// How far short of the target the release is centred, metres.
const RELEASE_LEAD_M: f32 = 35.0;
/// How far an engine's jet reaches for burning cells, metres.
const JET_REACH_M: f32 = 95.0;
/// Height of the nozzle above the ground (engine drawn at symbol scale), metres.
const NOZZLE_M: f32 = 9.0;
const ARC_SEGMENTS: usize = 24;
/// Evacuating vehicles: real-time speed, spacing between departures, and the
/// lane offset to the right of the road centre line.
const CAR_MPS: f32 = 42.0;
const CAR_EVERY_S: f32 = 1.4;
const LANE_M: f32 = 2.6;
const MAX_CARS: usize = 160;
/// Road within this distance of the fire is avoided by evacuees, metres.
const FIRE_AVOID_M: f32 = 700.0;

#[derive(Resource)]
pub struct EffectAssets {
    puff: Handle<Mesh>,
    water: Handle<StandardMaterial>,
    pub(crate) mist: Handle<StandardMaterial>,
    pub(crate) steam: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    pub(crate) jet: Handle<StandardMaterial>,
    wet: Handle<StandardMaterial>,
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mut unlit = |c: LinearRgba| {
        mats.add(StandardMaterial {
            base_color: Color::LinearRgba(c),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    commands.insert_resource(EffectAssets {
        puff: meshes.add(Sphere::new(1.0).mesh().ico(1).expect("small icosphere")),
        // Overbright: unlit output goes through the tone mapper, which greys
        // anything at or below 1.0 against the terrain.
        water: unlit(LinearRgba::new(1.1, 1.45, 2.0, 0.3)),
        mist: unlit(LinearRgba::new(1.3, 1.4, 1.55, 0.14)),
        steam: unlit(LinearRgba::new(1.05, 1.05, 1.08, 0.3)),
        dust: unlit(LinearRgba::new(0.55, 0.42, 0.28, 0.35)),
        jet: unlit(LinearRgba::new(0.55, 1.3, 2.6, 0.6)),
        // Deliberately faint: busy aircraft wet a hundred cells in an hour, and
        // any stronger it is a lake painted over the hillside.
        wet: unlit(LinearRgba::new(0.15, 0.45, 1.1, 0.13)),
    });
}

/// A small deterministic noise source (no RNG dependency in the renderer).
#[derive(Default)]
pub struct Noise(u32);
impl Noise {
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    pub(crate) fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

#[derive(Component)]
pub struct Puff {
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
    grow: f32,
    gravity: f32,
    drag: f32,
    floor: f32,
}

pub(crate) struct PuffSpec {
    pub at: Vec3,
    pub vel: Vec3,
    pub size: f32,
    pub grow: f32,
    pub life: f32,
    pub gravity: f32,
    pub drag: f32,
}

pub(crate) fn spawn_puff(
    commands: &mut Commands,
    fx: &EffectAssets,
    material: &Handle<StandardMaterial>,
    floor: f32,
    p: PuffSpec,
) {
    commands.spawn((
        PbrBundle {
            mesh: fx.puff.clone(),
            material: material.clone(),
            transform: Transform::from_translation(p.at).with_scale(Vec3::splat(p.size)),
            ..default()
        },
        bevy::pbr::NotShadowCaster,
        Puff {
            vel: p.vel,
            age: 0.0,
            life: p.life,
            size: p.size,
            grow: p.grow,
            gravity: p.gravity,
            drag: p.drag,
            floor,
        },
    ));
}

/// Integrate the puffs: fall (or rise), slow down, spread on the ground, fade.
pub fn puffs(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Puff, &mut Transform)>,
) {
    let dt = time.delta_seconds().min(0.1);
    for (e, mut p, mut tf) in &mut q {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= p.gravity * dt;
        let damp = (1.0 - p.drag * dt).max(0.0);
        p.vel *= damp;
        tf.translation += p.vel * dt;
        if tf.translation.y < p.floor {
            // Water hitting the ground bursts sideways into spray.
            tf.translation.y = p.floor;
            p.vel = Vec3::new(p.vel.x * 0.25, 0.0, p.vel.z * 0.25);
            p.gravity = 0.0;
            p.grow = p.grow * 1.5 + 2.5;
            // Spray dies away quickly once it is down.
            p.life = p.life.min(p.age + 1.4);
        }
        let f = p.age / p.life;
        let fade = if f > 0.55 {
            1.0 - (f - 0.55) / 0.45
        } else {
            1.0
        };
        let r = ((p.size + p.grow * p.age) * fade).max(0.01);
        // Falling water streaks: stretched along the fall.
        let streak = if p.gravity > 0.0 {
            1.0 + (-p.vel.y).max(0.0) / 10.0
        } else {
            1.0
        };
        tf.scale = Vec3::new(r, r * streak, r);
    }
}

/// The ground height (drawn, exaggerated) under a Bevy point.
fn ground_y(s: &Scenario, v: Vec3) -> f32 {
    height_at(s, crate::frame::to_world(s, v)) * EXAGGERATION
}

// ---------------------------------------------------------------- air drops

#[derive(Component)]
pub struct GhostTanker;

struct DropAnim {
    at: Pos,
    radius_m: f32,
    /// Flight direction, world metres (x east, y north), unit length.
    dir: Vec2,
    start: f32,
    /// The pass is flown by a stand-in aircraft (no deployment is there to fly it).
    ghost: Option<Entity>,
    /// Altitude above the ground the water is released from.
    alt: f32,
}

#[derive(Default)]
pub struct DropState {
    scene: usize,
    seen: usize,
    active: Vec<DropAnim>,
    noise: Noise,
}

/// Each new water drop is replayed as a pass: the tanker runs in along its
/// approach (into the wind when nothing better is known) and releases a
/// curtain of water that falls, spreads into mist and drifts away.
#[allow(clippy::too_many_arguments)]
pub fn water_drops(
    mut commands: Commands,
    time: Res<Time>,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    fx: Option<Res<EffectAssets>>,
    pal: Option<Res<DeploymentPalette>>,
    mut st: Local<DropState>,
    mut ghosts: Query<&mut Transform, With<GhostTanker>>,
    puffs: Query<(), With<Puff>>,
) {
    let (Some(fx), Some(pal)) = (fx, pal) else {
        return;
    };
    let s = &scn.0;
    let now = time.elapsed_seconds();
    let scene = Arc::as_ptr(&scn.0) as usize;
    if st.scene != scene {
        *st = DropState { scene, ..default() };
    }
    if snap.drops.len() < st.seen {
        st.seen = snap.drops.len(); // rolled back
    }
    let mut stagger = 0.0;
    for d in &snap.drops[st.seen..] {
        if snap.time_s - d.t_s > REPLAY_WINDOW_S {
            continue;
        }
        // The aircraft that made the drop, if it is on the map: fly its heading.
        let tanker = snap.deployments.iter().find(|t| {
            t.resource == "air_tankers"
                && (t.target[0] - d.at[0]).hypot(t.target[1] - d.at[1]) < 60.0
        });
        let dir = match tanker {
            Some(t) => Vec2::new(t.target[0] - t.origin[0], t.target[1] - t.origin[1]),
            None => Vec2::ZERO,
        };
        let dir = if dir.length() > 1.0 {
            dir.normalize()
        } else {
            // Into the wind: it blows *from* wind_dir_deg (clockwise from north).
            let a = snap
                .weather
                .map_or(0.0, |w| w.wind_dir_deg as f32)
                .to_radians();
            Vec2::new(a.sin(), a.cos())
        };
        let flown_by_deployment = tanker.is_some_and(|t| {
            let p = t.position(snap.time_s);
            (p[0] - d.at[0]).hypot(p[1] - d.at[1]) < 400.0
        });
        let ghost = (!flown_by_deployment).then(|| {
            let (mesh, material) = pal.body("tanker");
            commands
                .spawn((
                    MaterialMeshBundle {
                        mesh,
                        material,
                        transform: Transform::from_scale(Vec3::splat(2.8)),
                        visibility: Visibility::Hidden,
                        ..default()
                    },
                    GhostTanker,
                ))
                .id()
        });
        st.active.push(DropAnim {
            at: d.at,
            radius_m: d.radius_m,
            dir,
            start: now + stagger,
            ghost,
            alt: if flown_by_deployment {
                115.0
            } else {
                PASS_ALT_M
            },
        });
        stagger += 2.5;
    }
    st.seen = snap.drops.len();

    let mut budget = MAX_PUFFS.saturating_sub(puffs.iter().count());
    let DropState { active, noise, .. } = &mut *st;
    active.retain(|a| {
        let t = now - a.start;
        if t > PASS_S + 0.5 {
            if let Some(g) = a.ghost {
                commands.entity(g).despawn_recursive();
            }
            return false;
        }
        if t < 0.0 {
            return true;
        }
        let along = -PASS_HALF_M + 2.0 * PASS_HALF_M * (t / PASS_S);
        let p = [a.at[0] + a.dir.x * along, a.at[1] + a.dir.y * along];
        // Clear the terrain along the whole pass, not just at the target.
        let ground = crate::frame::flight_floor(s, p, a.dir);
        let plane = to_bevy(s, p, 0.0).with_y(ground + a.alt);
        if let Some(Ok(mut tf)) = a.ghost.map(|g| ghosts.get_mut(g)) {
            tf.translation = plane;
            tf.rotation = Quat::from_rotation_y(a.dir.x.atan2(-a.dir.y));
            commands
                .entity(a.ghost.unwrap())
                .insert(Visibility::Inherited);
        }
        // Release just short of the target (the water carries forward as it
        // falls): a curtain as long as the drop is wide.
        if (along + RELEASE_LEAD_M).abs() < a.radius_m * 1.2 {
            let fwd = Vec3::new(a.dir.x, 0.0, -a.dir.y);
            for _ in 0..5 {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let at = plane - Vec3::Y * 4.0
                    + fwd * noise.signed() * 6.0
                    + fwd.cross(Vec3::Y) * noise.signed() * 5.0;
                spawn_puff(
                    &mut commands,
                    &fx,
                    if noise.next() < 0.75 {
                        &fx.water
                    } else {
                        &fx.mist
                    },
                    ground_y(s, at) + 2.0,
                    PuffSpec {
                        at,
                        vel: fwd * (22.0 + 8.0 * noise.next())
                            + Vec3::new(noise.signed() * 3.0, -25.0, noise.signed() * 3.0),
                        size: 1.6 + noise.next() * 1.2,
                        grow: 1.5 + noise.next() * 1.5,
                        life: 3.4 + noise.next() * 1.4,
                        gravity: 55.0,
                        drag: 0.3,
                    },
                );
            }
        }
        true
    });
}

// ---------------------------------------------------------------- engines

/// The arc of water one working engine throws, rewritten every frame.
#[derive(Component)]
pub struct Jet {
    task: u64,
}

/// Where an engine at `p` aims: the hottest burning cells within reach, or
/// towards the nearest fire when none is in reach, or `None` with no fire about.
fn jet_targets(s: &Scenario, snap: &Snapshot, p: Pos) -> Vec<Pos> {
    let g = s.grid;
    let Some(c) = s.cell_of(p) else {
        return Vec::new();
    };
    let mut near: Vec<(f32, Pos)> = Vec::new();
    let mut nearest: Option<(f32, Pos)> = None;
    let span = (400.0 / g.cellsize).ceil() as isize;
    for dr in -span..=span {
        for dc in -span..=span {
            let (r, k) = (c.row as isize + dr, c.col as isize + dc);
            if r < 0 || k < 0 || r >= g.rows as isize || k >= g.cols as isize {
                continue;
            }
            let cell = scenario::Cell {
                row: r as usize,
                col: k as usize,
            };
            let i = g.index(cell);
            if snap.cells.get(i) != Some(&cell::BURNING) {
                continue;
            }
            let q = s.cell_centre(cell);
            let d = (q[0] - p[0]).hypot(q[1] - p[1]);
            if d <= JET_REACH_M {
                near.push((snap.intensity.get(i).copied().unwrap_or(0.0), q));
            }
            if nearest.is_none_or(|(best, _)| d < best) {
                nearest = Some((d, q));
            }
        }
    }
    if !near.is_empty() {
        near.sort_by(|a, b| b.0.total_cmp(&a.0));
        return near.into_iter().take(4).map(|(_, q)| q).collect();
    }
    // Out of reach: the crew wets the fuel on the side the fire comes from.
    let Some((d, q)) = nearest else {
        return Vec::new();
    };
    let u = [(q[0] - p[0]) / d, (q[1] - p[1]) / d];
    let side = [-u[1], u[0]];
    (-1..=1)
        .map(|k| {
            let k = k as f32 * 18.0;
            [
                p[0] + u[0] * 45.0 + side[0] * k,
                p[1] + u[1] * 45.0 + side[1] * k,
            ]
        })
        .collect()
}

/// A ribbon along `points` (position, width): two quads crossed, one flat and
/// one upright, so the stream reads as a body of water from any side.
pub(crate) fn arc_mesh(points: &[(Vec3, f32)]) -> Mesh {
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let n = points.len();
    for (i, &(p, w)) in points.iter().enumerate() {
        let a = points[i.saturating_sub(1)].0;
        let b = points[(i + 1).min(n - 1)].0;
        let dir = (b - a).normalize_or_zero();
        let side = dir.cross(Vec3::Y).normalize_or_zero();
        let up = side.cross(dir).normalize_or_zero();
        for off in [side, -side, up, -up] {
            pos.push((p + off * w * 0.5).to_array());
            nrm.push([0.0, 1.0, 0.0]);
            uv.push([0.0, i as f32 / (n - 1) as f32]);
        }
    }
    for i in 0..n as u32 - 1 {
        let (a, b) = (i * 4, (i + 1) * 4);
        idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        idx.extend([a + 2, b + 2, a + 3, a + 3, b + 2, b + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Working engines throw water at the fire (sweeping across the hottest cells
/// in reach, raising steam where it lands on flames); working hand crews raise
/// dust where they cut.
#[allow(clippy::too_many_arguments)]
pub fn ground_work(
    mut commands: Commands,
    time: Res<Time>,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    fx: Option<Res<EffectAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut jets: Query<(Entity, &Jet, &Handle<Mesh>)>,
    puffs: Query<(), With<Puff>>,
    mut noise: Local<Noise>,
) {
    let Some(fx) = fx else { return };
    let s = &scn.0;
    let now = time.elapsed_seconds();
    let mut budget = MAX_PUFFS.saturating_sub(puffs.iter().count());
    let working = |t: &&view::Deployment| t.status == TaskStatus::Working && t.qty > 0;
    let engines: Vec<&view::Deployment> = snap
        .deployments
        .iter()
        .filter(working)
        .filter(|t| t.action == "DeployEngine" || (t.action.is_empty() && t.resource == "engines"))
        .collect();
    let mut have: Vec<u64> = Vec::new();
    for (e, jet, handle) in &mut jets {
        let Some(task) = engines.iter().find(|t| t.id == jet.task) else {
            commands.entity(e).despawn();
            continue;
        };
        have.push(jet.task);
        let from = task.position(snap.time_s);
        let targets = jet_targets(s, &snap, from);
        if targets.is_empty() {
            commands.entity(e).insert(Visibility::Hidden);
            continue;
        }
        // Rest on one target, then turn to the next.
        let (dwell, turn) = (1.6, 0.9);
        let k = now / (dwell + turn) + jet.task as f32 * 0.31;
        let r = ((k.fract() * (dwell + turn) - dwell).max(0.0) / turn).min(1.0);
        let i = k.floor() as usize;
        let (a, b) = (targets[i % targets.len()], targets[(i + 1) % targets.len()]);
        let e2 = r * r * (3.0 - 2.0 * r);
        let to = [a[0] + (b[0] - a[0]) * e2, a[1] + (b[1] - a[1]) * e2];
        let y0 = height_at(s, from) * EXAGGERATION + NOZZLE_M;
        let y1 = height_at(s, to) * EXAGGERATION + 2.0;
        let len = (to[0] - from[0]).hypot(to[1] - from[1]);
        let crown = 8.0 + 0.18 * len;
        let points: Vec<(Vec3, f32)> = (0..=ARC_SEGMENTS)
            .map(|j| {
                let t = j as f32 / ARC_SEGMENTS as f32;
                let p = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ];
                let y = y0 + (y1 - y0) * t + crown * 4.0 * t * (1.0 - t);
                // Ripples run down the stream so it reads as flowing water.
                let ripple = 1.0 + 0.18 * (t * 22.0 - now * 9.0).sin();
                (to_bevy(s, p, 0.0).with_y(y), (1.6 + 2.6 * t) * ripple)
            })
            .collect();
        if let Some(m) = meshes.get_mut(handle) {
            *m = arc_mesh(&points);
        }
        commands.entity(e).insert(Visibility::Inherited);
        // Spray where it lands; steam where that is flame.
        if budget > 0 && noise.next() < 0.5 {
            budget -= 1;
            let burning = s
                .cell_of(to)
                .map(|c| snap.cells.get(s.grid.index(c)) == Some(&cell::BURNING))
                .unwrap_or(false);
            let at = to_bevy(s, to, 0.0).with_y(y1);
            spawn_puff(
                &mut commands,
                &fx,
                if burning { &fx.steam } else { &fx.mist },
                y1,
                PuffSpec {
                    at,
                    vel: Vec3::new(
                        noise.signed() * 3.0,
                        if burning { 7.0 } else { 2.0 },
                        noise.signed() * 3.0,
                    ),
                    size: 1.5,
                    grow: if burning { 5.0 } else { 3.0 },
                    life: if burning { 3.0 } else { 1.6 },
                    gravity: 0.0,
                    drag: 0.4,
                },
            );
        }
    }
    for task in &engines {
        if have.contains(&task.id) {
            continue;
        }
        let flat: Vec<(Vec3, f32)> = (0..=ARC_SEGMENTS).map(|_| (Vec3::ZERO, 0.0)).collect();
        commands.spawn((
            PbrBundle {
                // Never an empty mesh: the prepass panics on one.
                mesh: meshes.add(arc_mesh(&flat)),
                material: fx.jet.clone(),
                visibility: Visibility::Hidden,
                ..default()
            },
            bevy::pbr::NotShadowCaster,
            bevy::render::view::NoFrustumCulling,
            Jet { task: task.id },
        ));
    }
    // Hand crews cutting line: dust and chips at the work post.
    for task in snap.deployments.iter().filter(working).filter(|t| {
        t.action == "CutFireLine" || (t.action.is_empty() && t.resource == "ground_crews")
    }) {
        if budget == 0 || noise.next() > 0.25 {
            continue;
        }
        budget -= 1;
        let p = task.position(snap.time_s);
        let at = to_bevy(
            s,
            [p[0] + noise.signed() * 8.0, p[1] + noise.signed() * 8.0],
            1.0,
        );
        spawn_puff(
            &mut commands,
            &fx,
            &fx.dust,
            at.y,
            PuffSpec {
                at,
                vel: Vec3::new(noise.signed() * 2.0, 3.0, noise.signed() * 2.0),
                size: 1.2,
                grow: 2.5,
                life: 2.2,
                gravity: 0.0,
                drag: 0.5,
            },
        );
    }
}

// ---------------------------------------------------------------- wet ground

#[derive(Component)]
pub struct WetLayer;

/// Wetted, still unburnt cells as a faint blue wash draped over the terrain,
/// rebuilt only when the set of wet cells changes.
pub fn wet_ground(
    mut commands: Commands,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    fx: Option<Res<EffectAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    layers: Query<Entity, With<WetLayer>>,
    mut seen: Local<(usize, u64)>,
) {
    let Some(fx) = fx else { return };
    if !snap.is_changed() {
        return;
    }
    let s = &scn.0;
    let wet: Vec<usize> = snap
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == cell::WET)
        .map(|(i, _)| i)
        .collect();
    let key = (
        Arc::as_ptr(&scn.0) as usize,
        wet.iter().fold(wet.len() as u64, |h, &i| {
            h.wrapping_mul(31).wrapping_add(i as u64)
        }),
    );
    if key == *seen && layers.iter().next().is_some() != wet.is_empty() {
        return;
    }
    *seen = key;
    for e in &layers {
        commands.entity(e).despawn();
    }
    if wet.is_empty() {
        return;
    }
    let g = s.grid;
    let h = g.cellsize * 0.5;
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in wet {
        let c = scenario::Cell {
            row: i / g.cols,
            col: i % g.cols,
        };
        let q = s.cell_centre(c);
        let o = pos.len() as u32;
        for (dx, dy) in [(-h, -h), (h, -h), (h, h), (-h, h)] {
            pos.push(to_bevy(s, [q[0] + dx, q[1] + dy], 2.5).to_array());
            nrm.push([0.0, 1.0, 0.0]);
        }
        idx.extend([o, o + 2, o + 1, o, o + 3, o + 2]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_indices(Indices::U32(idx));
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(mesh),
            material: fx.wet.clone(),
            ..default()
        },
        bevy::pbr::NotShadowCaster,
        WetLayer,
    ));
}

// ---------------------------------------------------------------- evacuation

/// The drivable road network as a graph: polyline vertices, junctions merged.
pub struct RoadGraph {
    nodes: Vec<Pos>,
    adj: Vec<Vec<(usize, f32)>>,
}

impl RoadGraph {
    pub fn new(s: &Scenario) -> Self {
        let mut key: HashMap<(i32, i32), usize> = HashMap::new();
        let mut g = RoadGraph {
            nodes: Vec::new(),
            adj: Vec::new(),
        };
        for road in s.roads.iter().filter(|r| r.drivable) {
            let mut prev: Option<usize> = None;
            for p in &road.points {
                let k = ((p[0] / 2.0).round() as i32, (p[1] / 2.0).round() as i32);
                let id = *key.entry(k).or_insert_with(|| {
                    g.nodes.push(*p);
                    g.adj.push(Vec::new());
                    g.nodes.len() - 1
                });
                if let Some(a) = prev.filter(|a| *a != id) {
                    g.link(a, id);
                }
                prev = Some(id);
            }
        }
        // Road ends that stop just short of another road join it.
        let ends: Vec<usize> = (0..g.nodes.len())
            .filter(|&i| g.adj[i].len() <= 1)
            .collect();
        for a in ends {
            let best = (0..g.nodes.len())
                .filter(|&b| b != a && !g.adj[a].iter().any(|(n, _)| *n == b))
                .map(|b| (g.dist(a, b), b))
                .filter(|(d, _)| *d < 25.0)
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((_, b)) = best {
                g.link(a, b);
            }
        }
        g
    }

    fn dist(&self, a: usize, b: usize) -> f32 {
        let (p, q) = (self.nodes[a], self.nodes[b]);
        (p[0] - q[0]).hypot(p[1] - q[1])
    }

    fn link(&mut self, a: usize, b: usize) {
        let d = self.dist(a, b);
        self.adj[a].push((b, d));
        self.adj[b].push((a, d));
    }

    fn nearest(&self, p: Pos) -> Option<usize> {
        (0..self.nodes.len()).min_by(|&a, &b| {
            let da = (self.nodes[a][0] - p[0]).hypot(self.nodes[a][1] - p[1]);
            let db = (self.nodes[b][0] - p[0]).hypot(self.nodes[b][1] - p[1]);
            da.total_cmp(&db)
        })
    }

    /// Shortest distances and predecessors from `from` (Dijkstra). Road near
    /// the fire costs many times its length: evacuees drive away from it.
    fn search(&self, from: usize, fire: Option<Pos>) -> (Vec<f32>, Vec<usize>) {
        let n = self.nodes.len();
        let (mut dist, mut prev) = (vec![f32::INFINITY; n], vec![usize::MAX; n]);
        let mut heap = BinaryHeap::new();
        dist[from] = 0.0;
        heap.push(Reverse(((0.0f32).to_bits(), from)));
        while let Some(Reverse((d, a))) = heap.pop() {
            let d = f32::from_bits(d);
            if d > dist[a] {
                continue;
            }
            for &(b, w) in &self.adj[a] {
                let near = fire.is_some_and(|f| {
                    let p = self.nodes[b];
                    (p[0] - f[0]).hypot(p[1] - f[1]) < FIRE_AVOID_M
                });
                let nd = d + if near { w * 8.0 } else { w };
                if nd < dist[b] {
                    dist[b] = nd;
                    prev[b] = a;
                    // Non-negative floats order like their bit patterns.
                    heap.push(Reverse((nd.to_bits(), b)));
                }
            }
        }
        (dist, prev)
    }

    /// A road route from `from` to the shelter `to`, or, with none named, out
    /// of the valley: to the reachable road end farthest from the fire.
    pub fn route(&self, s: &Scenario, from: Pos, to: Option<Pos>, fire: Option<Pos>) -> Vec<Pos> {
        let Some(start) = self.nearest(from) else {
            return Vec::new();
        };
        let (dist, prev) = self.search(start, fire);
        let goal = match to.and_then(|p| self.nearest(p)) {
            Some(g) if dist[g].is_finite() => g,
            _ => {
                let (w, h) = (s.grid.width_m(), s.grid.height_m());
                let score = |i: usize| {
                    let p = self.nodes[i];
                    let edge = p[0].min(p[1]).min(w - p[0]).min(h - p[1]);
                    let away = fire.map_or(0.0, |f| (p[0] - f[0]).hypot(p[1] - f[1]));
                    // Leaving the map is the natural way out, but never towards the fire.
                    away * if edge < 150.0 { 1.4 } else { 1.0 }
                };
                match (0..self.nodes.len())
                    .filter(|&i| dist[i].is_finite())
                    .max_by(|&a, &b| score(a).total_cmp(&score(b)))
                {
                    Some(g) => g,
                    None => return Vec::new(),
                }
            }
        };
        let mut path = vec![self.nodes[goal]];
        let mut at = goal;
        while at != start && prev[at] != usize::MAX {
            at = prev[at];
            path.push(self.nodes[at]);
        }
        path.push(from);
        path.reverse();
        path
    }
}

/// A route drawn in the right-hand lane, densified so it follows the terrain.
pub struct Lane {
    points: Vec<Vec3>,
    /// Cumulative length at each point.
    at: Vec<f32>,
}

impl Lane {
    fn new(s: &Scenario, route: &[Pos]) -> Option<Self> {
        let mut flat: Vec<Pos> = Vec::new();
        for w in route.windows(2) {
            let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            let n = (len / 10.0).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = k as f32 / n as f32;
                flat.push([
                    w[0][0] + (w[1][0] - w[0][0]) * f,
                    w[0][1] + (w[1][1] - w[0][1]) * f,
                ]);
            }
        }
        flat.push(*route.last()?);
        if flat.len() < 2 {
            return None;
        }
        let mut points = Vec::with_capacity(flat.len());
        for (i, p) in flat.iter().enumerate() {
            let a = flat[i.saturating_sub(1)];
            let b = flat[(i + 1).min(flat.len() - 1)];
            let d = Vec2::new(b[0] - a[0], b[1] - a[1]).normalize_or_zero();
            // Right of the direction of travel, in world metres (x east, y north).
            let right = Vec2::new(d.y, -d.x) * LANE_M;
            points.push(to_bevy(s, [p[0] + right.x, p[1] + right.y], 0.3));
        }
        let mut at = vec![0.0];
        for w in points.windows(2) {
            at.push(at.last().unwrap() + (w[1] - w[0]).length());
        }
        Some(Lane { points, at })
    }

    fn length(&self) -> f32 {
        *self.at.last().unwrap_or(&0.0)
    }

    fn sample(&self, d: f32) -> (Vec3, Vec3) {
        let i = self
            .at
            .partition_point(|x| *x <= d)
            .clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[i - 1], self.points[i]);
        let f = ((d - self.at[i - 1]) / (self.at[i] - self.at[i - 1]).max(0.01)).clamp(0.0, 1.0);
        (a.lerp(b, f), b - a)
    }
}

#[derive(Component)]
pub struct EvacVehicle {
    lane: Arc<Lane>,
    d: f32,
}

struct Emitter {
    left: u32,
    next: f32,
    bus: bool,
    lane: Arc<Lane>,
    /// The road node where it joins the network: departures there are spaced.
    entry: (i32, i32),
}

#[derive(Default)]
pub struct EvacState {
    scene: usize,
    graph: Option<Arc<RoadGraph>>,
    emitters: HashMap<usize, Emitter>,
    /// Last departure through each road entry, real seconds.
    entries: HashMap<(i32, i32), f32>,
    last_time_s: i64,
}

/// Evacuees leave by car (homes) or by bus (schools, hospitals): from every
/// building being evacuated, a few vehicles depart in turn and drive the road
/// network to the named shelter, or out of the valley away from the fire.
#[allow(clippy::too_many_arguments)]
pub fn evacuation(
    mut commands: Commands,
    time: Res<Time>,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    pal: Option<Res<DeploymentPalette>>,
    mut st: Local<EvacState>,
    mut cars: Query<(Entity, &mut EvacVehicle, &mut Transform)>,
) {
    let Some(pal) = pal else { return };
    let s = &scn.0;
    let now = time.elapsed_seconds();
    let dt = time.delta_seconds().min(0.1);
    let scene = Arc::as_ptr(&scn.0) as usize;
    if st.scene != scene || snap.time_s < st.last_time_s {
        // A new territory or a rollback: what was on the road is gone.
        st.scene = scene;
        st.graph = None;
        st.emitters.clear();
        st.entries.clear();
        for (e, ..) in &cars {
            commands.entity(e).despawn_recursive();
        }
    }
    st.last_time_s = snap.time_s;

    // Drive.
    for (e, mut car, mut tf) in &mut cars {
        car.d += CAR_MPS * dt;
        if car.d >= car.lane.length() {
            commands.entity(e).despawn_recursive();
            continue;
        }
        let (p, dir) = car.lane.sample(car.d);
        tf.translation = p;
        if dir.length_squared() > 0.01 {
            tf.rotation = Quat::from_rotation_y(dir.x.atan2(dir.z));
        }
    }

    if snap.traffic_model {
        st.emitters.clear();
        return;
    }
    let evacuating: Vec<usize> = snap
        .buildings
        .iter()
        .enumerate()
        .filter(|(i, b)| b.evacuating && b.evac_people > 0 && *i < s.buildings.len())
        .map(|(i, _)| i)
        .collect();
    st.emitters.retain(|i, _| evacuating.contains(i));
    if evacuating.is_empty() {
        return;
    }
    let graph = st
        .graph
        .get_or_insert_with(|| Arc::new(RoadGraph::new(s)))
        .clone();
    let fire = fire_centre(s, &snap);
    for i in evacuating {
        if st.emitters.contains_key(&i) {
            continue;
        }
        let b = &snap.buildings[i];
        let bus = s.buildings[i].kind.needs_transport();
        let left = if bus {
            b.evac_people.div_ceil(40).clamp(1, 4)
        } else {
            b.evac_people.div_ceil(4).clamp(1, 3)
        };
        let route = graph.route(s, s.buildings[i].pos, b.evac_to, fire);
        let Some(lane) = Lane::new(s, &route) else {
            continue;
        };
        let join = route.get(1).copied().unwrap_or(route[0]);
        st.emitters.insert(
            i,
            Emitter {
                left,
                next: now + (i % 11) as f32 * 0.7,
                bus,
                lane: Arc::new(lane),
                entry: (join[0].round() as i32, join[1].round() as i32),
            },
        );
    }
    let mut on_road = cars.iter().count();
    let EvacState {
        emitters, entries, ..
    } = &mut *st;
    for em in emitters.values_mut() {
        if em.left == 0 || now < em.next || on_road >= MAX_CARS {
            continue;
        }
        // One vehicle at a time onto the road: a gap, not a pile-up.
        let last = entries.entry(em.entry).or_insert(f32::MIN);
        if now - *last < CAR_EVERY_S {
            continue;
        }
        *last = now;
        em.left -= 1;
        em.next = now + CAR_EVERY_S * if em.bus { 2.0 } else { 1.0 };
        on_road += 1;
        let (mesh, material) = pal.body(if em.bus { "bus" } else { "car" });
        let (p, _) = em.lane.sample(0.0);
        commands.spawn((
            MaterialMeshBundle {
                mesh,
                material,
                transform: Transform::from_translation(p).with_scale(Vec3::splat(2.0)),
                ..default()
            },
            EvacVehicle {
                lane: em.lane.clone(),
                d: 0.0,
            },
        ));
    }
}

/// Centroid of the burning cells, if any.
fn fire_centre(s: &Scenario, snap: &Snapshot) -> Option<Pos> {
    let (mut x, mut y, mut n) = (0.0, 0.0, 0.0);
    for (i, c) in snap.cells.iter().enumerate() {
        if *c == cell::BURNING || *c == cell::BURNT {
            let p = s.cell_centre(scenario::Cell {
                row: i / s.grid.cols,
                col: i % s.grid.cols,
            });
            x += p[0];
            y += p[1];
            n += 1.0;
        }
    }
    (n > 0.0).then(|| [x / n, y / n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evacuation_routes_follow_roads_to_the_shelter() {
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let g = RoadGraph::new(&s);
        assert!(!g.nodes.is_empty());
        let home = s
            .buildings
            .iter()
            .find(|b| b.occupants > 0)
            .expect("an occupied building");
        let shelter = s
            .buildings
            .iter()
            .rev()
            .find(|b| b.name.is_some())
            .expect("a facility");
        let route = g.route(&s, home.pos, Some(shelter.pos), None);
        assert!(route.len() >= 2);
        assert_eq!(route[0], home.pos);
        let end = route.last().unwrap();
        let near_shelter = g.nearest(shelter.pos).map(|n| g.nodes[n]).unwrap();
        assert_eq!(*end, near_shelter);
        let lane = Lane::new(&s, &route).expect("a drawable lane");
        assert!(lane.length() > 0.0);
    }

    #[test]
    fn without_a_shelter_evacuees_drive_away_from_the_fire() {
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let g = RoadGraph::new(&s);
        let home = s.buildings.iter().find(|b| b.occupants > 0).unwrap().pos;
        let fire = [home[0] + 300.0, home[1] + 300.0];
        let route = g.route(&s, home, None, Some(fire));
        let end = *route.last().unwrap();
        let d = |p: Pos| (p[0] - fire[0]).hypot(p[1] - fire[1]);
        assert!(d(end) > d(home) + 500.0, "ends {} m from the fire", d(end));
        // The way out does not run towards the fire.
        let closest = route.iter().map(|p| d(*p)).fold(f32::MAX, f32::min);
        assert!(
            closest > d(home) - 150.0,
            "passes {closest} m from the fire"
        );
    }
}
