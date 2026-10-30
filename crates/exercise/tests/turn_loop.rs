//! The whole turn loop headless, with the LLM's output supplied by hand.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::*;
use serde_json::json;
use sim::{Command, Loc, World};

fn exercise() -> Exercise {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap());
    for role in ex.role_ids() { let lang = ex.language_of(&role); ex.set_language(&role, &lang).unwrap(); }
    ex
}

fn reply(ex: &mut Exercise, role: &str, text: &str) -> anyhow::Result<u64> {
    let parent = match ex.messages.iter().rev().find(|m| m.to == role && m.kind == MsgKind::Inject) {
        Some(m) => m.id,
        None if ex.phase == Phase::Orders => ex.send_inject(None, "Task", "EXCON", &[role.into()], "Exercise task", &BTreeMap::new())?[0],
        None => 0,
    };
    ex.player_reply(role, text, parent, DecisionEvidence::judgment("Operational judgment based on received information"))
}

fn parsed(actions: serde_json::Value) -> ParsedInput {
    ParsedInput { resource_limit_draft:None, communication_drafts: vec![], language: "it".into(), translation_en: "t".into(), actions: actions.as_array().unwrap().clone(), clarification: None, summary: "s".into(), inject_drafts: vec![] }
}

#[test]
fn full_turn_inject_reply_approve_resolve() {
    let mut ex = exercise();
    assert!(reply(&mut ex, "mayor", "hello").is_err(), "replies before start must fail");
    ex.start().unwrap();

    let mut tr = BTreeMap::new();
    tr.insert("mayor".to_string(), "Incendio sul Monte Pino".to_string());
    let ids = ex.send_inject(Some("I-01"), "Fire reported", "112", &["all".into()], "Fire on Pine Hill", &tr).unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ex.inbox("mayor")[0].text, "Incendio sul Monte Pino");
    assert_eq!(ex.inbox("fire_commander")[0].text, "Fire on Pine Hill");

    // Mayor: warn and evacuate F4 (school included; the mayor lacks buses for 120 -> verdict rejected).
    let pid = reply(&mut ex, "mayor", "Allertate F4 ed evacuate la zona").unwrap();
    let q = ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"},{"type":"EvacuateZone","zone":"F4"},{"type":"Nonsense"}]))).unwrap();
    assert!(q.is_none());
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert_eq!(p.commands.len(), 2);
    assert_eq!(p.invalid.len(), 1);
    assert!(p.verdicts[0].is_ok());
    assert!(!p.verdicts[1].is_ok(), "F4 evacuation needs 3 buses: {:?}", p.verdicts[1]);

    // The EXCON edits: keep the warning, evacuate the school by bus instead... still 3 buses; give one more bus first.
    ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }).unwrap();
    let edited = vec![
        Command::IssueWarning { zone: "F4".into() },
        Command::EvacuateZone { zone: "F4".into(), destination: None },
    ];
    let out = ex.decide(pid, Decision { approve: true, commands: Some(edited), note: Some("good call".into()), ..Default::default() }).unwrap();
    assert_eq!(out.notify.as_deref(), Some("mayor"));
    assert!(out.facts_en.contains("APPROVED: Evacuate zone F4"), "{}", out.facts_en);
    assert!(out.facts_en.contains("good call"));

    // Fire commander: a drop; a second drop in another proposal exceeds the single air tanker.
    let p1 = reply(&mut ex, "fire_commander", "drop on G3").unwrap();
    ex.attach_parse(p1, parsed(json!([{"type":"WaterDrop","at":"G3"}]))).unwrap();
    ex.decide(p1, Decision { approve: true, ..Default::default() }).unwrap();
    let p2 = reply(&mut ex, "fire_commander", "another drop on F3").unwrap();
    ex.attach_parse(p2, parsed(json!([{"type":"WaterDrop","at":"F3"}]))).unwrap();
    assert!(!ex.proposal(p2).unwrap().verdicts[0].is_ok(), "approved drop must hold the air tanker");
    ex.decide(p2, Decision { approve: false, commands: None, note: None, ..Default::default() }).unwrap();

    let t0 = ex.world.time_s();
    let report = ex.resolve_turn(Some(30)).unwrap();
    assert_eq!(report.from_s, t0);
    assert_eq!(report.to_s, t0 + 1800);
    assert_eq!(report.applied.len(), 3);
    assert!(report.applied.iter().all(|a| a.ok), "{:?}", report.applied);
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Applied);
    assert_eq!(ex.proposal(p2).unwrap().status, ProposalStatus::Rejected);
    assert_eq!(ex.turn, 2);
    assert!(ex.world.d.warned.contains_key("F4"));
    assert_eq!(ex.world.d.pools["fire_commander"]["air_tankers"].used, 1, "deployments persist across turns until explicitly returned");
    // Warned F4 evacuates in 10 min (+10 for the school): all out after 30.
    let school = ex.world.find_building("Pineta Primary School").unwrap();
    assert_eq!(ex.world.d.buildings[school].people_inside, 0);
}

#[test]
fn clarification_goes_back_to_the_player() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "send two teams there").unwrap();
    let mut p = parsed(json!([]));
    p.clarification = Some("Where exactly should the two crews cut the line?".into());
    let q = ex.attach_parse(pid, p).unwrap();
    assert!(q.is_some());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::NeedsClarification);
    assert!(!ex.inbox("fire_commander").iter().any(|m| m.kind == MsgKind::Clarification));
    assert!(ex.deliver_approved_communication(pid, 0, "Where?").is_err());
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    ex.deliver_approved_communication(pid, 0, "Where exactly should the two crews cut the line?").unwrap();
    let last = ex.inbox("fire_commander").last().cloned().unwrap();
    assert_eq!(last.kind, MsgKind::Clarification);
    assert!(ex.deliver_approved_communication(pid, 0, "Where?").is_err());
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
    let pid = reply(&mut ex, "fire_commander", "cut a line along the south edge of G3").unwrap();
    ex.parse_failed(pid, "timeout").unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    let cmds = vec![Command::CutFireLine { from: None, to: None, along: Some("G3:S".into()), width_m: None }];
    ex.decide(pid, Decision { approve: true, commands: Some(cmds), note: None, ..Default::default() }).unwrap();
    let r = ex.resolve_turn(None).unwrap();
    assert!(r.applied[0].ok);
    assert!(ex.world.stats().line_km>0.4);
}

#[test]
fn rollback_restores_world_and_marks_proposals() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "drop").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"WaterDrop","at":{"x":3000.0,"y":2500.0}}]))).unwrap();
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    ex.resolve_turn(None).unwrap();
    let notes = ex.direct_command(&Command::IssueWarning { zone: "F4".into() }).unwrap();
    assert!(notes[0].contains("population of F4 warned at"), "{notes:?}");
    let logged = ex.log.iter().rev().find(|e| e.kind == "master_command").unwrap();
    assert!(logged.data["result"]["ok"][0].as_str().unwrap().contains("10 min instead of 20"));
    ex.resolve_turn(None).unwrap();
    assert_eq!(ex.turn, 3);
    assert!(ex.world.d.warned.contains_key("F4"));
    // Fog of war: a role sees a warned zone only once it is told about it.
    assert_eq!(ex.player_map("mayor").unwrap()["warned"], json!([]));
    let o = ex.zone_observation(&["F4".into()], &["buildings".into()]).unwrap();
    ex.acquire_map_layers("mayor", "Sala operativa", o, None, vec!["buildings".into()]).unwrap();
    let pm = ex.player_map("mayor").unwrap();
    assert_eq!(pm["warned"], json!(["F4"]));
    assert!(pm["warnings"][0]["clock"].is_string());
    assert!(pm["warnings"][0].get("people_inside").is_none());
    assert_eq!(ex.player_map("fire_commander").unwrap()["warned"], json!([]));
    ex.rollback(1).unwrap();
    assert_eq!(ex.turn, 1);
    assert_eq!(ex.world.time_s(), 0);
    assert!(ex.world.d.drops.is_empty());
    assert!(ex.world.d.warned.is_empty(), "warnings roll back with the world");
    // What the mayor was told stays told (like every other observation), until a new reveal.
    assert_eq!(ex.player_map("mayor").unwrap()["warned"], json!(["F4"]));
    let o = ex.zone_observation(&["F4".into()], &["buildings".into()]).unwrap();
    ex.acquire_map_layers("mayor", "Sala operativa", o, None, vec!["buildings".into()]).unwrap();
    assert_eq!(ex.player_map("mayor").unwrap()["warned"], json!([]), "a fresh reveal clears it");
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
    ex.end().unwrap();
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
    let pid = reply(&mut ex, "mayor", "evacuate the school").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"EvacuateBuilding","building":"Pineta Primary School"}]))).unwrap();
    assert!(!ex.proposal(pid).unwrap().verdicts[0].is_ok(), "2 buses for 120 people");
    ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }).unwrap();
    let out = ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    assert!(out.facts_en.starts_with("APPROVED: Evacuate"), "{}", out.facts_en);
}

