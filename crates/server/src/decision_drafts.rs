//! Private interpretation before a player sends a decision into a conversation.

use crate::access::Principal;
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
use crate::{bad, ApiResult, AppState};
#[cfg(not(target_arch = "wasm32"))]
use rocket::serde::json::Json;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub(crate) struct PrepareBody {
    to: String,
    text: String,
    #[serde(default)]
    reply_to: Option<u64>,
    #[serde(default)]
    draft: Option<u64>,
    #[serde(default)]
    revision: Option<u64>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/draft", data = "<body>")
)]
pub(crate) async fn prepare(
    _principal: Principal,
    state: AppState,
    role: String,
    body: Json<PrepareBody>,
) -> ApiResult {
    let Json(body) = body;
    let previous = match (body.draft, body.revision) {
        (None, None) => None,
        (Some(id), Some(revision)) => Some((id, revision)),
        _ => return Err(bad("provide both draft and revision")),
    };
    let (draft, role_spec, language, catalogue, others, gazetteer, digest, history) = {
        let mut ex = state.lock();
        let draft = ex.begin_player_draft(&role, &body.to, &body.text, previous, body.reply_to)?;
        let role_spec = ex.world.scn.role(&role).unwrap().clone();
        let others = ex
            .world
            .scn
            .roles
            .iter()
            .filter(|r| r.id != role)
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect::<Vec<_>>();
        let digest = json!({"clock":ex.world.clock(),"recipient":body.to,"inject":body.reply_to.and_then(|id|ex.messages.iter().find(|m|m.id==id)),"map":ex.player_map(&role)?,"resources":ex.world.d.pools.get(&role),"deployments":ex.world.d.tasks.iter().filter(|task|task.role==role).collect::<Vec<_>>()});
        let history = ex
            .conversation(&role)
            .into_iter()
            .map(|turn| llm::ChatMessage {
                role: turn.role,
                content: turn.content,
            })
            .collect::<Vec<_>>();
        (
            draft,
            role_spec.clone(),
            ex.language_of(&role),
            sim::catalogue(&role_spec.allowed_actions),
            others,
            ex.world.gazetteer(),
            digest,
            history,
        )
    };
    let evolving = draft
        .transcript
        .iter()
        .map(|turn| {
            format!(
                "{}: {}",
                if turn.role == "user" {
                    "PLAYER"
                } else {
                    "PRIVATE ASSISTANT QUESTION"
                },
                turn.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let result = state
        .llm
        .prepare_decision(llm::tasks::ReplyInput {
            role_id: &role,
            role_name: &role_spec.name,
            role_description: &role_spec.description,
            language: &language,
            catalogue: &catalogue,
            other_roles: &others,
            gazetteer: &gazetteer,
            digest: &digest,
            history: &history,
            reply: &evolving,
        })
        .await;
    let mut ex = state.lock();
    let completed = match result {
        Ok((parsed, message)) => {
            let input = exercise::ParsedInput {
                language: parsed.detected_language,
                translation_en: parsed.translation_en,
                actions: parsed.actions,
                clarification: parsed.clarification,
                summary: parsed.summary,
                ..Default::default()
            };
            match ex.finish_player_draft(&role, draft.id, draft.revision, input, message) {
                Ok(draft) => draft,
                Err(error) => {
                    tracing::warn!(%error, "invalid private decision interpretation");
                    ex.player_draft_failed(&role, draft.id, draft.revision)?
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "private decision interpretation unavailable");
            ex.player_draft_failed(&role, draft.id, draft.revision)?
        }
    };
    Ok(Json(json!({"draft":completed})))
}

#[derive(Deserialize)]
pub(crate) struct SubmitBody {
    draft: u64,
    revision: u64,
    #[serde(default)]
    manual: bool,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/draft/submit", data = "<body>")
)]
pub(crate) async fn submit(
    _principal: Principal,
    state: AppState,
    role: String,
    body: Json<SubmitBody>,
) -> ApiResult {
    let Json(body) = body;
    let (draft, language, target_language) = {
        let ex = state.lock();
        let draft = ex.private_draft(&role, body.draft, body.revision)?;
        if draft.status != exercise::drafts::DraftStatus::Ready
            && !(body.manual && draft.status == exercise::drafts::DraftStatus::AiUnavailable)
        {
            return Err(bad(
                "answer the assistant's question before sending this decision",
            ));
        }
        let target = ex.language_of(&draft.to);
        (draft, ex.language_of(&role), target)
    };
    // The working English message is produced by the same interpretation the player reviewed.
    let mut failed = false;
    let english = {
        // No private parsed data is accepted from the HTTP client.
        let ex = state.lock();
        ex.draft_english(&role, body.draft, body.revision)?
    };
    let english = if body.manual && language != "en" {
        match state
            .llm
            .translate_to_english(&draft.message, &language)
            .await
        {
            Ok(text) => text,
            Err(_) => {
                failed = true;
                draft.message.clone()
            }
        }
    } else {
        english
    };
    let translated = if failed || target_language == "en" {
        english.clone()
    } else {
        match state.llm.translate(&english, &target_language).await {
            Ok(text) => text,
            Err(_) => {
                failed = true;
                english.clone()
            }
        }
    };
    let (message, proposal) = state.lock().submit_player_draft(
        &role,
        body.draft,
        body.revision,
        &english,
        &translated,
        failed,
        body.manual,
    )?;
    Ok(Json(
        json!({"message":message,"proposal":proposal,"translation_failed":failed}),
    ))
}
