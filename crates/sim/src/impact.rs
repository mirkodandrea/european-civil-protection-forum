//! Impact picture: what the fire has done to buildings, people, roads and
//! shelters, as a pure read of the world. It never changes anything; the EXCON
//! "Situation" view shows it now and, from a preview run, at the end of the turn.
//!
//! Rows list the elements that matter (facilities, shelters, anything touched
//! by the fire, anything with people on the move); plain houses are only
//! counted, per sector, until the fire reaches them.

use std::collections::BTreeMap;

use scenario::{BuildingKind, Pos, Scenario};
use serde::Serialize;

use crate::{Damage, Dyn, World, WorldSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Building,
    Road,
}

/// Worst condition of an element, in increasing severity (roads: `Closed` < `Cut`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementState {
    Normal,
    Threatened,
    Closed,
    Cut,
    Damaged,
    Destroyed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactRow {
    /// Stable id: `b<building id>` or `r:<road id>`.
    pub id: String,
    pub kind: ElementKind,
    pub name: String,
    /// Building kind (snake case) or road class.
    pub category: String,
    pub sector: String,
    pub pos: Pos,
    pub state: ElementState,
    pub people_inside: u32,
    /// People who are leaving (not out yet).
    pub evacuating: u32,
    /// People who have left the building.
    pub arrived: u32,
    pub at_risk: u32,
    pub needs_transport: bool,
    pub shelter_capacity: u32,
    /// People currently sheltered here (open shelters only).
    pub sheltered: u32,
}

/// Additive counters (signed so the same type carries deltas).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub buildings: i64,
    pub threatened: i64,
    pub damaged: i64,
    pub destroyed: i64,
    pub people_present: i64,
    pub people_evacuating: i64,
    pub people_arrived: i64,
    pub people_at_risk: i64,
    pub roads_total: i64,
    pub roads_cut: i64,
    pub shelter_capacity: i64,
    pub sheltered: i64,
}

