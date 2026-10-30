//! World behaviour: validation, commands, buildings, resources, rollback.
use std::sync::Arc;

use sim::{Actor, Area, Command, Damage, Loc, Verdict, World};

fn world() -> World {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut w = World::new(scn, 1).unwrap();
    let (x, y) = (scenario::synthetic::IGNITION[0], scenario::synthetic::IGNITION[1]);
    let fire = Command::Ignite { at: Loc::Point { x, y }, radius_m: Some(scenario::synthetic::IGNITION_RADIUS_M) };
    w.apply(&fire, &Actor::Master).unwrap();
    w
}

fn role(r: &str) -> Actor {
    Actor::Role(r.into())
}

fn school(w: &World) -> usize {
    w.find_building("Pineta Primary School").unwrap()
}

#[test]
fn role_authority_is_enforced() {
    let w = world();
    let drop = Command::WaterDrop { at: Loc::Named("F3".into()) };
    assert!(w.validate(&drop, &role("fire_commander")).is_ok());
    let v = w.validate(&drop, &role("mayor"));
    assert!(matches!(v, Verdict::Rejected { ref reason } if reason.contains("not authorised")), "{v:?}");
    // EXCON-only commands are refused to every player, allowed to the EXCON.
    let ext = Command::Extinguish { area: Area::Named("G3".into()) };
    assert!(!w.validate(&ext, &role("fire_commander")).is_ok());
    assert!(w.validate(&ext, &Actor::Master).is_ok());
}

#[test]
fn resources_limit_a_batch() {
    let w = world();
    let drop = Command::WaterDrop { at: Loc::Named("G3".into()) };
    // One air tanker: the second drop in the same turn is rejected.
    let v = w.validate_batch(&[], &[drop.clone(), drop.clone()], &role("fire_commander"));
    assert!(v[0].is_ok());
    assert!(matches!(&v[1], Verdict::Rejected { reason } if reason.contains("air_tankers")));
    // Already-approved commands count too.
    let v = w.validate_batch(&[(role("fire_commander"), drop.clone())], &[drop], &role("fire_commander"));
    assert!(!v[0].is_ok());
}

#[test]
fn fire_line_crews_scale_with_length() {
    let w = world();
    // 3 crews x 600 m: a 2 km line is too long, one zone edge (500 m) is fine.
    let long = Command::CutFireLine { from: Some(Loc::Point { x: 1000.0, y: 2500.0 }), to: Some(Loc::Point { x: 3000.0, y: 2500.0 }), along: None, width_m: None };
    assert!(!w.validate(&long, &role("fire_commander")).is_ok());
    let edge = Command::CutFireLine { from: None, to: None, along: Some("G3:S".into()), width_m: None };
    assert!(w.validate(&edge, &role("fire_commander")).is_ok());
    let bad = Command::CutFireLine { from: None, to: None, along: Some("Z9:S".into()), width_m: None };
    assert!(!w.validate(&bad, &Actor::Master).is_ok());
}

#[test]
fn unknown_places_are_rejected_not_guessed() {
    let w = world();
    let c = Command::WaterDrop { at: Loc::Named("somewhere over there".into()) };
    assert!(matches!(w.validate(&c, &role("fire_commander")), Verdict::Rejected { reason } if reason.contains("unknown place")));
}

#[test]
fn school_evacuation_needs_buses_and_takes_time() {
    let mut w = world();
    let i = school(&w);
    let c = Command::EvacuateBuilding { building: "Pineta Primary School".into(), destination: None };
    // 120 pupils -> 3 buses; the mayor has 2.
    assert!(matches!(w.validate(&c, &role("mayor")), Verdict::Rejected { reason } if reason.contains("buses")));
    w.apply(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }, &Actor::Master).unwrap();
    w.apply(&c, &role("mayor")).unwrap();
    assert_eq!(w.d.pools["mayor"]["buses"].used, 3);
    assert!(w.d.buildings[i].evac_done_at.is_none(),"buses are still travelling");
    w.advance(w.d.tasks[0].arrival_s-w.time_s()).unwrap();
    assert!(w.d.buildings[i].evac_done_at.is_some());
    w.advance(10 * 60).unwrap();
    assert_eq!(w.d.buildings[i].evacuated, 0, "school emptied too fast");
    w.advance(25 * 60).unwrap();
    assert_eq!(w.d.buildings[i].people_inside, 0);
    assert_eq!(w.d.buildings[i].evacuated, 120);
}

