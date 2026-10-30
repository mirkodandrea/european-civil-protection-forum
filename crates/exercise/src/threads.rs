//! References and immutable evidence for a selected conversation thread.
use crate::{Exercise, Message, MsgKind, MASTER};
use anyhow::{bail, Result};
use crate::{Proposal, ProposalStatus, Source};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessage {
    pub id: u64,
    pub turn: u32,
    pub clock: String,
    pub from: String,
    pub to: String,
    pub kind: MsgKind,
    pub title: Option<String>,
    /// English working copy for EXCON review.
    pub text: String,
    pub reply_to: Option<u64>,
    pub inject_id: Option<u64>,
}

impl From<&Message> for ThreadMessage {
    fn from(m: &Message) -> Self {
        Self {
            id: m.id,
            turn: m.turn,
            clock: m.clock.clone(),
            from: m.from.clone(),
            to: m.to.clone(),
            kind: m.kind,
            title: m.title.clone(),
            text: m.text_en.clone(),
            reply_to: m.reply_to,
            inject_id: m.inject_id,
        }
    }
}

impl Exercise {
    pub fn validate_action_reference(&self, role: &str, id: u64) -> Result<&Message> {
        let m = self.messages.iter().find(|m| m.id == id).ok_or_else(|| {
            anyhow::anyhow!("reply_to must reference a message visible to your role")
        })?;
        // An EXCON clarification question can always be answered, even when the
        // declaration it concerns was not a reply to an inject.
        let received_inject = m.to == role
            && (m.kind == MsgKind::Clarification
                || (m.inject_id.is_some() && m.kind == MsgKind::Inject));
        let peer_chat = m.kind == MsgKind::Chat
            && (m.from == role || m.to == role)
            && m.from != MASTER
            && m.to != MASTER;
        // The inject delivery a conversation of this role is linked to can be
        // answered from that conversation (see [`crate::conversations`]).
        let via_conversation = m.kind == MsgKind::Inject
            && m.inject_id.is_some()
            && self.conversation_for_delivery(role, id).is_some();
        if !received_inject && !peer_chat && !via_conversation {
            bail!("reply_to must reference a peer chat or inject delivered to your role");
        }
        Ok(m)
    }

