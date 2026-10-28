//! Reveal actions of injects (fog of war) and the manual refresh of role maps, headless.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::actions::{ActionStatus, SendActions};
use exercise::*;
use scenario::{InjectAction, RevealSpec, RevealZones, ZoneRule};
use sim::World;

/// The default Valverde injects (I-01 ignites and gives an approximate reveal,
/// I-03 an exact one), all planned far away so only explicit sends matter.
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

fn send(ex: &mut Exercise, id: &str, to: &[&str]) {
    let to: Vec<String> = to.iter().map(|r| r.to_string()).collect();
    ex.send_inject_with(Some(id), "T", "112", &to, "text", &BTreeMap::new(), &SendActions::all()).unwrap();
}

fn run_of(ex: &Exercise, action: &str) -> exercise::actions::ActionRun {
    ex.action_runs.values().find(|r| r.action == action).unwrap().clone()
}

fn estimate(ex: &Exercise, role: &str) -> serde_json::Value {
    ex.player_map(role).unwrap()["estimates"][0].clone()
}

#[test]
fn approximate_reveal_is_deterministic_and_within_error() {
    let mut a = exercise_with(|_| {});
    send(&mut a, "I-01", &["mayor", "fire_commander"]);
    assert_eq!(run_of(&a, "A-02").status, ActionStatus::Done, "{:?}", run_of(&a, "A-02").result);
    let (truth, area, _) = a.actual_fire().expect("the ignition burns");
    let e = estimate(&a, "mayor");
    let at = [e["at"][0].as_f64().unwrap(), e["at"][1].as_f64().unwrap()];
    let miss = ((at[0] - truth[0] as f64).powi(2) + (at[1] - truth[1] as f64).powi(2)).sqrt();
    assert!(miss <= 400.0 + 1e-3, "position error {miss}");
    let reported = e["area_ha"].as_f64().unwrap();
    assert!(reported >= area * 0.7 * 0.94 && reported <= area * 1.3 * 1.06, "{reported} vs {area}");
    assert!(e["spread_toward_deg"].as_f64().unwrap() < 360.0);
    assert_eq!(e["source"], "112");
    assert!(e["radius_m"].as_f64().unwrap() >= 400.0);
    // The knowledge event is the delivered message's, with the sender as source.
    let ev = a.knowledge_events.iter().find(|k| k.role == "mayor").unwrap();
    assert!(matches!(ev.source, KnowledgeSource::Delivered));
    assert_eq!(a.player_map("mayor").unwrap()["last_observation_source"], "112");
    // Same scenario, seed, inject, action and role: same report.
    let mut b = exercise_with(|_| {});
    send(&mut b, "I-01", &["mayor", "fire_commander"]);
    assert_eq!(estimate(&b, "mayor"), e);
    assert_ne!(estimate(&b, "fire_commander")["at"], e["at"], "each role has its own error");
    // An estimate leaves the role knowing no zone.
    assert!(a.known_zones("mayor").is_empty());
}

#[test]
fn spread_follows_wind_for_a_fresh_fire() {
    let mut ex = exercise_with(|_| {});
    send(&mut ex, "I-01", &["mayor"]);
    let (_, _, spread) = ex.actual_fire().unwrap();
    let wind = ex.world.fire.weather().wind_dir_deg;
    let want = (wind + 180.0).rem_euclid(360.0) as f32;
    let diff = ((spread - want + 540.0).rem_euclid(360.0) - 180.0).abs();
    assert!(diff < 1.0, "spread {spread} wind-toward {want}");
}

#[test]
fn exact_reveal_resolves_zone_rule_at_execution() {
    let mut ex = exercise_with(|_| {});
    send(&mut ex, "I-01", &["mayor"]);
    send(&mut ex, "I-03", &["fire_commander"]);
    let run = run_of(&ex, "A-03");
    assert_eq!(run.status, ActionStatus::Done, "{:?}", run.result);
    let want = ex.world.scn.zones.with_neighbours(&ex.world.burning_zones());
    assert!(!want.is_empty());
    let mut got = ex.known_zones("fire_commander");
    got.sort();
    assert_eq!(got, want);
    let map = ex.player_map("fire_commander").unwrap();
    assert!(!map["cells"].as_array().unwrap().is_empty());
    assert!(!map["roads"].as_object().unwrap().is_empty());
    assert!(ex.known_zones("mayor").is_empty(), "only the inject's recipients learn");
    let row = ex.planned_injects().into_iter().find(|i| i["id_inject"] == "I-03").unwrap();
    assert!(row["actions"][0]["summary"].as_str().unwrap().contains("exact"));
    assert_eq!(row["actions"][0]["status"], "done");
}

#[test]
fn exact_reveal_without_fire_fails_and_explicit_zones_work() {
    let mut ex = exercise_with(|scn| {
        scn.msel[2].actions[0].reveal = Some(RevealSpec::Exact {
            zones: RevealZones::Rule(ZoneRule::Burning),
            layers: vec!["fire".into()],
            source: Some("Patrol".into()),
        });
        scn.msel[3].actions.push(InjectAction {
            id: "A-ZONES".into(),
            title: "zones".into(),
            command: serde_json::Value::Null,
            reveal: Some(RevealSpec::Exact { zones: RevealZones::List(vec!["c3".into()]), layers: vec!["fire".into(), "roads".into()], source: None }),
            offset_s: 0,
            note: String::new(),
        });
    });
    send(&mut ex, "I-03", &["fire_commander"]);
    let run = run_of(&ex, "A-03");
    assert_eq!(run.status, ActionStatus::Failed);
    assert!(run.result.unwrap().contains("no zone is burning"));
    send(&mut ex, "I-04", &["mayor"]);
    assert_eq!(run_of(&ex, "A-ZONES").status, ActionStatus::Done);
    assert_eq!(ex.known_zones("mayor"), vec!["C3".to_string()]);
}

