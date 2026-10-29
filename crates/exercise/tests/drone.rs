//! The monitoring drone: deployment, periodic map updates on simulated time, endurance, recharge, rollback.
use std::sync::Arc;

use exercise::*;
use sim::{Actor, Command, TaskStatus, World};

fn exercise_with(edit: impl FnOnce(&mut scenario::Scenario)) -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
    }
    edit(&mut scn);
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex
}

fn monitor(zones: &[&str]) -> Command {
    Command::MonitorArea {
        zones: zones.iter().map(|z| z.to_string()).collect(),
        role: None,
    }
}

fn ignite(ex: &mut Exercise) {
    ex.direct_command(&Command::Ignite {
        at: sim::Loc::Named("F3".into()),
        radius_m: Some(100.0),
    })
    .unwrap();
}

fn run_to(ex: &mut Exercise, to_s: i64, step: i64) {
    ex.begin_resolution_until(to_s, true).unwrap();
    while !ex.step_resolution(step).unwrap() {}
    ex.finish_resolution().unwrap();
}

fn drone_times(ex: &Exercise) -> Vec<i64> {
    ex.knowledge_events
        .iter()
        .filter(|e| matches!(e.source, KnowledgeSource::Monitoring))
        .map(|e| e.t_s)
        .collect()
}

fn task(ex: &Exercise) -> &sim::DeploymentTask {
    ex.world
        .d
        .tasks
        .iter()
        .find(|t| matches!(t.command, Command::MonitorArea { .. }))
        .unwrap()
}

fn drone_messages(ex: &Exercise) -> Vec<String> {
    ex.messages
        .iter()
        .filter(|m| {
            m.to == "fire_commander" && m.title.as_deref().is_some_and(|t| t.starts_with("Drone"))
        })
        .map(|m| m.title.clone().unwrap())
        .collect()
}

#[test]
fn updates_follow_simulated_intervals_whatever_the_step() {
    let mut runs = Vec::new();
    for step in [1, 7, 60] {
        let mut ex = exercise_with(|_| {});
        ignite(&mut ex);
        ex.direct_command(&monitor(&["F3-G4"])).unwrap();
        let arrival = task(&ex).arrival_s;
        assert_eq!(task(&ex).status, TaskStatus::EnRoute);
        // Nothing is learned while the drone is in transit.
        assert!(ex.player_map("fire_commander").unwrap()["cells"]
            .as_array()
            .unwrap()
            .is_empty());
        run_to(&mut ex, arrival + 1000, step);
        let times = drone_times(&ex);
        assert_eq!(
            times,
            vec![arrival, arrival + 300, arrival + 600, arrival + 900],
            "step {step}"
        );
        let map = ex.player_map("fire_commander").unwrap();
        assert_eq!(map["last_observation_source"], "Drone");
        assert_eq!(map["live_zones"].as_array().unwrap().len(), 4);
        assert_eq!(map["cells"].as_array().unwrap().len(), 4 * 25 * 25);
        // One message at the start, none per update.
        assert_eq!(drone_messages(&ex), vec!["Drone on station"], "step {step}");
        runs.push((times, serde_json::to_string(&map["cells"]).unwrap()));
    }
    assert!(
        runs.windows(2).all(|w| w[0] == w[1]),
        "observations do not depend on the step size"
    );
}

#[test]
fn coverage_limit_is_enforced() {
    let ex = exercise_with(|_| {});
    let who = Actor::Role("fire_commander".into());
    for ok in [&["F3-G4"][..], &["F3", "G4"], &["A1"], &["f3-f6"]] {
        assert!(ex.world.validate(&monitor(ok), &who).is_ok(), "{ok:?}");
    }
    for bad in [
        &["A1-E5"][..],
        &["A1", "E1"],
        &["F3-H5"],
        &["Z9"],
        &[],
        &["F3-"],
    ] {
        assert!(!ex.world.validate(&monitor(bad), &who).is_ok(), "{bad:?}");
    }
    let mayor = Actor::Role("mayor".into());
    assert!(
        !ex.world.validate(&monitor(&["F3"]), &mayor).is_ok(),
        "only the drone's owner may order it"
    );
}

