//! AI action suggestions for narrative responses: the context each reader may
//! give the model, and the validation of what comes back.
//!
//! Two scopes (see `llm::tasks::SuggestScope`):
//! * the **player** writing a narrative gets suggestions built only from what
//!   that role knows (its map, resources, deployments, threads, the
//!   conversations it takes part in and its own AI conversation);
//! * the **EXCON** reviewing a narrative response may ask for suggestions built
//!   from the full exercise state and every conversation around the inject.
//!
//! Nothing here changes the exercise: suggestions are drafts a human adds to a
//! response (player) or to the reviewed actions of a proposal (EXCON), and
//! every player action still needs EXCON approval.

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sim::{Actor, Command, Verdict};

use crate::{ChatTurn, Exercise, MsgKind, Phase, ProposalStatus, Source, MASTER};

/// Longest narrative sent for suggestions (same bound as a response narrative).
const MAX_NARRATIVE: usize = 8000;
/// Peer-chat messages included in a context, newest kept.
const MAX_CHATS: usize = 40;

/// What the model may read for one suggestion request.
#[derive(Debug, Clone)]
pub struct SuggestionContext {
    /// Role whose actions are suggested (the narrative's author).
    pub role: String,
    pub narrative: String,
    pub digest: Value,
    pub conversations: Value,
    /// The author's continuous AI conversation, oldest first.
    pub history: Vec<ChatTurn>,
}

/// One validated suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedAction {
    pub command: Command,
    pub description: String,
    /// Why the model suggests it (in the reader's language).
    pub reason: String,
    /// Feasibility check now; a rejected verdict is shown, never hidden.
    pub verdict: Verdict,
}

/// Validated suggestions plus how many model actions were dropped.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SuggestionCheck {
    pub actions: Vec<SuggestedAction>,
    /// Model actions that were not valid commands of the role, or repeats.
    pub dropped: usize,
}

impl Exercise {
    /// Context for suggestions shown to `role` while it writes a narrative:
    /// only role-visible information.
    pub fn player_suggestion_context(
        &self,
        role: &str,
        narrative: &str,
        reply_to: Option<u64>,
        conversation: Option<u64>,
        staged: &[Command],
    ) -> Result<SuggestionContext> {
        if self.phase != Phase::Orders {
            bail!("suggestions are available while the turn is open for orders");
        }
        if self.world.scn.role(role).is_none() {
            bail!("unknown role {role}");
        }
        check_narrative(narrative)?;
        let thread = match reply_to {
            Some(id) => serde_json::to_value(self.thread_evidence(role, id)?)?,
            None => json!([]),
        };
        let mut linked = self.player_linked_conversations(role);
        if let Some(cid) = conversation {
            let Some(pos) = linked.iter().position(|c| c["id"] == cid) else {
                bail!("conversation {cid} is not one of yours");
            };
            // The conversation being answered from comes first.
            let selected = linked.remove(pos);
            linked.insert(0, selected);
        }
        let chats = self
            .role_chats(role)
            .map(|m| {
                let text = if m.from == role {
                    m.original.as_deref().unwrap_or(&m.text)
                } else {
                    &m.text
                };
                json!({"id": m.id, "from": m.from, "to": m.to, "clock": m.clock, "text": text, "reply_to": m.reply_to})
            })
            .collect::<Vec<_>>();
        let digest = json!({
            "clock": self.world.clock(),
            "turn": self.turn,
            "map": self.player_map(role)?,
            "resources": self.world.d.pools.get(role),
            "resource_accounting": self.resource_accounting(Some(role))[role],
            "deployments": self.world.d.tasks.iter().filter(|t| t.role == role).collect::<Vec<_>>(),
            "own_proposals_this_turn": self.own_proposals(role),
            "already_staged": staged.iter().map(Command::describe).collect::<Vec<_>>(),
            "thread": thread,
        });
        Ok(SuggestionContext {
            role: role.into(),
            narrative: narrative.into(),
            digest,
            conversations: json!({"linked_conversations": linked, "peer_chats": chats}),
            history: self.conversation(role),
        })
    }

