//! Metric road graph and deterministic shortest paths, adapted from propagator_abm.
//! Shared vertices are junctions; geometric crossings without a vertex are bridges.
use crate::RoadState;
use scenario::{Pos, Scenario};
use serde::Serialize;
use std::collections::{BTreeMap, BinaryHeap};

pub fn distance(a: Pos, b: Pos) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
pub fn interpolate(a: Pos, b: Pos, t: f32) -> Pos {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
#[derive(Debug, Clone, Serialize)]
pub struct Leg {
    pub from: Pos,
    pub to: Pos,
    pub road: Option<String>,
    pub link: Option<usize>,
    pub length_m: f32,
    pub speed_mps: f32,
    pub storage: u32,
    pub flow_per_s: f32,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Route {
    pub legs: Vec<Leg>,
}
impl Route {
    pub fn length_m(&self) -> f32 {
        self.legs.iter().map(|l| l.length_m).sum()
    }
    pub fn seconds(&self, speed: f32) -> f32 {
        self.legs
            .iter()
            .map(|l| l.length_m / l.speed_mps.min(speed))
            .sum()
    }
    pub fn position(&self, elapsed: f32, speed: f32) -> Pos {
        let mut left = elapsed.max(0.0);
        for leg in &self.legs {
            let duration = leg.length_m / leg.speed_mps.min(speed);
            if left < duration {
                return interpolate(leg.from, leg.to, left / duration.max(0.001));
            }
            left -= duration;
        }
        self.legs.last().map(|l| l.to).unwrap_or([0.0, 0.0])
    }
    pub fn points(&self) -> Vec<Pos> {
        self.legs
            .first()
            .map(|l| l.from)
            .into_iter()
            .chain(self.legs.iter().map(|l| l.to))
            .collect()
    }
}
#[derive(Debug, Clone)]
struct Edge {
    a: usize,
    b: usize,
    oneway: bool,
    road: String,
    drivable: bool,
    speed: f32,
    lanes: u32,
    flow: f32,
    travel_factor: f32,
}
/// Stable link geometry; closure state lives in Dyn and is restored on rollback.
#[derive(Debug, Clone, Serialize)]
pub struct RoadSegment {
    pub road: String,
    pub links: [usize; 2],
    pub from: Pos,
    pub to: Pos,
}
#[derive(Debug, Default, Clone)]
pub struct Network {
    nodes: Vec<Pos>,
    edges: Vec<Edge>,
    adj: Vec<Vec<(usize, usize)>>,
}
#[derive(Clone, Copy, PartialEq)]
struct Visit(f32, usize);
impl Eq for Visit {}
impl Ord for Visit {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}
impl PartialOrd for Visit {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Network {
    pub fn build(s: &Scenario) -> Self {
        let mut net = Self::default();
        let mut ids = BTreeMap::new();
        for road in &s.roads {
            let (speed, lanes, flow) = match road.class.as_str() {
                "motorway" => (27.8, 2, 1900.0),
                "primary" => (16.7, 1, 1600.0),
                "secondary" => (13.9, 1, 1400.0),
                "service" => (5.6, 1, 400.0),
                "track" | "path" => (3.0, 1, 100.0),
                _ => (8.3, 1, 800.0),
            };
            for (segment, pair) in road.points.windows(2).enumerate() {
                if distance(pair[0], pair[1]) < 0.05 {
                    continue;
                }
                let mut ends = [0; 2];
                for k in 0..2 {
                    let p = pair[k];
                    let key = ((p[0] * 20.0).round() as i64, (p[1] * 20.0).round() as i64);
                    ends[k] = *ids.entry(key).or_insert_with(|| {
                        let n = net.nodes.len();
                        net.nodes.push(p);
                        net.adj.push(vec![]);
                        n
                    });
                }
                let id = net.edges.len();
                let [a, b] = ends;
                net.edges.push(Edge {
                    a,
                    b,
                    oneway: road.oneway,
                    road: road.id.clone(),
                    drivable: road.drivable,
                    speed,
                    lanes,
                    flow: flow / 3600.0,
                    travel_factor: if s.operations.terrain_travel {
                        let elevation = |p| s.cell_of(p).map(|c| s.elevation_at(c)).unwrap_or(0.0);
                        let grade = (elevation(pair[1]) - elevation(pair[0])).abs()
                            / distance(pair[0], pair[1]);
                        // Scenario assumptions, deliberately conservative; not validated vehicle physics.
                        let grade_factor = 1.0 / (1.0 + 4.0 * grade.min(0.5));
                        let curve_factor = if segment > 0 {
                            let prev = road.points[segment - 1];
                            let u = [pair[0][0] - prev[0], pair[0][1] - prev[1]];
                            let v = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
                            let cosine = ((u[0] * v[0] + u[1] * v[1])
                                / (distance(prev, pair[0]) * distance(pair[0], pair[1]))
                                    .max(0.001))
                            .clamp(-1.0, 1.0);
                            1.0 - 0.45 * (cosine.acos() / std::f32::consts::PI)
                        } else {
                            1.0
                        };
                        grade_factor * curve_factor
                    } else {
                        1.0
                    },
                });
                net.adj[a].push((b, id));
                if !road.oneway {
                    net.adj[b].push((a, id));
                }
            }
        }
        net
    }
    fn open(
        &self,
        id: usize,
        states: &BTreeMap<String, RoadState>,
        civilian: bool,
        walk: bool,
    ) -> bool {
        let edge = &self.edges[id];
        (walk || edge.drivable)
            && states.get(&edge.road).is_some_and(|s| {
                let fire_cut = s
                    .fire_closed_links
                    .as_ref()
                    .map(|links| {
                        (links.is_empty() && s.cut_by_fire)
                            || links.contains(&(id * 2))
                            || links.contains(&(id * 2 + 1))
                    })
                    .unwrap_or(s.cut_by_fire);
                !fire_cut && (!civilian || !s.closed)
            })
    }
    fn anchors(
        &self,
        p: Pos,
        states: &BTreeMap<String, RoadState>,
        civilian: bool,
        walk: bool,
        max_access: f32,
    ) -> Vec<(usize, Pos, f32)> {
        let mut anchors = self
            .edges
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let a = self.nodes[e.a];
                let b = self.nodes[e.b];
                let d = distance(a, b).powi(2);
                let t = (((p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1])) / d)
                    .clamp(0.0, 1.0);
                let q = interpolate(a, b, t);
                let access = distance(p, q);
                (access <= max_access).then_some((i, q, access))
            })
            .collect::<Vec<_>>();
        anchors.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        if let Some(first) = anchors.first() {
            let nearest = first.2;
            // A position on the network must stay on that network: do not
            // turn an interrupted road into an off-road shortcut.
            if nearest <= 1.0 {
                anchors.retain(|a| a.2 <= nearest + 1.0);
            }
        }
        anchors.retain(|a| self.open(a.0, states, civilian, walk));
        // Off-road access may choose a slightly farther reachable component.
        anchors
    }
    fn leg(&self, id: usize, a: Pos, b: Pos, walk: bool) -> Leg {
        let e = &self.edges[id];
        let reverse = (b[0] - a[0]) * (self.nodes[e.b][0] - self.nodes[e.a][0])
            + (b[1] - a[1]) * (self.nodes[e.b][1] - self.nodes[e.a][1])
            < 0.0;
        let full = distance(self.nodes[e.a], self.nodes[e.b]);
        Leg {
            from: a,
            to: b,
            road: Some(e.road.clone()),
            link: Some(id * 2 + reverse as usize),
            length_m: distance(a, b),
            speed_mps: if walk { 3.0 } else { e.speed * e.travel_factor },
            storage: ((full * e.lanes as f32 / 7.5).floor() as u32).max(1),
            flow_per_s: e.flow * e.lanes as f32,
        }
    }
    fn access(a: Pos, b: Pos, walk: bool) -> Leg {
        Leg {
            from: a,
            to: b,
            road: None,
            link: None,
            length_m: distance(a, b),
            speed_mps: if walk { 1.1 } else { 3.0 },
            storage: u32::MAX,
            flow_per_s: 1.0,
        }
    }
    pub fn segments(&self) -> Vec<RoadSegment> {
        self.edges
            .iter()
            .enumerate()
            .map(|(id, e)| RoadSegment {
                road: e.road.clone(),
                links: [id * 2, id * 2 + 1],
                from: self.nodes[e.a],
                to: self.nodes[e.b],
            })
            .collect()
    }
    pub fn leg_open(
        &self,
        leg: &Leg,
        states: &BTreeMap<String, RoadState>,
        civilian: bool,
    ) -> bool {
        leg.road.as_ref().is_none_or(|road| {
            states.get(road).is_some_and(|s| {
                (!civilian || !s.closed)
                    && !s
                        .fire_closed_links
                        .as_ref()
                        .map(|links| {
                            (links.is_empty() && s.cut_by_fire)
                                || leg.link.is_some_and(|id| links.contains(&id))
                        })
                        .unwrap_or(s.cut_by_fire)
            })
        })
    }
    pub fn route_open(
        &self,
        route: &Route,
        states: &BTreeMap<String, RoadState>,
        civilian: bool,
    ) -> bool {
        route
            .legs
            .iter()
            .all(|l| self.leg_open(l, states, civilian))
    }
    /// Fastest route from `from` to `to`. All access anchors are seeded into a
    /// single multi-source Dijkstra and only the chosen path is rebuilt: with
    /// the 1.5 km walking access radius there can be hundreds of anchors at
    /// each end, and a search plus a path per anchor pair was quadratic.
    pub fn route(
        &self,
        from: Pos,
        to: Pos,
        states: &BTreeMap<String, RoadState>,
        civilian: bool,
        walk: bool,
    ) -> Option<Route> {
        if distance(from, to) < 0.1 {
            return Some(Route {
                legs: vec![Self::access(from, to, walk)],
            });
        }
        let max_access = if walk { 1500.0 } else { 300.0 };
        let starts = self.anchors(from, states, civilian, walk, max_access);
        let ends = self.anchors(to, states, civilian, walk, max_access);
        let access_speed = if walk { 1.1 } else { 3.0 };
        let mut costs = vec![f32::INFINITY; self.nodes.len()];
        let mut prev: Vec<Option<(usize, usize)>> = vec![None; self.nodes.len()];
        // Start anchor that seeded a root node (a node whose `prev` is None).
        let mut seed: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        for (k, (si, sq, _)) in starts.iter().enumerate() {
            let se = &self.edges[*si];
            for n in [se.a, se.b] {
                if se.oneway && n == se.a && distance(*sq, self.nodes[n]) > 0.01 {
                    continue;
                }
                let access = Self::access(from, *sq, walk);
                let leg = self.leg(*si, *sq, self.nodes[n], walk);
                let cost = access.length_m / access.speed_mps + leg.length_m / leg.speed_mps;
                if cost < costs[n] {
                    costs[n] = cost;
                    seed[n] = Some(k);
                    heap.push(Visit(cost, n));
                }
            }
        }
        while let Some(Visit(cost, n)) = heap.pop() {
            if cost > costs[n] {
                continue;
            }
            for &(next, eid) in &self.adj[n] {
                let e = &self.edges[eid];
                if !self.open(eid, states, civilian, walk) {
                    continue;
                }
                let c = cost
                    + distance(self.nodes[n], self.nodes[next])
                        / if walk { 3.0 } else { e.speed * e.travel_factor };
                if c < costs[next] {
                    costs[next] = c;
                    prev[next] = Some((n, eid));
                    heap.push(Visit(c, next));
                }
            }
        }
        enum Choice {
            /// Start and end anchors lie on the same edge.
            Direct(usize, usize),
            /// End anchor reached through graph node `n`.
            Via(usize, usize),
        }
        let start_on: BTreeMap<usize, usize> = starts
            .iter()
            .enumerate()
            .map(|(k, (si, _, _))| (*si, k))
            .collect();
        let mut best: Option<(f32, Choice)> = None;
        for (j, (ei, eq, ea)) in ends.iter().enumerate() {
            let ee = &self.edges[*ei];
            if let Some(&k) = start_on.get(ei) {
                let (_, sq, sa) = &starts[k];
                if !ee.oneway
                    || (eq[0] - sq[0]) * (self.nodes[ee.b][0] - self.nodes[ee.a][0])
                        + (eq[1] - sq[1]) * (self.nodes[ee.b][1] - self.nodes[ee.a][1])
                        >= 0.0
                {
                    let cost = sa / access_speed
                        + distance(*sq, *eq) / self.leg(*ei, *sq, *eq, walk).speed_mps
                        + ea / access_speed;
                    if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                        best = Some((cost, Choice::Direct(k, j)));
                    }
                }
            }
            for n in [ee.a, ee.b] {
                if ee.oneway && n == ee.b && distance(*eq, self.nodes[n]) > 0.01 {
                    continue;
                }
                if !costs[n].is_finite() {
                    continue;
                }
                let cost = costs[n]
                    + distance(self.nodes[n], *eq)
                        / self.leg(*ei, self.nodes[n], *eq, walk).speed_mps
                    + ea / access_speed;
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, Choice::Via(j, n)));
                }
            }
        }
        let (_, choice) = best?;
        let (sq, eq, mut legs) = match choice {
            Choice::Direct(k, j) => {
                let (si, sq, _) = starts[k];
                let (_, eq, _) = ends[j];
                (sq, eq, vec![self.leg(si, sq, eq, walk)])
            }
            Choice::Via(j, n) => {
                let (ei, eq, _) = ends[j];
                let mut path = vec![];
                let mut cur = n;
                while let Some((p, id)) = prev[cur] {
                    path.push(self.leg(id, self.nodes[p], self.nodes[cur], walk));
                    cur = p;
                }
                let (si, sq, _) = starts[seed[cur].expect("route root is a seeded start node")];
                path.push(self.leg(si, sq, self.nodes[cur], walk));
                path.reverse();
                path.push(self.leg(ei, self.nodes[n], eq, walk));
                (sq, eq, path)
            }
        };
        legs.insert(0, Self::access(from, sq, walk));
        legs.push(Self::access(eq, to, walk));
        legs.retain(|l| l.length_m > 0.01);
        Some(Route { legs })
    }
}
