//! Inject suggestions for critical situations: explainable rules over the
//! present impact picture and the turn preview.
//!
//! Suggestions are drafts for the EXCON to open, edit and send (or plan) by
//! hand. Nothing here sends, schedules or changes anything. Each suggestion
//! has a stable id (`rule:object`), so one the EXCON dismissed, sent or
//! planned is not proposed again.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};
use scenario::{BuildingKind, InjectTemplate, Pos, Scenario};
use serde::Serialize;
use serde_json::json;
use sim::network::Network;
use sim::traffic::Vehicle;
use sim::{Damage, RoadState, TaskStatus, World};

use crate::situation::Preview;
use crate::{Exercise, Phase, MASTER};

/// Minutes before the fire arrives at which the message should go out, on top
/// of the evacuation time.
const MARGIN_MIN: i64 = 5;

#[derive(Debug, Clone, Serialize)]
pub struct LText {
    pub it: String,
    pub en: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectRef {
    /// `building`, `road`, `place`, `sector` or `task`.
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub pos: Option<Pos>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    /// `rule:object`, stable while the situation lasts.
    pub id: String,
    pub rule: &'static str,
    pub severity: Severity,
    /// The message is due now.
    pub now: bool,
    /// Latest sensible sending time when it is not due yet (simulated seconds).
    pub at_s: Option<i64>,
    /// When the fire (or the cut) is expected.
    pub reach_s: Option<i64>,
    pub object: ObjectRef,
    pub recipients: Vec<String>,
    pub sender: String,
    pub title: LText,
    /// The draft of the inject.
    pub text: LText,
    /// Why the rule fired, in plain words.
    pub evidence: LText,
}

/// What the EXCON did with a suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestionMark {
    /// `dismissed`, `sent` or `planned`.
    pub status: String,
    pub at_s: i64,
}

fn lt(it: impl Into<String>, en: impl Into<String>) -> LText {
    LText {
        it: it.into(),
        en: en.into(),
    }
}

/// `HH:MM` of a simulated instant.
fn hhmm(scn: &Scenario, t: i64) -> String {
    let c = scenario::format_clock(scn.start_clock_s + t);
    c.rsplit(' ').next().unwrap_or(&c).to_string()
}

/// Roles that can act on the matter: those allowed any of `kinds`, local ones
/// first; everybody when the scenario defines nobody.
pub fn recipients_for(scn: &Scenario, kinds: &[&str]) -> Vec<String> {
    let able: Vec<_> = scn
        .roles
        .iter()
        .filter(|r| {
            r.allowed_actions
                .iter()
                .any(|a| kinds.contains(&a.as_str()))
        })
        .collect();
    let local: Vec<_> = able.iter().filter(|r| r.level == "local").collect();
    let pick: Vec<String> = if local.is_empty() {
        able.iter().map(|r| r.id.clone()).collect()
    } else {
        local.iter().map(|r| r.id.clone()).collect()
    };
    if pick.is_empty() {
        vec!["all".into()]
    } else {
        pick
    }
}

const EVACUATE: &[&str] = &["EvacuateBuilding", "EvacuateZone", "IssueWarning"];

struct Ctx<'a> {
    w: &'a World,
    now: i64,
    preview: Option<&'a Preview>,
}

impl Ctx<'_> {
    fn scn(&self) -> &Scenario {
        &self.w.scn
    }
    fn tl(&self, id: &str) -> Option<&crate::situation::Timeline> {
        self.preview.and_then(|p| p.timeline(id))
    }
    /// When a road is (or will be) cut by the fire: now if it already is.
    fn cut_s(&self, road: &str) -> Option<i64> {
        if self.w.d.roads.get(road).is_some_and(|r| r.cut_by_fire) {
            return Some(self.now);
        }
        self.tl(&format!("r:{road}")).and_then(|t| t.cut_s)
    }
    fn road_name(&self, road: &str) -> String {
        self.scn()
            .road(road)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| road.into())
    }
    /// Send time for a message about something happening at `reach` that
    /// people need `lead_min` minutes to react to.
    fn timing(&self, reach: i64, lead_min: i64) -> (bool, Option<i64>) {
        let at = reach - (lead_min + MARGIN_MIN) * 60;
        if at <= self.now {
            (true, None)
        } else {
            (false, Some(at))
        }
    }
}

