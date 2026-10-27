use scenario::{Operations, Road};
use sim::{
    network::{Network, Route},
    traffic::{LinkQueue, Traffic},
    RoadState, World,
};
use std::{collections::BTreeMap, sync::Arc};

fn road(id: &str, points: &[[f32; 2]]) -> Road {
    Road {
        id: id.into(),
        name: id.into(),
        points: points.to_vec(),
        class: "primary".into(),
        drivable: true,
        oneway: false,
    }
}
fn scene() -> scenario::Scenario {
    let mut s = scenario::synthetic::generate(7);
    s.operations = Operations::enhanced();
    s.dem.fill(0.0);
    s.roads = vec![
        road(
            "main",
            &[[0., 200.], [1000., 200.], [2000., 200.], [3000., 200.]],
        ),
        road(
            "detour",
            &[[1000., 200.], [1000., 1000.], [2000., 1000.], [2000., 200.]],
        ),
    ];
    s
}
fn states(s: &scenario::Scenario) -> BTreeMap<String, RoadState> {
    s.roads
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                RoadState {
                    closed: false,
                    cut_by_fire: false,
                    fire_closed_links: Some(vec![]),
                },
            )
        })
        .collect()
}
fn cut(states: &mut BTreeMap<String, RoadState>, id: usize) {
    let s = states.get_mut("main").unwrap();
    s.cut_by_fire = true;
    s.fire_closed_links = Some(vec![id * 2, id * 2 + 1]);
}
#[test]
fn local_cut_preserves_access_and_selects_detour_without_offroad_bypass() {
    let s = scene();
    let net = Network::build(&s);
    let mut state = states(&s);
    cut(&mut state, 1);
    let unaffected = net
        .route([0., 200.], [500., 200.], &state, false, false)
        .unwrap();
    assert!((unaffected.length_m() - 500.).abs() < 0.1);
    let detour = net
        .route([500., 200.], [2500., 200.], &state, false, false)
        .unwrap();
    assert!(detour
        .legs
        .iter()
        .any(|l| l.road.as_deref() == Some("detour")));
    assert!(net.route_open(&detour, &state, true));
    assert!(
        net.route([1500., 200.], [2500., 200.], &state, false, false)
            .is_none(),
        "position inside closed segment cannot teleport to an open road"
    );
    state.get_mut("main").unwrap().closed = true;
    assert!(net
        .route([0., 200.], [500., 200.], &state, true, false)
        .is_none());
    assert!(
        net.route([0., 200.], [500., 200.], &state, false, false)
            .is_some(),
        "police closures exempt emergency vehicles"
    );
}
#[test]
fn offroad_access_uses_reachable_component_instead_of_nearest_island() {
    let mut s = scene();
    s.roads = vec![
        road("island", &[[500., 100.], [700., 100.]]),
        road("main", &[[500., 250.], [3000., 250.]]),
    ];
    let net = Network::build(&s);
    let route = net
        .route([600., 130.], [2500., 250.], &states(&s), false, false)
        .unwrap();
    assert!(route
        .legs
        .iter()
        .all(|l| l.road.as_deref() != Some("island")));
}
#[test]
fn one_way_dead_end_is_unreachable() {
    let mut s = scene();
    s.roads.truncate(1);
    s.roads[0].oneway = true;
    let net = Network::build(&s);
    let states = states(&s);
    assert!(net
        .route([2500., 200.], [500., 200.], &states, false, false)
        .is_none());
    assert!(net
        .route([500., 200.], [2500., 200.], &states, false, false)
        .is_some());
}
fn assert_eta_matches_position(route: &Route, speed: f32) {
    let end = route.legs.last().unwrap().to;
    let at = route.position(route.seconds(speed), speed);
    assert!((at[0] - end[0]).abs() < 0.01 && (at[1] - end[1]).abs() < 0.01);
    assert_ne!(route.position(route.seconds(speed) * 0.5, speed), end);
}
#[test]
fn terrain_and_queue_delays_share_eta_and_motion_rules_and_are_opt_in() {
    let mut s = scene();
    for row in 0..s.grid.rows {
        for col in 0..s.grid.cols {
            s.dem[row * s.grid.cols + col] = col as f32 * s.grid.cellsize * 0.15;
        }
    }
    let states = states(&s);
    let enhanced = Network::build(&s)
        .route([500., 200.], [2500., 200.], &states, false, false)
        .unwrap();
    s.operations.terrain_travel = false;
    let baseline = Network::build(&s)
        .route([500., 200.], [2500., 200.], &states, false, false)
        .unwrap();
    assert!(
        (baseline.seconds(20.) - 2000.0 / 16.7).abs() < 0.01,
        "disabled terrain adjustment preserves road-class speed"
    );
    assert!(enhanced.seconds(20.) > baseline.seconds(20.));
    assert_eta_matches_position(&enhanced, 20.);
    let mut traffic = Traffic::default();
    for leg in &enhanced.legs {
        if let Some(link) = leg.link {
            traffic.links.insert(
                link,
                LinkQueue {
                    queue: vec![0, 1, 2, 3],
                    next_release_s: 0.,
                },
            );
        }
    }
    let mut delayed = enhanced.clone();
    traffic.delay_route(&mut delayed, 20.);
    assert!(delayed.seconds(20.) > enhanced.seconds(20.));
    assert_eta_matches_position(&delayed, 20.);
    let mut no_traffic = enhanced.clone();
    Traffic::default().delay_route(&mut no_traffic, 20.);
    assert_eq!(enhanced.seconds(20.), no_traffic.seconds(20.));
}
#[test]
fn segment_state_and_geometry_restore_with_world_snapshot() {
    let mut w = World::with_options(
        Arc::new(scene()),
        1,
        sim::FireOptions {
            realizations: 1,
            cutoff: 0.5,
        },
    )
    .unwrap();
    let before = w.snapshot();
    let geometry = serde_json::to_value(w.network.segments()).unwrap();
    cut(&mut w.d.roads, 1);
    assert!(!w.d.roads["main"]
        .fire_closed_links
        .as_ref()
        .unwrap()
        .is_empty());
    w.restore(&before).unwrap();
    assert!(!w.d.roads["main"].cut_by_fire);
    assert!(w.d.roads["main"]
        .fire_closed_links
        .as_ref()
        .unwrap()
        .is_empty());
    assert_eq!(
        geometry,
        serde_json::to_value(w.network.segments()).unwrap()
    );
}