#[test]
fn warning_is_persistent_timed_visible_and_rolled_back() {
    let mut w = world();
    w.advance(5 * 60).unwrap();
    let before = w.snapshot();
    let out = w.apply(&Command::IssueWarning { zone: "f4".into() }, &role("mayor")).unwrap();
    // A readable effect line with time, population and consequence.
    let line = out.join(" ");
    assert!(line.contains("population of F4 warned at") && line.contains(&w.clock_at(300)), "{line}");
    assert!(line.contains("people in") && line.contains("10 min instead of 20"), "{line}");
    assert!(w.d.events.iter().any(|e| e.kind == "warning" && e.zone.as_deref() == Some("F4") && e.text.contains("warned at")));
    assert_eq!(w.d.warned.get("F4"), Some(&300));
    // Visible to the UIs and the AI.
    let warnings = w.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!((warnings[0].zone.as_str(), warnings[0].t_s), ("F4", 300));
    assert!(warnings[0].people_inside > 0);
    let view = w.view();
    assert_eq!(view["warned"], serde_json::json!(["F4"]));
    assert_eq!(view["warnings"][0]["t_s"], 300);
    let digest = w.digest(None);
    assert_eq!(digest["warnings_in_force"][0]["zone"], "F4");
    assert!(digest["warnings_in_force"][0]["effect"].as_str().unwrap().contains("10 min instead of 20"));
    assert!(digest["populated_zones"].as_array().unwrap().iter().any(|z| z["zone"] == "F4" && z["warned_at"] == serde_json::json!(w.clock_at(300))));
    // Repeating keeps the original time.
    w.advance(60).unwrap();
    let again = w.apply(&Command::IssueWarning { zone: "F4".into() }, &Actor::Master).unwrap();
    assert!(again[0].contains("warned since"), "{again:?}");
    assert_eq!(w.d.warned.get("F4"), Some(&300));
    // Rollback removes it.
    w.restore(&before).unwrap();
    assert!(w.d.warned.is_empty());
    assert!(w.warnings().is_empty());
}

#[test]
fn evacuate_zone_by_code_with_warning_and_shelter() {
    let mut w = world();
    w.apply(&Command::OpenShelter { building: "Valverde Sports Centre".into() }, &role("mayor")).unwrap();
    w.apply(&Command::IssueWarning { zone: "f4".into() }, &role("mayor")).unwrap();
    // F4 holds the school: the EXCON is not bound by buses.
    let people_before: u32 = w.d.buildings.iter().map(|b| b.people_inside).sum();
    w.apply(&Command::EvacuateZone { zone: "F4".into(), destination: Some("Valverde Sports Centre".into()) }, &Actor::Master).unwrap();
    w.advance(11 * 60).unwrap(); // warned: 10 min
    let people_after: u32 = w.d.buildings.iter().map(|b| b.people_inside).sum();
    assert!(people_after < people_before);
    let sheltered: u32 = w.d.shelters.values().sum();
    assert_eq!(sheltered, people_before - people_after);
    assert!(w.d.events.iter().any(|e| e.kind == "evacuated"));
}

#[test]
fn evacuating_an_empty_zone_is_rejected() {
    let w = world();
    assert!(!w.validate(&Command::EvacuateZone { zone: "A1".into(), destination: None }, &role("mayor")).is_ok());
    assert!(!w.validate(&Command::EvacuateZone { zone: "Q7".into(), destination: None }, &role("mayor")).is_ok());
}

