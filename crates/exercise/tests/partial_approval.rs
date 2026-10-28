//! Per-action EXCON decisions: partial approval, acknowledgement of replies
//! without actions, and the review state shown in the lists.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::*;
use serde_json::json;
use sim::{Command, World};

fn exercise() -> Exercise {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap());
    for role in ex.role_ids() {
        let lang = ex.language_of(&role);
        ex.set_language(&role, &lang).unwrap();
    }
    ex.start().unwrap();
    ex
}

fn reply(ex: &mut Exercise, role: &str, text: &str) -> u64 {
    let parent = match ex.messages.iter().rev().find(|m| m.to == role && m.kind == MsgKind::Inject) {
        Some(m) => m.id,
        None => ex
            .send_inject(None, "Task", "EXCON", &[role.into()], "Exercise task", &BTreeMap::new())
            .unwrap()[0],
    };
    ex.player_reply(role, text, parent, DecisionEvidence::judgment("judgment")).unwrap()
}

fn parsed(actions: serde_json::Value) -> ParsedInput {
    ParsedInput {
        language: "en".into(),
        translation_en: "t".into(),
        actions: actions.as_array().unwrap().clone(),
        summary: "s".into(),
        ..Default::default()
    }
}

fn reject(index: usize, note: &str) -> ActionRejection {
    ActionRejection { index, note: Some(note.into()) }
}

/// Warning on F4 is feasible; the F4 evacuation needs more buses than the mayor has.
fn warn_and_evacuate(ex: &mut Exercise) -> u64 {
    let pid = reply(ex, "mayor", "Warn F4 and evacuate it");
    ex.attach_parse(
        pid,
        parsed(json!([{"type":"IssueWarning","zone":"F4"},{"type":"EvacuateZone","zone":"F4"}])),
    )
    .unwrap();
    assert!(!ex.proposal(pid).unwrap().verdicts[1].is_ok());
    pid
}

#[test]
fn partial_approval_keeps_the_right_actions_and_explains_the_rest() {
    let mut ex = exercise();
    let pid = warn_and_evacuate(&mut ex);
    assert_eq!(ex.proposal(pid).unwrap().review, Review::InReview);
    // Approving everything is refused (one action is infeasible) ...
    assert!(ex.decide(pid, Decision { approve: true, ..Default::default() }).is_err());
    // ... but the infeasible one can be rejected alone.
    let out = ex
        .decide(
            pid,
            Decision {
                approve: true,
                note: Some("Warn now".into()),
                rejected_actions: vec![reject(1, "Not enough buses for the whole zone")],
                ..Default::default()
            },
        )
        .unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Approved);
    assert_eq!(p.review, Review::PartiallyApproved);
    assert_eq!(p.commands, vec![Command::IssueWarning { zone: "F4".into() }]);
    let d = p.decision.as_ref().unwrap();
    assert!(d.approve);
    assert_eq!(d.review, Review::PartiallyApproved);
    assert_eq!(d.proposed_commands.len(), 2);
    assert_eq!(d.effective_commands.len(), 1);
    assert_eq!(d.rejected_actions.len(), 1);
    assert_eq!(d.rejected_actions[0].index, 1);
    assert!(matches!(d.rejected_actions[0].command, Command::EvacuateZone { .. }));
    assert_eq!(d.rejected_actions[0].note.as_deref(), Some("Not enough buses for the whole zone"));
    // The player is told which action was approved and which was not, and why.
    let f = &out.facts_en;
    assert!(f.contains("approved 1 of the 2 actions"), "{f}");
    assert!(f.contains("APPROVED: Issue warning to zone F4"), "{f}");
    assert!(f.contains("NOT APPROVED: Evacuate zone F4"), "{f}");
    assert!(f.contains("Reason: Not enough buses for the whole zone"), "{f}");
    assert!(f.contains("Note from"), "{f}");
    // The decision is in the event log with its review state.
    let logged = ex.log.iter().rev().find(|l| l.kind == "proposal_decided").unwrap();
    assert_eq!(logged.data["review"], "partially_approved");
    assert_eq!(logged.data["decision"]["rejected_actions"][0]["index"], 1);
    // Only the approved action runs at resolution.
    let report = ex.resolve_turn(Some(5)).unwrap();
    let mine: Vec<_> = report.applied.iter().filter(|a| a.proposal == pid).collect();
    assert_eq!(mine.len(), 1);
    assert!(mine[0].ok);
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Applied);
    assert_eq!(p.review, Review::PartiallyApproved, "execution keeps the verdict");
    // A rollback undoes the execution but keeps the decision record.
    ex.rollback(1).unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::RolledBack);
    assert_eq!(p.review, Review::PartiallyApproved);
    assert_eq!(p.decision.as_ref().unwrap().rejected_actions.len(), 1);
}

