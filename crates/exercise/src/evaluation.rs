//! Read-only, evidence-derived evaluator views. No simulation mutation or AI.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use crate::{BasisKind, DecisionEvidence, Exercise, MsgKind, Phase, ProposalStatus};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct EvidenceFilter {
    pub role: Option<String>,
    pub inject: Option<u64>,
    pub turn: Option<u32>,
    pub phase: Option<Phase>,
    pub exercise_phase: Option<crate::ExercisePhase>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

impl Exercise {
    pub fn set_reference_materials(&mut self, references: Vec<DecisionEvidence>) -> Result<()> {
        if references.len() > 100 { bail!("at most 100 reference plans/procedures are allowed"); }
        for reference in &references {
            reference.validate()?;
            if matches!(reference.basis.kind, BasisKind::OperationalJudgment) { bail!("reference library requires a plan or procedure"); }
        }
        self.record("reference_materials_updated", crate::MASTER, json!({"before":self.reference_materials,"after":references}));
        self.reference_materials = references;
        Ok(())
    }

    pub fn evaluator_view(&self, f: &EvidenceFilter) -> Result<Value> {
        if let Some(role) = &f.role { if !self.players.contains_key(role) && role != crate::MASTER { bail!("unknown role {role}"); } }
        if let Some(status) = &f.status {
            if !["parsing","pending","needs_clarification","approved","rejected","applied","failed","rolled_back"].contains(&status.as_str()) { bail!("unknown proposal status"); }
        }
        // Proposal filters select chains by declaration. Their complete later history
        // stays visible even if a decision/execution occurred in another turn/phase.
        let proposals = self.proposals.iter().filter(|p| {
            let declaration_phase = self.log.iter().find(|l| (l.kind == "proposal_declared" && l.data["id"] == p.id) || (l.kind == "master_prompt" && l.data["proposal"] == p.id)).map(|l| l.phase);
            f.role.as_ref().is_none_or(|r| p.source.label() == r) && f.inject.is_none_or(|id| p.inject_id == Some(id))
                && f.turn.is_none_or(|t| p.turn == t) && f.phase.is_none_or(|phase| declaration_phase == Some(phase))
                && f.exercise_phase.is_none_or(|phase| self.log.iter().any(|l| ((l.kind=="proposal_declared" && l.data["id"]==p.id) || (l.kind=="master_prompt" && l.data["proposal"]==p.id)) && l.exercise_phase==phase))
                && f.status.as_ref().is_none_or(|status| serde_json::to_value(p.status).unwrap() == status.as_str())
        }).collect::<Vec<_>>();
        let pids = proposals.iter().map(|p| p.id).collect::<BTreeSet<_>>();
        let roots = proposals.iter().filter_map(|p| p.inject_id).chain(f.inject).collect::<BTreeSet<_>>();
        let messages = self.messages.iter().filter(|m| {
            let in_chain = m.proposal.is_some_and(|id| pids.contains(&id)) || m.inject_id.is_some_and(|id| roots.contains(&id));
            let direct = f.role.as_ref().is_none_or(|r| m.to == *r || m.from == *r) && f.inject.is_none_or(|id| m.inject_id == Some(id))
                && f.turn.is_none_or(|t| m.turn == t) && f.phase.is_none_or(|phase| self.log.iter().any(|l| l.kind == "message" && l.data["id"] == m.id && l.phase == phase));
            // Role filter also constrains a shared multi-recipient inject thread.
            (in_chain || (direct && f.status.is_none())) && f.role.as_ref().is_none_or(|r| m.to == *r || m.from == *r)
        }).collect::<Vec<_>>();
        let mids = messages.iter().map(|m| m.id).collect::<BTreeSet<_>>();
        let log = self.log.iter().filter(|l| {
            let proposal_id = l.data["proposal"].as_u64().or_else(|| if l.kind == "proposal_declared" { l.data["id"].as_u64() } else { None });
            let message_id = if l.kind == "message" { l.data["id"].as_u64() } else { l.data["message"].as_u64() };
            let role_match = f.role.as_ref().is_none_or(|r| l.actor == *r || l.data["role"] == r.as_str() || l.data["to"] == r.as_str() || l.data["from"] == r.as_str() || l.data["recipients"].as_array().is_some_and(|roles| roles.iter().any(|role| role == r.as_str())) || proposal_id.is_some_and(|id| pids.contains(&id)) || message_id.is_some_and(|id| mids.contains(&id)));
            let inject_match = f.inject.is_none_or(|id| l.data["inject_id"] == id || proposal_id.is_some_and(|id| pids.contains(&id)) || message_id.is_some_and(|id| mids.contains(&id)));
            role_match && inject_match && f.turn.is_none_or(|t| l.turn == t) && f.phase.is_none_or(|p| l.phase == p) && f.exercise_phase.is_none_or(|phase| l.exercise_phase==phase) && f.kind.as_ref().is_none_or(|kind| l.kind == *kind)
        }).collect::<Vec<_>>();
        let timeline_total = log.len();
        let offset = f.offset.unwrap_or(0);
        let limit = f.limit.unwrap_or(200).clamp(1, 1000);
        let log = log.into_iter().rev().skip(offset).take(limit).collect::<Vec<_>>();
        let chains = proposals.iter().map(|p| {
            let parent = self.messages.iter().find(|m| Some(m.id) == p.reply_to);
            let declared = p.history.first();
            json!({"proposal":p, "messages":messages.iter().filter(|m| (p.inject_id.is_some() && m.inject_id == p.inject_id) || m.proposal == Some(p.id)).collect::<Vec<_>>(),
                "reply_delay_wall_ms":parent.zip(declared).map(|(m,h)| h.wall_ms.saturating_sub(m.wall_ms)),
                "reply_delay_sim_s":parent.zip(declared).map(|(m,h)| h.t_s - m.t_s),
                "master_queue_wall_ms":declared.zip(p.decision.as_ref()).map(|(h,d)| d.wall_ms.saturating_sub(h.wall_ms)),
                "master_queue_sim_s":declared.zip(p.decision.as_ref()).map(|(h,d)| d.t_s - h.t_s)})
        }).collect::<Vec<_>>();
        let cards = self.players.values().map(|player| {
            let own = self.proposals.iter().filter(|p| p.source.label() == player.role).collect::<Vec<_>>();
            let last_inject = self.messages.iter().rev().find(|m| m.to == player.role && m.kind == MsgKind::Inject);
            let first_reply = last_inject.and_then(|m| own.iter().find(|p| p.inject_id == m.inject_id));
            json!({"player":player,"last_inject":last_inject,"last_reply":own.last(),"awaiting_reply":last_inject.is_some() && first_reply.is_none(),
                "counts":{"declared":own.len(),"approved":own.iter().filter(|p| p.decision.as_ref().is_some_and(|d| d.approve)).count(),"edited":own.iter().filter(|p| p.decision.as_ref().is_some_and(|d| d.edited)).count(),"rejected":own.iter().filter(|p| p.decision.as_ref().is_some_and(|d| !d.approve)).count(),"executed":own.iter().filter(|p| p.executions.iter().any(|e| e.ok)).count(),"failed":own.iter().filter(|p| p.status == ProposalStatus::Failed).count()}})
        }).collect::<Vec<_>>();
        let objectives = self.world.scn.msel.iter().map(|i| {
            let deliveries = self.log.iter().filter(|l| l.kind == "inject_sent" && l.data["msel_id"] == i.id).collect::<Vec<_>>();
            json!({"msel_id":i.id,"title":i.title,"objective":i.objective,"planned_turn":i.turn,"deliveries":deliveries})
        }).collect::<Vec<_>>();
        Ok(json!({"context":{"clock":self.world.clock(),"exercise_phase":self.exercise_phase,"clock_control":self.clock_control,"turn":self.turn,"phase":self.phase,"stats":self.world.stats(),"weather":self.world.fire.weather(),"pending":self.proposals.iter().filter(|p| matches!(p.status, ProposalStatus::Parsing | ProposalStatus::Pending | ProposalStatus::NeedsClarification)).count()},
            "role_cards":cards,"objectives":objectives,"reference_materials":self.reference_materials,"chains":chains,"messages":messages,"timeline":log,"timeline_total":timeline_total,"offset":offset,"limit":limit,
            "knowledge_events":self.knowledge_events.iter().filter(|e| mids.contains(&e.message)).collect::<Vec<_>>(),"resource_limits":self.resource_limits,"resource_accounting":self.resource_accounting(None)}))
    }
}
