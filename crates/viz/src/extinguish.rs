//! EXCON extinguish interventions made legible: the model puts the fire out at
//! once with no units (`Extinguish`, `ExtinguishArea`), so on its own the fire
//! would just vanish. For a short while after each one this stages a crew
//! scene at the footprint, after `propagator_abm`'s suppression layer: fire
//! engines roll up to the nearest road point and park, firefighters spread
//! along the scar's edge with hose lines back to their engine and jets aimed
//! at the edge, steam rises where the water lands and the ground goes dark and
//! wet; then the crews pack up and the engines drive off and nothing remains.
//!
//! Presentation only, from [`view::Extinguished`] (footprint, simulated time,
//! cells put out) and the cell codes at the moment it is first seen. Like an
//! air drop's pass the scene runs on **real** seconds (`ARRIVE_S + WORK_S +
//! LEAVE_S`, about 20 s), integrated per frame, whatever the simulation speed:
//! at maximum speed simulated minutes last a fraction of a second, between
//! turns they do not pass at all. Interventions older than [`REPLAY_WINDOW_S`]
//! simulated seconds when first seen are not replayed; a rollback that removes
//! one clears its scene.

use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::{
    mesh::{Indices, PrimitiveTopology},
    render_asset::RenderAssetUsages,
};
use scenario::{Pos, Scenario};
use view::{cell, ExtinguishShape, Extinguished};

use crate::deployments::DeploymentPalette;
use crate::effects::{arc_mesh, spawn_puff, EffectAssets, Noise, Puff, PuffSpec, MAX_PUFFS};
use crate::frame::{height_at, to_bevy, EXAGGERATION};
use crate::models::Rig;
use crate::{Scn, Snapshot};

/// Real seconds the engines take to roll in, the crews work, and they leave.
pub(crate) const ARRIVE_S: f32 = 2.5;
pub(crate) const WORK_S: f32 = 14.0;
pub(crate) const LEAVE_S: f32 = 4.0;
/// Interventions older than this (simulated seconds) when first seen are not
/// replayed, so joining a running exercise does not set off its history.
const REPLAY_WINDOW_S: i64 = 900;
/// Road farther than this from a crew group is not used: the engine parks
/// off-road at the edge of the footprint instead.
const ROAD_REACH_M: f32 = 350.0;
/// Edge length one engine (and its crews) is staged for.
const EDGE_PER_ENGINE_M: f32 = 450.0;
/// Where a firefighter stands, outside the scar's edge, and how far beyond
/// the edge the jet lands (metres).
const POST_OUT_M: f32 = 30.0;
const AIM_IN_M: f32 = 10.0;
/// Engines roll in and out along the road this far.
const ROLL_IN_M: f32 = 90.0;
const ROLL_OUT_M: f32 = 140.0;
/// Firefighters walk this far from the engine side to their post as they appear.
const WALK_M: f32 = 22.0;
/// Symbol scales, as for the role deployments.
const ENGINE_SCALE: f32 = 3.5;
const CREW_SCALE: f32 = 4.0;
/// Height of the nozzle (a firefighter's hands at symbol scale), metres.
const NOZZLE_M: f32 = 5.0;
const ARC_SEGMENTS: usize = 16;
/// Peak opacity of the wet, darkened ground.
const WET_ALPHA: f32 = 0.42;
/// The darkening fades in over this distance inside the footprint's edge.
const WET_FEATHER_M: f32 = 45.0;
/// Most cells the wet layer is drawn with (coarser beyond).
const WET_MAX_CELLS: usize = 20_000;

/// What the graphics level allows a scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Caps {
    pub engines: usize,
    pub crews_per_engine: usize,
    /// Steam puffs per crew per real second.
    pub puffs_per_crew_s: f32,
}

pub(crate) fn caps(quality: &str) -> Caps {
    match quality {
        "low" => Caps {
            engines: 2,
            crews_per_engine: 2,
            puffs_per_crew_s: 2.0,
        },
        "high" => Caps {
            engines: 6,
            crews_per_engine: 3,
            puffs_per_crew_s: 7.0,
        },
        _ => Caps {
            engines: 4,
            crews_per_engine: 3,
            puffs_per_crew_s: 4.5,
        },
    }
}

/// Engines and firefighters for an edge of `edge_m` metres: one engine per
/// [`EDGE_PER_ENGINE_M`] (at least one, at most the cap), its crews, and never
/// more firefighters than there are edge points to stand at.
pub(crate) fn unit_counts(edge_m: f32, edge_points: usize, caps: Caps) -> (usize, usize) {
    let engines = ((edge_m / EDGE_PER_ENGINE_M).ceil() as usize).clamp(1, caps.engines.max(1));
    let crews = (engines * caps.crews_per_engine).min(edge_points.max(1));
    (engines.min(crews), crews)
}

// ---------------------------------------------------------------- geometry