#[test]
fn unchecked_fire_damages_pineta_and_flags_people_at_risk() {
    let mut w = world();
    let ev = w.advance(150 * 60).unwrap();
    let s = w.stats();
    assert!(s.damaged + s.destroyed > 0, "no building damaged after 2.5 h: {s:?}");
    assert!(s.people_at_risk > 0);
    assert!(ev.iter().any(|e| e.kind == "threatened"));
}

#[test]
fn extinguish_zone_and_rollback() {
    let mut w = world();
    w.advance(20 * 60).unwrap();
    let snap = w.snapshot();
    let burning = w.burning_zones();
    assert!(!burning.is_empty());
    for z in &burning {
        w.apply(&Command::Extinguish { area: Area::Named(z.clone()) }, &Actor::Master).unwrap();
    }
    assert_eq!(w.stats().burning_cells, 0);
    w.restore(&snap).unwrap();
    assert!(w.stats().burning_cells > 0);
    assert_eq!(w.time_s(), 20 * 60);
}

#[test]
fn extinguish_drawn_area_keeps_fuel_and_rolls_back() {
    let mut w = world();
    w.advance(20 * 60).unwrap();
    let snap = w.snapshot();
    let veg = w.fire.vegetation().to_vec();
    let [x, y] = scenario::synthetic::IGNITION;
    let pt = |dx: f32, dy: f32| Loc::Point { x: x + dx, y: y + dy };
    // A box around the fire that stays on the 4 × 4 km map.
    let area = Command::ExtinguishArea { polygon: vec![pt(-1500., -1500.), pt(800., -1500.), pt(800., 1200.), pt(-1500., 1200.)] };
    // EXCON only, and a polygon needs a surface.
    assert!(!w.validate(&area, &role("fire_commander")).is_ok());
    assert!(w.validate(&area, &Actor::Master).is_ok(), "{:?}", w.validate(&area, &Actor::Master));
    let flat = Command::ExtinguishArea { polygon: vec![pt(0., 0.), pt(100., 0.), pt(200., 0.)] };
    assert!(!w.validate(&flat, &Actor::Master).is_ok());
    assert!(!w.validate(&Command::ExtinguishArea { polygon: vec![pt(0., 0.), pt(100., 0.)] }, &Actor::Master).is_ok());
    assert!(w.command_geometry(&area)["nominal_area_ha"].as_f64().unwrap() > 600.0);

    let notes = w.apply(&area, &Actor::Master).unwrap();
    assert!(notes.iter().any(|n| n.contains("fuel unchanged")), "{notes:?}");
    assert_eq!(w.stats().burning_cells, 0);
    w.advance(30 * 60).unwrap();
    assert_eq!(w.stats().burning_cells, 0);
    assert_eq!(w.fire.vegetation(), &veg[..]);
    assert!(w.fire.cleared().iter().all(|c| !c));
    w.restore(&snap).unwrap();
    assert!(w.stats().burning_cells > 0);
    assert_eq!(w.time_s(), 20 * 60);
}

#[test]
fn reinforcements_arrive_after_delay_and_turns_preserve_deployments() {
    let mut w = world();
    let req = Command::RequestReinforcement { resource: "ground_crews".into(), qty: 2 };
    w.apply(&req, &role("regional_cp")).unwrap();
    // Offer max is 3: asking 2 more is refused.
    assert!(!w.validate(&req, &role("regional_cp")).is_ok());
    w.apply(&Command::CutFireLine { from: None, to: None, along: Some("G3:S".into()), width_m: None }, &role("fire_commander")).unwrap();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used, 1);
    w.advance(61 * 60).unwrap();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].total, 5);
    w.new_turn();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used, 1);
}

#[test]
fn master_can_set_building_status_by_zone() {
    let mut w = world();
    w.apply(&Command::SetBuildingStatus { building: "F4".into(), damage: Some(Damage::Destroyed), evacuated: Some(true) }, &Actor::Master).unwrap();
    let i = school(&w);
    assert_eq!(w.d.buildings[i].damage, Damage::Destroyed);
    assert_eq!(w.d.buildings[i].people_inside, 0);
}