/// All suggestions for the present situation, most urgent first. `preview`
/// is optional: without it only what already happened is considered.
pub fn suggest(
    w: &World,
    preview: Option<&Preview>,
    marks: &BTreeMap<String, SuggestionMark>,
) -> Vec<Suggestion> {
    let cx = Ctx {
        w,
        now: w.time_s(),
        preview,
    };
    let mut out = Vec::new();
    out.extend(building_reached(&cx));
    out.extend(houses_reached(&cx));
    out.extend(place_without_exit(&cx));
    out.extend(evacuees_on_cut_road(&cx));
    out.extend(shelters(&cx));
    out.extend(deployments_cut_off(&cx));
    out.retain(|s| !marks.contains_key(&s.id));
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.now.cmp(&a.now))
            .then(a.at_s.cmp(&b.at_s))
            .then(a.id.cmp(&b.id))
    });
    out
}

/// When the fire first reaches a building (threatened or damaged): now if it already does.
fn reach_of(cx: &Ctx, i: usize) -> Option<i64> {
    let st = &cx.w.d.buildings[i];
    if st.threatened || st.damage != Damage::Intact {
        return Some(cx.now);
    }
    let tl = cx.tl(&format!("b{}", st.id))?;
    [tl.threatened_s, tl.damaged_s, tl.destroyed_s]
        .into_iter()
        .flatten()
        .min()
}

/// When the people inside are out: `None` when not within the preview.
fn out_by(cx: &Ctx, i: usize) -> Option<i64> {
    let st = &cx.w.d.buildings[i];
    if st.people_inside == 0 {
        return Some(cx.now);
    }
    match cx.preview {
        Some(_) => cx.tl(&format!("b{}", st.id)).and_then(|t| t.emptied_s),
        None => st.evac_done_at,
    }
}

fn evac_status(cx: &Ctx, i: usize) -> (LText, bool) {
    let st = &cx.w.d.buildings[i];
    let scn = cx.scn();
    if st.evac_done_at.is_some() {
        match out_by(cx, i) {
            Some(t) => (
                lt(
                    format!("evacuazione in corso, completata verso le {}", hhmm(scn, t)),
                    format!("evacuation under way, complete around {}", hhmm(scn, t)),
                ),
                true,
            ),
            None => (
                lt(
                    "evacuazione in corso ma non completata entro la fine del turno",
                    "evacuation under way but not complete by the end of the turn",
                ),
                true,
            ),
        }
    } else {
        (
            lt("evacuazione non avviata", "evacuation not started"),
            false,
        )
    }
}

/// Rule `building_reached`: a named facility or institution with people still
/// inside that the fire reaches before the evacuation is complete.
fn building_reached(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let mut out = Vec::new();
    for (i, (b, st)) in scn.buildings.iter().zip(&cx.w.d.buildings).enumerate() {
        if st.people_inside == 0 || (b.kind == BuildingKind::House && b.name.is_none()) {
            continue;
        }
        let Some(reach) = reach_of(cx, i) else {
            continue;
        };
        let done = out_by(cx, i);
        if done.is_some_and(|d| d <= reach) {
            continue;
        }
        let name = b.name.clone().unwrap_or_else(|| format!("#{}", b.id));
        let transport = b.kind.needs_transport();
        let (status, _) = evac_status(cx, i);
        let lead = crate::EVAC_LEAD_MIN + if transport { 10 } else { 0 };
        let (now, at_s) = cx.timing(reach, lead);
        let when_it = if reach <= cx.now {
            "è già minacciato dal fuoco".to_string()
        } else {
            format!("raggiunto verso le {}", hhmm(scn, reach))
        };
        let when_en = if reach <= cx.now {
            "is already threatened by the fire".to_string()
        } else {
            format!("reached around {}", hhmm(scn, reach))
        };
        let n = st.people_inside;
        let note_it = if transport {
            " Servono mezzi di trasporto."
        } else {
            ""
        };
        let note_en = if transport {
            " Transport is needed."
        } else {
            ""
        };
        out.push(Suggestion {
            id: format!("building_reached:b{}", b.id),
            rule: "building_reached",
            severity: if transport || reach <= cx.now { Severity::High } else { Severity::Medium },
            now,
            at_s,
            reach_s: Some(reach),
            object: ObjectRef { kind: "building", id: format!("b{}", b.id), name: name.clone(), pos: Some(b.pos) },
            recipients: recipients_for(scn, EVACUATE),
            sender: scn.default_persona_name("it"),
            title: lt(format!("{name}: evacuare prima dell'arrivo del fuoco"), format!("{name}: evacuate before the fire arrives")),
            text: lt(
                format!("{name} ({n} persone all'interno) {when_it}; {}.{note_it} Serve una decisione immediata sull'evacuazione.", status.it),
                format!("{name} ({n} people inside) {when_en}; {}.{note_en} An evacuation decision is needed immediately.", status.en),
            ),
            evidence: lt(
                format!("{name}: {n} persone, {when_it}, {}", status.it),
                format!("{name}: {n} people, {when_en}, {}", status.en),
            ),
        });
    }
    out
}

