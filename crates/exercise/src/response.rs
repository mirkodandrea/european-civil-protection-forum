//! A player's response ("Rispondi"): optional narrative text plus optional
//! simulator actions, addressed to the fictitious sender EXCON plays or to
//! other players.
//!
//! One submission, one proposal, always stored in the inject's history (even
//! with no mechanical effect). Two kinds:
//! * a *communication* is information only: it is delivered at once
//!   (status `Informational`), needs no basis and no EXCON approval, and can
//!   carry no action;
//! * a *decision* (narrated, with or without one action) needs its basis
//!   (plan/procedure, law or operational judgement) and EXCON review. The
//!   narrative is a statement EXCON may answer with a narrative outcome; the
//!   commands still need EXCON approval and feasibility checks and are the
//!   only part that can touch the world.
//!
//! A narrative needs an inject to answer. A response addressed to players is
//! also posted in a conversation with them linked to the inject.
use crate::{BasisKind, DecisionBasis, DecisionEvidence, Exercise, PdfUpload, MsgKind, ParsedInput, Phase, ProposalStatus, MASTER};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim::Command;
use std::collections::BTreeMap;

/// What a response is: information, or a decision that needs EXCON review.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseKind {
    Communication,
    #[default]
    Decision,
}

/// Recipient of a response: the fictitious sender impersonated by EXCON
/// (`persona`), or real players. `conversation` is the linked conversation the
/// response was posted in when it went to players.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Addressee {
    pub persona: Option<String>,
    pub players: Vec<String>,
    pub conversation: Option<u64>,
}

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
    /// What the response rests on (plans, laws or operational judgement).
    pub basis: Option<BasisKind>,
    /// PDF of the plan or law the response cites; needs a plan/law basis.
    pub document: Option<PdfUpload>,
    /// Communication or decision (required by the API; a decision needs `basis`).
    pub kind: ResponseKind,
    /// Fictitious sender the response answers (default: the inject's sender).
    pub to_persona: Option<String>,
    /// Real players the response goes to instead of the fictitious sender.
    pub to_players: Vec<String>,
    /// Text of the narrative in each recipient player's language (missing:
    /// the English working copy).
    pub texts: BTreeMap<String, String>,
}

