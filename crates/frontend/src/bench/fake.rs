//! In-memory stand-in for the exercise server, just large enough for the
//! workflow benches: injects, responses, decisions, clarification threads and
//! linked conversations of one small scenario. It answers the same paths and
//! JSON shapes the components send and read; there is no simulator, no
//! exercise crate and no language model (AI answers are fixed constants).
use serde_json::{json, Value};

pub const MASTER: &str = "master";
const PERSONA: &str = "Sala operativa";

#[derive(Clone)]
pub struct Role {
    pub id: &'static str,
    pub name: &'static str,
    pub allowed: &'static [&'static str],
}

pub const ROLES: &[Role] = &[
    Role {
        id: "mayor",
        name: "Sindaco di Valverde",
        allowed: &["IssueWarning", "EvacuateZone", "OpenShelter"],
    },
    Role {
        id: "police",
        name: "Polizia locale",
        allowed: &["CloseRoad", "IssueWarning"],
    },
];

pub fn role_name(id: &str) -> &str {
    ROLES
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.name)
        .unwrap_or(id)
}

/// Canned AI suggestions, by role: what a model would propose for the bench narratives.
pub fn canned_suggestions(role: &str) -> Vec<(Value, &'static str)> {
    match role {
        "police" => vec![
            (
                json!({"type":"CloseRoad","road":"sp12"}),
                "La narrativa chiude la SP12 al traffico civile.",
            ),
            (
                json!({"type":"IssueWarning","zone":"E3"}),
                "Avvisare la zona prima di deviare il traffico.",
            ),
        ],
        _ => vec![
            (
                json!({"type":"OpenShelter","building":"Palestra comunale"}),
                "La narrativa apre la palestra come area di accoglienza.",
            ),
            (
                json!({"type":"IssueWarning","zone":"F4"}),
                "La narrativa chiede di avvisare la popolazione di F4.",
            ),
        ],
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: u64,
    /// inject | reply | clarification
    pub kind: &'static str,
    pub from: String,
    pub to: String,
    pub title: String,
    pub text: String,
    pub t_s: i64,
    /// Delivery the message belongs to.
    pub delivery: Option<u64>,
    pub proposal: Option<u64>,
    pub reply_to: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u64,
    pub role: String,
    pub original: String,
    pub narrative: bool,
    pub carries_actions: bool,
    pub commands: Vec<Value>,
    /// pending | needs_clarification | approved | rejected | applied
    pub status: &'static str,
    /// "" until decided: approved | partially_approved | rejected | acknowledged
    pub review: &'static str,
    pub reply_to: Option<u64>,
    pub delivery: Option<u64>,
    pub via_conversation: Option<u64>,
    pub decision: Option<Value>,
    pub clarification_request: Option<String>,
    pub outcomes: Vec<Value>,
    pub t_s: i64,
    /// What the player said the reply rests on, its reference (plan or
    /// law) and why (rationale).
    pub basis: Option<String>,
    pub basis_note: Option<String>,
    pub rationale: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub id: u64,
    pub role: String,
    pub delivery: Option<u64>,
    pub proposal: u64,
    /// awaiting_player | awaiting_excon | closed
    pub status: &'static str,
    /// (author "excon"|"player", text, message id, t_s)
    pub entries: Vec<(&'static str, String, u64, i64)>,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: u64,
    /// The inject message it quotes (as delivered to its creator).
    pub quote: u64,
    pub delivery: u64,
    pub participants: Vec<String>,
    /// Why the creator started it: basis, reference (plan or law), rationale.
    pub reason: (String, Option<String>, String),
    /// (id, from, text, reply_to, t_s)
    pub messages: Vec<(u64, String, String, Option<u64>, i64)>,
}

/// Same rule as `exercise::conversations::ConversationReason::new`: starting
/// a conversation needs a basis (plan or procedure, law, or personal
/// initiative), the reference of a plan or law, and a rationale.
pub fn conversation_reason(b: &Value) -> Result<(String, Option<String>, String), String> {
    let filled = |k: &str| b[k].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
    let basis = b["basis"].as_str().filter(|v| ["plan", "procedure", "law", "operational_judgment"].contains(v))
        .ok_or("say why you start this conversation: a plan or procedure, a law or regulation, or personal initiative")?;
    let reference = filled("basis_note").filter(|_| basis != "operational_judgment");
    if basis != "operational_judgment" && reference.is_none() {
        return Err("say which plan, procedure or law the conversation rests on (e.g. the plan's section or the article)".into());
    }
    let rationale = filled("rationale").ok_or("starting a conversation needs its rationale: why you start it")?;
    Ok((basis.to_string(), reference, rationale))
}

#[derive(Clone, Debug)]
pub struct Planned {
    pub id: &'static str,
    pub title: &'static str,
    pub text: &'static str,
    pub recipients: Vec<&'static str>,
    pub sent_to: Vec<String>,
}

/// What happened on the fake server (the step checks of the benches read it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace {
    pub excon_suggestions: u32,
    pub rechecks: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Fake {
    pub lang: String,
    pub messages: Vec<Message>,
    pub proposals: Vec<Proposal>,
    pub threads: Vec<Thread>,
    pub conversations: Vec<Conversation>,
    pub planned: Vec<Planned>,
    pub trace: Trace,
    next_id: u64,
    pub now_s: i64,
    pub generation: u64,
}

/// `Giorno 1 HH:MM` of a scenario second (scenario start 13:00).
pub fn clock(t_s: i64) -> String {
    let m = 13 * 60 + t_s / 60;
    format!("Giorno 1 {:02}:{:02}", m / 60 % 24, m % 60)
}

fn body_text(body: &Value, key: &str) -> String {
    body[key].as_str().unwrap_or("").trim().to_string()
}

impl Fake {
    pub fn new(lang: &str) -> Self {
        Fake {
            lang: lang.into(),
            next_id: 1,
            ..Default::default()
        }
    }
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    /// Every change takes a simulated minute, so the clock shows the order.
    fn tick(&mut self) -> i64 {
        self.now_s += 60;
        self.generation += 1;
        self.now_s
    }

    // ------------------------------------------------------------ seeding

    pub fn plan(
        &mut self,
        id: &'static str,
        title: &'static str,
        text: &'static str,
        recipients: &[&'static str],
    ) {
        self.planned.push(Planned {
            id,
            title,
            text,
            recipients: recipients.to_vec(),
            sent_to: Vec::new(),
        });
    }
    /// Deliver planned inject `id` to `recipients`; returns the delivery id.
    pub fn send(&mut self, id: &str, recipients: &[String]) -> Result<u64, String> {
        let p = self
            .planned
            .iter()
            .position(|p| p.id == id)
            .ok_or("inject sconosciuto")?;
        if recipients.is_empty() {
            return Err("nessun destinatario".into());
        }
        let t = self.tick();
        let delivery = self.id();
        let (title, text) = (
            self.planned[p].title.to_string(),
            self.planned[p].text.to_string(),
        );
        for r in recipients {
            let mid = self.id();
            self.messages.push(Message {
                id: mid,
                kind: "inject",
                from: MASTER.into(),
                to: r.clone(),
                title: title.clone(),
                text: text.clone(),
                t_s: t,
                delivery: Some(delivery),
                proposal: None,
                reply_to: None,
            });
            self.planned[p].sent_to.push(r.clone());
        }
        Ok(delivery)
    }
    /// The inject message `role` received for delivery `delivery`.
    pub fn delivered(&self, role: &str, delivery: u64) -> Option<&Message> {
        self.messages
            .iter()
            .find(|m| m.kind == "inject" && m.to == role && m.delivery == Some(delivery))
    }
    /// A response of `role` to message `reply_to` (as `POST /api/player/<role>/actions`).
    pub fn respond(
        &mut self,
        role: &str,
        narrative: &str,
        commands: Vec<Value>,
        reply_to: Option<u64>,
        conversation: Option<u64>,
    ) -> Result<u64, String> {
        if narrative.trim().is_empty() && commands.is_empty() {
            return Err("risposta vuota".into());
        }
        let parent = reply_to.and_then(|id| self.messages.iter().find(|m| m.id == id).cloned());
        if !narrative.trim().is_empty() && !parent.as_ref().is_some_and(|m| m.kind == "inject") {
            return Err("una narrativa risponde a un inject".into());
        }
        let t = self.tick();
        let pid = self.id();
        let delivery = parent.as_ref().and_then(|m| m.delivery);
        let original = if narrative.trim().is_empty() {
            commands
                .iter()
                .map(|c| crate::widgets::command_summary("it", c))
                .collect::<Vec<_>>()
                .join("; ")
        } else {
            narrative.trim().to_string()
        };
        // A newer response replaces the open one to the same inject.
        for p in self.proposals.iter_mut().filter(|p| {
            p.role == role && p.delivery == delivery && delivery.is_some() && p.status == "pending"
        }) {
            p.status = "superseded";
        }
        self.proposals.push(Proposal {
            id: pid,
            role: role.into(),
            original: original.clone(),
            narrative: !narrative.trim().is_empty(),
            carries_actions: !commands.is_empty(),
            commands,
            status: "pending",
            review: "",
            reply_to,
            delivery,
            via_conversation: conversation,
            decision: None,
            clarification_request: None,
            outcomes: Vec::new(),
            t_s: t,
            basis: None,
            basis_note: None,
            rationale: None,
        });
        let mid = self.id();
        self.messages.push(Message {
            id: mid,
            kind: "reply",
            from: role.into(),
            to: MASTER.into(),
            title: String::new(),
            text: original,
            t_s: t,
            delivery,
            proposal: Some(pid),
            reply_to,
        });
        Ok(pid)
    }
    fn proposal_mut(&mut self, id: u64) -> Result<&mut Proposal, String> {
        self.proposals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("proposta {id} sconosciuta"))
    }
    pub fn proposal(&self, id: u64) -> Option<&Proposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    // ------------------------------------------------------------ requests

    /// One request as the server would answer it.
    pub fn handle(&mut self, who: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let (path, _) = path.split_once('?').unwrap_or((path, ""));
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let b = body.clone().unwrap_or(Value::Null);
        let master = who == MASTER;
        let id = |s: &str| s.parse::<u64>().map_err(|_| "id non valido".to_string());
        match (body.is_some(), parts.as_slice()) {
            (false, ["api", "static"]) => Ok(self.stat()),
            (false, ["api", "state"]) => Ok(self.excon_state()),
            (false, ["api", "master"]) => Ok(self.master()),
            (false, ["api", "player", role]) => Ok(self.player(role)),
            (true, ["api", "master", "msel", inject, "send"]) if master => {
                let recipients: Vec<String> = b["recipients"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.as_str().map(String::from))
                    .collect();
                let delivery = self.send(inject, &recipients)?;
                Ok(json!({"delivery": delivery}))
            }
            (true, ["api", "player", role, "suggest-actions"]) if master || who == *role => {
                Ok(self.suggestions(role, &[]))
            }
            (true, ["api", "player", role, "actions"]) if who == *role => {
                let commands = b["commands"].as_array().cloned().unwrap_or_default();
                // Same rules as `exercise::response`: a communication carries
                // no action and needs no approval; a decision needs its basis.
                let communication = b["kind"] == "communication";
                if communication && !commands.is_empty() {
                    return Err("una comunicazione non porta azioni".into());
                }
                if b["kind"] != "communication" && b["kind"] != "decision" {
                    return Err("kind richiesto: communication o decision".into());
                }
                if b["kind"] == "decision" && !b["basis"].is_string() {
                    return Err("una decisione richiede il fondamento".into());
                }
                // A narrated reply to the sender: basis, reference of a plan
                // or law, and rationale are required.
                let filled = |k: &str| b[k].as_str().is_some_and(|t| !t.trim().is_empty());
                let to_sender = b["to_players"].as_array().is_none_or(|p| p.is_empty());
                if to_sender && filled("narrative") {
                    let basis = b["basis"].as_str().unwrap_or("");
                    if !["plan", "procedure", "law", "operational_judgment"].contains(&basis) {
                        return Err("a reply to the sender needs its basis: plan or procedure, law or regulation, or operational judgement".into());
                    }
                    if basis != "operational_judgment" && !filled("basis_note") {
                        return Err("say which plan, procedure or law the reply rests on (e.g. the plan's section or the article)".into());
                    }
                    if !filled("rationale") {
                        return Err("a reply to the sender needs its rationale: why this decision".into());
                    }
                }
                let pid = self.respond(
                    role,
                    &body_text(&b, "narrative"),
                    commands,
                    b["reply_to"].as_u64(),
                    b["conversation"].as_u64(),
                )?;
                let p = self.proposal_mut(pid)?;
                if communication {
                    p.status = "informational";
                }
                p.basis = b["basis"].as_str().map(String::from);
                p.basis_note = b["basis_note"].as_str().filter(|n| !n.trim().is_empty()).map(String::from);
                p.rationale = b["rationale"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from);
                Ok(json!({"proposal": pid, "translation_failed": false}))
            }
            (true, ["api", "player", role, "reply"]) if who == *role => {
                self.answer(role, &body_text(&b, "text"), b["reply_to"].as_u64())
            }
            (true, ["api", "player", role, "conversations"]) if who == *role => {
                let quote = b["delivery"].as_u64().ok_or("inject mancante")?;
                let delivery = self
                    .messages
                    .iter()
                    .find(|m| m.id == quote && m.to == *role)
                    .and_then(|m| m.delivery)
                    .ok_or("inject non ricevuto")?;
                let text = body_text(&b, "text");
                if text.is_empty() {
                    return Err("messaggio vuoto".into());
                }
                let reason = conversation_reason(&b)?;
                let mut participants = vec![role.to_string()];
                participants.extend(
                    b["participants"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| p.as_str().map(String::from))
                        .filter(|p| p != role),
                );
                let t = self.tick();
                let (cid, mid) = (self.id(), self.id());
                self.conversations.push(Conversation {
                    id: cid,
                    quote,
                    delivery,
                    participants,
                    reason,
                    messages: vec![(mid, role.to_string(), text, None, t)],
                });
                Ok(json!({"conversation": cid}))
            }
            (true, ["api", "player", role, "conversations", conv, "messages"]) if who == *role => {
                let cid = id(conv)?;
                let text = body_text(&b, "text");
                if text.is_empty() {
                    return Err("messaggio vuoto".into());
                }
                let t = self.tick();
                let mid = self.id();
                let c = self
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == cid && c.participants.iter().any(|p| p == role))
                    .ok_or("conversazione sconosciuta")?;
                c.messages
                    .push((mid, role.to_string(), text, b["reply_to"].as_u64(), t));
                Ok(json!({"message": mid}))
            }
            (true, ["api", "master", "proposals", pid, "suggest-actions"]) if master => {
                let pid = id(pid)?;
                let p = self.proposal(pid).ok_or("proposta sconosciuta")?.clone();
                self.trace.excon_suggestions += 1;
                let mut out = self.suggestions(&p.role, &p.commands);
                out["proposal"] = json!(pid);
                Ok(out)
            }
            (true, ["api", "master", "proposals", pid, "revalidate"]) if master => {
                let pid = self.proposal(id(pid)?).ok_or("proposta sconosciuta")?.id;
                self.trace.rechecks += 1;
                let commands = b["commands"].as_array().cloned().unwrap_or_default();
                Ok(json!({"id": pid, "commands": commands, "verdicts": verdicts(&commands)}))
            }
            (true, ["api", "master", "proposals", pid, "decide"]) if master => {
                self.decide(id(pid)?, &b)
            }
            (true, ["api", "master", "proposals", pid, "clarify"]) if master => {
                self.clarify(id(pid)?, &body_text(&b, "note"))
            }
            (true, ["api", "master", "proposals", pid, "execute"]) if master => {
                let t = self.tick();
                let p = self.proposal_mut(id(pid)?)?;
                if p.status != "approved" {
                    return Err("solo una proposta approvata si esegue".into());
                }
                p.status = "applied";
                p.t_s = t;
                Ok(json!({"ok": true}))
            }
            (true, ["api", "master", "proposals", pid, "narrative-outcome"]) if master => {
                let t = self.tick();
                let text = body_text(&b, "text");
                let persona = b["persona"].as_str().map(String::from);
                let p = self.proposal_mut(id(pid)?)?;
                if text.is_empty() && b["commands"].as_array().is_none_or(Vec::is_empty) {
                    return Err("esito vuoto".into());
                }
                p.outcomes
                    .push(json!({"text": text, "persona": persona, "t_s": t, "interventions": []}));
                Ok(json!({"message": 0, "translation_failed": false, "interventions": []}))
            }
            (true, ["api", "master", "clarifications", tid, "close"]) if master => {
                let tid = id(tid)?;
                self.tick();
                let t = self
                    .threads
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("chiarimento sconosciuto")?;
                t.status = "closed";
                let pid = t.proposal;
                // Closing without an answer leaves the proposal to decide as it is.
                if let Ok(p) = self.proposal_mut(pid) {
                    if p.status == "needs_clarification" {
                        p.status = "pending";
                        p.clarification_request = None;
                    }
                }
                Ok(json!({"ok": true}))
            }
            _ => Err(format!("Banco di prova: {path} non è simulato")),
        }
    }

    fn suggestions(&self, role: &str, already: &[Value]) -> Value {
        let actions: Vec<Value> = canned_suggestions(role)
            .into_iter()
            .filter(|(c, _)| !already.contains(c))
            .map(|(command, reason)| json!({"command": command, "reason": reason, "verdict": {"verdict": "ok"}}))
            .collect();
        json!({"status": if actions.is_empty() {"none"} else {"ready"}, "role": role, "actions": actions, "dropped": 0,
            "summary": "Suggerimenti di prova (AI simulata)."})
    }

    fn decide(&mut self, pid: u64, b: &Value) -> Result<Value, String> {
        let approve = b["approve"] == true;
        let note = body_text(b, "note");
        let t = self.tick();
        let p = self.proposal_mut(pid)?;
        if !["pending", "needs_clarification"].contains(&p.status) {
            return Err("proposta già decisa".into());
        }
        if !approve && note.is_empty() {
            return Err("per rifiutare serve una nota".into());
        }
        let commands = b["commands"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| p.commands.clone());
        let skipped: Vec<(usize, String)> = b["rejected_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some((
                    r["index"].as_u64()? as usize,
                    r["note"].as_str().unwrap_or("").to_string(),
                ))
            })
            .collect();
        let rejected_actions: Vec<Value> = skipped
            .iter()
            .filter_map(|(i, n)| {
                commands
                    .get(*i)
                    .map(|c| json!({"index": i, "command": c, "note": n}))
            })
            .collect();
        let kept: Vec<Value> = commands
            .iter()
            .enumerate()
            .filter(|(i, _)| !skipped.iter().any(|(s, _)| s == i))
            .map(|(_, c)| c.clone())
            .collect();
        let narrative_only = p.narrative && kept.is_empty();
        p.review = if !approve {
            "rejected"
        } else if !rejected_actions.is_empty() {
            "partially_approved"
        } else if kept.is_empty() && !narrative_only {
            "acknowledged"
        } else {
            "approved"
        };
        p.status = if approve { "approved" } else { "rejected" };
        if approve {
            p.carries_actions = !kept.is_empty();
            p.commands = kept.clone();
        }
        p.clarification_request = None;
        p.decision = Some(
            json!({"approve": approve, "review": p.review, "note": note, "t_s": t, "persona": b["persona"].as_str().unwrap_or(PERSONA),
            "rejected_actions": rejected_actions, "effective_commands": kept}),
        );
        Ok(json!({"ok": true, "proposal": pid}))
    }

    fn clarify(&mut self, pid: u64, note: &str) -> Result<Value, String> {
        if note.is_empty() {
            return Err("scrivi la domanda per il player".into());
        }
        let t = self.tick();
        let mid = self.id();
        let p = self.proposal_mut(pid)?;
        if !["pending", "needs_clarification"].contains(&p.status) {
            return Err("proposta già decisa".into());
        }
        p.status = "needs_clarification";
        p.clarification_request = Some(note.into());
        let (role, delivery) = (p.role.clone(), p.delivery);
        self.messages.push(Message {
            id: mid,
            kind: "clarification",
            from: MASTER.into(),
            to: role.clone(),
            title: String::new(),
            text: note.into(),
            t_s: t,
            delivery,
            proposal: Some(pid),
            reply_to: None,
        });
        match self
            .threads
            .iter_mut()
            .find(|t| t.proposal == pid && t.status != "closed")
        {
            Some(th) => {
                th.status = "awaiting_player";
                th.entries.push(("excon", note.into(), mid, t));
            }
            None => {
                // As on the server, the thread opens with the response it is about.
                let opening = self
                    .messages
                    .iter()
                    .find(|m| m.kind == "reply" && m.proposal == Some(pid))
                    .map(|m| ("player", m.text.clone(), m.id, m.t_s));
                let tid = self.id();
                let entries = opening
                    .into_iter()
                    .chain([("excon", note.to_string(), mid, t)])
                    .collect();
                self.threads.push(Thread {
                    id: tid,
                    role,
                    delivery,
                    proposal: pid,
                    status: "awaiting_player",
                    entries,
                });
            }
        }
        Ok(json!({"ok": true}))
    }

    /// The player's text answer to EXCON's latest question (`reply_to`).
    fn answer(&mut self, role: &str, text: &str, reply_to: Option<u64>) -> Result<Value, String> {
        if text.is_empty() {
            return Err("risposta vuota".into());
        }
        let t = self.tick();
        let mid = self.id();
        let th = self
            .threads
            .iter_mut()
            .find(|th| {
                th.role == role
                    && th.status == "awaiting_player"
                    && th
                        .entries
                        .iter()
                        .any(|e| e.0 == "excon" && Some(e.2) == reply_to)
            })
            .ok_or("nessun chiarimento aperto per questo messaggio")?;
        th.status = "awaiting_excon";
        th.entries.push(("player", text.into(), mid, t));
        let (pid, delivery) = (th.proposal, th.delivery);
        self.messages.push(Message {
            id: mid,
            kind: "reply",
            from: role.into(),
            to: MASTER.into(),
            title: String::new(),
            text: text.into(),
            t_s: t,
            delivery,
            proposal: Some(pid),
            reply_to,
        });
        if let Ok(p) = self.proposal_mut(pid) {
            p.status = "pending";
        }
        Ok(json!({"ok": true, "message": mid}))
    }

    // ------------------------------------------------------------ views

    pub fn stat(&self) -> Value {
        json!({
            "name": "Banco di prova · Valverde",
            "roles": ROLES.iter().map(|r| json!({"id": r.id, "name": r.name})).collect::<Vec<_>>(),
            "contacts": [],
            "personas": {"roles": ROLES.iter().map(|r| json!({"id": r.id, "name": r.name})).collect::<Vec<_>>(),
                "senders": [PERSONA, "112 Dispatch"], "default": PERSONA, "default_en": "Operations room"},
            "resource_types": [],
            "zones": {"cols": 8, "rows": 8},
            "buildings": [{"id": 1, "name": "Palestra comunale"}, {"id": 2, "name": "Scuola primaria Pineta"}],
            "roads": [{"id": "sp12", "name": "SP12 Valverde–Pineta"}],
            "places": [],
            "msel": [],
        })
    }
    pub fn excon_state(&self) -> Value {
        json!({"generation": self.generation, "phase": "orders", "turn": 1, "clock": clock(self.now_s), "time_s": self.now_s, "world": {"weather": {}}})
    }

    fn proposal_json(&self, p: &Proposal, for_master: bool) -> Value {
        let mut v = json!({
            "id": p.id, "turn": 1, "t_s": p.t_s, "status": p.status, "review": p.review,
            "original": p.original, "inject_id": p.delivery, "reply_to": p.reply_to,
            "narrative": p.narrative, "carries_actions": p.carries_actions, "commands": p.commands,
            "via_conversation": p.via_conversation, "clarification_request": p.clarification_request,
            "decision": p.decision, "executions": [], "history": [],
            "narrative_outcomes": p.outcomes.iter().map(|o| { let mut o = o.clone(); if o["persona"].is_null() { o["persona"] = json!(PERSONA); } o["effects"] = json!([]); o }).collect::<Vec<_>>(),
        });
        if for_master {
            v["source"] = json!({"kind": "player", "role": p.role});
            v["translation_en"] = json!("");
            v["summary"] = json!(if p.narrative {
                "Iniziativa narrativa del player"
            } else {
                "Azioni proposte dal player"
            });
            v["verdicts"] = json!(verdicts(&p.commands));
            v["thread"] = json!([]);
        }
        v
    }
    fn message_json(&self, m: &Message) -> Value {
        let clarification = self
            .threads
            .iter()
            .find(|t| t.entries.iter().any(|e| e.2 == m.id))
            .map(|t| t.id);
        // What a reply rests on, as the server's thread projection says.
        let basis = (m.kind == "reply").then(|| m.proposal.and_then(|pid| self.proposals.iter().find(|p| p.id == pid))).flatten();
        json!({"id": m.id, "kind": m.kind, "from": m.from, "to": m.to, "turn": 1, "t_s": m.t_s, "clock": clock(m.t_s),
            "title": m.title, "text": m.text, "inject_id": m.delivery, "reply_to": m.reply_to,
            "clarification_thread": clarification,
            "basis": basis.as_ref().map(|p| p.basis.clone()), "basis_note": basis.and_then(|p| p.basis_note.clone()),
            "rationale": basis.and_then(|p| p.rationale.clone())})
    }
    fn thread_json(&self, t: &Thread) -> Value {
        json!({"id": t.id, "role": t.role, "inject_id": t.delivery, "status": t.status, "current_proposal": t.proposal,
            "messages": t.entries.iter().map(|e| e.2).collect::<Vec<_>>(),
            "entries": t.entries.iter().map(|(author, text, message, t_s)| json!({"author": author, "text": text, "message": message,
                "proposal": t.proposal, "clock": clock(*t_s), "persona": if *author == "excon" { json!(PERSONA) } else { Value::Null }})).collect::<Vec<_>>()})
    }
    fn conversation_json(&self, c: &Conversation, viewer: Option<&str>) -> Value {
        // A participant who never received the inject reads its title only.
        let own = viewer.and_then(|r| self.delivered(r, c.delivery));
        let quote = own
            .or_else(|| self.messages.iter().find(|m| m.id == c.quote))
            .map(|m| {
                let mut q = self.message_json(m);
                let title_only = viewer.is_some() && own.is_none();
                if title_only {
                    q["text"] = Value::Null;
                }
                q["title_only"] = json!(title_only);
                q
            })
            .unwrap_or(Value::Null);
        let (basis, reference, rationale) = &c.reason;
        let messages: Vec<Value> = c.messages.iter().map(|(id, from, text, reply_to, t_s)| json!({"id": id, "from": from, "turn": 1, "t_s": t_s,
            "clock": clock(*t_s), "text": text, "text_en": text, "original": text, "reply_to": reply_to})).collect();
        let responses: Vec<Value> = self.proposals.iter().filter(|p| p.via_conversation == Some(c.id))
            .map(|p| json!({"id": p.id, "by": p.role, "status": p.status, "narrative": p.narrative, "actions": p.commands.len()})).collect();
        json!({"id": c.id, "delivery": c.quote, "inject_id": c.delivery, "created_by": c.participants[0], "participants": c.participants,
            "turn": 1, "t_s": c.messages.first().map(|m| m.4).unwrap_or(0), "quote": quote, "messages": messages, "message_count": c.messages.len(),
            "reason": {"basis": basis, "reference": reference, "rationale": rationale, "rationale_en": rationale},
            "responses": if viewer.is_some() { json!(responses) } else { json!(responses.iter().map(|r| r["id"].clone()).collect::<Vec<_>>()) }})
    }

    /// `/api/player/<role>`.
    pub fn player(&self, role: &str) -> Value {
        let spec = ROLES.iter().find(|r| r.id == role);
        let inbox: Vec<Value> = self
            .messages
            .iter()
            .filter(|m| m.to == role || m.from == role)
            .map(|m| {
                let mut v = self.message_json(m);
                if m.from == MASTER {
                    v["persona"] = json!(PERSONA);
                }
                v
            })
            .collect();
        json!({
            "phase": "orders", "turn": 1, "clock": clock(self.now_s), "time_s": self.now_s,
            "player": {"role": role, "language": self.lang, "language_confirmed": true},
            "default_persona": PERSONA,
            "role": {"id": role, "name": role_name(role), "allowed_actions": spec.map(|s| s.allowed.to_vec()).unwrap_or_default()},
            "inbox": inbox,
            "proposals": self.proposals.iter().filter(|p| p.role == role).map(|p| self.proposal_json(p, false)).collect::<Vec<_>>(),
            "clarification_threads": self.threads.iter().filter(|t| t.role == role).map(|t| self.thread_json(t)).collect::<Vec<_>>(),
            "linked_conversations": self.conversations.iter().filter(|c| c.participants.iter().any(|p| p == role)).map(|c| self.conversation_json(c, Some(role))).collect::<Vec<_>>(),
            "resources": {}, "resource_accounting": {}, "deployments": [], "map": {},
        })
    }

    /// `/api/master`.
    pub fn master(&self) -> Value {
        let responses: Vec<Value> = self.messages.iter().filter(|m| m.kind == "inject").map(|m| {
            let mine = |p: &&Proposal| p.delivery == m.delivery && (p.role == m.to || self.conversations.iter().any(|c| Some(c.id) == p.via_conversation && c.participants.contains(&m.to) && c.quote == m.id));
            let replies: Vec<Value> = self.messages.iter().filter(|r| r.kind == "reply" && r.from == m.to && r.delivery == m.delivery)
                .map(|r| json!({"id": r.id, "proposal": r.proposal, "text": r.text, "original": r.text, "clock": clock(r.t_s), "reply_to": r.reply_to,
                    "basis": r.proposal.and_then(|pid| self.proposals.iter().find(|p| p.id == pid)).map(|p| p.basis.clone().unwrap_or_else(|| "operational_judgment".into())),
                    "basis_note": r.proposal.and_then(|pid| self.proposals.iter().find(|p| p.id == pid)).and_then(|p| p.basis_note.clone()),
                    "rationale": r.proposal.and_then(|pid| self.proposals.iter().find(|p| p.id == pid)).and_then(|p| p.rationale.clone())})).collect();
            let pending: Vec<Value> = self.proposals.iter().filter(mine).filter(|p| ["pending", "needs_clarification"].contains(&p.status))
                .map(|p| json!({"id": p.id, "status": p.status, "summary": p.original, "original": p.original, "role": p.role,
                    "narrative": p.narrative, "carries_actions": p.carries_actions,
                    "awaiting_clarification": p.status == "needs_clarification" && p.clarification_request.is_some(),
                    "clarification_request": p.clarification_request})).collect();
            let decided: Vec<Value> = self.proposals.iter().filter(mine).filter(|p| p.decision.is_some())
                .map(|p| { let d = p.decision.clone().unwrap_or_default(); json!({"id": p.id, "status": p.status, "review": p.review, "note": d["note"], "t_s": d["t_s"],
                    "approved": d["effective_commands"].as_array().map_or(0, Vec::len), "rejected": d["rejected_actions"].as_array().map_or(0, Vec::len)}) }).collect();
            let thread: Vec<Value> = self.messages.iter().filter(|t| t.delivery == m.delivery && (t.from == m.to || t.to == m.to)).map(|t| self.message_json(t)).collect();
            json!({
                "inject_message_id": m.id, "delivery_id": m.delivery, "title": m.title, "role": m.to, "clock": clock(m.t_s), "t_s": m.t_s,
                "inject_text": m.text, "replies": replies, "pending_proposals": pending, "decided_proposals": decided, "thread": thread,
                "conversations": self.conversations.iter().filter(|c| c.quote == m.id || (c.delivery == m.delivery.unwrap_or(0) && c.participants.contains(&m.to))).map(|c| self.conversation_json(c, None)).collect::<Vec<_>>(),
                "clarification_threads": self.threads.iter().filter(|t| t.delivery == m.delivery && t.role == m.to).map(|t| self.thread_json(t)).collect::<Vec<_>>(),
            })
        }).collect();
        json!({
            "inject_responses": responses,
            "proposals": self.proposals.iter().map(|p| self.proposal_json(p, true)).collect::<Vec<_>>(),
            "linked_conversations": self.conversations.iter().map(|c| self.conversation_json(c, None)).collect::<Vec<_>>(),
            "clarification_threads": self.threads.iter().map(|t| self.thread_json(t)).collect::<Vec<_>>(),
            "planned_injects": self.planned.iter().map(|p| json!({"id_inject": p.id, "title": p.title, "text": p.text,
                "scenario_time": clock(0), "recipients_planned": p.recipients, "sent_to": p.sent_to,
                "not_yet_sent_to": p.recipients.iter().filter(|r| !p.sent_to.iter().any(|s| s == *r)).collect::<Vec<_>>(), "actions": []})).collect::<Vec<_>>(),
            "messages": [],
            "agenda": {"now_s": self.now_s, "turn_start_s": 0},
        })
    }
}

