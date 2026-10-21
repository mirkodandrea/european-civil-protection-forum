//! The wildfire model: the PROPAGATOR Rust core running in-process as a
//! **Monte Carlo ensemble** (default 32 realizations), reduced to one fire.
//!
//! ## From an ensemble to one fire
//!
//! A cell counts as reached when a fraction `cutoff` of the realizations
//! have reached it (default 0.5 = the median fire; 0.25 = a pessimistic,
//! planning fire). Its arrival time is the corresponding **quantile of the
//! realizations' arrival times**: the `ceil(cutoff·R)`-th earliest. Arrival
//! times never change once set, so a decided cell stays decided, the fire only
//! grows, and "burning" / "burnt", building exposure and the live animation
//! all follow from one consistent arrival-time field. The fraction of
//! realizations that reached each cell is kept as [`FireSim::probability`]
//! (the uncertainty halo in the UIs).
//!
//! Every way the exercise can touch the fire goes through the core's own
//! boundary conditions, never by editing its state:
//!
//! | Lever | Core field | Effect |
//! |---|---|---|
//! | Ignition | `ignitions` | lights a patch |
//! | Fire line (fuel break) | `vegetation_changes` → non-vegetated | the front cannot cross cleared cells |
//! | Water drop | `additional_moisture` | wets fuel ahead of the front; decays 1 %/min |
//! | Extinguish (EXCON) | `vegetation_changes` → non-vegetated over the area | nothing in the area can burn again; burning cells there are shown as burnt |
//! | Weather | `wind_dir`, `wind_speed`, `moisture` | from the next event on |
//!
//! Two things learnt the hard way in `propagator_abm` and kept here:
//! - `vegetation_changes` is **sparse by NaN** — every non-NaN cell *sets* the
//!   fuel. Always start from a NaN-filled grid.
//! - Suppression only acts on unburnt fuel: the kernel never un-lights a cell.
//!   Burn-out (burning → burnt) is this crate's own ageing layer.

use anyhow::{Context, Result};
use propagator_core::{
    BoundaryConditions, FieldInput, Grid2, Ignitions, OobMode, Propagator, PropagatorCheckpoint,
    PropagatorConfig,
};
use scenario::{fuel, Cell, GridSpec, Pos, Scenario, WeatherSpec};
use serde::Serialize;

pub mod forecast;

/// Percentage points of fuel moisture per litre per square metre. Fine fuel
/// load ~1 kg/m², about a third of a drop reaches it: 1 L/m² ≈ 30 points.
/// The core's moisture of extinction is 30 points.
pub const MOISTURE_POINTS_PER_LITRE: f32 = 30.0;
/// Accounting cap on added moisture, points.
pub const MAX_ADDED_MOISTURE_PTS: f32 = 100.0;
/// A cell keeps burning this long after ignition, then it is burnt out.
pub const BURNOUT_S: i32 = 20 * 60;
const MOISTURE_DECAY_PER_MIN: f32 = 0.01;
/// The ensemble summary (quantile arrivals, probability) is recomputed at most
/// this often in simulated seconds. Arrival times are exact whenever computed;
/// only the moment a cell appears can lag by up to this much.
pub const REFRESH_S: i64 = 10;
/// A single-cell ignition fizzles ~20 % of the time; ignitions always light at
/// least this radius.
pub const MIN_IGNITION_RADIUS_M: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[repr(u8)]
pub enum CellFire {
    Unburnt = 0,
    Burning = 1,
    Burnt = 2,
}

/// Byram flame length (m) from fireline intensity (kW/m): `L = 0.0775 · I^0.46`.
pub fn flame_length_m(fli_kw_m: f32) -> f32 {
    if fli_kw_m <= 0.0 {
        0.0
    } else {
        0.0775 * fli_kw_m.powf(0.46)
    }
}

/// Ensemble size and the probability cutoff that defines the "actual" fire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FireOptions {
    pub realizations: usize,
    /// Fraction of realizations that must reach a cell, in (0, 1].
    pub cutoff: f32,
}

impl Default for FireOptions {
    fn default() -> Self {
        FireOptions {
            realizations: 32,
            cutoff: 0.5,
        }
    }
}

/// Everything needed to put a [`FireSim`] back to an earlier moment.
#[derive(Clone)]
pub struct FireSnapshot {
    pending_veg: Option<Grid2<f64>>,
    pending_moisture: Option<Grid2<f32>>,
    core: PropagatorCheckpoint,
    weather: WeatherSpec,
    time_s: i64,
    state: Vec<CellFire>,
    ignited_at: Vec<i32>,
    probability: Vec<f32>,
    intensity: Vec<f32>,
    cleared: Vec<bool>,
    extinguished: Vec<bool>,
    added_moisture: Vec<f32>,
}

