//! The model side of the view contract: turns the exercise into a
//! [`view::ViewState`] for any viewer (big screen, remote viewer, a future
//! HTML canvas). Read-only: nothing here can change the exercise.
//!
//! In-process viewers use [`ExerciseSource`], which `try_lock`s so a slow
//! request never stalls a frame; remote ones use `GET /api/view/scene` and
//! `GET /api/view/state` (EXCON only: the view shows the ground truth).

use std::sync::{Arc, Mutex};

use exercise::Exercise;
use view::{
    BuildingDraw, Cue, Deployment, Drop, Ensemble, FireLine, Have, RoadSegment, Stats, TaskStatus,
    Track, TurnWindow, Vehicle, ViewSource, ViewState,
};

/// How many feed lines the big screen shows.
const FEED_LINES: usize = 9;
/// How many of the latest extinguish interventions a viewer is sent.
const EXTINGUISHED_SENT: usize = 16;

fn task_status(s: sim::TaskStatus) -> TaskStatus {
    use sim::TaskStatus as S;
    match s {
        S::EnRoute => TaskStatus::EnRoute,
        S::Working => TaskStatus::Working,
        S::Refilling => TaskStatus::Refilling,
        S::Withdrawing => TaskStatus::Withdrawing,
        S::Suspended => TaskStatus::Suspended,
        S::Completed => TaskStatus::Completed,
        S::Failed => TaskStatus::Failed,
        S::Cancelled => TaskStatus::Cancelled,
    }
}

/// The task's motion as waypoints in simulated time. Mirrors
/// `DeploymentTask::position` (a test pins them together).
fn track(task: &sim::DeploymentTask, now_s: i64) -> Track {
    if task.status == sim::TaskStatus::Working && task.work_to.is_some() {
        return Track {
            points: vec![(now_s as f32, task.position(now_s))],
        };
    }
    let start = task.departed_s as f32;
    let points = match &task.route {
        Some(route) if route.legs.is_empty() => vec![(start, task.target)],
        Some(route) => {
            let mut t = start;
            let mut points = vec![(t, route.legs[0].from)];
            for leg in &route.legs {
                t += leg.length_m / leg.speed_mps.min(task.speed_mps);
                points.push((t, leg.to));
            }
            points
        }
        None if task.arrival_s <= task.departed_s => vec![(start, task.target)],
        None => vec![
            (start, task.origin),
            (task.arrival_s as f32, task.target),
        ],
    };
    Track { points }
}

fn deployment(w: &sim::World, task: &sim::DeploymentTask) -> Deployment {
    Deployment {
        id: task.id,
        resource: task.resource.clone(),
        status: task_status(task.status),
        qty: w.task_active_qty(task),
        origin: task.origin,
        target: task.target,
        speed_mps: task.speed_mps,
        arrival_s: task.arrival_s,
        track: track(task, w.time_s()),
        route: task.route.as_ref().map(|r| r.points()),
        action: task.command.kind().to_string(),
        zones: match &task.command {
            sim::Command::MonitorArea { zones, .. } => w
                .d
                .monitors
                .get(&task.id)
                .map(|m| m.zones.clone())
                .unwrap_or_else(|| w.parse_monitor_zones(zones).unwrap_or_default()),
            sim::Command::Reconnoitre { zone, .. } => w.parse_monitor_zones(std::slice::from_ref(zone)).unwrap_or_default(),
            _ => Vec::new(),
        },
    }
}

fn extinguished(r: &sim::ExtinguishRec) -> view::Extinguished {
    view::Extinguished {
        shape: match &r.shape {
            sim::ExtinguishShape::Rect { bounds } => view::ExtinguishShape::Rect { bounds: *bounds },
            sim::ExtinguishShape::Disc { centre, radius_m } => view::ExtinguishShape::Disc {
                centre: *centre,
                radius_m: *radius_m,
            },
            sim::ExtinguishShape::Polygon { vertices } => view::ExtinguishShape::Polygon {
                vertices: vertices.clone(),
            },
        },
        t_s: r.t_s,
        cells_out: u32::try_from(r.cells_out).unwrap_or(u32::MAX),
    }
}

