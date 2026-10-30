//! Exercise timers: real (wall-clock) time since the EXCON started the
//! exercise and since the open turn began, and where the simulated clock
//! stands in the open turn and in the scenario's planned span.
//!
//! The wall-clock instants are recorded by the exercise itself (never by a
//! browser). The elapsed time counts the whole session, pauses of the
//! within-turn clock included: it measures how long the participants have
//! been at the table. It stops when the exercise ends. A rollback keeps the
//! start (the session went on) and restarts the turn timer.
//!
//! Replays (server restart, standalone reload) set [`Exercise::replay_wall_ms`]
//! to the instant each journaled request was originally made, so the instants
//! come back as they were.
use crate::{Exercise, Phase};
use serde::Serialize;

/// The timers as the EXCON console shows them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Wall-clock instant (ms since the epoch) the exercise was started.
    pub started_wall_ms: Option<u128>,
    /// Wall-clock instant the exercise was ended (elapsed time stops there).
    pub ended_wall_ms: Option<u128>,
    /// Wall-clock instant the open turn began.
    pub turn_wall_ms: Option<u128>,
    /// Real time since the start (up to the end), at `now_wall_ms`.
    pub elapsed_ms: u128,
    /// Real time since the open turn began, at `now_wall_ms`.
    pub turn_elapsed_ms: u128,
    pub now_wall_ms: u128,
    /// Whether the elapsed timers are still counting.
    pub counting: bool,
    pub turn: u32,
    /// Turns the scenario plans (last planned inject's turn); `None` without injects.
    pub planned_turns: Option<u32>,
    /// Simulated seconds since the open turn began.
    pub turn_s: i64,
    /// Planned length of the open turn, simulated seconds.
    pub turn_len_s: i64,
    /// Simulated seconds since the start of the scenario.
    pub scenario_s: i64,
    /// Planned scenario span, simulated seconds (end of the last planned turn).
    pub scenario_span_s: Option<i64>,
    pub start_clock: String,
    pub end_clock: Option<String>,
    pub clock: String,
}

impl Exercise {
    /// Now on the wall clock, or the original instant of the request being replayed.
    pub(crate) fn now_wall_ms(&self) -> u128 {
        self.replay_wall_ms.unwrap_or_else(crate::wall_ms)
    }

    /// Simulated span the scenario plans, in seconds from t = 0: up to the end
    /// of the turn holding the last planned inject. `None` without injects.
    pub fn planned_span_s(&self) -> Option<(u32, i64)> {
        let turn_s = self.world.scn.turn_minutes.max(1) as i64 * 60;
        let last = self
            .world
            .scn
            .msel
            .iter()
            .map(|t| self.planned_due_s(&t.id, t.turn, t.at_s).max(0))
            .max()?;
        let turns = (last / turn_s + 1) as u32;
        Some((turns, turns as i64 * turn_s))
    }

    /// The timers at `now_wall_ms` (see the module docs).
    pub fn progress_at(&self, now_wall_ms: u128) -> Progress {
        let stop = self.ended_wall_ms.unwrap_or(now_wall_ms);
        let since = |t: Option<u128>| t.map_or(0, |t| stop.saturating_sub(t));
        let planned = self.planned_span_s();
        let start = self.world.scn.start_clock_s;
        let now_s = self.world.time_s();
        Progress {
            started_wall_ms: self.started_wall_ms,
            ended_wall_ms: self.ended_wall_ms,
            turn_wall_ms: self.turn_wall_ms,
            elapsed_ms: since(self.started_wall_ms),
            turn_elapsed_ms: if self.phase == Phase::Debrief { 0 } else { since(self.turn_wall_ms) },
            now_wall_ms,
            counting: self.started_wall_ms.is_some() && self.ended_wall_ms.is_none(),
            turn: self.turn,
            planned_turns: planned.map(|p| p.0),
            turn_s: if self.turn == 0 { 0 } else { (now_s - self.turn_start_s).max(0) },
            turn_len_s: self.turn_minutes as i64 * 60,
            scenario_s: now_s,
            scenario_span_s: planned.map(|p| p.1),
            start_clock: scenario::format_clock(start),
            end_clock: planned.map(|p| scenario::format_clock(start + p.1)),
            clock: self.world.clock(),
        }
    }