impl PlayerResponse {
    fn narrative_text(&self) -> Option<&str> {
        self.narrative.as_deref().filter(|t| !t.trim().is_empty())
    }
    pub fn is_communication(&self) -> bool {
        self.kind == ResponseKind::Communication
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
        if let Some(document) = &r.document {
            if !matches!(
                r.basis,
                Some(BasisKind::Plan | BasisKind::Law | BasisKind::Procedure)
            ) {
                bail!("a PDF can be attached only when the response rests on plans or laws");
            }
            self.validate_document(role, document)?;
        }
        let narrative = r.narrative_text();
        if narrative.is_none() && r.commands.is_empty() {
            bail!("a response needs a narrative or at least one action");
        }
        match r.kind {
            ResponseKind::Communication => {
                if !r.commands.is_empty() {
                    bail!("a communication carries no simulator action: send it as a decision");
                }
                if narrative.is_none() {
                    bail!("a communication needs a text");
                }
            }
            // The basis is required for every decision, even a narrated one.
            ResponseKind::Decision if r.basis.is_none() => {
                bail!("a decision needs its basis: plan or procedure, law or operational judgement");
            }
            _ => {}
        }
        if r.to_persona.is_some() && !r.to_players.is_empty() {
            bail!("address the response to the fictitious sender or to players, not both");
        }
        if r.to_persona.as_ref().is_some_and(|p| p.trim().is_empty() || p.chars().count() > 120) {
            bail!("the recipient name needs 1–120 characters");
        }
        if !r.to_players.is_empty() {
            if narrative.is_none() {
                bail!("a response to players needs a text");
            }
            if r.to_players.len() > crate::conversations::MAX_PARTICIPANTS {
                bail!("address at most {} players", crate::conversations::MAX_PARTICIPANTS);
            }
            for (i, p) in r.to_players.iter().enumerate() {
                if p == role || p == MASTER || !self.players.contains_key(p) || r.to_players[..i].contains(p) {
                    bail!("invalid recipient {p}");
                }
            }
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
            if narrative.is_some() || r.conversation.is_some() || !r.to_players.is_empty() {
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
        if !r.to_players.is_empty() {
            match r.conversation.and_then(|cid| self.linked_conversation(cid)) {
                // From a conversation: only its participants can be addressed.
                Some(c) if r.to_players.iter().any(|p| !c.has(p)) => {
                    bail!("from a conversation, address only its participants");
                }
                Some(_) => {}
                None if parent.kind != MsgKind::Inject || parent.to != role => {
                    bail!("address players only when answering an inject you received");
                }
                None => {}
            }
        }
        if self.clarification_target(role, reply_to).is_some() {
            bail!("this answers an open question: send it as a text answer, or create a new action without replying to the question");
        }
        Ok(())
    }

    pub fn player_response(&mut self, role: &str, mut r: PlayerResponse) -> Result<u64> {
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
        let evidence = match r.basis {
            Some(kind) => Some(DecisionEvidence {
                basis: DecisionBasis {
                    kind,
                    reference: match &r.document {
                        Some(d) => format!("Declared by the player: {}; document {}", basis_label(kind), d.name),
                        None => format!("Declared by the player: {}", basis_label(kind)),
                    },
                },
                attachments: Vec::new(),
            }),
            // Only a communication has no basis (validated above).
            None => None,
        };
        let communication = r.is_communication();
        // Default recipient: the fictitious sender of the inject being answered.
        let addressed_to = Addressee {
            persona: if r.to_players.is_empty() {
                r.to_persona.clone().map(|p| p.trim().to_string()).or_else(|| {
                    r.reply_to
                        .and_then(|id| self.messages.iter().find(|m| m.id == id))
                        .filter(|m| m.kind == MsgKind::Inject)
                        .map(|m| m.persona.clone().unwrap_or_else(|| m.from.clone()))
                })
            } else {
                None
            },
            players: r.to_players.clone(),
            conversation: None,
        };
        let pid = self.open_chat_proposal(role, &original, inject_id, r.reply_to, evidence)?;
        let document = match r.document.take() {
            Some(upload) => Some(self.store_document(role, pid, upload)?),
            None => None,
        };
        {
            let p = self.proposal_mut(pid)?;
            p.thread = thread;
            p.via_conversation = r.conversation;
            p.communication = communication;
            p.addressed_to = addressed_to;
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
            if communication {
                // Delivered as it is: nothing to approve, the flow is not blocked.
                self.transition(
                    pid,
                    ProposalStatus::Informational,
                    role,
                    "Communication delivered; no EXCON approval needed",
                )?;
            } else {
                self.transition(
                    pid,
                    ProposalStatus::Pending,
                    role,
                    "Narrative decision submitted for EXCON review",
                )?;
            }
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
            let mut note = if r.commands.is_empty() {
                original.clone()
            } else {
                format!("{original}\n[actions] {actions_text}")
            };
            // The AI context keeps what the player said the decision rests on.
            if let Some(kind) = r.basis {
                note.push_str(&format!("\n[basis] {}", basis_label(kind)));
                if let Some(d) = &document {
                    note.push_str(&format!("; attached PDF: {}", d.name));
                }
            }
            self.converse(role, "user", note);
            self.record(
                "narrative_submitted",
                role,
                json!({"proposal":pid,"message":mid,"inject_id":inject_id,"translation_failed":r.translation_failed,"actions":r.commands.len(),"conversation":r.conversation,"basis":r.basis,"document":document}),
            );
        }
        if !r.to_players.is_empty() {
            let cid = self.deliver_to_players(role, pid, &r, &original, &english)?;
            let p = self.proposal_mut(pid)?;
            p.addressed_to.conversation = Some(cid);
            p.via_conversation.get_or_insert(cid);
        }
        self.record(
            "response_recorded",
            role,
            json!({"proposal":pid,"message":mid,"inject_id":inject_id,
                "kind": if communication { "communication" } else { "decision" },
                "addressed_to": self.proposal(pid).map(|p| p.addressed_to.clone()),
                "actions": r.commands.len(), "text": original}),
        );
        if let Some(cid) = r.conversation {
            self.record(
                "conversation_response",
                role,
                json!({"proposal":pid,"conversation":cid,"inject_id":inject_id}),
            );
        }
        Ok(pid)
    }

    /// Post a response addressed to players in the conversation linked to the
    /// inject: the one it was written from, an existing one with exactly these
    /// participants, or a new one. Returns the conversation id.
    fn deliver_to_players(
        &mut self,
        role: &str,
        pid: u64,
        r: &PlayerResponse,
        text: &str,
        text_en: &str,
    ) -> Result<u64> {
        let parent = r.reply_to.expect("validated reply_to");
        let mut wanted: Vec<&str> = r.to_players.iter().map(String::as_str).collect();
        wanted.push(role);
        wanted.sort_unstable();
        let existing = r.conversation.or_else(|| {
            self.linked_conversations
                .iter()
                .rev()
                .find(|c| {
                    let mut have: Vec<&str> = c.participants.iter().map(String::as_str).collect();
                    have.sort_unstable();
                    c.delivery == parent && have == wanted
                })
                .map(|c| c.id)
        });
        let (cid, mid) = match existing {
            Some(cid) => {
                let mid = self.push_conversation_message(
                    cid, role, text, text_en, &r.texts, Some(parent), r.translation_failed,
                );
                (cid, mid)
            }
            None => self.start_linked_conversation(
                role, parent, &r.to_players, text, text_en, &r.texts, r.translation_failed,
            )?,
        };
        if let Some(m) = self
            .linked_conversations
            .iter_mut()
            .find(|c| c.id == cid)
            .and_then(|c| c.messages.iter_mut().find(|m| m.id == mid))
        {
            m.proposal = Some(pid);
        }
        Ok(cid)
    }
}

/// English wording of a basis for logs and the AI context.
pub fn basis_label(kind: BasisKind) -> &'static str {
    match kind {
        BasisKind::Plan => "plans",
        BasisKind::Law => "laws",
        BasisKind::Procedure => "procedures",
        BasisKind::OperationalJudgment => "operational judgement",
    }
}
