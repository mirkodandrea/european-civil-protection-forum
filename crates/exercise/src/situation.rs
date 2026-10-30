//! EXCON situation view: a deterministic preview of the open turn.
//!
//! The preview forks the exercise (a copy of the world plus the decisions that
//! shape the next advance), then does exactly what a real resolution to the
//! planned end of the turn does: runs the queued EXCON actions
//! ([`crate::queue`]), applies the approved proposals, runs the planned inject
//! and EXCON actions at their instants, and steps the world one second
//! at a time. Because the world copy carries the random streams of the fire
//! ensemble, the result is what the real resolution will produce if nobody
//! does anything else. It never changes the exercise, never sends an inject
//! and never schedules one.
//!
//! The copy is made under the exercise lock (a memory copy); the run itself
//! ([`PreviewJob::run`]) happens off the lock.

use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::hash::{Hash, Hasher};

use anyhow::{bail, Result};
use serde::Serialize;
use sim::impact::{ElementState, ImpactReport};
use sim::{Damage, RoadState};

use crate::actions::ActionStatus;
use crate::{AppliedAction, Exercise, Phase, ProposalStatus, STEP_S};

/// When an element first reached each condition during the previewed advance.
/// Conditions already true when the preview starts have no entry.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timeline {
    pub threatened_s: Option<i64>,
    pub damaged_s: Option<i64>,
    pub destroyed_s: Option<i64>,
    pub cut_s: Option<i64>,
    /// Everybody has left the building.
    pub emptied_s: Option<i64>,
}

/// Fire state at the end of the preview, in the raster format of the views.
#[derive(Debug, Clone, Serialize)]
pub struct FireField {
    pub cols: usize,
    pub rows: usize,
    /// Base64, one `sim::digest::cell_code` byte per cell, row 0 north.
    pub raster: String,
    pub burned_ha: f64,
    pub burning_cells: usize,
}

/// An action that ran during the previewed advance.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewAction {
    pub at_s: i64,
    pub text: String,
    pub inject: Option<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// Fingerprint of the exercise the preview was computed for ([`Exercise::preview_key`]).
    pub key: u64,
    pub from_s: i64,
    pub to_s: i64,
    pub clock_from: String,
    pub clock_to: String,
    /// Impact at the end of the turn, with deltas against the start of the turn.
    pub end: ImpactReport,
    /// Per element id (`b12`, `r:road`) with at least one transition.
    pub timelines: BTreeMap<String, Timeline>,
    pub fire: FireField,
    /// Approved proposals applied and planned actions run by the advance.
    pub applied: Vec<AppliedAction>,
    pub actions: Vec<PreviewAction>,
    pub sim_events: usize,
    /// Wall time of the run, milliseconds.
    pub wall_ms: u128,
    /// Buildings as the maps draw them, at the end (same order as the scenario).
    pub buildings: Vec<sim::digest::BuildingView>,
    /// Road conditions at the end (also used to evaluate routes at that time).
    pub end_roads: BTreeMap<String, RoadState>,
}

impl Preview {
    pub fn timeline(&self, id: &str) -> Option<&Timeline> {
        self.timelines.get(id)
    }
}

/// Everything the off-lock run needs.
pub struct PreviewJob {
    fork: Exercise,
    key: u64,
    start: ImpactReport,
}

#[derive(Default)]
struct Tracker {
    damage: Vec<Damage>,
    threatened: Vec<bool>,
    inside: Vec<u32>,
    cut: BTreeMap<String, bool>,
    out: BTreeMap<String, Timeline>,
}

impl Tracker {
    fn new(w: &sim::World) -> Tracker {
        Tracker {
            damage: w.d.buildings.iter().map(|b| b.damage).collect(),
            threatened: w.d.buildings.iter().map(|b| b.threatened).collect(),
            inside: w.d.buildings.iter().map(|b| b.people_inside).collect(),
            cut: w
                .d
                .roads
                .iter()
                .map(|(k, r)| (k.clone(), r.cut_by_fire))
                .collect(),
            out: BTreeMap::new(),
        }
    }