    /// The timers now.
    pub fn progress(&self) -> Progress {
        self.progress_at(crate::wall_ms())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn exercise() -> Exercise {
        let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
        Exercise::new(sim::World::new(scn, 1).unwrap())
    }

    #[test]
    fn nothing_counts_before_the_start() {
        let ex = exercise();
        let p = ex.progress_at(5_000);
        assert_eq!((p.started_wall_ms, p.elapsed_ms, p.turn_elapsed_ms, p.counting), (None, 0, 0, false));
        assert_eq!((p.turn, p.turn_s), (0, 0));
    }

    #[test]
    fn start_is_recorded_once_and_survives_rollback_and_end_freezes() {
        let mut ex = exercise();
        ex.replay_wall_ms = Some(1_000);
        ex.start().unwrap();
        assert!(ex.start().is_err());
        ex.replay_wall_ms = Some(61_000);
        ex.resolve_turn(Some(10)).unwrap();
        assert_eq!(ex.turn, 2);
        let p = ex.progress_at(91_000);
        assert_eq!(p.started_wall_ms, Some(1_000));
        assert_eq!((p.elapsed_ms, p.turn_elapsed_ms), (90_000, 30_000));
        // A rollback restarts the turn timer, never the exercise's.
        ex.replay_wall_ms = Some(100_000);
        ex.rollback(1).unwrap();
        let p = ex.progress_at(110_000);
        assert_eq!((p.started_wall_ms, p.elapsed_ms, p.turn_elapsed_ms), (Some(1_000), 109_000, 10_000));
        assert_eq!(p.turn_s, 0);
        ex.replay_wall_ms = Some(200_000);
        ex.end().unwrap();
        let p = ex.progress_at(900_000);
        assert_eq!((p.elapsed_ms, p.counting, p.turn_elapsed_ms), (199_000, false, 0));
    }

    #[test]
    fn turn_and_scenario_progress_follow_the_simulated_clock() {
        let mut ex = exercise();
        ex.start().unwrap();
        let tm = ex.world.scn.turn_minutes as i64;
        let (turns, span) = ex.planned_span_s().expect("the synthetic MSEL plans turns");
        assert_eq!(span, turns as i64 * tm * 60);
        let last = ex.world.scn.msel.iter().map(|t| t.planned_s(tm as u32)).max().unwrap();
        assert!(last < span && last >= span - tm * 60, "the last inject falls in the last turn");
        // A shorter resolution closes turn 1 at 12 simulated minutes.
        ex.resolve_turn(Some(12)).unwrap();
        // An intermediate advance of 5 minutes keeps turn 2 open.
        ex.begin_resolution_until(ex.world.time_s() + 5 * 60, true).unwrap();
        while !ex.step_resolution(crate::STEP_S).unwrap() {}
        ex.finish_resolution().unwrap();
        let p = ex.progress();
        assert_eq!((p.turn, p.turn_s, p.turn_len_s, p.scenario_s), (2, 300, tm * 60, 17 * 60));
        assert_eq!(p.planned_turns, Some(turns));
        assert_eq!(p.start_clock, scenario::format_clock(ex.world.scn.start_clock_s));
        assert_eq!(p.end_clock, Some(scenario::format_clock(ex.world.scn.start_clock_s + span)));
    }

    #[test]
    fn a_scenario_without_injects_has_no_planned_total() {
        let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        scn.msel.clear();
        let ex = Exercise::new(sim::World::new(Arc::new(scn), 1).unwrap());
        let p = ex.progress_at(0);
        assert_eq!((p.planned_turns, p.scenario_span_s, p.end_clock), (None, None, None));
    }
}
