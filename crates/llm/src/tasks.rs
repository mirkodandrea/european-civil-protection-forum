//! The exercise workflows (spec §6): each builds a prompt from exercise data
//! and returns text or candidate actions. Inputs are plain strings / JSON so
//! this crate does not depend on the simulator.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ChatMessage, Llm};

/// Human-readable names for the language codes the UI offers.
pub fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "it" => "Italian",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "hy" => "Armenian",
        "ka" => "Georgian",
        "uk" => "Ukrainian",
        "ro" => "Romanian",
        "az" => "Azerbaijani",
        "ru" => "Russian",
        "el" => "Greek",
        "pt" => "Portuguese",
        other => other,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleMessage {
    pub to: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InjectDraft {
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub text: String,
}

/// What the model made of a player reply or a EXCON prompt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedReply {
    /// EXCON-only recommendation; applied only after explicit review/approval.
    #[serde(default)]
    pub resource_limits:Option<Value>,
    #[serde(default)]
    pub detected_language: String,
    /// The reply in English (the working language), for the EXCON.
    #[serde(default)]
    pub translation_en: String,
    /// Candidate commands, raw JSON (validated by the simulator).
    #[serde(default)]
    pub actions: Vec<Value>,
    /// Messages to other roles (information sharing, requests).
    #[serde(default)]
    pub messages: Vec<RoleMessage>,
    /// Injects the EXCON asked to draft (EXCON prompts only).
    #[serde(default)]
    pub injects: Vec<InjectDraft>,
    /// A question back to the author, in their language, when intent is unclear.
    #[serde(default)]
    pub clarification: Option<String>,
    /// One-line English summary of the interpretation.
    #[serde(default)]
    pub summary: String,
}

impl ParsedReply {
    fn from_value(v: Value) -> Result<ParsedReply> {
        let mut p: ParsedReply = serde_json::from_value(v)?;
        if p.clarification.as_deref().is_some_and(|c| c.trim().is_empty() || c.trim() == "null") {
            p.clarification = None;
        }
        Ok(p)
    }
}

pub struct ReplyInput<'a> {
    pub role_id: &'a str,
    pub role_name: &'a str,
    pub role_description: &'a str,
    pub language: &'a str,
    /// Allowed-command documentation (from `sim::catalogue`).
    pub catalogue: &'a str,
    pub other_roles: &'a [(String, String)],
    pub gazetteer: &'a Value,
    pub digest: &'a Value,
    /// The conversation so far with this player: what they received
    /// (injects, messages, feedback, situation updates), what they wrote and
    /// how it was interpreted. Oldest first.
    pub history: &'a [ChatMessage],
    pub reply: &'a str,
}

pub struct MasterInput<'a> {
    pub catalogue: &'a str,
    pub roles: &'a [(String, String)],
    pub gazetteer: &'a Value,
    pub digest: &'a Value,
    /// The conversation so far with the EXCON, oldest first.
    pub history: &'a [ChatMessage],
    pub prompt: &'a str,
}

const REPLY_SCHEMA: &str = r#"{
  "detected_language": "ISO 639-1 code of the reply",
  "translation_en": "the reply translated to English (verbatim meaning)",
  "actions": [ /* zero or more action objects exactly as documented */ ],
  "messages": [ {"to": "role_id", "text": "message in English"} ],
  "clarification": null or "short clarification question in English for Master review",
  "summary": "one English sentence: what the player decided"
}"#;

impl Llm {
    /// Translate an exercise message. English targets are returned unchanged
    /// without a call (the working language is English).
    pub async fn translate(&self, text: &str, target_lang: &str) -> Result<String> {
        if target_lang == "en" || text.trim().is_empty() {
            return Ok(text.to_string());
        }
        let system = format!(
            "You translate civil-protection exercise messages into {}. Keep place names, facility names, zone codes (like F4), road ids, numbers and times unchanged. Preserve the original meaning exactly. Never add facts, decisions, advice, interpretations, explanations, urgency, or operational instructions. Never change a request or pending proposal into an accomplished action. Output only the translation, nothing else.",
            language_name(target_lang)
        );
        Ok(self.complete("translate", &system, text, false).await?.trim().to_string())
    }

