//! Private, immutable end-of-turn evidence. No global digest or EXCON context.
use crate::{Exercise, MsgKind};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct TurnSummary {
    pub id: u64,
    pub role: String,
    pub turn: u32,
    pub clock: String,
    pub text: String,
    pub status: String,
    #[serde(skip)]
    evidence: Value,
    #[serde(skip)]
    language: String,
}

impl Exercise {
    pub fn player_turn_summaries(&self, role: &str) -> Vec<&TurnSummary> {
        self.turn_summaries
            .iter()
            .filter(|s| s.role == role)
            .collect()
    }

    pub(crate) fn prepare_turn_summaries(&mut self, turn: u32) {
        for role in self.role_ids() {
            // Use the exact role-visible message body, never translations for EXCON,
            // recipient lists, hidden interpretations or other participants' threads.
            let messages = self.inbox(&role).into_iter()
                .filter(|m| m.turn == turn && !matches!(m.kind, MsgKind::Situation))
                .map(|m| json!({"from":m.from,"to":m.to,"kind":m.kind,"clock":m.clock,
                    "title":m.title,"text":if m.from == role {m.original.as_ref().unwrap_or(&m.text)} else {&m.text},"reply_to":m.reply_to}))
                .collect::<Vec<_>>();
            let actions = self.proposals.iter().filter(|p| p.source.label() == role && p.turn == turn)
                .map(|p| json!({"text":p.original,"status":p.status,"reply_to":p.reply_to,
                    "decision":p.decision.as_ref().map(|d|json!({"approve":d.approve,"note":d.note})),"executions":p.executions}))
                .collect::<Vec<_>>();
            let evidence = json!({"role":role,"turn":turn,"messages":messages,"own_actions":actions,
                "own_resources":self.resource_accounting(Some(&role))[&role]});
            let language = self.players[&role].language.clone();
            let summary = TurnSummary {
                id: self.id(),
                role,
                turn,
                clock: self.world.clock(),
                text: String::new(),
                status: "pending".into(),
                evidence,
                language,
            };
            self.turn_summaries.push(summary);
        }
    }

    /// Claim once; the id also guards against late AI responses after rollback.
    pub fn claim_turn_summaries(&mut self) -> Vec<(u64, String, Value)> {
        self.turn_summaries
            .iter_mut()
            .filter(|s| s.status == "pending")
            .map(|s| {
                s.status = "generating".into();
                (s.id, s.language.clone(), s.evidence.clone())
            })
            .collect()
    }

    pub fn complete_turn_summary(&mut self, id: u64, result: Option<String>) {
        if let Some(summary) = self
            .turn_summaries
            .iter_mut()
            .find(|s| s.id == id && s.status == "generating")
        {
            match result.filter(|s| !s.trim().is_empty()) {
                Some(text) => {
                    summary.text = text;
                    summary.status = "ready".into();
                }
                None => {
                    summary.status = "unavailable".into();
                }
            }
            self.touch();
        }
    }
}
