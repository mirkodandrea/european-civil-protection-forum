//! Aggregate evacuation vehicles with directed-link FIFO, storage and discharge.
//! No individual civilian ABM: a vehicle carries a packet of building occupants.
use crate::{
    network::{interpolate, Route},
    World,
};
use scenario::Pos;
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default, Serialize)]
pub struct Traffic {
    pub vehicles: Vec<Vehicle>,
    pub links: BTreeMap<usize, LinkQueue>,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkQueue {
    pub queue: Vec<usize>,
    pub next_release_s: f32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Vehicle {
    pub id: usize,
    pub building: usize,
    pub people: u32,
    pub ready_s: i64,
    pub destination: Option<u32>,
    pub target: Option<Pos>,
    pub route: Option<Route>,
    pub leg: usize,
    pub entered_s: i64,
    pub departed: bool,
    pub completed: bool,
    pub waiting: bool,
    pub at_risk: bool,
    /// Bound for its borgo's gathering area (no explicit destination): may
    /// use any gathering area, opened or not, when its own is out of reach.
    #[serde(default)]
    pub gathering: bool,
    pub detail: String,
}
impl Vehicle {
    pub fn position(&self, t: i64, origin: Pos) -> Pos {
        let Some(route) = &self.route else {
            return origin;
        };
        let Some(leg) = route.legs.get(self.leg) else {
            return self.target.unwrap_or(origin);
        };
        if self.waiting {
            return leg.from;
        }
        interpolate(
            leg.from,
            leg.to,
            ((t - self.entered_s) as f32 * leg.speed_mps / leg.length_m.max(0.01)).clamp(0.0, 1.0),
        )
    }
}
impl Traffic {
    /// Snapshot FIFO delay at dispatch: bake it into the same leg duration used
    /// by both ETA and position. Does not reserve priority over civilian traffic.
    pub fn delay_route(&self, route: &mut Route, speed: f32) {
        for leg in &mut route.legs {
            let queued = leg
                .link
                .and_then(|id| self.links.get(&id))
                .map_or(0, |q| q.queue.len());
            if queued == 0 || leg.length_m <= 0.01 {
                continue;
            }
            let base_s = leg.length_m / leg.speed_mps.min(speed).max(0.01);
            let delay_s = queued as f32 / leg.flow_per_s.max(0.01);
            leg.speed_mps = leg.length_m / (base_s + delay_s);
        }
    }
}
impl World {
    pub(crate) fn schedule_traffic(
        &mut self,
        building: usize,
        people: u32,
        ready_s: i64,
        destination: Option<u32>,
    ) {
        let seats = if self.scn.buildings[building].kind.needs_transport() {
            crate::BUS_SEATS
        } else {
            4
        };
        let mut left = people;
        while left > 0 {
            let n = left.min(seats);
            let id = self.d.traffic.vehicles.len();
            self.d.traffic.vehicles.push(Vehicle {
                id,
                building,
                people: n,
                ready_s,
                destination,
                target: None,
                route: None,
                leg: 0,
                entered_s: ready_s,
                departed: false,
                completed: false,
                waiting: false,
                at_risk: false,
                gathering: false,
                detail: "Preparing to leave".into(),
            });
            left -= n;
        }
    }
    fn evacuation_route(&self, v: &Vehicle) -> Option<(Route, Pos, Option<u32>)> {
        let origin = v.position(self.time_s(), self.scn.buildings[v.building].pos);
        let mut targets = Vec::new();
        // Own gathering area first, if it is safe and reachable.
        if v.gathering {
            if let Some(own) = v.destination.and_then(|id| self.scn.buildings.iter().find(|b| b.id == id)) {
                if self.d.hazard.at(own.pos).danger < 0.55 {
                    if let Some(mut r) = self.network.route(origin, own.pos, &self.d.roads, true, false) {
                        for leg in &mut r.legs {
                            leg.speed_mps = leg.speed_mps.min(20.0);
                        }
                        return Some((r, own.pos, Some(own.id)));
                    }
                }
            }
        }
        for b in &self.scn.buildings {
            let usable = self.d.shelters.contains_key(&b.id) || (v.gathering && self.is_gathering(b.id));
            if !usable || (!v.gathering && v.destination.is_some_and(|id| id != b.id)) {
                continue;
            }
            let reserved: u32 = self
                .d
                .traffic
                .vehicles
                .iter()
                .filter(|x| !x.completed && x.destination == Some(b.id) && x.id != v.id)
                .map(|x| x.people)
                .sum();
            if self.d.shelters.get(&b.id).copied().unwrap_or(0) + reserved + v.people <= b.shelter_capacity
                && self.d.hazard.at(b.pos).danger < 0.55
            {
                targets.push((b.pos, Some(b.id)));
            }
        }
        if v.destination.is_none() || v.gathering {
            let w = self.scn.grid.width_m();
            let h = self.scn.grid.height_m();
            for road in &self.scn.roads {
                if road.drivable {
                    for &p in &road.points {
                        if p[0] <= 1.0 || p[1] <= 1.0 || p[0] >= w - 1.0 || p[1] >= h - 1.0 {
                            targets.push((p, None));
                        }
                    }
                }
            }
        }
        targets
            .into_iter()
            .filter(|(p, _)| self.d.hazard.at(*p).danger < 0.55)
            .filter_map(|(p, d)| {
                self.network
                    .route(origin, p, &self.d.roads, true, false)
                    .map(|mut r| {
                        // The speed cap used for destination ETA must also govern
                        // the physical movement clock on motorway-class links.
                        for leg in &mut r.legs {
                            leg.speed_mps = leg.speed_mps.min(20.0);
                        }
                        (r, p, d)
                    })
            })
            .min_by(|(a, _, _), (b, _, _)| a.seconds(20.0).total_cmp(&b.seconds(20.0)))
    }
    pub(crate) fn step_traffic(&mut self) {
        if !self.scn.operations.traffic {
            return;
        }
        let t = self.time_s();
        for i in 0..self.d.traffic.vehicles.len() {
            let mut v = self.d.traffic.vehicles[i].clone();
            if v.completed || t < v.ready_s {
                continue;
            }
            if v.route.is_none() {
                // Retry on a fixed cadence, not at every physics tick.
                if v.waiting && (t - v.ready_s) % 60 != 0 {
                    continue;
                }
                let Some((r, p, dest)) = self.evacuation_route(&v) else {
                    v.waiting = true;
                    v.detail = "Waiting: no reachable safe destination with capacity".into();
                    self.d.traffic.vehicles[i] = v;
                    continue;
                };
                v.route = Some(r);
                v.target = Some(p);
                v.destination = dest;
                v.leg = 0;
                v.entered_s = t;
                v.waiting = true;
            }
            let route = v.route.as_ref().unwrap();
            let leg = route.legs.get(v.leg).cloned();
            let Some(leg) = leg else {
                self.complete_vehicle(&mut v);
                self.d.traffic.vehicles[i] = v;
                continue;
            };
            if v.waiting {
                if !self.network.leg_open(&leg, &self.d.roads, true) {
                    v.route = None;
                    v.ready_s = t;
                    v.detail = "Rerouting around closed or unsafe road".into();
                    self.d.traffic.vehicles[i] = v;
                    continue;
                }
                let room = leg.link.is_none_or(|id| {
                    self.d
                        .traffic
                        .links
                        .get(&id)
                        .map(|q| q.queue.len() < leg.storage as usize)
                        .unwrap_or(true)
                });
                if !room {
                    v.detail = "Queue: downstream link is full".into();
                    self.d.traffic.vehicles[i] = v;
                    continue;
                }
                if let Some(id) = leg.link {
                    self.d.traffic.links.entry(id).or_default().queue.push(i);
                }
                if !v.departed {
                    self.d.buildings[v.building].people_inside = self.d.buildings[v.building]
                        .people_inside
                        .saturating_sub(v.people);
                    v.departed = true;
                }
                v.entered_s = t;
                v.waiting = false;
                v.detail = "Travelling to safety".into();
                self.d.traffic.vehicles[i] = v;
                continue;
            }
            let pos = self.traffic_position(&v, t);
            if self.d.hazard.at(pos).danger >= 0.55 {
                if !v.at_risk {
                    v.at_risk = true;
                    self.d.buildings[v.building].at_risk += v.people;
                    self.event(
                        "evacuation_route_unsafe",
                        format!("{} evacuees exposed on route", v.people),
                        self.scn.zone_of(pos),
                    );
                }
                v.detail = "Stopped: route exposed to fire".into();
                self.d.traffic.vehicles[i] = v;
                continue;
            }
            if ((t - v.entered_s) as f32) < leg.length_m / leg.speed_mps {
                self.d.traffic.vehicles[i] = v;
                continue;
            }
            let next = route.legs.get(v.leg + 1);
            let next_room = next.is_none_or(|l| {
                l.link.is_none_or(|id| {
                    self.d
                        .traffic
                        .links
                        .get(&id)
                        .map(|q| q.queue.len() < l.storage as usize)
                        .unwrap_or(true)
                })
            });
            let next_open = next.is_none_or(|l| self.network.leg_open(l, &self.d.roads, true));
            let release = leg.link.is_none_or(|id| {
                let q = self.d.traffic.links.entry(id).or_default();
                q.queue.first() == Some(&i) && t as f32 >= q.next_release_s
            });
            if !next_open {
                // A blocked next link permits a new route from this junction;
                // keeping the current FIFO reservation until a route exists prevents teleporting.
                if t % 60 == 0 {
                    let mut copy = v.clone();
                    copy.route = None;
                    if let Some((r, p, d)) = self.evacuation_route_from(&copy, leg.to) {
                        if let Some(id) = leg.link {
                            self.d
                                .traffic
                                .links
                                .get_mut(&id)
                                .unwrap()
                                .queue
                                .retain(|x| *x != i);
                        }
                        v.route = Some(r);
                        v.target = Some(p);
                        v.destination = d;
                        v.leg = 0;
                        v.waiting = true;
                    }
                }
                v.detail = "Waiting for reroute".into();
                self.d.traffic.vehicles[i] = v;
                continue;
            }
            if !release || !next_room {
                v.detail = "Queue at bottleneck or downstream storage".into();
                self.d.traffic.vehicles[i] = v;
                continue;
            }
            if let Some(id) = leg.link {
                let q = self.d.traffic.links.get_mut(&id).unwrap();
                q.queue.remove(0);
                q.next_release_s = t as f32 + 1.0 / leg.flow_per_s;
            }
            v.leg += 1;
            v.waiting = true;
            v.entered_s = t;
            if next.is_none() {
                self.complete_vehicle(&mut v);
            }
            self.d.traffic.vehicles[i] = v;
        }
    }
    fn evacuation_route_from(&self, v: &Vehicle, origin: Pos) -> Option<(Route, Pos, Option<u32>)> {
        let target = v.target?;
        self.network
            .route(origin, target, &self.d.roads, true, false)
            .map(|mut r| {
                for leg in &mut r.legs {
                    leg.speed_mps = leg.speed_mps.min(20.0);
                }
                (r, target, v.destination)
            })
    }
    fn complete_vehicle(&mut self, v: &mut Vehicle) {
        v.completed = true;
        v.waiting = false;
        v.detail = "Arrived at safety".into();
        let b = &mut self.d.buildings[v.building];
        b.evacuated += v.people;
        b.evac_pending = b.evac_pending.saturating_sub(v.people);
        if b.evac_pending == 0 {
            b.evac_done_at = None;
        }
        if let Some(dest) = v.destination {
            if let Some(n) = self.d.shelters.get_mut(&dest) {
                *n += v.people;
            } else if self.is_gathering(dest) {
                self.d.shelters.insert(dest, v.people);
            }
        }
        self.event(
            "evacuees_arrived",
            format!(
                "{} people from building #{} arrived at safety",
                v.people, self.scn.buildings[v.building].id
            ),
            self.scn
                .zone_of(v.target.unwrap_or(self.scn.buildings[v.building].pos)),
        );
    }
    pub fn traffic_position(&self, v: &Vehicle, t: i64) -> Pos {
        let origin = self.scn.buildings[v.building].pos;
        let Some(route) = &v.route else { return origin };
        let Some(leg) = route.legs.get(v.leg) else {
            return v.target.unwrap_or(origin);
        };
        if v.waiting {
            return leg.from;
        }
        let rank = leg
            .link
            .and_then(|id| self.d.traffic.links.get(&id))
            .and_then(|q| q.queue.iter().position(|id| *id == v.id))
            .unwrap_or(0);
        let along = ((t - v.entered_s).max(0) as f32 * leg.speed_mps)
            .min((leg.length_m - rank as f32 * 7.5).max(0.0));
        interpolate(leg.from, leg.to, along / leg.length_m.max(0.01))
    }
    pub fn traffic_view(&self) -> serde_json::Value {
        serde_json::json!({"vehicles":self.d.traffic.vehicles.iter().map(|v|serde_json::json!({"vehicle":v,"position_m":self.traffic_position(v,self.time_s())})).collect::<Vec<_>>(),"links":self.d.traffic.links,"in_transit_people":self.d.traffic.vehicles.iter().filter(|v|v.departed&&!v.completed).map(|v|v.people).sum::<u32>()})
    }
}