/// Rule `houses_reached`: plain houses are summed per sector.
fn houses_reached(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let mut per: BTreeMap<&str, (u32, u32, i64)> = BTreeMap::new();
    for (i, (b, st)) in scn.buildings.iter().zip(&cx.w.d.buildings).enumerate() {
        if st.people_inside == 0 || b.kind != BuildingKind::House || b.name.is_some() {
            continue;
        }
        let Some(reach) = reach_of(cx, i) else {
            continue;
        };
        if out_by(cx, i).is_some_and(|d| d <= reach) {
            continue;
        }
        let e = per.entry(b.sector.as_str()).or_insert((0, 0, i64::MAX));
        e.0 += 1;
        e.1 += st.people_inside;
        e.2 = e.2.min(reach);
    }
    per.into_iter()
        .map(|(sector, (houses, people, reach))| {
            let name = scn.sectors.iter().find(|s| s.id == sector).map(|s| s.name.clone()).unwrap_or_else(|| sector.into());
            let pos = scn.sectors.iter().find(|s| s.id == sector).map(|s| s.centre());
            let (now, at_s) = cx.timing(reach, crate::EVAC_LEAD_MIN);
            let (it, en) = if reach <= cx.now {
                ("già minacciate dal fuoco".to_string(), "already threatened by the fire".to_string())
            } else {
                (format!("raggiunte verso le {}", hhmm(scn, reach)), format!("reached around {}", hhmm(scn, reach)))
            };
            Suggestion {
                id: format!("houses_reached:{sector}"),
                rule: "houses_reached",
                severity: if reach <= cx.now { Severity::High } else { Severity::Medium },
                now,
                at_s,
                reach_s: Some(reach),
                object: ObjectRef { kind: "sector", id: sector.into(), name: name.clone(), pos },
                recipients: recipients_for(scn, EVACUATE),
                sender: scn.default_persona_name("it"),
                title: lt(format!("{name}: abitazioni in arrivo del fuoco"), format!("{name}: homes about to be reached by the fire")),
                text: lt(
                    format!("{houses} abitazioni di {name} ({people} persone) {it}. Nessuna evacuazione completata: serve un avviso o un ordine di evacuazione."),
                    format!("{houses} homes in {name} ({people} people) {en}. No evacuation completed: a warning or an evacuation order is needed."),
                ),
                evidence: lt(
                    format!("{name}: {houses} abitazioni, {people} persone, {it}"),
                    format!("{name}: {houses} homes, {people} people, {en}"),
                ),
            }
        })
        .collect()
}

/// Radius around a place without an extent that counts as "the settlement".
const SETTLEMENT_RADIUS_M: f32 = 400.0;
/// Buildings a place needs around it to count as a settlement (not a single facility).
const SETTLEMENT_MIN_BUILDINGS: usize = 8;

