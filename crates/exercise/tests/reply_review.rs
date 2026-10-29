//! Only the EXCON acts on the world (2026-10-29): players reply in free text,
//! the AI reads the reply thread and suggests catalogue actions, the EXCON
//! executes, edits or ignores them. The acceptance scenario of the redesign,
//! headless, with the model's readings scripted.
use exercise::reply_review::{ItemState, QuestionState, ReadAction, ReadQuestion, Reading};
use exercise::response::{PlayerResponse, ResponseKind};
use exercise::*;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

/// The synthetic valley with the operational model (engine deployments travel).
fn exercise() -> Exercise {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.enable_operational_model();
    let mut ex = Exercise::new(sim::World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex
}

fn inject(ex: &mut Exercise, to: &[&str], text: &str) -> Vec<u64> {
    ex.send_inject(
        None,
        "Il fronte scavalca il crinale",
        "Sala operativa",
        &to.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        text,
        &BTreeMap::new(),
    )
    .unwrap()
}

/// A free-text reply, as the player page sends it.
fn reply(ex: &mut Exercise, role: &str, parent: u64, text: &str) -> u64 {
    ex.player_response(
        role,
        PlayerResponse {
            narrative: Some(text.into()),
            narrative_en: text.into(),
            reply_to: Some(parent),
            kind: ResponseKind::Communication,
            ..Default::default()
        },
    )
    .unwrap()
}

fn action(role: &str, kind: &str, params: serde_json::Value, quote: &str) -> ReadAction {
    ReadAction {
        role: role.into(),
        kind: kind.into(),
        params,
        quote: quote.into(),
        ..Default::default()
    }
}

fn items(ex: &Exercise, pid: u64) -> Vec<exercise::reply_review::ReviewItem> {
    ex.review
        .items
        .iter()
        .filter(|i| i.proposal == pid)
        .cloned()
        .collect()
}

fn clarifications(ex: &Exercise) -> usize {
    ex.messages
        .iter()
        .filter(|m| m.kind == MsgKind::Clarification)
        .count()
}

const INJECT: &str =
    "Ore 14:20, il fronte ha scavalcato il crinale in E3 e scende verso F4, vento W 25 km/h";
const DOS: &str = "Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco";
const DOS_ANSWER: &str = "Lungo la strada da E3 a F3, con il bulldozer del Comune";

/// The DOS step of the acceptance scenario, up to the question draft.
fn dos_reading(ex: &mut Exercise) -> (u64, u64) {
    let parent = inject(ex, &["fire_commander", "mayor"], INJECT)[0];
    let pid = reply(ex, "fire_commander", parent, DOS);
    assert!(ex.reviewable(pid));
    let rev = ex.begin_review(pid).unwrap();
    let reading = Reading {
        actions: vec![
            action(
                "fire_commander",
                "DeployEngine",
                json!({"at": "F4:N"}),
                "Mandiamo due squadre AIB sul lato nord di F4",
            ),
            action(
                "fire_commander",
                "DeployEngine",
                json!({"at": "F4:N"}),
                "Mandiamo due squadre AIB sul lato nord di F4",
            ),
            ReadAction {
                notes: vec!["Endpoints and means not stated.".into()],
                ..action(
                    "fire_commander",
                    "CutFireLine",
                    json!({}),
                    "apriamo una linea tagliafuoco",
                )
            },
        ],
        questions: vec![ReadQuestion {
            to: "fire_commander".into(),
            text: "Per la linea tagliafuoco: tra quali punti e con quali mezzi?".into(),
            about: vec![2],
            reason: String::new(),
        }],
        summary: "Two AIB teams to the north side of F4; a fire line without endpoints.".into(),
        dropped: 0,
    };
    ex.ingest_reading(pid, rev, reading).unwrap();
    (parent, pid)
}

#[test]
fn a_player_reply_needs_no_approval_and_becomes_excon_suggestions() {
    let mut ex = exercise();
    let (_, pid) = dos_reading(&mut ex);
    let p = ex.proposal(pid).unwrap();
    assert_eq!(
        p.status,
        ProposalStatus::Informational,
        "the player's reply is never an approval request"
    );
    assert!(p.commands.is_empty());
    let all = items(&ex, pid);
    assert_eq!(
        all.len(),
        3,
        "incomplete suggestions are kept, never dropped"
    );
    assert_eq!(
        all.iter()
            .filter(|i| i.state == ItemState::Complete)
            .count(),
        2
    );
    let line = all.iter().find(|i| i.kind == "CutFireLine").unwrap();
    assert_eq!(line.state, ItemState::Incomplete);
    assert_eq!(
        line.missing,
        vec!["from", "to"],
        "the missing fields are named, never guessed"
    );
    assert_eq!(line.source_role, "fire_commander");
    assert_eq!(line.quote, "apriamo una linea tagliafuoco");
    // One reviewed draft, never sent by itself.
    let drafts: Vec<_> = ex
        .review
        .questions
        .iter()
        .filter(|q| q.state == QuestionState::Draft)
        .collect();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].about, vec![line.id]);
    assert_eq!(
        clarifications(&ex),
        0,
        "AI drafts never reach a player without EXCON review"
    );
    // Nothing touched the world yet.
    assert!(ex.world.d.tasks.is_empty());
}

