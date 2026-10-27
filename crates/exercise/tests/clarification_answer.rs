//! A player's answer to an EXCON clarification question carries the requested
//! action: it must reach the EXCON as a pending, approvable proposal inside the
//! inject's responses, by every route a player has.
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::threads::ThreadStatus;
use exercise::*;
use serde_json::json;
use sim::World;

fn exercise() -> Exercise {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = Exercise::new(World::new(scn, 1).unwrap());
    for role in ex.role_ids() {
        let lang = ex.language_of(&role);
        ex.set_language(&role, &lang).unwrap();
    }
    ex
}

fn ev() -> DecisionEvidence {
    DecisionEvidence::judgment("Based on the received inject")
}

fn parsed(en: &str) -> ParsedInput {
    ParsedInput {
        language: "it".into(),
        translation_en: en.into(),
        actions: vec![json!({"type":"IssueWarning","zone":"F4"})],
        summary: "s".into(),
        ..Default::default()
    }
}

fn row_of(ex: &Exercise, role: &str) -> serde_json::Value {
    ex.inject_responses().into_iter().find(|r| r["role"] == role).unwrap()
}

fn warn() -> Vec<sim::Command> {
    vec![sim::Command::IssueWarning { zone: "F4".into() }]
}

/// The answer is a message to the EXCON, not interpreted: the questioned proposal
/// keeps its commands, returns to Pending, and the answer shows in the thread.
#[test]
fn answer_is_a_plain_message_and_the_proposal_stays_approvable() {
    let mut ex = exercise();
    ex.start().unwrap();
    let ids = ex.send_inject(Some("I-01"), "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new()).unwrap();
    let p1 = ex.player_reply("mayor", "warn people", ids[0], ev()).unwrap();
    ex.attach_parse(p1, parsed("warn people")).unwrap();
    ex.request_clarification(p1, "Which side?").unwrap();
    let q = ex.deliver_clarification(p1, "Quale lato?", "Which side?").unwrap();
    let before = ex.proposals.len();

    let (mid, pid) = ex.answer_clarification("mayor", "lato nord", q, "north side", false).unwrap();
    assert_eq!(pid, p1);
    assert_eq!(ex.proposals.len(), before, "no new proposal, nothing superseded");
    let p = ex.proposal(p1).unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert_eq!(p.commands.len(), 1, "original commands intact");
    assert_eq!(p.superseded_by, None);
    let m = ex.messages.iter().find(|m| m.id == mid).unwrap();
    assert_eq!((m.kind, m.reply_to, m.proposal, m.inject_id), (MsgKind::Reply, Some(q), Some(p1), ex.proposal(p1).unwrap().inject_id));
    assert_eq!(m.text_en, "north side");
    assert!(ex.log.iter().any(|e| e.kind == "clarification_answered"));

    let t = ex.clarification_thread(p1).unwrap();
    assert_eq!((t.status, t.current_proposal), (ThreadStatus::AwaitingExcon, p1));
    let last = t.entries.last().unwrap();
    assert_eq!((last.message, last.text.as_str(), last.text_en.as_str()), (mid, "lato nord", "north side"));
    let row = row_of(&ex, "mayor");
    assert!(row["pending_proposals"].as_array().unwrap().iter().any(|x| x["id"] == p1 && x["status"] == "pending"));
    assert!(row["replies"].as_array().unwrap().iter().any(|x| x["id"] == mid));
    assert!(row["thread"].as_array().unwrap().iter().any(|x| x["id"] == mid && x["clarification_thread"] == p1));
    assert!(ex.conversation("mayor").iter().any(|c| c.content.contains("north side")), "AI history has the answer");

    // EXCON may ask again (thread continues on the same proposal), or approve.
    ex.request_clarification(p1, "And the road?").unwrap();
    let q2 = ex.deliver_clarification(p1, "E la strada?", "And the road?").unwrap();
    ex.answer_clarification("mayor", "chiusa", q2, "closed", false).unwrap();
    assert_eq!(ex.clarification_thread(p1).unwrap().entries.len(), 5);
    ex.decide(p1, Decision { approve: true, ..Default::default() }).unwrap();
}

#[test]
fn actions_or_drafts_cannot_answer_a_question_and_a_closed_question_takes_no_answer() {
    let mut ex = exercise();
    ex.start().unwrap();
    let p1 = ex.player_actions("mayor", "warn", warn(), None).unwrap();
    ex.request_clarification(p1, "Which zone?").unwrap();
    let q = ex.deliver_clarification(p1, "Quale zona?", "Which zone?").unwrap();
    let err = ex.player_actions("mayor", "warn F4", warn(), Some(q)).unwrap_err().to_string();
    assert!(err.contains("open EXCON question"), "{err}");
    assert!(ex.player_reply("mayor", "F4", q, ev()).is_err());
    assert!(ex.begin_player_draft("mayor", MASTER, "F4", None, Some(q)).is_err());
    assert!(ex.answer_clarification("fire_commander", "x", q, "x", false).is_err(), "someone else's question");
    ex.close_clarification(p1).unwrap();
    assert!(ex.answer_clarification("mayor", "late", q, "late", false).is_err());
    // A new action on its own is fine.
    assert!(ex.player_actions("mayor", "warn F4", warn(), None).is_ok());
}
