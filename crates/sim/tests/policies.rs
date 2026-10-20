//! Fixed hazard fields isolate approved local execution from stochastic fire spread.
use scenario::{GridSpec, Operations, Road};
use sim::{Actor, Command, Loc, TaskStatus, World};
use std::sync::Arc;

fn scene() -> scenario::Scenario {
    let mut s = scenario::synthetic::generate(7);
    s.grid = GridSpec {
        rows: 20,
        cols: 20,
        cellsize: 20.,
    };
    s.fuel = vec![scenario::fuel::GRASSLAND; 400];
    s.dem = vec![0.; 400];
    s.buildings.clear();
    s.places.clear();
    s.msel.clear();
    s.initial_ignitions.clear();
    s.roads = vec![Road {
        id: "main".into(),
        name: "Main".into(),
        points: vec![[0., 100.], [400., 100.]],
        class: "primary".into(),
        drivable: true,
        oneway: false,
    }];
    s.operations = Operations::enhanced();
    s.tactical_policy.reassess_s = 10;
    s.tactical_policy.regroup_s = 30;
    s.tactical_policy.sector_radius_m = 160.;
    s
}
fn make(s: scenario::Scenario) -> World {
    let mut w = World::with_options(
        Arc::new(s),
        1,
        sim::FireOptions {
            realizations: 1,
            cutoff: 0.5,
        },
    )
    .unwrap();
    w.apply(
        &Command::SetDeploymentTravel {
            role: "fire_commander".into(),
            resource: "engines".into(),
            at: Loc::Point { x: 100., y: 100. },
            speed_mps: 10.,
        },
        &Actor::Master,
    )
    .unwrap();
    w
}
fn deploy(w: &mut World) {
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 100., y: 100. },
        },
        &Actor::Role("fire_commander".into()),
    )
    .unwrap();
}

#[test]
fn authored_hose_reach_is_consistent_at_dispatch_and_after_physical_refill() {
    let mut s = scene();
    s.tactical_policy.hose_reach_m = 90.;
    let mut legacy = s.clone();
    legacy.operations.local_autonomy = false;
    let command = Command::DeployEngine {
        at: Loc::Point { x: 100., y: 180. },
    };
    let actor = Actor::Role("fire_commander".into());
    assert!(make(legacy).apply(&command, &actor).is_err());
    let mut w = make(s);
    w.apply(&command, &actor).unwrap();
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Working);
    assert_eq!(w.d.tasks[0].target, [100., 100.]);
    w.d.tasks[0].water_l = 0.;
    w.d.tasks[0].status = TaskStatus::Refilling;
    w.d.tasks[0].refill_done_s = Some(2);
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::EnRoute);
    assert_eq!(w.d.tasks[0].mission_centre, [100., 180.]);
    assert_eq!(w.d.tasks[0].water_l, 2500.);
}
fn source(w: &mut World, pos: [f32; 2]) {
    w.d.hazard.sources = vec![sim::hazard::Source {
        cell_radius_m: 0.,
        pos,
        radiant_m: 1.,
        ember_m: 0.,
        wind: [1., 0.],
    }];
    w.d.hazard.updated_s = 100000;
}
fn local_world() -> World {
    let mut w = make(scene());
    deploy(&mut w);
    w.advance(1).unwrap();
    source(&mut w, [250., 110.]);
    w
}

#[test]
fn local_moves_stay_inside_approved_sector_and_regroup_without_teleporting() {
    let mut w = local_world();
    w.advance(9).unwrap();
    let task = &w.d.tasks[0];
    assert_eq!(task.status, TaskStatus::EnRoute);
    let new_target = task.target;
    assert!(
        sim::network::distance(task.target, task.mission_centre)
            <= w.scn.tactical_policy.sector_radius_m
    );
    assert_eq!(task.position(w.time_s()), [100., 100.]);
    let departure = task.departed_s;
    w.advance(departure - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].position(w.time_s()), [100., 100.]);
    w.advance(1).unwrap();
    let pos = w.d.tasks[0].position(w.time_s());
    assert!(pos[0] > 100. && pos[0] < new_target[0]);
    assert_eq!(w.d.tasks[0].used_water_l, 0.);
    let arrival = w.d.tasks[0].arrival_s;
    w.advance(arrival - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].position(w.time_s()), new_target);
}

