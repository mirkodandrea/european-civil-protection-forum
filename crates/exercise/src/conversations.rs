//! Player conversations linked to an inject.
//!
//! A player who received an inject can open a conversation with one or more
//! other players about it. Other players cannot read it; EXCON reads every
//! transcript. It is *evidence only*: nothing said in it
//! changes the world. Its purpose is to let the group prepare a response.
//!
//! Rules (all enforced here, never in the UI):
//! * only the recipient of an inject delivery can start a conversation on it;
//! * starting one is motivated ([`ConversationReason`]): what it rests on (a
//!   plan or procedure, a law or regulation, or personal initiative, stored as
//!   [`BasisKind::OperationalJudgment`]), the reference for a plan or law, and
//!   why the player starts it. Participants read it as the opening context;
//!   EXCON and the AI contexts that read conversations get it too. Later
//!   messages need none of it;
//! * participants are other known players (1..=[`MAX_PARTICIPANTS`] besides the creator);
//! * the inject quoted at the top of the conversation aggregates it under that
//!   inject: participants who received the inject read their own copy, the
//!   others only its title, sender and time (never its text, in any language);
//! * a message may cite a previous message of the conversation or the inject;
//! * a participant may answer the inject itself from the conversation
//!   (`Exercise::player_response` with `conversation`), even without having
//!   received it. The provenance is stored on the response;
//! * a response addressed to players (not to the fictitious sender) is posted
//!   in a conversation with them, tagged with its proposal, so it stays in the
//!   inject's history and the conversation reads continuously;
//! * nothing said in a conversation is a decision: "vado in ricognizione" in
//!   chat is an intent, recorded only as text (never parsed, never a mission).
use crate::{chat_policy, BasisKind, Exercise, Message, MsgKind, Phase, MASTER};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Players that can be invited next to the creator.
pub const MAX_PARTICIPANTS: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct ConvMessage {
    pub id: u64,
    pub from: String,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
    /// What the sender wrote.
    pub original: String,
    /// English working copy.
    pub text_en: String,
    /// Role -> text in that role's language.
    pub texts: BTreeMap<String, String>,
    /// A previous message of the conversation, or the inject delivery.
    pub reply_to: Option<u64>,
    pub translation_failed: bool,
    /// The response (proposal) this message delivers, when a player answered
    /// an inject with "Rispondi" addressed to other players.
    pub proposal: Option<u64>,
}

/// Why a player started a conversation (required when starting one).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConversationReason {
    /// Plan, procedure, law, or personal initiative (`OperationalJudgment`).
    pub basis: BasisKind,
    /// Which plan, procedure or law (required for those bases).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Why the player starts the conversation, as written.
    pub rationale: String,
    /// English working copy of the rationale.
    pub rationale_en: String,
    /// Role -> rationale in that role's language (missing: the English copy).
    pub texts: BTreeMap<String, String>,
}

/// Rationale stored, on replay, for a conversation of a session saved before
/// starting one needed a reason.
pub const LEGACY_REASON: &str =
    "(not recorded: conversation started before a reason was required)";

impl ConversationReason {
    /// Check what the player gave; translations are filled in later
    /// (`rationale_en` starts as the rationale itself).
    pub fn new(
        basis: Option<BasisKind>,
        reference: Option<&str>,
        rationale: Option<&str>,
    ) -> Result<Self> {
        let Some(basis) = basis else {
            bail!("say why you start this conversation: a plan or procedure, a law or regulation, or personal initiative");
        };
        let reference = reference.map(str::trim).filter(|r| !r.is_empty());
        if basis != BasisKind::OperationalJudgment && reference.is_none() {
            bail!("say which plan, procedure or law the conversation rests on (e.g. the plan's section or the article)");
        }
        let Some(rationale) = rationale.map(str::trim).filter(|r| !r.is_empty()) else {
            bail!("starting a conversation needs its rationale: why you start it");
        };
        if rationale.chars().count() > 2000 || reference.is_some_and(|r| r.chars().count() > 2000) {
            bail!("the reference and the rationale are limited to 2000 characters each");
        }
        Ok(Self {
            basis,
            // Personal initiative has no reference line: a leftover one is dropped.
            reference: reference
                .filter(|_| basis != BasisKind::OperationalJudgment)
                .map(String::from),
            rationale: rationale.into(),
            rationale_en: rationale.into(),
            texts: BTreeMap::new(),
        })
    }

