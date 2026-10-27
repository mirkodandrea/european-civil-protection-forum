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

/// An action sent without a parent cannot be linked to the open question: it must
/// answer the question message (what the revise button of the player UI sends).
#[test]
fn structured_answer_to_an_inject_less_question_supersedes_it_only_with_the_question_as_parent() {
    let mut ex = exercise();
    ex.start().unwrap();
    let warn = || vec![sim::Command::IssueWarning { zone: "F4".into() }];
    let p1 = ex.player_actions("mayor", "warn", warn(), None).unwrap();
    ex.request_clarification(p1, "Which zone?").unwrap();
    let q = ex.deliver_clarification(p1, "Quale zona?", "Which zone?").unwrap();
    let linked = ex.player_actions("mayor", "warn F4", warn(), Some(q)).unwrap();
    assert_eq!(ex.proposal(linked).unwrap().supersedes, Some(p1));
    assert_eq!(ex.proposal(p1).unwrap().status, ProposalStatus::Superseded);
    assert_eq!(ex.clarification_thread(p1).unwrap().current_proposal, linked);
}

#[test]
fn answer_with_requested_action_reaches_excon_on_every_route() {
    for route in ["reply", "actions", "draft"] {
        let mut ex = exercise();
        ex.start().unwrap();
        let ids = ex
            .send_inject(Some("I-01"), "Fire", "112", &["mayor".into()], "Fire", &BTreeMap::new())
            .unwrap();
        let p1 = ex.player_reply("mayor", "we must protect people", ids[0], ev()).unwrap();
        ex.attach_parse(
            p1,
            ParsedInput {
                language: "it".into(),
                translation_en: "we must protect people".into(),
                clarification: Some("Which action, where?".into()),
                summary: "vague".into(),
                ..Default::default()
            },
        )
        .unwrap();
        ex.request_clarification(p1, "Which action, and where?").unwrap();
        let q = ex.deliver_clarification(p1, "Quale azione?", "Which action, and where?").unwrap();

        let p2 = match route {
            "reply" => {
                let p = ex.player_reply("mayor", "warn zone F4", q, ev()).unwrap();
                ex.attach_parse(p, parsed("warn zone F4")).unwrap();
                p
            }
            "actions" => ex
                .player_actions(
                    "mayor",
                    "warn F4",
                    vec![sim::Command::IssueWarning { zone: "F4".into() }],
                    Some(q),
                )
                .unwrap(),
            _ => {
                let d = ex.begin_player_draft("mayor", MASTER, "warn zone F4", None, Some(q)).unwrap();
                ex.finish_player_draft("mayor", d.id, d.revision, parsed("warn zone F4"), "warn zone F4".into())
                    .unwrap();
                ex.submit_player_draft("mayor", d.id, d.revision, "warn zone F4", "avvisa F4", false, false)
                    .unwrap()
                    .1
                    .unwrap()
            }
        };
        let p = ex.proposal(p2).unwrap();
        assert_eq!(p.status, ProposalStatus::Pending, "{route}");
        assert_eq!(p.commands.len(), 1, "{route}");
        assert_eq!(p.supersedes, Some(p1), "{route}");
        assert_eq!(p.inject_id, ex.proposal(p1).unwrap().inject_id, "{route}: inject link kept");
        let row = row_of(&ex, "mayor");
        let pending = row["pending_proposals"].as_array().unwrap();
        assert!(pending.iter().any(|x| x["id"] == p2), "{route}: answer visible to EXCON in the inject row");
        assert!(
            row["replies"].as_array().unwrap().iter().any(|m| m["proposal"] == p2),
            "{route}: reply in thread"
        );
        let t = ex.clarification_thread(p1).unwrap();
        assert_eq!((t.status, t.current_proposal), (ThreadStatus::AwaitingExcon, p2), "{route}");
        ex.decide(p2, Decision { approve: true, ..Default::default() }).unwrap();
    }
}
