//! Player conversations linked to an inject (see `exercise::conversations`).
//! Translation happens here, outside the exercise lock; the rules live in the model.
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
use crate::{access::Principal, bad, ApiResult, AppState};
#[cfg(not(target_arch = "wasm32"))]
use crate::body::Json;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

fn own(principal: &Principal, role: &str) -> Result<(), crate::ApiError> {
    if matches!(principal, Principal::Master)
        || matches!(principal, Principal::Player { role: r } if r == role)
    {
        Ok(())
    } else {
        Err(bad("only this player may write in their conversations"))
    }
}

/// English working copy plus the text for each recipient's language.
/// Participants who share the sender's language get the literal text.
pub(crate) async fn translations(
    s: &AppState,
    text: &str,
    sender_language: &str,
    targets: &[(String, String)],
) -> (String, BTreeMap<String, String>, bool) {
    let mut failed = false;
    let english = if sender_language == "en" {
        text.to_string()
    } else {
        match s.llm.translate_to_english(text, sender_language).await {
            Ok(t) => t,
            Err(_) => {
                failed = true;
                text.to_string()
            }
        }
    };
    let mut per_language: BTreeMap<String, String> = BTreeMap::new();
    for (_, language) in targets {
        if per_language.contains_key(language) {
            continue;
        }
        let translated = if language == sender_language {
            text.to_string()
        } else if failed || language == "en" {
            english.clone()
        } else {
            match s.llm.translate(&english, language).await {
                Ok(t) => t,
                Err(_) => {
                    failed = true;
                    english.clone()
                }
            }
        };
        per_language.insert(language.clone(), translated);
    }
    let texts = targets
        .iter()
        .map(|(role, language)| (role.clone(), per_language[language].clone()))
        .collect();
    (english, texts, failed)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartBody {
    /// The player's own copy of the inject (a message id).
    delivery: u64,
    participants: Vec<String>,
    text: String,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/conversations", data = "<body>")
)]
pub(crate) async fn start(
    principal: Principal,
    s: AppState,
    role: String,
    body: Json<StartBody>,
) -> ApiResult {
    own(&principal, &role)?;
    let Json(body) = body;
    let (language, targets, turn, revision) = {
        let ex = s.lock();
        ex.validate_conversation_start(&role, body.delivery, &body.participants, &body.text)?;
        (
            ex.language_of(&role),
            body.participants
                .iter()
                .map(|p| (p.clone(), ex.language_of(p)))
                .collect::<Vec<_>>(),
            ex.turn,
            ex.clock_control.revision,
        )
    };
    let (english, texts, failed) = translations(&s, &body.text, &language, &targets).await;
    let mut ex = s.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed during translation; send again"));
    }
    let (conversation, message) = ex.start_linked_conversation(
        &role,
        body.delivery,
        &body.participants,
        &body.text,
        &english,
        &texts,
        failed,
    )?;
    Ok(Json(
        json!({"conversation":conversation,"message":message,"translation_failed":failed}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageBody {
    text: String,
    /// The inject (its delivery id) or a previous message of the conversation.
    #[serde(default)]
    reply_to: Option<u64>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/conversations/<id>/messages", data = "<body>")
)]
pub(crate) async fn post_message(
    principal: Principal,
    s: AppState,
    role: String,
    id: u64,
    body: Json<MessageBody>,
) -> ApiResult {
    own(&principal, &role)?;
    let Json(body) = body;
    let (language, targets, turn, revision) = {
        let ex = s.lock();
        ex.validate_conversation_message(&role, id, &body.text, body.reply_to)?;
        (
            ex.language_of(&role),
            ex.conversation_languages(id)
                .into_iter()
                .filter(|(p, _)| *p != role)
                .collect::<Vec<_>>(),
            ex.turn,
            ex.clock_control.revision,
        )
    };
    let (english, texts, failed) = translations(&s, &body.text, &language, &targets).await;
    let mut ex = s.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed during translation; send again"));
    }
    let message = ex.post_conversation_message(
        &role,
        id,
        &body.text,
        body.reply_to,
        &english,
        &texts,
        failed,
    )?;
    Ok(Json(json!({"message":message,"translation_failed":failed})))
}