    fn observe(&mut self, w: &sim::World) {
        let t = w.time_s();
        for (i, b) in w.d.buildings.iter().enumerate() {
            let id = || format!("b{}", b.id);
            if b.damage != self.damage[i] {
                let tl = self.out.entry(id()).or_default();
                if b.damage == Damage::Destroyed && tl.destroyed_s.is_none() {
                    tl.destroyed_s = Some(t);
                    // Destroyed straight from intact still passed through damaged.
                    tl.damaged_s.get_or_insert(t);
                } else if b.damage == Damage::Damaged && tl.damaged_s.is_none() {
                    tl.damaged_s = Some(t);
                }
                self.damage[i] = b.damage;
            }
            if b.threatened && !self.threatened[i] {
                let tl = self.out.entry(id()).or_default();
                tl.threatened_s.get_or_insert(t);
            }
            self.threatened[i] = b.threatened;
            if b.people_inside == 0 && self.inside[i] > 0 {
                self.out.entry(id()).or_default().emptied_s.get_or_insert(t);
            }
            self.inside[i] = b.people_inside;
        }
        for (k, r) in &w.d.roads {
            if r.cut_by_fire && !self.cut.get(k).copied().unwrap_or(false) {
                self.out
                    .entry(format!("r:{k}"))
                    .or_default()
                    .cut_s
                    .get_or_insert(t);
            }
            self.cut.insert(k.clone(), r.cut_by_fire);
        }
    }
}

impl PreviewJob {
    /// Run the previewed advance. Heavy: call it off the exercise lock.
    pub fn run(mut self) -> Result<Preview> {
        let started = web_time::Instant::now();
        let ex = &mut self.fork;
        let from_s = ex.world.time_s();
        let already: BTreeMap<String, bool> = ex
            .action_runs
            .iter()
            .map(|(k, r)| (k.clone(), r.executed_s.is_some()))
            .collect();
        // The advance a click on "close the turn" would start.
        ex.begin_resolution(None)?;
        let to_s = ex.resolving.as_ref().map(|r| r.to_s).unwrap_or(from_s);
        let mut tracker = Tracker::new(&ex.world);
        while !ex.step_resolution(STEP_S)? {
            tracker.observe(&ex.world);
        }
        tracker.observe(&ex.world);
        let res = ex.resolving.take();
        let (applied, sim_events) = res.map(|r| (r.applied, r.events.len())).unwrap_or_default();
        let start_clock = ex.world.scn.start_clock_s;
        let mut actions: Vec<PreviewAction> = ex
            .action_runs
            .iter()
            .filter(|(k, r)| r.executed_s.is_some() && !already.get(*k).copied().unwrap_or(false))
            .map(|(_, r)| PreviewAction {
                at_s: r.executed_s.unwrap_or(from_s),
                text: if r.queued.is_some() {
                    format!("EXCON: {}", r.title)
                } else {
                    format!("{}: {}", r.title, Exercise::describe_run(&r.command, r.reveal.as_ref()))
                },
                inject: r.inject.clone(),
                ok: r.status == ActionStatus::Done,
            })
            .collect();
        actions.sort_by_key(|a| a.at_s);
        let w = &ex.world;
        let end = w.impact().with_baseline(&self.start);
        Ok(Preview {
            key: self.key,
            from_s,
            to_s,
            clock_from: scenario::format_clock(start_clock + from_s),
            clock_to: scenario::format_clock(start_clock + to_s),
            end,
            timelines: tracker.out,
            fire: FireField {
                cols: w.scn.grid.cols,
                rows: w.scn.grid.rows,
                raster: w.fire_raster_b64(),
                burned_ha: w.fire.burned_area_ha(),
                burning_cells: w.fire.burning_cells(),
            },
            applied,
            actions,
            sim_events,
            wall_ms: started.elapsed().as_millis(),
            buildings: w.building_views(),
            end_roads: w.d.roads.clone(),
        })
    }
}