/// The footprint as a polygon (a disc as a 40-gon).
pub(crate) fn outline(shape: &ExtinguishShape) -> Vec<Pos> {
    match shape {
        ExtinguishShape::Rect { bounds: b } => {
            vec![[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]]
        }
        ExtinguishShape::Disc {
            centre: c,
            radius_m: r,
        } => (0..40)
            .map(|k| {
                let a = k as f32 / 40.0 * std::f32::consts::TAU;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect(),
        ExtinguishShape::Polygon { vertices } => vertices.clone(),
    }
}

/// Even-odd point in polygon.
pub(crate) fn inside(poly: &[Pos], p: Pos) -> bool {
    let mut odd = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

fn dist_to_segment(p: Pos, a: Pos, b: Pos) -> (f32, f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let f = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [a[0] + dx * f, a[1] + dy * f];
    ((p[0] - q[0]).hypot(p[1] - q[1]), f)
}

fn dist_to_outline(poly: &[Pos], p: Pos) -> f32 {
    (0..poly.len())
        .map(|i| dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()]).0)
        .fold(f32::MAX, f32::min)
}

fn bbox(poly: &[Pos]) -> [f32; 4] {
    poly.iter()
        .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        })
}

/// A point of the edge where the fire was put out, and the unit direction
/// (world, x east, y north) away from the burnt ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EdgePoint {
    pub at: Pos,
    pub out: Vec2,
}

/// The edge of the burnt ground inside the footprint: burnt cells with a
/// neighbour that is not burnt (unburnt ground, or fire still burning beyond
/// the footprint). That is where the front stood when it was put out. Empty
/// when the footprint holds no burnt ground.
pub(crate) fn burnt_edge(s: &Scenario, cells: &[u8], shape: &ExtinguishShape) -> Vec<EdgePoint> {
    let g = s.grid;
    if cells.len() != g.len() {
        return Vec::new();
    }
    let poly = outline(shape);
    let b = bbox(&poly);
    let col = |x: f32| ((x / g.cellsize).floor().max(0.0) as usize).min(g.cols);
    let (c0, c1) = (col(b[0]), (col(b[2]) + 1).min(g.cols));
    // Rows count from the north.
    let row = |y: f32| g.rows as isize - 1 - (y / g.cellsize).floor() as isize;
    let (r0, r1) = (
        row(b[3]).clamp(0, g.rows as isize) as usize,
        (row(b[1]) + 1).clamp(0, g.rows as isize) as usize,
    );
    let mut out = Vec::new();
    for r in r0..r1 {
        for c in c0..c1 {
            let here = scenario::Cell { row: r, col: c };
            if cells[g.index(here)] != cell::BURNT {
                continue;
            }
            let p = s.cell_centre(here);
            if !inside(&poly, p) {
                continue;
            }
            let mut away = Vec2::ZERO;
            for (dr, dc) in [
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (-1, 1),
                (1, -1),
                (1, 1),
            ] {
                let (rr, cc) = (r as isize + dr, c as isize + dc);
                if rr < 0 || cc < 0 || rr >= g.rows as isize || cc >= g.cols as isize {
                    continue;
                }
                let n = scenario::Cell {
                    row: rr as usize,
                    col: cc as usize,
                };
                if cells[g.index(n)] != cell::BURNT {
                    // Row down is south.
                    away += Vec2::new(dc as f32, -dr as f32);
                }
            }
            if away.length_squared() > 0.01 {
                out.push(EdgePoint {
                    at: p,
                    out: away.normalize(),
                });
            }
        }
    }
    out
}

/// The footprint's own outline as edge points facing outwards: where crews
/// stand when nothing inside was burnt.
fn outline_edge(poly: &[Pos], step_m: f32) -> Vec<EdgePoint> {
    let n = poly.len();
    let c = poly.iter().fold([0.0, 0.0], |a, p| {
        [a[0] + p[0] / n as f32, a[1] + p[1] / n as f32]
    });
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let k = (len / step_m).ceil().max(1.0) as usize;
        for j in 0..k {
            let f = j as f32 / k as f32;
            let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
            let out_dir = Vec2::new(p[0] - c[0], p[1] - c[1]).normalize_or_zero();
            // Inside the outline by the post distance, so the crew stands on it.
            out.push(EdgePoint {
                at: [p[0] - out_dir.x * POST_OUT_M, p[1] - out_dir.y * POST_OUT_M],
                out: out_dir,
            });
        }
    }
    out
}

/// `k` indices of `points`, spread out (farthest-point sampling from the
/// point farthest from their mean). Deterministic.
pub(crate) fn spread(points: &[Pos], k: usize) -> Vec<usize> {
    let n = points.len();
    if n == 0 || k == 0 {
        return Vec::new();
    }
    let m = points.iter().fold([0.0, 0.0], |a, p| {
        [a[0] + p[0] / n as f32, a[1] + p[1] / n as f32]
    });
    let d = |a: Pos, b: Pos| (a[0] - b[0]).hypot(a[1] - b[1]);
    let first = (0..n)
        .max_by(|&a, &b| d(points[a], m).total_cmp(&d(points[b], m)))
        .unwrap_or(0);
    let mut chosen = vec![first];
    let mut near: Vec<f32> = points.iter().map(|p| d(*p, points[first])).collect();
    while chosen.len() < k.min(n) {
        let next = (0..n)
            .max_by(|&a, &b| near[a].total_cmp(&near[b]))
            .unwrap_or(0);
        if near[next] <= 0.0 {
            break;
        }
        chosen.push(next);
        for (i, p) in points.iter().enumerate() {
            near[i] = near[i].min(d(*p, points[next]));
        }
    }
    chosen
}

