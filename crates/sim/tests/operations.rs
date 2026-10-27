use scenario::{Building, BuildingKind, GridSpec, Operations, Road, Sector, ZoneGrid};
use sim::{Actor, Command, Loc, TaskStatus, World};
use std::sync::Arc;
fn road(id: &str, points: &[[f32; 2]]) -> Road {
    Road {
        id: id.into(),
        name: id.into(),
        points: points.to_vec(),
        class: "service".into(),
        drivable: true,
        oneway: false,
    }
}
fn scene() -> scenario::Scenario {
    let mut s = scenario::synthetic::generate(7);
    s.grid = GridSpec {
        rows: 20,
        cols: 20,
        cellsize: 20.0,
    };
    s.fuel = vec![scenario::fuel::GRASSLAND; 400];
    s.dem = vec![0.0; 400];
    s.places.clear();
    s.msel.clear();
    s.sectors = vec![Sector {
        id: "test".into(),
        name: "test".into(),
        bounds: [0.0, 0.0, 401.0, 401.0],
    }];
    s.zones = ZoneGrid {
        rows: 8,
        cols: 8,
        size_m: 50.0,
    };
    s.buildings = vec![
        Building {
            visual_kind: None,
            id: 1,
            kind: BuildingKind::House,
            name: Some("Homes".into()),
            pos: [100.0, 100.0],
            size: [10.0, 10.0],
            occupants: 120,
            sector: "test".into(),
            shelter_capacity: 0,
            rotation_deg: 0.0,
        },
        Building {
            visual_kind: None,
            id: 2,
            kind: BuildingKind::SportsCentre,
            name: Some("Shelter".into()),
            pos: [380.0, 100.0],
            size: [10.0, 10.0],
            occupants: 0,
            sector: "test".into(),
            shelter_capacity: 200,
            rotation_deg: 0.0,
        },
    ];
    s.roads = vec![
        road(
            "main",
            &[
                [0.0, 100.0],
                [100.0, 100.0],
                [200.0, 100.0],
                [300.0, 100.0],
                [400.0, 100.0],
            ],
        ),
        road(
            "detour",
            &[
                [100.0, 100.0],
                [100.0, 300.0],
                [300.0, 300.0],
                [300.0, 100.0],
            ],
        ),
    ];
    s.operations = Operations::enhanced();
    s.check().unwrap();
    s
}
fn world(s: scenario::Scenario) -> World {
    World::with_options(
        Arc::new(s),
        1,
        sim::FireOptions {
            realizations: 1,
            cutoff: 0.5,
        },
    )
    .unwrap()
}
fn fire_role() -> Actor {
    Actor::Role("fire_commander".into())
}
fn stage(w: &mut World, resource: &str, at: [f32; 2]) {
    w.apply(
        &Command::SetDeploymentTravel {
            role: "fire_commander".into(),
            resource: resource.into(),
            at: Loc::Point { x: at[0], y: at[1] },
            speed_mps: 10.0,
        },
        &Actor::Master,
    )
    .unwrap();
}
#[test]
fn routes_follow_junctions_and_civilian_closures() {
    let mut w = world(scene());
    let from = [100.0, 100.0];
    let to = [300.0, 100.0];
    let direct = w.network.route(from, to, &w.d.roads, true, false).unwrap();
    assert!((direct.length_m() - 200.0).abs() < 0.1);
    w.d.roads.get_mut("main").unwrap().closed = true;
    let detour = w.network.route(from, to, &w.d.roads, true, false).unwrap();
    assert!((detour.length_m() - 600.0).abs() < 0.1);
    assert!(detour
        .legs
        .iter()
        .all(|l| l.road.as_deref() != Some("main")));
    let emergency = w.network.route(from, to, &w.d.roads, false, false).unwrap();
    assert!((emergency.length_m() - 200.0).abs() < 0.1);
    w.d.roads.get_mut("main").unwrap().cut_by_fire = true;
    w.d.roads.get_mut("detour").unwrap().cut_by_fire = true;
    assert!(w
        .network
        .route(from, to, &w.d.roads, false, false)
        .is_none());
}
#[test]
fn one_way_roads_and_foot_only_tracks_are_respected() {
    let mut s = scene();
    s.roads = vec![road("oneway", &[[0.0, 100.0], [400.0, 100.0]])];
    s.roads[0].oneway = true;
    let w = world(s.clone());
    assert!(w
        .network
        .route([50.0, 100.0], [350.0, 100.0], &w.d.roads, true, false)
        .is_some());
    assert!(w
        .network
        .route([350.0, 100.0], [50.0, 100.0], &w.d.roads, true, false)
        .is_none());
    s.roads[0].drivable = false;
    s.roads[0].oneway = false;
    let w = world(s);
    assert!(w
        .network
        .route([50.0, 100.0], [350.0, 100.0], &w.d.roads, false, false)
        .is_none());
    assert!(w
        .network
        .route([50.0, 100.0], [350.0, 100.0], &w.d.roads, false, true)
        .is_some());
}
#[test]
fn crew_work_is_progressive_and_cancelled_work_stops() {
    let mut w = world(scene());
    stage(&mut w, "ground_crews", [100.0, 100.0]);
    let command = Command::CutFireLine {
        from: Some(Loc::Point { x: 100.0, y: 100.0 }),
        to: Some(Loc::Point { x: 100.0, y: 350.0 }),
        along: None,
        width_m: Some(6.0),
    };
    w.apply(&command, &fire_role()).unwrap();
    let arrival = w.d.tasks[0].arrival_s;
    w.advance(arrival).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Working);
    assert!(w.d.lines.is_empty());
    w.advance(600).unwrap();
    assert!((w.d.tasks[0].work_m - 20.0).abs() < 0.02);
    assert_eq!(w.d.lines.len(), 1);
    let before = w.snapshot();
    let progress = w.d.tasks[0].work_m;
    w.advance(600).unwrap();
    let after = w.d.tasks[0].work_m;
    w.restore(&before).unwrap();
    for _ in 0..60 {
        w.advance(10).unwrap();
    }
    assert!((w.d.tasks[0].work_m - after).abs() < 0.02);
    assert!(after > progress);
    w.apply(
        &Command::ReturnResource {
            allocation: w.d.tasks[0].allocations[0],
            qty: 1,
        },
        &Actor::Master,
    )
    .unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Cancelled);
    let progress = w.d.tasks[0].work_m;
    w.advance(600).unwrap();
    assert_eq!(w.d.tasks[0].work_m, progress);
}
#[test]
fn finite_drop_returns_to_base_and_snapshot_restores_consumables() {
    let mut w = world(scene());
    stage(&mut w, "air_tankers", [100.0, 100.0]);
    w.apply(
        &Command::WaterDrop {
            at: Loc::Point { x: 300.0, y: 100.0 },
        },
        &fire_role(),
    )
    .unwrap();
    let due = w.d.tasks[0].arrival_s;
    assert!(due >= 1500);
    w.advance(due - 1).unwrap();
    assert!(w.d.drops.is_empty());
    let snapshot = w.snapshot();
    w.advance(1).unwrap();
    assert_eq!(w.d.drops.len(), 1);
    assert_eq!(w.d.tasks[0].status, TaskStatus::Refilling);
    assert_eq!(w.d.tasks[0].used_water_l, 6137.0);
    assert_eq!(w.d.tasks[0].water_l, 0.0);
    let done = w.d.tasks[0].refill_done_s.unwrap();
    w.advance(done - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Completed);
    assert_eq!(w.d.tasks[0].water_l, 6137.0);
    w.restore(&snapshot).unwrap();
    assert_eq!(w.d.tasks[0].used_water_l, 0.0);
    assert!(w.d.drops.is_empty());
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].used_water_l, 6137.0);
}
fn evacuate(w: &mut World) {
    w.apply(
        &Command::OpenShelter {
            building: "Shelter".into(),
        },
        &Actor::Master,
    )
    .unwrap();
    let zone = w.scn.zone_of(w.scn.buildings[0].pos).unwrap();
    w.apply(&Command::IssueWarning { zone }, &Actor::Master)
        .unwrap();
    w.apply(
        &Command::EvacuateBuilding {
            building: "Homes".into(),
            destination: Some("Shelter".into()),
        },
        &Actor::Master,
    )
    .unwrap();
}
#[test]
fn evacuation_has_queues_capacity_and_conserves_people() {
    let mut w = world(scene());
    evacuate(&mut w);
    w.advance(600).unwrap();
    assert_eq!(
        w.stats().evacuated_people,
        0,
        "preparation completion is not arrival"
    );
    let mut queued = false;
    for _ in 0..400 {
        w.advance(1).unwrap();
        queued |=
            w.d.traffic
                .vehicles
                .iter()
                .any(|v| !v.completed && v.detail.contains("Queue"));
        for (id, q) in &w.d.traffic.links {
            let capacity =
                w.d.traffic
                    .vehicles
                    .iter()
                    .flat_map(|v| v.route.iter().flat_map(|r| &r.legs))
                    .find(|l| l.link == Some(*id))
                    .unwrap()
                    .storage;
            assert!(q.queue.len() <= capacity as usize);
        }
        let inside: u32 = w.d.buildings.iter().map(|b| b.people_inside).sum();
        let travelling: u32 =
            w.d.traffic
                .vehicles
                .iter()
                .filter(|v| v.departed && !v.completed)
                .map(|v| v.people)
                .sum();
        assert_eq!(inside + travelling + w.stats().evacuated_people, 120);
    }
    assert!(queued);
    assert_eq!(w.stats().evacuated_people, 120);
    assert_eq!(w.stats().sheltered, 120);
    assert!(w.d.traffic.links.values().all(|q| q.queue.is_empty()));
}
#[test]
fn traffic_is_deterministic_across_advance_sizes_and_rollback() {
    let mut a = world(scene());
    let mut b = world(scene());
    evacuate(&mut a);
    evacuate(&mut b);
    a.advance(1000).unwrap();
    for _ in 0..100 {
        b.advance(10).unwrap();
    }
    assert_eq!(
        serde_json::to_value(&a.d.traffic).unwrap(),
        serde_json::to_value(&b.d.traffic).unwrap()
    );
    let snap = a.snapshot();
    a.advance(200).unwrap();
    let next = serde_json::to_value(&a.d.traffic).unwrap();
    a.restore(&snap).unwrap();
    a.advance(200).unwrap();
    assert_eq!(serde_json::to_value(&a.d.traffic).unwrap(), next);
}
#[test]
fn unreachable_or_full_shelters_do_not_magically_evacuate() {
    let mut s = scene();
    s.buildings[1].shelter_capacity = 20;
    let mut w = world(s);
    assert!(w
        .apply(
            &Command::EvacuateBuilding {
                building: "Homes".into(),
                destination: Some("Shelter".into())
            },
            &Actor::Master
        )
        .is_err());
    let mut w = world(scene());
    evacuate(&mut w);
    for r in w.d.roads.values_mut() {
        r.closed = true;
    }
    w.advance(2000).unwrap();
    assert_eq!(w.stats().evacuated_people, 0);
    assert_eq!(w.d.buildings[0].people_inside, 120);
    assert!(w.d.traffic.vehicles.iter().all(|v| v.waiting));
}
#[test]
fn communication_outage_delays_warning_and_has_no_baseline_effect() {
    let mut s = scene();
    s.communication_sites = vec![scenario::CommunicationSite {
        id: "mast".into(),
        pos: [100.0, 100.0],
        radius_m: 500.0,
    }];
    let mut w = world(s);
    assert!(!w.d.hazard.warning_lost([100.0, 100.0]));
    w.apply(
        &Command::Ignite {
            at: Loc::Point { x: 100.0, y: 100.0 },
            radius_m: Some(40.0),
        },
        &Actor::Master,
    )
    .unwrap();
    w.advance(10).unwrap();
    assert!(w.d.hazard.sites[0].down_at_s.is_some());
    let zone = w.scn.zone_of([100.0, 100.0]).unwrap();
    w.apply(
        &Command::IssueWarning { zone: zone.clone() },
        &Actor::Master,
    )
    .unwrap();
    assert!(!w.d.warned.contains(&zone));
    assert_eq!(w.d.hazard.pending_warnings.len(), 1);
    w.advance(1200).unwrap();
    assert!(w.d.warned.contains(&zone));
}
#[test]
fn engine_deployment_consumes_inventory_and_withdraws_from_fire() {
    let mut w = world(scene());
    stage(&mut w, "engines", [100.0, 100.0]);
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 100.0, y: 100.0 },
        },
        &fire_role(),
    )
    .unwrap();
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Working);
    assert_eq!(w.d.pools["fire_commander"]["engines"].used, 1);
    w.apply(
        &Command::Ignite {
            at: Loc::Point { x: 100.0, y: 100.0 },
            radius_m: Some(40.0),
        },
        &Actor::Master,
    )
    .unwrap();
    w.advance(10).unwrap();
    assert!(matches!(
        w.d.tasks[0].status,
        TaskStatus::Withdrawing | TaskStatus::Failed
    ));
}

