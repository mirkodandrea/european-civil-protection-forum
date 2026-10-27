//! Continuous visual reconstruction adapted from propagator_abm field.rs.
//! Only the renderer reads it. The authoritative ensemble, cutoffs, hectares,
//! exposure, roads and interventions stay on the original simulation grid.
use crate::Snapshot;
use bevy::prelude::*;
use scenario::{GridSpec, Pos};

#[derive(Resource)]
pub struct FirePresentation {
    pub continuous: bool,
}
impl Default for FirePresentation {
    fn default() -> Self {
        Self {
            continuous: std::env::var("TTX_FIRE_RENDER").as_deref() != Ok("cells"),
        }
    }
}
pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut presentation: ResMut<FirePresentation>,
    mut ctx: bevy_egui::EguiContexts,
) {
    if keys.just_pressed(KeyCode::KeyV) && !ctx.ctx_mut().wants_keyboard_input() {
        presentation.continuous = !presentation.continuous;
    }
}
pub struct FireField<'a> {
    pub snap: &'a Snapshot,
    pub grid: GridSpec,
}
pub struct Sample {
    pub coverage: f32,
    pub arrival: Option<f32>,
    pub intensity: f32,
}
impl FireField<'_> {
    fn corners(&self, p: Pos) -> [(usize, f32); 4] {
        let g = self.grid;
        let x = (p[0] / g.cellsize - 0.5).clamp(0., (g.cols - 1) as f32);
        let y = ((g.height_m() - p[1]) / g.cellsize - 0.5).clamp(0., (g.rows - 1) as f32);
        let (c, r) = (x.floor() as usize, y.floor() as usize);
        let (u, v) = (x.fract(), y.fract());
        [
            (r * g.cols + c, (1. - u) * (1. - v)),
            (r * g.cols + (c + 1).min(g.cols - 1), u * (1. - v)),
            ((r + 1).min(g.rows - 1) * g.cols + c, (1. - u) * v),
            (
                (r + 1).min(g.rows - 1) * g.cols + (c + 1).min(g.cols - 1),
                u * v,
            ),
        ]
    }
    /// Source warp is ±7 m on a 20 m grid; scale with the actual posting.
    pub fn warp(&self, p: Pos) -> Pos {
        let amplitude = self.grid.cellsize * 0.35;
        [
            p[0] + (noise(p[0] * 0.045, p[1] * 0.045, 11) - 0.5) * 2. * amplitude,
            p[1] + (noise(p[0] * 0.045, p[1] * 0.045, 29) - 0.5) * 2. * amplitude,
        ]
    }
    pub fn cleared(&self, p: Pos) -> bool {
        let g = self.grid;
        if p[0] < 0. || p[1] < 0. || p[0] >= g.width_m() || p[1] >= g.height_m() {
            return true;
        }
        let c = (p[0] / g.cellsize) as usize;
        let r = ((g.height_m() - p[1]) / g.cellsize) as usize;
        self.snap
            .cells
            .get(r.min(g.rows - 1) * g.cols + c.min(g.cols - 1))
            == Some(&3)
    }
    pub fn sample(&self, p: Pos) -> Sample {
        if self.cleared(p) {
            return Sample {
                coverage: 0.,
                arrival: None,
                intensity: 0.,
            };
        }
        let mut coverage = 0.;
        let mut arrival = 0.;
        let mut timed_weight = 0.;
        let mut intensity = 0.;
        for (i, w) in self.corners(self.warp(p)) {
            if !matches!(self.snap.cells.get(i), Some(1 | 2)) {
                continue;
            }
            coverage += w;
            intensity += w * self.snap.intensity.get(i).copied().unwrap_or(0.);
            if let Some(&a) = self.snap.arrival.get(i).filter(|a| **a != i32::MIN) {
                arrival += w * a as f32;
                timed_weight += w;
            }
        }
        // Source rule: missing/unreached neighbours delay the visual arrival
        // rather than making the whole stand catch at one cell timestamp.
        Sample {
            coverage,
            arrival: (timed_weight > 0.)
                .then(|| arrival / timed_weight + (1. - timed_weight) * 90.),
            intensity,
        }
    }
}

fn bilinear(a: f32, b: f32, c: f32, d: f32, u: f32, v: f32) -> f32 {
    (a * (1. - u) + b * u) * (1. - v) + (c * (1. - u) + d * u) * v
}
/// Smooth value noise in `[0, 1]`, on a unit lattice.
pub fn noise(x: f32, y: f32, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let corner = |ix: f32, iy: f32| {
        let mut h = seed
            ^ (ix as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (iy as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 29;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 32;
        (h >> 40) as f32 / (1u32 << 24) as f32
    };
    bilinear(
        corner(x0, y0),
        corner(x0 + 1.0, y0),
        corner(x0, y0 + 1.0),
        corner(x0 + 1.0, y0 + 1.0),
        sx,
        sy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(code: u8) -> Snapshot {
        Snapshot {
            cells: vec![code; 9],
            arrival: vec![120; 9],
            intensity: vec![3000.; 9],
            ..default()
        }
    }
    fn grid() -> GridSpec {
        GridSpec {
            rows: 3,
            cols: 3,
            cellsize: 20.,
        }
    }
    #[test]
    fn confirmed_uniform_fields_keep_their_values_and_are_deterministic() {
        let s = snapshot(1);
        let f = FireField {
            snap: &s,
            grid: grid(),
        };
        let a = f.sample([30., 30.]);
        let b = f.sample([30., 30.]);
        assert!((a.coverage - 1.).abs() < 1e-6);
        assert!((a.arrival.unwrap() - 120.).abs() < 1e-4);
        assert!((a.intensity - 3000.).abs() < 0.001);
        assert_eq!(a.coverage, b.coverage);
        let p = [30., 30.];
        let warped = f.warp(p);
        assert!((warped[0] - p[0]).abs() <= 7. && (warped[1] - p[1]).abs() <= 7.);
    }
    #[test]
    fn halo_arrivals_do_not_become_confirmed_fire_and_clearings_stay_open() {
        let mut s = snapshot(5);
        let g = grid();
        let f = FireField { snap: &s, grid: g };
        let a = f.sample([30., 30.]);
        assert_eq!(a.coverage, 0.);
        assert!(a.arrival.is_none());
        assert_eq!(a.intensity, 0.);
        s.cells.fill(1);
        s.cells[4] = 3;
        let f = FireField { snap: &s, grid: g };
        assert_eq!(f.sample([30., 30.]).coverage, 0.);
        assert_eq!(f.sample([-1., 30.]).coverage, 0.);
    }
    #[test]
    fn unreached_corners_delay_visual_arrival_without_changing_snapshot() {
        let mut s = snapshot(1);
        s.cells[4] = 0;
        s.arrival[4] = i32::MIN;
        let before = s.cells.clone();
        let a = FireField {
            snap: &s,
            grid: grid(),
        }
        .sample([30., 30.]);
        assert!(a.coverage > 0. && a.coverage < 1.);
        assert!(a.arrival.unwrap() > 120.);
        assert_eq!(s.cells, before);
        assert_eq!(s.arrival[4], i32::MIN);
    }
}