#[test]
fn resolution_steps_second_by_second() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "line").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"CutFireLine","along":"F4:N"}]))).unwrap();
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();

    ex.begin_resolution(Some(5)).unwrap();
    assert_eq!(ex.phase, Phase::Resolving);
    // Approved resource actions dispatch at resolution start; effect waits for arrival.
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Applied);
    assert_eq!(ex.world.stats().line_km,0.0);
    assert_eq!(ex.world.d.tasks[0].status,sim::TaskStatus::EnRoute);
    // Nothing else may happen while the turn is being simulated.
    assert!(reply(&mut ex, "mayor", "evacuate").is_err());
    assert!(ex.begin_resolution(None).is_err());
    assert!(ex.rollback(1).is_err());
    assert!(ex.end().is_err());

    let mut steps = 0;
    let mut generation = ex.generation;
    loop {
        let done = ex.step_resolution(STEP_S).unwrap();
        steps += 1;
        assert_eq!(ex.world.time_s(), steps * STEP_S, "one step at a time");
        assert!(ex.generation > generation, "every step is published");
        generation = ex.generation;
        if done {
            break;
        }
    }
    assert_eq!(steps * STEP_S, 300);
    assert!(ex.world.stats().line_km>0.4);
    assert_eq!(ex.world.d.tasks[0].status,sim::TaskStatus::Completed);
    let r = ex.finish_resolution().unwrap();
    assert_eq!((r.from_s, r.to_s), (0, 300));
    assert_eq!(ex.phase, Phase::Orders);
    assert_eq!(ex.turn, 2);
    assert!(ex.resolving.is_none());
}

#[test]
fn stepped_and_one_shot_resolution_agree() {
    let mut a = exercise();
    let mut b = exercise();
    a.start().unwrap();
    b.start().unwrap();
    a.resolve_turn(Some(30)).unwrap();
    b.begin_resolution(Some(30)).unwrap();
    while !b.step_resolution(STEP_S).unwrap() {}
    b.finish_resolution().unwrap();
    assert_eq!(a.world.stats().burned_ha, b.world.stats().burned_ha);
    assert_eq!(a.world.time_s(), b.world.time_s());
}

#[test]
fn conversations_collect_what_each_player_received() {
    let mut ex = exercise();
    ex.start().unwrap();
    ex.send_inject(None, "School in session", "Pineta Primary School", &["mayor".into()], "120 pupils on site", &BTreeMap::new()).unwrap();
    ex.add_message(MASTER, "mayor", MsgKind::Feedback, Some("Orders reviewed"), "approvato", "approved", None);
    ex.add_message(MASTER, "mayor", MsgKind::Clarification, None, "quale zona?", "which zone?", None);
    let c = ex.conversation("mayor");
    assert_eq!(c.len(), 2, "clarifications are the AI's own words, not context: {c:?}");
    assert!(c[0].content.contains("INJECT") && c[0].content.contains("120 pupils"));
    assert!(c[1].content.contains("approved"));
    assert!(ex.conversation("fire_commander").is_empty());
    ex.converse("mayor", "assistant", "{}");
    for i in 0..(CONVERSATION_MAX + 5) {
        ex.converse("mayor", "user", format!("note {i}"));
    }
    assert_eq!(ex.conversation("mayor").len(), CONVERSATION_MAX);
}

#[test]
fn replies_require_delivered_inject_and_keep_clarification_thread() {
    let mut ex = exercise();
    ex.start().unwrap();
    let ids = ex.send_inject(Some("I-01"), "Fire", "112", &["mayor".into()], "Fire reported", &BTreeMap::new()).unwrap();
    let mid = ids[0];
    let root = ex.messages.last().unwrap().inject_id.unwrap();
    assert!(ex.player_reply("mayor", "hello", 99999, DecisionEvidence::judgment("Based on the received inject")).is_err());
    assert!(ex.player_reply("fire_commander", "hello", mid, DecisionEvidence::judgment("Based on the received inject")).is_err());
    assert!(ex.proposals.is_empty());
    let pid = ex.player_reply("mayor", "Where?", mid, DecisionEvidence::judgment("Based on the received inject")).unwrap();
    ex.attach_parse(pid, ParsedInput { clarification: Some("Which location?".into()), ..Default::default() }).unwrap();
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    ex.deliver_approved_communication(pid, 0, "Which location?").unwrap();
    let clarification = ex.messages.last().unwrap();
    assert_eq!(clarification.inject_id, Some(root));
    assert_eq!(clarification.reply_to, Some(mid));
    let followup = ex.player_reply("mayor", "School", clarification.id, DecisionEvidence::judgment("Based on the received inject")).unwrap();
    assert_eq!(ex.proposal(followup).unwrap().inject_id, Some(root));
    ex.resolve_turn(Some(1)).unwrap();
    let d = ex.debrief();
    assert!(d["messages"].as_array().unwrap().iter().filter(|m| m["inject_id"] == root).count() >= 4);
    assert_eq!(ex.proposal(pid).unwrap().reply_to, Some(mid));
}

#[test]
fn repeated_msel_deliveries_have_distinct_threads_shared_by_recipients() {
    let mut ex = exercise();
    let ids = ex.send_inject(Some("I-01"), "Fire", "112", &["all".into()], "Fire", &BTreeMap::new()).unwrap();
    let first = ex.messages[0].inject_id;
    assert!(ex.messages.iter().all(|m| m.inject_id == first && m.recipients.len() == 3));
    // A role that already received the MSEL inject is skipped silently.
    assert!(ex.send_inject(Some("I-01"), "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new()).unwrap().is_empty());
    assert_eq!(ex.inject_sent_to["I-01"].len(), 3);
    // Ad-hoc injects still open a distinct thread each time.
    ex.send_inject(None, "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new()).unwrap();
    assert_ne!(ex.messages.last().unwrap().inject_id, first);
    assert_eq!(ids.len(), 3);
}

#[test]
fn lifecycle_retains_approval_execution_and_rejects_late_parse_or_duplicate_run() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "warn F4").unwrap();
    assert!(ex.log.iter().any(|l| l.kind == "proposal_declared" && l.data["id"] == pid));
    assert_eq!(ex.proposal(pid).unwrap().history[0].status, ProposalStatus::Parsing);
    let original_world = ex.world.view();
    ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    assert_eq!(ex.world.view(), original_world, "pending proposals cannot change the world");
    ex.decide(pid, Decision { approve: true, note: Some("Use warning plan".into()), ..Default::default() }).unwrap();
    assert_eq!(ex.world.view(), original_world, "approval queues player actions until resolution");
    assert!(ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"G4"}]))).is_err());
    assert!(ex.parse_failed(pid, "late timeout").is_err());
    assert!(ex.decide(pid, Decision { approve: true, ..Default::default() }).is_err());
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.decision.as_ref().unwrap().actor, MASTER);
    assert_eq!(p.decision.as_ref().unwrap().note.as_deref(), Some("Use warning plan"));
    let first = ex.resolve_turn(Some(1)).unwrap();
    assert_eq!(first.applied.len(), 1);
    let second = ex.resolve_turn(Some(1)).unwrap();
    assert!(second.applied.is_empty());
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.executions.len(), 1);
    assert!(p.executions[0].ok);
    assert_eq!(p.executions[0].origin, "approved_player_decision");
    assert_eq!(p.history.iter().map(|h| h.status).collect::<Vec<_>>(), vec![ProposalStatus::Parsing, ProposalStatus::Pending, ProposalStatus::Approved, ProposalStatus::Applied]);
    ex.rollback(1).unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.history.last().unwrap().status, ProposalStatus::RolledBack);
    assert_eq!(p.executions.len(), 1, "rollback retains execution evidence");
}

