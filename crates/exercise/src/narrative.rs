//! Explicit initiatives adjudicated by EXCON, independent of world physics.
use crate::{Exercise, MsgKind, Phase, ProposalStatus, MASTER};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;
use sim::Command;

/// A world change EXCON made together with a narrative outcome: an ordinary
/// EXCON intervention (`Exercise::direct_command_logged`) linked by log entry.
#[derive(Debug, Clone, Serialize)]
pub struct OutcomeIntervention {
    /// `seq` of the `master_command` entry in the exercise log.
    pub log_seq: u64,
    pub description: String,
    pub command: Command,
}

/// What EXCON records on an approved response.
#[derive(Debug, Clone, Default)]
pub struct OutcomeInput {
    /// Original text (may be empty when only `commands` are given).
    pub text: String,
    pub text_en: String,
    pub player_text: String,
    pub translation_failed: bool,
    /// Who the outcome is spoken as (default: the inject's sender, then the scenario default).
    pub persona: Option<String>,
    /// Applied now as EXCON interventions, in order.
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NarrativeOutcome {
    /// Persona the outcome is spoken as; `None` is the scenario default.
    pub persona: Option<String>,
    /// Interventions applied together with the outcome.
    pub interventions: Vec<OutcomeIntervention>,
    /// EXCON's account, not an observed or simulated physical result.
    pub text: String,
    pub text_en: String,
    pub player_text: String,
    pub turn: u32,
    pub t_s: i64,
    pub message: u64,
}

impl Exercise {
    pub fn validate_narrative(&self, role: &str, text: &str, reply_to: u64) -> Result<()> {
        self.validate_chat(role, MASTER, text, true)?;
        let parent = self.validate_action_reference(role, reply_to)?;
        if parent.kind != MsgKind::Inject || parent.to != role {
            bail!("a narrative initiative must answer an inject delivered to your role");
        }
        Ok(())
    }

    /// No parsing or invented commands; translation is supplied by the controller.
    pub fn player_narrative(
        &mut self,
        role: &str,
        text: &str,
        reply_to: u64,
        text_en: &str,
        translation_failed: bool,
    ) -> Result<u64> {
        self.validate_narrative(role, text, reply_to)?;
        self.player_response(
            role,
            crate::response::PlayerResponse {
                narrative: Some(text.into()),
                narrative_en: text_en.into(),
                translation_failed,
                reply_to: Some(reply_to),
                ..Default::default()
            },
        )
    }

    pub fn validate_narrative_outcome(&self, pid: u64, text: &str) -> Result<String> {
        self.validate_outcome(pid, text, 0)
    }

    /// The narrative and the actions are each optional, but not both missing.
    pub fn validate_outcome(&self, pid: u64, text: &str, actions: usize) -> Result<String> {
        if self.phase != Phase::Orders {
            bail!("narrative outcomes require an open turn");
        }
        if text.chars().count() > 8000 {
            bail!("narrative outcome requires at most 8000 characters");
        }
        if text.trim().is_empty() && actions == 0 {
            bail!("an outcome needs a narrative or at least one action");
        }
        if actions > 20 {
            bail!("provide at most 20 actions");
        }
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        // A response that also carried actions moves on to Applied/Failed once
        // they run; its narrative part can still be answered.
        let decided = p.status == ProposalStatus::Approved
            || (p.carries_actions
                && matches!(p.status, ProposalStatus::Applied | ProposalStatus::Failed));
        if !p.narrative || !decided || !p.decision.as_ref().is_some_and(|d| d.approve)
        {
            bail!("only an approved narrative initiative can receive an outcome");
        }
        Ok(p.source.label().into())
    }

    pub fn record_narrative_outcome(
        &mut self,
        pid: u64,
        text: &str,
        text_en: &str,
        player_text: &str,
        translation_failed: bool,
    ) -> Result<u64> {
        self.record_outcome(
            pid,
            OutcomeInput {
                text: text.into(),
                text_en: text_en.into(),
                player_text: player_text.into(),
                translation_failed,
                ..Default::default()
            },
        )
    }

    /// Record EXCON's outcome of an approved response: an optional narrative
    /// (spoken as a persona) and optional interventions, all or nothing.
    pub fn record_outcome(&mut self, pid: u64, i: OutcomeInput) -> Result<u64> {
        let role = self.validate_outcome(pid, &i.text, i.commands.len())?;
        let narrative = !i.text.trim().is_empty();
        if narrative
            && (i.player_text.trim().is_empty()
                || i.player_text.chars().count() > 16000
                || i.text_en.trim().is_empty()
                || i.text_en.chars().count() > 16000)
        {
            bail!("invalid translated narrative outcome");
        }
        // Same checks as the Controllo tools, on a copy, so a refused action
        // leaves the world untouched.
        if !i.commands.is_empty() {
            let preview = self.preview_commands(&i.commands)?;
            for r in preview["results"].as_array().into_iter().flatten() {
                if let Some(reason) = r["result"].get("Err") {
                    bail!("action refused: {} ({reason})", r["command"]["type"]);
                }
            }
        }
        let persona = self.persona_for(
            i.persona.as_deref(),
            self.proposal(pid).and_then(|p| p.inject_id),
        );
        let mut interventions = Vec::new();
        for cmd in &i.commands {
            let (_, log_seq) = self.direct_command_logged(
                cmd,
                json!({"origin":"narrative_outcome","proposal":pid,"role":role}),
            )?;
            interventions.push(OutcomeIntervention {
                log_seq,
                description: cmd.describe(),
                command: cmd.clone(),
            });
        }
        let effects = interventions
            .iter()
            .map(|x| x.description.clone())
            .collect::<Vec<_>>()
            .join("; ");
        let (text, text_en, player_text) = if narrative {
            (i.text.trim().to_string(), i.text_en.clone(), i.player_text.clone())
        } else {
            (effects.clone(), effects.clone(), effects)
        };
        let p = self.proposal(pid).unwrap();
        let thread = p
            .inject_id
            .map(|root| (root, p.reply_to, vec![role.clone()]));
        let reply_to = p.reply_to;
        let mid = self.add_message_in_thread(
            MASTER,
            &role,
            MsgKind::Feedback,
            Some("Narrative outcome".into()),
            &player_text,
            &text_en,
            Some(pid),
            thread,
            reply_to,
        );
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == mid) {
            m.translation_failed = i.translation_failed;
            m.persona = persona.clone();
        }
        let outcome = NarrativeOutcome {
            persona,
            interventions,
            text,
            text_en,
            player_text,
            turn: self.turn,
            t_s: self.world.time_s(),
            message: mid,
        };
        self.proposal_mut(pid)?
            .narrative_outcomes
            .push(outcome.clone());

        self.record(
            "narrative_outcome",
            MASTER,
            json!({"proposal":pid,"role":role,"outcome":outcome,"simulated":false,"interventions":outcome.interventions.iter().map(|x|x.log_seq).collect::<Vec<_>>()}),
        );
        Ok(mid)
    }

    pub(crate) fn rollback_narrative_outcomes(&mut self, turn: u32) {
        let mut removed = std::collections::BTreeSet::new();
        for p in &mut self.proposals {
            p.narrative_outcomes.retain(|o| {
                if o.turn >= turn {
                    removed.insert(o.message);
                    false
                } else {
                    true
                }
            });
        }
        self.messages.retain(|m| !removed.contains(&m.id));
    }
}
