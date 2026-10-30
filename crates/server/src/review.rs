//! EXCON review of player replies over HTTP (see `exercise::reply_review`).
//!
//! The AI reads a reply thread outside the exercise lock ([`read`]); the
//! answer is validated and merged under the lock. Every EXCON act is one call
//! of `POST /api/master/review/<op>/<id>`:
//!
//! | op | id | body | effect |
//! |---|---|---|---|
//! | `execute` | suggestion | `{params?, role?}` | the command runs now |
//! | `edit` | suggestion | `{params, role?}` | parameters changed, feasibility rechecked |
//! | `ignore` / `restore` | suggestion | `{note?}` | not acted on / back to open |
//! | `send` | question | `{text?, persona?}` | the reviewed draft is sent (the only way) |
//! | `discard` | question | | the draft (or unanswered question) is dropped |
//! | `ask` | reply | `{to, text}` | an EXCON-written draft |
//! | `close` | reply | | review closed; unsent drafts discarded; the reply is settled (no longer pending) |
//! | `reread` | reply | | the AI reads the thread again |

use crate::access::Principal;
#[cfg(not(target_arch = "wasm32"))]
use crate::body::Json;
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
#[cfg(target_arch = "wasm32")]
use crate::browser_runtime as tokio;
use crate::{bad, ApiResult, AppState};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewBody {
    #[serde(default)]
    params: Option<Value>,
    /// `execute` only: `"play"` (default, queued until time moves) or `"now"`.
    #[serde(default)]
    when: Option<String>,
    /// `execute` only: queue for this `HH:MM` instead of the next advance.
    #[serde(default)]
    at_clock: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    persona: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/review/<op>/<id>", data = "<body>")
)]
pub(crate) async fn post(
    principal: Principal,
    state: AppState,
    op: String,
    id: u64,
    body: Json<ReviewBody>,
) -> ApiResult {
    if !matches!(principal, Principal::Master) {
        return Err(bad("only EXCON reviews replies"));
    }
    let Json(b) = body;
    match op.as_str() {
        "execute" => {
            let mut ex = state.lock();
            let when = crate::excon_queue::when_of(
                &json!({"when": b.when, "at_clock": b.at_clock}),
                ex.world.scn.start_clock_s,
            )?
            .unwrap_or_default();
            let run = ex.execute_review_item(id, b.params, b.role, when)?;
            let queued = ex.review.items.iter().find(|i| i.id == id).and_then(|i| i.queued);
            Ok(Json(json!({"execution": run, "queued": queued})))
        }
        "edit" => {
            let mut ex = state.lock();
            ex.edit_review_item(id, b.role, b.params.ok_or_else(|| bad("params required"))?)?;
            Ok(Json(json!({"ok": true})))
        }
        "ignore" => {
            state.lock().ignore_review_item(id, b.note)?;
            Ok(Json(json!({"ok": true})))
        }
        "restore" => {
            state.lock().restore_review_item(id)?;
            Ok(Json(json!({"ok": true})))
        }
        "send" => send(&state, id, b).await,
        "discard" => {
            state.lock().discard_review_question(id)?;
            Ok(Json(json!({"ok": true})))
        }
        "ask" => {
            let to = b.to.ok_or_else(|| bad("recipient required"))?;
            let q = state
                .lock()
                .add_review_question(id, &to, b.text.as_deref().unwrap_or(""))?;
            Ok(Json(json!({"question": q})))
        }
        "close" => {
            state.lock().close_review(id)?;
            Ok(Json(json!({"ok": true})))
        }
        "reread" => {
            if !state.lock().reviewable(id) {
                return Err(bad("this reply cannot be read again"));
            }
            tokio::spawn(read(state.clone(), id));
            Ok(Json(json!({"ok": true})))
        }
        _ => Err(bad("unknown review operation")),
    }
}

/// Send the draft `qid` as reviewed: the text goes out verbatim in the
/// player's language; the English copy is a translation for the record.
async fn send(state: &AppState, qid: u64, b: ReviewBody) -> ApiResult {
    let (to, text, persona, language, turn, revision) = {
        let ex = state.lock();
        let (to, text, persona) =
            ex.review_question_to_send(qid, b.text.as_deref(), b.persona.as_deref())?;
        let language = ex.language_of(&to);
        (
            to,
            text,
            persona,
            language,
            ex.turn,
            ex.clock_control.revision,
        )
    };
    let text_en = if language == "en" || !state.llm.is_available() {
        text.clone()
    } else {
        state
            .llm
            .translate_to_english(&text, &language)
            .await
            .unwrap_or_else(|_| text.clone())
    };
    let mut ex = state.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed while sending; send again"));
    }
    let message = ex.send_review_question(qid, &text, &text_en, persona)?;
    Ok(Json(json!({"message": message, "to": to})))
}

/// Have the AI read the thread of reply `pid` (call it spawned). Nothing is
/// executed and nothing is sent: the result is a set of drafts for the EXCON.
pub async fn read(state: AppState, pid: u64) {
    let prep = {
        let mut ex = state.lock();
        if !ex.reviewable(pid) {
            return;
        }
        let Ok(revision) = ex.begin_review(pid) else {
            return;
        };
        let Ok(context) = ex.review_context(pid) else {
            return;
        };
        let mut kinds: Vec<String> = ex
            .world
            .scn
            .roles
            .iter()
            .flat_map(|r| r.allowed_actions.clone())
            .collect();
        kinds.sort();
        kinds.dedup();
        let catalogue = sim::catalogue_for(&ex.world.scn, &kinds);
        (
            revision,
            context,
            catalogue,
            ex.world.gazetteer(),
            ex.language_of(exercise::MASTER),
        )
    };
    let (revision, context, catalogue, gazetteer, language) = prep;
    if !state.llm.is_available() {
        let _ = state.lock().review_failed(pid, revision, "AI unavailable");
        return;
    }
    let result = state
        .llm
        .review_reply(llm::tasks::ReviewInput {
            persona: &context.persona,
            language: &language,
            catalogue: &catalogue,
            roles: &context.roles,
            gazetteer: &gazetteer,
            digest: &context.digest,
            thread: &context.thread,
            items: &context.items,
            words: &context.player_words,
        })
        .await;
    let mut ex = state.lock();
    let outcome = result.and_then(|r| {
        let dropped = r.dropped;
        let mut reading: exercise::reply_review::Reading =
            serde_json::from_value(serde_json::to_value(r)?)?;
        reading.dropped = dropped;
        ex.ingest_reading(pid, revision, reading)
    });
    if let Err(e) = outcome {
        tracing::warn!("review of reply {pid}: {e}");
        let _ = ex.review_failed(pid, revision, &e.to_string());
    }
}