#[test]
fn endurance_return_recharge_then_available_again() {
    let mut ex = exercise_with(|_| {});
    ignite(&mut ex);
    ex.direct_command(&monitor(&["F3"])).unwrap();
    let pool = |ex: &Exercise| ex.world.d.pools["fire_commander"]["drones"];
    assert_eq!(pool(&ex).available(), 0);
    assert!(
        ex.direct_command(&monitor(&["F4"])).is_err(),
        "the only drone is committed"
    );
    let arrival = task(&ex).arrival_s;
    run_to(&mut ex, arrival + 2400 + 5, 30);
    assert_eq!(
        task(&ex).status,
        TaskStatus::Refilling,
        "{:?}",
        task(&ex).detail
    );
    let times = drone_times(&ex);
    assert_eq!(times.len(), 9, "{times:?}");
    assert_eq!(*times.last().unwrap(), arrival + 2400);
    assert_eq!(
        drone_messages(&ex),
        vec!["Drone on station", "Drone returned"]
    );
    assert_eq!(pool(&ex).available(), 0, "still recharging");
    let done = task(&ex).refill_done_s.unwrap();
    assert!(done >= arrival + 2400 + 900);
    run_to(&mut ex, done + 1, 60);
    assert_eq!(task(&ex).status, TaskStatus::Completed);
    assert_eq!(pool(&ex).available(), 1);
    assert_eq!(
        drone_times(&ex).len(),
        9,
        "no updates after the end of the mission"
    );
    ex.direct_command(&monitor(&["F4"])).unwrap();
}

#[test]
fn role_order_flies_from_the_staging_base() {
    let mut ex = exercise_with(|_| {});
    ignite(&mut ex);
    let a = ex
        .world
        .apply(&monitor(&["F3-G3"]), &Actor::Role("fire_commander".into()))
        .unwrap();
    assert!(a.iter().any(|n| n.contains("Dispatched")), "{a:?}");
    let t = task(&ex);
    assert_eq!(
        t.origin,
        ex.world.d.travel_settings["fire_commander"]["drones"].origin
    );
    assert!(t.arrival_s > ex.world.time_s(), "transit takes time");
    assert_eq!(t.speed_mps, 20.0);
}

#[test]
fn rollback_forgets_later_observations() {
    let mut ex = exercise_with(|_| {});
    ignite(&mut ex);
    ex.direct_command(&monitor(&["F3"])).unwrap();
    let arrival = task(&ex).arrival_s;
    run_to(&mut ex, arrival + 700, 60);
    assert_eq!(drone_times(&ex).len(), 3);
    assert!(!ex.player_map("fire_commander").unwrap()["cells"]
        .as_array()
        .unwrap()
        .is_empty());
    ex.rollback(1).unwrap();
    assert!(drone_times(&ex).is_empty());
    assert!(
        ex.world.d.tasks.is_empty(),
        "the order was given after the snapshot"
    );
    assert!(ex.player_map("fire_commander").unwrap()["cells"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(ex.player_map("fire_commander").unwrap()["drones"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn wind_above_the_limit_grounds_and_recalls() {
    let limit = |s: &mut scenario::Scenario| {
        s.resource_types
            .iter_mut()
            .find(|r| r.id == "drones")
            .unwrap()
            .drone
            .as_mut()
            .unwrap()
            .wind_limit_kmh = Some(50.0);
    };
    let mut ex = exercise_with(limit);
    ex.direct_command(&Command::SetWeather {
        wind_dir_deg: None,
        wind_speed_kmh: Some(70.0),
        moisture_pct: None,
    })
    .unwrap();
    assert!(ex.direct_command(&monitor(&["F3"])).is_err(), "grounded");
    ex.direct_command(&Command::SetWeather {
        wind_dir_deg: None,
        wind_speed_kmh: Some(30.0),
        moisture_pct: None,
    })
    .unwrap();
    ignite(&mut ex);
    ex.direct_command(&monitor(&["F3"])).unwrap();
    let arrival = task(&ex).arrival_s;
    run_to(&mut ex, arrival + 400, 60);
    assert_eq!(drone_times(&ex).len(), 2);
    ex.direct_command(&Command::SetWeather {
        wind_dir_deg: None,
        wind_speed_kmh: Some(70.0),
        moisture_pct: None,
    })
    .unwrap();
    let now = ex.world.time_s();
    run_to(&mut ex, now + 700, 60);
    assert_eq!(task(&ex).status, TaskStatus::Refilling);
    assert_eq!(
        drone_times(&ex).len(),
        2,
        "no observation after the wind recalled it"
    );
    assert_eq!(drone_messages(&ex).last().unwrap(), "Drone returned");
}

#[test]
fn excon_recall_stops_coverage_at_once() {
    let mut ex = exercise_with(|_| {});
    ignite(&mut ex);
    ex.direct_command(&monitor(&["F3"])).unwrap();
    let arrival = task(&ex).arrival_s;
    run_to(&mut ex, arrival + 100, 30);
    let allocation = ex
        .world
        .d
        .allocations
        .iter()
        .find(|a| a.resource == "drones")
        .unwrap()
        .id;
    ex.direct_command(&Command::ReturnResource { allocation, qty: 1 })
        .unwrap();
    let now = ex.world.time_s();
    run_to(&mut ex, now + 900, 60);
    assert_eq!(drone_times(&ex).len(), 1);
    assert_eq!(ex.world.d.pools["fire_commander"]["drones"].available(), 1);
    assert_eq!(drone_messages(&ex).last().unwrap(), "Drone returned");
}