#[test]
fn partial_return_halves_two_crew_production() {
    let mut s = scene();
    s.grid = GridSpec {
        rows: 50,
        cols: 50,
        cellsize: 20.0,
    };
    s.fuel = vec![scenario::fuel::GRASSLAND; 2500];
    s.dem = vec![0.0; 2500];
    let mut w = world(s);
    stage(&mut w, "ground_crews", [100.0, 100.0]);
    w.apply(
        &Command::CutFireLine {
            from: Some(Loc::Point { x: 100.0, y: 100.0 }),
            to: Some(Loc::Point { x: 100.0, y: 800.0 }),
            along: None,
            width_m: Some(6.0),
        },
        &fire_role(),
    )
    .unwrap();
    w.advance(1).unwrap();
    assert_eq!(w.task_active_qty(&w.d.tasks[0]), 2);
    w.advance(600).unwrap();
    let before = w.d.tasks[0].work_m;
    w.apply(
        &Command::ReturnResource {
            allocation: w.d.tasks[0].allocations[0],
            qty: 1,
        },
        &Actor::Master,
    )
    .unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Working);
    w.advance(600).unwrap();
    let after = w.d.tasks[0].work_m;
    assert!((before - 40.0).abs() < 0.05);
    assert!((after - before - 20.0).abs() < 0.05);
}