impl Exercise {
    /// Fingerprint of everything that shapes the next advance. A preview is
    /// current while this value is unchanged.
    pub fn preview_key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        (
            self.turn,
            self.phase as u8,
            self.world.time_s(),
            self.planned_end_s(),
            self.turn_minutes,
            self.restore_epoch,
        )
            .hash(&mut h);
        for p in self
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved && !p.narrative)
        {
            (p.id, serde_json::to_string(&p.commands).unwrap_or_default()).hash(&mut h);
        }
        for (k, r) in &self.action_runs {
            (k, r.status as u8, r.run_at_s, r.executed_s, r.command.to_string()).hash(&mut h);
        }
        for (k, v) in self
            .disabled_actions
            .iter()
            .chain(self.skipped_injects.iter())
        {
            (k, v).hash(&mut h);
        }
        for (k, v) in &self.inject_sent_to {
            (k, v).hash(&mut h);
        }
        for t in &self.world.scn.msel {
            (&t.id, t.at_s, t.actions.len()).hash(&mut h);
        }
        let w = &self.world;
        format!("{:?}|{:?}", w.stats(), w.fire.weather()).hash(&mut h);
        (
            w.d.allocations.len(),
            w.d.tasks.len(),
            w.d.resource_events.len(),
            w.d.warned.len(),
            w.d.shelters.len(),
            w.d.roads.values().filter(|r| r.closed).count(),
            w.d.pools
                .values()
                .flat_map(|p| p.values())
                .map(|p| p.used)
                .sum::<u32>(),
        )
            .hash(&mut h);
        // Within 2^52, so the key survives a round trip through JSON numbers.
        h.finish() & ((1 << 52) - 1)
    }

    /// Capture a preview job for the open turn. Fails outside the preparation phase.
    pub fn preview_job(&self) -> Result<PreviewJob> {
        if self.phase != Phase::Orders {
            bail!("a preview needs an open turn in preparation");
        }
        let mut fork = Exercise::new(self.world.clone());
        fork.turn = self.turn;
        fork.phase = self.phase;
        fork.turn_minutes = self.turn_minutes;
        fork.turn_start_s = self.turn_start_s;
        fork.exercise_phase = self.exercise_phase;
        fork.resource_limits = self.resource_limits.clone();
        fork.clock_control = self.clock_control.clone();
        fork.scheduled_injects = self.scheduled_injects.clone();
        fork.action_runs = self.action_runs.clone();
        fork.skipped_injects = self.skipped_injects.clone();
        fork.disabled_actions = self.disabled_actions.clone();
        fork.inject_sent_to = self.inject_sent_to.clone();
        fork.delivery_msel = self.delivery_msel.clone();
        fork.sent_injects = self.sent_injects.clone();
        fork.proposals = self.proposals.clone();
        fork.players = self.players.clone();
        fork.next_id = self.next_id;
        Ok(PreviewJob {
            fork,
            key: self.preview_key(),
            start: self.turn_start_impact(),
        })
    }

    /// Impact picture at the start of the open turn (the rollback snapshot),
    /// or the present one when there is no snapshot.
    pub fn turn_start_impact(&self) -> ImpactReport {
        self.snapshots
            .iter()
            .rev()
            .find(|(t, _)| *t == self.turn)
            .map(|(_, s)| s.impact(&self.world.scn))
            .unwrap_or_else(|| self.world.impact())
    }

    /// The present impact picture with deltas against the start of the turn.
    pub fn impact_report(&self) -> ImpactReport {
        self.world.impact().with_baseline(&self.turn_start_impact())
    }
}

/// Worst state of an element in a report (`Normal` when unlisted).
pub fn state_in(report: &ImpactReport, id: &str) -> ElementState {
    report
        .row(id)
        .map(|r| r.state)
        .unwrap_or(ElementState::Normal)
}