/// The nearest point on a drivable road: `(road index, segment, fraction, point)`.
pub(crate) fn nearest_road(s: &Scenario, p: Pos, reach_m: f32) -> Option<(usize, usize, f32, Pos)> {
    let mut best: Option<(f32, usize, usize, f32)> = None;
    for (ri, road) in s.roads.iter().enumerate().filter(|(_, r)| r.drivable) {
        for (si, w) in road.points.windows(2).enumerate() {
            let (d, f) = dist_to_segment(p, w[0], w[1]);
            if d <= reach_m && best.is_none_or(|b| d < b.0) {
                best = Some((d, ri, si, f));
            }
        }
    }
    best.map(|(_, ri, si, f)| {
        let w = &s.roads[ri].points[si..si + 2];
        (
            ri,
            si,
            f,
            [
                w[0][0] + (w[1][0] - w[0][0]) * f,
                w[0][1] + (w[1][1] - w[0][1]) * f,
            ],
        )
    })
}

/// The point `dist` metres along a polyline from segment `seg` at fraction
/// `f` (negative = backwards), clamped to its ends, and the unit direction of
/// travel there (world).
pub(crate) fn along(points: &[Pos], seg: usize, f: f32, dist: f32) -> (Pos, Vec2) {
    let seg_len =
        |i: usize| (points[i + 1][0] - points[i][0]).hypot(points[i + 1][1] - points[i][1]);
    let dir = |i: usize| {
        Vec2::new(
            points[i + 1][0] - points[i][0],
            points[i + 1][1] - points[i][1],
        )
        .normalize_or_zero()
    };
    let lerp = |i: usize, t: f32| {
        [
            points[i][0] + (points[i + 1][0] - points[i][0]) * t,
            points[i][1] + (points[i + 1][1] - points[i][1]) * t,
        ]
    };
    if points.len() < 2 {
        return (points.first().copied().unwrap_or([0.0, 0.0]), Vec2::Y);
    }
    let (mut i, mut left) = (seg.min(points.len() - 2), dist);
    let mut t = f.clamp(0.0, 1.0);
    let sign = if dist < 0.0 { -1.0 } else { 1.0 };
    left = left.abs();
    loop {
        let len = seg_len(i).max(0.001);
        let room = if sign > 0.0 { (1.0 - t) * len } else { t * len };
        if left <= room {
            t += sign * left / len;
            return (lerp(i, t), dir(i) * sign);
        }
        left -= room;
        if sign > 0.0 && i + 2 < points.len() {
            i += 1;
            t = 0.0;
        } else if sign < 0.0 && i > 0 {
            i -= 1;
            t = 1.0;
        } else {
            return (lerp(i, if sign > 0.0 { 1.0 } else { 0.0 }), dir(i) * sign);
        }
    }
}

/// Where an engine parks: on a road (and which), or off-road.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Park {
    pub at: Pos,
    /// Unit heading when parked (world).
    pub heading: Vec2,
    /// `(road index, segment, fraction)` when parked on a road.
    pub road: Option<(usize, usize, f32)>,
}

/// Where a firefighter stands and what the jet is aimed at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Post {
    pub at: Pos,
    pub aim: Pos,
    /// Lateral sweep direction of the jet (world, unit).
    pub across: Vec2,
    pub engine: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    pub engines: Vec<Park>,
    pub crews: Vec<Post>,
}

/// Stage a scene: crews spread along the burnt edge (else the footprint's
/// outline), grouped round as many engines as the edge needs and the caps
/// allow, each engine parked at the road point nearest its group.
pub(crate) fn plan(s: &Scenario, cells: &[u8], shape: &ExtinguishShape, caps: Caps) -> Plan {
    let poly = outline(shape);
    if poly.len() < 3 {
        return Plan::default();
    }
    let mut edge = burnt_edge(s, cells, shape);
    let edge_m = edge.len() as f32 * s.grid.cellsize;
    if edge.is_empty() {
        edge = outline_edge(&poly, 40.0);
    }
    let (n_engines, n_crews) = unit_counts(edge_m.max(1.0), edge.len(), caps);
    let at: Vec<Pos> = edge.iter().map(|e| e.at).collect();
    let mut crews: Vec<Post> = spread(&at, n_crews)
        .into_iter()
        .map(|i| {
            let e = edge[i];
            Post {
                at: [
                    e.at[0] + e.out.x * POST_OUT_M,
                    e.at[1] + e.out.y * POST_OUT_M,
                ],
                aim: [e.at[0] - e.out.x * AIM_IN_M, e.at[1] - e.out.y * AIM_IN_M],
                across: Vec2::new(-e.out.y, e.out.x),
                engine: 0,
            }
        })
        .collect();
    let posts: Vec<Pos> = crews.iter().map(|c| c.at).collect();
    let seeds = spread(&posts, n_engines);
    let d = |a: Pos, b: Pos| (a[0] - b[0]).hypot(a[1] - b[1]);
    for c in &mut crews {
        c.engine = (0..seeds.len())
            .min_by(|&a, &b| d(c.at, posts[seeds[a]]).total_cmp(&d(c.at, posts[seeds[b]])))
            .unwrap_or(0);
    }
    let engines = (0..seeds.len())
        .map(|k| {
            let group: Vec<&Post> = crews.iter().filter(|c| c.engine == k).collect();
            let n = group.len().max(1) as f32;
            let centre = group
                .iter()
                .fold([0.0, 0.0], |a, c| [a[0] + c.at[0] / n, a[1] + c.at[1] / n]);
            let out = group
                .iter()
                .fold(Vec2::ZERO, |a, c| {
                    a + Vec2::new(c.at[0] - c.aim[0], c.at[1] - c.aim[1])
                })
                .normalize_or_zero();
            match nearest_road(s, centre, ROAD_REACH_M) {
                Some((ri, si, f, p)) => {
                    let (_, dir) = along(&s.roads[ri].points, si, f, 0.0);
                    Park {
                        at: p,
                        heading: dir,
                        road: Some((ri, si, f)),
                    }
                }
                None => Park {
                    at: [centre[0] + out.x * 25.0, centre[1] + out.y * 25.0],
                    // Nose towards the work, back to the way out.
                    heading: -out,
                    road: None,
                },
            }
        })
        .collect();
    Plan { engines, crews }
}