#[test]
fn infeasible_approval_is_refused_and_execution_failures_are_truthful() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "evacuate school").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"EvacuateBuilding","building":"Pineta Primary School"}]))).unwrap();
    assert!(ex.decide(pid, Decision { approve: true, ..Default::default() }).is_err());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    assert!(ex.proposal(pid).unwrap().decision.is_none());
    ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: 1 }).unwrap();
    ex.decide(pid, Decision { approve: true, ..Default::default() }).unwrap();
    assert!(ex.direct_command(&Command::AdjustResource { role: "mayor".into(), resource: "buses".into(), delta: -3 }).is_err(), "inventory cannot undercut approved commitments");
    ex.direct_command(&Command::SetBuildingStatus {building:"Pineta Primary School".into(),damage:None,evacuated:Some(true)}).unwrap();
    let report = ex.resolve_turn(Some(1)).unwrap();
    assert!(!report.applied[0].ok);
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Failed);
    assert!(!p.executions[0].ok);
    assert!(p.executions[0].detail.contains("already empty"));
    assert!(ex.log.iter().any(|l| l.kind == "master_command" && l.data["origin"] == "direct_excon_intervention"));
}

#[test]
fn master_can_reject_during_parse_without_late_ai_resurrection() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "request transport").unwrap();
    ex.set_declaration_kind(pid, DeclarationKind::Request).unwrap();
    ex.decide(pid, Decision { approve: false, ..Default::default() }).unwrap();
    assert!(ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).is_err());
    assert!(ex.parse_failed(pid, "timeout").is_err());
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Rejected);
    assert_eq!(ex.proposal(pid).unwrap().declaration_kind, DeclarationKind::Request);
    ex.end().unwrap();
    assert!(ex.decide(pid, Decision::default()).is_err());
}

#[test]
fn pending_and_actual_resources_remain_distinct_across_turns() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "drop on G3").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"WaterDrop","at":"G3"}]))).unwrap();
    let before = ex.log[0].seq;
    let accounting = ex.resource_accounting(Some("fire_commander"));
    assert_eq!(accounting["fire_commander"]["air_tankers"]["requested"], 1);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["deployed"], 0);
    ex.resolve_turn(Some(1)).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    assert_eq!(ex.proposal(pid).unwrap().turn, 1);
    ex.decide(pid, Decision { approve:true, ..Default::default() }).unwrap();
    let accounting = ex.resource_accounting(Some("fire_commander"));
    assert_eq!(accounting["fire_commander"]["air_tankers"]["requested"], 0);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["committed"], 1);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["available"], 0);
    ex.resolve_turn(Some(1)).unwrap();
    let accounting = ex.resource_accounting(Some("fire_commander"));
    assert_eq!(accounting["fire_commander"]["air_tankers"]["committed"], 0);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["deployed"], 1);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["available"], 0);
    let allocation = ex.proposal(pid).unwrap().executions[0].allocations[0];
    let p2 = reply(&mut ex, "fire_commander", "another drop on F3").unwrap();
    ex.attach_parse(p2, parsed(json!([{"type":"WaterDrop","at":"F3"}]))).unwrap();
    assert!(ex.decide(p2, Decision { approve:true, ..Default::default() }).is_err());
    ex.direct_command(&Command::ReturnResource { allocation, qty:1 }).unwrap();
    let accounting = ex.resource_accounting(Some("fire_commander"));
    assert_eq!(accounting["fire_commander"]["air_tankers"]["deployed"], 0);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["returned"], 1);
    assert_eq!(accounting["fire_commander"]["air_tankers"]["available"], 1);
    assert_eq!(ex.world.d.drops.len(), 1);
    assert_eq!(ex.log[0].seq, before);
    let d = ex.debrief();
    assert_eq!(d["resource_allocations"][0]["id"], allocation);
    assert!(d["resource_events"].as_array().unwrap().iter().any(|e| e["kind"] == "returned"));
    assert!(d["physical_state"]["drops"].as_array().unwrap().len() == 1);
}

#[test]
fn rollback_keeps_resource_history_even_when_actual_ledger_is_restored() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "drop G3").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"WaterDrop","at":"G3"}]))).unwrap();
    ex.decide(pid, Decision { approve:true, ..Default::default() }).unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    let allocation = ex.world.d.allocations[0].id;
    ex.direct_command(&Command::ReturnResource { allocation, qty:1 }).unwrap();
    ex.rollback(1).unwrap();
    assert!(ex.world.d.allocations.is_empty());
    assert_eq!(ex.resource_history.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), vec!["deployed", "returned"]);
    assert_eq!(ex.debrief()["resource_events"].as_array().unwrap().len(), 2);
    assert!(ex.log.iter().any(|l| l.kind == "rollback"));
}

#[test]
fn default_language_needs_no_confirmation_and_evidence_is_validated_before_recording() {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap());
    ex.start().unwrap();
    let mid = ex.send_inject(None, "Fire", "112", &["mayor".into()], "Fire reported", &BTreeMap::new()).unwrap()[0];
    assert!(ex.players["mayor"].language_confirmed);
    assert!(ex.pending_languages().is_empty());
    assert!(!ex.players["mayor"].language.is_empty());
    assert!(ex.set_language("mayor", "invented-language").is_err());
    ex.set_language("mayor", "it").unwrap();
    assert!(ex.players["mayor"].language_confirmed);
    assert!(ex.player_reply("mayor", "warn F4", mid, DecisionEvidence::judgment(" ")).is_err());
    let unsafe_link = DecisionEvidence { basis:DecisionBasis { kind:BasisKind::Plan, reference:"Municipal plan".into(), rationale: None }, attachments:vec![Attachment {title:"Map".into(), url:"javascript:alert(1)".into()}] };
    assert!(ex.player_reply("mayor", "warn F4", mid, unsafe_link).is_err());
    assert!(ex.proposals.is_empty());
    let evidence = DecisionEvidence { basis:DecisionBasis { kind:BasisKind::Procedure, reference:"Municipal warning procedure, section 3".into(), rationale: None }, attachments:vec![Attachment {title:"Warning map".into(), url:"https://example.test/warning-map.pdf".into()}] };
    let expected = serde_json::to_value(&evidence).unwrap();
    let pid = ex.player_reply("mayor", "warn F4", mid, evidence).unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    ex.decide(pid, Decision { approve:true, ..Default::default() }).unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    ex.rollback(1).unwrap();
    assert_eq!(serde_json::to_value(ex.proposal(pid).unwrap().evidence.as_ref().unwrap()).unwrap(), expected);
    assert_eq!(ex.debrief()["proposals"][0]["evidence"], expected);
    assert!(ex.log.iter().any(|e| e.kind == "proposal_declared" && e.data["evidence"] == expected));
}