    pub fn thread_evidence(&self, role: &str, parent: u64) -> Result<Vec<ThreadMessage>> {
        let selected = self.validate_action_reference(role, parent)?;
        let root = selected.inject_id;
        let mut chain = Vec::new();
        let mut next = Some(parent);
        while let Some(id) = next {
            let Some(message) = self.messages.iter().find(|m| m.id == id) else {
                break;
            };
            if message.from != role && message.to != role {
                break;
            }
            if chain.iter().any(|m: &ThreadMessage| m.id == id) {
                break;
            }
            chain.push(ThreadMessage::from(message));
            next = message.reply_to;
        }
        if let Some(root) = root {
            if let Some(inject) = self
                .messages
                .iter()
                .find(|m| m.kind == MsgKind::Inject && m.inject_id == Some(root) && m.to == role)
            {
                if !chain.iter().any(|m| m.id == inject.id) {
                    chain.push(ThreadMessage::from(inject));
                }
            }
        }
        // A participant of a linked conversation who never received the
        // inject knows it by its title only (see `crate::conversations`).
        if selected.kind == MsgKind::Inject
            && selected.to != role
            && !chain.iter().any(|m| m.id == selected.id)
        {
            let mut quoted = ThreadMessage::from(selected);
            if self.own_inject_copy(role, selected).is_none() {
                quoted.text.clear();
            }
            chain.push(quoted);
        }
        // Include replies already exchanged in the selected branch. A peer
        // parent stays within that pair; an inject can connect several peers.
        let peer = if selected.kind == MsgKind::Chat {
            Some(if selected.from == role {
                &selected.to
            } else {
                &selected.from
            })
        } else {
            None
        };
        loop {
            let mut added = false;
            for m in &self.messages {
                let same_peer = peer.is_none_or(|peer| {
                    (m.from == role && &m.to == peer) || (m.to == role && &m.from == peer)
                });
                if m.kind != MsgKind::Chat
                    || !same_peer
                    || (m.from != role && m.to != role)
                    || chain.iter().any(|entry| entry.id == m.id)
                {
                    continue;
                }
                if m.reply_to
                    .is_some_and(|id| chain.iter().any(|entry| entry.id == id))
                {
                    chain.push(ThreadMessage::from(m));
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        chain.sort_by_key(|m| m.id);
        Ok(chain)
    }
}

// ------------------------------------------------------------------ clarification threads
//
// A clarification thread is the text conversation between EXCON and one player
// about a single declaration ("chiedi chiarimento"). It is a *view* over
// existing records, not a second copy of them: the chain of proposals linked by
// `supersedes`/`superseded_by` (each player answer replaces the question's
// proposal) plus the `Clarification` messages EXCON sent and the `Reply`/`Chat`
// messages the player wrote about those proposals. Only `clarification_closed`
// on the latest proposal is extra state. The thread id is the id of the
// originating proposal.

/// Who is expected to speak next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// EXCON asked; the player has not answered yet.
    AwaitingPlayer,
    /// The player answered (or the question is not out yet); EXCON may ask again, decide or close.
    AwaitingExcon,
    /// EXCON closed the thread, or the proposal was decided.
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadAuthor {
    Excon,
    Player,
}

/// One message of the conversation. `seq` starts at 0 and is dense.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadEntry {
    pub seq: u32,
    pub author: ThreadAuthor,
    /// `"master"` or the player's role id.
    pub from: String,
    /// In the player's language.
    pub text: String,
    /// English working copy.
    pub text_en: String,
    /// The stored message this entry is.
    pub message: u64,
    /// Who EXCON spoke as (players see this resolved name instead of "EXCON").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// The proposal the message belongs to.
    pub proposal: Option<u64>,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClarificationThread {
    /// Originating proposal id.
    pub id: u64,
    pub role: String,
    pub inject_id: Option<u64>,
    /// The latest proposal of the chain: the one EXCON can still ask about or decide.
    pub current_proposal: u64,
    /// Every proposal of the chain, oldest first.
    pub proposals: Vec<u64>,
    pub status: ThreadStatus,
    pub entries: Vec<ThreadEntry>,
}

impl Exercise {
    /// All clarification threads, oldest first.
    pub fn clarification_threads(&self) -> Vec<ClarificationThread> {
        self.proposals
            .iter()
            .filter(|p| p.supersedes.is_none() && matches!(p.source, Source::Player(_)))
            .filter_map(|root| self.build_thread(root))
            .collect()
    }

    pub fn clarification_thread(&self, id: u64) -> Option<ClarificationThread> {
        self.proposal(id)
            .filter(|p| p.supersedes.is_none())
            .and_then(|p| self.build_thread(p))
    }

    /// The thread whose chain contains `pid`.
    pub fn clarification_thread_of(&self, pid: u64) -> Option<ClarificationThread> {
        let mut root = self.proposal(pid)?;
        while let Some(prev) = root.supersedes.and_then(|id| self.proposal(id)) {
            root = prev;
        }
        self.clarification_thread(root.id)
    }

    /// Threads of one role as the player sees them: no English working copies.
    pub fn player_clarification_threads(&self, role: &str) -> Vec<Value> {
        self.clarification_threads()
            .into_iter()
            .filter(|t| t.role == role)
            .map(|t| {
                let mut v = json!(t);
                let language = self.language_of(role);
                for e in v["entries"].as_array_mut().into_iter().flatten() {
                    if e["author"] == "excon" {
                        let name = self.persona_name(e["persona"].as_str(), &language);
                        e["persona"] = name.into();
                    }
                    e.as_object_mut().map(|o| o.remove("text_en"));
                }
                v
            })
            .collect()
    }

    fn build_thread(&self, root: &Proposal) -> Option<ClarificationThread> {
        let Source::Player(role) = &root.source else { return None };
        let mut chain: Vec<&Proposal> = vec![root];
        while let Some(next) = chain.last().and_then(|p| p.superseded_by).and_then(|id| self.proposal(id)) {
            if chain.iter().any(|p| p.id == next.id) {
                break;
            }
            chain.push(next);
        }
        let pids: Vec<u64> = chain.iter().map(|p| p.id).collect();
        let mut entries: Vec<ThreadEntry> = Vec::new();
        let mut asked = false;
        for m in &self.messages {
            let Some(pid) = m.proposal.filter(|pid| pids.contains(pid)) else { continue };
            let (author, text_en) = match m.kind {
                MsgKind::Clarification if m.from == MASTER && &m.to == role => {
                    asked = true;
                    (ThreadAuthor::Excon, m.text_en.clone())
                }
                MsgKind::Reply | MsgKind::Chat if &m.from == role && m.to == MASTER => {
                    // The proposal's translation belongs to the declaration (the
                    // first player message); later answers carry their own.
                    let first = !entries.iter().any(|e| e.author == ThreadAuthor::Player && e.proposal == Some(pid));
                    let en = self
                        .proposal(pid)
                        .filter(|_| first)
                        .map(|p| p.translation_en.clone())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| m.text_en.clone());
                    (ThreadAuthor::Player, en)
                }
                _ => continue,
            };
            let text = if author == ThreadAuthor::Player {
                m.original.clone().unwrap_or_else(|| m.text.clone())
            } else {
                m.text.clone()
            };
            entries.push(ThreadEntry {
                seq: entries.len() as u32,
                author,
                from: m.from.clone(),
                text,
                text_en,
                message: m.id,
                persona: m.persona.clone(),
                proposal: Some(pid),
                turn: m.turn,
                t_s: m.t_s,
                clock: m.clock.clone(),
            });
        }
        // A question EXCON has asked but not yet delivered still makes a thread.
        let asked_pending = chain.iter().any(|p| p.clarification_request.is_some());
        if !asked && !asked_pending {
            return None;
        }
        let current = *chain.last()?;
        let open = matches!(
            current.status,
            ProposalStatus::Parsing | ProposalStatus::Pending | ProposalStatus::NeedsClarification
        );
        let status = if current.clarification_closed || !open {
            ThreadStatus::Closed
        } else if entries.last().is_some_and(|e| e.author == ThreadAuthor::Excon) {
            ThreadStatus::AwaitingPlayer
        } else {
            ThreadStatus::AwaitingExcon
        };
        Some(ClarificationThread {
            id: root.id,
            role: role.clone(),
            inject_id: root.inject_id,
            current_proposal: current.id,
            proposals: pids,
            status,
            entries,
        })
    }

    /// EXCON considers the thread settled and goes back to deciding on the
    /// latest proposal. A question the player has not answered is withdrawn
    /// (the proposal returns to `Pending`); a new `request_clarification`
    /// reopens the thread.
    pub fn close_clarification(&mut self, thread_id: u64) -> Result<()> {
        let t = self
            .clarification_thread(thread_id)
            .ok_or_else(|| anyhow::anyhow!("unknown clarification thread {thread_id}"))?;
        if t.status == ThreadStatus::Closed {
            bail!("clarification thread {thread_id} is already closed");
        }
        let pid = t.current_proposal;
        self.proposal_mut(pid)?.clarification_closed = true;
        if self.proposal(pid).map(|p| p.status) == Some(ProposalStatus::NeedsClarification) {
            self.transition(pid, ProposalStatus::Pending, MASTER, "Clarification closed by EXCON")?;
        }
        self.record("clarification_closed", MASTER, json!({"thread": thread_id, "proposal": pid}));
        Ok(())
    }
}