/// 0 → 1 ease.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How present the scene is at age `a` (real seconds): 0 before, ramps in,
/// 1 while working, ramps out to 0 at the end.
pub(crate) fn envelope(a: f32) -> f32 {
    let end = ARRIVE_S + WORK_S + LEAVE_S;
    if a <= 0.0 || a >= end {
        0.0
    } else {
        smooth(a / ARRIVE_S).min(smooth((end - a) / LEAVE_S))
    }
}

// ---------------------------------------------------------------- render

struct Unit {
    root: Entity,
    parts: Vec<(Entity, Rig)>,
}

struct Scene {
    index: usize,
    t_s: i64,
    start: f32,
    plan: Plan,
    engines: Vec<Unit>,
    crews: Vec<Unit>,
    jets: Vec<(Entity, Handle<Mesh>)>,
    hoses: Vec<Entity>,
    wet: Option<(Entity, Handle<StandardMaterial>)>,
    /// Burnt cells inside the footprint, for the steam of doused ground.
    smoulder: Vec<Pos>,
    puff_carry: f32,
}

impl Scene {
    fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.engines
            .iter()
            .chain(&self.crews)
            .map(|u| u.root)
            .chain(self.jets.iter().map(|j| j.0))
            .chain(self.hoses.iter().copied())
            .chain(self.wet.iter().map(|w| w.0))
    }
}

#[derive(Default)]
pub struct State {
    scene: usize,
    seen: usize,
    active: Vec<Scene>,
    noise: Noise,
}

fn spawn_unit(commands: &mut Commands, pal: &DeploymentPalette, name: &str) -> Unit {
    let (mesh, material) = pal.body(name);
    let mut parts = Vec::new();
    let root = commands
        .spawn(MaterialMeshBundle {
            mesh,
            material: material.clone(),
            transform: Transform::from_scale(Vec3::ZERO),
            ..default()
        })
        .with_children(|p| {
            for (m, rig) in pal.parts(name) {
                let e = p
                    .spawn(MaterialMeshBundle {
                        mesh: m.clone(),
                        material: material.clone(),
                        transform: Transform::from_translation(rig.pivot),
                        ..default()
                    })
                    .id();
                parts.push((e, rig.clone()));
            }
        })
        .id();
    Unit { root, parts }
}

/// A ground-hugging line (a hose), sampled every few metres on the terrain.
fn hose_mesh(s: &Scenario, a: Pos, b: Pos) -> Mesh {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = ((len / 8.0).ceil() as usize).clamp(2, 200);
    let pts: Vec<(Vec3, f32)> = (0..=n)
        .map(|k| {
            let f = k as f32 / n as f32;
            // A slight sag sideways so it reads as a laid hose, not a ruler line.
            let side = (f * std::f32::consts::PI).sin() * len * 0.06;
            let (dx, dy) = ((b[0] - a[0]) / len.max(0.01), (b[1] - a[1]) / len.max(0.01));
            let p = [
                a[0] + (b[0] - a[0]) * f - dy * side,
                a[1] + (b[1] - a[1]) * f + dx * side,
            ];
            (to_bevy(s, p, 0.8), 0.9)
        })
        .collect();
    arc_mesh(&pts)
}

/// The footprint darkened and wet: cell-corner grid over its bounding box,
/// vertex alpha feathered in from the outline. `None` when nothing is inside.
fn wet_mesh(s: &Scenario, shape: &ExtinguishShape) -> Option<Mesh> {
    let poly = outline(shape);
    let b = bbox(&poly);
    let g = s.grid;
    let mut step = g.cellsize;
    while ((b[2] - b[0]) / step).ceil() * ((b[3] - b[1]) / step).ceil() > WET_MAX_CELLS as f32 {
        step *= 2.0;
    }
    let (nx, ny) = (
        ((b[2] - b[0]) / step).ceil() as usize + 1,
        ((b[3] - b[1]) / step).ceil() as usize + 1,
    );
    let (mut pos, mut nrm, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (w, h) = (g.width_m(), g.height_m());
    for j in 0..ny {
        for i in 0..nx {
            let p = [
                (b[0] + i as f32 * step).clamp(0.0, w),
                (b[1] + j as f32 * step).clamp(0.0, h),
            ];
            let a = if inside(&poly, p) {
                smooth(dist_to_outline(&poly, p) / WET_FEATHER_M)
            } else {
                0.0
            };
            pos.push(to_bevy(s, p, 1.8).to_array());
            nrm.push([0.0, 1.0, 0.0]);
            col.push([1.0, 1.0, 1.0, a]);
        }
    }
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let o = (j * nx + i) as u32;
            let (a, b2, c, d) = (o, o + 1, o + 1 + nx as u32, o + nx as u32);
            if [a, b2, c, d].iter().all(|&v| col[v as usize][3] <= 0.0) {
                continue;
            }
            // Counter-clockwise seen from above (Bevy's front face).
            idx.extend([a, b2, c, a, c, d]);
        }
    }
    if idx.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    Some(mesh)
}