#[test]
fn inaccessible_work_outside_sector_does_not_expand_the_approved_mission() {
    let mut s = scene();
    s.tactical_policy.sector_radius_m = 40.;
    let mut w = make(s);
    deploy(&mut w);
    w.advance(1).unwrap();
    source(&mut w, [350., 110.]);
    w.advance(100).unwrap();
    let task = &w.d.tasks[0];
    assert_eq!(task.target, [100., 100.]);
    assert_eq!(task.used_water_l, 0.);
    assert!(task.policy_reason.contains("no safe reachable"));
}

#[test]
fn coordinated_pumping_stops_at_authored_wet_limit_and_charges_delivered_water_only() {
    let mut w = make(scene());
    deploy(&mut w);
    deploy(&mut w);
    w.advance(1).unwrap();
    // One threatened cell: leave < one second of useful water demand, then
    // ensure two working engines cannot each spend an entire ten-second quantum.
    source(&mut w, [130., 110.]);
    w.fire.water_drop([130., 110.], 1., 29.9 / 30.);
    let cell = w.scn.cell_of([130., 110.]).unwrap();
    let j = w.scn.grid.index(cell);
    let before = w.fire.added_moisture()[j];
    assert!(before > 29. && before < 30.);
    w.advance(8).unwrap();
    let checkpoint = w.snapshot();
    let mut without_pump = make(scene());
    without_pump.restore(&checkpoint).unwrap();
    for task in &mut without_pump.d.tasks {
        task.status = TaskStatus::Cancelled;
    }
    without_pump.advance(1).unwrap();
    let before_pump = without_pump.fire.added_moisture()[j];
    w.advance(1).unwrap();
    let after = w.fire.added_moisture()[j];
    assert!(
        after <= w.scn.tactical_policy.wet_limit_pts + 0.001,
        "moisture overshot limit: {after}"
    );
    let used: f32 = w.d.tasks.iter().map(|t| t.used_water_l).sum();
    let area = std::f32::consts::PI * (w.scn.grid.cellsize * 0.45).powi(2);
    let useful = (30. - before_pump) / fire::MOISTURE_POINTS_PER_LITRE * area;
    assert!(
        used <= useful + 0.05,
        "charged {used} L for only {useful} L demand"
    );
    assert!(used > 0.);
}

#[test]
fn withdrawal_suspends_and_resumes_same_allocation_from_physical_staging() {
    let mut w = local_world();
    // Isolate the physical return from a dangerous current post.
    w.d.hazard.sources = vec![sim::hazard::Source {
        cell_radius_m: 0.,
        pos: [100., 100.],
        radiant_m: 1.,
        ember_m: 0.,
        wind: [1., 0.],
    }];
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Withdrawing);
    let done = w.d.tasks[0].refill_done_s.unwrap();
    w.advance(done - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Suspended);
    assert_eq!(w.d.tasks[0].position(w.time_s()), [100., 100.]);
    let allocation = w.d.tasks[0].allocations.clone();
    let pool = serde_json::to_value(&w.d.pools).unwrap();
    source(&mut w, [250., 110.]);
    w.advance(10).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::EnRoute);
    assert_eq!(w.d.tasks[0].allocations, allocation);
    assert_eq!(w.d.allocations.len(), 1);
    assert_eq!(serde_json::to_value(&w.d.pools).unwrap(), pool);
    assert_eq!(w.d.tasks.len(), 1);
    assert_eq!(w.d.tasks[0].position(w.d.tasks[0].departed_s), [100., 100.]);
}

#[test]
fn local_policy_timing_water_and_position_are_step_invariant_and_rollback_safe() {
    let mut a = local_world();
    let mut b = local_world();
    a.advance(120).unwrap();
    for _ in 0..120 {
        b.advance(1).unwrap();
    }
    assert_eq!(
        serde_json::to_value(&a.d.tasks).unwrap(),
        serde_json::to_value(&b.d.tasks).unwrap()
    );
    assert_eq!(a.fire.added_moisture(), b.fire.added_moisture());
    let checkpoint = a.snapshot();
    a.advance(60).unwrap();
    let expected = serde_json::to_value(&a.d.tasks).unwrap();
    let wet = a.fire.added_moisture().to_vec();
    a.restore(&checkpoint).unwrap();
    for _ in 0..6 {
        a.advance(10).unwrap();
    }
    assert_eq!(serde_json::to_value(&a.d.tasks).unwrap(), expected);
    assert_eq!(a.fire.added_moisture(), wet);
}