#[test]
fn evaluator_durations_use_actual_delivery_declaration_and_decision_clocks() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "warn F4").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    ex.decide(pid, Decision {approve:true, ..Default::default()}).unwrap();
    let parent = ex.proposal(pid).unwrap().reply_to.unwrap();
    let m = ex.messages.iter_mut().find(|m| m.id == parent).unwrap();
    m.wall_ms=1000;m.t_s=10;
    let p=ex.proposals.iter_mut().find(|p| p.id==pid).unwrap();
    p.history[0].wall_ms=4000;p.history[0].t_s=20;
    let d=p.decision.as_mut().unwrap();d.wall_ms=8000;d.t_s=30;
    let view=ex.evaluator_view(&exercise::evaluation::EvidenceFilter::default()).unwrap();
    let c=&view["chains"][0];
    assert_eq!(c["reply_delay_wall_ms"],3000);assert_eq!(c["reply_delay_sim_s"],10);
    assert_eq!(c["master_queue_wall_ms"],4000);assert_eq!(c["master_queue_sim_s"],10);
    let before=ex.generation;
    assert!(ex.set_reference_materials(vec![DecisionEvidence::judgment("not a reference plan")]).is_err());
    assert_eq!(ex.generation,before);
}

#[test]
fn clock_and_training_phases_keep_one_turn_and_accumulated_resources() {
    let mut ex=exercise();ex.start().unwrap();
    let deployment=reply(&mut ex,"fire_commander","Cut F4 north edge").unwrap();
    ex.attach_parse(deployment,parsed(json!([{"type":"CutFireLine","along":"F4:N"}]))).unwrap();
    ex.decide(deployment,Decision{approve:true,..Default::default()}).unwrap();ex.execute_approved(deployment).unwrap();
    let used=ex.world.d.pools["fire_commander"]["ground_crews"].used;assert_eq!(used,1);
    for (msel,due) in [("I-01",0),("I-02",1800),("I-03",3600)] {ex.schedule_inject(msel,due).unwrap();}
    ex.configure_clock(true,60.0,"One real second represents one simulated minute").unwrap();
    for target in [0,1800,3600] {
        while ex.world.time_s()<target {ex.advance_clock((target-ex.world.time_s()).min(60)).unwrap();}
        for (id,t) in ex.claim_due_injects() {
            let ids=ex.send_inject(Some(&t.id),&t.title,&t.sender,&t.recipients,&t.text,&BTreeMap::new()).unwrap();
            ex.complete_schedule(id,Ok(ids));
        }
    }
    assert_eq!(ex.turn,1);assert_eq!(ex.phase,Phase::Orders);assert_eq!(ex.exercise_phase,ExercisePhase::EarlyWarning);
    assert_eq!(ex.scheduled_injects.iter().map(|i|i.delivered_s.unwrap()).collect::<Vec<_>>(),vec![0,1800,3600]);
    assert!(ex.messages.iter().any(|m|m.clock=="Day 1 13:00"));
    assert!(ex.messages.iter().any(|m|m.clock=="Day 1 13:30"));
    assert!(ex.messages.iter().any(|m|m.clock=="Day 1 14:00"));
    let history=ex.resource_history.len();let world=ex.world.view();
    for phase in [ExercisePhase::EarlyAction,ExercisePhase::Escalation,ExercisePhase::FullResponse] {ex.set_exercise_phase(phase,"EXCON confirms next incident stage").unwrap();}
    assert_eq!(ex.turn,1);assert_eq!(ex.world.view(),world);assert_eq!(ex.resource_history.len(),history);
    assert_eq!(ex.world.d.pools["fire_commander"]["ground_crews"].used,used);
    assert_eq!(ex.phase_history.len(),4);
    assert_eq!(ex.debrief()["scheduled_injects"][2]["delivered_s"],3600);
    ex.configure_clock(false,120.0,"Pause to review").unwrap();assert!(ex.advance_clock(1).is_err());
    assert!(ex.configure_clock(true,f64::NAN,"invalid").is_err());
    assert!(ex.schedule_inject("I-01",0).is_err());
    ex.configure_clock(true,120.0,"Resume").unwrap();ex.begin_resolution(Some(1)).unwrap();assert!(!ex.clock_control.running);
}

#[test]
fn scheduled_deliveries_and_explicit_execution_cannot_duplicate_after_rollback() {
    let mut ex=exercise();ex.start().unwrap();
    let schedule=ex.schedule_inject("I-01",0).unwrap();
    assert_eq!(ex.claim_due_injects().len(),1);assert!(ex.claim_due_injects().is_empty());
    ex.cancel_schedule(schedule,"Cancelled during translation").unwrap();
    ex.complete_schedule(schedule,Ok(vec![123]));assert_eq!(ex.scheduled_injects[0].status,ScheduleStatus::Cancelled);
    let pid=reply(&mut ex,"mayor","warn F4").unwrap();
    assert!(ex.execute_approved(pid).is_err());
    ex.attach_parse(pid,parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    ex.decide(pid,Decision{approve:true,..Default::default()}).unwrap();
    ex.execute_approved(pid).unwrap();assert_eq!(ex.proposal(pid).unwrap().status,ProposalStatus::Applied);
    assert_eq!(ex.turn,1);assert!(ex.execute_approved(pid).is_err());
    ex.configure_clock(true,60.0,"Run").unwrap();ex.advance_clock(60).unwrap();
    ex.rollback(1).unwrap();assert!(!ex.clock_control.running);assert!(ex.claim_due_injects().is_empty());
    assert_eq!(ex.proposal(pid).unwrap().executions.len(),1);
    assert!(ex.log.iter().any(|l|l.kind=="clock_paused"));
}

#[test]
fn total_and_phase_limits_count_deployed_and_committed_units_across_phases() {
    let mut ex=exercise();ex.start().unwrap();
    let limits:ResourceLimits=serde_json::from_value(json!({"total":{"ground_crews":2},"phases":{"early_warning":{"ground_crews":1},"early_action":{"ground_crews":2},"escalation":{"ground_crews":1}}})).unwrap();
    ex.set_resource_limits(limits.clone(),"Progressive tactical capacity",None).unwrap();
    let p1=reply(&mut ex,"fire_commander","Cut F4 north").unwrap();
    ex.attach_parse(p1,parsed(json!([{"type":"CutFireLine","along":"F4:N"}]))).unwrap();
    ex.decide(p1,Decision{approve:true,..Default::default()}).unwrap();
    let p2=reply(&mut ex,"fire_commander","Cut G4 north").unwrap();
    ex.attach_parse(p2,parsed(json!([{"type":"CutFireLine","along":"G4:N"}]))).unwrap();
    assert!(!ex.proposal(p2).unwrap().verdicts[0].is_ok());assert!(ex.decide(p2,Decision{approve:true,..Default::default()}).is_err());
    assert_eq!(ex.resource_accounting(Some("fire_commander"))["fire_commander"]["ground_crews"]["available"],0);
    ex.set_exercise_phase(ExercisePhase::EarlyAction,"Deploy second crew").unwrap();
    ex.decide(p2,Decision{approve:true,..Default::default()}).unwrap();
    ex.execute_approved(p1).unwrap();ex.execute_approved(p2).unwrap();
    assert_eq!(ex.world.d.pools["fire_commander"]["ground_crews"].used,2);
    let p3=reply(&mut ex,"fire_commander","Cut H4 north").unwrap();
    ex.attach_parse(p3,parsed(json!([{"type":"CutFireLine","along":"H4:N"}]))).unwrap();
    assert!(!ex.proposal(p3).unwrap().verdicts[0].is_ok());
    assert!(ex.set_exercise_phase(ExercisePhase::Escalation,"Lower cap cannot cancel effective work").is_err());
    assert_eq!(ex.exercise_phase,ExercisePhase::EarlyAction);
    let too_small:ResourceLimits=serde_json::from_value(json!({"total":{"ground_crews":1}})).unwrap();
    assert!(ex.set_resource_limits(too_small,"Reduce cap",None).is_err());assert_eq!(ex.resource_limits,limits);
    let arrival=ex.world.d.tasks.iter().map(|task|task.arrival_s).max().unwrap();
    ex.direct_command(&Command::AdvanceTime {minutes:((arrival+59)/60) as u32}).unwrap();
    let allocation=ex.world.d.allocations[0].id;
    ex.direct_command(&Command::ReturnResource{allocation,qty:1}).unwrap();
    ex.set_exercise_phase(ExercisePhase::Escalation,"One crew actually returned").unwrap();
    assert_eq!(ex.world.d.lines.len(),2);
    let ledger=ex.resource_accounting(Some("fire_commander"));let crews=&ledger["fire_commander"]["ground_crews"];
    assert_eq!(crews["effective_capacity"],1);assert_eq!(crews["deployed"],1);assert_eq!(crews["returned"],1);assert_eq!(crews["available"],0);
    assert!(ex.log.iter().any(|l|l.kind=="resource_limits_changed"));
    assert_eq!(ex.debrief()["resource_limits"]["total"]["ground_crews"],2);
}

#[test]
fn ai_limit_drafts_are_inert_until_master_approval_and_invalid_drafts_can_be_rejected() {
    let mut ex=exercise();ex.start().unwrap();
    let limits=json!({"total":{"ground_crews":1},"phases":{"early_warning":{"ground_crews":1}}});
    let pid=ex.master_prompt("Suggest realistic resource limits");
    ex.attach_parse(pid,ParsedInput{resource_limit_draft:Some(limits.clone()),..Default::default()}).unwrap();
    assert_eq!(ex.resource_limits,ResourceLimits::default());
    ex.decide(pid,Decision{approve:false,..Default::default()}).unwrap();assert_eq!(ex.resource_limits,ResourceLimits::default());
    let pid=ex.master_prompt("Recommend reviewed limits");
    ex.attach_parse(pid,ParsedInput{resource_limit_draft:Some(limits.clone()),..Default::default()}).unwrap();
    ex.decide(pid,Decision{approve:true,note:Some("Reviewed for warning phase".into()),..Default::default()}).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status,ProposalStatus::Applied);
    assert_eq!(serde_json::to_value(&ex.resource_limits).unwrap(),limits);
    assert!(ex.log.iter().any(|l|l.kind=="resource_limits_changed" && l.data["proposal"]==pid && l.data["reason"]=="Reviewed for warning phase"));
    let bad=ex.master_prompt("Invalid AI cap");
    ex.attach_parse(bad,ParsedInput{resource_limit_draft:Some(json!({"total":{"invented_resource":3}})),..Default::default()}).unwrap();
    assert!(!ex.proposal(bad).unwrap().invalid.is_empty());
    ex.decide(bad,Decision{approve:false,..Default::default()}).unwrap();
    assert_eq!(serde_json::to_value(&ex.resource_limits).unwrap(),limits);
}

