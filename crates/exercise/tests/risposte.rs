//! The "Risposte" flow of the 28/10 playtest: every response is stored under
//! its inject, communications need no approval, decisions need their basis and
//! EXCON review, approval and publication are distinct events, and a
//! conversation started from an inject can lead to a recorded decision.
use exercise::response::{PlayerResponse, ResponseKind};
use exercise::*;
use std::{collections::BTreeMap, sync::Arc};

fn exercise() -> Exercise {
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
    ex
}

fn inject(ex: &mut Exercise, sender: &str, to: &[&str], title: &str) -> Vec<u64> {
    ex.send_inject(
        None,
        title,
        sender,
        &to.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        title,
        &BTreeMap::new(),
    )
    .unwrap()
}

fn text(t: &str, parent: u64) -> PlayerResponse {
    PlayerResponse {
        narrative: Some(t.into()),
        narrative_en: t.into(),
        reply_to: Some(parent),
        // A reply to the sender states its basis and why.
        basis: Some(BasisKind::OperationalJudgment),
        rationale: Some("Rationale stated by the player".into()),
        ..Default::default()
    }
}

fn row(ex: &Exercise, delivery: u64) -> serde_json::Value {
    ex.inject_responses()
        .into_iter()
        .find(|r| r["inject_message_id"] == delivery)
        .unwrap()
}

/// Scenario A: fire bulletin from the weather service; the municipality
/// decides, in words only, to publish a warning to the population.
#[test]
fn a_narrated_decision_is_stored_reviewed_and_published_as_distinct_events() {
    let mut ex = exercise();
    let bulletin = inject(&mut ex, "Servizio meteo", &["mayor"], "Bollettino incendi: pericolosità alta")[0];
    let warning = "Pubblichiamo un avviso alla popolazione: niente fuochi, evitare i boschi";
    // A decision without its basis is refused, with nothing stored.
    let no_basis = PlayerResponse { kind: ResponseKind::Decision, basis: None, ..text(warning, bulletin) };
    assert!(ex.player_response("mayor", no_basis).is_err());
    assert!(ex.proposals.is_empty());
    let p = ex
        .player_response(
            "mayor",
            PlayerResponse {
                kind: ResponseKind::Decision,
                basis: Some(BasisKind::OperationalJudgment),
                rationale: Some("Rationale stated by the player".into()),
                ..text(warning, bulletin)
            },
        )
        .unwrap();
    // Stored and addressed to the fictitious sender of the inject.
    let prop = ex.proposal(p).unwrap();
    assert_eq!(prop.status, ProposalStatus::Pending, "a decision waits for EXCON");
    assert!(!prop.communication && prop.pure_narrative());
    assert_eq!(prop.addressed_to.persona.as_deref(), Some("Servizio meteo"));
    assert_eq!(prop.evidence.as_ref().unwrap().basis.kind, BasisKind::OperationalJudgment);
    assert!(ex.log.iter().any(|e| e.kind == "response_recorded" && e.data["proposal"] == p));
    // EXCON sees it under the bulletin, pending, with the text.
    let r = row(&ex, bulletin);
    assert_eq!(r["pending_proposals"][0]["id"], p);
    assert_eq!(r["replies"][0]["text"], warning);
    assert_eq!(r["replies"][0]["response"]["addressed_to"]["persona"], "Servizio meteo");
    // Approval and publication are two events.
    ex.decide(p, Decision { approve: true, ..Default::default() }).unwrap();
    assert_eq!(ex.proposal(p).unwrap().status, ProposalStatus::Approved);
    assert!(ex.proposal(p).unwrap().narrative_outcomes.is_empty(), "approved, not yet published");
    ex.record_narrative_outcome(p, "Avviso pubblicato sul sito del Comune", "Warning published", "Avviso pubblicato sul sito del Comune", false)
        .unwrap();
    let decided = ex.log.iter().position(|e| e.kind == "proposal_status" && e.data["proposal"] == p && e.data["change"]["status"] == "approved");
    let published = ex.log.iter().position(|e| e.kind == "narrative_outcome" && e.data["proposal"] == p);
    assert!(decided.is_some() && published.is_some() && decided < published, "{decided:?} {published:?}");
    let r = row(&ex, bulletin);
    assert_eq!(r["replies"][0]["response"]["outcomes"], 1);
}

