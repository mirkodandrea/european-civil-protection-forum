//! The whole turn loop headless, with the LLM's output supplied by hand.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::*;
use serde_json::json;
use sim::{Command, Loc, World};

fn exercise() -> Exercise {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    Exercise::new(World::new(scn, 1).unwrap())
}

fn parsed(actions: serde_json::Value) -> ParsedInput {
    ParsedInput { language: "it".into(), translation_en: "t".into(), actions: actions.as_array().unwrap().clone(), clarification: None, summary: "s".into(), inject_drafts: vec![] }
}

#[test]
fn full_turn_inject_reply_approve_resolve() {
    let mut ex = exercise();
    assert!(ex.player_reply("mayor", "hello").is_err(), "replies before start must fail");
    ex.start().unwrap();

    let mut tr = BTreeMap::new();
    tr.insert("mayor".to_string(), "Incendio sul Monte Pino".to_string());
    let ids = ex.send_inject(Some("I-01"), "Fire reported", "112", &["all".into()], "Fire on Pine Hill", &tr).unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ex.inbox("mayor")[0].text, "Incendio sul Monte Pino");
    assert_eq!(ex.inbox("fire_commander")[0].text, "Fire on Pine Hill");

    // Mayor: warn and evacuate F4 (school included; the mayor lacks buses for 120 -> verdict rejected).
    let pid = ex.player_reply("mayor", "Allertate F4 ed evacuate la zona").unwrap();
    let q = ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"},{"type":"EvacuateZone","zone":"F4"},{"type":"Nonsense"}]))).unwrap();
    assert!(q.is_none());
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert_eq!(p.commands.len(), 2);
    assert_eq!(p.invalid.len(), 1);
    assert!(p.verdicts[0].is_ok());
    assert!(!p.verdicts[1].is_ok(), "F4 evacuation needs 3 buses: {:?}", p.verdicts[1]);

    // The Master edits: keep the warning, evacuate the school by bus instead... still 3 buses; give one more bus first.
    ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }).unwrap();
    let edited = vec![
        Command::IssueWarning { zone: "F4".into() },
        Command::EvacuateZone { zone: "F4".into(), destination: None },
    ];
    let out = ex.decide(pid, Decision { approve: true, commands: Some(edited), note: Some("good call".into()) }).unwrap();
    assert_eq!(out.notify.as_deref(), Some("mayor"));
    assert!(out.facts_en.contains("APPROVED: Evacuate zone F4"), "{}", out.facts_en);
    assert!(out.facts_en.contains("good call"));

    // Fire commander: a drop; a second drop in another proposal exceeds the single air tanker.
    let p1 = ex.player_reply("fire_commander", "drop on G3").unwrap();
    ex.attach_parse(p1, parsed(json!([{"type":"WaterDrop","at":"G3"}]))).unwrap();
    ex.decide(p1, Decision { approve: true, ..Default::default() }).unwrap();
    let p2 = ex.player_reply("fire_commander", "another drop on F3").unwrap();
    ex.attach_parse(p2, parsed(json!([{"type":"WaterDrop","at":"F3"}]))).unwrap();
    assert!(!ex.proposal(p2).unwrap().verdicts[0].is_ok(), "approved drop must hold the air tanker");
    ex.decide(p2, Decision { approve: false, commands: None, note: None }).unwrap();

    let t0 = ex.world.time_s();
    let report = ex.resolve_turn(Some(30)).unwrap();
    assert_eq!(report.from_s, t0);
    assert_eq!(report.to_s, t0 + 1800);
    assert_eq!(report.applied.len(), 3);
    assert!(report.applied.iter().all(|a| a.ok), "{:?}", report.applied);
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Applied);
    assert_eq!(ex.proposal(p2).unwrap().status, ProposalStatus::Rejected);
    assert_eq!(ex.turn, 2);
    assert!(ex.world.d.warned.contains("F4"));
    assert_eq!(ex.world.d.pools["fire_commander"]["air_tankers"].used, 0, "resources reset at new turn");
    // Warned F4 evacuates in 10 min (+10 for the school): all out after 30.
    let school = ex.world.find_building("Pineta Primary School").unwrap();
    assert_eq!(ex.world.d.buildings[school].people_inside, 0);
}

