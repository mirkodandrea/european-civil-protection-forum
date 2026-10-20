//! Reviewable projection from a frozen present, never the realised future.
use crate::{FireOptions, FireSim, FireSnapshot};
use anyhow::{ensure, Result};
use scenario::{GridSpec, Scenario, WeatherSpec};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForecastSpec {
    pub seed: u64,
    /// Weather groups, each retaining the authoritative core's realizations.
    pub members: usize,
    pub horizons_s: Vec<i64>,
    pub direction_spread_deg: f64,
    pub speed_spread_fraction: f64,
    /// Explicit EXCON bulletin only; no future scenario injects are inspected.
    pub bulletin: Vec<ForecastWeather>,
}
impl Default for ForecastSpec {
    fn default() -> Self {
        Self {
            seed: 1,
            members: 4,
            horizons_s: vec![3600, 10800],
            direction_spread_deg: 10.0,
            speed_spread_fraction: 0.1,
            bulletin: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastWeather {
    pub after_s: i64,
    pub weather: WeatherSpec,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastHorizon {
    pub after_s: i64,
    pub probability: Vec<f32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forecast {
    pub issued_s: i64,
    pub grid: GridSpec,
    pub spec: ForecastSpec,
    pub realizations_per_member: usize,
    pub assumptions: String,
    pub horizons: Vec<ForecastHorizon>,
}
impl ForecastSpec {
    pub fn check(&self) -> Result<()> {
        ensure!(
            (1..=16).contains(&self.members),
            "forecast members must be 1..16"
        );
        ensure!(
            !self.horizons_s.is_empty()
                && self.horizons_s.len() <= 4
                && self.horizons_s.iter().all(|v| (1..=21600).contains(v))
                && self.horizons_s.windows(2).all(|w| w[0] < w[1]),
            "forecast horizons must increase, within 1..21600 s"
        );
        ensure!(
            self.direction_spread_deg.is_finite()
                && (0.0..=90.0).contains(&self.direction_spread_deg)
                && self.speed_spread_fraction.is_finite()
                && (0.0..=0.9).contains(&self.speed_spread_fraction),
            "invalid forecast weather spread"
        );
        ensure!(
            self.bulletin.len() <= 8
                && self
                    .bulletin
                    .windows(2)
                    .all(|w| w[0].after_s < w[1].after_s),
            "bulletin must have <=8 increasing times"
        );
        for b in &self.bulletin {
            ensure!(
                (1..=21600).contains(&b.after_s)
                    && b.weather.wind_dir_deg.is_finite()
                    && b.weather.wind_speed_kmh.is_finite()
                    && (0.0..=200.0).contains(&b.weather.wind_speed_kmh)
                    && b.weather.moisture_pct.is_finite()
                    && (0.0..=100.0).contains(&b.weather.moisture_pct),
                "invalid bulletin weather"
            );
        }
        Ok(())
    }
}
impl Forecast {
    pub fn check(&self, grid: GridSpec) -> Result<()> {
        self.spec.check()?;
        ensure!(
            self.grid.rows == grid.rows
                && self.grid.cols == grid.cols
                && self.grid.cellsize == grid.cellsize
                && self.issued_s >= 0
                && self.horizons.len() == self.spec.horizons_s.len()
                && self.realizations_per_member > 0
                && self.assumptions.len() <= 4000,
            "forecast does not match map"
        );
        for (h, t) in self.horizons.iter().zip(&self.spec.horizons_s) {
            ensure!(
                h.after_s == *t
                    && h.probability.len() == grid.len()
                    && h.probability
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "invalid forecast probability field"
            );
        }
        Ok(())
    }
}
/// Owns all inputs so callers can run it off the exercise mutex/render thread.
pub struct ForecastJob {
    scn: Arc<Scenario>,
    snapshot: FireSnapshot,
    options: FireOptions,
    spec: ForecastSpec,
}
impl ForecastJob {
    pub fn capture(fire: &FireSim, scn: Arc<Scenario>, spec: ForecastSpec) -> Result<Self> {
        spec.check()?;
        ensure!(
            scn.grid
                .len()
                .saturating_mul(spec.members)
                .saturating_mul(fire.options().realizations)
                <= 32_000_000,
            "forecast workload too large; reduce weather members"
        );
        let mut snapshot = fire.snapshot();
        // Preserve present interventions but exclude any future boundary edits.
        snapshot
            .core
            .scheduler_events
            .retain(|(time, _)| *time <= snapshot.time_s);
        Ok(Self {
            scn,
            snapshot,
            options: fire.options(),
            spec,
        })
    }
    pub fn run(self) -> Result<Forecast> {
        let issued_s = self.snapshot.time_s;
        let n = self.scn.grid.len();
        let mut horizons = self
            .spec
            .horizons_s
            .iter()
            .map(|after_s| ForecastHorizon {
                after_s: *after_s,
                probability: vec![0.0; n],
            })
            .collect::<Vec<_>>();
        for member in 0..self.spec.members {
            let seed = mix(self.spec.seed ^ mix(issued_s as u64) ^ mix(member as u64));
            let mut sim = FireSim::with_options(&self.scn, seed, self.options)?;
            sim.restore(&self.snapshot, seed)?;
            let d = (unit(mix(seed ^ 11)) * 2.0 - 1.0) * self.spec.direction_spread_deg;
            let speed = 1.0 + (unit(mix(seed ^ 13)) * 2.0 - 1.0) * self.spec.speed_spread_fraction;
            let perturb = |w: WeatherSpec| WeatherSpec {
                wind_dir_deg: (w.wind_dir_deg + d).rem_euclid(360.0),
                wind_speed_kmh: w.wind_speed_kmh * speed,
                ..w
            };
            sim.set_weather(perturb(self.snapshot.weather))?;
            let mut bulletin = self.spec.bulletin.iter().peekable();
            for h in &mut horizons {
                let end = issued_s + h.after_s;
                while let Some(b) = bulletin.peek().filter(|b| issued_s + b.after_s <= end) {
                    let at = issued_s + b.after_s;
                    sim.advance(at - sim.time_s())?;
                    sim.set_weather(perturb(b.weather))?;
                    bulletin.next();
                }
                sim.advance(end - sim.time_s())?;
                sim.refresh()?;
                for (sum, v) in h.probability.iter_mut().zip(sim.probability()) {
                    *sum += *v / self.spec.members as f32;
                }
            }
        }
        Ok(Forecast{issued_s,grid:self.scn.grid,spec:self.spec,realizations_per_member:self.options.realizations,
            assumptions:"Authoritative present checkpoint, current weather and explicit EXCON bulletin only. Existing fuel breaks and water retained; no future crew work or orders. Weather-perturbed ensembles; probabilities describe this training model, not an operational prediction.".into(),horizons})
    }
}
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn unit(x: u64) -> f64 {
    (x >> 11) as f64 / (1u64 << 53) as f64
}