#[test]
fn engine_integrates_finite_water_flow_and_rollback() {
    let mut w = world(scene());
    stage(&mut w, "engines", [100.0, 100.0]);
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 100.0, y: 100.0 },
        },
        &fire_role(),
    )
    .unwrap();
    w.advance(1).unwrap();
    // Prescribed, fixed heat field isolates engine consumption from stochastic
    // fire spread; the post is safe and fuel within hose reach is exposed.
    w.d.hazard.sources = vec![sim::hazard::Source {
        cell_radius_m: 14.0,
        pos: [150.0, 110.0],
        radiant_m: 20.0,
        ember_m: 0.0,
        wind: [1.0, 0.0],
    }];
    w.d.hazard.updated_s = 100000;
    w.advance(59).unwrap();
    assert!((w.d.tasks[0].used_water_l - 400.0 / 60.0 * 59.0).abs() < 0.05);
    assert!(w.fire.added_moisture().iter().any(|m| *m > 0.0));
    let checkpoint = w.snapshot();
    let expected = w.d.tasks[0].used_water_l;
    w.advance(10).unwrap();
    let used = w.d.tasks[0].used_water_l;
    w.restore(&checkpoint).unwrap();
    for _ in 0..10 {
        w.advance(1).unwrap();
    }
    assert!((w.d.tasks[0].used_water_l - used).abs() < 0.05);
    assert!(used > expected);
}
#[test]
fn approved_evacuations_reserve_shelter_capacity_together() {
    let mut s = scene();
    let mut extra = s.buildings[0].clone();
    extra.id = 3;
    extra.name = Some("Other homes".into());
    s.buildings.push(extra);
    let w = world(s);
    let commands = [
        Command::EvacuateBuilding {
            building: "Homes".into(),
            destination: Some("Shelter".into()),
        },
        Command::EvacuateBuilding {
            building: "Other homes".into(),
            destination: Some("Shelter".into()),
        },
    ];
    let verdicts = w.validate_batch(&[], &commands, &Actor::Master);
    assert!(verdicts[0].is_ok());
    assert!(!verdicts[1].is_ok());
}

