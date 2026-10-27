//! Contract checks on the generated, shipped territory (not a mock fixture).
use scenario::{fuel, Scenario};
use sim::network::Network;
use std::collections::BTreeMap;

fn landscape() -> Scenario {
    Scenario::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/scenarios/valverde_factory.json"
    ))
    .unwrap()
}

#[test]
fn generated_facilities_are_reachable_on_the_real_road_graph() {
    let s = landscape();
    let graph = Network::build(&s);
    let states = s
        .roads
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                sim::RoadState {
                    closed: false,
                    cut_by_fire: false,
                    fire_closed_links: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let staging = s
        .buildings
        .iter()
        .find(|b| b.name.as_deref() == Some("Valverde Sports Centre"))
        .unwrap()
        .pos;
    for b in s.buildings.iter().filter(|b| b.name.is_some()) {
        let route = graph.route(staging, b.pos, &states, false, false);
        assert!(route.is_some(), "unreachable facility: {:?}", b.name);
    }
    // Keep evacuation exits connected to the shelter network too.
    for exit in [[0., 1300.], [4000., 1500.], [1400., 0.]] {
        assert!(graph.route(staging, exit, &states, true, false).is_some());
    }
}

#[test]
fn generated_footprints_do_not_overlap_and_occupants_are_preserved() {
    let s = landscape();
    assert_eq!(s.grid.cellsize, 20.);
    assert_eq!(s.grid.len(), 40_000);
    assert_eq!(s.render_terrain.as_ref().unwrap().posting_m, 5.);
    let template = scenario::synthetic::generate(7);
    assert_eq!(
        s.buildings.iter().map(|b| b.occupants).sum::<u32>(),
        template.buildings.iter().map(|b| b.occupants).sum::<u32>()
    );
    for (i, a) in s.buildings.iter().enumerate() {
        assert_eq!(s.fuel_at(s.cell_of(a.pos).unwrap()), fuel::NON_VEGETATED);
        for b in &s.buildings[i + 1..] {
            assert!(
                (a.pos[0] - b.pos[0]).abs() >= (a.size[0] + b.size[0]) / 2.
                    || (a.pos[1] - b.pos[1]).abs() >= (a.size[1] + b.size[1]) / 2.,
                "overlap {} and {}",
                a.id,
                b.id
            );
        }
    }
    assert_eq!(s.msel.len(), template.msel.len());
    assert_eq!(s.roles.len(), template.roles.len());
    assert!(
        s.dem.iter().copied().fold(f32::MIN, f32::max)
            - s.dem.iter().copied().fold(f32::MAX, f32::min)
            > 300.
    );
    assert!(s.render_terrain.is_some());
}

#[test]
fn authored_roads_have_no_reversing_spikes_or_facility_collisions() {
    let s = landscape();
    for road in s.roads.iter().filter(|r| r.drivable) {
        for p in road.points.windows(3) {
            let u = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let v = [p[2][0] - p[1][0], p[2][1] - p[1][1]];
            let lu = u[0].hypot(u[1]);
            let lv = v[0].hypot(v[1]);
            if lu > 1. && lv > 1. {
                assert!(
                    (u[0] * v[0] + u[1] * v[1]) / (lu * lv) > -0.09,
                    "reversing spike in {}",
                    road.id
                );
            }
        }
        for b in s.buildings.iter().filter(|b| b.name.is_some()) {
            for p in road.points.windows(2) {
                for k in 0..=8 {
                    let t = k as f32 / 8.;
                    let at = [
                        p[0][0] + t * (p[1][0] - p[0][0]),
                        p[0][1] + t * (p[1][1] - p[0][1]),
                    ];
                    let margin = if road.class == "secondary" { 5.5 } else { 3.5 };
                    assert!(
                        (at[0] - b.pos[0]).abs() >= b.size[0] / 2. + margin
                            || (at[1] - b.pos[1]).abs() >= b.size[1] / 2. + margin,
                        "{} intersects {:?}",
                        road.id,
                        b.name
                    );
                }
            }
        }
    }
}

#[test]
fn drivable_network_is_one_connected_component_without_access_shortcuts() {
    let s = landscape();
    type Key = (i64, i64);
    let key = |p: [f32; 2]| ((p[0] * 20.).round() as i64, (p[1] * 20.).round() as i64);
    let mut adjacency = BTreeMap::<Key, Vec<Key>>::new();
    for r in s.roads.iter().filter(|r| r.drivable) {
        for p in r.points.windows(2) {
            let (a, b) = (key(p[0]), key(p[1]));
            adjacency.entry(a).or_default().push(b);
            adjacency.entry(b).or_default().push(a);
        }
    }
    let mut visited = std::collections::BTreeSet::new();
    let mut stack = vec![*adjacency.keys().next().unwrap()];
    while let Some(node) = stack.pop() {
        if visited.insert(node) {
            stack.extend(adjacency[&node].iter().copied());
        }
    }
    assert_eq!(
        visited.len(),
        adjacency.len(),
        "roads must share real junctions"
    );
}