#[test]
fn weather_and_time_skip() {
    let mut w = world();
    w.apply(&Command::SetWeather { wind_dir_deg: Some(180.0), wind_speed_kmh: None, moisture_pct: Some(8.0) }, &Actor::Master).unwrap();
    assert_eq!(w.fire.weather().wind_dir_deg, 180.0);
    w.apply(&Command::AdvanceTime { minutes: 45 }, &Actor::Master).unwrap();
    assert_eq!(w.time_s(), 45 * 60);
    assert_eq!(w.clock(), "Day 1 13:45");
}

#[test]
fn digest_and_view_are_well_formed() {
    let w = world();
    let d = w.digest(Some("mayor"));
    assert!(d["resources_this_turn"]["mayor"]["buses"]["total"].as_u64() == Some(2));
    assert!(d["resources_this_turn"].get("fire_commander").is_none());
    assert!(!d["facilities"].as_array().unwrap().is_empty());
    let v = w.view();
    assert_eq!(v["buildings"].as_array().unwrap().len(), w.scn.buildings.len());
    use base64::Engine;
    let raster = base64::engine::general_purpose::STANDARD.decode(v["fire"].as_str().unwrap()).unwrap();
    assert_eq!(raster.len(), w.scn.grid.len());
    assert!(w.gazetteer()["map"].as_str().unwrap().contains("A1"));
}

#[test]
fn allocations_persist_and_returns_cannot_duplicate_or_undo_physical_work() {
    let mut w = world();
    let cmd = Command::CutFireLine { from: None, to: None, along: Some("G3:S".into()), width_m: None };
    w.apply(&cmd, &role("fire_commander")).unwrap();
    let allocation = w.d.allocations[0].id;
    assert!(w.d.lines.is_empty(),"travel has not completed");
    w.advance(w.d.tasks[0].arrival_s-w.time_s()).unwrap();
    let line_count = w.d.lines.len();
    assert_eq!(w.d.allocations[0].qty, 1);
    let deployed = w.snapshot();
    w.new_turn();
    w.advance(60).unwrap();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used, 1);
    let release = Command::ReturnResource { allocation, qty: 1 };
    assert!(w.apply(&release, &role("fire_commander")).is_err(), "returns require EXCON confirmation");
    assert!(w.apply(&Command::AdjustResource { role:"fire_commander".into(), resource:"ground_crews".into(), delta:-3 }, &Actor::Master).is_err());
    w.apply(&release, &Actor::Master).unwrap();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used, 0);
    assert_eq!(w.d.allocations[0].returned, 1);
    assert_eq!(w.d.lines.len(), line_count);
    assert!(w.apply(&release, &Actor::Master).is_err());
    assert_eq!(w.d.resource_events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), vec!["deployed", "returned"]);
    w.restore(&deployed).unwrap();
    assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used, 1);
    assert_eq!(w.d.allocations[0].returned, 0);
    w.apply(&cmd, &role("fire_commander")).unwrap();
    assert!(w.d.allocations[1].id > allocation, "rollback cannot reuse allocation identity");
}

#[test]
fn batch_reinforcements_and_returns_respect_existing_reservations() {
    let mut w = world();
    let req = Command::RequestReinforcement { resource:"ground_crews".into(), qty:2 };
    let verdicts = w.validate_batch(&[], &[req.clone(), req.clone()], &role("regional_cp"));
    assert!(verdicts[0].is_ok());
    assert!(!verdicts[1].is_ok());
    let prior = vec![(role("regional_cp"), req.clone())];
    assert!(!w.validate_batch(&prior, std::slice::from_ref(&req), &role("regional_cp"))[0].is_ok());
    w.apply(&req, &role("regional_cp")).unwrap();
    w.advance(61 * 60).unwrap();
    assert!(w.d.resource_events.iter().any(|e| e.kind == "requested"));
    assert!(w.d.resource_events.iter().any(|e| e.kind == "arrived"));
    assert!(!w.validate(&req, &role("regional_cp")).is_ok(), "arrived reinforcements still count against total offer");
    w.apply(&Command::CutFireLine { from:None, to:None, along:Some("G3:S".into()), width_m:None }, &role("fire_commander")).unwrap();
    let release = Command::ReturnResource { allocation:w.d.allocations[0].id, qty:1 };
    let verdicts = w.validate_batch(&[], &[release.clone(), release], &Actor::Master);
    assert!(verdicts[0].is_ok());
    assert!(!verdicts[1].is_ok());
}

