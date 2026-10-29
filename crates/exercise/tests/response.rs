//! Merged responses (narrative + actions) and conversations linked to an inject.
use exercise::response::PlayerResponse;
use exercise::*;
use serde_json::json;
use sim::Command;
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
            "Smoke over the village",
            "112",
            &["mayor".into()],
            "Residents report smoke from the north.",
            &BTreeMap::new(),
        )
        .unwrap();
    (ex, ids[0])
}
fn warn() -> Command {
    Command::IssueWarning { zone: "F4".into() }
}
fn narrative(text: &str, parent: u64) -> PlayerResponse {
    PlayerResponse {
        narrative: Some(text.into()),
        narrative_en: text.into(),
        reply_to: Some(parent),
        basis: Some(BasisKind::OperationalJudgment),
        ..Default::default()
    }
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
fn a_response_needs_a_narrative_or_an_action() {
    let (mut ex, parent) = fixture();
    let none = PlayerResponse {
        reply_to: Some(parent),
        basis: Some(BasisKind::OperationalJudgment),
        ..Default::default()
    };
    assert!(ex.player_response("mayor", none).is_err());
    let blank = PlayerResponse {
        narrative: Some("  ".into()),
        narrative_en: "x".into(),
        reply_to: Some(parent),
        basis: Some(BasisKind::OperationalJudgment),
        ..Default::default()
    };
    assert!(ex.player_response("mayor", blank).is_err());
    assert!(ex.proposals.is_empty());
}

#[test]
fn narrative_only_stays_a_pure_narrative_initiative() {
    let (mut ex, parent) = fixture();
    let p = ex
        .player_response("mayor", narrative("Alert the volunteers", parent))
        .unwrap();
    let prop = ex.proposal(p).unwrap();
    assert!(prop.narrative && !prop.carries_actions && prop.pure_narrative());
    assert!(prop.commands.is_empty());
    assert_eq!(prop.status, ProposalStatus::Pending);
    approve(&mut ex, p);
    assert!(ex.execute_approved(p).is_err());
    ex.record_narrative_outcome(p, "ok", "ok", "ok", false)
        .unwrap();
}

#[test]
fn actions_only_is_the_operational_proposal_it_always_was() {
    let (mut ex, parent) = fixture();
    let p = ex
        .player_response(
            "mayor",
            PlayerResponse {
                commands: vec![warn()],
                reply_to: Some(parent),
                basis: Some(BasisKind::OperationalJudgment),
                ..Default::default()
            },
        )
        .unwrap();
    let prop = ex.proposal(p).unwrap();
    assert!(!prop.narrative && prop.commands.len() == 1);
    assert_eq!(prop.status, ProposalStatus::Pending);
    assert!(
        !prop.original.is_empty(),
        "text defaults to the action description"
    );
    // Narrative outcomes are only for narrative statements.
    approve(&mut ex, p);
    assert!(ex
        .record_narrative_outcome(p, "x", "x", "x", false)
        .is_err());
}

#[test]
fn narrative_and_actions_travel_in_one_response_and_both_are_kept() {
    let (mut ex, parent) = fixture();
    let p = ex
        .player_response(
            "mayor",
            PlayerResponse {
                commands: vec![warn()],
                ..narrative("Warn the village and call the volunteers", parent)
            },
        )
        .unwrap();
    assert_eq!(ex.proposals.len(), 1);
    let prop = ex.proposal(p).unwrap();
    assert!(prop.narrative && prop.carries_actions && !prop.pure_narrative());
    assert_eq!(prop.commands.len(), 1);
    assert_eq!(prop.original, "Warn the village and call the volunteers");
    assert_eq!(prop.status, ProposalStatus::Pending);
    // The commands need approval and execute; the narrative waits for an outcome.
    assert!(ex.execute_approved(p).is_err());
    approve(&mut ex, p);
    assert!(ex.proposal(p).unwrap().decision.as_ref().unwrap().approve);
    ex.execute_approved(p).unwrap();
    let before = ex.world.view();
    ex.record_narrative_outcome(
        p,
        "Volunteers ready",
        "Volunteers ready",
        "Volontari pronti",
        false,
    )
    .unwrap();
    assert_eq!(
        ex.world.view(),
        before,
        "a narrative outcome never changes physics"
    );
    let prop = ex.proposal(p).unwrap();
    assert_eq!(prop.narrative_outcomes.len(), 1);
    assert!(!prop.executions.is_empty());
}

#[test]
fn authority_and_inject_ownership_still_apply() {
    let (mut ex, parent) = fixture();
    // Not the recipient, and no conversation.
    assert!(ex
        .player_response("regional_cp", narrative("Let me help", parent))
        .is_err());
    // Action outside the role's authority.
    let r = PlayerResponse {
        commands: vec![Command::DeployEngine {
            at: sim::Loc::Named("F4".into()),
        }],
        reply_to: Some(parent),
        ..Default::default()
    };
    assert!(ex.player_response("mayor", r).is_err());
    // A narrative cannot be independent.
    assert!(ex
        .player_response(
            "mayor",
            PlayerResponse {
                narrative: Some("x".into()),
                narrative_en: "x".into(),
                basis: Some(BasisKind::OperationalJudgment),
                ..Default::default()
            }
        )
        .is_err());
}

fn conversation(ex: &mut Exercise, parent: u64) -> u64 {
    let (cid, _) = ex
        .start_linked_conversation(
            "mayor",
            parent,
            &["regional_cp".into()],
            "Serve un parere sul fumo",
            "We need your view on the smoke",
            &BTreeMap::from([(
                "regional_cp".into(),
                "We need your view on the smoke".into(),
            )]),
            false,
        )
        .unwrap();
    cid
}

#[test]
fn a_participant_can_answer_the_inject_from_a_linked_conversation_only() {
    let (mut ex, parent) = fixture();
    let cid = conversation(&mut ex, parent);
    // The conversation id alone is not enough for an outsider ...
    let outsider = PlayerResponse {
        conversation: Some(cid),
        ..narrative("Me too", parent)
    };
    assert!(ex.player_response("fire_commander", outsider).is_err());
    // ... a participant still must name the conversation ...
    assert!(ex
        .player_response("regional_cp", narrative("From regional level", parent))
        .is_err());
    // ... and it must be linked to the very inject being answered.
    let other = ex
        .send_inject(
            None,
            "Other",
            "112",
            &["mayor".into()],
            "Other",
            &BTreeMap::new(),
        )
        .unwrap()[0];
    let wrong = PlayerResponse {
        conversation: Some(cid),
        ..narrative("Wrong inject", other)
    };
    assert!(ex.player_response("regional_cp", wrong).is_err());
    // Allowed: provenance is recorded.
    let p = ex
        .player_response(
            "regional_cp",
            PlayerResponse {
                commands: vec![warn()],
                conversation: Some(cid),
                ..narrative("Regional warning issued", parent)
            },
        )
        .unwrap();
    let prop = ex.proposal(p).unwrap();
    assert_eq!(prop.source.label(), "regional_cp");
    assert_eq!(prop.via_conversation, Some(cid));
    assert_eq!(prop.reply_to, Some(parent));
    assert_eq!(
        prop.inject_id,
        ex.messages
            .iter()
            .find(|m| m.id == parent)
            .unwrap()
            .inject_id
    );
    // EXCON sees it under the inject with the responding role and the conversation.
    let row = ex
        .inject_responses()
        .into_iter()
        .find(|r| r["inject_message_id"] == parent)
        .unwrap();
    assert_eq!(row["pending_proposals"][0]["role"], "regional_cp");
    assert_eq!(row["pending_proposals"][0]["conversation"], cid);
    assert_eq!(row["replies"][0]["from"], "regional_cp");
    assert_eq!(row["conversations"][0]["id"], cid);
    // It can be approved and executed like any response.
    approve(&mut ex, p);
    ex.execute_approved(p).unwrap();
}

#[test]
fn only_the_recipient_starts_a_conversation_and_outsiders_cannot_read_it() {
    let (mut ex, parent) = fixture();
    let start = |ex: &mut Exercise, who: &str, others: &[&str]| {
        ex.start_linked_conversation(
            who,
            parent,
            &others.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "hello",
            "hello",
            &BTreeMap::new(),
            false,
        )
    };
    assert!(
        start(&mut ex, "fire_commander", &["mayor"]).is_err(),
        "not the recipient"
    );
    assert!(start(&mut ex, "mayor", &[]).is_err(), "needs a participant");
    assert!(start(&mut ex, "mayor", &["mayor"]).is_err());
    assert!(start(&mut ex, "mayor", &["ghost"]).is_err());
    assert!(start(&mut ex, "mayor", &["master"]).is_err());
    assert!(start(&mut ex, "mayor", &["regional_cp", "regional_cp"]).is_err());
    let cid = conversation(&mut ex, parent);
    // Quoted inject: the guest never received it but reads it through the conversation.
    let guest = ex.player_linked_conversations("regional_cp");
    assert_eq!(guest.len(), 1);
    assert_eq!(guest[0]["quote"]["title"], "Smoke over the village");
    assert_eq!(
        guest[0]["quote"]["text"],
        "Residents report smoke from the north."
    );
    assert!(ex
        .inbox("regional_cp")
        .iter()
        .all(|m| m.kind != MsgKind::Inject));
    // Outsiders see nothing, and cannot post.
    assert!(ex.player_linked_conversations("fire_commander").is_empty());
    assert!(ex
        .post_conversation_message(
            "fire_commander",
            cid,
            "hi",
            None,
            "hi",
            &BTreeMap::new(),
            false
        )
        .is_err());
    // Messages cite the inject or a previous message, nothing else.
    let first = ex.linked_conversation(cid).unwrap().messages[0].id;
    for target in [first, parent] {
        ex.post_conversation_message(
            "regional_cp",
            cid,
            "ok",
            Some(target),
            "ok",
            &BTreeMap::new(),
            false,
        )
        .unwrap();
    }
    assert!(ex
        .post_conversation_message(
            "regional_cp",
            cid,
            "ok",
            Some(999_999),
            "ok",
            &BTreeMap::new(),
            false
        )
        .is_err());
    // Each participant reads the text in their own language; EXCON reads the transcript too.
    let master = ex.master_linked_conversations();
    assert_eq!(master[0]["message_count"], 3);
    assert_eq!(master[0]["messages"].as_array().unwrap().len(), 3);
    assert!(master[0].to_string().contains("smoke"), "EXCON reads the text");
    assert!(ex
        .log
        .iter()
        .any(|e| e.kind == "conversation_message" && e.data.to_string().contains("view")));
    assert!(ex.debrief()["linked_conversations"].to_string().contains("smoke"));
    assert_eq!(
        json!(ex.player_linked_conversations("mayor")[0]["messages"][0]["original"]),
        "Serve un parere sul fumo"
    );
}

#[test]
fn the_quote_falls_back_to_the_english_copy_when_languages_differ() {
    let (mut ex, parent) = fixture();
    ex.set_language("mayor", "it").unwrap();
    ex.set_language("regional_cp", "fr").unwrap();
    conversation(&mut ex, parent);
    let guest = ex.player_linked_conversations("regional_cp");
    let m = ex.messages.iter().find(|m| m.id == parent).unwrap();
    assert_eq!(guest[0]["quote"]["text"], m.text_en);
}

#[test]
fn excon_can_add_suggested_actions_to_a_narrative_and_take_them_back() {
    let (mut ex, parent) = fixture();
    let p = ex
        .player_response("mayor", narrative("Alert the village", parent))
        .unwrap();
    ex.revalidate_proposal(p, Some(vec![warn()])).unwrap();
    let prop = ex.proposal(p).unwrap();
    assert!(prop.carries_actions && prop.excon_actions && !prop.pure_narrative());
    assert!(ex.log.iter().any(|l| l.kind == "narrative_actions_added"));
    ex.revalidate_proposal(p, Some(vec![])).unwrap();
    let prop = ex.proposal(p).unwrap();
    assert!(prop.pure_narrative() && !prop.excon_actions, "removing them restores the initiative");
}

#[test]
fn suggestions_keep_only_new_allowed_commands_and_scope_the_context() {
    let (ex, parent) = fixture();
    let raw = vec![
        json!({"type":"IssueWarning","zone":"F4"}),
        json!({"type":"IssueWarning","zone":"F4"}),
        json!({"type":"SetWeather","wind_speed_kmh":60}),
        json!({"type":"Nope"}),
        json!({"type":"IssueWarning","zone":"G5"}),
    ];
    let check = ex.check_suggestions("mayor", &raw, &["a".into(), "b".into()], &[Command::IssueWarning { zone: "G5".into() }]);
    assert_eq!(check.actions.len(), 1, "{check:?}");
    assert_eq!(check.actions[0].reason, "a");
    assert_eq!(check.dropped, 4, "repeat, EXCON-only, unknown and already staged");
    // The player context never carries another role's inject; EXCON's does.
    let mut ex = ex;
    let other = ex
        .send_inject(None, "Secret", "112", &["fire_commander".into()], "Hidden tanker route", &BTreeMap::new())
        .unwrap()[0];
    let mine = ex.player_suggestion_context("mayor", "We alert", Some(parent), None, &[]).unwrap();
    let all = format!("{}{}", mine.digest, mine.conversations);
    assert!(all.contains("Smoke over the village") && !all.contains("Hidden tanker route"));
    assert!(ex.player_suggestion_context("mayor", "x", Some(other), None, &[]).is_err());
    assert!(ex.player_suggestion_context("mayor", " ", None, None, &[]).is_err());
    let p = ex.player_response("mayor", narrative("We alert", parent)).unwrap();
    let full = ex.excon_suggestion_context(p).unwrap();
    assert!(full.digest.to_string().contains("training_design"));
    assert_eq!(full.role, "mayor");
}