/// Settlements: places with a list of buildings, or with at least a few
/// buildings around them. Of places close to each other only the largest is
/// kept. Returns the place and the indices of its buildings.
fn settlements(scn: &Scenario) -> Vec<(&scenario::Place, Vec<usize>)> {
    let mut found: Vec<(&scenario::Place, Vec<usize>)> = scn
        .places
        .iter()
        .filter_map(|p| {
            let members: Vec<usize> = if !p.buildings.is_empty() {
                scn.buildings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| p.buildings.contains(&b.id))
                    .map(|(i, _)| i)
                    .collect()
            } else {
                let r = p.radius_m.unwrap_or(SETTLEMENT_RADIUS_M);
                scn.buildings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| sim::network::distance(b.pos, p.pos) <= r)
                    .map(|(i, _)| i)
                    .collect()
            };
            (!p.buildings.is_empty() || members.len() >= SETTLEMENT_MIN_BUILDINGS)
                .then_some((p, members))
        })
        .collect();
    found.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.name.cmp(&b.0.name)));
    let mut kept: Vec<(&scenario::Place, Vec<usize>)> = Vec::new();
    for c in found {
        if kept
            .iter()
            .all(|k| sim::network::distance(k.0.pos, c.0.pos) > SETTLEMENT_RADIUS_M)
        {
            kept.push(c);
        }
    }
    kept
}

/// Road ends near the map edge: where traffic leaves the territory.
fn exits(scn: &Scenario) -> Vec<Pos> {
    let (w, h) = (scn.grid.width_m(), scn.grid.height_m());
    let m = 150.0;
    let mut out: Vec<Pos> = Vec::new();
    for r in scn.roads.iter().filter(|r| r.drivable) {
        for p in [r.points.first(), r.points.last()].into_iter().flatten() {
            let edge = p[0] < m || p[1] < m || p[0] > w - m || p[1] > h - m;
            if edge && !out.iter().any(|q| sim::network::distance(*q, *p) < 200.0) {
                out.push(*p);
            }
        }
    }
    out
}

fn can_leave(
    net: &Network,
    from: Pos,
    exits: &[Pos],
    states: &BTreeMap<String, RoadState>,
) -> bool {
    exits
        .iter()
        .any(|to| net.route(from, *to, states, true, false).is_some())
}

/// Rule `place_without_exit`: a named place whose people are left with no
/// passable road out of the territory.
fn place_without_exit(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let exits = exits(scn);
    if exits.is_empty() {
        return Vec::new();
    }
    let net = &cx.w.network;
    let mut cuts: Vec<(i64, &String)> = Vec::new();
    if let Some(p) = cx.preview {
        for (id, tl) in &p.timelines {
            if let (Some(road), Some(t)) = (id.strip_prefix("r:"), tl.cut_s) {
                if let Some((k, _)) = p.end_roads.get_key_value(road) {
                    cuts.push((t, k));
                }
            }
        }
        cuts.sort();
    }
    let mut out = Vec::new();
    for (place, members) in settlements(scn) {
        let people: u32 = members
            .iter()
            .map(|&i| cx.w.d.buildings[i].people_inside)
            .sum();
        if people == 0 {
            continue;
        }
        let mut states = cx.w.d.roads.clone();
        let (reach, cause): (i64, Vec<String>) = if !can_leave(net, place.pos, &exits, &states) {
            (cx.now, Vec::new())
        } else {
            let Some(end) = cx.preview.map(|p| &p.end_roads) else {
                continue;
            };
            let mut found = None;
            let mut cause = Vec::new();
            for (t, road) in &cuts {
                if let Some(s) = end.get(*road) {
                    states.insert((*road).clone(), s.clone());
                }
                cause.push(cx.road_name(road));
                if !can_leave(net, place.pos, &exits, &states) {
                    found = Some(*t);
                    break;
                }
            }
            match found {
                Some(t) => (t, cause),
                None => continue,
            }
        };
        let (now, at_s) = cx.timing(reach, crate::EVAC_LEAD_MIN + 10);
        let causes = cause.join(", ");
        let (it, en) = if reach <= cx.now {
            (
                "non ha più nessuna via d'uscita percorribile".to_string(),
                "has no passable exit left".to_string(),
            )
        } else {
            (
                format!(
                    "resterà senza via d'uscita percorribile verso le {} ({causes})",
                    hhmm(scn, reach)
                ),
                format!(
                    "will be left with no passable exit around {} ({causes})",
                    hhmm(scn, reach)
                ),
            )
        };
        out.push(Suggestion {
            id: format!("place_without_exit:{}", place.name),
            rule: "place_without_exit",
            severity: Severity::High,
            now,
            at_s,
            reach_s: Some(reach),
            object: ObjectRef { kind: "place", id: place.name.clone(), name: place.name.clone(), pos: Some(place.pos) },
            recipients: recipients_for(scn, EVACUATE),
            sender: scn.default_persona_name("it"),
            title: lt(format!("{}: isolamento stradale", place.name), format!("{}: cut off by road", place.name)),
            text: lt(
                format!("{} ({people} persone) {it}. Chi è ancora sul posto non può più andarsene in auto: valutare rifugio sul posto o altri mezzi.", place.name),
                format!("{} ({people} people) {en}. Whoever is still there cannot drive out any more: consider sheltering in place or other means.", place.name),
            ),
            evidence: lt(format!("{}: {people} persone, {it}", place.name), format!("{}: {people} people, {en}", place.name)),
        });
    }
    out
}

