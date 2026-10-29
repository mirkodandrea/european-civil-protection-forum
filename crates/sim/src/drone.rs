//! Monitoring drones: a role resource that, once on station, samples the world
//! over its zones every update interval of simulated time. The samples are
//! simulation state (they live in [`crate::Dyn`], so a rollback drops them);
//! what a role learns from them is the exercise layer's business.
use crate::travel::TaskStatus;
use crate::{Command, World};
use scenario::DroneSpec;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    /// First observation, on arrival over the area.
    Start,
    /// Periodic observation while on station.
    Update,
    /// Coverage over: endurance exhausted, wind, or recalled.
    End,
}

/// The world as the drone saw it at `t_s`.
#[derive(Debug, Clone)]
pub struct MonitorSample {
    pub task: u64,
    pub role: String,
    pub kind: SampleKind,
    pub t_s: i64,
    pub zones: Vec<String>,
    /// [`World::view`] at `t_s` (empty for [`SampleKind::End`]).
    pub view: Value,
    /// Why coverage ended (End only).
    pub reason: Option<String>,
    /// When the drone can fly again (End only).
    pub available_s: Option<i64>,
    /// A ground reconnaissance report ([`Command::Reconnoitre`]), not a drone.
    pub recon: bool,
}

/// Schedule of a drone on station, keyed by task id.
#[derive(Debug, Clone)]
pub struct MonitorState {
    pub zones: Vec<String>,
    pub started_s: i64,
    pub next_s: i64,
    pub end_s: i64,
}

impl World {
    /// Resources that fly: air tankers and drones follow no road.
    pub fn flies(&self, resource: &str) -> bool {
        resource == "air_tankers" || self.scn.drone_spec(resource).is_some()
    }

    /// Zones named by a monitoring order: codes, or rectangles `F3-G4`.
    /// Sorted row by row, without duplicates.
    pub fn parse_monitor_zones(&self, entries: &[String]) -> Result<Vec<String>, String> {
        let z = &self.scn.zones;
        let mut set = BTreeSet::new();
        for e in entries {
            let parts: Vec<&str> = e.split(['-', '–', '—']).collect();
            let corner = |s: &str| {
                z.parse(s)
                    .or_else(|| self.place_zone(s).and_then(|c| z.parse(&c)))
                    .ok_or_else(|| format!("unknown zone \"{}\"", s.trim()))
            };
            match parts.as_slice() {
                [one] => {
                    let (c, r) = corner(one)?;
                    set.insert((r, c));
                }
                [a, b] => {
                    let ((c0, r0), (c1, r1)) = (corner(a)?, corner(b)?);
                    for r in r0.min(r1)..=r0.max(r1) {
                        for c in c0.min(c1)..=c0.max(c1) {
                            set.insert((r, c));
                        }
                    }
                }
                _ => return Err(format!("cannot read zone \"{e}\"")),
            }
        }
        if set.is_empty() {
            return Err("name at least one zone to monitor".into());
        }
        Ok(set.into_iter().map(|(r, c)| z.name(c, r)).collect())
    }

    /// Zones of an order, checked against the drone's coverage limit.
    pub(crate) fn checked_monitor_zones(
        &self,
        entries: &[String],
        spec: &DroneSpec,
    ) -> Result<Vec<String>, String> {
        let zones = self.parse_monitor_zones(entries)?;
        let cells: Vec<(usize, usize)> = zones
            .iter()
            .filter_map(|n| self.scn.zones.parse(n))
            .collect();
        let span = |f: fn(&(usize, usize)) -> usize| {
            let (lo, hi) = (
                cells.iter().map(f).min().unwrap_or(0),
                cells.iter().map(f).max().unwrap_or(0),
            );
            hi - lo + 1
        };
        let area = span(|c| c.0) * span(|c| c.1);
        if area > spec.max_zones as usize {
            return Err(format!(
                "the drone covers at most {} zones; the area ordered spans {area}",
                spec.max_zones
            ));
        }
        Ok(zones)
    }

    /// The role whose drone EXCON tasks: the named one, else the first that owns a drone.
    pub(crate) fn drone_owner(&self, named: Option<&str>) -> Result<String, String> {
        let owns =
            |r: &scenario::Role| r.resources.keys().any(|k| self.scn.drone_spec(k).is_some());
        match named {
            Some(id) => match self.scn.role(id) {
                Some(r) if owns(r) => Ok(r.id.clone()),
                Some(r) => Err(format!("{} has no drone", r.name)),
                None => Err(format!("unknown role {id}")),
            },
            None => self
                .scn
                .roles
                .iter()
                .find(|r| owns(r))
                .map(|r| r.id.clone())
                .ok_or_else(|| "no role owns a drone".into()),
        }
    }

    /// The drone resource a role owns, with its parameters.
    pub(crate) fn drone_of(&self, role: &scenario::Role) -> Option<(String, DroneSpec)> {
        role.resources
            .keys()
            .find_map(|k| self.scn.drone_spec(k).map(|s| (k.clone(), s)))
    }