    /// Placeholder for a conversation replayed from an older session.
    pub fn legacy() -> Self {
        Self {
            basis: BasisKind::OperationalJudgment,
            reference: None,
            rationale: LEGACY_REASON.into(),
            rationale_en: LEGACY_REASON.into(),
            texts: BTreeMap::new(),
        }
    }

    /// English wording of the basis (logs, AI context).
    pub fn basis_label(&self) -> &'static str {
        match self.basis {
            BasisKind::OperationalJudgment => "personal initiative",
            kind => crate::response::basis_label(kind),
        }
    }

    /// The reason as participant `role` reads it.
    fn for_role(&self, role: &str, creator: &str) -> Value {
        let text = if role == creator {
            &self.rationale
        } else {
            self.texts.get(role).unwrap_or(&self.rationale_en)
        };
        json!({"basis": self.basis, "reference": self.reference, "rationale": text})
    }

    /// The reason as EXCON reads it: original and English copy.
    fn for_master(&self) -> Value {
        json!({"basis": self.basis, "basis_label": self.basis_label(), "reference": self.reference,
            "rationale": self.rationale, "rationale_en": self.rationale_en})
    }

    /// One English line for the AI contexts.
    pub fn line(&self) -> String {
        match &self.reference {
            Some(r) => format!("{} ({r}): {}", self.basis_label(), self.rationale_en),
            None => format!("{}: {}", self.basis_label(), self.rationale_en),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkedConversation {
    pub id: u64,
    /// Message id of the creator's copy of the inject: the citation target.
    pub delivery: u64,
    /// Delivery event shared by every recipient of the inject (`Message::inject_id`).
    pub inject_id: u64,
    pub created_by: String,
    /// Creator first.
    pub participants: Vec<String>,
    pub turn: u32,
    pub t_s: i64,
    /// Why the creator started it.
    pub reason: ConversationReason,
    pub messages: Vec<ConvMessage>,
}

impl LinkedConversation {
    pub fn has(&self, role: &str) -> bool {
        self.participants.iter().any(|p| p == role)
    }
}

fn check_text(ex: &Exercise, from: &str, text: &str) -> Result<()> {
    if ex.phase == Phase::Debrief {
        bail!("the exercise has ended");
    }
    if text.trim().is_empty() || text.chars().count() > 8000 {
        bail!("message must contain 1–8000 characters");
    }
    chat_policy::validate_player_chat(text, &ex.language_of(from))
}

impl Exercise {
    pub fn linked_conversation(&self, id: u64) -> Option<&LinkedConversation> {
        self.linked_conversations.iter().find(|c| c.id == id)
    }

    /// The conversation `role` takes part in that is linked to inject copy `delivery`.
    pub fn conversation_for_delivery(
        &self,
        role: &str,
        delivery: u64,
    ) -> Option<&LinkedConversation> {
        self.linked_conversations
            .iter()
            .rev()
            .find(|c| c.delivery == delivery && c.has(role))
    }

    /// Participants with their language, to prepare the translations.
    pub fn conversation_languages(&self, id: u64) -> Vec<(String, String)> {
        self.linked_conversation(id)
            .map(|c| {
                c.participants
                    .iter()
                    .map(|p| (p.clone(), self.language_of(p)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn validate_conversation_start(
        &self,
        creator: &str,
        delivery: u64,
        others: &[String],
        text: &str,
    ) -> Result<()> {
        check_text(self, creator, text)?;
        let inject = self.messages.iter().find(|m| m.id == delivery);
        if !inject
            .is_some_and(|m| m.kind == MsgKind::Inject && m.to == creator && m.inject_id.is_some())
        {
            bail!("a conversation can only be linked to an inject delivered to your role");
        }
        if others.is_empty() || others.len() > MAX_PARTICIPANTS {
            bail!("invite between 1 and {MAX_PARTICIPANTS} other players");
        }
        for (i, other) in others.iter().enumerate() {
            if other == creator || other == MASTER || !self.players.contains_key(other) {
                bail!("invalid conversation participant");
            }
            if others[..i].contains(other) {
                bail!("duplicate conversation participant");
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_linked_conversation(
        &mut self,
        creator: &str,
        delivery: u64,
        others: &[String],
        text: &str,
        text_en: &str,
        texts: &BTreeMap<String, String>,
        translation_failed: bool,
        reason: ConversationReason,
    ) -> Result<(u64, u64)> {
        self.validate_conversation_start(creator, delivery, others, text)?;
        let inject_id = self
            .messages
            .iter()
            .find(|m| m.id == delivery)
            .and_then(|m| m.inject_id)
            .expect("validated delivery");
        let id = self.id();
        let mut participants = vec![creator.to_string()];
        participants.extend(others.iter().cloned());
        self.linked_conversations.push(LinkedConversation {
            id,
            delivery,
            inject_id,
            created_by: creator.into(),
            participants,
            turn: self.turn,
            t_s: self.world.time_s(),
            reason: reason.clone(),
            messages: Vec::new(),
        });
        self.record(
            "conversation_started",
            creator,
            json!({"conversation": id, "delivery": delivery, "inject_id": inject_id, "participants": self.linked_conversation(id).map(|c| c.participants.clone()),
                "reason": reason.for_master()}),
        );
        // The opening context, in every participant's AI conversation.
        let title = self
            .messages
            .iter()
            .find(|m| m.id == delivery)
            .and_then(|m| m.title.clone())
            .unwrap_or_default();
        let clock = self.world.clock();
        let participants = self
            .linked_conversation(id)
            .map(|c| c.participants.clone())
            .unwrap_or_default();
        for p in &participants {
            let who = if p == creator { "you" } else { creator };
            let line = format!(
                "[{clock}] CONVERSATION about inject \"{title}\" started by {who} with {} — reason: {}",
                participants.join(", "),
                reason.line()
            );
            self.converse(p, "user", line);
        }
        let mid = self.push_conversation_message(
            id,
            creator,
            text,
            text_en,
            texts,
            Some(delivery),
            translation_failed,
        );
        Ok((id, mid))
    }

    pub fn validate_conversation_message(
        &self,
        role: &str,
        id: u64,
        text: &str,
        reply_to: Option<u64>,
    ) -> Result<()> {
        check_text(self, role, text)?;
        let c = self
            .linked_conversation(id)
            .filter(|c| c.has(role))
            .ok_or_else(|| anyhow::anyhow!("unknown conversation"))?;
        if let Some(r) = reply_to {
            if r != c.delivery && !c.messages.iter().any(|m| m.id == r) {
                bail!("reply_to must cite the inject or a message of this conversation");
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_conversation_message(
        &mut self,
        role: &str,
        id: u64,
        text: &str,
        reply_to: Option<u64>,
        text_en: &str,
        texts: &BTreeMap<String, String>,
        translation_failed: bool,
    ) -> Result<u64> {
        self.validate_conversation_message(role, id, text, reply_to)?;
        Ok(self.push_conversation_message(
            id,
            role,
            text,
            text_en,
            texts,
            reply_to,
            translation_failed,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_conversation_message(
        &mut self,
        id: u64,
        from: &str,
        text: &str,
        text_en: &str,
        texts: &BTreeMap<String, String>,
        reply_to: Option<u64>,
        translation_failed: bool,
    ) -> u64 {
        let mid = self.id();
        let (participants, delivery) = {
            let c = self.linked_conversation(id).expect("conversation");
            (c.participants.clone(), c.delivery)
        };
        let mut by_role = BTreeMap::new();
        for p in &participants {
            by_role.insert(
                p.clone(),
                if p == from {
                    text.to_string()
                } else {
                    texts.get(p).cloned().unwrap_or_else(|| text_en.to_string())
                },
            );
        }
        let message = ConvMessage {
            id: mid,
            from: from.into(),
            turn: self.turn,
            t_s: self.world.time_s(),
            clock: self.world.clock(),
            original: text.into(),
            text_en: text_en.into(),
            texts: by_role,
            reply_to,
            translation_failed,
            proposal: None,
        };
        // EXCON reads the text like every other message, so the log carries it too.
        self.record(
            "conversation_message",
            from,
            json!({"conversation": id, "message": mid, "reply_to": reply_to, "chars": text.chars().count(), "original": text, "text_en": text_en}),
        );
        let title = self
            .messages
            .iter()
            .find(|m| m.id == delivery)
            .and_then(|m| m.title.clone())
            .unwrap_or_default();
        let clock = message.clock.clone();
        for p in &participants {
            let line = if p == from {
                format!("[{clock}] CONVERSATION about inject \"{title}\" — you wrote: {text_en}")
            } else {
                format!("[{clock}] CONVERSATION about inject \"{title}\" — {from} wrote: {text_en}")
            };
            self.converse(p, "user", line);
        }
        self.linked_conversations
            .iter_mut()
            .find(|c| c.id == id)
            .expect("conversation")
            .messages
            .push(message);
        mid
    }

    /// `role`'s own copy of the inject delivered as `delivery`, if EXCON sent it to them.
    pub fn own_inject_copy(&self, role: &str, delivery: &Message) -> Option<&Message> {
        self.messages.iter().find(|m| {
            m.kind == MsgKind::Inject
                && m.inject_id.is_some()
                && m.inject_id == delivery.inject_id
                && m.to == role
        })
    }

    /// The inject as `role` reads it inside a conversation (see module rules):
    /// their own copy when they received it, else the title, sender and time
    /// only (`text` null, `title_only` true).
    fn quote(&self, role: &str, c: &LinkedConversation) -> Value {
        let Some(m) = self.messages.iter().find(|m| m.id == c.delivery) else {
            return Value::Null;
        };
        let own = self.own_inject_copy(role, m);
        json!({"id": m.id, "kind": "inject", "inject_id": m.inject_id,
            "title": own.and_then(|o| o.title.clone()).or_else(|| m.title.clone()),
            "from": m.from, "to": m.to, "persona": m.persona, "clock": m.clock, "turn": m.turn, "t_s": m.t_s,
            "text": own.map(|o| o.text.clone()), "title_only": own.is_none()})
    }

    /// Conversations of one role as that player sees them.
    pub fn player_linked_conversations(&self, role: &str) -> Vec<Value> {
        self.linked_conversations
            .iter()
            .filter(|c| c.has(role))
            .map(|c| {
                json!({"id": c.id, "delivery": c.delivery, "inject_id": c.inject_id,
                    "created_by": c.created_by, "participants": c.participants,
                    "turn": c.turn, "t_s": c.t_s, "quote": self.quote(role, c),
                    "reason": c.reason.for_role(role, &c.created_by),
                    // Responses given through this conversation, visible to all its participants.
                    "responses": self.proposals.iter()
                        .filter(|p| p.via_conversation == Some(c.id))
                        .map(|p| json!({"id": p.id, "by": p.source.label(), "status": p.status,
                            "narrative": p.narrative, "actions": p.commands.len(),
                            "turn": p.turn, "t_s": p.t_s}))
                        .collect::<Vec<_>>(),
                    "messages": c.messages.iter().map(|m| {
                        json!({"id": m.id, "from": m.from, "turn": m.turn, "clock": m.clock,
                            "text": m.texts.get(role).unwrap_or(&m.text_en),
                            "original": (m.from == role).then_some(&m.original),
                            "reply_to": m.reply_to, "translation_failed": m.translation_failed,
                            "proposal": m.proposal})
                    }).collect::<Vec<_>>()})
            })
            .collect()
    }

    /// Every conversation for EXCON, transcript included (EXCON is omnipotent;
    /// other players still cannot read it).
    pub fn master_linked_conversations(&self) -> Vec<Value> {
        self.linked_conversations
            .iter()
            .map(|c| {
                json!({"id": c.id, "delivery": c.delivery, "inject_id": c.inject_id,
                    "created_by": c.created_by, "participants": c.participants,
                    "turn": c.turn, "t_s": c.t_s, "message_count": c.messages.len(),
                    "reason": c.reason.for_master(),
                    "messages": c.messages.iter().map(|m| json!({"id": m.id, "from": m.from, "turn": m.turn,
                        "t_s": m.t_s, "clock": m.clock, "original": m.original, "text_en": m.text_en,
                        "reply_to": m.reply_to, "translation_failed": m.translation_failed, "proposal": m.proposal})).collect::<Vec<_>>(),
                    "responses": self.proposals.iter()
                        .filter(|p| p.via_conversation == Some(c.id))
                        .map(|p| p.id).collect::<Vec<_>>()})
            })
            .collect()
    }
}
