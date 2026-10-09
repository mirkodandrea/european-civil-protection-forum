//! World behaviour: validation, commands, buildings, resources, rollback.
use std::sync::Arc;

use sim::{Actor, Area, Command, Damage, Loc, Verdict, World};

fn world() -> World {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    World::new(scn, 1).unwrap()
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
    // Master-only commands are refused to every player, allowed to the Master.
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
    assert!(w.d.buildings[i].evac_done_at.is_some());
    w.advance(10 * 60).unwrap();
    assert_eq!(w.d.buildings[i].evacuated, 0, "school emptied too fast");
    w.advance(25 * 60).unwrap();
    assert_eq!(w.d.buildings[i].people_inside, 0);
    assert_eq!(w.d.buildings[i].evacuated, 120);
}

#[test]
fn evacuate_zone_by_code_with_warning_and_shelter() {
    let mut w = world();
    w.apply(&Command::OpenShelter { building: "Valverde Sports Centre".into() }, &role("mayor")).unwrap();
    w.apply(&Command::IssueWarning { zone: "f4".into() }, &role("mayor")).unwrap();
    // F4 holds the school: the Master is not bound by buses.
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
