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
    pub sites: Vec<SiteState>,
    pub pending_warnings: Vec<(String, i64)>,
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
#[derive(Debug, Clone, Serialize)]
pub struct SiteState {
    pub id: String,
    pub pos: Pos,
    pub radius_m: f32,
    pub down_at_s: Option<i64>,
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
    pub fn warning_lost(&self, pos: Pos) -> bool {
        let covering = self
            .sites
            .iter()
            .filter(|s| distance(pos, s.pos) <= s.radius_m)
            .collect::<Vec<_>>();
        !covering.is_empty() && covering.iter().all(|s| s.down_at_s.is_some())
    }
}
impl World {
    pub(crate) fn refresh_hazard(&mut self) {
        if !self.scn.operations.road_routing
            && !self.scn.operations.intensity_exposure
            && !self.scn.operations.progressive_suppression
            && !self.scn.operations.communications
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
        if self.scn.operations.communications {
            let failed = self
                .d
                .hazard
                .sites
                .iter()
                .enumerate()
                .filter(|(_, s)| s.down_at_s.is_none() && self.d.hazard.at(s.pos).danger >= 0.35)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            for i in failed {
                self.d.hazard.sites[i].down_at_s = Some(t);
                let s = self.d.hazard.sites[i].clone();
                self.event(
                    "communication_outage",
                    format!(
                        "{} failed; warnings in its sole coverage area are delayed",
                        s.id
                    ),
                    self.scn.zone_of(s.pos),
                );
            }
        }
        let due = self
            .d
            .hazard
            .pending_warnings
            .iter()
            .filter(|(_, at)| *at <= t)
            .map(|(z, _)| z.clone())
            .collect::<Vec<_>>();
        self.d.hazard.pending_warnings.retain(|(_, at)| *at > t);
        for z in due {
            self.d.warned.insert(z.clone());
            self.event(
                "warning_delivered",
                format!("Delayed warning delivered in {z}"),
                Some(z),
            );
        }
    }
    pub(crate) fn operational_sites(&self) -> Vec<SiteState> {
        if !self.scn.communication_sites.is_empty() {
            return self
                .scn
                .communication_sites
                .iter()
                .map(|s| SiteState {
                    id: s.id.clone(),
                    pos: s.pos,
                    radius_m: s.radius_m,
                    down_at_s: None,
                })
                .collect();
        }
        // Maximum household coverage; no baseline reception is removed.
        let mut uncovered = self
            .scn
            .buildings
            .iter()
            .filter(|b| b.occupants > 0)
            .map(|b| b.pos)
            .collect::<Vec<_>>();
        let mut sites = Vec::new();
        while !uncovered.is_empty() && sites.len() < 10 {
            let best = self
                .scn
                .buildings
                .iter()
                .map(|b| b.pos)
                .max_by_key(|p| {
                    uncovered
                        .iter()
                        .filter(|q| distance(*p, **q) <= 1500.0)
                        .count()
                })
                .unwrap();
            uncovered.retain(|p| distance(*p, best) > 1500.0);
            sites.push(SiteState {
                id: format!("training_mast_{}", sites.len() + 1),
                pos: best,
                radius_m: 1500.0,
                down_at_s: None,
            });
        }
        sites
    }
}