#[test]
fn eu12_class_three_burns_and_lines_use_a_non_burnable_custom_id() {
    let mut s = scene();
    s.fuel_definitions =
        serde_json::from_str(include_str!("../../../scripts/factory/fuels_eu12.json")).unwrap();
    s.fuel = vec![3; 400];
    s.operations.spotting = true;
    let mut w = world(s);
    let c = scenario::Cell { row: 10, col: 10 };
    assert!(
        w.fire.is_burnable(c),
        "EU12 class 3 is grass, not a settlement"
    );
    w.fire.fire_line([10.0, 190.0], [390.0, 190.0], 30.0);
    assert!(!w.fire.is_burnable(c));
    w.fire.ignite([100.0, 300.0], 40.0).unwrap();
    w.advance(600).unwrap();
    assert!(w.fire.burned_area_ha() > 0.0);
    assert_eq!(w.fire.cell_state(c), fire::CellFire::Unburnt);
}

#[test]
fn refilled_engine_returns_to_road_post_within_hose_reach() {
    let mut w = world(scene());
    stage(&mut w, "engines", [100.0, 100.0]);
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 150.0, y: 130.0 },
        },
        &fire_role(),
    )
    .unwrap();
    assert_eq!(w.d.tasks[0].target, [150.0, 100.0]);
    w.d.tasks[0].status = TaskStatus::Refilling;
    w.d.tasks[0].target = [100.0, 100.0];
    w.d.tasks[0].refill_done_s = Some(0);
    w.d.tasks[0].water_l = 0.0;
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::EnRoute);
    assert_eq!(w.d.tasks[0].water_l, 2500.0);
    assert_eq!(w.d.tasks[0].target, [150.0, 100.0]);
}