    /// Context for suggestions the EXCON asks for on narrative proposal `pid`:
    /// the full exercise state and every conversation around its inject.
    pub fn excon_suggestion_context(&self, pid: u64) -> Result<SuggestionContext> {
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown proposal {pid}"))?;
        let Source::Player(role) = &p.source else {
            bail!("only player responses get action suggestions");
        };
        if !p.narrative {
            bail!("proposal {pid} has no narrative");
        }
        if matches!(
            p.status,
            ProposalStatus::Superseded | ProposalStatus::RolledBack | ProposalStatus::Rejected
        ) {
            bail!("proposal {pid} is already {:?}", p.status);
        }
        let narrative = if p.translation_en.trim().is_empty() {
            p.original.clone()
        } else {
            p.translation_en.clone()
        };
        let inject_thread = self
            .messages
            .iter()
            .filter(|m| p.inject_id.is_some() && m.inject_id == p.inject_id)
            .map(|m| json!({"id": m.id, "kind": m.kind, "from": m.from, "to": m.to, "clock": m.clock, "title": m.title, "text": m.text_en, "reply_to": m.reply_to}))
            .collect::<Vec<_>>();
        let linked = self
            .master_linked_conversations()
            .into_iter()
            .filter(|c| {
                p.inject_id.is_some_and(|i| c["inject_id"] == i)
                    || c["participants"]
                        .as_array()
                        .is_some_and(|x| x.iter().any(|r| r == role.as_str()))
            })
            .collect::<Vec<_>>();
        let chats = self
            .role_chats(role)
            .map(|m| json!({"id": m.id, "from": m.from, "to": m.to, "clock": m.clock, "text": m.text_en, "reply_to": m.reply_to}))
            .collect::<Vec<_>>();
        let digest = json!({
            "clock": self.world.clock(),
            "turn": self.turn,
            "exercise_phase": self.exercise_phase,
            "training_design": self.design_view(),
            "world": self.world.digest(None),
            "deployments": self.world.deployment_view(),
            "resource_limits": self.resource_limits,
            "resource_accounting": self.resource_accounting(None),
            "player_knowledge_map": self.player_map(role).unwrap_or_default(),
            "proposal": {"id": p.id, "status": p.status, "original": p.original, "narrative_en": narrative, "already_proposed": p.descriptions},
            "role_proposals_this_turn": self.own_proposals(role),
        });
        Ok(SuggestionContext {
            role: role.clone(),
            narrative,
            digest,
            conversations: json!({"inject_thread": inject_thread, "linked_conversations": linked, "role_peer_chats": chats}),
            history: self.conversation(role),
        })
    }

    /// Keep only valid, allowed, new commands of `role`, each with its reason
    /// and a feasibility verdict computed after the approved and `staged` ones.
    pub fn check_suggestions(
        &self,
        role: &str,
        actions: &[Value],
        reasons: &[String],
        staged: &[Command],
    ) -> SuggestionCheck {
        let allowed = self
            .world
            .scn
            .role(role)
            .map(|r| r.allowed_actions.clone())
            .unwrap_or_default();
        let mut kept: Vec<(Command, String)> = Vec::new();
        let mut dropped = 0;
        for (i, raw) in actions.iter().enumerate() {
            let Ok(cmd) = serde_json::from_value::<Command>(raw.clone()) else {
                dropped += 1;
                continue;
            };
            if !allowed.iter().any(|k| k == cmd.kind()) {
                dropped += 1;
                continue;
            }
            let (cmd, _) = self.world.reconcile_command(&cmd);
            if staged.contains(&cmd) || kept.iter().any(|(c, _)| *c == cmd) {
                dropped += 1;
                continue;
            }
            let reason = reasons
                .get(i)
                .map(|r| r.trim().chars().take(300).collect())
                .unwrap_or_default();
            kept.push((cmd, reason));
        }
        let actor = Actor::Role(role.into());
        let mut prior = self.prior(u64::MAX);
        prior.extend(staged.iter().map(|c| (actor.clone(), c.clone())));
        let commands = kept.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
        let mut verdicts = self.world.validate_batch(&prior, &commands, &actor);
        self.check_limit_batch(u64::MAX, &commands, &actor, &mut verdicts);
        SuggestionCheck {
            actions: kept
                .into_iter()
                .zip(verdicts)
                .map(|((command, reason), verdict)| SuggestedAction {
                    description: command.describe(),
                    command,
                    reason,
                    verdict,
                })
                .collect(),
            dropped,
        }
    }

    /// Peer chats involving `role`, newest `MAX_CHATS`.
    fn role_chats<'a>(&'a self, role: &'a str) -> impl Iterator<Item = &'a crate::Message> + 'a {
        let all = self
            .messages
            .iter()
            .filter(move |m| {
                m.kind == MsgKind::Chat
                    && m.from != MASTER
                    && m.to != MASTER
                    && (m.from == role || m.to == role)
            })
            .collect::<Vec<_>>();
        let skip = all.len().saturating_sub(MAX_CHATS);
        all.into_iter().skip(skip)
    }

    /// The role's proposals of the current turn, so suggestions do not repeat them.
    fn own_proposals(&self, role: &str) -> Vec<Value> {
        self.proposals
            .iter()
            .filter(|p| p.turn == self.turn && p.source.label() == role)
            .filter(|p| {
                !matches!(
                    p.status,
                    ProposalStatus::Superseded | ProposalStatus::RolledBack
                )
            })
            .map(|p| json!({"id": p.id, "status": p.status, "actions": p.descriptions}))
            .collect()
    }
}

fn check_narrative(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("write the narrative first");
    }
    if text.chars().count() > MAX_NARRATIVE {
        bail!("narrative is too long");
    }
    Ok(())
}