pub struct FireSim {
    core: Propagator,
    grid: GridSpec,
    burnable: Vec<bool>,
    cleared_fuel_code: i32,
    weather: WeatherSpec,
    time_s: i64,
    state: Vec<CellFire>,
    /// Quantile arrival time of each cell (see module docs); `i32::MIN` if not reached.
    ignited_at: Vec<i32>,
    /// Fraction of realizations that have reached each cell.
    probability: Vec<f32>,
    /// Mean fireline intensity (kW/m) over the realizations that reached each cell.
    intensity: Vec<f32>,
    options: FireOptions,
    /// Scratch buffer for the per-cell arrival quantile.
    scratch: Vec<i32>,
    last_refresh_s: i64,
    /// Cells turned into fuel breaks.
    cleared: Vec<bool>,
    /// Cells put out by the EXCON; drawn as burnt.
    extinguished: Vec<bool>,
    added_moisture: Vec<f32>,
    /// Pending boundary-condition edits, flushed at the start of `advance`.
    pending_veg: Option<Grid2<f64>>,
    pending_moisture: Option<Grid2<f32>>,
}

impl FireSim {
    /// Default ensemble (32 realizations, cutoff 0.5).
    pub fn new(scn: &Scenario, seed: u64) -> Result<FireSim> {
        FireSim::with_options(scn, seed, FireOptions::default())
    }

    pub fn with_options(scn: &Scenario, seed: u64, options: FireOptions) -> Result<FireSim> {
        anyhow::ensure!(options.realizations >= 1, "need at least one realization");
        anyhow::ensure!(
            options.cutoff > 0.0 && options.cutoff <= 1.0,
            "cutoff must be in (0, 1]"
        );
        let grid = scn.grid;
        let veg = Grid2::from_vec(grid.rows, grid.cols, scn.fuel.clone());
        let dem = Grid2::from_vec(
            grid.rows,
            grid.cols,
            scn.dem.iter().map(|&v| v as f64).collect(),
        );
        let mut config = PropagatorConfig::new(veg, dem);
        config.cellsize = grid.cellsize as f64;
        config.realizations = options.realizations;
        config.do_spotting = scn.operations.spotting;
        if !scn.fuel_definitions.is_empty() {
            let defs = scn
                .fuel_definitions
                .iter()
                .map(|f| propagator_core::FuelDef {
                    id: f.id,
                    name: f.name.clone(),
                    v0: f.v0,
                    d0: f.d0,
                    d1: f.d1,
                    hhv: f.hhv,
                    humidity: f.humidity,
                    spotting: f.spotting,
                    prob_ign_by_embers: f.prob_ign_by_embers,
                    burn: f.burn,
                    spread_probability: f.spread_probability.clone(),
                })
                .collect::<Vec<_>>();
            config.fuels = Some(propagator_core::FuelSystem::from_defs(&defs)?);
        }
        config.seed = Some(seed);
        config.oob_mode = OobMode::Ignore;
        config.n_threads = None; // realizations run in parallel; results do not depend on thread count
        let core = Propagator::new(config).context("building PROPAGATOR core")?;
        let n = grid.len();
        let mut fire = FireSim {
            core,
            grid,
            cleared_fuel_code: scn
                .fuel_definitions
                .iter()
                .find(|f| !f.burn)
                .map(|f| f.id)
                .unwrap_or(fuel::NON_VEGETATED),
            burnable: scn
                .fuel
                .iter()
                .map(|&f| {
                    if scn.fuel_definitions.is_empty() {
                        f != fuel::NON_VEGETATED && f != 0
                    } else {
                        scn.fuel_definitions.iter().any(|d| d.id == f && d.burn)
                    }
                })
                .collect(),
            weather: scn.weather,
            time_s: 0,
            state: vec![CellFire::Unburnt; n],
            ignited_at: vec![i32::MIN; n],
            probability: vec![0.0; n],
            intensity: vec![0.0; n],
            options,
            scratch: Vec::with_capacity(options.realizations),
            last_refresh_s: -REFRESH_S,
            cleared: vec![false; n],
            extinguished: vec![false; n],
            added_moisture: vec![0.0; n],
            pending_veg: None,
            pending_moisture: None,
        };
        let bc = fire.weather_bc();
        fire.core
            .set_boundary_conditions(bc)
            .context("initial weather")?;
        Ok(fire)
    }

