//! References and immutable evidence for a selected conversation thread.
use crate::{Exercise, Message, MsgKind, MASTER};
use anyhow::{bail, Result};
use serde::Serialize;

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
        let received_inject = m.to == role
            && m.inject_id.is_some()
            && matches!(m.kind, MsgKind::Inject | MsgKind::Clarification);
        let peer_chat = m.kind == MsgKind::Chat
            && (m.from == role || m.to == role)
            && m.from != MASTER
            && m.to != MASTER;
        if !received_inject && !peer_chat {
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
