//! Private player preparation. Drafts do not create messages, proposals or world changes.
use crate::{ChatTurn, Exercise, ParsedInput, Phase, MASTER};
use anyhow::{bail, Result};
use serde::Serialize;
use sim::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    Interpreting,
    NeedsClarification,
    Ready,
    Unsupported,
    AiUnavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerDraft {
    pub id: u64,
    pub revision: u64,
    pub to: String,
    pub reply_to: Option<u64>,
    pub status: DraftStatus,
    pub message: String,
    pub question: Option<String>,
    pub summary: String,
    pub actions: Vec<Command>,
    pub action_descriptions: Vec<String>,
    pub turn: u32,
    /// Why interpretation failed (set with `AiUnavailable`), so the player and EXCON can see it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(skip)]
    pub role: String,
    #[serde(skip)]
    pub clock_revision: u64,
    #[serde(skip)]
    pub transcript: Vec<ChatTurn>,
    #[serde(skip)]
    parsed: Option<ParsedInput>,
}

impl Exercise {
    pub fn player_drafts(&self, role: &str) -> Vec<&PlayerDraft> {
        self.player_drafts
            .values()
            .filter(|draft| {
                draft.role == role
                    && draft.turn == self.turn
                    && draft.clock_revision == self.clock_control.revision
            })
            .collect()
    }

    pub fn private_draft(&self, role: &str, id: u64, revision: u64) -> Result<PlayerDraft> {
        let draft = self
            .player_drafts
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("draft expired; prepare the message again"))?;
        if draft.role != role {
            bail!("draft belongs to a different role");
        }
        if draft.revision != revision {
            bail!("draft changed; review the current version");
        }
        if draft.turn != self.turn
            || draft.clock_revision != self.clock_control.revision
            || self.phase != Phase::Orders
        {
            bail!("exercise changed; prepare the message again");
        }
        Ok(draft.clone())
    }

    pub fn begin_player_draft(
        &mut self,
        role: &str,
        to: &str,
        text: &str,
        previous: Option<(u64, u64)>,
        reply_to: Option<u64>,
    ) -> Result<PlayerDraft> {
        self.validate_chat(role, to, text, true)?;
        if to != MASTER {
            bail!("operational decisions must be addressed to EXCON");
        }
        if let Some(parent) = reply_to {
            self.validate_action_reference(role, parent)?;
            if self.clarification_target(role, parent).is_some() {
                bail!("this answers an open EXCON question: send it as a text answer, or create a new action without replying to the question");
            }
        }
        let mut draft = if let Some((id, revision)) = previous {
            let mut draft = self.private_draft(role, id, revision)?;
            if draft.to != to
                || draft.reply_to != reply_to
                || draft.status != DraftStatus::NeedsClarification
            {
                bail!("this draft is not awaiting your answer");
            }
            if draft.transcript.len() >= 16 {
                bail!("start a new draft with the clarified message");
            }
            if let Some(question) = &draft.question {
                draft.transcript.push(ChatTurn {
                    role: "assistant".into(),
                    content: question.clone(),
                });
            }
            draft.revision += 1;
            draft
        } else {
            // Bound memory per role and conversation. Drafts are temporary, not exercise state.
            self.player_drafts
                .retain(|_, draft| draft.role != role || draft.to != to);
            PlayerDraft {
                id: self.id(),
                revision: 1,
                role: role.into(),
                to: to.into(),
                reply_to,
                status: DraftStatus::Interpreting,
                message: text.into(),
                question: None,
                summary: String::new(),
                actions: Vec::new(),
                action_descriptions: Vec::new(),
                turn: self.turn,
                failure: None,
                clock_revision: self.clock_control.revision,
                transcript: Vec::new(),
                parsed: None,
            }
        };
        if draft
            .transcript
            .iter()
            .filter(|turn| turn.role == "user")
            .map(|turn| turn.content.chars().count())
            .sum::<usize>()
            + text.chars().count()
            > 8000
        {
            bail!("draft is too long; start a new concise message");
        }
        draft.transcript.push(ChatTurn {
            role: "user".into(),
            content: text.into(),
        });
        draft.status = DraftStatus::Interpreting;
        draft.failure = None;
        draft.question = None;
        draft.parsed = None;
        self.player_drafts.insert(draft.id, draft.clone());
        Ok(draft)
    }

    pub fn finish_player_draft(
        &mut self,
        role: &str,
        id: u64,
        revision: u64,
        parsed: ParsedInput,
        message: String,
    ) -> Result<PlayerDraft> {
        let mut draft = self.private_draft(role, id, revision)?;
        if draft.status != DraftStatus::Interpreting {
            bail!("draft is not awaiting interpretation");
        }
        if message.trim().is_empty() || message.chars().count() > 8000 {
            bail!("invalid prepared message length");
        }
        if parsed.clarification.is_none()
            && self.language_of(role) != "en"
            && parsed.translation_en.trim().is_empty()
        {
            bail!("prepared decision is missing its English working copy");
        }
        let actions = parsed
            .actions
            .iter()
            .cloned()
            .map(serde_json::from_value::<Command>)
            .collect::<Result<Vec<_>, _>>();
        let actions = match actions {
            Ok(actions) => actions,
            Err(_) => {
                draft.status = DraftStatus::Unsupported;
                draft.message = join_user_turns(&draft.transcript);
                draft.failure = Some(unsupported_message(&self.language_of(role)).into());
                self.player_drafts.insert(id, draft.clone());
                return Ok(draft);
            }
        };
        let allowed = &self.world.scn.role(role).unwrap().allowed_actions;
        if actions
            .iter()
            .any(|action| !allowed.iter().any(|kind| kind == action.kind()))
        {
            draft.status = DraftStatus::Unsupported;
            draft.message = join_user_turns(&draft.transcript);
            draft.failure = Some(unsupported_message(&self.language_of(role)).into());
            self.player_drafts.insert(id, draft.clone());
            return Ok(draft);
        }
        draft.actions = actions;
        draft.action_descriptions = draft.actions.iter().map(Command::describe).collect();
        draft.message = message;
        draft.summary = parsed.summary.clone();
        draft.question = parsed
            .clarification
            .as_ref()
            .filter(|q| !q.trim().is_empty())
            .cloned();
        draft.status = if draft.question.is_some() {
            DraftStatus::NeedsClarification
        } else if draft.actions.is_empty() {
            draft.failure = Some(unsupported_message(&self.language_of(role)).into());
            DraftStatus::Unsupported
        } else {
            DraftStatus::Ready
        };
        draft.parsed = Some(parsed);
        self.player_drafts.insert(id, draft.clone());
        Ok(draft)
    }

    pub fn player_draft_failed(
        &mut self,
        role: &str,
        id: u64,
        revision: u64,
        reason: &str,
    ) -> Result<PlayerDraft> {
        let mut draft = self.private_draft(role, id, revision)?;
        draft.status = DraftStatus::AiUnavailable;
        draft.failure = Some(reason.chars().take(300).collect());
        draft.question = None;
        draft.parsed = None;
        // Keep all the player's words if interpretation failed after a clarification.
        draft.message = join_user_turns(&draft.transcript);
        self.player_drafts.insert(id, draft.clone());
        Ok(draft)
    }

    pub fn draft_english(&self, role: &str, id: u64, revision: u64) -> Result<String> {
        let draft = self.private_draft(role, id, revision)?;
        Ok(draft
            .parsed
            .as_ref()
            .map(|parsed| parsed.translation_en.as_str())
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(&draft.message)
            .to_string())
    }

    /// Commit exactly the privately reviewed revision; EXCON approval remains separate.
    pub fn submit_player_draft(
        &mut self,
        role: &str,
        id: u64,
        revision: u64,
        english: &str,
        translated: &str,
        translation_failed: bool,
        _manual: bool,
    ) -> Result<(u64, Option<u64>)> {
        let draft = self.private_draft(role, id, revision)?;
        if draft.status != DraftStatus::Ready || draft.actions.is_empty() {
            bail!("No simulator action is ready; clarify the decision before sending it to EXCON");
        }
        let interpreted = true;
        let result = if let Some(parent) = draft.reply_to {
            let pid = self.player_reply(
                role,
                &draft.message,
                parent,
                crate::DecisionEvidence::judgment("Player reviewed response to received inject"),
            )?;
            let message = self.messages.last_mut().expect("recorded player reply");
            message.text = translated.into();
            message.text_en = english.into();
            message.original = Some(draft.message.clone());
            message.translation_failed = translation_failed;
            let mid = message.id;
            let recorded = message.clone();
            self.record("reply_source", role, serde_json::json!(recorded));
            (mid, Some(pid))
        } else {
            self.chat(
                role,
                &draft.to,
                &draft.message,
                english,
                translated,
                translation_failed,
                interpreted,
            )?
        };
        if let Some(pid) = result.1 {
            self.attach_parse(pid, draft.parsed.clone().unwrap())?;
        }
        // Keep rehearsal and clarification private even after submission.
        // Only the confirmed message and its final interpretation enter shared evidence.
        if interpreted {
            self.converse(
                role,
                "user",
                format!(
                    "CONFIRMED MESSAGE to {} (reply to {:?}): {}",
                    draft.to, draft.reply_to, draft.message
                ),
            );
            self.converse(
                role,
                "assistant",
                serde_json::json!({"actions":draft.actions,"summary":draft.summary}).to_string(),
            );
        }
        self.record("player_draft_submitted", role, serde_json::json!({"draft":id,"revision":revision,"message":result.0,"proposal":result.1,"manual":false,"to":draft.to,"reply_to":draft.reply_to,"reviewer":MASTER}));
        self.player_drafts.remove(&id);
        Ok(result)
    }
}

