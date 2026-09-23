//! The exercise workflows (spec §6): each builds a prompt from exercise data
//! and returns text or candidate actions. Inputs are plain strings / JSON so
//! this crate does not depend on the simulator.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Llm;

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

/// What the model made of a player reply or a Master prompt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedReply {
    #[serde(default)]
    pub detected_language: String,
    /// The reply in English (the working language), for the Master.
    #[serde(default)]
    pub translation_en: String,
    /// Candidate commands, raw JSON (validated by the simulator).
    #[serde(default)]
    pub actions: Vec<Value>,
    /// Messages to other roles (information sharing, requests).
    #[serde(default)]
    pub messages: Vec<RoleMessage>,
    /// Injects the Master asked to draft (Master prompts only).
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
    /// Recent messages the player received, in English.
    pub recent_injects: &'a [String],
    pub reply: &'a str,
}

pub struct MasterInput<'a> {
    pub catalogue: &'a str,
    pub roles: &'a [(String, String)],
    pub gazetteer: &'a Value,
    pub digest: &'a Value,
    pub prompt: &'a str,
}

const REPLY_SCHEMA: &str = r#"{
  "detected_language": "ISO 639-1 code of the reply",
  "translation_en": "the reply translated to English (verbatim meaning)",
  "actions": [ /* zero or more action objects exactly as documented */ ],
  "messages": [ {"to": "role_id", "text": "message in English"} ],
  "clarification": null or "short question to the player, in the player's language",
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
            "You translate civil-protection exercise messages into {}. Keep place names, facility names, zone codes (like F4), road ids, numbers and times unchanged. Keep the tone of an operational message. Output only the translation, nothing else.",
            language_name(target_lang)
        );
        Ok(self.complete("translate", &system, text, false).await?.trim().to_string())
    }

    /// Player free text → candidate actions, messages, or a clarification question.
    pub async fn parse_reply(&self, i: ReplyInput<'_>) -> Result<ParsedReply> {
        let system = format!(
            "You are the exercise assistant of a Civil Protection table-top exercise (TTX). \
You turn a player's free-text reply into structured actions for the simulator. Rules:\n\
- Use ONLY the action types documented below, with exactly the documented JSON shape.\n\
- Use ONLY place names, facility names, road ids and zone codes from the gazetteer, or zone codes like F4. Never invent places, resources or facts.\n\
- The player decides: add an action only if the player clearly ordered it. Do not add your own ideas.\n\
- If an order is ambiguous or misses essential information (which zone/place, which road), do NOT guess: leave it out and ask one short clarification question in the player's language.\n\
- Information shared or requested with other roles goes into \"messages\" (role ids: {}).\n\
- Do not check resource availability yourself; the simulator validates.\n\
Answer with ONE JSON object:\n{REPLY_SCHEMA}\n\nAllowed actions for this player:\n{}",
            i.other_roles.iter().map(|(id, name)| format!("{id} = {name}")).collect::<Vec<_>>().join(", "),
            i.catalogue
        );
        let user = format!(
            "PLAYER ROLE: {} ({}) — {}\nPLAYER LANGUAGE: {}\n\nGAZETTEER:\n{}\n\nCURRENT SITUATION:\n{}\n\nRECENT INJECTS RECEIVED BY THE PLAYER:\n{}\n\nPLAYER REPLY:\n\"\"\"{}\"\"\"",
            i.role_name,
            i.role_id,
            i.role_description,
            language_name(i.language),
            i.gazetteer,
            i.digest,
            if i.recent_injects.is_empty() { "(none)".to_string() } else { i.recent_injects.join("\n---\n") },
            i.reply
        );
        ParsedReply::from_value(self.complete_json("parse_reply", &system, &user).await?)
    }

    /// Master natural-language instruction → commands and/or inject drafts.
    pub async fn master_prompt(&self, i: MasterInput<'_>) -> Result<ParsedReply> {
        let system = format!(
            "You assist the Exercise Director (Game Master) of a Civil Protection table-top exercise. \
The Director is omnipotent within the simulator: every action below is allowed. Convert the instruction into simulator actions \
and/or draft injects (messages to players, in English, from a simulated sender such as \"112 Dispatch\" or \"ERCC\"). \
Use only names from the gazetteer or zone codes. If the instruction is unclear, return no actions and a clarification question.\n\
Answer with ONE JSON object:\n{{\"actions\": [...], \"injects\": [{{\"recipients\": [\"role_id\" or \"all\"], \"title\": \"...\", \"sender\": \"...\", \"text\": \"...\"}}], \"clarification\": null, \"summary\": \"one sentence\", \"translation_en\": \"the instruction in English\"}}\n\n\
Actions:\n{}\nRoles: {}",
            i.catalogue,
            i.roles.iter().map(|(id, name)| format!("{id} = {name}")).collect::<Vec<_>>().join(", ")
        );
        let user = format!("GAZETTEER:\n{}\n\nCURRENT SITUATION:\n{}\n\nDIRECTOR INSTRUCTION:\n\"\"\"{}\"\"\"", i.gazetteer, i.digest, i.prompt);
        ParsedReply::from_value(self.complete_json("master_prompt", &system, &user).await?)
    }

    /// EXCON's answer to a player once the Master has decided on a proposal.
    pub async fn feedback(&self, role_name: &str, lang: &str, decision_en: &str) -> Result<String> {
        let system = format!(
            "You write as EXCON (exercise control) of a Civil Protection table-top exercise. Write a brief operational reply (2-4 sentences) to the {role_name} in {}, \
reporting the outcome of their orders exactly as given in the facts. Do not add facts, advice or judgement. Output only the message.",
            language_name(lang)
        );
        Ok(self.complete("feedback", &system, decision_en, false).await?.trim().to_string())
    }

    /// End-of-turn situation update for one role.
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
            recent_injects: &[],
            reply,
        }
    }

    #[tokio::test]
    async fn parse_reply_maps_model_json() {
        let llm = Llm::mock(|task, system, user| {
            assert_eq!(task, "parse_reply");
            assert!(system.contains("EvacuateZone"));
            assert!(user.contains("evacuate F4"));
            Ok(r#"{"detected_language":"it","translation_en":"evacuate F4","actions":[{"type":"EvacuateZone","zone":"F4"}],"messages":[],"clarification":"","summary":"Evacuate F4"}"#.into())
        });
        let g = json!({});
        let p = llm.parse_reply(input("evacuate F4", &g)).await.unwrap();
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