#[test]
fn rollback_cannot_restore_deployments_above_current_reviewed_limits() {
    let mut ex=exercise();ex.start().unwrap();
    let pid=reply(&mut ex,"fire_commander","Cut F4 north").unwrap();
    ex.attach_parse(pid,parsed(json!([{"type":"CutFireLine","along":"F4:N"}]))).unwrap();ex.decide(pid,Decision{approve:true,..Default::default()}).unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    let allocation=ex.world.d.allocations[0].id;ex.direct_command(&Command::ReturnResource{allocation,qty:1}).unwrap();
    ex.set_resource_limits(serde_json::from_value(json!({"total":{"ground_crews":0}})).unwrap(),"All crews stood down",None).unwrap();
    let before=ex.world.view();let generation=ex.generation;
    assert!(ex.rollback(2).is_err());assert_eq!(ex.world.view(),before);assert_eq!(ex.generation,generation);
    ex.set_resource_limits(serde_json::from_value(json!({"total":{"ground_crews":1}})).unwrap(),"Review restoration capacity",None).unwrap();
    ex.rollback(2).unwrap();assert_eq!(ex.world.d.pools["fire_commander"]["ground_crews"].used,1);
}

#[test]
fn named_place_fire_line_player_action_terminates() {
    let mut ex = exercise();
    ex.start().unwrap();
    let cmd: Command = serde_json::from_value(json!({"type":"CutFireLine","from":"Pineta Fuel Station","to":"Pineta Primary School"})).unwrap();
    let pid = ex.player_actions("fire_commander", "Linea tagliafuoco · Pineta Fuel Station", vec![cmd], None, exercise::BasisKind::OperationalJudgment).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().verdicts.len(), 1);
}

/// The standalone WASM build runs the Valverde factory scenario with the
/// operational model, where ground crews walk with a 1.5 km access radius.
/// Validating their staging route used to run one Dijkstra and rebuild one
/// path per anchor pair (> 60 s native, a frozen browser tab in WASM).
#[test]
fn factory_operational_fire_line_validation_is_fast() {
    let mut scn: scenario::Scenario = serde_json::from_str(include_str!(
        "../../../data/scenarios/valverde_factory.json"
    ))
    .unwrap();
    scn.enable_operational_model();
    let world = World::with_options(
        Arc::new(scn),
        1,
        sim::FireOptions { realizations: 32, cutoff: 0.5 },
    )
    .unwrap();
    let mut ex = Exercise::new(world);
    ex.start().unwrap();
    let t = std::time::Instant::now();
    let cmd: Command = serde_json::from_value(json!({"type":"CutFireLine","from":"F4","to":"G4"})).unwrap();
    let pid = ex.player_actions("fire_commander", "Linea · F4", vec![cmd], None, exercise::BasisKind::OperationalJudgment).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().verdicts.len(), 1);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "validation took {:?}", t.elapsed());
}

#[test]
fn factory_operational_fire_line_reply_to_inject_is_fast() {
    let mut scn: scenario::Scenario = serde_json::from_str(include_str!(
        "../../../data/scenarios/valverde_factory.json"
    ))
    .unwrap();
    scn.enable_operational_model();
    let world = World::with_options(
        Arc::new(scn),
        1,
        sim::FireOptions { realizations: 32, cutoff: 0.5 },
    )
    .unwrap();
    let mut ex = Exercise::new(world);
    ex.start().unwrap();
    let parent = ex.send_inject(None, "Fire near Pineta", "EXCON", &["fire_commander".into()], "Report actions", &BTreeMap::new()).unwrap()[0];
    let t = std::time::Instant::now();
    for (from, to) in [("Pineta Fuel Station", "Pineta Primary School"), ("F5", "H5")] {
        let cmd: Command = serde_json::from_value(json!({"type":"CutFireLine","from":from,"to":to})).unwrap();
        let pid = ex.player_actions("fire_commander", "Linea", vec![cmd], Some(parent), exercise::BasisKind::OperationalJudgment).unwrap();
        assert_eq!(ex.proposal(pid).unwrap().verdicts.len(), 1);
        eprintln!("{from}->{to}: {:?} verdict={:?}", t.elapsed(), ex.proposal(pid).unwrap().verdicts[0]);
    }
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "validation took {:?}", t.elapsed());
}