#[test]
fn delayed_reveal_sends_a_situation_message() {
    let mut ex = exercise_with(|scn| scn.msel[0].actions[1].offset_s = 120);
    send(&mut ex, "I-01", &["mayor"]);
    assert_eq!(run_of(&ex, "A-02").status, ActionStatus::Approved);
    assert!(ex.knowledge_events.is_empty());
    ex.begin_resolution_until(300, true).unwrap();
    while !ex.step_resolution(exercise::STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    assert_eq!(run_of(&ex, "A-02").status, ActionStatus::Done);
    let ev = &ex.knowledge_events[0];
    assert!(matches!(ev.source, KnowledgeSource::Acquired));
    assert_eq!(ev.source_name.as_deref(), Some("112"));
    let m = ex.messages.iter().find(|m| m.id == ev.message).unwrap();
    assert_eq!(m.kind, MsgKind::Situation);
}

#[test]
fn validation_rejects_bad_reveals() {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.check().unwrap();
    let ok = scn.msel[0].actions[1].clone();
    assert!(ok.reveal.is_some());
    let mut negative = ok.clone();
    negative.offset_s = -60;
    assert!(scn.check_action(&negative).unwrap_err().to_string().contains("before its inject"));
    let mut both = ok.clone();
    both.command = serde_json::json!({"type": "SetWeather"});
    assert!(scn.check_action(&both).unwrap_err().to_string().contains("both"));
    let mut neither = ok.clone();
    neither.reveal = None;
    assert!(scn.check_action(&neither).is_err());
    let mut bad_layer = ok.clone();
    bad_layer.reveal = Some(RevealSpec::Exact { zones: RevealZones::Rule(ZoneRule::Burning), layers: vec!["people".into()], source: None });
    assert!(scn.check_action(&bad_layer).is_err());
    let mut bad_zone = ok.clone();
    bad_zone.reveal = Some(RevealSpec::Exact { zones: RevealZones::List(vec!["Z99".into()]), layers: vec!["fire".into()], source: None });
    assert!(scn.check_action(&bad_zone).is_err());
    scn.msel[1].actions.push(negative);
    assert!(scn.check().is_err(), "the scenario refuses it when it loads");
    // Custom injects are checked too.
    let mut ex = exercise_with(|_| {});
    let mut custom = ok;
    custom.offset_s = -1;
    let selection = SendActions { run: None, custom: vec![custom] };
    assert!(ex.send_inject_with(None, "T", "EXCON", &["mayor".into()], "x", &BTreeMap::new(), &selection).is_err());
}

#[test]
fn old_logs_and_packs_still_load() {
    let o: MapObservation = serde_json::from_str(r#"{"zones":["A1"]}"#).unwrap();
    assert!(o.estimates.is_empty());
    let a: InjectAction = serde_json::from_str(r#"{"id":"X","title":"t","command":{"type":"SetWeather"}}"#).unwrap();
    assert!(a.reveal.is_none());
    let r: InjectAction = serde_json::from_str(r#"{"id":"X","title":"t","reveal":{"kind":"exact","zones":["A1"]}}"#).unwrap();
    assert!(matches!(r.reveal, Some(RevealSpec::Exact { ref layers, .. }) if layers == &["fire"]));
}

#[test]
fn refresh_repeats_the_latest_zone_reveal_as_a_fresh_snapshot() {
    let mut ex = exercise_with(|_| {});
    assert!(ex.refresh_role_maps(Some("EXCON"), &BTreeMap::new()).is_err(), "nothing to refresh yet");
    send(&mut ex, "I-01", &["mayor", "fire_commander"]);
    send(&mut ex, "I-03", &["fire_commander"]);
    let plan = ex.refresh_plan();
    assert_eq!(plan.len(), 1, "an estimate is not a zone reveal: {plan:?}");
    assert_eq!(plan[0]["role"], "fire_commander");
    assert_eq!(plan[0]["layers"], serde_json::json!(["fire", "roads"]));
    let zones = ex.known_zones("fire_commander");
    let before = ex.player_map("fire_commander").unwrap()["cells"].as_array().unwrap().len();
    // The fire grows; the player map stays a snapshot until the refresh.
    ex.begin_resolution_until(3600, true).unwrap();
    while !ex.step_resolution(exercise::STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    let events = ex.knowledge_events.len();
    let done = ex.refresh_role_maps(Some("Sala operativa EXCON"), &BTreeMap::new()).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(ex.knowledge_events.len(), events + 1);
    let ev = ex.knowledge_events.last().unwrap();
    assert_eq!((ev.role.as_str(), ev.source_name.as_deref()), ("fire_commander", Some("Sala operativa EXCON")));
    assert_eq!(ev.layers, vec!["fire".to_string(), "roads".to_string()]);
    let mut repeated = ev.observation.zones.clone();
    repeated.sort();
    assert_eq!(repeated, zones, "same zones as the reveal it repeats");
    assert_eq!(ev.t_s, ex.world.time_s());
    assert!(ex.player_map("fire_commander").unwrap()["cells"].as_array().unwrap().len() >= before);
    assert!(ex.player_map("mayor").unwrap()["cells"].as_array().unwrap().is_empty(), "other roles are untouched");
}