/// Roads still to be driven by a vehicle.
fn remaining_roads(v: &Vehicle) -> BTreeSet<&str> {
    v.route
        .iter()
        .flat_map(|r| r.legs.iter().skip(v.leg))
        .filter_map(|l| l.road.as_deref())
        .collect()
}

/// Rule `evac_route_cut`: evacuees already travelling along a road that the
/// fire cuts (or has cut).
fn evacuees_on_cut_road(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let mut per: BTreeMap<&str, (u32, u32, i64)> = BTreeMap::new();
    for v in
        cx.w.d
            .traffic
            .vehicles
            .iter()
            .filter(|v| v.departed && !v.completed)
    {
        for road in remaining_roads(v) {
            if let Some(t) = cx.cut_s(road) {
                let e = per.entry(road).or_insert((0, 0, t));
                e.0 += 1;
                e.1 += v.people;
                e.2 = e.2.min(t);
            }
        }
    }
    per.into_iter()
        .map(|(road, (vehicles, people, t))| {
            let name = cx.road_name(road);
            let (now, at_s) = cx.timing(t, 5);
            let pos = scn.road(road).and_then(|r| r.points.get(r.points.len() / 2).copied());
            let (it, en) = if t <= cx.now {
                ("è già interrotta dal fuoco".to_string(), "is already cut by the fire".to_string())
            } else {
                (format!("sarà interrotta dal fuoco verso le {}", hhmm(scn, t)), format!("will be cut by the fire around {}", hhmm(scn, t)))
            };
            Suggestion {
                id: format!("evac_route_cut:{road}"),
                rule: "evac_route_cut",
                severity: Severity::High,
                now,
                at_s,
                reach_s: Some(t),
                object: ObjectRef { kind: "road", id: road.into(), name: name.clone(), pos },
                recipients: recipients_for(scn, &["CloseRoad", "EvacuateZone", "EvacuateBuilding"]),
                sender: scn.default_persona_name("it"),
                title: lt(format!("{name}: evacuati in viaggio su strada a rischio"), format!("{name}: evacuees travelling on a road at risk")),
                text: lt(
                    format!("{vehicles} veicoli di evacuazione ({people} persone) percorrono {name}, che {it}. Serve un itinerario alternativo o una scorta."),
                    format!("{vehicles} evacuation vehicles ({people} people) are driving along {name}, which {en}. An alternative route or an escort is needed."),
                ),
                evidence: lt(
                    format!("{vehicles} veicoli, {people} persone su {name}, che {it}"),
                    format!("{vehicles} vehicles, {people} people on {name}, which {en}"),
                ),
            }
        })
        .collect()
}

