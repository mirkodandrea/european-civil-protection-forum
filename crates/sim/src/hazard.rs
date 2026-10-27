//! Ensemble-aware exposure and instantaneous danger, adapted from ABM layers.
//! Sources belong to the thresholded fire; intensity is the mean of reached runs.
use crate::{network::distance, World};
use fire::CellFire;
use scenario::Pos;
use serde::Serialize;
#[derive(Debug, Clone, Default, Serialize)]
pub struct HazardState {
    pub updated_s: i64,
    pub sources: Vec<Source>,
    pub loads: Vec<Load>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub cell_radius_m: f32,
    pub pos: Pos,
    pub radiant_m: f32,
    pub ember_m: f32,
    pub wind: [f32; 2],
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Load {
    pub radiant: f32,
    pub ember: f32,
    pub danger: f32,
}
impl HazardState {
    pub fn at(&self, pos: Pos) -> Load {
        let mut load = Load::default();
        for s in &self.sources {
            let d = distance(pos, s.pos);
            let radiant = (1.0 - (d - s.cell_radius_m).max(0.0) / s.radiant_m).clamp(0.0, 1.0);
            let dot = if d > 0.01 {
                ((pos[0] - s.pos[0]) * s.wind[0] + (pos[1] - s.pos[1]) * s.wind[1]) / d
            } else {
                1.0
            };
            let ember = if dot > 0.0 && d < s.ember_m {
                dot * (-d / 700.0).exp()
            } else {
                0.0
            };
            load.radiant = load.radiant.max(radiant);
            load.ember = load.ember.max(ember);
            // Remote structure ember exposure is not a lethal pedestrian field.
            load.danger = load
                .danger
                .max(radiant.max(if d < 400.0 { ember * 0.3 } else { 0.0 }));
        }
        load
    }
}
impl World {
    pub(crate) fn refresh_hazard(&mut self) {
        if !self.scn.operations.road_routing
            && !self.scn.operations.intensity_exposure
            && !self.scn.operations.progressive_suppression
            && !self.scn.operations.traffic
        {
            return;
        }
        let t = self.time_s();
        if t - self.d.hazard.updated_s < 10 {
            return;
        }
        let weather = self.fire.weather();
        let a = (weather.wind_dir_deg as f32 + 180.0).to_radians();
        let wind = [a.sin(), a.cos()];
        self.d.hazard.sources = self
            .fire
            .state()
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == CellFire::Burning)
            .map(|(i, _)| {
                let intensity = self.fire.intensity()[i].max(200.0);
                Source {
                    cell_radius_m: self.scn.grid.cellsize * std::f32::consts::FRAC_1_SQRT_2,
                    pos: self.scn.cell_centre(scenario::Cell {
                        row: i / self.scn.grid.cols,
                        col: i % self.scn.grid.cols,
                    }),
                    radiant_m: (4.0 * fire::flame_length_m(intensity)).clamp(10.0, 200.0),
                    ember_m: (120.0
                        * (intensity / 500.0).sqrt()
                        * (1.0 + weather.wind_speed_kmh as f32 / 20.0))
                        .clamp(0.0, 2500.0),
                    wind,
                }
            })
            .collect();
        self.d.hazard.loads = self
            .scn
            .buildings
            .iter()
            .map(|b| self.d.hazard.at(b.pos))
            .collect();
        self.d.hazard.updated_s = t;
    }
}