#[test]
fn a_communication_is_stored_without_approval_and_carries_no_action() {
    let mut ex = exercise();
    let bulletin = inject(&mut ex, "Servizio meteo", &["mayor"], "Bollettino")[0];
    let action = PlayerResponse {
        kind: ResponseKind::Communication,
        commands: vec![sim::Command::IssueWarning { zone: "F4".into() }],
        ..text("Ricevuto", bulletin)
    };
    assert!(ex.player_response("mayor", action).is_err(), "an action makes it a decision");
    // Even a communication to the sender states its basis and rationale.
    let bare = PlayerResponse { kind: ResponseKind::Communication, basis: None, rationale: None, ..text("Ricevuto", bulletin) };
    assert!(ex.player_response("mayor", bare).is_err(), "a reply to the sender needs its basis");
    let p = ex
        .player_response("mayor", PlayerResponse { kind: ResponseKind::Communication, ..text("Ricevuto, seguiamo", bulletin) })
        .unwrap();
    let prop = ex.proposal(p).unwrap();
    assert!(prop.communication);
    assert_eq!(prop.status, ProposalStatus::Informational, "delivered, nothing to approve");
    assert_eq!(prop.evidence.as_ref().unwrap().basis.rationale.as_deref(), Some("Rationale stated by the player"));
    let r = row(&ex, bulletin);
    assert!(r["pending_proposals"].as_array().unwrap().is_empty(), "never blocks EXCON");
    assert_eq!(r["replies"][0]["text"], "Ricevuto, seguiamo");
    assert_eq!(r["replies"][0]["response"]["communication"], true);
    assert!(ex.decide(p, Decision { approve: true, ..Default::default() }).is_err());
}

/// Scenario B: coordination asks the fire direction in a conversation started
/// from the inject; the direction replies in chat, then records its decision.
#[test]
fn a_conversation_from_an_inject_leads_to_a_recorded_decision() {
    let mut ex = exercise();
    // Both received the inject (each has their own copy).
    let ids = inject(&mut ex, "Centrale 112", &["regional_cp", "fire_commander"], "Fumo sul crinale");
    let (coord, direction) = (ids[0], ids[1]);
    let (cid, _) = ex
        .start_linked_conversation("regional_cp", coord, &["fire_commander".into()], "Servono mezzi aerei?", "Do you need aircraft?", &BTreeMap::new(), false)
        .unwrap();
    // A chat intent is text, not a decision and not a mission.
    let before = (ex.proposals.len(), ex.world.d.tasks.len());
    ex.post_conversation_message("fire_commander", cid, "Vado in ricognizione e vi dico", None, "Going to recon", &BTreeMap::new(), false)
        .unwrap();
    assert_eq!((ex.proposals.len(), ex.world.d.tasks.len()), before, "chat is never an executed action");
    // The conversation keeps its inject; the guest reads it from its own row.
    let seen = ex.player_linked_conversations("fire_commander");
    assert_eq!(seen[0]["inject_id"], ex.messages.iter().find(|m| m.id == direction).unwrap().inject_id.unwrap());
    assert_eq!(seen[0]["quote"]["title"], "Fumo sul crinale");
    // Rispondi from the conversation (own copy also received): to the coordination.
    let p = ex
        .player_response(
            "fire_commander",
            PlayerResponse {
                kind: ResponseKind::Decision,
                basis: Some(BasisKind::Procedure),
                rationale: Some("Rationale stated by the player".into()),
                conversation: Some(cid),
                to_players: vec!["regional_cp".into()],
                ..text("Chiediamo un elicottero e chiudiamo la strada del crinale", coord)
            },
        )
        .unwrap();
    let prop = ex.proposal(p).unwrap();
    assert_eq!(prop.status, ProposalStatus::Pending);
    assert_eq!(prop.via_conversation, Some(cid));
    assert_eq!(prop.addressed_to.players, vec!["regional_cp".to_string()]);
    assert_eq!(prop.addressed_to.conversation, Some(cid));
    // The decision is in the conversation (tagged) and both messages are linked to the inject.
    let c = ex.linked_conversation(cid).unwrap();
    assert_eq!(c.messages.len(), 3);
    assert_eq!(c.messages[2].proposal, Some(p));
    let r = row(&ex, coord);
    assert_eq!(r["conversations"][0]["id"], cid);
    assert_eq!(r["conversations"][0]["messages"].as_array().unwrap().len(), 3, "EXCON reads the whole chat");
    assert!(r["pending_proposals"].as_array().unwrap().is_empty(), "not on the coordination's row");
    // EXCON reviews it on the direction's own row, next to the same conversation.
    let own = row(&ex, direction);
    assert_eq!(own["pending_proposals"][0]["role"], "fire_commander");
    assert_eq!(own["conversations"][0]["id"], cid);
    // The direction's own inject row shows the response too (same delivery event).
    assert!(ex.proposals.iter().any(|q| q.id == p && q.inject_id == ex.messages.iter().find(|m| m.id == direction).unwrap().inject_id));
}