#[allow(clippy::too_many_arguments)]
fn start_scene(
    commands: &mut Commands,
    s: &Scenario,
    snap: &Snapshot,
    pal: &DeploymentPalette,
    fx: &EffectAssets,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    caps: Caps,
    index: usize,
    rec: &Extinguished,
    now: f32,
) -> Scene {
    let plan = plan(s, &snap.cells, &rec.shape, caps);
    let engines = plan
        .engines
        .iter()
        .map(|_| spawn_unit(commands, pal, "engine"))
        .collect();
    let crews = plan
        .crews
        .iter()
        .map(|_| spawn_unit(commands, pal, "firefighter"))
        .collect();
    let flat: Vec<(Vec3, f32)> = (0..=ARC_SEGMENTS).map(|_| (Vec3::ZERO, 0.0)).collect();
    let jets = plan
        .crews
        .iter()
        .map(|_| {
            // Never an empty mesh: the prepass panics on one.
            let h = meshes.add(arc_mesh(&flat));
            let e = commands
                .spawn((
                    PbrBundle {
                        mesh: h.clone(),
                        material: fx.jet.clone(),
                        visibility: Visibility::Hidden,
                        ..default()
                    },
                    bevy::pbr::NotShadowCaster,
                    bevy::render::view::NoFrustumCulling,
                ))
                .id();
            (e, h)
        })
        .collect();
    let hose_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.09, 0.06),
        perceptual_roughness: 0.8,
        ..default()
    });
    let hoses = plan
        .crews
        .iter()
        .map(|c| {
            let from = plan.engines.get(c.engine).map_or(c.at, |e| e.at);
            commands
                .spawn((
                    PbrBundle {
                        mesh: meshes.add(hose_mesh(s, from, c.at)),
                        material: hose_mat.clone(),
                        visibility: Visibility::Hidden,
                        ..default()
                    },
                    bevy::pbr::NotShadowCaster,
                ))
                .id()
        })
        .collect();
    let wet = wet_mesh(s, &rec.shape).map(|mesh| {
        let m = mats.add(StandardMaterial {
            // Soaked ash: darker, a touch cold. Vertex alpha feathers the edge.
            base_color: Color::srgba(0.015, 0.02, 0.03, 0.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let e = commands
            .spawn((
                PbrBundle {
                    mesh: meshes.add(mesh),
                    material: m.clone(),
                    ..default()
                },
                bevy::pbr::NotShadowCaster,
                bevy::pbr::NotShadowReceiver,
            ))
            .id();
        (e, m)
    });
    let poly = outline(&rec.shape);
    let smoulder = burnt_edge(s, &snap.cells, &rec.shape)
        .into_iter()
        .map(|e| e.at)
        .filter(|p| inside(&poly, *p))
        .collect();
    Scene {
        index,
        t_s: rec.t_s,
        start: now,
        plan,
        engines,
        crews,
        jets,
        hoses,
        wet,
        smoulder,
        puff_carry: 0.0,
    }
}

/// Stage, animate and clear the extinguish scenes.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    scn: Res<Scn>,
    snap: Res<Snapshot>,
    quality: Res<crate::quality::Quality>,
    fx: Option<Res<EffectAssets>>,
    pal: Option<Res<DeploymentPalette>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut st: Local<State>,
    mut tfs: Query<&mut Transform, Without<Puff>>,
    mut vis: Query<&mut Visibility>,
    puffs: Query<(), With<Puff>>,
) {
    let (Some(fx), Some(pal)) = (fx, pal) else {
        return;
    };
    let s = &scn.0;
    let now = time.elapsed_seconds();
    let dt = time.delta_seconds().min(0.1);
    let scene = Arc::as_ptr(&scn.0) as usize;
    if st.scene != scene {
        // A new territory: its entities were despawned with the scene.
        *st = State { scene, ..default() };
    }
    if snap.cells.len() != s.grid.len() {
        return;
    }
    // A rollback (or a replaced record) ends the scenes it took away.
    let recs = &snap.extinguished;
    st.active.retain(|sc| {
        let keep = recs.get(sc.index).is_some_and(|r| r.t_s == sc.t_s) && snap.time_s >= sc.t_s;
        if !keep {
            for e in sc.entities() {
                commands.entity(e).despawn_recursive();
            }
        }
        keep
    });
    if recs.len() < st.seen {
        st.seen = recs.len();
    }
    let caps = caps(quality.name);
    for (i, rec) in recs.iter().enumerate().skip(st.seen) {
        if snap.time_s - rec.t_s > REPLAY_WINDOW_S {
            continue;
        }
        let sc = start_scene(
            &mut commands,
            s,
            &snap,
            &pal,
            &fx,
            &mut meshes,
            &mut mats,
            caps,
            i,
            rec,
            now,
        );
        st.active.push(sc);
    }
    st.seen = recs.len();

    let mut budget = MAX_PUFFS.saturating_sub(puffs.iter().count());
    let end = ARRIVE_S + WORK_S + LEAVE_S;
    let State { active, noise, .. } = &mut *st;
    active.retain_mut(|sc| {
        let a = now - sc.start;
        if a >= end {
            for e in sc.entities() {
                commands.entity(e).despawn_recursive();
            }
            if let Some((_, m)) = &sc.wet {
                mats.remove(m);
            }
            return false;
        }
        let leave = ((a - ARRIVE_S - WORK_S) / LEAVE_S).clamp(0.0, 1.0);
        let working = a >= ARRIVE_S && leave <= 0.0;
        // Engines: roll in along the road, park, roll out.
        for (unit, park) in sc.engines.iter().zip(&sc.plan.engines) {
            let arrive = smooth(a / ARRIVE_S);
            let offset = -(1.0 - arrive) * ROLL_IN_M + leave * leave * ROLL_OUT_M;
            let (p, dir) = match park.road {
                Some((ri, si, f)) => along(&s.roads[ri].points, si, f, offset),
                None => (
                    [
                        park.at[0] + park.heading.x * offset,
                        park.at[1] + park.heading.y * offset,
                    ],
                    park.heading,
                ),
            };
            // Rolling backwards along the polyline flips its direction: the
            // engine still faces the way it drives in.
            let dir = if offset < 0.0 { -dir } else { dir };
            let moving = arrive < 1.0 || leave > 0.0;
            let scale = ENGINE_SCALE * smooth(a / 0.4) * smooth((1.0 - leave) / 0.3);
            if let Ok(mut tf) = tfs.get_mut(unit.root) {
                tf.translation = to_bevy(s, p, 0.2);
                tf.rotation = Quat::from_rotation_y(dir.x.atan2(-dir.y));
                tf.scale = Vec3::splat(scale);
            }
            for (e, rig) in &unit.parts {
                let Ok(mut tf) = tfs.get_mut(*e) else {
                    continue;
                };
                match rig.kind.as_str() {
                    "wheel" if moving => {
                        tf.rotation = Quat::from_axis_angle(rig.axis.normalize_or_zero(), now * 8.0)
                    }
                    "blink" => {
                        let lit = ((now * 3.0).floor() as i32 + rig.phase as i32) % 2 == 0;
                        tf.scale = Vec3::splat(if lit { 1.0 } else { 0.0 });
                    }
                    _ => {}
                }
            }
        }
        // Crews: appear by their engine's side, walk to the post, work, walk back.
        let mut working_crews = 0;
        for (k, (unit, post)) in sc.crews.iter().zip(&sc.plan.crews).enumerate() {
            let stagger = (k % 6) as f32 * 0.12;
            let show = smooth((a - ARRIVE_S * 0.55 - stagger) / 0.5)
                * smooth((1.0 - leave) / 0.35 - stagger);
            let from = sc.plan.engines.get(post.engine).map_or(post.at, |e| e.at);
            let back = Vec2::new(from[0] - post.at[0], from[1] - post.at[1]).normalize_or_zero();
            let walk_in = 1.0 - smooth((a - ARRIVE_S * 0.55 - stagger) / (ARRIVE_S * 0.45 + 0.4));
            let off = (walk_in + smooth(leave / 0.8)) * WALK_M;
            let p = [post.at[0] + back.x * off, post.at[1] + back.y * off];
            let walking = (walk_in > 0.0 && walk_in < 1.0) || (leave > 0.0 && leave < 0.8);
            let face = if leave > 0.0 {
                back
            } else {
                Vec2::new(post.aim[0] - post.at[0], post.aim[1] - post.at[1]).normalize_or_zero()
            };
            if let Ok(mut tf) = tfs.get_mut(unit.root) {
                tf.translation = to_bevy(s, p, 0.2);
                tf.rotation = Quat::from_rotation_y(face.x.atan2(-face.y));
                tf.scale = Vec3::splat(CREW_SCALE * show);
            }
            for (e, rig) in &unit.parts {
                let Ok(mut tf) = tfs.get_mut(*e) else {
                    continue;
                };
                let ang = match rig.kind.as_str() {
                    "swing" if walking => (now * 7.0 + rig.phase).sin() * 0.5,
                    // The hose hand held forward, working the nozzle.
                    "tool" if working => -1.1 + (now * 2.3 + k as f32).sin() * 0.12,
                    _ => 0.0,
                };
                tf.rotation = Quat::from_axis_angle(rig.axis.normalize_or_zero(), ang);
            }
            let jet_on = working && show > 0.99 && walk_in <= 0.0;
            if let Ok(mut v) = vis.get_mut(sc.hoses[k]) {
                *v = if show > 0.5 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            let (je, jh) = &sc.jets[k];
            if let Ok(mut v) = vis.get_mut(*je) {
                *v = if jet_on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            if !jet_on {
                continue;
            }
            working_crews += 1;
            // The jet sweeps along the edge, back and forth.
            let sweep = (now * 0.9 + k as f32 * 1.3).sin() * 14.0;
            let to = [
                post.aim[0] + post.across.x * sweep,
                post.aim[1] + post.across.y * sweep,
            ];
            let (x0, y0) = (p, height_at(s, p) * EXAGGERATION + NOZZLE_M);
            let y1 = height_at(s, to) * EXAGGERATION + 1.0;
            let len = (to[0] - x0[0]).hypot(to[1] - x0[1]);
            let crown = 4.0 + 0.2 * len;
            let points: Vec<(Vec3, f32)> = (0..=ARC_SEGMENTS)
                .map(|j| {
                    let t = j as f32 / ARC_SEGMENTS as f32;
                    let q = [x0[0] + (to[0] - x0[0]) * t, x0[1] + (to[1] - x0[1]) * t];
                    let y = y0 + (y1 - y0) * t + crown * 4.0 * t * (1.0 - t);
                    let ripple = 1.0 + 0.18 * (t * 18.0 - now * 9.0).sin();
                    (to_bevy(s, q, 0.0).with_y(y), (0.9 + 2.2 * t) * ripple)
                })
                .collect();
            if let Some(m) = meshes.get_mut(jh) {
                *m = arc_mesh(&points);
            }
            // Steam where the water lands on the hot edge.
            if budget > 0 && noise.next() < caps.puffs_per_crew_s * envelope(a) * dt {
                budget -= 1;
                spawn_puff(
                    &mut commands,
                    &fx,
                    if noise.next() < 0.7 {
                        &fx.steam
                    } else {
                        &fx.mist
                    },
                    y1,
                    PuffSpec {
                        at: to_bevy(s, to, 1.5),
                        vel: Vec3::new(
                            noise.signed() * 2.5,
                            6.0 + noise.next() * 4.0,
                            noise.signed() * 2.5,
                        ),
                        size: 1.4,
                        grow: 3.0,
                        life: 2.6,
                        gravity: 0.0,
                        drag: 0.35,
                    },
                );
            }
        }
        // The doused ground keeps steaming for a while, dying down.
        if working && !sc.smoulder.is_empty() {
            let damp = 1.0 - ((a - ARRIVE_S) / WORK_S).clamp(0.0, 1.0);
            sc.puff_carry += caps.puffs_per_crew_s * 0.5 * working_crews.max(1) as f32 * damp * dt;
            while sc.puff_carry >= 1.0 && budget > 0 {
                sc.puff_carry -= 1.0;
                budget -= 1;
                let q = sc.smoulder
                    [(noise.next() * sc.smoulder.len() as f32) as usize % sc.smoulder.len()];
                let at = to_bevy(
                    s,
                    [q[0] + noise.signed() * 10.0, q[1] + noise.signed() * 10.0],
                    1.0,
                );
                spawn_puff(
                    &mut commands,
                    &fx,
                    if noise.next() < 0.5 {
                        &fx.mist
                    } else {
                        &fx.steam
                    },
                    at.y,
                    PuffSpec {
                        at,
                        vel: Vec3::new(
                            noise.signed() * 1.5,
                            4.0 + noise.next() * 3.0,
                            noise.signed() * 1.5,
                        ),
                        size: 1.8,
                        grow: 3.2,
                        life: 3.0,
                        gravity: 0.0,
                        drag: 0.3,
                    },
                );
            }
            sc.puff_carry = sc.puff_carry.min(4.0);
        }
        // The ground darkens as it is soaked and dries out of view as they leave.
        if let Some((_, m)) = &sc.wet {
            let soak = smooth((a - ARRIVE_S * 0.6) / 2.5) * smooth((1.0 - leave) / 1.0);
            if let Some(mat) = mats.get_mut(m) {
                mat.base_color.set_alpha(WET_ALPHA * soak);
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scn() -> Scenario {
        scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED)
    }

    /// Cells: a burnt square of `half` cells round `centre`, everything else unburnt.
    fn burnt_square(s: &Scenario, centre: Pos, half_m: f32) -> Vec<u8> {
        let g = s.grid;
        (0..g.len())
            .map(|i| {
                let p = s.cell_centre(scenario::Cell {
                    row: i / g.cols,
                    col: i % g.cols,
                });
                if (p[0] - centre[0]).abs() <= half_m && (p[1] - centre[1]).abs() <= half_m {
                    cell::BURNT
                } else {
                    cell::UNBURNT
                }
            })
            .collect()
    }

    #[test]
    fn counts_scale_with_the_edge_and_respect_the_caps() {
        let c = caps("projector");
        assert_eq!(unit_counts(100.0, 50, c), (1, 3));
        assert_eq!(unit_counts(900.0, 50, c), (2, 6));
        // A huge edge is capped.
        assert_eq!(unit_counts(50_000.0, 500, c), (4, 12));
        let low = caps("low");
        assert_eq!(unit_counts(50_000.0, 500, low), (2, 4));
        let high = caps("high");
        assert_eq!(unit_counts(50_000.0, 500, high), (6, 18));
        // Never more people than places to stand.
        assert_eq!(unit_counts(50_000.0, 2, c), (2, 2));
    }

    #[test]
    fn spread_picks_far_apart_points() {
        let pts: Vec<Pos> = (0..100).map(|i| [i as f32 * 10.0, 0.0]).collect();
        let k = spread(&pts, 3);
        assert_eq!(k.len(), 3);
        let mut xs: Vec<f32> = k.iter().map(|&i| pts[i][0]).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!((xs[0], xs[2]), (0.0, 990.0));
        assert!((xs[1] - 495.0).abs() <= 10.0, "{xs:?}");
        // Fewer points than asked for: each once.
        assert_eq!(spread(&pts[..2], 5).len(), 2);
        assert!(spread(&[], 3).is_empty());
    }

    #[test]
    fn crews_stand_outside_the_burnt_edge_aiming_at_it() {
        let s = scn();
        let c = [s.grid.width_m() * 0.5, s.grid.height_m() * 0.5];
        let cells = burnt_square(&s, c, 200.0);
        let shape = ExtinguishShape::Rect {
            bounds: [c[0] - 400.0, c[1] - 400.0, c[0] + 400.0, c[1] + 400.0],
        };
        let edge = burnt_edge(&s, &cells, &shape);
        assert!(!edge.is_empty());
        for e in &edge {
            // On the square's rim, pointing away from its centre.
            let r = (e.at[0] - c[0]).abs().max((e.at[1] - c[1]).abs());
            assert!(r > 200.0 - 2.0 * s.grid.cellsize, "edge point {r} m in");
            let away = Vec2::new(e.at[0] - c[0], e.at[1] - c[1]).normalize();
            assert!(e.out.dot(away) > 0.3, "{:?} vs {:?}", e.out, away);
        }
        let p = plan(&s, &cells, &shape, caps("projector"));
        assert!(!p.engines.is_empty() && p.engines.len() <= 4);
        assert!(p.crews.len() <= 12 && p.crews.len() >= p.engines.len());
        for post in &p.crews {
            let d = |q: Pos| (q[0] - c[0]).abs().max((q[1] - c[1]).abs());
            assert!(d(post.at) > d(post.aim), "the crew faces the burnt ground");
            assert!(post.engine < p.engines.len());
        }
        // Spread out along the edge, not bunched.
        let min_gap = p
            .crews
            .iter()
            .enumerate()
            .flat_map(|(i, a)| {
                p.crews[i + 1..]
                    .iter()
                    .map(move |b| (a.at[0] - b.at[0]).hypot(a.at[1] - b.at[1]))
            })
            .fold(f32::MAX, f32::min);
        assert!(min_gap > 60.0, "crews {min_gap} m apart");
    }

    #[test]
    fn nothing_burnt_falls_back_to_the_outline_and_engines_use_roads() {
        let s = scn();
        let road = s
            .roads
            .iter()
            .find(|r| r.drivable && r.points.len() > 2)
            .unwrap();
        let near = road.points[road.points.len() / 2];
        let shape = ExtinguishShape::Disc {
            centre: [near[0] + 120.0, near[1]],
            radius_m: 150.0,
        };
        let cells = vec![cell::UNBURNT; s.grid.len()];
        assert!(burnt_edge(&s, &cells, &shape).is_empty());
        let p = plan(&s, &cells, &shape, caps("low"));
        assert_eq!(p.engines.len(), 1);
        assert!(!p.crews.is_empty());
        let e = &p.engines[0];
        assert!(e.road.is_some(), "a road within reach is used");
        let (ri, si, f) = e.road.unwrap();
        let (q, _) = along(&s.roads[ri].points, si, f, 0.0);
        assert!((q[0] - e.at[0]).hypot(q[1] - e.at[1]) < 0.01);
    }

    #[test]
    fn along_walks_a_polyline_both_ways_and_clamps() {
        let pts = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
        let (p, d) = along(&pts, 0, 0.5, 80.0);
        assert!((p[0] - 100.0).abs() < 1e-3 && (p[1] - 30.0).abs() < 1e-3);
        assert_eq!(d, Vec2::Y);
        let (p, d) = along(&pts, 1, 0.5, -80.0);
        assert!((p[0] - 70.0).abs() < 1e-3 && p[1].abs() < 1e-3);
        assert_eq!(d, -Vec2::X);
        let (p, _) = along(&pts, 0, 0.0, 1e6);
        assert_eq!(p, [100.0, 100.0]);
    }

    #[test]
    fn the_scene_ends_and_leaves_nothing_on() {
        assert_eq!(envelope(-1.0), 0.0);
        assert_eq!(envelope(ARRIVE_S + 1.0), 1.0);
        assert_eq!(envelope(ARRIVE_S + WORK_S + LEAVE_S), 0.0);
        assert!(envelope(ARRIVE_S + WORK_S + LEAVE_S * 0.5) < 1.0);
    }

    #[test]
    fn disc_and_polygon_outlines_contain_their_centre() {
        let d = ExtinguishShape::Disc {
            centre: [500.0, 500.0],
            radius_m: 100.0,
        };
        assert!(inside(&outline(&d), [500.0, 500.0]));
        assert!(!inside(&outline(&d), [620.0, 500.0]));
        let t = ExtinguishShape::Polygon {
            vertices: vec![[0.0, 0.0], [100.0, 0.0], [0.0, 100.0]],
        };
        assert!(inside(&outline(&t), [20.0, 20.0]));
        assert!(!inside(&outline(&t), [80.0, 80.0]));
    }
}