/// Rules `shelter_front` and `shelter_over`.
fn shelters(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let mut incoming: BTreeMap<u32, u32> = BTreeMap::new();
    for st in &cx.w.d.buildings {
        if let (Some(dest), true) = (st.evac_destination, st.evac_done_at.is_some()) {
            *incoming.entry(dest).or_default() += st.evac_pending;
        }
    }
    let mut out = Vec::new();
    for (i, b) in scn.buildings.iter().enumerate() {
        if b.shelter_capacity == 0 {
            continue;
        }
        let sheltered = cx.w.d.shelters.get(&b.id).copied();
        let coming = incoming.get(&b.id).copied().unwrap_or(0);
        if sheltered.is_none() && coming == 0 {
            continue;
        }
        let name = b.name.clone().unwrap_or_else(|| format!("#{}", b.id));
        let there = sheltered.unwrap_or(0);
        let obj = ObjectRef {
            kind: "building",
            id: format!("b{}", b.id),
            name: name.clone(),
            pos: Some(b.pos),
        };
        let rec = recipients_for(scn, &["OpenShelter", "EvacuateBuilding", "EvacuateZone"]);
        if let Some(reach) = reach_of(cx, i) {
            let (now, at_s) = cx.timing(reach, crate::EVAC_LEAD_MIN);
            let (it, en) = if reach <= cx.now {
                (
                    "è già minacciato dal fuoco".to_string(),
                    "is already threatened by the fire".to_string(),
                )
            } else {
                (
                    format!("sarà raggiunto verso le {}", hhmm(scn, reach)),
                    format!("will be reached around {}", hhmm(scn, reach)),
                )
            };
            out.push(Suggestion {
                id: format!("shelter_front:b{}", b.id),
                rule: "shelter_front",
                severity: Severity::High,
                now,
                at_s,
                reach_s: Some(reach),
                object: obj.clone(),
                recipients: rec.clone(),
                sender: scn.default_persona_name("it"),
                title: lt(format!("{name}: rifugio vicino al fronte"), format!("{name}: shelter near the front")),
                text: lt(
                    format!("Il rifugio {name} ({there} accolti, {coming} in arrivo) {it}. Individuare un rifugio alternativo e dirottare gli evacuati."),
                    format!("The shelter {name} ({there} sheltered, {coming} arriving) {en}. Find an alternative shelter and redirect the evacuees."),
                ),
                evidence: lt(
                    format!("{name}: rifugio con {there} accolti e {coming} in arrivo, {it}"),
                    format!("{name}: shelter with {there} sheltered and {coming} arriving, {en}"),
                ),
            });
        }
        let total = there + coming;
        if total > b.shelter_capacity {
            out.push(Suggestion {
                id: format!("shelter_over:b{}", b.id),
                rule: "shelter_over",
                severity: Severity::Medium,
                now: true,
                at_s: None,
                reach_s: None,
                object: obj,
                recipients: rec,
                sender: scn.default_persona_name("it"),
                title: lt(format!("{name}: capienza superata"), format!("{name}: capacity exceeded")),
                text: lt(
                    format!("Il rifugio {name} ha {total} persone previste per {} posti. Aprire un secondo rifugio o limitare gli arrivi.", b.shelter_capacity),
                    format!("The shelter {name} expects {total} people for {} places. Open a second shelter or limit arrivals.", b.shelter_capacity),
                ),
                evidence: lt(
                    format!("{name}: {there} accolti + {coming} in arrivo = {total} su {} posti", b.shelter_capacity),
                    format!("{name}: {there} sheltered + {coming} arriving = {total} for {} places", b.shelter_capacity),
                ),
            });
        }
    }
    out
}

/// Rule `task_route_cut`: a deployment whose route crosses a road the fire cuts.
fn deployments_cut_off(cx: &Ctx) -> Vec<Suggestion> {
    let scn = cx.scn();
    let mut out = Vec::new();
    for task in &cx.w.d.tasks {
        if !matches!(
            task.status,
            TaskStatus::EnRoute | TaskStatus::Withdrawing | TaskStatus::Refilling
        ) {
            continue;
        }
        let Some(route) = &task.route else { continue };
        let cut = route
            .legs
            .iter()
            .filter_map(|l| l.road.as_deref())
            .filter_map(|r| cx.cut_s(r).map(|t| (t, r)))
            .min();
        let Some((t, road)) = cut else { continue };
        let name = cx.road_name(road);
        let zone = scn.zone_of(task.target).unwrap_or_default();
        let (now, at_s) = cx.timing(t, 5);
        let (it, en) = if t <= cx.now {
            ("è già interrotta".to_string(), "is already cut".to_string())
        } else {
            (
                format!("sarà interrotta verso le {}", hhmm(scn, t)),
                format!("will be cut around {}", hhmm(scn, t)),
            )
        };
        out.push(Suggestion {
            id: format!("task_route_cut:{}", task.id),
            rule: "task_route_cut",
            severity: Severity::Medium,
            now,
            at_s,
            reach_s: Some(t),
            object: ObjectRef { kind: "task", id: task.id.to_string(), name: format!("{} {}", task.qty, task.resource), pos: Some(task.target) },
            recipients: vec![task.role.clone()],
            sender: scn.default_persona_name("it"),
            title: lt(format!("{}: percorso dei mezzi a rischio", task.role), format!("{}: deployed units' route at risk", task.role)),
            text: lt(
                format!("Il percorso dei vostri mezzi ({} {}) verso {zone} passa da {name}, che {it}. Valutare un percorso alternativo o il rientro.", task.qty, task.resource),
                format!("The route of your units ({} {}) to {zone} runs along {name}, which {en}. Consider an alternative route or returning.", task.qty, task.resource),
            ),
            evidence: lt(
                format!("{} {} di {} diretti a {zone} via {name}, che {it}", task.qty, task.resource, task.role),
                format!("{} {} of {} heading to {zone} via {name}, which {en}", task.qty, task.resource, task.role),
            ),
        });
    }
    out
}