impl Counts {
    pub fn minus(&self, o: &Counts) -> Counts {
        Counts {
            buildings: self.buildings - o.buildings,
            threatened: self.threatened - o.threatened,
            damaged: self.damaged - o.damaged,
            destroyed: self.destroyed - o.destroyed,
            people_present: self.people_present - o.people_present,
            people_evacuating: self.people_evacuating - o.people_evacuating,
            people_arrived: self.people_arrived - o.people_arrived,
            people_at_risk: self.people_at_risk - o.people_at_risk,
            roads_total: self.roads_total - o.roads_total,
            roads_cut: self.roads_cut - o.roads_cut,
            shelter_capacity: self.shelter_capacity - o.shelter_capacity,
            sheltered: self.sheltered - o.sheltered,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaceImpact {
    /// Sector id.
    pub id: String,
    pub name: String,
    pub counts: Counts,
    /// Change since the baseline (the start of the turn); empty without one.
    pub delta: Counts,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub t_s: i64,
    pub clock: String,
    pub rows: Vec<ImpactRow>,
    pub totals: Counts,
    pub delta: Counts,
    /// Sectors (alphabetical by id) followed by nothing else: roads are only in `totals`.
    pub places: Vec<PlaceImpact>,
}

fn state_of(d: Damage, threatened: bool) -> ElementState {
    match d {
        Damage::Destroyed => ElementState::Destroyed,
        Damage::Damaged => ElementState::Damaged,
        Damage::Intact if threatened => ElementState::Threatened,
        Damage::Intact => ElementState::Normal,
    }
}

/// Build the report from a scenario and its dynamic state.
pub fn report(scn: &Scenario, d: &Dyn, t_s: i64) -> ImpactReport {
    let mut rows = Vec::new();
    let mut totals = Counts::default();
    let mut sectors: BTreeMap<String, Counts> = BTreeMap::new();
    for (b, st) in scn.buildings.iter().zip(&d.buildings) {
        let state = state_of(st.damage, st.threatened);
        let evacuating = if st.evac_done_at.is_some() {
            st.evac_pending
        } else {
            0
        };
        let sheltered = d.shelters.get(&b.id).copied().unwrap_or(0);
        for c in [&mut totals, sectors.entry(b.sector.clone()).or_default()] {
            c.buildings += 1;
            c.threatened += (state == ElementState::Threatened) as i64;
            c.damaged += (state == ElementState::Damaged) as i64;
            c.destroyed += (state == ElementState::Destroyed) as i64;
            c.people_present += st.people_inside as i64 - evacuating as i64;
            c.people_evacuating += evacuating as i64;
            c.people_arrived += st.evacuated as i64;
            c.people_at_risk += st.at_risk as i64;
            if d.shelters.contains_key(&b.id) {
                c.shelter_capacity += b.shelter_capacity as i64;
                c.sheltered += sheltered as i64;
            }
        }
        let listed = b.kind != BuildingKind::House
            || b.name.is_some()
            || b.shelter_capacity > 0
            || state != ElementState::Normal
            || evacuating > 0
            || st.evacuated > 0;
        if listed {
            rows.push(ImpactRow {
                id: format!("b{}", b.id),
                kind: ElementKind::Building,
                name: b.name.clone().unwrap_or_else(|| format!("#{}", b.id)),
                category: b.category().to_string(),
                sector: b.sector.clone(),
                pos: b.pos,
                state,
                people_inside: st.people_inside - evacuating.min(st.people_inside),
                evacuating,
                arrived: st.evacuated,
                at_risk: st.at_risk,
                needs_transport: b.kind.needs_transport(),
                shelter_capacity: b.shelter_capacity,
                sheltered,
            });
        }
    }
    for r in &scn.roads {
        let Some(st) = d.roads.get(&r.id) else {
            continue;
        };
        totals.roads_total += 1;
        let state = if st.cut_by_fire {
            ElementState::Cut
        } else if st.closed {
            ElementState::Closed
        } else {
            ElementState::Normal
        };
        totals.roads_cut += (state == ElementState::Cut) as i64;
        if state != ElementState::Normal {
            let pos = r
                .points
                .get(r.points.len() / 2)
                .copied()
                .unwrap_or([0.0, 0.0]);
            rows.push(ImpactRow {
                id: format!("r:{}", r.id),
                kind: ElementKind::Road,
                name: r.name.clone(),
                category: r.class.clone(),
                sector: String::new(),
                pos,
                state,
                people_inside: 0,
                evacuating: 0,
                arrived: 0,
                at_risk: 0,
                needs_transport: false,
                shelter_capacity: 0,
                sheltered: 0,
            });
        }
    }
    let mut places: Vec<PlaceImpact> = Vec::new();
    for s in &scn.sectors {
        places.push(PlaceImpact {
            id: s.id.clone(),
            name: s.name.clone(),
            counts: sectors.remove(&s.id).unwrap_or_default(),
            delta: Counts::default(),
        });
    }
    for (id, counts) in sectors {
        places.push(PlaceImpact {
            name: id.clone(),
            id,
            counts,
            delta: Counts::default(),
        });
    }
    ImpactReport {
        t_s,
        clock: scenario::format_clock(scn.start_clock_s + t_s),
        rows,
        totals,
        delta: Counts::default(),
        places,
    }
}

impl ImpactReport {
    /// Fill the deltas against an earlier report (normally the turn start).
    pub fn with_baseline(mut self, base: &ImpactReport) -> ImpactReport {
        self.delta = self.totals.minus(&base.totals);
        let before: BTreeMap<&str, &Counts> = base
            .places
            .iter()
            .map(|p| (p.id.as_str(), &p.counts))
            .collect();
        for p in &mut self.places {
            p.delta = match before.get(p.id.as_str()) {
                Some(b) => p.counts.minus(b),
                None => p.counts.clone(),
            };
        }
        self
    }

    pub fn row(&self, id: &str) -> Option<&ImpactRow> {
        self.rows.iter().find(|r| r.id == id)
    }
}

impl World {
    /// The present impact picture.
    pub fn impact(&self) -> ImpactReport {
        report(&self.scn, &self.d, self.time_s())
    }
}

impl WorldSnapshot {
    /// The impact picture the world had when the snapshot was taken.
    pub fn impact(&self, scn: &Scenario) -> ImpactReport {
        report(scn, &self.d, self.time_s)
    }
}