    pub fn grid(&self) -> GridSpec {
        self.grid
    }
    pub fn time_s(&self) -> i64 {
        self.time_s
    }
    pub fn weather(&self) -> WeatherSpec {
        self.weather
    }
    pub fn state(&self) -> &[CellFire] {
        &self.state
    }
    /// Fraction of realizations that have reached each cell (0..=1).
    pub fn probability(&self) -> &[f32] {
        &self.probability
    }
    /// Mean fireline intensity (kW/m, Byram) over the realizations that reached each cell.
    pub fn intensity(&self) -> &[f32] {
        &self.intensity
    }
    pub fn options(&self) -> FireOptions {
        self.options
    }
    pub fn cleared(&self) -> &[bool] {
        &self.cleared
    }
    pub fn added_moisture(&self) -> &[f32] {
        &self.added_moisture
    }
    /// Simulated second each cell ignited, `i32::MIN` if never.
    pub fn arrival_times(&self) -> &[i32] {
        &self.ignited_at
    }

    pub fn is_burnable(&self, c: Cell) -> bool {
        self.burnable[self.grid.index(c)] && !self.cleared[self.grid.index(c)]
    }

    pub fn cell_state(&self, c: Cell) -> CellFire {
        self.state[self.grid.index(c)]
    }

    pub fn burning_cells(&self) -> usize {
        self.state
            .iter()
            .filter(|s| **s == CellFire::Burning)
            .count()
    }

    /// Area burning or burnt, hectares.
    pub fn burned_area_ha(&self) -> f64 {
        let n = self
            .state
            .iter()
            .filter(|s| **s != CellFire::Unburnt)
            .count();
        n as f64 * (self.grid.cellsize as f64).powi(2) / 10_000.0
    }

    fn cell_of(&self, p: Pos) -> Option<Cell> {
        let cs = self.grid.cellsize;
        if p[0] < 0.0 || p[1] < 0.0 {
            return None;
        }
        let (col, from_south) = ((p[0] / cs) as usize, (p[1] / cs) as usize);
        (col < self.grid.cols && from_south < self.grid.rows).then(|| Cell {
            row: self.grid.rows - 1 - from_south,
            col,
        })
    }

    fn centre(&self, c: Cell) -> Pos {
        let cs = self.grid.cellsize;
        [
            (c.col as f32 + 0.5) * cs,
            ((self.grid.rows - 1 - c.row) as f32 + 0.5) * cs,
        ]
    }