#[test]
fn physical_geometry_reports_area_units_and_rejects_invalid_ignition_scale() {
 let w=world();
 let c=Command::Ignite {at:Loc::Named("Pine Hill".into()),radius_m:Some(100.0)};
 let g=w.command_geometry(&c);
 assert!((g["nominal_area_ha"].as_f64().unwrap()-std::f64::consts::PI).abs()<1e-6);
 assert_eq!(g["radius_m"],100.0);
 for r in [0.0,-80.0,f32::NAN,f32::INFINITY,100_000.0] {
  assert!(!w.validate(&Command::Ignite {at:Loc::Named("Pine Hill".into()),radius_m:Some(r)},&Actor::Master).is_ok());
 }
 assert!(!w.validate(&Command::Ignite {at:Loc::Point {x:f32::NAN,y:100.0},radius_m:Some(80.0)},&Actor::Master).is_ok());
 let zone=w.scn.zone_bounds("F4").unwrap();
 let area=(zone[2]-zone[0])*(zone[3]-zone[1])/10000.0;
 assert_eq!(w.command_geometry(&Command::Extinguish {area:Area::Named("F4".into())})["nominal_area_ha"],serde_json::json!(area));
}

#[test]
fn deployed_travel_waits_for_arrival_and_remains_consistent_with_resources() {
 let mut w=world();
 w.apply(&Command::SetDeploymentTravel {role:"fire_commander".into(),resource:"ground_crews".into(),at:Loc::Point{x:1000.0,y:1000.0},speed_mps:10.0},&Actor::Master).unwrap();
 let cmd=Command::CutFireLine {from:Some(Loc::Point{x:2000.0,y:1000.0}),to:Some(Loc::Point{x:2500.0,y:1000.0}),along:None,width_m:None};
 w.apply(&cmd,&role("fire_commander")).unwrap();
 let task=w.d.tasks[0].clone();
 assert_eq!((task.departed_s,task.arrival_s),(0,100));
 assert_eq!(task.status,sim::TaskStatus::EnRoute);
 assert!(w.d.lines.is_empty());assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used,1);
 w.advance(50).unwrap();assert_eq!(task.position(w.time_s()),[1500.0,1000.0]);assert!(w.d.lines.is_empty());
 // Changing future staging settings cannot teleport an already dispatched task.
 w.apply(&Command::SetDeploymentTravel {role:"fire_commander".into(),resource:"ground_crews".into(),at:Loc::Named("Pine Hill".into()),speed_mps:100.0},&Actor::Master).unwrap();
 w.advance(49).unwrap();assert!(w.d.lines.is_empty());
 w.advance(1).unwrap();assert_eq!(w.d.lines.len(),1);assert_eq!(w.d.lines[0].by,"fire_commander");
 assert_eq!(w.d.tasks[0].arrived_s,Some(100));assert_eq!(w.d.tasks[0].completed_s,Some(100));assert_eq!(w.d.tasks[0].status,sim::TaskStatus::Completed);
 assert_eq!(w.d.allocations.len(),1);assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used,1);
 w.advance(10).unwrap();assert_eq!(w.d.lines.len(),1,"arrival cannot apply twice");
 w.apply(&Command::ReturnResource {allocation:task.id,qty:1},&Actor::Master).unwrap();
 assert_eq!(w.task_active_qty(&w.d.tasks[0]),0);assert_eq!(w.d.lines.len(),1,"return keeps completed work");
}

