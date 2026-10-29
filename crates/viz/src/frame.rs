//! Per-frame display state, and the one place the world ↔ Bevy frame flip lives.
//!
//! World frame: x east, y north, metres. Bevy: Y up, −Z forward, so
//! world `(x, y)` at elevation `h` becomes `(x − W/2, h·EXAGGERATION, −(y − H/2))`,
//! centred on the map.

use bevy::prelude::*;
use scenario::{Pos, Scenario};

use crate::{Scn, Snapshot};

/// Vertical exaggeration of the terrain.
pub const EXAGGERATION: f32 = 1.3;

pub fn to_bevy(scn: &Scenario, p: Pos, lift: f32) -> Vec3 {
    let h = height_at(scn, p) * EXAGGERATION + lift;
    Vec3::new(
        p[0] - scn.grid.width_m() * 0.5,
        h,
        -(p[1] - scn.grid.height_m() * 0.5),
    )
}

pub fn to_world(scn: &Scenario, v: Vec3) -> Pos {
    [
        v.x + scn.grid.width_m() * 0.5,
        -v.z + scn.grid.height_m() * 0.5,
    ]
}

/// The drawn ground an aircraft at `p`, heading along `dir` (world, unit
/// length), must clear: the highest terrain within reach ahead and behind,
/// lowered with distance so the aircraft climbs before a ridge and sinks after
/// it instead of hugging every fold. Exaggerated, like `to_bevy`'s heights.
pub fn flight_floor(scn: &Scenario, p: Pos, dir: Vec2) -> f32 {
    /// How far ahead and behind the terrain is looked at, and the climb
    /// gradient the floor is allowed to fall away with.
    const REACH_M: f32 = 600.0;
    const STEP_M: f32 = 25.0;
    const GRADIENT: f32 = 0.3;
    let n = (REACH_M / STEP_M) as i32;
    (-n..=n)
        .map(|k| {
            let d = k as f32 * STEP_M;
            let q = [p[0] + dir.x * d, p[1] + dir.y * d];
            height_at(scn, q) * EXAGGERATION - d.abs() * GRADIENT
        })
        .fold(f32::MIN, f32::max)
}

/// Bilinear elevation at a world position, metres (not exaggerated).
pub fn height_at(scn: &Scenario, p: Pos) -> f32 {
    if let Some(t) = &scn.render_terrain {
        let x = (p[0] / t.posting_m).clamp(0.0, (t.cols - 1) as f32);
        let y = ((t.world_size_m[1] - p[1]) / t.posting_m).clamp(0.0, (t.rows - 1) as f32);
        let (c, r) = (x.floor() as usize, y.floor() as usize);
        let (c1, r1) = ((c + 1).min(t.cols - 1), (r + 1).min(t.rows - 1));
        let (u, v) = (x - c as f32, y - r as f32);
        let a = t.elev[r * t.cols + c] * (1.0 - u) + t.elev[r * t.cols + c1] * u;
        let b = t.elev[r1 * t.cols + c] * (1.0 - u) + t.elev[r1 * t.cols + c1] * u;
        return a * (1.0 - v) + b * v;
    }
    let g = scn.grid;
    let fx = (p[0] / g.cellsize - 0.5).clamp(0.0, (g.cols - 1) as f32);
    let fy = ((g.height_m() - p[1]) / g.cellsize - 0.5).clamp(0.0, (g.rows - 1) as f32);
    let (c0, r0) = (fx.floor() as usize, fy.floor() as usize);
    let (c1, r1) = ((c0 + 1).min(g.cols - 1), (r0 + 1).min(g.rows - 1));
    let (tx, ty) = (fx - c0 as f32, fy - r0 as f32);
    let at = |r: usize, c: usize| scn.dem[r * g.cols + c];
    let a = at(r0, c0) + (at(r0, c1) - at(r0, c0)) * tx;
    let b = at(r1, c0) + (at(r1, c1) - at(r1, c0)) * tx;
    a + (b - a) * ty
}

/// The cell codes on screen this frame, and which cells changed since last frame.
#[derive(Resource, Default)]
pub struct Frame {
    pub display_s: i64,
    pub cells: Vec<u8>,
    pub changed: Vec<usize>,
    pub all_changed: bool,
}

/// The screen shows the live world: during a resolution the snapshot moves
/// forward one simulated minute at a time.
pub fn update(snap: Res<Snapshot>, scn: Res<Scn>, mut frame: ResMut<Frame>) {
    let n = scn.0.grid.len();
    if snap.cells.len() != n || (!snap.is_changed() && frame.cells.len() == n) {
        frame.changed.clear();
        frame.all_changed = false;
        return;
    }
    frame.all_changed = frame.cells.len() != n;
    frame.changed = if frame.all_changed {
        Vec::new()
    } else {
        (0..n)
            .filter(|&i| frame.cells[i] != snap.cells[i])
            .collect()
    };
    frame.cells = snap.cells.clone();
    frame.display_s = snap.time_s;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_north_is_minus_z() {
        let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let p = [1234.0, 3210.0];
        let v = to_bevy(&scn, p, 0.0);
        let back = to_world(&scn, v);
        assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
        assert!(to_bevy(&scn, [0.0, 3999.0], 0.0).z < to_bevy(&scn, [0.0, 1.0], 0.0).z);
    }

    #[test]
    fn supplied_visual_nodes_are_north_first_and_leave_physics_unchanged() {
        let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let physical = scn.dem.clone();
        // A deliberately tiny planar fixture verifies node rather than cell sampling.
        scn.render_terrain = Some(scenario::RenderTerrain {
            rows: 2,
            cols: 2,
            posting_m: 4000.0,
            world_size_m: [4000.0, 4000.0],
            elev: vec![10.0, 20.0, 30.0, 40.0],
        });
        assert_eq!(height_at(&scn, [0.0, 4000.0]), 10.0);
        assert_eq!(height_at(&scn, [4000.0, 0.0]), 40.0);
        assert_eq!(height_at(&scn, [2000.0, 2000.0]), 25.0);
        assert_eq!(scn.dem, physical);
    }

    #[test]
    fn height_is_bilinear_between_cell_centres() {
        let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let c = scenario::Cell { row: 50, col: 50 };
        let p = scn.cell_centre(c);
        assert!((height_at(&scn, p) - scn.elevation_at(c)).abs() < 1e-3);
    }

    #[test]
    fn a_tanker_pass_clears_the_terrain_ahead() {
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let (w, h) = (s.grid.width_m(), s.grid.height_m());
        for dir in [Vec2::X, Vec2::Y, Vec2::new(1.0, 1.0).normalize()] {
            for k in 0..60 {
                let p = [w * (k as f32 + 0.5) / 60.0, h * ((k * 37 % 60) as f32 + 0.5) / 60.0];
                let floor = flight_floor(&s, p, dir);
                // Never below the ground under the aircraft, nor ever below
                // the ridge it is about to cross once within the last 100 m.
                assert!(floor >= height_at(&s, p) * EXAGGERATION - 1e-3);
                for d in [-100.0, -50.0, 50.0, 100.0] {
                    let q = [p[0] + dir.x * d, p[1] + dir.y * d];
                    assert!(floor >= height_at(&s, q) * EXAGGERATION - 0.3 * 100.0 - 1e-3);
                }
            }
        }
    }
}
