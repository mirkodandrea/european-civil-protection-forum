//! The shipped Selvariva pack: map reveals on its injects and the fire director's drone, headless.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::actions::{ActionStatus, SendActions};
use exercise::*;
use sim::{Command, World};

const ROLES: [&str; 3] = ["coordinamento_civile", "direzione_soccorso", "sala_operativa"];

fn data(name: &str) -> String {
    std::fs::read_to_string(format!("{}/../../data/scenarios/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn exercise() -> Exercise {
    let pack: scenario::ScenarioPack =
        serde_json::from_str(&data("selvariva_crisi_integrata.pack.json")).unwrap();
    let mut base: scenario::Scenario = serde_json::from_str(&data(&format!("{}.json", pack.base))).unwrap();
    base.migrate_legacy_timeline().unwrap();
    let scn = pack.build(base).unwrap();
    let opts = sim::FireOptions { realizations: 8, cutoff: pack.run.cutoff.unwrap_or(0.5) };
    let world = World::with_options(Arc::new(scn), pack.run.seed.unwrap_or(1), opts).unwrap();
    let mut ex = Exercise::new(world);
    ex.start().unwrap();
    ex
}

fn send(ex: &mut Exercise, id: &str) {
    let inject = ex.world.scn.msel.iter().find(|i| i.id == id).unwrap().clone();
    ex.send_inject_with(
        Some(id),
        &inject.title,
        &inject.sender,
        &inject.recipients,
        &inject.text,
        &BTreeMap::new(),
        &SendActions::all(),
    )
    .unwrap();
}

fn assert_done(ex: &Exercise, action: &str) {
    let run = ex.action_runs.values().find(|r| r.action == action).unwrap();
    assert_eq!(run.status, ActionStatus::Done, "{action}: {:?}", run.result);
}

fn run_to(ex: &mut Exercise, to_s: i64) {
    ex.begin_resolution_until(to_s, true).unwrap();
    while !ex.step_resolution(60).unwrap() {}
    ex.finish_resolution().unwrap();
}

#[test]
fn the_fire_director_owns_a_drone_that_flies_in_the_announced_wind() {
    let ex = exercise();
    let scn = &ex.world.scn;
    let ds = scn.role("direzione_soccorso").unwrap();
    assert_eq!(ds.resources.get("drones"), Some(&1));
    assert!(ds.allowed_actions.iter().any(|a| a == "MonitorArea"));
    let spec = scn.drone_spec("drones").unwrap();
    let windy = scn.msel.iter().find(|i| i.id == "SCS-05").unwrap();
    let kmh = windy.actions[0].command["wind_speed_kmh"].as_f64().unwrap() as f32;
    assert!(spec.wind_limit_kmh.unwrap() > kmh, "the 17:05 wind must not ground the drone");
    for i in &scn.msel {
        assert!(i.turn >= 1 && i.turn <= 5, "{}", i.id);
    }
}

#[test]
fn injects_reveal_the_map_and_the_drone_keeps_it_current() {
    let mut ex = exercise();
    for r in ROLES {
        assert!(ex.known_zones(r).is_empty(), "{r} starts blind");
    }
    run_to(&mut ex, 900);
    send(&mut ex, "SCS-02");
    assert_done(&ex, "SCS-R01");
    for r in ROLES {
        assert!(ex.player_map(r).unwrap()["estimates"][0]["radius_m"].as_f64().unwrap() > 0.0, "{r}");
    }

    run_to(&mut ex, 2100);
    send(&mut ex, "SCS-03");
    assert_done(&ex, "SCS-R02");
    assert_eq!(ex.known_zones("coordinamento_civile"), ["C3", "C4", "D3", "D4"]);
    assert!(ex.known_zones("sala_operativa").is_empty(), "not a recipient of SCS-03");

    ex.direct_command(&Command::MonitorArea { zones: vec!["C3-D4".into()], role: None }).unwrap();
    run_to(&mut ex, 3900);
    send(&mut ex, "SCS-05");
    run_to(&mut ex, 4800);
    let drone = ex
        .knowledge_events
        .iter()
        .filter(|e| matches!(e.source, KnowledgeSource::Monitoring))
        .count();
    assert!(drone >= 5, "the drone kept updating through the wind change ({drone})");

    send(&mut ex, "SCS-06");
    assert_done(&ex, "SCS-R03");
    run_to(&mut ex, 6600);
    send(&mut ex, "SCS-08");
    assert_done(&ex, "SCS-R04");
    run_to(&mut ex, 8400);
    send(&mut ex, "SCS-10");
    assert_done(&ex, "SCS-R05");
    let want = ex.world.scn.zones.with_neighbours(&ex.world.burning_zones());
    for r in ROLES {
        let known = ex.known_zones(r);
        assert!(want.iter().all(|z| known.contains(z)), "{r} lacks part of the handover map");
    }
}
