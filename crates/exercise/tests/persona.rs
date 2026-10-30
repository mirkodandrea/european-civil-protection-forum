//! Personas (who EXCON speaks as) and narrative outcomes that carry interventions.
use exercise::narrative::OutcomeInput;
use exercise::*;
use sim::Command;
use std::{collections::BTreeMap, sync::Arc};


/// A narrated decision (basis: operational judgement), as `Rispondi` sends it.
fn narrated(ex: &mut Exercise, role: &str, text: &str, parent: u64, text_en: &str, failed: bool) -> anyhow::Result<u64> {
    ex.player_response(
        role,
        exercise::response::PlayerResponse {
            kind: exercise::ResponseKind::Decision,
            basis: Some(exercise::BasisKind::OperationalJudgment),
            narrative: Some(text.into()),
            narrative_en: text_en.into(),
            translation_failed: failed,
            reply_to: Some(parent),
            ..Default::default()
        },
    )
}

fn fixture(sender: &str) -> (Exercise, u64) {
    let mut ex = Exercise::new(
        sim::World::new(
            Arc::new(scenario::synthetic::generate(
                scenario::synthetic::DEFAULT_SEED,
            )),
            1,
        )
        .unwrap(),
    );
    ex.start().unwrap();
    let ids = ex
        .send_inject(
            None,
            "People need assistance",
            sender,
            &["mayor".into()],
            "Organise psychological support",
            &BTreeMap::new(),
        )
        .unwrap();
    (ex, ids[0])
}

fn approved_narrative(ex: &mut Exercise, parent: u64) -> u64 {
    let p = narrated(ex, "mayor", "Attivo supporto", parent, "Activate support", false)
        .unwrap();
    ex.decide(
        p,
        Decision {
            approve: true,
            ..Default::default()
        },
    )
    .unwrap();
    p
}

fn evacuate_school() -> Command {
    Command::SetBuildingStatus {
        building: "Pineta Primary School".into(),
        damage: None,
        evacuated: Some(true),
    }
}

#[test]
fn persona_defaults_to_the_inject_sender_then_the_scenario_default() {
    let (mut ex, parent) = fixture("112 Dispatch");
    let delivery = ex.messages.iter().find(|m| m.id == parent).unwrap().inject_id;
    assert_eq!(ex.persona_for(None, delivery).as_deref(), Some("112 Dispatch"));
    assert_eq!(
        ex.persona_for(Some("Il Sindaco"), delivery).as_deref(),
        Some("Il Sindaco")
    );
    // "EXCON" is never a persona.
    assert_eq!(ex.persona_for(Some(" excon "), None), None);
    assert_eq!(ex.persona_name(None, "it"), "Sala operativa");
    assert_eq!(ex.persona_name(None, "en"), "Operations room");
    assert_eq!(ex.persona_name(Some("excon"), "en"), "Operations room");
    Arc::make_mut(&mut ex.world.scn).default_persona = "Control cell".into();
    assert_eq!(ex.persona_name(None, "it"), "Control cell");
    let personas = ex.personas();
    assert!(personas["roles"].as_array().unwrap().iter().any(|r| r["id"] == "mayor"));
    assert!(personas["senders"].as_array().is_some());
}