/// Every bench command is valid: the fake has no simulator to say otherwise.
fn verdicts(commands: &[Value]) -> Vec<Value> {
    commands
        .iter()
        .map(|_| json!({"verdict": "ok", "notes": []}))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (Fake, u64) {
        let mut f = Fake::new("it");
        f.plan(
            "I1",
            "Fumo sulla collina",
            "Segnalato fumo sopra Valverde.",
            &["mayor", "police"],
        );
        let d = f.send("I1", &["mayor".into(), "police".into()]).unwrap();
        (f, d)
    }

    #[test]
    fn inject_response_and_decision_reach_both_sides() {
        let (mut f, d) = seeded();
        let inject = f.delivered("mayor", d).unwrap().id;
        assert_eq!(f.player("mayor")["inbox"].as_array().unwrap().len(), 1);
        let r = f
            .handle(
                "mayor",
                "/api/player/mayor/actions",
                Some(
                    json!({"kind": "decision", "basis": "operational_judgment", "rationale": "Le famiglie evacuate devono stare al coperto", "narrative": "Apriamo la palestra", "commands": [], "reply_to": inject}),
                ),
            )
            .unwrap();
        let pid = r["proposal"].as_u64().unwrap();
        // `kind` is required, and a decision needs its basis.
        assert!(f.handle("mayor", "/api/player/mayor/actions", Some(json!({"narrative": "x", "reply_to": inject}))).is_err());
        assert!(f.handle("mayor", "/api/player/mayor/actions", Some(json!({"kind": "decision", "narrative": "x", "reply_to": inject}))).is_err());
        // A reply to the sender also needs its rationale, and a plan its reference.
        let reply = |extra: Value| { let mut b = json!({"kind": "communication", "narrative": "x", "reply_to": inject}); for (k, v) in extra.as_object().unwrap() { b[k] = v.clone(); } b };
        assert!(f.handle("mayor", "/api/player/mayor/actions", Some(reply(json!({"basis": "operational_judgment"})))).is_err());
        assert!(f.handle("mayor", "/api/player/mayor/actions", Some(reply(json!({"basis": "plan", "rationale": "y"})))).is_err());
        let m = f.master();
        let row = m["inject_responses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["role"] == "mayor")
            .unwrap()
            .clone();
        assert_eq!(crate::risposte::pending_count(&m), 1);
        assert_eq!(row["replies"].as_array().unwrap().len(), 1);
        // A rejection needs a note; an approval does not.
        assert!(f
            .handle(
                "master",
                &format!("/api/master/proposals/{pid}/decide"),
                Some(json!({"approve": false, "note": ""}))
            )
            .is_err());
        f.handle(
            "master",
            &format!("/api/master/proposals/{pid}/decide"),
            Some(json!({"approve": true, "commands": [], "note": ""})),
        )
        .unwrap();
        assert_eq!(f.player("mayor")["proposals"][0]["status"], "approved");
        // Players cannot act for each other.
        assert!(f
            .handle(
                "police",
                "/api/player/mayor/actions",
                Some(json!({"narrative": "x", "reply_to": inject}))
            )
            .is_err());
    }

    #[test]
    fn suggestions_clarifications_and_conversations() {
        let (mut f, d) = seeded();
        let inject = f.delivered("mayor", d).unwrap().id;
        let s = f
            .handle(
                "mayor",
                "/api/player/mayor/suggest-actions",
                Some(json!({"narrative": "..."})),
            )
            .unwrap();
        assert_eq!(s["status"], "ready");
        let cmd = s["actions"][0]["command"].clone();
        let pid = f
            .respond("mayor", "", vec![cmd.clone()], Some(inject), None)
            .unwrap();
        // Suggestions already among the actions are not offered again.
        let e = f
            .handle(
                "master",
                &format!("/api/master/proposals/{pid}/suggest-actions"),
                Some(json!({})),
            )
            .unwrap();
        assert!(e["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["command"] != cmd));
        f.handle(
            "master",
            &format!("/api/master/proposals/{pid}/clarify"),
            Some(json!({"note": "Quale zona?"})),
        )
        .unwrap();
        let th = f.player("mayor")["clarification_threads"][0].clone();
        assert_eq!(th["status"], "awaiting_player");
        let q = crate::clarify::last_question(&th).unwrap();
        f.handle(
            "mayor",
            "/api/player/mayor/reply",
            Some(crate::clarify::answer_body("F4", q)),
        )
        .unwrap();
        assert_eq!(
            f.master()["clarification_threads"][0]["status"],
            "awaiting_excon"
        );
        let tid = th["id"].as_u64().unwrap();
        f.handle(
            "master",
            &format!("/api/master/clarifications/{tid}/close"),
            Some(json!({})),
        )
        .unwrap();
        assert_eq!(f.threads[0].status, "closed");
        // A conversation linked to the inject shows up for both players and EXCON.
        // Starting it needs its reason, as on the server.
        let start = json!({"delivery": inject, "participants": ["police"], "text": "Chiudete la SP12?"});
        assert!(f.handle("mayor", "/api/player/mayor/conversations", Some(start.clone())).unwrap_err().contains("why you start"));
        let mut law = start.clone();
        law["basis"] = json!("law");
        law["rationale"] = json!("x");
        assert!(f.handle("mayor", "/api/player/mayor/conversations", Some(law)).unwrap_err().contains("which plan"));
        let mut start = start;
        start["basis"] = json!("operational_judgment");
        start["basis_note"] = json!("ignored");
        start["rationale"] = json!(" La Polizia chiude la strada ");
        let c = f.handle("mayor", "/api/player/mayor/conversations", Some(start)).unwrap();
        let cid = c["conversation"].as_u64().unwrap();
        f.handle(
            "police",
            &format!("/api/player/police/conversations/{cid}/messages"),
            Some(json!({"text": "Sì"})),
        )
        .unwrap();
        assert_eq!(
            f.player("police")["linked_conversations"][0]["messages"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            f.player("police")["linked_conversations"][0]["quote"]["to"],
            "police"
        );
        // Police received the inject too: its own copy, text included.
        assert!(f.player("police")["linked_conversations"][0]["quote"]["text"].is_string());
        let reason = &f.player("police")["linked_conversations"][0]["reason"];
        assert_eq!((reason["basis"].as_str(), reason["rationale"].as_str()), (Some("operational_judgment"), Some("La Polizia chiude la strada")));
        assert!(reason["reference"].is_null(), "personal initiative has no reference");
        let row = f.master()["inject_responses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["role"] == "mayor")
            .cloned()
            .unwrap();
        assert_eq!(row["conversations"].as_array().unwrap().len(), 1);
    }

    /// A participant who never received the inject reads its title only; the
    /// reason travels with the conversation to participants and EXCON.
    #[test]
    fn a_guest_reads_the_inject_title_and_the_reason_only() {
        let mut f = Fake::new("it");
        f.plan("I2", "Strada chiusa", "La SP12 è bloccata da un albero.", &["mayor"]);
        let d = f.send("I2", &["mayor".into()]).unwrap();
        let inject = f.delivered("mayor", d).unwrap().id;
        f.handle("mayor", "/api/player/mayor/conversations", Some(json!({"delivery": inject, "participants": ["police"], "text": "Potete aiutare?",
            "basis": "plan", "basis_note": "Piano comunale, scheda 2", "rationale": "Serve la Polizia sulla SP12"}))).unwrap();
        let guest = &f.player("police")["linked_conversations"][0];
        assert_eq!(guest["quote"]["title"], "Strada chiusa");
        assert!(guest["quote"]["text"].is_null());
        assert_eq!(guest["quote"]["title_only"], true);
        assert!(!f.player("police").to_string().contains("bloccata"));
        assert_eq!(guest["reason"]["reference"], "Piano comunale, scheda 2");
        assert_eq!(f.player("mayor")["linked_conversations"][0]["quote"]["text"], "La SP12 è bloccata da un albero.");
        let row = f.master()["inject_responses"][0].clone();
        assert_eq!(row["conversations"][0]["reason"]["rationale"], "Serve la Polizia sulla SP12");
        assert!(row["conversations"][0]["quote"]["text"].is_string(), "EXCON reads everything");
    }

    #[test]
    fn partial_approval_keeps_the_reasons() {
        let (mut f, d) = seeded();
        let inject = f.delivered("police", d).unwrap().id;
        let pid = f
            .respond(
                "police",
                "",
                canned_suggestions("police")
                    .into_iter()
                    .map(|c| c.0)
                    .collect(),
                Some(inject),
                None,
            )
            .unwrap();
        f.handle("master", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve": true, "note": "", "commands": f.proposal(pid).unwrap().commands, "rejected_actions": [{"index": 1, "note": "non serve"}]}))).unwrap();
        let p = f.proposal(pid).unwrap();
        assert_eq!((p.review, p.commands.len()), ("partially_approved", 1));
        assert_eq!(
            p.decision.as_ref().unwrap()["rejected_actions"][0]["note"],
            "non serve"
        );
    }
}
