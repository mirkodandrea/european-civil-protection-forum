//! The contract between the exercise model and any viewer of it.
//!
//! The model side (`server::view`) turns the exercise into a [`ViewState`];
//! a viewer (the Bevy big screen, and in future an HTML canvas) consumes
//! nothing else, plus the static [`scenario::Scenario`]. This crate depends on
//! neither the simulator nor the exercise, so a viewer cannot reach into the
//! model, cannot change it, and can run in another process or machine: the
//! state is plain serde data, carried in-process, over HTTP, or in a browser.
//!
//! Positions are metres in the scenario world frame. Moving things carry a
//! [`Track`] of timestamped waypoints, so a viewer can interpolate smoothly
//! between updates instead of snapping to the last one received.

use std::sync::Arc;

use scenario::{Pos, Scenario, WeatherSpec};
use serde::{Deserialize, Serialize};

mod packed;
pub mod wire;

/// What a viewer asks for: only changes after what it already has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Have {
    /// `ViewState::generation` of the last state received (0 = none yet).
    pub generation: u64,
    /// `Cue::seq` of the last camera cue received.
    pub cue: u64,
}

/// A source of views: the model in this process, or a remote server.
///
/// `poll` must never block a render frame; return `None` when nothing new is
/// ready (including "the model is busy right now").
pub trait ViewSource: Send + 'static {
    /// The static world; fetched once at start.
    fn scenario(&mut self) -> Result<Arc<Scenario>, String>;
    /// The newest state, if it differs from `have`.
    fn poll(&mut self, have: Have) -> Option<ViewState>;
}

/// Something worth looking at, derived from the exercise log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    /// Increasing identifier (the exercise log sequence number).
    pub seq: u64,
    pub pos: Pos,
    /// Size of what to frame, metres.
    pub radius_m: f32,
    /// How long to hold the shot, seconds.
    pub hold_s: f32,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    EnRoute,
    Working,
    Refilling,
    Withdrawing,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

/// A piecewise-linear path in simulated time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// `(simulated second, position)`, ascending in time. Before the first
    /// waypoint the thing is at the first, after the last at the last.
    pub points: Vec<(f32, Pos)>,
}

impl Track {
    pub fn position(&self, t: f32) -> Pos {
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else {
            return [0.0, 0.0];
        };
        if t <= first.0 {
            return first.1;
        }
        if t >= last.0 {
            return last.1;
        }
        let i = self.points.partition_point(|p| p.0 <= t);
        let (t0, a) = self.points[i - 1];
        let (t1, b) = self.points[i];
        let f = ((t - t0) / (t1 - t0).max(0.001)).clamp(0.0, 1.0);
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
    }
}

/// An approved role deployment: where it is, where it goes, what it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: u64,
    pub resource: String,
    pub status: TaskStatus,
    /// Units currently active (not yet returned or lost).
    pub qty: u32,
    pub origin: Pos,
    pub target: Pos,
    pub speed_mps: f32,
    /// Simulated second at which the unit is expected to arrive.
    pub arrival_s: i64,
    pub track: Track,
    /// Road path to draw while travelling; `None` when there is no route.
    pub route: Option<Vec<Pos>>,
}

impl Deployment {
    pub fn position(&self, t: i64) -> Pos {
        self.track.position(t as f32)
    }
}