impl Exercise {
    /// Suggestions for the present situation (see [`suggest`]).
    pub fn suggestions(&self, preview: Option<&Preview>) -> Vec<Suggestion> {
        suggest(&self.world, preview, &self.suggestion_marks)
    }

    /// Record that the EXCON dismissed, sent or planned a suggestion; it is not
    /// proposed again. A rollback to before the decision forgets it.
    pub fn mark_suggestion(&mut self, id: &str, status: &str) -> Result<()> {
        if !matches!(status, "dismissed" | "sent" | "planned") {
            bail!("unknown suggestion status {status}");
        }
        if id.is_empty() || id.len() > 200 {
            bail!("invalid suggestion id");
        }
        let at_s = self.world.time_s();
        self.suggestion_marks.insert(
            id.to_string(),
            SuggestionMark {
                status: status.into(),
                at_s,
            },
        );
        self.record(
            "suggestion_marked",
            MASTER,
            json!({"suggestion": id, "status": status}),
        );
        Ok(())
    }

    /// Add a planned inject to the timeline (MSEL) at a future instant. It is
    /// only listed: like every inject it is sent by the EXCON, never by itself.
    pub fn add_planned_inject(
        &mut self,
        title: &str,
        sender: &str,
        recipients: &[String],
        text: &str,
        at_s: i64,
        suggestion: Option<&str>,
    ) -> Result<String> {
        if self.phase == Phase::Debrief {
            bail!("the exercise has ended");
        }
        if title.trim().is_empty()
            || text.trim().is_empty()
            || title.chars().count() > 200
            || text.chars().count() > 4000
        {
            bail!("an inject needs a title (up to 200 characters) and a text (up to 4000)");
        }
        if at_s < self.world.time_s() || at_s > 7 * 86_400 {
            bail!("the planned time must be now or later, within seven simulated days");
        }
        for r in recipients {
            if r != "all" && !self.players.contains_key(r) {
                bail!("unknown recipient {r}");
            }
        }
        if recipients.is_empty() {
            bail!("no recipients");
        }
        let mut n = self.world.scn.msel.len() + 1;
        let id = loop {
            let id = format!("SG-{n:02}");
            if !self.world.scn.msel.iter().any(|t| t.id == id) {
                break id;
            }
            n += 1;
        };
        let turn = (at_s / (self.turn_minutes as i64 * 60)) as u32 + 1;
        let template = InjectTemplate {
            id: id.clone(),
            turn,
            title: title.trim().to_string(),
            sender: if sender.trim().is_empty() {
                "EXCON".into()
            } else {
                sender.trim().to_string()
            },
            recipients: recipients.to_vec(),
            text: text.trim().to_string(),
            objective: "Added from a situation suggestion".into(),
            event: "Situation".into(),
            at_s: Some(at_s),
            actions: Vec::new(),
        };
        std::sync::Arc::make_mut(&mut self.world.scn)
            .msel
            .push(template);
        self.record(
            "planned_inject_added",
            MASTER,
            json!({"inject": id, "at_s": at_s, "suggestion": suggestion}),
        );
        if let Some(s) = suggestion {
            self.mark_suggestion(s, "planned")?;
        }
        Ok(id)
    }
}
