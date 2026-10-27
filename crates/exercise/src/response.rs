//! A player's response: optional narrative text plus optional simulator actions.
//!
//! One submission, one proposal. The narrative is a statement EXCON may answer
//! with a narrative outcome; the commands still need EXCON approval and
//! feasibility checks and are the only part that can touch the world. At least
//! one of the two is required. A narrative needs an inject to answer.
use crate::{DecisionEvidence, Exercise, MsgKind, ParsedInput, Phase, ProposalStatus, MASTER};
use anyhow::{bail, Result};
use serde_json::json;
use sim::Command;

/// What the player sent. `narrative_en` is the English working copy of the
/// narrative, supplied by the caller (translation happens outside the model).
#[derive(Debug, Clone, Default)]
pub struct PlayerResponse {
    pub narrative: Option<String>,
    pub narrative_en: String,
    pub translation_failed: bool,
    /// Short summary of the actions; defaults to their description.
    pub text: String,
    pub commands: Vec<Command>,
    pub reply_to: Option<u64>,
    /// Linked conversation the response comes out of (needed to answer an inject
    /// the player never received).
    pub conversation: Option<u64>,
}

impl PlayerResponse {
    fn narrative_text(&self) -> Option<&str> {
        self.narrative.as_deref().filter(|t| !t.trim().is_empty())
    }
}

impl Exercise {
    /// Every check that can be made before translating anything.
    pub fn validate_player_response(&self, role: &str, r: &PlayerResponse) -> Result<()> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for replies");
        }
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        let narrative = r.narrative_text();
        if narrative.is_none() && r.commands.is_empty() {
            bail!("a response needs a narrative or at least one action");
        }
        if r.commands.len() > 32 {
            bail!("provide at most 32 actions");
        }
        let allowed = &self.world.scn.role(role).unwrap().allowed_actions;
        if r.commands
            .iter()
            .any(|c| !allowed.iter().any(|kind| kind == c.kind()))
        {
            bail!("action outside this role's authority");
        }
        if let Some(text) = narrative {
            self.validate_chat(role, MASTER, text, true)?;
        } else if !r.text.trim().is_empty() {
            self.validate_chat(role, MASTER, &r.text, true)?;
        }
        let Some(reply_to) = r.reply_to else {
            if narrative.is_some() || r.conversation.is_some() {
                bail!("a narrative must answer an inject delivered to your role");
            }
            return Ok(());
        };
        let parent = self.validate_action_reference(role, reply_to)?;
        let foreign_inject = parent.kind == MsgKind::Inject && parent.to != role;
        if narrative.is_some() && parent.kind != MsgKind::Inject {
            bail!("a narrative initiative must answer an inject delivered to your role");
        }
        match r.conversation {
            Some(cid) => {
                let linked = self
                    .linked_conversation(cid)
                    .is_some_and(|c| c.has(role) && c.delivery == reply_to);
                if !linked {
                    bail!("the conversation is not linked to this inject");
                }
            }
            None if foreign_inject => {
                bail!(
                    "you did not receive this inject: answer it from a conversation linked to it"
                );
            }
            None => {}
        }
        if self.clarification_target(role, reply_to).is_some() {
            bail!("this answers an open question: send it as a text answer, or create a new action without replying to the question");
        }
        Ok(())
    }

    pub fn player_response(&mut self, role: &str, r: PlayerResponse) -> Result<u64> {
        self.validate_player_response(role, &r)?;
        let narrative = r.narrative_text().map(str::to_string);
        if narrative.is_some()
            && (r.narrative_en.trim().is_empty() || r.narrative_en.chars().count() > 16000)
        {
            bail!("narrative translation must contain 1–16000 characters");
        }
        let actions_text = if !r.text.trim().is_empty() {
            r.text.clone()
        } else {
            r.commands
                .iter()
                .map(Command::describe)
                .collect::<Vec<_>>()
                .join("; ")
        };
        let original = narrative.clone().unwrap_or_else(|| actions_text.clone());
        let english = if narrative.is_some() {
            r.narrative_en.clone()
        } else {
            actions_text.clone()
        };
        let (inject_id, thread) = match r.reply_to {
            Some(parent) => (
                self.messages
                    .iter()
                    .find(|m| m.id == parent)
                    .and_then(|m| m.inject_id),
                self.thread_evidence(role, parent)?,
            ),
            None => (None, Vec::new()),
        };
        let evidence = match (r.reply_to, narrative.is_some(), r.commands.is_empty()) {
            (Some(_), false, _) => Some(DecisionEvidence::judgment(
                "Structured operational decision",
            )),
            (Some(_), true, false) => Some(DecisionEvidence::judgment(
                "Narrative response with structured operational decision",
            )),
            _ => None,
        };
        let pid = self.open_chat_proposal(role, &original, inject_id, r.reply_to, evidence)?;
        {
            let p = self.proposal_mut(pid)?;
            p.thread = thread;
            p.via_conversation = r.conversation;
        }
        let mid = r.reply_to.map(|parent| {
            let mid = self.add_message_in_thread(
                role,
                MASTER,
                MsgKind::Reply,
                None,
                &original,
                &english,
                Some(pid),
                inject_id.map(|root| (root, Some(parent), vec![MASTER.into()])),
                Some(parent),
            );
            if narrative.is_some() {
                let language = self.players[role].language.clone();
                if let Some(m) = self.messages.iter_mut().find(|m| m.id == mid) {
                    m.original = Some(original.clone());
                    m.language_src = Some(language);
                    m.translation_failed = r.translation_failed;
                }
            }
            mid
        });
        if r.commands.is_empty() {
            let p = self.proposal_mut(pid)?;
            p.narrative = true;
            p.translation_en = english.clone();
            p.summary = english.clone();
            self.transition(
                pid,
                ProposalStatus::Pending,
                role,
                "Narrative initiative submitted for EXCON review",
            )?;
        } else {
            self.attach_parse(
                pid,
                ParsedInput {
                    language: self.language_of(role),
                    translation_en: english.clone(),
                    actions: r
                        .commands
                        .iter()
                        .map(|c| serde_json::to_value(c).expect("command serialization"))
                        .collect(),
                    summary: english.clone(),
                    ..Default::default()
                },
            )?;
            if narrative.is_some() {
                let p = self.proposal_mut(pid)?;
                p.narrative = true;
                p.carries_actions = true;
            }
        }
        if narrative.is_some() {
            let note = if r.commands.is_empty() {
                original.clone()
            } else {
                format!("{original}\n[actions] {actions_text}")
            };
            self.converse(role, "user", note);
            self.record(
                "narrative_submitted",
                role,
                json!({"proposal":pid,"message":mid,"inject_id":inject_id,"translation_failed":r.translation_failed,"actions":r.commands.len(),"conversation":r.conversation}),
            );
        }
        if let Some(cid) = r.conversation {
            self.record(
                "conversation_response",
                role,
                json!({"proposal":pid,"conversation":cid,"inject_id":inject_id}),
            );
        }
        Ok(pid)
    }
}