    /// Player free text → candidate actions, messages, or a clarification
    /// question. The request is the ongoing conversation with this player
    /// (`history`) plus the new reply and the current situation, so follow-ups
    /// ("send them there", answers to a clarification) keep their context.
    pub async fn parse_reply(&self, i: ReplyInput<'_>) -> Result<ParsedReply> {
        let system = format!(
            "You are the exercise assistant of a Civil Protection table-top exercise (TTX), in an ongoing conversation with one player: \
the {} ({}) — {}. The player writes in {}. \
The conversation contains what the player received (injects, messages, feedback, situation updates), what the player wrote, and your earlier interpretations. \
You turn each new reply into structured actions for the simulator. Rules:\n\
- Use ONLY the action types documented below, with exactly the documented JSON shape.\n\
- Use ONLY place names, facility names, road ids and zone codes from the gazetteer, or zone codes like F4. Never invent places, resources or facts.\n\
- The player decides: add an action only if the player clearly ordered it in the NEW reply. Do not repeat actions from earlier replies. Do not add your own ideas.\n\
- Use the conversation to resolve references (\"there\", \"them\", an answer to your earlier question). If an order is still ambiguous or misses essential information, do NOT guess: leave it out and draft one short clarification question in English for Master review.\n\
- Information shared or requested with other roles goes into \"messages\" (role ids: {}).\n\
- Do not check resource availability yourself; the simulator validates.\n\
Answer with ONE JSON object:\n{REPLY_SCHEMA}\n\nAllowed actions for this player:\n{}\n\nGAZETTEER:\n{}",
            i.role_name,
            i.role_id,
            i.role_description,
            language_name(i.language),
            i.other_roles.iter().map(|(id, name)| format!("{id} = {name}")).collect::<Vec<_>>().join(", "),
            i.catalogue,
            i.gazetteer
        );
        let mut messages = i.history.to_vec();
        messages.push(ChatMessage::user(format!("CURRENT SITUATION:\n{}\n\nNEW PLAYER REPLY:\n\"\"\"{}\"\"\"", i.digest, i.reply)));
        ParsedReply::from_value(self.chat_json("parse_reply", &system, &messages).await?)
    }

    /// EXCON natural-language instruction → commands and/or inject drafts,
    /// in the ongoing conversation with the EXCON.
    pub async fn master_prompt(&self, i: MasterInput<'_>) -> Result<ParsedReply> {
        let system = format!(
            "You assist the Exercise Director (Game Master) of a Civil Protection table-top exercise, in an ongoing conversation. \
The Director reviews every suggestion before it becomes effective. Convert each new instruction into simulator actions \
and/or draft injects (messages to players, in English, from a simulated sender such as \"112 Dispatch\" or \"ERCC\"). \
Respect the original training design, current phase/path, actual physical/resource state and recorded decisions in CURRENT SITUATION. Never undo successful containment merely to force escalation. If a proposed contingency or adaptation diverges from the original design or observed successful outcome, prefix summary with UNPLANNED DIVERGENCE and explain why explicit Director review and rationale are needed. No path changes occur without explicit Director approval. Use the conversation for context (earlier instructions and what you proposed). Use only names from the gazetteer or zone codes. \
You may suggest exercise-wide and per-training-phase deployable resource caps as resource_limits: {{total:{{resource_id:units}},phases:{{early_warning:{{resource_id:units}},early_action:{{...}},escalation:{{...}},full_response:{{...}}}}}} or null. Supply the complete replacement configuration and respect active deployments, committed orders and inventory from CURRENT SITUATION. Limits are recommendations: never claim they changed before Director approval. If the instruction is unclear, return no actions and a clarification question.\n\
Answer with ONE JSON object:\n{{\"resource_limits\": null, \"actions\": [...], \"injects\": [{{\"recipients\": [\"role_id\" or \"all\"], \"title\": \"...\", \"sender\": \"...\", \"text\": \"...\"}}], \"clarification\": null, \"summary\": \"one sentence\", \"translation_en\": \"the instruction in English\"}}\n\n\
Actions:\n{}\nRoles: {}\n\nGAZETTEER:\n{}",
            i.catalogue,
            i.roles.iter().map(|(id, name)| format!("{id} = {name}")).collect::<Vec<_>>().join(", "),
            i.gazetteer
        );
        let mut messages = i.history.to_vec();
        messages.push(ChatMessage::user(format!("CURRENT SITUATION:\n{}\n\nNEW DIRECTOR INSTRUCTION:\n\"\"\"{}\"\"\"", i.digest, i.prompt)));
        ParsedReply::from_value(self.chat_json("master_prompt", &system, &messages).await?)
    }