#[test]
fn private_turn_summaries_freeze_role_evidence_and_discard_rollback_results() {
    let mut ex = exercise();
    ex.start().unwrap();
    ex.send_inject(None, "Visible", "EXCON", &["mayor".into()], "MAYOR_VISIBLE", &BTreeMap::new()).unwrap();
    ex.send_inject(None, "Private", "EXCON", &["fire_commander".into()], "FIRE_PRIVATE", &BTreeMap::new()).unwrap();
    ex.add_message("fire_commander", "regional_cp", MsgKind::Chat, None, "PRIVATE_THREAD", "PRIVATE_THREAD", None);
    ex.add_message("mayor", "regional_cp", MsgKind::Chat, None, "OWN_CONVERSATION", "OWN_CONVERSATION", None);
    ex.resolve_turn(Some(1)).unwrap();
    let summaries = ex.claim_turn_summaries();
    let (id, _, evidence) = summaries.iter().find(|(_, _, evidence)| evidence.to_string().contains("MAYOR_VISIBLE")).unwrap();
    let evidence_text = evidence.to_string();
    assert!(evidence_text.contains("OWN_CONVERSATION"));
    assert!(!evidence_text.contains("FIRE_PRIVATE"));
    assert!(!evidence_text.contains("PRIVATE_THREAD"));
    assert!(!evidence_text.contains("text_en"));
    assert_eq!(evidence["turn"], 1);
    assert!(ex.claim_turn_summaries().is_empty(), "one AI call per role and turn");
    ex.complete_turn_summary(*id, Some("Private recap".into()));
    assert_eq!(ex.player_turn_summaries("mayor")[0].text, "Private recap");
    assert!(ex.player_turn_summaries("fire_commander").iter().all(|s| s.text != "Private recap"));
    ex.rollback(1).unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    ex.claim_turn_summaries();
    ex.complete_turn_summary(*id, Some("Stale response".into()));
    assert!(ex.player_turn_summaries("mayor").iter().all(|s| s.text != "Stale response"));
    assert_eq!(ex.player_turn_summaries("mayor").len(), 1);
    // Serialized role responses contain no hidden prompt/evidence.
    let response = serde_json::to_value(ex.player_turn_summaries("mayor")).unwrap();
    assert!(response[0].get("evidence").is_none());
}

#[test]
fn edited_flag_is_true_only_when_actions_actually_change() {
    let acts = json!([{"type":"IssueWarning","zone":"F4"}]);
    // Note only, and re-submitting identical commands: not edited.
    for resubmit in [false, true] {
        let mut ex = exercise();
        ex.start().unwrap();
        let pid = reply(&mut ex, "mayor", "warn F4").unwrap();
        ex.attach_parse(pid, parsed(acts.clone())).unwrap();
        let same = ex.proposal(pid).unwrap().commands.clone();
        let commands = if resubmit { Some(same) } else { None };
        ex.decide(pid, Decision { approve: true, commands, note: Some("ok".into()), ..Default::default() }).unwrap();
        assert!(!ex.proposal(pid).unwrap().decision.as_ref().unwrap().edited);
    }
    // Real change: edited.
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "warn F4").unwrap();
    ex.attach_parse(pid, parsed(acts)).unwrap();
    let other: Vec<Command> = serde_json::from_value(json!([{"type":"IssueWarning","zone":"G4"}])).unwrap();
    ex.decide(pid, Decision { approve: true, commands: Some(other), note: None, ..Default::default() }).unwrap();
    assert!(ex.proposal(pid).unwrap().decision.as_ref().unwrap().edited);
}

#[test]
fn excon_clarification_request_keeps_the_proposal_open() {
    let mut ex = exercise();
    // Not during briefing.
    assert!(ex.request_clarification(1, "where?").is_err());
    ex.start().unwrap();
    let pid = reply(&mut ex, "fire_commander", "warn F4").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    // A note is required.
    assert!(ex.request_clarification(pid, "  ").is_err());
    let req = ex.request_clarification(pid, "Which side of F4?").unwrap();
    assert_eq!((req.role.as_str(), req.text_en.as_str()), ("fire_commander", "Which side of F4?"));
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::NeedsClarification);
    assert_eq!(p.clarification_request.as_deref(), Some("Which side of F4?"));
    assert_eq!(p.history.last().unwrap().actor, MASTER);
    assert!(ex.log.iter().any(|e| e.kind == "clarification_requested"));
    // Delivered to the player in the proposal's thread.
    ex.deliver_clarification(pid, "Quale lato di F4?", "Which side of F4?").unwrap();
    let m = ex.inbox("fire_commander").last().cloned().unwrap();
    assert_eq!(m.kind, MsgKind::Clarification);
    assert_eq!(m.text, "Quale lato di F4?");
    assert_eq!(m.proposal, Some(pid));
    assert_eq!(m.inject_id, ex.proposal(pid).unwrap().inject_id);
    // The proposal can still be decided afterwards.
    ex.decide(pid, Decision { approve: false, note: Some("no".into()), ..Default::default() }).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Rejected);
    // Decided proposals and EXCON proposals cannot be sent back.
    assert!(ex.request_clarification(pid, "again?").is_err());
    assert!(ex.deliver_clarification(pid, "x", "x").is_err());
    let mp = ex.master_prompt("do something");
    assert!(ex.request_clarification(mp, "what?").is_err());
}

#[test]
fn revalidate_updates_verdicts_without_deciding() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = reply(&mut ex, "mayor", "evacuate F4").unwrap();
    ex.attach_parse(pid, parsed(json!([{"type":"EvacuateZone","zone":"F4"}]))).unwrap();
    assert!(!ex.proposal(pid).unwrap().verdicts[0].is_ok(), "needs more buses than the mayor has");
    // Same commands: still infeasible, status untouched.
    ex.revalidate_proposal(pid, None).unwrap();
    assert!(!ex.proposal(pid).unwrap().verdicts[0].is_ok());
    // Edited commands: feasible, descriptions follow, still undecided.
    let edited: Vec<Command> = serde_json::from_value(json!([{"type":"IssueWarning","zone":"F4"}])).unwrap();
    ex.revalidate_proposal(pid, Some(edited)).unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert!(p.decision.is_none());
    assert!(p.verdicts.iter().all(|v| v.is_ok()));
    assert_eq!(p.descriptions.len(), 1);
    assert!(ex.log.iter().any(|e| e.kind == "proposal_revalidated"));
    // Decided proposals cannot be revalidated.
    ex.decide(pid, Decision { approve: false, ..Default::default() }).unwrap();
    assert!(ex.revalidate_proposal(pid, None).is_err());
}

#[test]
fn player_answer_is_a_message_and_keeps_the_clarified_proposal() {
    let mut ex = exercise();
    ex.start().unwrap();
    let old = reply(&mut ex, "fire_commander", "warn").unwrap();
    ex.attach_parse(old, parsed(json!([{"type":"IssueWarning","zone":"F4"}]))).unwrap();
    ex.request_clarification(old, "Which zone?").unwrap();
    let qid = ex.deliver_clarification(old, "Quale zona?", "Which zone?").unwrap();
    let before = ex.inject_responses();
    assert!(before.iter().any(|r| r["pending_proposals"].as_array().unwrap().iter().any(|p| p["id"] == old)));
    let (_, same) = ex.answer_clarification("fire_commander", "F4", qid, "F4", false).unwrap();
    assert_eq!(same, old);
    let o = ex.proposal(old).unwrap();
    assert_eq!(o.status, ProposalStatus::Pending);
    assert_eq!((o.superseded_by, o.commands.len()), (None, 1));
    assert!(ex.log.iter().any(|e| e.kind == "clarification_answered"));
    // Each row carries its whole thread in message order: inject, replies, question, answer.
    let row = ex
        .inject_responses()
        .into_iter()
        .find(|r| r["role"] == "fire_commander" && !r["replies"].as_array().unwrap().is_empty())
        .expect("row with replies");
    assert!(row["pending_proposals"].as_array().unwrap().iter().any(|p| p["id"] == old && p["status"] == "pending"));
    let thread = row["thread"].as_array().unwrap();
    assert_eq!(thread[0]["kind"], "inject");
    assert!(thread.iter().any(|t| t["kind"] == "clarification"));
    assert_eq!(thread.iter().filter(|t| t["kind"] == "reply").count(), 2, "declaration and answer");
    assert!(thread.windows(2).all(|w| w[0]["id"].as_u64() < w[1]["id"].as_u64()));
    // An unrelated reply is its own proposal.
    let other = reply(&mut ex, "mayor", "x").unwrap();
    assert_eq!(ex.proposal(other).unwrap().supersedes, None);
}

fn act(id: &str, offset_s: i64, wind: f64) -> scenario::InjectAction {
    scenario::InjectAction {
        id: id.into(),
        title: format!("Action {id}"),
        command: json!({"type": "SetWeather", "wind_dir_deg": wind}),
        reveal: None,
        offset_s,
        note: String::new(),
    }
}

