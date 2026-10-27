//! The remaining suggestion rules, each on a hand-made situation.
use std::sync::Arc;

use exercise::suggestions::Suggestion;
use exercise::*;
use sim::{Command, World};

fn started(turn_minutes: u32) -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
    }
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = turn_minutes;
    ex
}

fn ignite_near(ex: &mut Exercise, pos: [f32; 2]) {
    for (dx, dy) in [(-120.0, 0.0), (120.0, 0.0), (0.0, 120.0), (0.0, -120.0), (-200.0, 80.0), (200.0, -80.0), (0.0, 250.0), (0.0, -250.0)] {
        let at = sim::Loc::Point { x: pos[0] + dx, y: pos[1] + dy };
        if ex.direct_command(&Command::Ignite { at, radius_m: Some(60.0) }).is_ok() {
            return;
        }
    }
    panic!("nowhere to ignite near {pos:?}");
}

fn ids(list: &[Suggestion]) -> Vec<&str> {
    list.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn houses_are_summed_per_sector() {
    let mut ex = started(120);
    let school = ex.world.scn.buildings.iter().find(|b| b.kind == scenario::BuildingKind::School).unwrap().pos;
    ignite_near(&mut ex, school);
    let preview = ex.preview_job().unwrap().run().unwrap();
    let list = ex.suggestions(Some(&preview));
    let houses: Vec<_> = list.iter().filter(|s| s.rule == "houses_reached").collect();
    assert!(!houses.is_empty(), "{:?}", ids(&list));
    assert!(houses.iter().all(|s| s.object.kind == "sector" && s.id.starts_with("houses_reached:")));
}

#[test]
fn shelter_near_the_front_and_over_capacity() {
    let mut ex = started(120);
    let shelter = ex.world.scn.buildings.iter().find(|b| b.shelter_capacity > 0).cloned().unwrap();
    let cap = shelter.shelter_capacity;
    ex.world.d.shelters.insert(shelter.id, cap + 20);
    let over = ex.suggestions(None);
    let s = over.iter().find(|s| s.rule == "shelter_over").unwrap_or_else(|| panic!("{:?}", ids(&over)));
    assert!(s.evidence.en.contains(&format!("for {cap} places")), "{}", s.evidence.en);
    assert!(over.iter().all(|s| s.rule != "shelter_front"));
    ignite_near(&mut ex, shelter.pos);
    let preview = ex.preview_job().unwrap().run().unwrap();
    let list = ex.suggestions(Some(&preview));
    assert!(list.iter().any(|s| s.rule == "shelter_front" && s.object.id == format!("b{}", shelter.id)), "{:?}", ids(&list));
}

fn leg(road: &str) -> sim::network::Leg {
    sim::network::Leg {
        from: [0.0, 0.0],
        to: [100.0, 0.0],
        road: Some(road.into()),
        link: None,
        length_m: 100.0,
        speed_mps: 10.0,
        storage: 10,
        flow_per_s: 1.0,
    }
}

#[test]
fn evacuees_on_a_road_that_is_cut_and_isolated_places() {
    let mut ex = started(60);
    let road = ex.world.scn.roads[0].id.clone();
    ex.world.d.traffic.vehicles.push(sim::traffic::Vehicle {
        id: 1,
        building: 0,
        people: 40,
        ready_s: 0,
        destination: None,
        target: None,
        route: Some(sim::network::Route { legs: vec![leg(&road)] }),
        leg: 0,
        entered_s: 0,
        departed: true,
        completed: false,
        waiting: false,
        at_risk: false,
        detail: String::new(),
    });
    assert!(ex.suggestions(None).iter().all(|s| s.rule != "evac_route_cut"), "road still open");
    ex.world.d.roads.get_mut(&road).unwrap().cut_by_fire = true;
    let list = ex.suggestions(None);
    let s = list.iter().find(|s| s.rule == "evac_route_cut").unwrap_or_else(|| panic!("{:?}", ids(&list)));
    assert!(s.now && s.evidence.en.contains("40 people") && s.evidence.it.contains("40 persone"));
    // Cut every road: every populated place is isolated.
    for r in ex.world.d.roads.values_mut() {
        r.cut_by_fire = true;
    }
    let list = ex.suggestions(None);
    assert!(list.iter().any(|s| s.rule == "place_without_exit" && s.now), "{:?}", ids(&list));
}

#[test]
fn a_task_whose_route_will_be_cut_is_flagged() {
    let mut ex = started(60);
    let road = ex.world.scn.roads[0].id.clone();
    ex.world.d.roads.get_mut(&road).unwrap().cut_by_fire = true;
    let target = [500.0, 500.0];
    ex.world.d.tasks.push(sim::DeploymentTask {
        id: 7,
        role: "fire_commander".into(),
        allocations: vec![],
        resource: "engines".into(),
        qty: 2,
        command: Command::IssueWarning { zone: "F4".into() },
        origin: [0.0, 0.0],
        target,
        speed_mps: 10.0,
        departed_s: 0,
        arrival_s: 600,
        arrived_s: None,
        completed_s: None,
        status: sim::TaskStatus::EnRoute,
        detail: String::new(),
        route: Some(sim::network::Route { legs: vec![leg(&road)] }),
        route_checked_s: 0,
        work_to: None,
        work_m: 0.0,
        water_l: 0.0,
        used_water_l: 0.0,
        last_work_s: 0,
        refill_done_s: None,
        mission_centre: target,
        policy_version: String::new(),
        policy_reason: String::new(),
        last_policy_s: 0,
        regroup_until_s: None,
    });
    let list = ex.suggestions(None);
    let s = list.iter().find(|s| s.rule == "task_route_cut").unwrap_or_else(|| panic!("{:?}", ids(&list)));
    assert_eq!(s.recipients, vec!["fire_commander".to_string()]);
    assert!(s.evidence.en.contains("2 engines"), "{}", s.evidence.en);
}
