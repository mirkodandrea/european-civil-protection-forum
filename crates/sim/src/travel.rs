//! Deployment routes, work and replenishment are simulation state, never animation.
use crate::{
    network::{distance, interpolate, Route},
    Actor, Command, World,
};
use scenario::Pos;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize)]
pub struct TravelSetting {
    pub origin: Pos,
    pub speed_mps: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    EnRoute,
    Working,
    Refilling,
    Withdrawing,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize)]
pub struct DeploymentTask {
    pub id: u64,
    pub role: String,
    pub allocations: Vec<u64>,
    pub resource: String,
    pub qty: u32,
    pub command: Command,
    pub origin: Pos,
    pub target: Pos,
    pub speed_mps: f32,
    pub departed_s: i64,
    pub arrival_s: i64,
    pub arrived_s: Option<i64>,
    pub completed_s: Option<i64>,
    pub status: TaskStatus,
    pub detail: String,
    pub route: Option<Route>,
    pub route_checked_s: i64,
    pub work_to: Option<Pos>,
    pub work_m: f32,
    pub water_l: f32,
    pub used_water_l: f32,
    pub last_work_s: i64,
    pub refill_done_s: Option<i64>,
}
impl DeploymentTask {
    pub fn position(&self, t: i64) -> Pos {
        if self.status == TaskStatus::Working {
            if let Some(to) = self.work_to {
                return interpolate(
                    self.target,
                    to,
                    (self.work_m / distance(self.target, to).max(0.01)).clamp(0.0, 1.0),
                );
            }
        }
        if let Some(route) = &self.route {
            if route.legs.is_empty() {
                return self.target;
            }
            return route.position((t - self.departed_s) as f32, self.speed_mps);
        }
        let fraction = if self.arrival_s <= self.departed_s {
            1.0
        } else {
            ((t - self.departed_s) as f32 / (self.arrival_s - self.departed_s) as f32)
                .clamp(0.0, 1.0)
        };
        interpolate(self.origin, self.target, fraction)
    }
}
impl World {
    pub(crate) fn evacuation_targets(&self, cmd: &Command) -> Vec<usize> {
        match cmd {
            Command::EvacuateZone { zone, .. } => self
                .zone_code(zone)
                .map(|z| self.buildings_in_zone(&z))
                .unwrap_or_default(),
            Command::EvacuateBuilding { building, .. } => {
                self.find_building(building).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }
    pub(crate) fn travel_target(&self, cmd: &Command) -> Option<Pos> {
        match cmd {
            Command::CutFireLine {
                from, to, along, ..
            } => self.line_endpoints(from, to, along).ok().map(|(a, _)| a),
            Command::WaterDrop { at } | Command::DeployEngine { at } => self.resolve_loc(at).ok(),
            Command::CloseRoad { road } => self
                .find_road(road)
                .and_then(|id| self.scn.road(&id))
                .and_then(|r| r.points.first().copied()),
            Command::EvacuateBuilding { building, .. } => self
                .find_building(building)
                .map(|i| self.scn.buildings[i].pos),
            Command::EvacuateZone { zone, .. } => self
                .zone_code(zone)
                .and_then(|z| self.scn.resolve_place(&z)),
            _ => None,
        }
    }
    pub(crate) fn dispatch(
        &mut self,
        cmd: &Command,
        role: &str,
        allocations: Vec<u64>,
    ) -> Option<String> {
        let mut target = self.travel_target(cmd)?;
        let a = self
            .d
            .allocations
            .iter()
            .find(|a| allocations.contains(&a.id))?;
        let setting = self.d.travel_settings.get(role)?.get(&a.resource)?;
        let mut route = if self.scn.operations.road_routing && a.resource != "air_tankers" {
            self.network.route(
                setting.origin,
                target,
                &self.d.roads,
                false,
                a.resource == "ground_crews",
            )
        } else {
            None
        };
        if a.resource == "engines" {
            if let Some(r) = route.as_mut() {
                if r.legs
                    .last()
                    .is_some_and(|l| l.road.is_none() && l.length_m > 0.01)
                {
                    target = r.legs.pop().unwrap().from;
                }
            }
        }
        let departed_s = self.time_s()
            + if self.scn.operations.progressive_suppression && a.resource == "air_tankers" {
                1500
            } else {
                0
            };
        let duration = route
            .as_ref()
            .map(|r| r.seconds(setting.speed_mps))
            .unwrap_or(distance(setting.origin, target) / setting.speed_mps);
        let arrival_s = departed_s + duration.ceil().max(1.0) as i64;
        let task = DeploymentTask {
            id: a.id,
            role: role.into(),
            allocations,
            resource: a.resource.clone(),
            qty: a.qty,
            command: cmd.clone(),
            origin: setting.origin,
            target,
            speed_mps: setting.speed_mps,
            departed_s,
            arrival_s,
            arrived_s: None,
            completed_s: None,
            status: TaskStatus::EnRoute,
            detail: format!("Dispatched; arrival at t+{arrival_s}s; effect waits for arrival"),
            route,
            route_checked_s: departed_s,
            work_to: match cmd {
                Command::CutFireLine {
                    from, to, along, ..
                } => self.line_endpoints(from, to, along).ok().map(|(_, b)| b),
                _ => None,
            },
            work_m: 0.0,
            water_l: if a.resource == "engines" {
                2500.0
            } else {
                6137.0
            },
            used_water_l: 0.0,
            last_work_s: departed_s,
            refill_done_s: None,
        };
        let detail = task.detail.clone();
        self.d.tasks.push(task);
        self.event(
            "deployment_dispatched",
            format!("{role}: {detail}"),
            self.scn.zone_of(target),
        );
        Some(detail)
    }
    fn end_task(&mut self, i: usize, status: TaskStatus, detail: String) {
        let t = self.time_s();
        let task = &mut self.d.tasks[i];
        task.status = status;
        task.completed_s = Some(t);
        task.detail = detail.clone();
        let id = task.id;
        self.event(
            "deployment_completed",
            format!("Task #{id}: {detail}"),
            None,
        );
    }
    fn return_to_base(&mut self, i: usize, status: TaskStatus) {
        let t = self.time_s();
        let task = &self.d.tasks[i];
        let current = task.position(t);
        let base = self.d.travel_settings[&task.role][&task.resource].origin;
        let route = if self.scn.operations.road_routing && task.resource != "air_tankers" {
            self.network.route(
                current,
                base,
                &self.d.roads,
                false,
                task.resource == "ground_crews",
            )
        } else {
            None
        };
        if self.scn.operations.road_routing && task.resource != "air_tankers" && route.is_none() {
            self.end_task(
                i,
                TaskStatus::Failed,
                "No safe return route; unit stranded".into(),
            );
            return;
        }
        let task = &mut self.d.tasks[i];
        task.origin = current;
        task.target = base;
        task.departed_s = t;
        task.arrival_s = t + route
            .as_ref()
            .map(|r| r.seconds(task.speed_mps))
            .unwrap_or(distance(current, base) / task.speed_mps)
            .ceil()
            .max(1.0) as i64;
        task.route = route;
        task.status = status;
        task.refill_done_s =
            Some(task.arrival_s + if task.resource == "engines" { 150 } else { 90 });
        task.detail = if status == TaskStatus::Withdrawing {
            "Withdrawing for safety"
        } else {
            "Returning to staging for replenishment"
        }
        .into();
    }
    pub(crate) fn step_tasks(&mut self) {
        let t = self.time_s();
        for i in 0..self.d.tasks.len() {
            let task = self.d.tasks[i].clone();
            let qty = self.task_active_qty(&task);
            if qty == 0 {
                continue;
            }
            if matches!(task.status, TaskStatus::Refilling | TaskStatus::Withdrawing) {
                if t < task.refill_done_s.unwrap_or(task.arrival_s) {
                    continue;
                }
                if task.status == TaskStatus::Withdrawing {
                    self.end_task(i, TaskStatus::Failed, "Safety withdrawal complete".into());
                    continue;
                }
                self.d.tasks[i].water_l = if task.resource == "engines" {
                    2500.0
                } else {
                    6137.0
                };
                if task.resource == "engines" {
                    let mut at = self.travel_target(&task.command).unwrap();
                    let origin = task.target;
                    let mut route = self.network.route(origin, at, &self.d.roads, false, false);
                    if let Some(r) = route.as_mut() {
                        if r.legs
                            .last()
                            .is_some_and(|l| l.road.is_none() && l.length_m > 60.0)
                        {
                            self.end_task(
                                i,
                                TaskStatus::Failed,
                                "Assigned target exceeds hose reach after refill".into(),
                            );
                            continue;
                        }
                        if r.legs
                            .last()
                            .is_some_and(|l| l.road.is_none() && l.length_m > 0.01)
                        {
                            at = r.legs.pop().unwrap().from;
                        }
                    }
                    if route.is_none() {
                        self.end_task(i, TaskStatus::Failed, "No route after refill".into());
                        continue;
                    }
                    let q = &mut self.d.tasks[i];
                    q.route = route;
                    q.origin = origin;
                    q.target = at;
                    q.departed_s = t;
                    q.arrival_s = t + q.route.as_ref().unwrap().seconds(q.speed_mps).ceil() as i64;
                    q.status = TaskStatus::EnRoute;
                    q.detail = "Refilled; returning to assigned post".into();
                } else {
                    self.end_task(
                        i,
                        TaskStatus::Completed,
                        "Drop delivered; aircraft returned and replenished".into(),
                    );
                }
                continue;
            }
            if task.status == TaskStatus::EnRoute {
                if self.scn.operations.road_routing
                    && task.resource != "air_tankers"
                    && t - task.route_checked_s >= 30
                {
                    self.d.tasks[i].route_checked_s = t;
                    if task.route.as_ref().is_some_and(|r| {
                        r.legs.iter().any(|l| {
                            l.road
                                .as_ref()
                                .is_some_and(|id| self.d.roads[id].cut_by_fire)
                        })
                    }) {
                        let current = task.position(t);
                        let route = self.network.route(
                            current,
                            task.target,
                            &self.d.roads,
                            false,
                            task.resource == "ground_crews",
                        );
                        if let Some(route) = route {
                            let q = &mut self.d.tasks[i];
                            q.arrival_s = t + route.seconds(q.speed_mps).ceil() as i64;
                            q.origin = current;
                            q.departed_s = t;
                            q.route = Some(route);
                            q.detail = "Rerouted around fire-cut road".into();
                        } else {
                            self.end_task(
                                i,
                                TaskStatus::Failed,
                                "Route cut by fire; target unreachable".into(),
                            );
                        }
                        continue;
                    }
                }
                if t < task.arrival_s {
                    continue;
                }
                self.d.tasks[i].arrived_s = Some(t);
                self.d.tasks[i].last_work_s = t;
                if self.scn.operations.progressive_suppression
                    && matches!(
                        task.command,
                        Command::CutFireLine { .. } | Command::DeployEngine { .. }
                    )
                {
                    let work_position = if let Command::CutFireLine {
                        from, to, along, ..
                    } = &task.command
                    {
                        let (a, b) = self.line_endpoints(from, to, along).unwrap();
                        interpolate(a, b, (task.work_m / distance(a, b)).clamp(0.0, 1.0))
                    } else {
                        task.target
                    };
                    if self.d.hazard.at(work_position).danger >= 0.35 {
                        self.return_to_base(i, TaskStatus::Withdrawing);
                    } else {
                        self.d.tasks[i].status = TaskStatus::Working;
                        self.d.tasks[i].detail = "Arrived; work proceeds on simulated time".into();
                    }
                } else if self.scn.operations.progressive_suppression
                    && matches!(task.command, Command::WaterDrop { .. })
                {
                    let density =
                        task.water_l / (std::f32::consts::PI * crate::DROP_RADIUS_M.powi(2));
                    self.fire
                        .water_drop(task.target, crate::DROP_RADIUS_M, density);
                    self.d.drops.push(crate::DropRec {
                        at: task.target,
                        radius_m: crate::DROP_RADIUS_M,
                        by: task.role.clone(),
                        t_s: t,
                    });
                    self.d.tasks[i].used_water_l += task.water_l;
                    self.d.tasks[i].water_l = 0.0;
                    self.event(
                        "water_drop",
                        format!(
                            "{} delivered {:.0} L; {:.2} L/m²",
                            task.role, task.water_l, density
                        ),
                        self.scn.zone_of(task.target),
                    );
                    self.return_to_base(i, TaskStatus::Refilling);
                } else {
                    let mut pools = self.d.pools.clone();
                    let result = match self.check_inner(&task.command, &Actor::Master, &mut pools) {
                        Ok(_) => self.apply_effect(&task.command, &Actor::Role(task.role.clone())),
                        Err(e) => Err(e),
                    };
                    match result {
                        Ok(notes) => {
                            let evacuation = !self.evacuation_targets(&task.command).is_empty();
                            let q = &mut self.d.tasks[i];
                            q.status = if evacuation {
                                TaskStatus::Working
                            } else {
                                TaskStatus::Completed
                            };
                            q.detail = notes.join("; ");
                            if !evacuation {
                                q.completed_s = Some(t);
                            }
                        }
                        Err(e) => self.end_task(i, TaskStatus::Failed, e),
                    }
                }
                self.event(
                    "deployment_arrived",
                    format!(
                        "{} task #{} arrived: {}",
                        task.role, task.id, self.d.tasks[i].detail
                    ),
                    self.scn.zone_of(task.target),
                );
                continue;
            }
            if task.status != TaskStatus::Working {
                continue;
            }
            if !self.evacuation_targets(&task.command).is_empty() {
                if self
                    .evacuation_targets(&task.command)
                    .iter()
                    .all(|i| self.d.buildings[*i].evac_done_at.is_none())
                {
                    self.end_task(
                        i,
                        TaskStatus::Completed,
                        "Evacuation completed; units remain deployed until EXCON return".into(),
                    );
                }
                continue;
            }
            if self.d.hazard.at(task.target).danger >= 0.35 {
                self.return_to_base(i, TaskStatus::Withdrawing);
                continue;
            }
            let elapsed = (t - task.last_work_s).max(0) as f32;
            self.d.tasks[i].last_work_s = t;
            match &task.command {
                Command::CutFireLine {
                    from,
                    to,
                    along,
                    width_m,
                } => {
                    let (a, b) = self.line_endpoints(from, to, along).unwrap();
                    let length = distance(a, b);
                    let width = width_m.unwrap_or(6.0);
                    let new = (task.work_m + 120.0 / 3600.0 * qty as f32 * 6.0 / width * elapsed)
                        .min(length);
                    // Clear only earned cell-sized progress, not a full cell on the first second.
                    let before = (task.work_m / self.scn.grid.cellsize).floor();
                    let after = (new / self.scn.grid.cellsize).floor();
                    self.d.tasks[i].work_m = new;
                    if after > before || new >= length {
                        let end = interpolate(a, b, new / length);
                        self.fire.fire_line(a, end, width);
                        if let Some(line) = self
                            .d
                            .lines
                            .iter_mut()
                            .find(|l| l.by == format!("task:{}", task.id))
                        {
                            line.to = end;
                        } else {
                            self.d.lines.push(crate::LineRec {
                                from: a,
                                to: end,
                                width_m: width,
                                by: format!("task:{}", task.id),
                                t_s: t,
                            });
                        }
                    }
                    self.d.tasks[i].detail = format!("Cut {:.0}/{length:.0} m; {qty} crews", new);
                    if new >= length {
                        self.end_task(
                            i,
                            TaskStatus::Completed,
                            format!("{length:.0} m line completed"),
                        );
                    }
                }
                Command::DeployEngine { .. } => {
                    // Pump in 10-second quanta; integrate the actual elapsed interval.
                    if t % 10 != 0 {
                        self.d.tasks[i].last_work_s = task.last_work_s;
                        continue;
                    }
                    let candidates = self
                        .fire
                        .state()
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| **s == fire::CellFire::Unburnt)
                        .filter_map(|(c, _)| {
                            let p = self.scn.cell_centre(scenario::Cell {
                                row: c / self.scn.grid.cols,
                                col: c % self.scn.grid.cols,
                            });
                            (self.fire.added_moisture()[c] < 30.0
                                && distance(p, task.target) <= 60.0
                                && self.fire.is_burnable(scenario::Cell {
                                    row: c / self.scn.grid.cols,
                                    col: c % self.scn.grid.cols,
                                }))
                            .then_some(p)
                        })
                        .filter(|p| self.d.hazard.at(*p).danger > 0.0)
                        .collect::<Vec<_>>();
                    if candidates.is_empty() {
                        continue;
                    }
                    let litres = (400.0 / 60.0 * elapsed).min(task.water_l);
                    let radius = self.scn.grid.cellsize * 0.45;
                    let density =
                        litres / (candidates.len() as f32 * std::f32::consts::PI * radius.powi(2));
                    for p in candidates {
                        self.fire.water_drop(p, radius, density);
                    }
                    self.d.tasks[i].water_l -= litres;
                    self.d.tasks[i].used_water_l += litres;
                    self.d.tasks[i].detail = format!(
                        "Engine protecting fuel within hose reach; {:.0} L remaining",
                        self.d.tasks[i].water_l
                    );
                    if self.d.tasks[i].water_l <= 0.01 {
                        self.return_to_base(i, TaskStatus::Refilling);
                    }
                }
                _ => {}
            }
        }
    }
    pub fn task_active_qty(&self, task: &DeploymentTask) -> u32 {
        self.d
            .allocations
            .iter()
            .filter(|a| task.allocations.contains(&a.id))
            .map(|a| a.qty.saturating_sub(a.returned))
            .sum()
    }
    pub fn deployment_view(&self) -> serde_json::Value {
        serde_json::json!(self.d.tasks.iter().map(|task|serde_json::json!({"task":task,"position_m":task.position(self.time_s()),"active_qty":self.task_active_qty(task)})).collect::<Vec<_>>())
    }
}