#[test]
fn cancelling_enroute_deployment_releases_units_without_applying_task() {
 let mut w=world();
 let cmd=Command::CutFireLine {from:Some(Loc::Point{x:1000.0,y:2000.0}),to:Some(Loc::Point{x:2100.0,y:2000.0}),along:None,width_m:None};
 w.apply(&cmd,&role("fire_commander")).unwrap();let task=w.d.tasks[0].clone();
 assert_eq!(task.qty,2);
 assert!(w.apply(&Command::ReturnResource {allocation:task.id,qty:1},&Actor::Master).is_err());
 let checkpoint=w.snapshot();
 w.apply(&Command::ReturnResource {allocation:task.id,qty:2},&Actor::Master).unwrap();
 assert_eq!(w.d.tasks[0].status,sim::TaskStatus::Cancelled);assert_eq!(w.d.pools["fire_commander"]["ground_crews"].used,0);
 w.advance(task.arrival_s+1).unwrap();assert!(w.d.lines.is_empty());
 w.restore(&checkpoint).unwrap();assert_eq!(w.d.tasks[0].status,sim::TaskStatus::EnRoute);assert_eq!(w.task_active_qty(&w.d.tasks[0]),2);
 w.advance(task.arrival_s).unwrap();assert_eq!(w.d.lines.len(),1);
}

#[test]
fn task_arrival_failure_and_evacuation_working_state_are_truthful() {
 let mut w=world();w.apply(&Command::AdjustResource {role:"mayor".into(),resource:"buses".into(),delta:1},&Actor::Master).unwrap();
 let cmd=Command::EvacuateBuilding {building:"Pineta Primary School".into(),destination:None};w.apply(&cmd,&role("mayor")).unwrap();let task=w.d.tasks[0].clone();
 assert!(w.apply(&cmd,&role("mayor")).is_err(),"duplicate dispatch refused");
 w.advance(task.arrival_s).unwrap();assert_eq!(w.d.tasks[0].status,sim::TaskStatus::Working);
 assert!(w.apply(&Command::ReturnResource {allocation:task.id,qty:3},&Actor::Master).is_err());
 w.advance(30*60).unwrap();assert_eq!(w.d.tasks[0].status,sim::TaskStatus::Completed);assert_eq!(w.d.tasks[0].completed_s,Some(task.arrival_s+30*60));
 let mut failed=world();failed.apply(&Command::AdjustResource {role:"mayor".into(),resource:"buses".into(),delta:1},&Actor::Master).unwrap();failed.apply(&cmd,&role("mayor")).unwrap();let due=failed.d.tasks[0].arrival_s;
 failed.apply(&Command::SetBuildingStatus {building:"Pineta Primary School".into(),damage:None,evacuated:Some(true)},&Actor::Master).unwrap();failed.advance(due).unwrap();assert_eq!(failed.d.tasks[0].status,sim::TaskStatus::Failed);
 assert_eq!(failed.d.pools["mayor"]["buses"].used,3,"failed task requires explicit return");
}

#[test]
fn tanker_and_patrol_effects_begin_on_arrival_without_double_allocation() {
 let mut w=world();
 let drop=Command::WaterDrop {at:Loc::Named("G3".into())};
 w.apply(&drop,&role("fire_commander")).unwrap();assert!(w.d.drops.is_empty());let due=w.d.tasks[0].arrival_s;
 w.advance(due).unwrap();assert_eq!(w.d.drops.len(),1);assert_eq!(w.d.tasks[0].status,sim::TaskStatus::Completed);assert_eq!(w.d.drops[0].by,"fire_commander");assert_eq!(w.d.allocations.len(),1);
 let road=w.scn.roads[0].id.clone();w.apply(&Command::CloseRoad {road:road.clone()},&role("mayor")).unwrap();assert!(!w.d.roads[&road].closed);
 let due=w.d.tasks[1].arrival_s;w.advance(due-w.time_s()).unwrap();assert!(w.d.roads[&road].closed);assert_eq!(w.d.tasks[1].status,sim::TaskStatus::Completed);assert_eq!(w.d.pools["mayor"]["police_patrols"].used,1);
 let mut direct=world();direct.apply(&drop,&Actor::Master).unwrap();assert_eq!(direct.d.drops.len(),1);assert!(direct.d.tasks.is_empty());assert!(direct.d.allocations.is_empty(),"EXCON intervention cannot fabricate deployed teams");
}