#[test]
fn the_acceptance_scenario_runs_end_to_end() {
    let mut ex = exercise();
    let (parent, pid) = dos_reading(&mut ex);
    // Complete suggestions run at once, without waiting for the clarification.
    let teams: Vec<u64> = items(&ex, pid)
        .iter()
        .filter(|i| i.kind == "DeployEngine")
        .map(|i| i.id)
        .collect();
    // The feasibility check is shown with the suggestion: the north edge of F4
    // is beyond hose reach from a road, so the EXCON edits the place.
    let first = items(&ex, pid)
        .into_iter()
        .find(|i| i.id == teams[0])
        .unwrap();
    assert!(
        matches!(&first.verdict, Some(sim::Verdict::Rejected { reason }) if reason.contains("hose reach")),
        "{:?}",
        first.verdict
    );
    for id in &teams {
        let run = ex
            .execute_review_item(*id, Some(json!({"at": "F4"})), None)
            .unwrap();
        assert!(run.ok, "{}", run.detail);
    }
    assert_eq!(ex.world.d.tasks.len(), 2, "two approved deployments travel");
    let line = items(&ex, pid)
        .into_iter()
        .find(|i| i.kind == "CutFireLine")
        .unwrap();
    assert!(
        ex.execute_review_item(line.id, None, None).is_err(),
        "an incomplete suggestion cannot run"
    );

    // EXCON reviews and sends the question.
    let q = ex
        .review
        .questions
        .iter()
        .find(|q| q.state == QuestionState::Draft)
        .unwrap()
        .id;
    let (to, text, persona) = ex.review_question_to_send(q, None, None).unwrap();
    assert_eq!(to, "fire_commander");
    assert_eq!(
        text, "Per la linea tagliafuoco: tra quali punti e con quali mezzi?",
        "sent as reviewed"
    );
    let asked = ex
        .send_review_question(
            q,
            &text,
            "For the fire line: between which points and with which means?",
            persona,
        )
        .unwrap();
    assert_eq!(ex.messages.last().unwrap().text, text);
    assert_eq!(clarifications(&ex), 1);
    let threads = ex.player_review_threads("fire_commander");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["status"], "awaiting_player");
    assert_eq!(threads[0]["entries"][0]["message"], asked);

    // The DOS answers: the answer is linked to the question.
    let (answer, answered) = ex
        .answer_review_question("fire_commander", DOS_ANSWER, asked, DOS_ANSWER, false)
        .unwrap();
    assert_eq!(answered, pid);
    let question = ex.review.questions.iter().find(|x| x.id == q).unwrap();
    assert_eq!(
        (question.state, question.answer),
        (QuestionState::Answered, Some(answer))
    );

    // The AI re-reads the whole thread.
    let rev = ex.begin_review(pid).unwrap();
    let context = ex.review_context(pid).unwrap();
    let kinds: Vec<_> = context
        .thread
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(kinds, vec!["inject", "reply", "question", "answer"]);
    assert!(context.player_words.iter().any(|w| w == DOS_ANSWER));
    let reading = Reading {
        actions: vec![
            ReadAction {
                ref_id: Some(teams[0]),
                ..action(
                    "fire_commander",
                    "DeployEngine",
                    json!({"at": "F4:N"}),
                    "Mandiamo due squadre AIB sul lato nord di F4",
                )
            },
            ReadAction {
                ref_id: Some(teams[1]),
                ..action(
                    "fire_commander",
                    "DeployEngine",
                    json!({"at": "F4:N"}),
                    "Mandiamo due squadre AIB sul lato nord di F4",
                )
            },
            ReadAction {
                ref_id: Some(line.id),
                notes: vec![
                    "The bulldozer is the Municipality's: has the Mayor authorised it?".into(),
                ],
                ..action(
                    "fire_commander",
                    "CutFireLine",
                    json!({"from": "E3", "to": "F3"}),
                    "Lungo la strada da E3 a F3",
                )
            },
        ],
        questions: vec![ReadQuestion {
            to: "mayor".into(),
            text: "Il DOS chiede il bulldozer del Comune per la linea E3–F3: lo autorizzate?"
                .into(),
            about: vec![2],
            reason: "Cross-role: the DOS uses the Municipality's bulldozer.".into(),
        }],
        summary: String::new(),
        dropped: 0,
    };
    let diff = ex.ingest_reading(pid, rev, reading).unwrap();
    assert_eq!(
        diff.completed,
        vec![line.id],
        "the diff names the completed suggestion"
    );
    assert!(diff.changed.is_empty() && diff.new.is_empty());
    let after = items(&ex, pid);
    for id in &teams {
        assert_eq!(
            after.iter().find(|i| i.id == *id).unwrap().state,
            ItemState::Executed,
            "decisions already taken stay"
        );
    }
    let line = after.iter().find(|i| i.id == line.id).unwrap();
    assert_eq!(line.state, ItemState::Complete);
    assert!(line.missing.is_empty());
    assert!(
        line.quote.contains("apriamo una linea tagliafuoco") && line.quote.contains("da E3 a F3")
    );
    assert!(line.notes[0].contains("Mayor"));
    // The cross-role question is only a proposal: EXCON may send or discard it.
    let cross = ex
        .review
        .questions
        .iter()
        .find(|q| q.state == QuestionState::Draft)
        .unwrap();
    assert!(cross.cross_role);
    assert_eq!(cross.to, "mayor");
    assert_eq!(clarifications(&ex), 1, "still nothing sent without EXCON");
    let cross = cross.id;
    ex.discard_review_question(cross).unwrap();
    assert!(ex.execute_review_item(line.id, None, None).unwrap().ok);

    // Mayor: evacuation and shelter, both complete.
    let mayor_inject = ex
        .messages
        .iter()
        .find(|m| m.kind == MsgKind::Inject && m.to == "mayor")
        .unwrap()
        .id;
    assert_ne!(mayor_inject, parent);
    let mp = reply(
        &mut ex,
        "mayor",
        mayor_inject,
        "Evacuiamo F4, apriamo la palestra e chiudo la provinciale a F4",
    );
    let rev = ex.begin_review(mp).unwrap();
    ex.ingest_reading(
        mp,
        rev,
        Reading {
            actions: vec![
                action(
                    "mayor",
                    "EvacuateZone",
                    json!({"zone": "F4"}),
                    "Evacuiamo F4",
                ),
                action(
                    "mayor",
                    "OpenShelter",
                    json!({"building": "Valverde Sports Centre"}),
                    "apriamo la palestra",
                ),
                action(
                    "mayor",
                    "CloseRoad",
                    json!({"road": "sp1"}),
                    "chiudo la provinciale a F4",
                ),
            ],
            ..Default::default()
        },
    )
    .unwrap();
    let mayor = items(&ex, mp);
    assert!(
        mayor.iter().all(|i| i.state == ItemState::Complete),
        "{mayor:?}"
    );
    assert!(
        ex.review.questions.iter().all(|q| q.proposal != mp),
        "nothing to ask: no question drafted"
    );
    for i in mayor {
        if i.kind == "EvacuateZone" {
            // Two buses for 120 people: shown before, refused if executed, left open.
            assert!(matches!(i.verdict, Some(sim::Verdict::Rejected { .. })));
            let refused = ex
                .execute_review_item(i.id, None, None)
                .unwrap_err()
                .to_string();
            assert!(refused.contains("buses"), "{refused}");
            ex.ignore_review_item(i.id, Some("not enough buses".into()))
                .unwrap();
            continue;
        }
        let run = ex.execute_review_item(i.id, None, None).unwrap();
        assert!(run.ok, "{}: {}", i.description, run.detail);
    }
    assert!(
        ex.log
            .iter()
            .filter(|e| e.kind == "action_executed" && e.data["review_item"].is_number())
            .count()
            == 5
    );
}

