//! Player responses (`/api/player/<role>/actions`) and EXCON narrative outcomes.
//! Translation only; EXCON owns approval and outcomes.
#[cfg(target_arch = "wasm32")]
use crate::browser::Json;
#[cfg(target_arch = "wasm32")]
use crate::browser_runtime as tokio;
use crate::{access::Principal, bad, ApiResult, AppState};
#[cfg(not(target_arch = "wasm32"))]
use crate::body::Json;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutcomeBody {
    /// Optional when `commands` are given.
    #[serde(default)]
    text: String,
    #[serde(default = "english")]
    language: String,
    /// Who the outcome is spoken as; empty: the inject's sender, then the scenario default.
    #[serde(default)]
    persona: Option<String>,
    /// EXCON interventions applied with the outcome (same as the Controllo tools).
    #[serde(default)]
    commands: Vec<sim::Command>,
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
            ex.validate_outcome(id, &body.text, body.commands.len())?,
            ex.turn,
            ex.clock_control.revision,
        )
    };
    let language = s.lock().language_of(&role);
    let (english, mut failed) = if body.text.trim().is_empty() {
        (String::new(), false)
    } else if body.language == "en" {
        (body.text.clone(), false)
    } else {
        match s.llm.translate_to_english(&body.text, &body.language).await {
            Ok(t) => (t, false),
            Err(_) => (body.text.clone(), true),
        }
    };
    let player_text = if body.text.trim().is_empty() {
        String::new()
    } else if language == body.language {
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
    let mid = ex.record_outcome(
        id,
        exercise::narrative::OutcomeInput {
            text: body.text,
            text_en: english,
            player_text,
            translation_failed: failed,
            persona: body.persona,
            commands: body.commands,
        },
    )?;
    let linked = ex
        .proposal(id)
        .and_then(|p| p.narrative_outcomes.last())
        .map(|o| o.interventions.iter().map(|x| x.log_seq).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(Json(
        json!({"message":mid,"translation_failed":failed,"interventions":linked}),
    ))
}

/// Body of the merged response: optional narrative plus optional actions.
#[derive(Deserialize)]
pub(crate) struct ResponseBody {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub commands: Vec<sim::Command>,
    #[serde(default)]
    pub reply_to: Option<u64>,
    #[serde(default)]
    pub narrative: Option<String>,
    /// Linked conversation the response comes out of.
    #[serde(default)]
    pub conversation: Option<u64>,
    /// What the response is based on: required for a decision.
    #[serde(default)]
    pub basis: Option<exercise::BasisKind>,
    /// Which plan, procedure or law applies (required for a plan/law basis
    /// in a reply to the sender).
    #[serde(default)]
    pub basis_note: Option<String>,
    /// Why this decision: required in a narrated reply to the sender.
    #[serde(default)]
    pub rationale: Option<String>,
    /// PDF of the plan or law cited (plan/law basis only).
    #[serde(default)]
    pub attachment: Option<AttachmentBody>,
    /// "communication" (no approval, no basis) or "decision" (basis required).
    pub kind: exercise::ResponseKind,
    /// Fictitious sender answered (default: the inject's sender).
    #[serde(default)]
    pub to_persona: Option<String>,
    /// Real players the response goes to instead.
    #[serde(default)]
    pub to_players: Vec<String>,
}

/// A file inside a JSON body: base64 data, checked before it is decoded.
#[derive(Deserialize)]
pub(crate) struct AttachmentBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    content_type: String,
    data: String,
}

impl AttachmentBody {
    fn decode(self) -> Result<exercise::PdfUpload, crate::ApiError> {
        use base64::Engine;
        if self.data.len() > exercise::files::MAX_PDF_BYTES / 3 * 4 + 8 {
            return Err(bad(format!(
                "the attached PDF exceeds {} MB",
                exercise::files::MAX_PDF_BYTES / 1024 / 1024
            )));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(self.data.trim())
            .map_err(|_| bad("the attached file is not valid base64"))?;
        Ok(exercise::PdfUpload { name: self.name, content_type: self.content_type, bytes })
    }
}

/// One response, one proposal. The narrative (if any) is translated outside
/// the exercise lock; the commands are never interpreted here.
pub(crate) async fn respond(s: AppState, role: String, body: ResponseBody) -> ApiResult {
    let mut response = exercise::response::PlayerResponse {
        narrative: body.narrative.filter(|t| !t.trim().is_empty()),
        text: body.text,
        commands: body.commands,
        reply_to: body.reply_to,
        conversation: body.conversation,
        basis: body.basis,
        basis_note: body.basis_note,
        rationale: body.rationale,
        document: body.attachment.map(AttachmentBody::decode).transpose()?,
        kind: body.kind,
        to_persona: body.to_persona,
        to_players: body.to_players,
        ..Default::default()
    };
    let (language, targets, turn, revision) = {
        let ex = s.lock();
        ex.validate_player_response(&role, &response)?;
        let targets = response
            .to_players
            .iter()
            .map(|p| (p.clone(), ex.language_of(p)))
            .collect::<Vec<_>>();
        (ex.language_of(&role), targets, ex.turn, ex.clock_control.revision)
    };
    if let Some(text) = response.narrative.clone() {
        // Players addressed directly read it in their own language.
        let (english, texts, failed) =
            crate::conversations::translations(&s, &text, &language, &targets).await;
        response.narrative_en = english;
        response.texts = texts;
        response.translation_failed = failed;
    }
    let failed = response.translation_failed;
    let mut ex = s.lock();
    if ex.turn != turn || ex.clock_control.revision != revision {
        return Err(bad("exercise changed during translation; send again"));
    }
    let pid = ex.player_response(&role, response)?;
    // Players never propose actions: the AI reads the reply for the EXCON.
    if ex.reviewable(pid) {
        tokio::spawn(crate::review::read(s.clone(), pid));
    }
    Ok(Json(json!({"proposal":pid,"translation_failed":failed})))
}