#[test]
fn clarification_goes_back_to_the_player() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.player_reply("fire_commander", "send two teams there").unwrap();
    let mut p = parsed(json!([]));
    p.clarification = Some("Where exactly should the two crews cut the line?".into());
    let q = ex.attach_parse(pid, p).unwrap();
    assert!(q.is_some());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::NeedsClarification);
    let last = ex.inbox("fire_commander").last().cloned().unwrap();
    assert_eq!(last.kind, MsgKind::Clarification);
    // Nothing to approve without actions.
    assert!(ex.decide(pid, Decision { approve: true, ..Default::default() }).is_err());
}

#[test]
fn master_prompt_applies_immediately_on_approval() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.master_prompt("new fire at the western forest edge");
    ex.attach_parse(pid, ParsedInput { actions: vec![json!({"type":"Ignite","at":"Forest edge west","radius_m":100})], ..Default::default() }).unwrap();
    let out = ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    assert!(out.notify.is_none());
    assert!(out.facts_en.contains("cells lit"), "{}", out.facts_en);
    // Ignitions take effect from the next simulated step.
    let before = ex.world.burning_zones();
    ex.direct_command(&Command::AdvanceTime { minutes: 2 }).unwrap();
    assert!(ex.world.burning_zones().contains(&"E3".to_string()) || ex.world.burning_zones().len() > before.len(), "{:?}", ex.world.burning_zones());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Applied);
}

#[test]
fn llm_failure_leaves_proposal_for_manual_entry() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.player_reply("fire_commander", "cut a line along the south edge of G3").unwrap();
    ex.parse_failed(pid, "timeout").unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    let cmds = vec![Command::CutFireLine { from: None, to: None, along: Some("G3:S".into()), width_m: None }];
    ex.decide(pid, Decision { approve: true, commands: Some(cmds), note: None }).unwrap();
    let r = ex.resolve_turn(None).unwrap();
    assert!(r.applied[0].ok);
    assert!(ex.world.stats().line_km > 0.4);
}

#[test]
fn rollback_restores_world_and_marks_proposals() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.player_reply("fire_commander", "drop").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"WaterDrop","at":{"x":3000.0,"y":2500.0}}]))).unwrap();
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    ex.resolve_turn(None).unwrap();
    ex.resolve_turn(None).unwrap();
    assert_eq!(ex.turn, 3);
    ex.rollback(1).unwrap();
    assert_eq!(ex.turn, 1);
    assert_eq!(ex.world.time_s(), 0);
    assert!(ex.world.d.drops.is_empty());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::RolledBack);
    assert!(ex.inbox("mayor").iter().any(|m| m.kind == MsgKind::System));
    ex.resolve_turn(None).unwrap();
    assert_eq!(ex.turn, 2);
}

#[test]
fn direct_commands_by_code_and_log_file() {
    let dir = std::env::temp_dir().join(format!("ttx-test-{}", std::process::id()));
    let path = dir.join("log.jsonl");
    let mut ex = exercise().with_log_file(path.clone());
    ex.start().unwrap();
    ex.direct_command(&Command::Extinguish { area: sim::Area::Named("G3".into()) }).unwrap();
    assert!(ex.direct_command(&Command::WaterDrop { at: Loc::Named("Atlantis".into()) }).is_err());
    ex.end();
    assert_eq!(ex.phase, Phase::Debrief);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.lines().count() >= 4);
    assert!(text.contains("master_command"));
    assert!(ex.debrief()["log"].as_array().unwrap().len() >= 4);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn languages_are_per_player() {
    let mut ex = exercise();
    assert_eq!(ex.language_of("mayor"), "it");
    ex.set_language("mayor", "fr").unwrap();
    assert_eq!(ex.language_of("mayor"), "fr");
    assert!(ex.set_language("nobody", "fr").is_err());
}

#[test]
fn decision_rechecks_feasibility_after_resources_change() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.player_reply("mayor", "evacuate the school").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"EvacuateBuilding","building":"Pineta Primary School"}]))).unwrap();
    assert!(!ex.proposal(pid).unwrap().verdicts[0].is_ok(), "2 buses for 120 people");
    ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }).unwrap();
    let out = ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    assert!(out.facts_en.starts_with("APPROVED: Evacuate"), "{}", out.facts_en);
}