#[test]
fn building_given_as_a_zone_is_adapted_or_explained() {
    let w = world();
    // The playtest case: a school typed where a zone is expected.
    let bad = Command::EvacuateZone { zone: "Pineta Primary School".into(), destination: None };
    let v = w.validate(&bad, &role("mayor"));
    let Verdict::Rejected { reason } = v else { panic!("direct validation must still reject") };
    assert!(reason.contains("is a building, not a zone") && reason.contains("Evacuate building") && reason.contains("Pineta Primary School"), "{reason}");
    let (fixed, note) = w.reconcile_command(&bad);
    assert!(matches!(&fixed, Command::EvacuateBuilding { building, .. } if building == "Pineta Primary School"), "{fixed:?}");
    assert!(note.unwrap().contains("treated as Evacuate building"));
    assert!(!matches!(w.validate(&fixed, &role("mayor")), Verdict::Rejected { ref reason } if reason.contains("unknown")));
}

#[test]
fn named_place_given_as_a_zone_maps_to_the_zone_where_it_lies() {
    let w = world();
    let (fixed, note) = w.reconcile_command(&Command::EvacuateZone { zone: "Pineta".into(), destination: None });
    let Command::EvacuateZone { zone, .. } = &fixed else { panic!("{fixed:?}") };
    assert!(w.zone_code(zone).is_some() && note.unwrap().contains("named place"));
    let (fixed, _) = w.reconcile_command(&Command::IssueWarning { zone: "Pineta Primary School".into() });
    assert!(matches!(fixed, Command::IssueWarning { ref zone } if w.zone_code(zone).is_some()), "{fixed:?}");
    // A zone code given as a building is read as that zone.
    let (fixed, _) = w.reconcile_command(&Command::EvacuateBuilding { building: "F4".into(), destination: None });
    assert!(matches!(fixed, Command::EvacuateZone { ref zone, .. } if zone == "F4"));
}

#[test]
fn unmatched_names_get_suggestions() {
    let w = world();
    let v = w.validate(&Command::EvacuateZone { zone: "Pineta Primry Skool".into(), destination: None }, &role("mayor"));
    let Verdict::Rejected { reason } = v else { panic!() };
    assert!(reason.contains("grid code") && reason.contains("did you mean") && reason.contains("Pineta"), "{reason}");
    let v = w.validate(&Command::EvacuateBuilding { building: "F4".into(), destination: None }, &role("mayor"));
    let Verdict::Rejected { reason } = v else { panic!() };
    assert!(reason.contains("is a zone"), "{reason}");
}

/// Burning cells of the "actual" fire among `cells`.
fn burning_in(w: &World, cells: &[scenario::Cell]) -> usize {
    cells.iter().filter(|c| w.fire.cell_state(**c) == fire::CellFire::Burning).count()
}

#[test]
fn extinguish_zone_puts_out_the_whole_zone_and_keeps_fuel() {
    // Playtest: "extinguish zone" acted like a single cell and wiped the
    // vegetation. A zone puts out every burning cell inside it, like a drawn area.
    let mut w = world();
    w.advance(25 * 60).unwrap();
    let zone = w.scn.zone_of(scenario::synthetic::IGNITION).unwrap();
    let b = w.scn.zone_bounds(&zone).unwrap();
    let cells = w.fire.cells_in_rect(b);
    let inside = burning_in(&w, &cells);
    assert!(inside > 1, "need a fire spread over the zone, got {inside} burning cells");
    let veg = w.fire.vegetation().to_vec();
    let cmd = Command::Extinguish { area: Area::Named(zone.to_lowercase()) };
    assert_eq!(w.command_geometry(&cmd)["bounds_m"], serde_json::json!(b));
    let notes = w.apply(&cmd, &Actor::Master).unwrap();
    assert!(notes.iter().any(|n| n.contains(&format!("{inside} burning cells put out")) && n.contains("fuel unchanged")), "{notes:?}");
    assert_eq!(burning_in(&w, &cells), 0);
    assert_eq!(w.fire.vegetation(), &veg[..], "Extinguish must not change the vegetation");
    w.advance(10 * 60).unwrap();
    assert_eq!(w.fire.vegetation(), &veg[..], "no deferred fuel change either");
}