#[test]
fn rejecting_every_action_is_a_rejection_and_indices_are_checked() {
    let mut ex = exercise();
    let pid = warn_and_evacuate(&mut ex);
    for bad in [vec![reject(2, "x")], vec![reject(0, "x"), reject(0, "y")]] {
        assert!(ex
            .decide(pid, Decision { approve: true, rejected_actions: bad, ..Default::default() })
            .is_err());
        assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    }
    let out = ex
        .decide(
            pid,
            Decision {
                approve: true,
                rejected_actions: vec![reject(0, "Wrong zone"), ActionRejection { index: 1, note: None }],
                ..Default::default()
            },
        )
        .unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Rejected);
    assert_eq!(p.review, Review::Rejected);
    let d = p.decision.as_ref().unwrap();
    assert!(!d.approve);
    assert!(d.effective_commands.is_empty());
    assert_eq!(d.rejected_actions.len(), 2);
    assert!(out.facts_en.contains("NOT approved"), "{}", out.facts_en);
    assert!(out.facts_en.contains("Reason: Wrong zone"), "{}", out.facts_en);
}

#[test]
fn whole_rejection_keeps_per_action_reasons() {
    let mut ex = exercise();
    let pid = warn_and_evacuate(&mut ex);
    let out = ex
        .decide(
            pid,
            Decision {
                approve: false,
                note: Some("Resubmit".into()),
                rejected_actions: vec![reject(1, "Use the school buses")],
                ..Default::default()
            },
        )
        .unwrap();
    let d = ex.proposal(pid).unwrap().decision.clone().unwrap();
    assert_eq!(d.review, Review::Rejected);
    assert_eq!(d.rejected_actions.len(), 2, "a rejection rejects every action");
    assert_eq!(d.rejected_actions[0].note, None);
    assert_eq!(d.rejected_actions[1].note.as_deref(), Some("Use the school buses"));
    assert!(out.facts_en.contains("Reason: Use the school buses"));
    assert!(out.facts_en.contains("Resubmit"));
}

#[test]
fn a_reply_without_actions_is_acknowledged() {
    let mut ex = exercise();
    let pid = reply(&mut ex, "mayor", "How many buses can reach the sports centre?");
    ex.attach_parse(pid, parsed(json!([]))).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Pending);
    let out = ex
        .decide(
            pid,
            Decision {
                approve: true,
                note: Some("Three buses, 120 seats; the centre is closed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let p = ex.proposal(pid).unwrap();
    assert_eq!(p.status, ProposalStatus::Acknowledged);
    assert_eq!(p.review, Review::Acknowledged);
    assert!(p.executions.is_empty());
    assert!(out.facts_en.contains("acknowledged"), "{}", out.facts_en);
    assert!(out.facts_en.contains("Three buses"), "{}", out.facts_en);
    assert_eq!(out.notify.as_deref(), Some("mayor"));
    // Final: no second decision, nothing to resolve, and a rollback keeps it.
    assert!(ex.decide(pid, Decision { approve: true, ..Default::default() }).is_err());
    assert!(ex.execute_approved(pid).is_err());
    ex.resolve_turn(Some(1)).unwrap();
    ex.rollback(1).unwrap();
    assert_eq!(ex.proposal(pid).unwrap().status, ProposalStatus::Acknowledged);
}

#[test]
fn lists_report_the_review_state() {
    let mut ex = exercise();
    let a = warn_and_evacuate(&mut ex);
    let rows = ex.inject_responses();
    let row = rows.iter().find(|r| r["role"] == "mayor").unwrap();
    assert_eq!(row["pending_proposals"][0]["review"], "in_review");
    ex.request_clarification(a, "Which part of F4?").unwrap();
    assert_eq!(ex.proposal(a).unwrap().review, Review::ClarificationRequested);
    ex.decide(
        a,
        Decision { approve: true, rejected_actions: vec![reject(1, "no buses")], ..Default::default() },
    )
    .unwrap();
    let b = reply(&mut ex, "mayor", "And the school?");
    ex.attach_parse(b, parsed(json!([]))).unwrap();
    ex.decide(b, Decision { approve: false, note: Some("Already covered".into()), ..Default::default() })
        .unwrap();
    let rows = ex.inject_responses();
    let row = rows.iter().find(|r| r["role"] == "mayor").unwrap();
    let decided = row["decided_proposals"].as_array().unwrap();
    assert_eq!(decided.len(), 2);
    assert_eq!(decided[0]["review"], "partially_approved");
    assert_eq!((decided[0]["approved"].as_u64(), decided[0]["rejected"].as_u64()), (Some(1), Some(1)));
    assert_eq!(decided[1]["review"], "rejected");
    assert_eq!(decided[1]["note"], "Already covered");
    assert!(row["pending_proposals"].as_array().unwrap().is_empty());
    let v = serde_json::to_value(ex.proposal(a).unwrap()).unwrap();
    assert_eq!(v["review"], "partially_approved");
    assert_eq!(v["decision"]["rejected_actions"][0]["note"], "no buses");
}