/// A vehicle on the road network (aggregate traffic), at the state time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vehicle {
    pub pos: Pos,
    pub people: u32,
    pub at_risk: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildingDraw {
    /// 0 intact, 1 damaged, 2 destroyed
    pub damage: u8,
    pub threatened: bool,
    pub evacuating: bool,
    pub empty_evacuated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadSegment {
    /// Index into `Scenario::roads`.
    pub road: usize,
    pub from: Pos,
    pub to: Pos,
    pub closed: bool,
    pub cut_by_fire: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FireLine {
    pub from: Pos,
    pub to: Pos,
    pub width_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Drop {
    pub at: Pos,
    pub radius_m: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub burned_ha: f64,
    pub burning_cells: usize,
    pub damaged: usize,
    pub destroyed: usize,
    pub threatened: usize,
    pub evacuated_people: u32,
    pub people_at_risk: u32,
    pub sheltered: u32,
    pub line_km: f32,
    pub drops: usize,
}

/// Ensemble size and the probability cutoff that defines the "actual" fire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ensemble {
    pub realizations: usize,
    pub cutoff: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnWindow {
    pub turn: u32,
    pub from_s: i64,
    pub to_s: i64,
}

/// Cell codes of [`ViewState::cells`].
pub mod cell {
    pub const UNBURNT: u8 = 0;
    pub const BURNING: u8 = 1;
    pub const BURNT: u8 = 2;
    /// Unburnt, fuel removed (fire line or bulldozed).
    pub const CLEARED: u8 = 3;
    /// Unburnt, wetted.
    pub const WET: u8 = 4;
    /// Unburnt, reached by some but not enough ensemble runs (the halo).
    pub const HALO: u8 = 5;
}

/// Everything a viewer draws, at one moment of the exercise.
///
/// Grid arrays are row-major like `Scenario::grid`, and travel base64-packed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    /// Bumped by the model whenever anything changes.
    pub generation: u64,
    pub turn: u32,
    pub phase: String,
    pub time_s: i64,
    pub clock: String,
    pub weather: Option<WeatherSpec>,
    pub ensemble: Option<Ensemble>,
    pub stats: Option<Stats>,
    /// Final per-cell code, see [`cell`].
    #[serde(with = "packed::bytes")]
    pub cells: Vec<u8>,
    /// Quantile arrival time per cell, seconds (negative = never).
    #[serde(with = "packed::i32s")]
    pub arrival: Vec<i32>,
    /// Ensemble-mean fireline intensity, kW/m.
    #[serde(with = "packed::f32s")]
    pub intensity: Vec<f32>,
    /// Fraction of runs that reached the cell.
    #[serde(with = "packed::f32s")]
    pub probability: Vec<f32>,
    pub buildings: Vec<BuildingDraw>,
    /// `(closed, cut_by_fire)` per `Scenario::roads` entry.
    pub roads: Vec<(bool, bool)>,
    pub road_segments: Vec<RoadSegment>,
    pub lines: Vec<FireLine>,
    pub drops: Vec<Drop>,
    pub traffic: Vec<Vehicle>,
    pub deployments: Vec<Deployment>,
    /// Zones with an issued warning.
    pub warned: Vec<String>,
    pub last_turn: Option<TurnWindow>,
    /// While a turn is being simulated.
    pub resolving: Option<TurnWindow>,
    /// EXCON free-running clock between turns (distinct from `resolving`).
    pub clock_running: bool,
    /// Last lines of the exercise feed, oldest first.
    pub feed: Vec<String>,
    /// New camera cues since `Have::cue`, oldest first.
    pub cues: Vec<Cue>,
}

/// Byram flame length (m) from fireline intensity (kW/m): `L = 0.0775 · I^0.46`.
///
/// A presentation copy of the model's `fire::flame_length_m`, so a viewer needs
/// no model crate; the server tests assert the two agree.
pub fn flame_length_m(fli_kw_m: f32) -> f32 {
    if fli_kw_m <= 0.0 {
        0.0
    } else {
        0.0775 * fli_kw_m.powf(0.46)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_interpolates_and_clamps() {
        let t = Track {
            points: vec![(10.0, [0.0, 0.0]), (20.0, [100.0, 0.0]), (30.0, [100.0, 50.0])],
        };
        assert_eq!(t.position(0.0), [0.0, 0.0]);
        assert_eq!(t.position(15.0), [50.0, 0.0]);
        assert_eq!(t.position(25.0), [100.0, 25.0]);
        assert_eq!(t.position(99.0), [100.0, 50.0]);
    }

    #[test]
    fn state_round_trips_through_json() {
        let s = ViewState {
            generation: 7,
            cells: vec![0, 1, 2, 5],
            arrival: vec![-1, 0, 60, 120],
            intensity: vec![0.0, 1500.5, 20.0, 0.25],
            probability: vec![0.0, 1.0, 0.5, 0.03125],
            cues: vec![Cue {
                seq: 3,
                pos: [1.0, 2.0],
                radius_m: 100.0,
                hold_s: 5.0,
                label: "x".into(),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ViewState>(&json).unwrap(), s);
    }

    #[test]
    fn corrupt_packed_array_is_an_error_not_a_panic() {
        let bad = r#"{"generation":1,"turn":0,"phase":"","time_s":0,"clock":"","weather":null,
            "ensemble":null,"stats":null,"cells":"!!","arrival":"","intensity":"AAA=","probability":"",
            "buildings":[],"roads":[],"road_segments":[],"lines":[],"drops":[],"traffic":[],
            "deployments":[],"warned":[],"last_turn":null,"resolving":null,"clock_running":false,
            "feed":[],"cues":[]}"#;
        assert!(serde_json::from_str::<ViewState>(bad).is_err());
    }
}