#[test]
fn extinguish_named_place_covers_its_footprint_and_keeps_fuel() {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    let at = scenario::synthetic::IGNITION;
    scn.places.push(scenario::Place { name: "Burn Scar".into(), pos: at, radius_m: Some(600.0), ..Default::default() });
    let mut w = World::new(Arc::new(scn), 1).unwrap();
    let (x, y) = (at[0], at[1]);
    w.apply(&Command::Ignite { at: Loc::Point { x, y }, radius_m: Some(scenario::synthetic::IGNITION_RADIUS_M) }, &Actor::Master).unwrap();
    w.advance(60 * 60).unwrap();
    let disc = w.fire.cells_in_radius(at, 600.0);
    let near = w.fire.cells_in_radius(at, sim::PLACE_EXTINGUISH_RADIUS_M);
    // The place's own radius matters: fire burns beyond the default 250 m.
    assert!(burning_in(&w, &disc) > burning_in(&w, &near), "fire should reach past 250 m");
    let veg = w.fire.vegetation().to_vec();
    let cmd = Command::Extinguish { area: Area::Named("burn scar".into()) };
    assert_eq!(w.command_geometry(&cmd)["radius_m"], serde_json::json!(600.0));
    w.apply(&cmd, &Actor::Master).unwrap();
    assert_eq!(burning_in(&w, &disc), 0);
    assert_eq!(w.fire.vegetation(), &veg[..]);
    // A gazetteer place without a radius keeps the 250 m default.
    let pine = w.command_geometry(&Command::Extinguish { area: Area::Named("Pine Hill".into()) });
    assert_eq!(pine["radius_m"], serde_json::json!(sim::PLACE_EXTINGUISH_RADIUS_M));
    assert!(!w.validate(&Command::Extinguish { area: Area::Named("Nowhere at all".into()) }, &Actor::Master).is_ok());
}

#[test]
fn extinguish_circle_keeps_fuel() {
    let mut w = world();
    w.advance(20 * 60).unwrap();
    let veg = w.fire.vegetation().to_vec();
    let [x, y] = scenario::synthetic::IGNITION;
    w.apply(&Command::Extinguish { area: Area::Circle { at: Loc::Point { x, y }, radius_m: 2000.0 } }, &Actor::Master).unwrap();
    assert_eq!(w.stats().burning_cells, 0);
    w.advance(10 * 60).unwrap();
    assert_eq!(w.fire.vegetation(), &veg[..]);
}

#[test]
fn static_view_marks_ground_below_sea_level_as_water() {
    use base64::Engine;
    let dry = World::new(Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED)), 1).unwrap();
    assert!(dry.static_view()["water"].is_null(), "no sea, no mask");
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    let cols = scn.grid.cols;
    // The whole first row and one cell of the second row below sea level.
    for i in (0..cols).chain([cols + 9]) {
        scn.dem[i] = -2.0;
    }
    let w = World::new(Arc::new(scn), 1).unwrap();
    let bits = base64::engine::general_purpose::STANDARD
        .decode(w.static_view()["water"].as_str().unwrap())
        .unwrap();
    let wet = |i: usize| bits[i / 8] >> (i % 8) & 1 == 1;
    assert_eq!(bits.len(), (cols * w.scn.grid.rows).div_ceil(8));
    assert!((0..cols).all(wet) && wet(cols + 9));
    assert!(!wet(cols) && !wet(cols + 8) && !wet(cols + 10));
}