    /// Translate the recorded EXCON decision without generating operational content.
    pub async fn feedback(&self, _role_name: &str, lang: &str, decision_en: &str) -> Result<String> {
        self.translate(decision_en, lang).await
    }

    /// Draft a situation summary for review. Operational delivery uses recorded facts
    /// and translation instead of sending this generated text automatically.
    pub async fn situation_update(&self, role_name: &str, lang: &str, digest: &Value, events_en: &[String]) -> Result<String> {
        let system = format!(
            "You write the situation update that EXCON sends to the {role_name} at the end of a turn of a Civil Protection table-top exercise, in {}. \
At most 120 words, factual, operational tone, only facts present in the data (clock, fire, what happened this turn, the player's resources). \
No advice on what to do. Output only the message.",
            language_name(lang)
        );
        let user = format!("EVENTS THIS TURN:\n{}\n\nSITUATION:\n{}", if events_en.is_empty() { "(none)".into() } else { events_en.join("\n") }, digest);
        Ok(self.complete("situation_update", &system, &user, false).await?.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input<'a>(reply: &'a str, g: &'a Value) -> ReplyInput<'a> {
        ReplyInput {
            role_id: "mayor",
            role_name: "Mayor",
            role_description: "local",
            language: "it",
            catalogue: "- EvacuateZone",
            other_roles: &[],
            gazetteer: g,
            digest: g,
            history: &[],
            reply,
        }
    }

    #[tokio::test]
    async fn parse_reply_maps_model_json() {
        let llm = Llm::mock(|task, system, user| {
            assert_eq!(task, "parse_reply");
            assert!(system.contains("EvacuateZone"));
            assert!(user.contains("evacuate F4"));
            assert!(user.contains("earlier context"), "history must be sent: {user}");
            Ok(r#"{"detected_language":"it","translation_en":"evacuate F4","actions":[{"type":"EvacuateZone","zone":"F4"}],"messages":[],"clarification":"","summary":"Evacuate F4"}"#.into())
        });
        let g = json!({});
        let history = [ChatMessage::user("earlier context"), ChatMessage::assistant("{}")];
        let p = llm.parse_reply(ReplyInput { history: &history, ..input("evacuate F4", &g) }).await.unwrap();
        assert_eq!(p.actions.len(), 1);
        assert_eq!(p.actions[0]["zone"], "F4");
        assert!(p.clarification.is_none(), "empty clarification must become None");
    }

    #[tokio::test]
    async fn translate_skips_english() {
        let llm = Llm::mock(|_, _, _| panic!("should not be called"));
        assert_eq!(llm.translate("hello", "en").await.unwrap(), "hello");
    }
}

#[cfg(test)]
mod translation_control_tests {
    #[tokio::test]
    async fn translation_preserves_original_input_and_forbids_operational_additions() {
        let facts = "PENDING: evacuate F4 at 13:30 with 2 buses. Not yet executed.";
        let model = crate::Llm::mock(move |task, system, user| {
            assert_eq!(task, "translate");
            assert_eq!(user, facts);
            assert!(system.contains("Never add facts, decisions, advice"));
            assert!(system.contains("Never change a request or pending proposal into an accomplished action"));
            Ok("IN ATTESA: evacuare F4 alle 13:30 con 2 autobus. Non ancora eseguito.".into())
        });
        assert!(model.translate(facts, "it").await.unwrap().contains("Non ancora eseguito"));
    }
}