    /// Centre of the area an order covers.
    pub(crate) fn monitor_centre(&self, entries: &[String]) -> Option<scenario::Pos> {
        let zones = self.parse_monitor_zones(entries).ok()?;
        let centres: Vec<_> = zones
            .iter()
            .filter_map(|z| self.scn.zone_centre(z))
            .collect();
        (!centres.is_empty()).then(|| {
            let n = centres.len() as f32;
            [
                centres.iter().map(|c| c[0]).sum::<f32>() / n,
                centres.iter().map(|c| c[1]).sum::<f32>() / n,
            ]
        })
    }

    fn wind_grounds(&self, spec: &DroneSpec) -> Option<f32> {
        let wind = self.fire.weather().wind_speed_kmh as f32;
        spec.wind_limit_kmh
            .filter(|limit| wind > *limit)
            .map(|_| wind)
    }

    /// Wind limit check for a launch.
    pub(crate) fn check_drone_wind(&self, spec: &DroneSpec) -> Result<(), String> {
        match self.wind_grounds(spec) {
            Some(w) => Err(format!(
                "the drone stays grounded: wind {w:.0} km/h exceeds its limit of {:.0} km/h",
                spec.wind_limit_kmh.unwrap_or_default()
            )),
            None => Ok(()),
        }
    }

    fn push_sample(
        &mut self,
        i: usize,
        kind: SampleKind,
        reason: Option<String>,
        available_s: Option<i64>,
    ) {
        let t = self.time_s();
        let task = &self.d.tasks[i];
        let zones = match self.d.monitors.get(&task.id) {
            Some(m) => m.zones.clone(),
            None => match &task.command {
                Command::MonitorArea { zones, .. } => {
                    self.parse_monitor_zones(zones).unwrap_or_default()
                }
                _ => Vec::new(),
            },
        };
        let (task_id, role) = (task.id, task.role.clone());
        let view = if kind == SampleKind::End {
            Value::Null
        } else {
            self.view()
        };
        self.d.monitor_samples.push(MonitorSample {
            task: task_id,
            role,
            kind,
            t_s: t,
            zones,
            view,
            reason,
            available_s,
            recon: false,
        });
    }

    /// The zone a reconnaissance order names: a code (`"F4"`) or a named place.
    pub(crate) fn recon_zone(&self, entry: &str) -> Result<String, String> {
        self.zone_code(entry)
            .or_else(|| self.place_zone(entry))
            .ok_or_else(|| format!("unknown zone \"{}\" for a reconnaissance", entry.trim()))
    }

    /// A ground crew reports once what it sees over `zone` now, for `role`.
    pub(crate) fn record_reconnaissance(&mut self, role: &str, zone: &str) {
        let view = self.view();
        self.d.monitor_samples.push(MonitorSample {
            task: 0,
            role: role.into(),
            kind: SampleKind::Start,
            t_s: self.time_s(),
            zones: vec![zone.into()],
            view,
            reason: None,
            available_s: None,
            recon: true,
        });
    }

    /// Samples taken since the last call, oldest first.
    pub fn take_monitor_samples(&mut self) -> Vec<MonitorSample> {
        std::mem::take(&mut self.d.monitor_samples)
    }

    /// Time of the next scheduled sample of any drone on station.
    pub fn next_monitor_s(&self) -> Option<i64> {
        self.d.monitors.values().map(|m| m.next_s).min()
    }

    /// Hand the drone's allocation back: it can be ordered again.
    fn release_drone(&mut self, i: usize) {
        let t = self.time_s();
        let ids = self.d.tasks[i].allocations.clone();
        for id in ids {
            let Some(a) = self.d.allocations.iter_mut().find(|a| a.id == id) else {
                continue;
            };
            let qty = a.qty.saturating_sub(a.returned);
            if qty == 0 {
                continue;
            }
            a.returned += qty;
            a.last_return_at_s = Some(t);
            let (role, resource) = (a.role.clone(), a.resource.clone());
            if let Some(p) = self
                .d
                .pools
                .get_mut(&role)
                .and_then(|p| p.get_mut(&resource))
            {
                p.used = p.used.saturating_sub(qty);
            }
            self.resource_event("returned", Some(id), &role, &resource, qty as i64);
        }
    }

    /// Coverage ends: sample for the last time, fly back to the staging base.
    fn end_coverage(&mut self, i: usize, reason: &str) {
        let t = self.time_s();
        let (id, role, resource, speed) = {
            let q = &self.d.tasks[i];
            (q.id, q.role.clone(), q.resource.clone(), q.speed_mps)
        };
        let at = self.d.tasks[i].position(t);
        let base = self.d.travel_settings[&role][&resource].origin;
        let spec = self.scn.drone_spec(&resource).unwrap_or_default();
        let arrival = t + (crate::network::distance(at, base) / speed).ceil().max(1.0) as i64;
        let available = arrival + spec.turnaround_s;
        self.push_sample(i, SampleKind::End, Some(reason.into()), Some(available));
        self.d.monitors.remove(&id);
        let q = &mut self.d.tasks[i];
        q.origin = at;
        q.target = base;
        q.route = None;
        q.departed_s = t;
        q.arrival_s = arrival;
        q.status = TaskStatus::Refilling;
        q.refill_done_s = Some(available);
        q.policy_reason = format!("Coverage ended ({reason}); returning to base to recharge");
        q.detail = q.policy_reason.clone();
        let text = format!("Task #{id}: drone off station ({reason}); back at base in {} s, available again at t+{available} s", arrival - t);
        self.event("drone_returning", text, None);
    }