#[test]
fn simulated_fire_closes_local_segments_independent_of_advance_chunk_size() {
    let mut s = scene();
    s.fuel.fill(scenario::fuel::GRASSLAND);
    let make = || {
        World::with_options(
            Arc::new(s.clone()),
            1,
            sim::FireOptions {
                realizations: 1,
                cutoff: 0.5,
            },
        )
        .unwrap()
    };
    let mut a = make();
    let mut b = make();
    a.fire.ignite([1500., 200.], 40.).unwrap();
    b.fire.ignite([1500., 200.], 40.).unwrap();
    a.advance(10).unwrap();
    for _ in 0..10 {
        b.advance(1).unwrap();
    }
    assert_eq!(
        serde_json::to_value(&a.d.roads).unwrap(),
        serde_json::to_value(&b.d.roads).unwrap()
    );
    let links = a.d.roads["main"].fire_closed_links.as_ref().unwrap();
    assert!(links.contains(&2) && links.contains(&3));
    assert!(!links.contains(&0) && !links.contains(&4));
    assert!(a
        .network
        .route([0., 200.], [500., 200.], &a.d.roads, false, false)
        .is_some());
}

#[test]
fn sharp_bends_slow_travel_only_when_terrain_rules_are_enabled() {
    let mut s = scene();
    s.roads = vec![road("main", &[[100., 100.], [1000., 100.], [1000., 1000.]])];
    let state = states(&s);
    let curved = Network::build(&s)
        .route([100., 100.], [1000., 1000.], &state, false, false)
        .unwrap();
    assert!(curved.legs[1].speed_mps < curved.legs[0].speed_mps);
    s.operations.terrain_travel = false;
    let legacy = Network::build(&s)
        .route([100., 100.], [1000., 1000.], &state, false, false)
        .unwrap();
    assert!(curved.seconds(20.) > legacy.seconds(20.));
    assert_eta_matches_position(&curved, 20.);
}