fn window(r: &exercise::TurnWindow) -> TurnWindow {
    TurnWindow {
        turn: r.turn,
        from_s: r.from_s,
        to_s: r.to_s,
    }
}

/// The view of `ex`, or `None` when the viewer already has this generation.
/// Cues are only those the viewer has not seen (`have.cue`).
pub fn extract(ex: &Exercise, have: Have) -> Option<ViewState> {
    if have.generation != 0 && ex.generation == have.generation {
        return None;
    }
    let w = &ex.world;
    let cleared = w.fire.cleared();
    let wet = w.fire.added_moisture();
    let prob = w.fire.probability();
    let cells = w
        .fire
        .state()
        .iter()
        .enumerate()
        .map(|(i, s)| match s {
            fire::CellFire::Burning => view::cell::BURNING,
            fire::CellFire::Burnt => view::cell::BURNT,
            fire::CellFire::Unburnt if cleared[i] => view::cell::CLEARED,
            fire::CellFire::Unburnt if wet[i] > 10.0 => view::cell::WET,
            fire::CellFire::Unburnt if prob[i] > 0.0 => view::cell::HALO,
            fire::CellFire::Unburnt => view::cell::UNBURNT,
        })
        .collect();
    let st = w.stats();
    let options = w.fire.options();
    Some(ViewState {
        scenario_id: w.scn.id.clone(),
        generation: ex.generation,
        turn: ex.turn,
        phase: format!("{:?}", ex.phase).to_lowercase(),
        time_s: w.time_s(),
        clock: w.clock(),
        weather: Some(w.fire.weather()),
        ensemble: Some(Ensemble {
            realizations: options.realizations,
            cutoff: options.cutoff,
        }),
        stats: Some(Stats {
            burned_ha: st.burned_ha,
            burning_cells: st.burning_cells,
            damaged: st.damaged,
            destroyed: st.destroyed,
            threatened: st.threatened,
            evacuated_people: st.evacuated_people,
            people_at_risk: st.people_at_risk,
            sheltered: st.sheltered,
            line_km: st.line_km,
            drops: st.drops,
        }),
        cells,
        arrival: w.fire.arrival_times().to_vec(),
        intensity: w.fire.intensity().to_vec(),
        probability: prob.to_vec(),
        buildings: w
            .d
            .buildings
            .iter()
            .map(|b| BuildingDraw {
                damage: match b.damage {
                    sim::Damage::Intact => 0,
                    sim::Damage::Damaged => 1,
                    sim::Damage::Destroyed => 2,
                },
                threatened: b.threatened,
                evacuating: b.evac_done_at.is_some(),
                empty_evacuated: b.people_inside == 0 && b.evacuated > 0,
                evac_people: if b.evac_done_at.is_some() {
                    b.evac_pending.min(b.people_inside)
                } else {
                    0
                },
                evac_to: b
                    .evac_destination
                    .and_then(|id| w.scn.buildings.iter().find(|s| s.id == id))
                    .map(|s| s.pos),
            })
            .collect(),
        roads: w
            .scn
            .roads
            .iter()
            .map(|r| {
                w.d.roads
                    .get(&r.id)
                    .map(|s| (s.closed, s.cut_by_fire))
                    .unwrap_or_default()
            })
            .collect(),
        road_segments: w
            .network
            .segments()
            .into_iter()
            .filter_map(|segment| {
                let i = w.scn.roads.iter().position(|r| r.id == segment.road)?;
                let st = w.d.roads.get(&segment.road)?;
                let cut = st
                    .fire_closed_links
                    .as_ref()
                    .map_or(st.cut_by_fire, |links| {
                        segment.links.iter().any(|id| links.contains(id))
                    });
                Some(RoadSegment {
                    road: i,
                    from: segment.from,
                    to: segment.to,
                    closed: st.closed,
                    cut_by_fire: cut,
                })
            })
            .collect(),
        lines: w
            .d
            .lines
            .iter()
            .map(|l| FireLine {
                from: l.from,
                to: l.to,
                width_m: l.width_m,
            })
            .collect(),
        drops: w
            .d
            .drops
            .iter()
            .map(|d| Drop {
                at: d.at,
                radius_m: d.radius_m,
                t_s: d.t_s,
            })
            .collect(),
        extinguished: w.d.extinguished[w.d.extinguished.len().saturating_sub(EXTINGUISHED_SENT)..]
            .iter()
            .map(extinguished)
            .collect(),
        traffic: w
            .d
            .traffic
            .vehicles
            .iter()
            .filter(|v| v.departed && !v.completed)
            .map(|v| Vehicle {
                pos: w.traffic_position(v, w.time_s()),
                people: v.people,
                at_risk: v.at_risk,
            })
            .collect(),
        deployments: w.d.tasks.iter().map(|t| deployment(w, t)).collect(),
        traffic_model: w.scn.operations.traffic,
        warned: w.d.warned.keys().cloned().collect(),
        warnings: w
            .warnings()
            .into_iter()
            .map(|x| view::Warning {
                zone: x.zone,
                t_s: x.t_s,
                clock: x.clock,
            })
            .collect(),
        last_turn: ex.last_turn.as_ref().map(window),
        resolving: ex.resolving.as_ref().map(|r| TurnWindow {
            turn: r.turn,
            from_s: r.from_s,
            to_s: r.to_s,
        }),
        clock_running: ex.clock_control.running,
        feed: ex
            .log
            .iter()
            .rev()
            .filter_map(feed_line)
            .take(FEED_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        cues: ex
            .log
            .iter()
            .filter(|e| e.seq > have.cue)
            .filter_map(|e| cue_for(ex, e))
            .collect(),
    })
}

/// A viewer in the same process as the exercise (native big screen, or the
/// WASM build). Never blocks: a busy exercise just means "nothing new yet".
pub struct ExerciseSource(pub Arc<Mutex<Exercise>>);

impl ViewSource for ExerciseSource {
    fn scenario(&mut self) -> Result<Arc<scenario::Scenario>, String> {
        self.0
            .lock()
            .map(|ex| ex.world.scn.clone())
            .map_err(|_| "exercise lock poisoned".to_string())
    }
    fn poll_scene(&mut self, current: &Arc<scenario::Scenario>) -> Option<Arc<scenario::Scenario>> {
        let ex = self.0.try_lock().ok()?;
        (!Arc::ptr_eq(current, &ex.world.scn)).then(|| ex.world.scn.clone())
    }
    fn poll(&mut self, have: Have) -> Option<ViewState> {
        let ex = self.0.try_lock().ok()?;
        extract(&ex, have)
    }
}

/// Where an order acts, and how big an area it covers.
fn command_place(w: &sim::World, cmd: &sim::Command) -> Option<(scenario::Pos, f32)> {
    use sim::Command as C;
    let zone = |z: &str| {
        w.zone_code(z)
            .and_then(|c| w.scn.zone_centre(&c))
            .map(|p| (p, 300.0))
    };
    let building = |b: &str| w.find_building(b).map(|i| (w.scn.buildings[i].pos, 160.0));
    match cmd {
        C::CutFireLine {
            from, to, along, ..
        } => {
            let (a, b) = match along {
                Some(code) => w.scn.zone_edge(code)?,
                None => (
                    w.resolve_loc(from.as_ref()?).ok()?,
                    w.resolve_loc(to.as_ref()?).ok()?,
                ),
            };
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            Some(([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], len / 2.0 + 80.0))
        }
        C::DeployEngine { at } | C::WaterDrop { at } | C::Ignite { at, .. } => {
            w.resolve_loc(at).ok().map(|p| (p, 180.0))
        }
        C::EvacuateZone { zone: z, .. } | C::IssueWarning { zone: z } => zone(z),
        C::EvacuateBuilding { building: b, .. } | C::OpenShelter { building: b } => building(b),
        C::EvacuateDistrict { district, .. } => w.scn.district(district).and_then(|d| {
            let p = w.scn.places.iter().find(|p| p.name == d.name)?;
            Some((p.pos, p.radius_m.unwrap_or(300.0)))
        }),
        C::SetBuildingStatus { building: b, .. } => building(b).or_else(|| zone(b)),
        C::CloseRoad { road } | C::ReopenRoad { road } => w
            .find_road(road)
            .and_then(|id| w.scn.road(&id))
            .and_then(|r| r.points.get(r.points.len() / 2).copied())
            .map(|p| (p, 350.0)),
        C::Extinguish { area } => w
            .extinguish_footprint(area)
            .ok()
            .map(|f| (f.centre(), f.reach_m())),
        C::ExtinguishArea { polygon } => w.resolve_polygon(polygon).ok().map(|poly| {
            let c = sim::polygon_centroid(&poly);
            let r = poly
                .iter()
                .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt())
                .fold(0.0, f32::max);
            (c, r)
        }),
        _ => None,
    }
}