/// First inject planned at 600 s with a post-action (offset 0), a delayed one
/// (+120 s) and a pre-action (-300 s, so at 300 s). Every other inject is far away.
fn with_actions() -> (Exercise, String) {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    for (n, i) in scn.msel.iter_mut().enumerate() {
        i.at_s = Some(100_000 + n as i64 * 60);
    }
    scn.msel[0].at_s = Some(600);
    scn.msel[0].actions = vec![act("wind_shift", 0, 123.0), act("late_wind", 120, 200.0), act("pre_wind", -300, 100.0)];
    let id = scn.msel[0].id.clone();
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = 30;
    (ex, id)
}

fn scan_to(ex: &mut Exercise, to_s: i64) {
    ex.begin_resolution_until(to_s, true).unwrap();
    while !ex.step_resolution(exercise::STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
}

fn send(ex: &mut Exercise, id: &str, to: &str, actions: &exercise::actions::SendActions) {
    ex.send_inject_with(Some(id), "T", "112", &[to.into()], "text", &BTreeMap::new(), actions).unwrap();
}

#[test]
fn sending_an_inject_runs_its_actions() {
    use exercise::actions::{ActionStatus, SendActions};
    let (mut ex, id) = with_actions();
    send(&mut ex, &id, "mayor", &SendActions::all());
    let run = |ex: &Exercise, k: &str| ex.action_runs.iter().find(|(_, r)| r.action == k).map(|(_, r)| r.clone()).unwrap();
    assert_eq!(run(&ex, "wind_shift").status, ActionStatus::Done, "delay 0 applies at send");
    let late = run(&ex, "late_wind");
    assert_eq!((late.status, late.run_at_s), (ActionStatus::Approved, Some(120)));
    assert_eq!(run(&ex, "pre_wind").status, ActionStatus::Done, "an early send runs the pending pre-action");
    // A second send to another role never runs them again.
    let n = ex.action_runs.len();
    send(&mut ex, &id, "fire_commander", &SendActions::all());
    assert_eq!(ex.action_runs.len(), n);
    // The view lists the actions under the inject.
    let row = ex.planned_injects().into_iter().find(|i| i["id_inject"] == id.as_str()).unwrap();
    assert_eq!(row["actions"].as_array().unwrap().len(), 3);
    assert_eq!(row["actions"][0]["status"], "done");
}

#[test]
fn unchecked_actions_do_not_run() {
    use exercise::actions::{ActionStatus, SendActions};
    let (mut ex, id) = with_actions();
    send(&mut ex, &id, "mayor", &SendActions { run: Some(vec!["wind_shift".into()]), custom: vec![] });
    let status = |ex: &Exercise, k: &str| ex.action_runs.values().find(|r| r.action == k).unwrap().status;
    assert_eq!(status(&ex, "wind_shift"), ActionStatus::Done);
    assert_eq!(status(&ex, "late_wind"), ActionStatus::NotRun);
    assert_eq!(status(&ex, "pre_wind"), ActionStatus::NotRun);
    scan_to(&mut ex, 1000);
    assert_eq!(status(&ex, "late_wind"), ActionStatus::NotRun, "never executed");
}

#[test]
fn delayed_action_runs_at_its_instant_and_rollback_forgets_it() {
    use exercise::actions::{ActionStatus, SendActions};
    let (mut ex, id) = with_actions();
    send(&mut ex, &id, "mayor", &SendActions::all());
    ex.begin_resolution_until(300, true).unwrap();
    while ex.world.time_s() < 119 {
        ex.step_resolution(exercise::STEP_S).unwrap();
    }
    let late = |ex: &Exercise| ex.action_runs.values().find(|r| r.action == "late_wind").unwrap().clone();
    assert_eq!(late(&ex).status, ActionStatus::Approved);
    ex.step_resolution(exercise::STEP_S).unwrap();
    ex.step_resolution(exercise::STEP_S).unwrap();
    assert_eq!((late(&ex).status, late(&ex).executed_s), (ActionStatus::Done, Some(120)));
    while !ex.step_resolution(exercise::STEP_S).unwrap() {}
    ex.finish_resolution().unwrap();
    ex.rollback(1).unwrap();
    assert!(ex.action_runs.is_empty(), "decisions taken after the restored time are forgotten");
}

#[test]
fn custom_inject_actions_run_and_cannot_precede_it() {
    use exercise::actions::{ActionStatus, SendActions};
    let (mut ex, _) = with_actions();
    let bad = SendActions { run: None, custom: vec![act("c1", -60, 90.0)] };
    assert!(ex.send_inject_with(None, "T", "EXCON", &["mayor".into()], "x", &BTreeMap::new(), &bad).is_err());
    assert!(ex.messages.is_empty(), "nothing is sent when an action is invalid");
    let ok = SendActions { run: None, custom: vec![act("c1", 0, 90.0), act("c2", 60, 80.0)] };
    ex.send_inject_with(None, "T", "EXCON", &["mayor".into()], "x", &BTreeMap::new(), &ok).unwrap();
    let st = |k: &str| ex.action_runs.values().find(|r| r.action == k).unwrap().status;
    assert_eq!((st("c1"), st("c2")), (ActionStatus::Done, ActionStatus::Approved));
}

#[test]
fn pre_actions_run_during_the_scan_and_can_be_disabled() {
    use exercise::actions::ActionStatus;
    let (mut ex, id) = with_actions();
    assert_eq!(ex.next_pending_inject().unwrap().0, 600);
    scan_to(&mut ex, 600);
    let pre = ex.action_runs.values().find(|r| r.action == "pre_wind").unwrap();
    assert_eq!((pre.status, pre.executed_s), (ActionStatus::Done, Some(300)), "{pre:?}");
    assert!(ex.action_runs.values().all(|r| r.action == "pre_wind"), "post-actions wait for the send");

    // Disabled beforehand: it does not run.
    let (mut ex, id2) = with_actions();
    ex.set_action_enabled(&id2, "pre_wind", false).unwrap();
    scan_to(&mut ex, 600);
    assert!(ex.action_runs.is_empty());
    let row = ex.planned_injects().into_iter().find(|i| i["id_inject"] == id2.as_str()).unwrap();
    assert_eq!(row["actions"][2]["status"], "not_run");
    assert!(ex.set_action_enabled(&id2, "wind_shift", false).is_err(), "only pre-actions toggle");
    let _ = id;
}

#[test]
fn skipping_an_inject_cancels_its_pre_actions_and_rollback_forgets_the_skip() {
    let (mut ex, id) = with_actions();
    ex.skip_inject(&id).unwrap();
    scan_to(&mut ex, 600);
    assert!(ex.action_runs.is_empty(), "a skipped inject's pre-actions never run");
    // Skipped injects do not count as pending nor as due.
    assert_ne!(ex.next_pending_inject().map(|p| p.1), Some(id.clone()));
    let item = ex.turn_agenda()["items"].as_array().unwrap().iter().find(|i| i["id"] == id.as_str()).cloned().unwrap();
    assert_eq!((item["status"].as_str(), item["due"].as_bool()), (Some("skipped"), Some(false)));
    ex.restore_inject(&id).unwrap();
    assert_eq!(ex.skipped_injects.len(), 0);
    ex.skip_inject(&id).unwrap();
    ex.rollback(1).unwrap();
    assert!(ex.skipped_injects.is_empty(), "skip decided after the restored time is forgotten");
}

#[test]
fn advance_target_stops_at_the_next_inject_or_closes_the_turn() {
    let (mut ex, id) = with_actions();
    let t = ex.advance_target();
    assert_eq!((t.to_s, t.closes_turn, t.inject.as_deref()), (600, false, Some(id.as_str())));
    assert_eq!(ex.turn_agenda()["next_event_s"], 600);
    // Clamped to the turn end: the advance closes the turn there.
    ex.skip_inject(&id).unwrap();
    let t = ex.advance_target();
    assert_eq!((t.to_s, t.closes_turn, t.inject), (1800, true, None));
    ex.restore_inject(&id).unwrap();
    ex.turn_minutes = 5;
    let t = ex.advance_target();
    assert_eq!((t.to_s, t.closes_turn), (300, true));
    ex.turn_minutes = 30;
    // Reaching the inject makes it due.
    scan_to(&mut ex, 600);
    let item = ex.turn_agenda()["items"].as_array().unwrap().iter().find(|i| i["id"] == id.as_str()).cloned().unwrap();
    assert_eq!(item["due"], true);
    assert!(ex.sent_injects.is_empty(), "never sent automatically");
    // Still pending and overdue: the next target is the turn end.
    assert!(ex.advance_target().closes_turn);
}

#[test]
fn substep_advances_inside_the_turn_without_closing_it() {
    let (mut ex, _) = with_actions();
    let agenda = ex.turn_agenda();
    assert_eq!((agenda["turn_start_s"].as_i64(), agenda["planned_end_s"].as_i64()), (Some(0), Some(1800)));
    scan_to(&mut ex, 600);
    assert_eq!((ex.turn, ex.phase, ex.world.time_s(), ex.turn_start_s), (1, Phase::Orders, 600, 0));
    assert_eq!(ex.substeps.len(), 1);
    // Closing the turn runs to the planned end, not another full turn.
    ex.resolve_turn(None).unwrap();
    assert_eq!((ex.world.time_s(), ex.turn, ex.turn_start_s), (1800, 2, 1800));
    assert!(ex.substeps.is_empty());
    ex.rollback(1).unwrap();
    assert_eq!((ex.turn_start_s, ex.world.time_s()), (0, 0));
    assert!(ex.substeps.is_empty());
}

#[test]
fn stopping_a_resolution_keeps_the_turn_open_at_the_current_second() {
    use exercise::actions::{ActionStatus, SendActions};
    let (mut ex, id) = with_actions();
    assert!(ex.stop_resolution().is_err(), "nothing is running yet");
    // A delayed action (+120 s) is approved; stop before the end, it stays approved.
    let mut custom = act("slow", 1000, 70.0);
    custom.title = "slow".into();
    let acts = SendActions { run: None, custom: vec![custom] };
    ex.send_inject_with(None, "T", "EXCON", &["mayor".into()], "x", &BTreeMap::new(), &acts).unwrap();
    let _ = id;
    ex.begin_resolution(None).unwrap();
    for _ in 0..100 {
        assert!(!ex.step_resolution(exercise::STEP_S).unwrap());
    }
    let at = ex.stop_resolution().unwrap();
    assert_eq!(at, ex.world.time_s());
    assert!(ex.step_resolution(exercise::STEP_S).unwrap(), "the resolution is over");
    assert_eq!(ex.world.time_s(), at, "no further second is simulated");
    let report = ex.finish_resolution().unwrap();
    assert_eq!((report.from_s, report.to_s), (0, at));
    assert_eq!((ex.turn, ex.phase), (1, Phase::Orders), "the turn stays open");
    assert_eq!(ex.substeps.len(), 1);
    assert_eq!(ex.last_turn.as_ref().map(|w| w.to_s), Some(at));
    let slow = |ex: &Exercise| ex.action_runs.values().find(|r| r.action == "slow").unwrap().status;
    assert_eq!(slow(&ex), ActionStatus::Approved);
    ex.resolve_turn(None).unwrap();
    assert_eq!((ex.world.time_s(), ex.turn), (1800, 2));
    assert_eq!(slow(&ex), ActionStatus::Done);
}

#[test]
fn inject_at_s_places_it_in_the_agenda() {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.msel[0].at_s = Some(600);
    let id = scn.msel[0].id.clone();
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = 30;
    let agenda = ex.turn_agenda();
    let item = agenda["items"].as_array().unwrap().iter().find(|i| i["id"] == id.as_str()).unwrap();
    assert_eq!(item["at_s"], 600);
    assert_eq!(item["due"], false);
    // Advancing to it makes it due: proposed to EXCON, never sent automatically.
    scan_to(&mut ex, 600);
    let agenda = ex.turn_agenda();
    let item = agenda["items"].as_array().unwrap().iter().find(|i| i["id"] == id.as_str()).unwrap();
    assert_eq!(item["due"], true);
    assert!(ex.sent_injects.is_empty());
}

/// The default scenario no longer starts burning: the ignition is an action of
/// inject I-01, which runs when EXCON sends it.
#[test]
fn default_scenario_ignition_is_an_action_of_the_first_inject() {
    use exercise::actions::SendActions;
    let scn: scenario::Scenario = serde_json::from_str(include_str!(
        "../../../data/scenarios/valverde_factory.json"
    ))
    .unwrap();
    assert_eq!(scn.msel.iter().find(|i| i.id == "I-01").unwrap().actions[0].id, "A-01");
    let world = World::with_options(
        Arc::new(scn),
        1,
        sim::FireOptions { realizations: 32, cutoff: 0.5 },
    )
    .unwrap();
    let mut ex = Exercise::new(world);
    ex.start().unwrap();
    assert!(ex.world.burning_zones().is_empty());
    ex.send_inject_with(Some("I-01"), "Fire", "112", &["all".into()], "Fire", &BTreeMap::new(), &SendActions::all()).unwrap();
    let run = ex.action_runs.values().find(|r| r.action == "A-01").unwrap().clone();
    assert_eq!(run.status, exercise::actions::ActionStatus::Done, "{run:?}");
    ex.resolve_turn(Some(30)).unwrap();
    assert!(!ex.world.burning_zones().is_empty(), "the fire must be burning after the ignition ran");
}

/// The turn length is part of the scenario; EXCON changing the live length
/// does not move injects that have no explicit time.
#[test]
fn live_turn_length_does_not_move_implied_inject_times() {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.turn_minutes = 20;
    scn.msel[0].turn = 3;
    scn.msel[0].at_s = None;
    let id = scn.msel[0].id.clone();
    scn.check().unwrap();
    let mut ex = Exercise::new(World::new(Arc::new(scn), 1).unwrap());
    assert_eq!(ex.turn_minutes, 20, "initialised from the scenario");
    let due = |ex: &Exercise| {
        ex.planned_injects().iter().find(|i| i["id_inject"] == id.as_str()).unwrap()["scenario_time_s"].as_i64().unwrap()
    };
    assert_eq!(due(&ex), 2 * 20 * 60);
    ex.turn_minutes = 45;
    assert_eq!(due(&ex), 2 * 20 * 60);
}

#[test]
fn building_or_place_written_as_a_zone_is_adapted_in_the_proposal() {
    let mut ex = exercise();
    ex.start().unwrap();
    for (name, building) in [("Pineta Primary School", true), ("Pineta", false)] {
        let pid = reply(&mut ex, "mayor", "evacuate").unwrap();
        ex.attach_parse(pid, parsed(json!([{"type":"EvacuateZone","zone":name}]))).unwrap();
        let p = ex.proposal(pid).unwrap();
        assert_eq!(matches!(p.commands[0], Command::EvacuateBuilding { .. }), building, "{name}: {:?}", p.commands);
        let reasons = format!("{:?}", p.verdicts);
        assert!(!reasons.contains("unknown zone"), "{name}: {reasons}");
        if let sim::Verdict::Ok { notes } = &p.verdicts[0] {
            assert!(notes[0].contains(name), "{notes:?}");
        }
    }
}

#[test]
fn language_choice_keeps_the_scenario_language_as_proposal() {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap()).with_language_choice();
    let mut pending = ex.pending_languages();
    pending.sort();
    assert_eq!(pending, ["fire_commander", "mayor", "regional_cp"]);
    assert_eq!(ex.players["mayor"].language, "it");
    ex.set_language("mayor", "it").unwrap();
    assert!(!ex.pending_languages().contains(&"mayor".to_string()));
}