#[test]
fn clarification_and_decision_are_spoken_as_the_chosen_persona() {
    let (mut ex, parent) = fixture("112 Dispatch");
    let p = narrated(&mut ex, "mayor", "Attivo supporto", parent, "Activate support", false)
        .unwrap();
    ex.request_clarification(p, "Which shelter?").unwrap();
    // No explicit persona: the inject sender.
    let q = ex.deliver_clarification(p, "Quale rifugio?", "Which shelter?").unwrap();
    let m = ex.messages.iter().find(|m| m.id == q).unwrap();
    assert_eq!(m.persona.as_deref(), Some("112 Dispatch"));
    assert_eq!(ex.message_persona(m, "it").as_deref(), Some("112 Dispatch"));
    let out = ex
        .decide(
            p,
            Decision {
                approve: true,
                note: Some("proceed".into()),
                persona: Some("Prefetto".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.persona.as_deref(), Some("Prefetto"));
    assert!(out.facts_en.contains("Note from Prefetto: proceed"));
    assert!(!out.facts_en.contains("EXCON"));
    // The player's thread resolves the persona too.
    let threads = ex.player_clarification_threads("mayor");
    let asked = threads[0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["author"] == "excon")
        .unwrap();
    assert_eq!(asked["persona"], "112 Dispatch");
}

#[test]
fn outcome_with_actions_applies_interventions_links_them_and_rolls_back() {
    let (mut ex, parent) = fixture("112");
    let p = approved_narrative(&mut ex, parent);
    let before = ex.world.view();
    let mid = ex
        .record_outcome(
            p,
            OutcomeInput {
                text: "Squadra al rifugio".into(),
                text_en: "Team at the shelter".into(),
                player_text: "Squadra al rifugio".into(),
                persona: Some("Protezione Civile".into()),
                commands: vec![evacuate_school()],
                ..Default::default()
            },
        )
        .unwrap();
    // Orders are open: the intervention waits for play.
    assert_eq!(ex.world.view(), before, "nothing changes under the players' hands");
    assert_eq!(ex.excon_queue().len(), 1);
    let o = ex.proposal(p).unwrap().narrative_outcomes[0].clone();
    assert_eq!(o.persona.as_deref(), Some("Protezione Civile"));
    assert_eq!(o.interventions.len(), 1);
    let entry = ex
        .log
        .iter()
        .find(|l| l.seq == o.interventions[0].log_seq)
        .unwrap();
    assert_eq!(entry.kind, "excon_action_queued");
    assert_eq!(entry.data["origin"]["origin"], "narrative_outcome");
    assert_eq!(entry.data["origin"]["proposal"], p);
    let msg = ex.messages.iter().find(|m| m.id == mid).unwrap();
    assert_eq!(msg.persona.as_deref(), Some("Protezione Civile"));
    // Play applies it.
    ex.begin_resolution(Some(1)).unwrap();
    assert_ne!(ex.world.view(), before, "the intervention changed the world");
    let applied = ex.log.iter().find(|l| l.kind == "master_command").unwrap();
    assert_eq!(applied.data["queued_origin"]["origin"], "narrative_outcome");
    while !ex.step_resolution(60).unwrap() {}
    ex.finish_resolution().unwrap();
    // Rollback to the turn start removes the outcome, its message, its effect and its queue entry.
    ex.rollback(1).unwrap();
    assert!(ex.excon_queue().is_empty());
    assert!(ex.proposal(p).unwrap().narrative_outcomes.is_empty());
    assert!(!ex.messages.iter().any(|m| m.id == mid));
    assert_eq!(ex.world.view(), before);
}

#[test]
fn outcome_needs_text_or_actions_and_refused_actions_change_nothing() {
    let (mut ex, parent) = fixture("112");
    let p = approved_narrative(&mut ex, parent);
    assert!(ex.record_outcome(p, OutcomeInput::default()).is_err());
    let before = ex.world.view();
    let unknown = Command::SetBuildingStatus {
        building: "Nowhere Hall".into(),
        damage: None,
        evacuated: Some(true),
    };
    let err = ex.record_outcome(
        p,
        OutcomeInput {
            commands: vec![evacuate_school(), unknown],
            ..Default::default()
        },
    );
    assert!(err.is_err());
    assert_eq!(ex.world.view(), before, "all or nothing");
    assert!(ex.proposal(p).unwrap().narrative_outcomes.is_empty());
    // Actions alone are enough; the player reads what was done.
    let mid = ex
        .record_outcome(
            p,
            OutcomeInput {
                commands: vec![evacuate_school()],
                ..Default::default()
            },
        )
        .unwrap();
    let m = ex.messages.iter().find(|m| m.id == mid).unwrap();
    assert!(m.text.contains("Pineta Primary School") || !m.text.is_empty());
    assert!(!m.text.contains("EXCON"));
}