#[test]
fn emergency_queue_delay_is_added_after_the_units_own_speed_limit() {
    let mut w = make(scene());
    w.d.traffic.links.insert(
        0,
        sim::traffic::LinkQueue {
            queue: vec![90, 91, 92, 93],
            next_release_s: 0.,
        },
    );
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 300., y: 100. },
        },
        &Actor::Role("fire_commander".into()),
    )
    .unwrap();
    let task = &w.d.tasks[0];
    // 200 m / 10 m/s + four primary-road FIFO discharge slots (4 / (1600/3600)).
    assert_eq!(task.arrival_s - task.departed_s, 29);
    let route = task.route.as_ref().unwrap();
    assert!((route.seconds(task.speed_mps) - 29.).abs() < 0.001);
    assert!(task.position(task.departed_s + 20)[0] < 300.);
}

#[test]
fn pending_engine_assignment_reserves_useful_work_for_other_engines() {
    let mut w = make(scene());
    deploy(&mut w);
    deploy(&mut w);
    w.advance(1).unwrap();
    source(&mut w, [250., 110.]);
    w.advance(9).unwrap();
    let covering = w
        .d
        .tasks
        .iter()
        .filter(|task| {
            sim::network::distance(task.target, [250., 110.]) <= w.scn.tactical_policy.hose_reach_m
        })
        .count();
    assert_eq!(
        covering, 1,
        "en-route destinations must reserve their work before arrival"
    );
    assert_eq!(
        w.d.tasks
            .iter()
            .filter(|task| task.status == TaskStatus::EnRoute)
            .count(),
        1
    );
    assert_eq!(w.d.tasks.len(), 2);
    assert_eq!(w.d.allocations.len(), 2);
}

#[test]
fn disabled_local_autonomy_keeps_fixed_post_behavior() {
    let mut s = scene();
    s.operations.local_autonomy = false;
    let mut w = make(s);
    deploy(&mut w);
    w.advance(1).unwrap();
    source(&mut w, [250., 110.]);
    w.advance(120).unwrap();
    assert_eq!(w.d.tasks[0].target, [100., 100.]);
    assert_eq!(w.d.tasks[0].policy_version, "fixed-post-v1");
    assert_eq!(w.d.tasks[0].used_water_l, 0.);
}

fn returning_world(detour: bool) -> World {
    let mut s = scene();
    s.roads[0].points = vec![
        [0., 100.],
        [100., 100.],
        [200., 100.],
        [300., 100.],
        [400., 100.],
    ];
    if detour {
        s.roads.push(Road {
            id: "detour".into(),
            name: "Detour".into(),
            points: vec![[200., 100.], [200., 300.], [100., 300.], [100., 100.]],
            class: "primary".into(),
            drivable: true,
            oneway: false,
        });
    }
    let mut w = make(s);
    w.apply(
        &Command::DeployEngine {
            at: Loc::Point { x: 300., y: 100. },
        },
        &Actor::Role("fire_commander".into()),
    )
    .unwrap();
    let arrival = w.d.tasks[0].arrival_s;
    w.advance(arrival).unwrap();
    source(&mut w, [300., 100.]);
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Withdrawing);
    w
}
#[test]
fn returning_and_withdrawing_units_reroute_before_crossing_new_fire_cut() {
    for status in [TaskStatus::Withdrawing, TaskStatus::Refilling] {
        let mut w = returning_world(true);
        w.d.tasks[0].status = status;
        let before = w.d.tasks[0].position(w.time_s());
        let old_arrival = w.d.tasks[0].arrival_s;
        let state = w.d.roads.get_mut("main").unwrap();
        state.cut_by_fire = true;
        state.fire_closed_links = Some(vec![2, 3]);
        w.advance(1).unwrap();
        let task = &w.d.tasks[0];
        assert_eq!(task.status, status);
        assert_eq!(
            task.position(w.time_s()),
            before,
            "rerouting must start at the last physical position"
        );
        assert!(task.arrival_s > old_arrival);
        assert!(task
            .route
            .as_ref()
            .unwrap()
            .legs
            .iter()
            .any(|leg| leg.road.as_deref() == Some("detour")));
        assert!(w
            .network
            .route_open(task.route.as_ref().unwrap(), &w.d.roads, false));
        let arrival = task.arrival_s;
        w.advance(arrival - w.time_s()).unwrap();
        assert_eq!(w.d.tasks[0].position(w.time_s()), [100., 100.]);
    }
}
#[test]
fn unreachable_return_stays_at_reached_position_and_does_not_arrive_by_clock() {
    let mut w = returning_world(false);
    let before = w.d.tasks[0].position(w.time_s());
    let state = w.d.roads.get_mut("main").unwrap();
    state.cut_by_fire = true;
    state.fire_closed_links = Some(vec![2, 3]);
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Failed);
    assert_eq!(w.d.tasks[0].position(w.time_s()), before);
    w.advance(300).unwrap();
    assert_eq!(w.d.tasks[0].position(w.time_s()), before);
    assert_eq!(w.d.tasks[0].water_l, 2500.);
}
#[test]
fn fire_cut_behind_returning_unit_does_not_invalidate_remaining_route() {
    let mut w = returning_world(false);
    w.d.hazard.sources.clear();
    w.advance(12).unwrap();
    let arrival = w.d.tasks[0].arrival_s;
    let state = w.d.roads.get_mut("main").unwrap();
    state.cut_by_fire = true;
    state.fire_closed_links = Some(vec![4, 5]);
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Withdrawing);
    assert_eq!(w.d.tasks[0].arrival_s, arrival);
    assert!(w.d.tasks[0].position(w.time_s())[0] < 200.);
}