/// The first place a text names: a facility, a gazetteer place (or alias), or a zone code.
fn named_place(w: &sim::World, text: &str) -> Option<(scenario::Pos, f32)> {
    let lower = text.to_lowercase();
    let mut best: Option<(usize, scenario::Pos, f32)> = None;
    let mut consider = |name: &str, pos: scenario::Pos, r: f32| {
        if name.len() < 3 {
            return;
        }
        if let Some(i) = lower.find(&name.to_lowercase()) {
            if best.is_none_or(|(j, _, _)| i < j) {
                best = Some((i, pos, r));
            }
        }
    };
    for b in &w.scn.buildings {
        if let Some(n) = &b.name {
            consider(n, b.pos, 170.0);
        }
    }
    for p in &w.scn.places {
        consider(&p.name, p.pos, 300.0);
        for a in &p.aliases {
            consider(a, p.pos, 300.0);
        }
    }
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        if token.len() == 2 {
            if let (Some(c), Some(pos)) = (w.zone_code(token), text.find(token)) {
                if best.is_none_or(|(j, _, _)| pos < j) {
                    best = w.scn.zone_centre(&c).map(|p| (pos, p, 300.0));
                }
            }
        }
    }
    best.map(|(_, p, r)| (p, r))
}

pub(crate) fn cue_for(ex: &Exercise, e: &exercise::LogEntry) -> Option<Cue> {
    let w = &ex.world;
    let d = &e.data;
    let cmds_cue = |cmds: &[sim::Command], label: String, hold: f32| {
        cmds.iter()
            .find_map(|c| command_place(w, c))
            .map(|(pos, radius_m)| Cue {
                seq: e.seq,
                pos,
                radius_m,
                hold_s: hold,
                label,
            })
    };
    match e.kind.as_str() {
        "inject_sent" => {
            let title = d["title"].as_str().unwrap_or_default();
            let text = ex
                .messages
                .iter()
                .rev()
                .find(|m| m.title.as_deref() == Some(title))
                .map(|m| m.text_en.clone())
                .unwrap_or_default();
            let (pos, radius_m) = named_place(w, &format!("{title} {text}"))?;
            Some(Cue {
                seq: e.seq,
                pos,
                radius_m,
                hold_s: 6.0,
                label: format!("inject: {title}"),
            })
        }
        "proposal_parsed" => {
            let cmds: Vec<sim::Command> = serde_json::from_value(d["commands"].clone()).ok()?;
            cmds_cue(&cmds, format!("proposal from {}", e.actor), 5.0)
        }
        "proposal_decided" if d["approve"].as_bool() == Some(true) => {
            let p = ex.proposal(d["proposal"].as_u64()?)?;
            cmds_cue(&p.commands, "approved".into(), 5.0)
        }
        "master_command" => {
            let c: sim::Command = serde_json::from_value(d["command"].clone()).ok()?;
            cmds_cue(std::slice::from_ref(&c), c.describe(), 6.0)
        }
        _ => None,
    }
}

