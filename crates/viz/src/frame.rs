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
    Vec3::new(p[0] - scn.grid.width_m() * 0.5, h, -(p[1] - scn.grid.height_m() * 0.5))
}

#[cfg(test)]
pub fn to_world(scn: &Scenario, v: Vec3) -> Pos {
    [v.x + scn.grid.width_m() * 0.5, -v.z + scn.grid.height_m() * 0.5]
}

/// Bilinear elevation at a world position, metres (not exaggerated).
pub fn height_at(scn: &Scenario, p: Pos) -> f32 {
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
    frame.changed = if frame.all_changed { Vec::new() } else { (0..n).filter(|&i| frame.cells[i] != snap.cells[i]).collect() };
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
    fn height_is_bilinear_between_cell_centres() {
        let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        let c = scenario::Cell { row: 50, col: 50 };
        let p = scn.cell_centre(c);
        assert!((height_at(&scn, p) - scn.elevation_at(c)).abs() < 1e-3);
    }
}