#[test]
fn crew_regroups_and_resumes_exact_approved_line_after_partial_return() {
    let mut s = scene();
    s.grid = GridSpec {
        rows: 50,
        cols: 50,
        cellsize: 20.,
    };
    s.fuel = vec![scenario::fuel::GRASSLAND; 2500];
    s.dem = vec![0.; 2500];
    let mut w = make(s);
    w.apply(
        &Command::SetDeploymentTravel {
            role: "fire_commander".into(),
            resource: "ground_crews".into(),
            at: Loc::Point { x: 100., y: 100. },
            speed_mps: 2.,
        },
        &Actor::Master,
    )
    .unwrap();
    w.apply(
        &Command::CutFireLine {
            from: Some(Loc::Point { x: 100., y: 100. }),
            to: Some(Loc::Point { x: 100., y: 800. }),
            along: None,
            width_m: Some(6.),
        },
        &Actor::Role("fire_commander".into()),
    )
    .unwrap();
    w.advance(601).unwrap();
    let earned = w.d.tasks[0].work_m;
    assert!((earned - 40.).abs() < 0.01);
    let position = w.d.tasks[0].position(w.time_s());
    source(&mut w, position);
    w.advance(1).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Withdrawing);
    let departure = w.d.tasks[0].departed_s;
    w.advance(departure - w.time_s()).unwrap();
    assert_eq!(
        w.d.tasks[0].position(w.time_s()),
        position,
        "crew must regroup at earned progress before travel"
    );
    let returned = w.d.tasks[0].refill_done_s.unwrap();
    w.advance(returned - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Suspended);
    assert_eq!(w.d.tasks[0].position(w.time_s()), [100., 100.]);
    w.apply(
        &Command::ReturnResource {
            allocation: w.d.tasks[0].allocations[0],
            qty: 1,
        },
        &Actor::Master,
    )
    .unwrap();
    assert_eq!(w.task_active_qty(&w.d.tasks[0]), 1);
    let allocations = serde_json::to_value(&w.d.allocations).unwrap();
    w.d.hazard.sources.clear();
    w.advance(10).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::EnRoute);
    assert_eq!(w.d.tasks[0].target, position);
    assert_eq!(w.d.tasks[0].work_to, Some([100., 800.]));
    let arrival = w.d.tasks[0].arrival_s;
    w.advance(arrival - w.time_s()).unwrap();
    assert_eq!(w.d.tasks[0].status, TaskStatus::Working);
    assert_eq!(w.d.tasks[0].position(w.time_s()), position);
    w.advance(60).unwrap();
    assert!(
        (w.d.tasks[0].work_m - earned - 2.).abs() < 0.01,
        "only retained crew earns further progress"
    );
    assert_eq!(serde_json::to_value(&w.d.allocations).unwrap(), allocations);
    assert_eq!(w.d.tasks[0].mission_centre, [100., 100.]);
}
