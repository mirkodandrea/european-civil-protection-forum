//! Explicit initiatives adjudicated by EXCON, independent of world physics.
use crate::{Exercise, MsgKind, Phase, ProposalStatus, MASTER};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
pub struct NarrativeOutcome {
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
        let parent = self.validate_action_reference(role, reply_to)?;
        let inject_id = parent.inject_id;
        let thread = self.thread_evidence(role, reply_to)?;
        if text_en.trim().is_empty() || text_en.chars().count() > 16000 {
            bail!("narrative translation must contain 1–16000 characters");
        }
        let pid = self.open_chat_proposal(role, text, inject_id, Some(reply_to), None)?;
        let p = self.proposal_mut(pid)?;
        p.narrative = true;
        p.thread = thread;
        p.translation_en = text_en.into();
        p.summary = text_en.into();
        self.transition(
            pid,
            ProposalStatus::Pending,
            role,
            "Narrative initiative submitted for EXCON review",
        )?;
        let mid = self.add_message_in_thread(
            role,
            MASTER,
            MsgKind::Reply,
            None,
            text,
            text_en,
            Some(pid),
            inject_id.map(|root| (root, Some(reply_to), vec![MASTER.into()])),
            Some(reply_to),
        );
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == mid) {
            m.original = Some(text.into());
            m.language_src = Some(self.players[role].language.clone());
            m.translation_failed = translation_failed;
        }
        self.converse(role, "user", text);
        self.record("narrative_submitted", role, json!({"proposal":pid,"message":mid,"inject_id":inject_id,"translation_failed":translation_failed}));
        Ok(pid)
    }

    pub fn validate_narrative_outcome(&self, pid: u64, text: &str) -> Result<String> {
        if self.phase != Phase::Orders {
            bail!("narrative outcomes require an open turn");
        }
        if text.trim().is_empty() || text.chars().count() > 8000 {
            bail!("narrative outcome requires 1–8000 characters");
        }
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        if !p.narrative
            || p.status != ProposalStatus::Approved
            || !p.decision.as_ref().is_some_and(|d| d.approve)
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
        let role = self.validate_narrative_outcome(pid, text)?;
        if player_text.trim().is_empty()
            || player_text.chars().count() > 16000
            || text_en.trim().is_empty()
            || text_en.chars().count() > 16000
        {
            bail!("invalid translated narrative outcome");
        }
        let p = self.proposal(pid).unwrap();
        let thread = p
            .inject_id
            .map(|root| (root, p.reply_to, vec![role.clone()]));
        let reply_to = p.reply_to;
        let mid = self.add_message_in_thread(
            MASTER,
            &role,
            MsgKind::Feedback,
            Some("EXCON narrative outcome".into()),
            player_text,
            text_en,
            Some(pid),
            thread,
            reply_to,
        );
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == mid) {
            m.translation_failed = translation_failed;
        }
        let outcome = NarrativeOutcome {
            text: text.trim().into(),
            text_en: text_en.into(),
            player_text: player_text.into(),
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
            json!({"proposal":pid,"role":role,"outcome":outcome,"simulated":false}),
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