#[test]
fn a_gap_the_model_does_not_ask_about_gets_a_template_question() {
    let mut ex = exercise();
    let parent = inject(&mut ex, &["fire_commander"], INJECT)[0];
    let pid = reply(&mut ex, "fire_commander", parent, DOS);
    let rev = ex.begin_review(pid).unwrap();
    ex.ingest_reading(
        pid,
        rev,
        Reading {
            actions: vec![action(
                "fire_commander",
                "CutFireLine",
                json!({}),
                "apriamo una linea tagliafuoco",
            )],
            ..Default::default()
        },
    )
    .unwrap();
    let q: Vec<_> = ex.review.questions.iter().collect();
    assert_eq!(q.len(), 1);
    assert!(q[0].template && q[0].state == QuestionState::Draft);
    // In the recipient's language (the DOS plays in English here).
    assert!(q[0].text.contains("fire line"), "{}", q[0].text);
    ex.set_language("fire_commander", "it").unwrap();
    let rev = ex.begin_review(pid).unwrap();
    ex.ingest_reading(pid, rev, Reading::default()).unwrap();
    let draft = ex
        .review
        .questions
        .iter()
        .find(|q| q.state == QuestionState::Draft)
        .unwrap();
    assert!(
        draft.text.contains("linea tagliafuoco") && draft.text.contains("tra quali punti"),
        "{}",
        draft.text
    );
    assert_eq!(clarifications(&ex), 0);
}