#[test]
fn a_response_from_a_conversation_is_listed_once_under_the_responders_own_row() {
    let mut ex = exercise();
    let ids = inject(&mut ex, "Centrale 112", &["regional_cp", "fire_commander"], "Fumo");
    let (coord, direction) = (ids[0], ids[1]);
    let (cid, _) = ex
        .start_linked_conversation("regional_cp", coord, &["fire_commander".into(), "mayor".into()], "Che fate?", "What now?", &BTreeMap::new(), false)
        .unwrap();
    let decide = |t: &str| PlayerResponse { kind: ResponseKind::Decision, basis: Some(BasisKind::OperationalJudgment), conversation: Some(cid), ..text(t, coord) };
    let p = ex.player_response("fire_commander", decide("Elicottero")).unwrap();
    // The mayor never received the inject: the conversation is their only way.
    let q = ex.player_response("mayor", decide("Avviso")).unwrap();
    let pending = |ex: &Exercise, delivery| row(ex, delivery)["pending_proposals"].as_array().unwrap().iter().map(|x| x["id"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(pending(&ex, direction), vec![p], "own copy: own row, never twice");
    assert_eq!(pending(&ex, coord), vec![q], "no copy: the row of the conversation's inject");
    // Both rows show the conversation.
    assert_eq!(row(&ex, direction)["conversations"][0]["id"], cid);
    assert_eq!(row(&ex, coord)["conversations"][0]["id"], cid);
}

#[test]
fn rispondi_to_a_player_opens_or_reuses_the_conversation_with_them() {
    let mut ex = exercise();
    let parent = inject(&mut ex, "Servizio meteo", &["mayor"], "Bollettino")[0];
    let to_cp = |t: &str| PlayerResponse {
        kind: ResponseKind::Communication,
        to_players: vec!["regional_cp".into()],
        texts: BTreeMap::from([("regional_cp".into(), format!("[en] {t}"))]),
        ..text(t, parent)
    };
    let first = ex.player_response("mayor", to_cp("Vi giro il bollettino")).unwrap();
    let second = ex.player_response("mayor", to_cp("Aggiornamento")).unwrap();
    let (a, b) = (ex.proposal(first).unwrap().addressed_to.conversation, ex.proposal(second).unwrap().addressed_to.conversation);
    assert!(a.is_some() && a == b, "same participants, same conversation");
    assert_eq!(ex.proposal(first).unwrap().addressed_to.persona, None);
    // The recipient reads it as a guest of the inject, in their language.
    let guest = ex.player_linked_conversations("regional_cp");
    assert_eq!(guest[0]["messages"][0]["text"], "[en] Vi giro il bollettino");
    // Persona and players together, unknown or own role: refused.
    let mut both = to_cp("x");
    both.to_persona = Some("Servizio meteo".into());
    assert!(ex.player_response("mayor", both).is_err());
    let mut own = to_cp("x");
    own.to_players = vec!["mayor".into()];
    assert!(ex.player_response("mayor", own).is_err());
}
