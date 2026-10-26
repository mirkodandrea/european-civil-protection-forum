//! EXCON-player clarification threads, headless: a view over proposals, messages
//! and the supersede chain, with one extra bit of state (closed).
use std::collections::BTreeMap;
use std::sync::Arc;

use exercise::threads::{ThreadAuthor, ThreadStatus};
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

/// Declaration -> question -> answer -> question -> answer, then decided.
#[test]
fn several_rounds_stay_in_one_thread_and_show_in_inject_responses() {
    let mut ex = exercise();
    ex.start().unwrap();
    let ids = ex.send_inject(Some("I-01"), "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new()).unwrap();
    let p1 = ex.player_reply("mayor", "warn people", ids[0], ev()).unwrap();
    ex.attach_parse(p1, parsed("warn people")).unwrap();
    assert!(ex.clarification_threads().is_empty(), "no question yet, no thread");

    ex.request_clarification(p1, "Which zone?").unwrap();
    // Asked but not delivered yet: the thread exists, nothing is out.
    assert_eq!(ex.clarification_threads().len(), 1);
    let q1 = ex.deliver_clarification(p1, "Quale zona?", "Which zone?").unwrap();
    let t = ex.clarification_thread(p1).unwrap();
    assert_eq!(t.status, ThreadStatus::AwaitingPlayer);
    assert_eq!(t.entries.len(), 2);
    assert_eq!((t.entries[0].author, t.entries[1].author), (ThreadAuthor::Player, ThreadAuthor::Excon));
    assert_eq!(t.entries[1].text, "Quale zona?");
    assert_eq!(t.entries[1].text_en, "Which zone?");

    let p2 = ex.player_reply("mayor", "F4", q1, ev()).unwrap();
    ex.attach_parse(p2, parsed("F4")).unwrap();
    let t = ex.clarification_thread_of(p2).unwrap();
    assert_eq!(t.id, p1, "the thread keeps the originating proposal as id");
    assert_eq!((t.status, t.current_proposal), (ThreadStatus::AwaitingExcon, p2));
    assert_eq!(t.entries.len(), 3);
    assert_eq!(t.entries[2].proposal, Some(p2));

    // Second round on the new proposal.
    ex.request_clarification(p2, "Which side of F4?").unwrap();
    let q2 = ex.deliver_clarification(p2, "Quale lato?", "Which side?").unwrap();
    assert_eq!(ex.clarification_thread(p1).unwrap().status, ThreadStatus::AwaitingPlayer);
    let p3 = ex.player_reply("mayor", "the north side", q2, ev()).unwrap();
    ex.attach_parse(p3, parsed("north side")).unwrap();
    let t = ex.clarification_thread(p1).unwrap();
    assert_eq!(t.proposals, vec![p1, p2, p3]);
    assert_eq!(t.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
    assert_eq!(t.entries.last().unwrap().text_en, "north side");

    // The same entries appear in the per-response thread of the EXCON view, tagged.
    let row = ex.inject_responses().into_iter().find(|r| r["role"] == "mayor").unwrap();
    let tagged: Vec<u64> = row["thread"].as_array().unwrap().iter()
        .filter(|m| m["clarification_thread"] == p1)
        .map(|m| m["id"].as_u64().unwrap())
        .collect();
    assert_eq!(tagged, t.entries.iter().map(|e| e.message).collect::<Vec<_>>());
    assert_eq!(row["clarification_threads"][0]["id"], p1);
    assert_eq!(row["clarification_threads"][0]["current_proposal"], p3);

    // The player's projection carries no English working copy.
    let mine = ex.player_clarification_threads("mayor");
    assert_eq!(mine.len(), 1);
    assert!(mine[0]["entries"][0].get("text_en").is_none());
    assert!(ex.player_clarification_threads("fire_commander").is_empty());

    // Deciding ends the conversation. Threads never change the world.
    ex.decide(p3, Decision { approve: false, ..Default::default() }).unwrap();
    assert_eq!(ex.clarification_thread(p1).unwrap().status, ThreadStatus::Closed);
    assert!(ex.debrief()["clarification_threads"].as_array().unwrap().len() == 1);
}

#[test]
fn closing_withdraws_an_unanswered_question_and_a_new_one_reopens() {
    let mut ex = exercise();
    ex.start().unwrap();
    let ids = ex.send_inject(None, "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new()).unwrap();
    let p = ex.player_reply("mayor", "warn", ids[0], ev()).unwrap();
    ex.attach_parse(p, parsed("warn")).unwrap();
    assert!(ex.close_clarification(p).is_err(), "no thread yet");
    ex.request_clarification(p, "Which zone?").unwrap();
    ex.deliver_clarification(p, "Quale zona?", "Which zone?").unwrap();
    ex.close_clarification(p).unwrap();
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::Pending, "EXCON can decide again");
    assert_eq!(ex.clarification_thread(p).unwrap().status, ThreadStatus::Closed);
    assert!(ex.close_clarification(p).is_err(), "already closed");
    assert!(ex.log.iter().any(|e| e.kind == "clarification_closed"));
    // The still-open question can no longer be answered into the thread.
    let q = ex.messages.iter().rev().find(|m| m.kind == MsgKind::Clarification).unwrap().id;
    let other = ex.player_reply("mayor", "late answer", q, ev()).unwrap();
    assert_eq!(ex.proposal(other).unwrap().supersedes, None);
    // Asking again reopens.
    ex.request_clarification(p, "One more thing").unwrap();
    ex.deliver_clarification(p, "Ancora una cosa", "One more thing").unwrap();
    assert_eq!(ex.clarification_thread(p).unwrap().status, ThreadStatus::AwaitingPlayer);
}

#[test]
fn clarification_without_inject_can_still_be_answered() {
    let mut ex = exercise();
    ex.start().unwrap();
    let pid = ex.player_actions("mayor", "warn F4", vec![sim::Command::IssueWarning { zone: "F4".into() }], None).unwrap();
    ex.request_clarification(pid, "Why F4?").unwrap();
    let q = ex.deliver_clarification(pid, "Perche F4?", "Why F4?").unwrap();
    assert_eq!(ex.messages.iter().find(|m| m.id == q).unwrap().inject_id, None);
    let next = ex.player_reply("mayor", "wind", q, ev()).unwrap();
    assert_eq!(ex.proposal(next).unwrap().supersedes, Some(pid));
    assert_eq!(ex.clarification_thread(pid).unwrap().entries.len(), 2, "structured actions leave no declaration message");
}