/// What the audience sees in the HUD feed.
pub(crate) fn feed_line(e: &exercise::LogEntry) -> Option<String> {
    let d = &e.data;
    let s = match e.kind.as_str() {
        "inject_sent" => format!(
            "INJECT  {} -> {}",
            d["title"].as_str()?,
            d["recipients"]
                .as_array()?
                .iter()
                .filter_map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "proposal_decided" if d["approve"].as_bool()? => format!(
            "APPROVED  {}",
            d["facts"]
                .as_str()?
                .lines()
                .next()?
                .trim_start_matches("APPROVED: ")
        ),
        "proposal_decided" => "REJECTED  a proposal".to_string(),
        // A warning shows what it did: zone, time and the evacuation effect.
        "master_command" if d["command"]["type"] == "IssueWarning" => format!(
            "WARNING  {}",
            truncate(d["result"]["ok"][0].as_str()?, 110)
        ),
        "master_command" => format!(
            "EXCON  {}",
            serde_json::from_value::<sim::Command>(d["command"].clone())
                .ok()?
                .describe()
        ),
        "turn_resolving" => format!("TURN {} SIMULATING  ({} min)", d["turn"], d["minutes"]),
        "turn_resolved" => format!("TURN {} RESOLVED  ({} min)", d["turn"], d["minutes"]),
        // The English summary, not the raw reply: the HUD font has no Armenian, Georgian… glyphs.
        "proposal_parsed" if e.actor != exercise::MASTER => format!(
            "ORDERS  {}: {}",
            e.actor,
            truncate(d["summary"].as_str().filter(|s| !s.is_empty())?, 90)
        ),
        "rollback" => format!("ROLLBACK to turn {}", d["to_turn"]),
        _ => return None,
    };
    Some(format!(
        "{}  {}",
        &e.clock[e.clock.len().saturating_sub(5)..],
        s
    ))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(v: serde_json::Value) -> sim::Command {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn extinguish_interventions_reach_the_view_and_roll_back() {
        let scn = Arc::new(scenario::synthetic::generate(1));
        let mut ex = Exercise::new(sim::World::new(scn, 1).unwrap());
        ex.start().unwrap();
        ex.direct_command(&cmd(serde_json::json!(
            {"type":"Ignite","at":{"x":3150,"y":2720},"radius_m":120}
        )))
        .unwrap();
        ex.resolve_turn(Some(30)).unwrap();
        let w = &ex.world;
        let burning = w
            .fire
            .state()
            .iter()
            .position(|s| *s == fire::CellFire::Burning)
            .expect("the opening fire burns after 30 min");
        let p = w.scn.cell_centre(scenario::Cell {
            row: burning / w.scn.grid.cols,
            col: burning % w.scn.grid.cols,
        });
        let zone = w.scn.zone_of(p).expect("a zone");
        let bounds = w.scn.zone_bounds(&zone).unwrap();
        let before = extract(&ex, Have::default()).unwrap();
        assert!(before.extinguished.is_empty());
        let t0 = ex.world.time_s();

        ex.direct_command(&cmd(serde_json::json!({"type":"Extinguish","area":zone})))
            .unwrap();
        let poly = [
            [p[0] + 400.0, p[1] - 300.0],
            [p[0] + 900.0, p[1] - 300.0],
            [p[0] + 900.0, p[1] + 300.0],
        ];
        let vertices: Vec<_> = poly
            .iter()
            .map(|q| serde_json::json!({"x": q[0], "y": q[1]}))
            .collect();
        ex.direct_command(&cmd(serde_json::json!({"type":"ExtinguishArea","polygon":vertices})))
            .unwrap();
        let v = extract(&ex, Have::default()).unwrap();
        assert_eq!(v.extinguished.len(), 2);
        let (zone_rec, area_rec) = (&v.extinguished[0], &v.extinguished[1]);
        assert_eq!(zone_rec.shape, view::ExtinguishShape::Rect { bounds });
        assert_eq!(zone_rec.t_s, t0);
        assert!(zone_rec.cells_out > 0, "the zone held burning cells");
        assert_eq!(
            area_rec.shape,
            view::ExtinguishShape::Polygon {
                vertices: poly.to_vec()
            }
        );
        assert_eq!(area_rec.t_s, t0);
        // The compact wire form carries them too.
        let back = view::wire::apply(&before, &view::wire::diff(&before, &v)).unwrap();
        assert_eq!(back.extinguished, v.extinguished);

        ex.rollback(2).unwrap();
        let after = extract(&ex, Have::default()).unwrap();
        assert!(after.extinguished.is_empty(), "rolled back with the world");
    }
}

