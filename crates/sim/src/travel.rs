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
    Suspended,
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
    /// Approved target, independent of a local work post or refill location.
    pub mission_centre: Pos,
    pub policy_version: String,
    pub policy_reason: String,
    pub last_policy_s: i64,
    pub regroup_until_s: Option<i64>,
}
impl DeploymentTask {
    pub fn position(&self, t: i64) -> Pos {
        if self.status == TaskStatus::Working {
            if let Some(to) = self.work_to {
                return interpolate(
                    self.mission_centre,
                    to,
                    (self.work_m / distance(self.mission_centre, to).max(0.01)).clamp(0.0, 1.0),
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
    pub fn engine_hose_reach_m(&self) -> f32 {
        if self.scn.operations.local_autonomy {
            self.scn.tactical_policy.hose_reach_m
        } else {
            60.0
        }
    }
    /// Bake current queues into this trip once: the ETA and interpolation then
    /// use exactly the same leg speeds, independent of render/update cadence.
    fn operational_route(
        &self,
        from: Pos,
        to: Pos,
        states: &std::collections::BTreeMap<String, crate::RoadState>,
        civilian: bool,
        walk: bool,
        speed_mps: f32,
    ) -> Option<Route> {
        let mut route = self.network.route(from, to, states, civilian, walk)?;
        if self.scn.operations.emergency_congestion && !civilian && !walk {
            self.d.traffic.delay_route(&mut route, speed_mps);
        }
        Some(route)
    }

    /// Deterministic local execution only. Never invents orders or allocations.
    fn step_local_policies(&mut self) {
        if !self.scn.operations.local_autonomy
            || !self.scn.operations.progressive_suppression
            || !self.scn.operations.road_routing
        {
            return;
        }
        let t = self.time_s();
        let p = self.scn.tactical_policy.clone();
        for i in 0..self.d.tasks.len() {
            let task = self.d.tasks[i].clone();
            if matches!(task.command, Command::CutFireLine { .. })
                && task.status == TaskStatus::Suspended
                && self.task_active_qty(&task) > 0
                && t - task.last_policy_s >= p.reassess_s
            {
                self.d.tasks[i].last_policy_s = t;
                let Some(end) = task.work_to else { continue };
                let length = distance(task.mission_centre, end);
                if task.work_m >= length {
                    continue;
                }
                let at = interpolate(task.mission_centre, end, task.work_m / length.max(0.01));
                let current = task.position(t);
                if self.d.hazard.at(at).danger >= p.work_danger
                    || self.d.hazard.at(current).danger >= p.work_danger
                {
                    continue;
                }
                if let Some(route) =
                    self.operational_route(current, at, &self.d.roads, false, true, task.speed_mps)
                {
                    let reason =
                        "Safe route to unfinished approved line; resuming at earned progress";
                    let q = &mut self.d.tasks[i];
                    q.origin = current;
                    q.target = at;
                    q.departed_s = t;
                    q.arrival_s = t + route.seconds(q.speed_mps).ceil().max(1.0) as i64;
                    q.route = Some(route);
                    q.route_checked_s = t;
                    q.status = TaskStatus::EnRoute;
                    q.refill_done_s = None;
                    q.regroup_until_s = None;
                    q.policy_version = p.version.clone();
                    q.policy_reason = reason.into();
                    q.detail = reason.into();
                    self.event(
                        "deployment_policy",
                        format!("Task #{} [{}]: {reason}", task.id, p.version),
                        self.scn.zone_of(at),
                    );
                }
                continue;
            }
            if !matches!(task.command, Command::DeployEngine { .. })
                || !matches!(task.status, TaskStatus::Working | TaskStatus::Suspended)
                || self.task_active_qty(&task) == 0
                || t - task.last_policy_s < p.reassess_s
            {
                continue;
            }
            self.d.tasks[i].last_policy_s = t;
            let current = task.position(t);
            if self.d.hazard.at(current).danger >= p.work_danger {
                continue;
            }
            let others = self
                .d
                .tasks
                .iter()
                .filter(|o| {
                    o.id != task.id
                        && o.resource == "engines"
                        && self.task_active_qty(o) > 0
                        && matches!(o.status, TaskStatus::Working | TaskStatus::EnRoute)
                })
                .map(|o| o.target)
                .collect::<Vec<_>>();
            let work = self
                .fire
                .state()
                .iter()
                .enumerate()
                .filter_map(|(j, s)| {
                    let cell = scenario::Cell {
                        row: j / self.scn.grid.cols,
                        col: j % self.scn.grid.cols,
                    };
                    let at = self.scn.cell_centre(cell);
                    (*s == fire::CellFire::Unburnt
                        && self.fire.is_burnable(cell)
                        && self.fire.added_moisture()[j] < p.wet_limit_pts
                        && distance(at, task.mission_centre) <= p.sector_radius_m + p.hose_reach_m
                        && (!p.share_work
                            || others.iter().all(|o| distance(*o, at) > p.hose_reach_m)))
                    .then(|| (at, self.d.hazard.at(at).danger))
                })
                .filter(|(_, v)| *v > 0.0)
                .collect::<Vec<_>>();
            let score = |at: Pos| -> f32 {
                work.iter()
                    .filter(|(q, _)| distance(at, *q) <= p.hose_reach_m)
                    .map(|(_, v)| *v)
                    .sum()
            };
            if task.status == TaskStatus::Working && score(current) > 0.0 {
                self.d.tasks[i].policy_reason =
                    "Useful unsaturated work remains at current post".into();
                continue;
            }
            let mut best: Option<(f32, f32, Pos, Route)> = None;
            for segment in self.network.segments() {
                let n = (distance(segment.from, segment.to) / self.scn.grid.cellsize)
                    .ceil()
                    .max(1.0) as usize;
                for k in 0..=n {
                    let at = interpolate(segment.from, segment.to, k as f32 / n as f32);
                    if distance(at, task.mission_centre) > p.sector_radius_m
                        || distance(at, current) < 5.0
                        || self.d.hazard.at(at).danger >= p.parking_danger
                    {
                        continue;
                    }
                    let utility = score(at);
                    if utility <= 0.0 {
                        continue;
                    }
                    let Some(route) = self.operational_route(
                        current,
                        at,
                        &self.d.roads,
                        false,
                        false,
                        task.speed_mps,
                    ) else {
                        continue;
                    };
                    // A vehicle may access the road at its start, but may not
                    // leave it to reach a local work post.
                    if route
                        .legs
                        .last()
                        .is_some_and(|l| l.road.is_none() && l.length_m > 0.01)
                    {
                        continue;
                    }
                    let seconds = route.seconds(task.speed_mps);
                    if best.as_ref().is_none_or(|(v, s, _, _)| {
                        utility > *v + 0.0001 || ((utility - *v).abs() < 0.0001 && seconds < *s)
                    }) {
                        best = Some((utility, seconds, at, route));
                    }
                }
            }
            if let Some((_, seconds, at, route)) = best {
                let regroup = if task.status == TaskStatus::Working {
                    p.regroup_s
                } else {
                    0
                };
                let reason = if task.status == TaskStatus::Suspended {
                    "Safe reachable work available; resuming approved mission"
                } else {
                    "Current post exhausted or reserved; regrouping for work inside approved sector"
                };
                let q = &mut self.d.tasks[i];
                q.origin = current;
                q.target = at;
                q.route = Some(route);
                q.departed_s = t + regroup;
                q.arrival_s = q.departed_s + seconds.ceil().max(1.0) as i64;
                q.regroup_until_s = Some(q.departed_s);
                q.status = TaskStatus::EnRoute;
                q.refill_done_s = None;
                q.last_work_s = q.arrival_s;
                q.route_checked_s = t;
                q.policy_reason = reason.into();
                q.detail = reason.into();
                self.event(
                    "deployment_policy",
                    format!(
                        "Task #{} [{}]: {}; destination {:.0},{:.0}; regroup {} s",
                        task.id, p.version, reason, at[0], at[1], regroup
                    ),
                    self.scn.zone_of(at),
                );
            } else {
                self.d.tasks[i].policy_reason =
                    "Waiting: no safe reachable unreserved work inside approved sector".into();
                self.d.tasks[i].detail = self.d.tasks[i].policy_reason.clone();
            }
        }
    }
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
            self.operational_route(
                setting.origin,
                target,
                &self.d.roads,
                false,
                a.resource == "ground_crews",
                setting.speed_mps,
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
            mission_centre: self.travel_target(cmd).unwrap_or(target),
            policy_version: if self.scn.operations.local_autonomy {
                self.scn.tactical_policy.version.clone()
            } else {
                "fixed-post-v1".into()
            },
            policy_reason: "Executing approved mission".into(),
            last_policy_s: departed_s,
            regroup_until_s: None,
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
        let at = task.position(t);
        task.origin = at;
        task.target = at;
        task.route = None;
        task.departed_s = t;
        task.arrival_s = t;
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
            self.operational_route(
                current,
                base,
                &self.d.roads,
                false,
                task.resource == "ground_crews",
                task.speed_mps,
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
        let regroup = if self.scn.operations.local_autonomy && task.resource == "ground_crews" {
            self.scn.tactical_policy.regroup_s
        } else {
            0
        };
        task.departed_s = t + regroup;
        task.regroup_until_s = (regroup > 0).then_some(task.departed_s);
        task.arrival_s = task.departed_s
            + route
                .as_ref()
                .map(|r| r.seconds(task.speed_mps))
                .unwrap_or(distance(current, base) / task.speed_mps)
                .ceil()
                .max(1.0) as i64;
        task.route = route;
        task.status = status;
        task.refill_done_s =
            Some(task.arrival_s + if task.resource == "engines" { 150 } else { 90 });
        task.policy_version = if self.scn.operations.local_autonomy {
            self.scn.tactical_policy.version.clone()
        } else {
            "fixed-post-v1".into()
        };
        task.policy_reason = if status == TaskStatus::Withdrawing {
            "Safety threshold exceeded; returning by a physical safe route"
        } else {
            "Consumables exhausted; returning physically for replenishment"
        }
        .into();
        task.detail = if status == TaskStatus::Withdrawing {
            "Withdrawing for safety"
        } else {
            "Returning to staging for replenishment"
        }
        .into();
    }
    /// Check only the untravelled route. Geometry behind a unit cannot strand it.
    /// Called each simulated second; path search happens only after a new blockage.
    fn check_task_route(&mut self, i: usize) -> bool {
        let t = self.time_s();
        let task = self.d.tasks[i].clone();
        if !self.scn.operations.road_routing
            || task.resource == "air_tankers"
            || !matches!(
                task.status,
                TaskStatus::EnRoute | TaskStatus::Refilling | TaskStatus::Withdrawing
            )
            || t > task.arrival_s
        {
            return false;
        }
        let Some(route) = &task.route else {
            return false;
        };
        // A closure is acted on before earning this second of movement.
        let checked_at = (t - 1).max(task.departed_s);
        let mut elapsed = (checked_at - task.departed_s).max(0) as f32;
        let mut remaining = Route::default();
        for leg in &route.legs {
            let duration = leg.length_m / leg.speed_mps.min(task.speed_mps).max(0.01);
            if elapsed >= duration {
                elapsed -= duration;
                continue;
            }
            let mut leg = leg.clone();
            leg.from = interpolate(leg.from, leg.to, elapsed / duration.max(0.001));
            leg.length_m = distance(leg.from, leg.to);
            remaining.legs.push(leg);
            elapsed = 0.0;
        }
        if self.network.route_open(&remaining, &self.d.roads, false) {
            return false;
        }
        let current = task.position(checked_at);
        self.d.tasks[i].route_checked_s = t;
        if let Some(route) = self.operational_route(
            current,
            task.target,
            &self.d.roads,
            false,
            task.resource == "ground_crews",
            task.speed_mps,
        ) {
            let q = &mut self.d.tasks[i];
            let refill = q.refill_done_s.map(|done| done - q.arrival_s);
            q.origin = current;
            q.departed_s = t.max(task.departed_s);
            q.arrival_s = q.departed_s + route.seconds(q.speed_mps).ceil().max(1.0) as i64;
            q.refill_done_s = refill.map(|duration| q.arrival_s + duration);
            q.route = Some(route);
            q.regroup_until_s = (q.departed_s > t).then_some(q.departed_s);
            q.policy_reason = "Untravelled road cut by fire; rerouted from current position".into();
            q.detail = q.policy_reason.clone();
            self.event(
                "deployment_rerouted",
                format!(
                    "Task #{} [{}]: return/mission route changed around fire",
                    task.id, task.policy_version
                ),
                self.scn.zone_of(current),
            );
        } else {
            let q = &mut self.d.tasks[i];
            q.origin = current;
            q.target = current;
            q.route = None;
            q.departed_s = t;
            q.arrival_s = t;
            q.refill_done_s = None;
            q.policy_reason = "No safe remaining route; stopped at last reached position".into();
            self.end_task(
                i,
                TaskStatus::Failed,
                format!(
                    "No safe remaining route [{}]: route cut by fire; unit stranded",
                    task.policy_version
                ),
            );
        }
        true
    }
    pub(crate) fn step_tasks(&mut self) {
        let t = self.time_s();
        self.step_local_policies();
        for i in 0..self.d.tasks.len() {
            let task = self.d.tasks[i].clone();
            let qty = self.task_active_qty(&task);
            if qty == 0 {
                continue;
            }
            if self.check_task_route(i) {
                continue;
            }
            if matches!(task.status, TaskStatus::Refilling | TaskStatus::Withdrawing) {
                if t < task.refill_done_s.unwrap_or(task.arrival_s) {
                    continue;
                }
                if task.status == TaskStatus::Withdrawing {
                    if self.scn.operations.local_autonomy
                        && self.scn.tactical_policy.resume_after_withdrawal
                        && matches!(
                            task.command,
                            Command::DeployEngine { .. } | Command::CutFireLine { .. }
                        )
                    {
                        self.d.tasks[i].status = TaskStatus::Suspended;
                        self.d.tasks[i].last_policy_s = t;
                        self.d.tasks[i].policy_reason =
                            "At staging; mission suspended pending safe reachable work".into();
                        self.d.tasks[i].detail = self.d.tasks[i].policy_reason.clone();
                        self.event(
                            "deployment_suspended",
                            format!(
                                "Task #{}: safety withdrawal complete; allocation retained",
                                task.id
                            ),
                            None,
                        );
                    } else {
                        self.end_task(i, TaskStatus::Failed, "Safety withdrawal complete".into());
                    }
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
                    let mut route = self.operational_route(
                        origin,
                        at,
                        &self.d.roads,
                        false,
                        false,
                        task.speed_mps,
                    );
                    if let Some(r) = route.as_mut() {
                        if r.legs.last().is_some_and(|l| {
                            l.road.is_none() && l.length_m > self.engine_hose_reach_m()
                        }) {
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
                    let limit = if self.scn.operations.local_autonomy {
                        self.scn.tactical_policy.work_danger
                    } else {
                        0.35
                    };
                    if self.d.hazard.at(work_position).danger >= limit {
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
            let limit = if self.scn.operations.local_autonomy {
                self.scn.tactical_policy.work_danger
            } else {
                0.35
            };
            if self.d.hazard.at(task.position(t)).danger >= limit {
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
                            (self.fire.added_moisture()[c]
                                < if self.scn.operations.local_autonomy {
                                    self.scn.tactical_policy.wet_limit_pts
                                } else {
                                    30.0
                                }
                                && distance(p, task.target)
                                    <= if self.scn.operations.local_autonomy {
                                        self.scn.tactical_policy.hose_reach_m
                                    } else {
                                        60.0
                                    }
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
                    let budget = (400.0 / 60.0 * elapsed).min(task.water_l);
                    let radius = self.scn.grid.cellsize * 0.45;
                    let area = std::f32::consts::PI * radius.powi(2);
                    let per_cell = budget / candidates.len() as f32;
                    let mut litres = 0.0;
                    for p in candidates {
                        let available = if self.scn.operations.local_autonomy {
                            let cell = self.scn.cell_of(p).unwrap();
                            (self.scn.tactical_policy.wet_limit_pts
                                - self.fire.added_moisture()[self.scn.grid.index(cell)])
                            .max(0.0)
                                / fire::MOISTURE_POINTS_PER_LITRE
                                * area
                        } else {
                            per_cell
                        };
                        let delivered = per_cell.min(available);
                        self.fire.water_drop(p, radius, delivered / area);
                        litres += delivered;
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
