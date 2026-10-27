//! Translation-only narrative initiatives; EXCON owns approval and outcomes.
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
use crate::{access::Principal, bad, ApiResult, AppState};
#[cfg(not(target_arch = "wasm32"))]
use rocket::serde::json::Json;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmitBody {
    text: String,
    reply_to: u64,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/player/<role>/narrative", data = "<body>")
)]
pub(crate) async fn submit(
    _principal: Principal,
    s: AppState,
    role: String,
    body: Json<SubmitBody>,
) -> ApiResult {
    if !matches!(&_principal, Principal::Master)
        && !matches!(&_principal, Principal::Player {role:r} if r==&role)
    {
        return Err(bad("only this player or EXCON may submit the initiative"));
    }
    let Json(body) = body;
    let (language, turn, revision) = {
        let ex = s.lock();
        ex.validate_narrative(&role, &body.text, body.reply_to)?;
        (ex.language_of(&role), ex.turn, ex.clock_control.revision)
    };
    let (english, failed) = if language == "en" {
        (body.text.clone(), false)
    } else {
        match s.llm.translate_to_english(&body.text, &language).await {
            Ok(text) => (text, false),
            Err(_) => (body.text.clone(), true),
        }
    };
    let mut ex = s.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed during translation; send again"));
    }
    let pid = ex.player_narrative(&role, &body.text, body.reply_to, &english, failed)?;
    Ok(Json(json!({"proposal":pid,"translation_failed":failed})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutcomeBody {
    text: String,
    #[serde(default = "english")]
    language: String,
}
fn english() -> String {
    "en".into()
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/proposals/<id>/narrative-outcome", data = "<body>")
)]
pub(crate) async fn outcome(
    _principal: Principal,
    s: AppState,
    id: u64,
    body: Json<OutcomeBody>,
) -> ApiResult {
    if !matches!(_principal, Principal::Master) {
        return Err(bad("only EXCON may record a narrative outcome"));
    }
    let Json(body) = body;
    if !["en", "it"].contains(&body.language.as_str()) {
        return Err(bad("outcome language must be en or it"));
    }
    let (role, turn, revision) = {
        let ex = s.lock();
        (
            ex.validate_narrative_outcome(id, &body.text)?,
            ex.turn,
            ex.clock_control.revision,
        )
    };
    let language = s.lock().language_of(&role);
    let (english, mut failed) = if body.language == "en" {
        (body.text.clone(), false)
    } else {
        match s.llm.translate_to_english(&body.text, &body.language).await {
            Ok(t) => (t, false),
            Err(_) => (body.text.clone(), true),
        }
    };
    let player_text = if language == body.language {
        body.text.clone()
    } else if language == "en" || failed {
        english.clone()
    } else {
        match s.llm.translate(&english, &language).await {
            Ok(t) => t,
            Err(_) => {
                failed = true;
                english.clone()
            }
        }
    };
    let mut ex = s.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad(
            "exercise changed during translation; record the outcome again",
        ));
    }
    let mid = ex.record_narrative_outcome(id, &body.text, &english, &player_text, failed)?;
    Ok(Json(json!({"message":mid,"translation_failed":failed})))
}