/// Join the player's request and clarification answers readably: a sentence
/// break is added only when the previous part does not already end in punctuation.
pub fn join_user_turns(transcript: &[ChatTurn]) -> String {
    let mut out = String::new();
    for part in transcript
        .iter()
        .filter(|turn| turn.role == "user")
        .map(|turn| turn.content.trim())
        .filter(|text| !text.is_empty())
    {
        if !out.is_empty() {
            if !out.ends_with(['.', '!', '?', ';', ':', ',']) {
                out.push('.');
            }
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn unsupported_message(language: &str) -> &'static str {
    match language {
        "it" => "Questa richiesta non corrisponde ad azioni supportate dal simulatore.",
        "fr" => "Cette demande ne correspond à aucune action prise en charge par le simulateur.",
        _ => "This request does not match a supported simulator action.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: &str, content: &str) -> ChatTurn {
        ChatTurn {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn joins_request_and_answer_with_sentence_break() {
        let t = vec![
            turn("user", "Close the road"),
            turn("assistant", "Which road?"),
            turn("user", "The main road south"),
        ];
        assert_eq!(join_user_turns(&t), "Close the road. The main road south");
        let t = vec![
            turn("user", "Close the road."),
            turn("user", "The main road"),
        ];
        assert_eq!(join_user_turns(&t), "Close the road. The main road");
    }
}