    /// All cells whose centre lies within `radius_m` of `p`.
    pub fn cells_in_radius(&self, p: Pos, radius_m: f32) -> Vec<Cell> {
        let cs = self.grid.cellsize;
        let r = (radius_m / cs).ceil() as i64 + 1;
        let Some(c0) = self.cell_of([
            p[0].clamp(0.0, self.grid.width_m() - 1.0),
            p[1].clamp(0.0, self.grid.height_m() - 1.0),
        ]) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dr in -r..=r {
            for dc in -r..=r {
                let (row, col) = (c0.row as i64 + dr, c0.col as i64 + dc);
                if row < 0
                    || col < 0
                    || row >= self.grid.rows as i64
                    || col >= self.grid.cols as i64
                {
                    continue;
                }
                let c = Cell {
                    row: row as usize,
                    col: col as usize,
                };
                let q = self.centre(c);
                if (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) <= radius_m * radius_m {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Cells covered by a straight band from `a` to `b`, `width_m` wide.
    pub fn cells_along(&self, a: Pos, b: Pos, width_m: f32) -> Vec<Cell> {
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let step = self.grid.cellsize * 0.5;
        let n = (len / step).ceil().max(1.0) as usize;
        let half = (width_m * 0.5).max(self.grid.cellsize * 0.75);
        let mut seen = vec![false; self.grid.len()];
        let mut out = Vec::new();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            for c in self.cells_in_radius(p, half) {
                let idx = self.grid.index(c);
                if !seen[idx] {
                    seen[idx] = true;
                    out.push(c);
                }
            }
        }
        out
    }

    /// Light a patch of burnable fuel. Returns the number of cells lit.
    pub fn ignite(&mut self, at: Pos, radius_m: f32) -> Result<usize> {
        let cells: Vec<(usize, usize)> = self
            .cells_in_radius(at, radius_m.max(MIN_IGNITION_RADIUS_M))
            .into_iter()
            .filter(|c| {
                let i = self.grid.index(*c);
                self.burnable[i] && !self.cleared[i] && self.state[i] == CellFire::Unburnt
            })
            .map(|c| (c.row, c.col))
            .collect();
        anyhow::ensure!(
            !cells.is_empty(),
            "no unburnt burnable fuel within {radius_m} m of ({:.0}, {:.0})",
            at[0],
            at[1]
        );
        let n = cells.len();
        self.flush()?;
        // Shown burning at once; the core lights them at its next step.
        for &(r, c) in &cells {
            let i = r * self.grid.cols + c;
            self.state[i] = CellFire::Burning;
            self.ignited_at[i] = self.time_s as i32;
        }
        self.core
            .set_boundary_conditions(BoundaryConditions {
                time: self.time_s,
                ignitions: Some(Ignitions::Points(cells)),
                ..self.weather_bc()
            })
            .context("scheduling ignition")?;
        Ok(n)
    }

    /// Clear fuel along a line. Returns the number of burnable cells cut.
    pub fn fire_line(&mut self, from: Pos, to: Pos, width_m: f32) -> usize {
        let cells = self.cells_along(from, to, width_m);
        self.clear_cells(&cells, false)
    }

    /// All cells whose centre lies inside `[x0, y0, x1, y1]`.
    pub fn cells_in_rect(&self, b: [f32; 4]) -> Vec<Cell> {
        let mut out = Vec::new();
        for row in 0..self.grid.rows {
            for col in 0..self.grid.cols {
                let c = Cell { row, col };
                let q = self.centre(c);
                if q[0] >= b[0] && q[0] < b[2] && q[1] >= b[1] && q[1] < b[3] {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Put the fire out over a disc (EXCON power). Returns burning cells put out.
    pub fn extinguish(&mut self, at: Pos, radius_m: f32) -> usize {
        let cells = self.cells_in_radius(at, radius_m);
        self.extinguish_cells(&cells)
    }

    /// Put the fire out over a rectangle, e.g. a whole evacuation zone.
    pub fn extinguish_rect(&mut self, bounds: [f32; 4]) -> usize {
        let cells = self.cells_in_rect(bounds);
        self.extinguish_cells(&cells)
    }

    fn extinguish_cells(&mut self, cells: &[Cell]) -> usize {
        let mut out = 0;
        for c in cells {
            let i = self.grid.index(*c);
            if self.state[i] == CellFire::Burning {
                self.extinguished[i] = true;
                self.state[i] = CellFire::Burnt;
                out += 1;
            }
        }
        self.clear_cells(cells, true);
        out
    }

    fn clear_cells(&mut self, cells: &[Cell], include_burning: bool) -> usize {
        let (rows, cols) = (self.grid.rows, self.grid.cols);
        let g = self
            .pending_veg
            .get_or_insert_with(|| Grid2::filled(rows, cols, f64::NAN));
        let mut n = 0;
        for c in cells {
            let i = c.row * cols + c.col;
            if !self.burnable[i] || (!include_burning && self.state[i] != CellFire::Unburnt) {
                continue;
            }
            g.as_mut_slice()[i] = self.cleared_fuel_code as f64;
            if !self.cleared[i] {
                self.cleared[i] = true;
                n += 1;
            }
        }
        n
    }

    /// Wet the fuel over a disc. Returns the number of cells wetted.
    pub fn water_drop(&mut self, at: Pos, radius_m: f32, litres_per_m2: f32) -> usize {
        let cells = self.cells_in_radius(at, radius_m);
        let (rows, cols) = (self.grid.rows, self.grid.cols);
        let g = self
            .pending_moisture
            .get_or_insert_with(|| Grid2::filled(rows, cols, 0.0));
        let add = litres_per_m2 * MOISTURE_POINTS_PER_LITRE;
        let mut n = 0;
        for c in cells {
            let i = c.row * cols + c.col;
            if !self.burnable[i] {
                continue;
            }
            let applied = add.min((MAX_ADDED_MOISTURE_PTS - self.added_moisture[i]).max(0.0));
            g.as_mut_slice()[i] += applied;
            self.added_moisture[i] += applied;
            n += 1;
        }
        n
    }

    pub fn set_weather(&mut self, weather: WeatherSpec) -> Result<()> {
        self.weather = weather;
        let bc = self.weather_bc();
        self.core
            .set_boundary_conditions(bc)
            .context("updating weather")
    }

    fn weather_bc(&self) -> BoundaryConditions {
        BoundaryConditions {
            time: self.time_s,
            wind_dir: Some(FieldInput::Scalar(self.weather.wind_dir_deg)),
            wind_speed: Some(FieldInput::Scalar(self.weather.wind_speed_kmh)),
            moisture: Some(FieldInput::Scalar(self.weather.moisture_pct)),
            ..Default::default()
        }
    }

    fn flush(&mut self) -> Result<()> {
        if self.pending_veg.is_none() && self.pending_moisture.is_none() {
            return Ok(());
        }
        let bc = BoundaryConditions {
            time: self.time_s,
            vegetation_changes: self.pending_veg.take(),
            additional_moisture: self.pending_moisture.take(),
            ..self.weather_bc()
        };
        self.core
            .set_boundary_conditions(bc)
            .context("applying interventions")
    }

    /// Advance simulated time by `seconds`.
    pub fn advance(&mut self, seconds: i64) -> Result<()> {
        if seconds <= 0 {
            return Ok(());
        }
        self.flush()?;
        self.core
            .step_window(seconds)
            .context("stepping fire core")?;
        self.time_s += seconds;
        let factor = (1.0 - MOISTURE_DECAY_PER_MIN).powf(seconds as f32 / 60.0);
        for v in &mut self.added_moisture {
            *v *= factor;
        }
        if self.time_s - self.last_refresh_s >= REFRESH_S {
            self.refresh()?;
        }
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        self.last_refresh_s = self.time_s;
        let fire = self.core.get_fire().context("reading fire state")?;
        let arrival = self
            .core
            .get_arrival_time()
            .context("reading arrival time")?;
        let fli = self
            .core
            .get_fireline_int()
            .context("reading fireline intensity")?;
        let masks: Vec<&[u8]> = fire.iter().map(|g| g.as_slice()).collect();
        let arrivals: Vec<&[i32]> = arrival.iter().map(|g| g.as_slice()).collect();
        let flis: Vec<&[f32]> = fli.iter().map(|g| g.as_slice()).collect();
        let r = masks.len();
        let k = ((self.options.cutoff * r as f32).ceil() as usize).clamp(1, r);
        let now = self.time_s as i32;
        for i in 0..self.state.len() {
            let reached = masks.iter().filter(|m| m[i] != 0).count();
            if reached > 0 {
                let sum: f32 = (0..r)
                    .filter(|&j| masks[j][i] != 0)
                    .map(|j| flis[j][i])
                    .sum();
                self.intensity[i] = sum / reached as f32;
            }
            // A cleared cell has no fuel; the core may still light it from an
            // ignition event scheduled before the clearing. That is not a fire.
            self.probability[i] = if self.cleared[i] && self.ignited_at[i] == i32::MIN {
                0.0
            } else {
                reached as f32 / r as f32
            };
            if self.ignited_at[i] == i32::MIN {
                if reached < k || self.cleared[i] {
                    continue;
                }
                // Quantile arrival: the k-th earliest among the realizations
                // that reached the cell (later ones cannot move it earlier).
                self.scratch.clear();
                self.scratch
                    .extend((0..r).filter(|&j| masks[j][i] != 0).map(|j| arrivals[j][i]));
                let (_, kth, _) = self.scratch.select_nth_unstable(k - 1);
                self.ignited_at[i] = (*kth).min(now);
            }
            self.state[i] = if self.extinguished[i] || now - self.ignited_at[i] >= BURNOUT_S {
                CellFire::Burnt
            } else {
                CellFire::Burning
            };
        }
        Ok(())
    }

    pub fn snapshot(&self) -> FireSnapshot {
        FireSnapshot {
            pending_veg: self.pending_veg.clone(),
            pending_moisture: self.pending_moisture.clone(),
            core: self.core.checkpoint(),
            weather: self.weather,
            time_s: self.time_s,
            state: self.state.clone(),
            ignited_at: self.ignited_at.clone(),
            probability: self.probability.clone(),
            intensity: self.intensity.clone(),
            cleared: self.cleared.clone(),
            extinguished: self.extinguished.clone(),
            added_moisture: self.added_moisture.clone(),
        }
    }

    /// Roll back to a snapshot. The RNG is reseeded so the continuation is
    /// deterministic (though not bitwise the original one).
    pub fn restore(&mut self, s: &FireSnapshot, reseed: u64) -> Result<()> {
        self.core.restore(&s.core).context("restoring fire core")?;
        self.core.reseed(reseed);
        self.weather = s.weather;
        self.time_s = s.time_s;
        self.state = s.state.clone();
        self.ignited_at = s.ignited_at.clone();
        self.probability = s.probability.clone();
        self.intensity = s.intensity.clone();
        self.cleared = s.cleared.clone();
        self.extinguished = s.extinguished.clone();
        self.added_moisture = s.added_moisture.clone();
        self.pending_veg = s.pending_veg.clone();
        self.pending_moisture = s.pending_moisture.clone();
        self.last_refresh_s = s.time_s;
        Ok(())
    }
}
