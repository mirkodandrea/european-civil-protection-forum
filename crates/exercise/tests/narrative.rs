use exercise::*;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
fn fixture() -> (Exercise, u64) {
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
            "112",
            &["mayor".into()],
            "Organise psychological support",
            &BTreeMap::new(),
        )
        .unwrap();
    (ex, ids[0])
}
fn submit(ex: &mut Exercise, parent: u64) -> u64 {
    ex.player_narrative(
        "mayor",
        "Attivo supporto psicologico",
        parent,
        "Activate psychological support",
        false,
    )
    .unwrap()
}
fn approve(ex: &mut Exercise, p: u64) {
    ex.decide(
        p,
        Decision {
            approve: true,
            ..Default::default()
        },
    )
    .unwrap();
}
#[test]
fn approval_and_outcome_never_execute_or_allocate_physical_resources() {
    let (mut ex, parent) = fixture();
    let before = ex.world.view();
    let p = submit(&mut ex, parent);
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::Pending);
    assert!(ex.proposal(p).unwrap().narrative);
    assert!(ex.proposal(p).unwrap().commands.is_empty());
    assert!(ex
        .record_narrative_outcome(
            p,
            "Team available",
            "Team available",
            "Team available",
            false
        )
        .is_err());
    approve(&mut ex, p);
    assert!(ex.execute_approved(p).is_err());
    ex.record_narrative_outcome(
        p,
        "EXCON: team available at shelter",
        "EXCON: team available at shelter",
        "Squadra disponibile al rifugio",
        false,
    )
    .unwrap();
    assert_eq!(ex.world.view(), before);
    let p = ex.proposal(p).unwrap();
    assert_eq!(p.status, ProposalStatus::Approved);
    assert!(p.executions.is_empty());
    assert_eq!(p.narrative_outcomes.len(), 1);
    let row = ex
        .inject_responses()
        .into_iter()
        .find(|r| r["role"] == "mayor")
        .unwrap();
    assert_eq!(row["replies"][0]["proposal"], p.id);
    let ev = ex.evaluator_view(&Default::default()).unwrap();
    assert_eq!(ev["chains"][0]["proposal"]["narrative"], true);
}
#[test]
fn narrative_questions_keep_the_original_initiative_and_rejection_blocks_outcomes() {
    let (mut ex, parent) = fixture();
    let p = submit(&mut ex, parent);
    ex.request_clarification(p, "Which shelter?").unwrap();
    let q = ex
        .deliver_clarification(p, "Quale rifugio?", "Which shelter?")
        .unwrap();
    let (_, same) = ex
        .answer_clarification("mayor", "Centro sportivo", q, "Sports centre", false)
        .unwrap();
    assert_eq!(same, p);
    assert_eq!(ex.proposals.len(), 1);
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::Pending);
    assert!(ex.proposal(p).unwrap().narrative);
    ex.decide(
        p,
        Decision {
            approve: false,
            note: Some("Support unavailable".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(ex
        .record_narrative_outcome(p, "Done", "Done", "Done", false)
        .is_err());
}
#[test]
fn explicit_narrative_path_cannot_bypass_operational_validation_or_role_visibility() {
    let (mut ex, parent) = fixture();
    assert!(ex
        .player_narrative("fire_commander", "search", parent, "search", false)
        .is_err());
    assert!(ex
        .player_narrative("mayor", " ", parent, "search", false)
        .is_err());
    assert!(ex
        .player_narrative("mayor", &"x".repeat(8001), parent, "search", false)
        .is_err());
    let p = submit(&mut ex, parent);
    let command = sim::Command::IssueWarning { zone: "F4".into() };
    assert!(ex
        .revalidate_proposal(p, Some(vec![command.clone()]))
        .is_err());
    assert!(ex
        .attach_parse(
            p,
            ParsedInput {
                actions: vec![json!(command)],
                ..Default::default()
            }
        )
        .is_err());
    let normal = ex
        .player_reply(
            "mayor",
            "hello",
            parent,
            DecisionEvidence::judgment("inject"),
        )
        .unwrap();
    ex.attach_parse(normal, ParsedInput::default()).unwrap();
    assert!(ex
        .decide(
            normal,
            Decision {
                approve: true,
                ..Default::default()
            }
        )
        .is_err());
    assert!(!ex
        .inbox("fire_commander")
        .iter()
        .any(|m| m.proposal == Some(p)));
}
#[test]
fn advancing_does_not_execute_narratives_and_rollback_restores_prior_outcome() {
    let (mut ex, parent) = fixture();
    let p = submit(&mut ex, parent);
    approve(&mut ex, p);
    let first = ex
        .record_narrative_outcome(
            p,
            "Preparation started",
            "Preparation started",
            "Preparazione avviata",
            false,
        )
        .unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::Approved);
    assert!(ex.proposal(p).unwrap().executions.is_empty());
    let second = ex
        .record_narrative_outcome(
            p,
            "Preparation completed",
            "Preparation completed",
            "Preparazione completata",
            false,
        )
        .unwrap();
    ex.rollback(2).unwrap();
    let outcomes = &ex.proposal(p).unwrap().narrative_outcomes;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].message, first);
    assert!(!ex.messages.iter().any(|m| m.id == second));
    ex.rollback(1).unwrap();
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::RolledBack);
    assert!(ex.proposal(p).unwrap().narrative_outcomes.is_empty());
}