    /// EXCON recalled the drone through [`Command::ReturnResource`]: coverage stops at once.
    pub(crate) fn drone_recalled(&mut self, allocation: u64) {
        let Some(i) = self.d.tasks.iter().position(|t| {
            t.allocations.contains(&allocation) && matches!(t.command, Command::MonitorArea { .. })
        }) else {
            return;
        };
        if self.d.monitors.contains_key(&self.d.tasks[i].id) {
            self.push_sample(i, SampleKind::End, Some("recalled by EXCON".into()), None);
            let id = self.d.tasks[i].id;
            self.d.monitors.remove(&id);
        }
    }

    /// One simulated second of a drone task (called from the task loop).
    pub(crate) fn step_drone(&mut self, i: usize) {
        let t = self.time_s();
        let task = self.d.tasks[i].clone();
        let Some(spec) = self.scn.drone_spec(&task.resource) else {
            return;
        };
        match task.status {
            TaskStatus::EnRoute => {
                if self.wind_grounds(&spec).is_some() {
                    self.end_coverage(i, "wind above the drone limit");
                    return;
                }
                if t < task.arrival_s {
                    return;
                }
                let Command::MonitorArea { zones, .. } = &task.command else {
                    return;
                };
                let zones = self.parse_monitor_zones(zones).unwrap_or_default();
                let q = &mut self.d.tasks[i];
                q.arrived_s = Some(t);
                q.status = TaskStatus::Working;
                q.detail = format!(
                    "On station over {}; map updates every {} s",
                    zones.join(", "),
                    spec.update_interval_s
                );
                let detail = q.detail.clone();
                self.d.monitors.insert(
                    task.id,
                    MonitorState {
                        zones,
                        started_s: t,
                        next_s: t + spec.update_interval_s,
                        end_s: t + spec.endurance_s,
                    },
                );
                self.push_sample(i, SampleKind::Start, None, None);
                self.event(
                    "deployment_arrived",
                    format!("{} task #{} arrived: {detail}", task.role, task.id),
                    self.scn.zone_of(task.target),
                );
            }
            TaskStatus::Working => {
                let Some(m) = self.d.monitors.get(&task.id).cloned() else {
                    return;
                };
                if self.wind_grounds(&spec).is_some() {
                    self.end_coverage(i, "wind above the drone limit");
                    return;
                }
                if t >= m.next_s && t <= m.end_s {
                    self.push_sample(i, SampleKind::Update, None, None);
                    if let Some(m) = self.d.monitors.get_mut(&task.id) {
                        while m.next_s <= t {
                            m.next_s += spec.update_interval_s;
                        }
                    }
                }
                if t >= m.end_s {
                    self.end_coverage(i, "endurance exhausted");
                }
            }
            TaskStatus::Refilling => {
                if t < task.refill_done_s.unwrap_or(task.arrival_s) {
                    return;
                }
                self.release_drone(i);
                self.end_task(
                    i,
                    TaskStatus::Completed,
                    "Drone landed and recharged; available again".into(),
                );
            }
            _ => {}
        }
    }

    /// Drones of `role` that are flying or recharging, for its Function screen and map.
    pub fn drone_status(&self, role: &str) -> Vec<Value> {
        let t = self.time_s();
        self.d
            .tasks
            .iter()
            .filter(|k| k.role == role && matches!(k.command, Command::MonitorArea { .. }))
            .filter(|k| matches!(k.status, TaskStatus::EnRoute | TaskStatus::Working | TaskStatus::Refilling))
            .filter(|k| self.task_active_qty(k) > 0)
            .map(|k| {
                let m = self.d.monitors.get(&k.id);
                let zones = match (&k.command, m) {
                    (_, Some(m)) => m.zones.clone(),
                    (Command::MonitorArea { zones, .. }, None) => self.parse_monitor_zones(zones).unwrap_or_default(),
                    _ => Vec::new(),
                };
                let status = match k.status {
                    TaskStatus::EnRoute => "en_route",
                    TaskStatus::Working => "on_station",
                    _ => "returning",
                };
                json!({"task":k.id,"status":status,"zones":zones,"position_m":k.position(t),
                    "started_s":m.map(|m| m.started_s),"ends_s":m.map(|m| m.end_s),
                    "next_update_s":m.map(|m| m.next_s),
                    "arrival_s":(k.status==TaskStatus::EnRoute).then_some(k.arrival_s),
                    "available_s":(k.status==TaskStatus::Refilling).then_some(k.refill_done_s).flatten()})
            })
            .collect()
    }
}