#[test]
fn excon_edits_and_ignores_survive_a_new_reading() {
    let mut ex = exercise();
    let (_, pid) = dos_reading(&mut ex);
    let all = items(&ex, pid);
    let (a, b) = (all[0].id, all[1].id);
    let line = all[2].id;
    ex.ignore_review_item(b, Some("one team is enough".into()))
        .unwrap();
    ex.edit_review_item(line, None, json!({"from": "E3"}))
        .unwrap();
    let rev = ex.begin_review(pid).unwrap();
    let diff = ex
        .ingest_reading(
            pid,
            rev,
            Reading {
                actions: vec![
                    ReadAction {
                        ref_id: Some(a),
                        ..action(
                            "fire_commander",
                            "DeployEngine",
                            json!({"at": "F4:N"}),
                            "due squadre AIB",
                        )
                    },
                    ReadAction {
                        ref_id: Some(b),
                        ..action(
                            "fire_commander",
                            "DeployEngine",
                            json!({"at": "F4:N"}),
                            "due squadre AIB",
                        )
                    },
                    ReadAction {
                        ref_id: Some(line),
                        ..action(
                            "fire_commander",
                            "CutFireLine",
                            json!({"from": "E2", "to": "F3"}),
                            "da E3 a F3",
                        )
                    },
                    action(
                        "fire_commander",
                        "WaterDrop",
                        json!({"at": "F4"}),
                        "lancio su F4",
                    ),
                ],
                ..Default::default()
            },
        )
        .unwrap();
    let after = items(&ex, pid);
    assert_eq!(
        after.iter().find(|i| i.id == b).unwrap().state,
        ItemState::Ignored,
        "an ignored, unchanged suggestion stays ignored"
    );
    let edited = after.iter().find(|i| i.id == line).unwrap();
    assert_eq!(edited.params["from"], "E3", "the EXCON's value is kept");
    assert_eq!(
        edited.params["to"], "F3",
        "only the missing value comes from the AI"
    );
    assert_eq!(diff.completed, vec![line]);
    assert_eq!(diff.new.len(), 1);
}

#[test]
fn an_old_reading_is_discarded_and_rollback_forgets_the_board() {
    let mut ex = exercise();
    let parent = inject(&mut ex, &["fire_commander"], INJECT)[0];
    let pid = reply(&mut ex, "fire_commander", parent, DOS);
    let old = ex.begin_review(pid).unwrap();
    let new = ex.begin_review(pid).unwrap();
    assert!(ex.ingest_reading(pid, old, Reading::default()).is_err());
    ex.ingest_reading(
        pid,
        new,
        Reading {
            actions: vec![action(
                "fire_commander",
                "DeployEngine",
                json!({"at": "F4"}),
                "due squadre",
            )],
            ..Default::default()
        },
    )
    .unwrap();
    let id = ex.review.items[0].id;
    assert!(ex.execute_review_item(id, None, None).unwrap().ok);
    ex.resolve_turn(Some(5)).unwrap();
    ex.rollback(1).unwrap();
    assert!(ex.review.items.is_empty() && ex.review.reviews.is_empty());
    assert!(ex.world.d.tasks.is_empty());
}
