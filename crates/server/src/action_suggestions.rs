//! AI action suggestions for narrative responses (see `exercise::action_suggestions`).
//!
//! The context is gathered under the exercise lock, the model runs outside it,
//! and its answer is validated again under the lock. Nothing is changed: the
//! suggestions go back to the reader, who adds them by hand.

use crate::access::Principal;
#[cfg(not(target_arch = "wasm32"))]
use crate::body::Json;
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
use crate::{bad, ApiResult, AppState};
use exercise::action_suggestions::SuggestionContext;
use llm::tasks::SuggestScope;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlayerBody {
    narrative: String,
    #[serde(default)]
    reply_to: Option<u64>,
    #[serde(default)]
    conversation: Option<u64>,
    /// Actions already in the response being composed (not suggested again).
    #[serde(default)]
    staged: Vec<sim::Command>,
}

/// Suggestions for the narrative `role` is writing, from what that role knows.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/suggest-actions", data = "<body>")
)]
pub(crate) async fn player(
    principal: Principal,
    state: AppState,
    role: String,
    body: Json<PlayerBody>,
) -> ApiResult {
    if !matches!(&principal, Principal::Master)
        && !matches!(&principal, Principal::Player { role: r } if r == &role)
    {
        return Err(bad(
            "only this player may ask for suggestions on their response",
        ));
    }
    let Json(body) = body;
    let (context, language) = {
        let ex = state.lock();
        let context = ex.player_suggestion_context(
            &role,
            &body.narrative,
            body.reply_to,
            body.conversation,
            &body.staged,
        )?;
        (context, ex.language_of(&role))
    };
    Ok(Json(
        run(
            &state,
            SuggestScope::Player,
            context,
            &language,
            &body.staged,
        )
        .await?,
    ))
}

/// Suggestions the EXCON asked for on narrative proposal `id`, from the full state.
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/suggest-actions")
)]
pub(crate) async fn excon(principal: Principal, state: AppState, id: u64) -> ApiResult {
    if !matches!(principal, Principal::Master) {
        return Err(bad("only EXCON may ask for suggestions on a response"));
    }
    let (context, language, staged) = {
        let ex = state.lock();
        let context = ex.excon_suggestion_context(id)?;
        let staged = ex
            .proposal(id)
            .map(|p| p.commands.clone())
            .unwrap_or_default();
        (context, ex.language_of(exercise::MASTER), staged)
    };
    let mut out = run(&state, SuggestScope::Excon, context, &language, &staged).await?;
    out["proposal"] = json!(id);
    Ok(Json(out))
}

/// Ask the model (outside the lock) and validate its answer for the role.
async fn run(
    state: &AppState,
    scope: SuggestScope,
    context: SuggestionContext,
    language: &str,
    staged: &[sim::Command],
) -> Result<Value, crate::ApiError> {
    let (spec, catalogue, gazetteer, turn, revision) = {
        let ex = state.lock();
        let spec = ex
            .world
            .scn
            .role(&context.role)
            .cloned()
            .ok_or_else(|| bad("unknown role"))?;
        let catalogue = sim::catalogue_for(&ex.world.scn, &spec.allowed_actions);
        (
            spec,
            catalogue,
            ex.world.gazetteer(),
            ex.turn,
            ex.clock_control.revision,
        )
    };
    if spec.allowed_actions.is_empty() || !state.llm.is_available() {
        return Ok(json!({"status": "ai_unavailable", "actions": [], "summary": "", "dropped": 0}));
    }
    let history = context
        .history
        .iter()
        .map(|t| llm::ChatMessage {
            role: t.role.clone(),
            content: t.content.clone(),
        })
        .collect::<Vec<_>>();
    let result = state
        .llm
        .suggest_actions(llm::tasks::SuggestInput {
            scope,
            role_id: &spec.id,
            role_name: &spec.name,
            role_description: &spec.description,
            language,
            catalogue: &catalogue,
            gazetteer: &gazetteer,
            digest: &context.digest,
            conversations: &context.conversations,
            history: &history,
            narrative: &context.narrative,
        })
        .await;
    let suggested = match result {
        Ok(s) => s,
        Err(error) => {
            tracing::warn!(%error, "action suggestions unavailable");
            let failure: String = error.to_string().chars().take(300).collect();
            return Ok(
                json!({"status": "ai_unavailable", "actions": [], "summary": "", "dropped": 0, "failure": failure}),
            );
        }
    };
    let ex = state.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed while suggesting; ask again"));
    }
    let check = ex.check_suggestions(
        &context.role,
        &suggested.actions,
        &suggested.reasons,
        staged,
    );
    Ok(json!({
        "status": if check.actions.is_empty() { "none" } else { "ready" },
        "role": context.role,
        "actions": check.actions,
        "dropped": check.dropped,
        "summary": suggested.summary.chars().take(500).collect::<String>(),
    }))
}
