use std::sync::Arc;

use exercise::{Exercise, MsgKind, Phase};
use sim::World;

fn exercise() -> Exercise {
    let scenario = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    Exercise::new(World::new(scenario, 1).unwrap())
}

#[test]
fn replies_only_reference_chat_messages_in_the_same_peer_thread() {
    let mut ex = exercise();
    ex.start().unwrap();
    let parent = ex.add_message(
        "mayor",
        "fire_commander",
        MsgKind::Chat,
        None,
        "Check the access road.",
        "Check the access road.",
        None,
    );
    let (reply, proposal) = ex
        .chat_reply(
            "fire_commander",
            "mayor",
            "Will do.",
            "Will do.",
            "Lo farò.",
            false,
            false,
            Some(parent),
        )
        .unwrap();
    assert_eq!(proposal, None);
    assert_eq!(
        ex.messages.iter().find(|m| m.id == reply).unwrap().reply_to,
        Some(parent)
    );
    assert_eq!(
        ex.log
            .iter()
            .find(|e| e.kind == "chat_source")
            .unwrap()
            .data["reply_to"],
        parent
    );
    assert_eq!(
        ex.log
            .iter()
            .find(|e| e.kind == "message" && e.data["id"] == reply)
            .unwrap()
            .data["reply_to"],
        parent
    );
    assert!(ex
        .conversation("fire_commander")
        .iter()
        .any(|m| m.content.contains(&format!("message {parent}"))));
    assert!(ex
        .conversation("mayor")
        .iter()
        .any(|m| m.content.contains(&format!("message {parent}"))));

    let inject = ex.add_message(
        "master",
        "mayor",
        MsgKind::Inject,
        None,
        "Update",
        "Update",
        None,
    );
    assert!(ex
        .validate_chat_reply("mayor", "fire_commander", "Thanks", false, Some(inject))
        .is_err());
    assert!(ex
        .validate_chat_reply("mayor", "regional_cp", "Thanks", false, Some(parent))
        .is_err());
    assert!(ex
        .validate_chat_reply("mayor", "fire_commander", "Thanks", false, Some(u64::MAX))
        .is_err());
    assert!(ex
        .validate_chat_reply("mayor", "mayor", "Thanks", false, Some(parent))
        .is_err());
}

#[test]
fn profanity_is_blocked_for_players_but_not_excon_prompts() {
    let mut ex = exercise();
    ex.start().unwrap();
    let before = ex.messages.len();
    let rejected = ex.chat_reply(
        "mayor",
        "fire_commander",
        "Sei uno strònzo!",
        "",
        "",
        false,
        false,
        None,
    );
    assert_eq!(
        rejected.unwrap_err().to_string(),
        "Messaggio rifiutato: evita insulti e volgarità."
    );
    assert_eq!(ex.messages.len(), before, "rejected text is never stored");

    // The role-facing filter is scoped to player chat, not EXCON operational input.
    assert!(ex
        .validate_chat("master", "mayor", "shit happens", false)
        .is_ok());
    assert!(
        ex.validate_chat_reply("mayor", "master", "shit happens", false, None)
            .is_ok(),
        "moderation applies only to peer-to-peer player chat"
    );
    assert_eq!(ex.phase, Phase::Orders);
}

#[test]
fn cross_turn_reply_summary_quotes_parent_in_role_visible_form() {
    let mut ex = exercise();
    ex.start().unwrap();
    let parent = ex.add_message(
        "mayor",
        "fire_commander",
        MsgKind::Chat,
        None,
        "Road access is blocked.",
        "Road access is blocked.",
        None,
    );
    ex.resolve_turn(Some(1)).unwrap();
    ex.chat_reply(
        "fire_commander",
        "mayor",
        "Can you reopen it?",
        "Can you reopen it?",
        "Puoi riaprirla?",
        false,
        false,
        Some(parent),
    )
    .unwrap();
    ex.resolve_turn(Some(1)).unwrap();
    let mayor = ex
        .claim_turn_summaries()
        .into_iter()
        .find(|(_, _, evidence)| evidence["role"] == "mayor" && evidence["turn"] == 2)
        .map(|(_, _, evidence)| evidence)
        .unwrap();
    let reply = mayor["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["text"] == "Puoi riaprirla?")
        .unwrap();
    assert_eq!(reply["reply_context"]["id"], parent);
    assert_eq!(reply["reply_context"]["text"], "Road access is blocked.");
}
