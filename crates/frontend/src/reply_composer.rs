//! The player's one reply composer (30/10 playtest: "two options, then a
//! second button, then a confirm: clunky"). The same box sits under an inject
//! and under a conversation linked to it: a visible recipient ("To"), the
//! text and one Send. A reply to the inject's sender also states what it
//! rests on (plan or procedure, law or regulation, operational judgement:
//! required), which plan or law (required for those two) and the player's
//! rationale ("Motivazione": why this decision, required); Send stays
//! disabled, with a hint saying what is missing, until all of it is there.
//! Messages to other players are coordination and need none of it.
//!
//! Players never act on the world: what they send to the inject's sender is a
//! reply EXCON's AI reads to suggest catalogue actions (`exercise::reply_review`);
//! what they send to other players is a coordination message in a linked
//! conversation. The recipient decides which endpoint one Send calls:
//!
//! | where | to | request |
//! |---|---|---|
//! | inject | sender | `POST /api/player/<r>/actions` (reply) |
//! | inject | players | `POST /api/player/<r>/conversations` (new conversation) |
//! | conversation | its players | `POST /api/player/<r>/conversations/<c>/messages` |
//! | conversation | sender | `POST /api/player/<r>/actions` with `conversation` |
//!
//! Drafts are kept per context (`inj:<message>` or `conv:<id>`) in the
//! player's fields: `reply:`, `to:`, `people:`, `basis:`, `basis_note:`,
//! `rationale:`.
use crate::{
    api::{self, list, text},
    i18n, Action, ActionKind, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;

/// Where the composer sits.
pub enum Place<'a> {
    /// Under an inject the player received (`msg` is the delivery).
    Inject { msg: &'a Value },
    /// Inside conversation `c`, linked to the inject it quotes.
    Conversation { c: &'a Value },
}

/// Who the reply goes to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum To {
    /// The inject's fictitious sender, played by exercise control.
    Sender,
    /// Other players (a conversation).
    Players,
}

impl Place<'_> {
    pub fn ctx(&self) -> String {
        match self {
            Place::Inject { msg } => format!("inj:{}", msg["id"]),
            Place::Conversation { c } => format!("conv:{}", c["id"]),
        }
    }
    /// The recipient when the player has not picked one: the sender under an
    /// inject, the other participants inside a conversation.
    fn default_to(&self) -> To {
        match self {
            Place::Inject { .. } => To::Sender,
            Place::Conversation { .. } => To::Players,
        }
    }
}

/// Draft field keys of context `ctx`.
pub fn draft_key(ctx: &str) -> String {
    format!("reply:{ctx}")
}
// Not `draft:…`: `Msg::Field` routes those keys to the chat and inject drafts.
const PER_CONTEXT: [&str; 6] = ["reply", "to", "people", "basis", "basis_note", "rationale"];

/// The reply of context `ctx` was sent: its draft goes, the conversation it
/// started (if any) opens.
pub fn reply_sent(fields: &mut std::collections::BTreeMap<String, String>, ctx: &str, value: &Value) {
    for k in PER_CONTEXT {
        fields.remove(&format!("{k}:{ctx}"));
    }
    fields.remove("inj:conv_reply");
    if let Some(id) = value["conversation"].as_u64().filter(|_| ctx.starts_with("inj:")) {
        fields.insert("inj:conv".into(), id.to_string());
    }
}

/// Body of the reply to the sender: the words, their basis, the reference
/// (plan or law only) and the rationale.
pub fn reply_body(narrative: &str, parent: u64, conversation: Option<u64>, basis: &str, note: &str, rationale: &str) -> Value {
    let mut body = json!({"kind": "communication", "narrative": narrative, "reply_to": parent, "conversation": conversation});
    if !basis.is_empty() {
        body["basis"] = json!(basis);
        if needs_reference(basis) && !note.trim().is_empty() {
            body["basis_note"] = json!(note.trim());
        }
    }
    if !rationale.trim().is_empty() {
        body["rationale"] = json!(rationale.trim());
    }
    body
}

/// The basis options, as the server names them, with their label and the
/// placeholder of the reference field (none for operational judgement).
pub const BASES: [(&str, &str, Option<&str>); 3] = [
    ("plan", "basis_plan", Some("basis_note_plan")),
    ("law", "basis_law", Some("basis_note_law")),
    ("operational_judgment", "basis_judgment", None),
];

/// A plan or a law needs its reference (which one, which part).
pub fn needs_reference(basis: &str) -> bool {
    BASES.iter().any(|(v, _, h)| *v == basis && h.is_some())
}

/// What a reply to the sender still lacks (the hint key), in fill order.
pub fn sender_missing(text: &str, basis: &str, note: &str, rationale: &str) -> Option<&'static str> {
    if text.trim().is_empty() {
        Some("need_text")
    } else if !BASES.iter().any(|(v, _, _)| *v == basis) {
        Some("need_reply_basis")
    } else if needs_reference(basis) && note.trim().is_empty() {
        Some("need_reference")
    } else if rationale.trim().is_empty() {
        Some("need_rationale")
    } else {
        None
    }
}

impl crate::player_view::PlayerView {
    /// The context whose draft a map click writes into, if a composer is open.
    pub fn active_reply_ctx(&self) -> Option<String> {
        let conv = self.field("inj:conv", "");
        if !conv.is_empty() {
            return Some(format!("conv:{conv}"));
        }
        let open = self.field("inj:open", "");
        let received = self.messages().iter().any(|m| m["kind"] == "inject" && m["id"].to_string() == open);
        received.then(|| format!("inj:{open}"))
    }

    /// The composer: recipient, text, basis, one Send.
    pub fn reply_composer(&self, can_act: bool, place: Place) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let own = self.own.clone();
        let ctx = place.ctx();
        let get = |k: &str| self.field(&format!("{k}:{ctx}"), "");
        let set = |k: &str| {
            let key = format!("{k}:{ctx}");
            self.cb(move |v: String| Msg::Field(key.clone(), v))
        };
        let draft = get("reply");
        let to = match get("to").as_str() {
            "sender" => To::Sender,
            "players" => To::Players,
            _ => place.default_to(),
        };
        let basis = get("basis");
        let note = get("basis_note");
        let rationale = get("rationale");
        // The inject being answered: the delivery, or the conversation's quote.
        let (inject, parent, conversation): (&Value, u64, Option<u64>) = match &place {
            Place::Inject { msg } => (msg, msg["id"].as_u64().unwrap_or(0), None),
            Place::Conversation { c } => (&c["quote"], c["delivery"].as_u64().unwrap_or(0), c["id"].as_u64()),
        };
        let sender = if inject["persona"].is_string() { text(&inject["persona"]).to_string() } else { self.name(text(&inject["from"])) };
        // Players: the conversation's participants, or the ones picked for a new one.
        let picked: Vec<String> = get("people").split(',').filter(|p| !p.is_empty()).map(String::from).collect();
        let players: Vec<(String, String)> = match &place {
            Place::Conversation { c } => list(&c["participants"]).iter().map(text).filter(|p| *p != own).map(|p| (p.to_string(), self.name(p))).collect(),
            Place::Inject { .. } => self.contacts().iter().filter(|r| r["id"] != "master").map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string())).collect(),
        };
        let recipients: Vec<String> = match (&place, to) {
            (_, To::Sender) => vec![sender.clone()],
            (Place::Conversation { .. }, To::Players) => players.iter().map(|(_, n)| n.clone()).collect(),
            (Place::Inject { .. }, To::Players) => players.iter().filter(|(id, _)| picked.contains(id)).map(|(_, n)| n.clone()).collect(),
        };
        let missing = if self.state["phase"] != "orders" && to == To::Sender {
            Some("work_need_orders")
        } else if to == To::Sender {
            sender_missing(&draft, &basis, &note, &rationale)
        } else if draft.trim().is_empty() {
            Some("need_text")
        } else if recipients.is_empty() {
            Some("need_players")
        } else {
            None
        };
        let cite = self.field("inj:conv_reply", "").parse::<u64>().ok();
        let send = {
            let (own, draft, ctx, basis, note, rationale, picked) =
                (own.clone(), draft.trim().to_string(), ctx.clone(), basis.clone(), note.clone(), rationale.clone(), picked.clone());
            let is_conv = matches!(place, Place::Conversation { .. });
            self.cb(move |_: ()| {
                let (path, body) = match (is_conv, to) {
                    (_, To::Sender) => (format!("/api/player/{own}/actions"), reply_body(&draft, parent, conversation, &basis, &note, &rationale)),
                    (false, To::Players) => (format!("/api/player/{own}/conversations"), json!({"delivery": parent, "participants": picked, "text": draft})),
                    (true, To::Players) => (format!("/api/player/{own}/conversations/{}/messages", conversation.unwrap_or(0)), json!({"text": draft, "reply_to": cite})),
                };
                let mut a = Action::post(path, body);
                a.kind = ActionKind::ReplySent(ctx.clone());
                Msg::Action(a)
            })
        };
        let ready = missing.is_none() && can_act && !self.busy;
        let pick_to = |t: To| {
            let key = format!("to:{ctx}");
            self.cb(move |_: Event| Msg::Field(key.clone(), if t == To::Sender { "sender" } else { "players" }.into()))
        };
        let toggle_person = |id: String, on: bool| {
            let (key, current) = (format!("people:{ctx}"), picked.clone());
            self.cb(move |_: Event| {
                let mut next: Vec<String> = current.iter().filter(|p| **p != id).cloned().collect();
                if !on {
                    next.push(id.clone());
                }
                Msg::Field(key.clone(), next.join(","))
            })
        };
        let players_label = match &place {
            Place::Conversation { .. } => i18n::tf(l, "to_conv", &[&players.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>().join(", ")]),
            Place::Inject { .. } => i18n::t(l, "to_players_new").to_string(),
        };
        // Unique per player too: the bench shows several players on one page.
        let radio = format!("reply-to-{}-{}", own, ctx.replace(':', "-"));
        let basis_hint = BASES.iter().find(|(v, _, _)| *v == basis).and_then(|(_, _, h)| *h);
        html! {<footer class="inj-composer reply-composer" data-reply-composer={ctx.clone()} data-reply-to={if to==To::Sender{"sender"}else{"players"}}>
            <fieldset class="reply-to" data-reply-recipient="">
                <legend>{i18n::t(l,"to_label")}</legend>
                <label class={classes!("reply-to-option",(to==To::Sender).then_some("is-selected"))}><input type="radio" name={radio.clone()} data-to="sender" checked={to==To::Sender} onchange={pick_to(To::Sender)}/><span>{i18n::tf(l,"to_sender",&[&sender])}</span></label>
                <label class={classes!("reply-to-option",(to==To::Players).then_some("is-selected"))}><input type="radio" name={radio} data-to="players" checked={to==To::Players} disabled={players.is_empty()} onchange={pick_to(To::Players)}/><span>{players_label}</span></label>
                if to==To::Players && matches!(place, Place::Inject{..}) {<div class="reply-people">{for players.iter().map(|(id,name)|{let on=picked.contains(id);html!{<label class="mt-check" key={id.clone()}><input type="checkbox" data-conv-person={id.clone()} checked={on} onchange={toggle_person(id.clone(),on)}/>{name.clone()}</label>}})}</div>}
            </fieldset>
            if to==To::Players {
                if let Some((who,what))=cite.and_then(|id|self.cited(&place,id)){<div class="chat-reply-composer" data-reply-preview={cite.map(|c|c.to_string()).unwrap_or_default()}><div><b>{i18n::tf(l,"replying_to",&[&who])}</b><p>{what}</p></div><button type="button" class="chat-reply-cancel" onclick={self.cb(|_|Msg::Field("inj:conv_reply".into(),String::new()))}>{i18n::t(l,"cancel_reply")}</button></div>}
            }
            <textarea id="response-narrative" data-response-narrative="" maxlength="8000" aria-label={i18n::t(l,"reply_text_label")}
                placeholder={if to==To::Sender{i18n::t(l,"reply_ph")}else{i18n::t(l,"write")}} value={draft.clone()}
                oninput={{let set=set("reply");Callback::from(move |e|set.emit(api::input(e)))}}
                onkeydown={{let send=send.clone();Callback::from(move |e:KeyboardEvent|{if ready&&e.key()=="Enter"&&(e.ctrl_key()||e.meta_key()){e.prevent_default();send.emit(());}})}}/>
            if to==To::Sender {
                <div class="reply-basis-pick" data-work-block="basis">
                    <span class="reply-basis-title">{i18n::t(l,"basis_title")}<small class="muted">{format!(" · {}",i18n::t(l,"basis_required"))}</small></span>
                    <div class="basis-options" role="radiogroup" aria-label={i18n::t(l,"basis_title")}>
                        {for BASES.iter().map(|(value,label,_)|{
                            let on=basis==*value;
                            let set=set("basis");
                            let next=if on{String::new()}else{value.to_string()};
                            html!{<button type="button" key={*value} class={classes!("basis-chip",on.then_some("is-selected"))} id={format!("basis-{value}")} data-basis={*value} aria-pressed={on.to_string()} onclick={Callback::from(move |_|set.emit(next.clone()))}>{i18n::t(l,label)}</button>}
                        })}
                    </div>
                    if let Some(hint)=basis_hint {
                        <input type="text" class="reply-basis-note" data-basis-note="" maxlength="2000" required=true placeholder={i18n::t(l,hint)} aria-label={i18n::t(l,hint)} value={note.clone()} oninput={{let set=set("basis_note");Callback::from(move |e|set.emit(api::input(e)))}}/>
                    }
                </div>
                <label class="reply-rationale" data-work-block="rationale">
                    <span class="reply-basis-title">{i18n::t(l,"rationale_title")}<small class="muted">{format!(" · {}",i18n::t(l,"basis_required"))}</small></span>
                    <textarea class="reply-rationale-text" data-rationale="" rows="2" maxlength="2000" required=true placeholder={i18n::t(l,"rationale_ph")} value={rationale.clone()} oninput={{let set=set("rationale");Callback::from(move |e|set.emit(api::input(e)))}}/>
                </label>
            }
            if !self.error.is_empty(){<p class="error" role="alert">{&self.error}</p>}
            <details class="reply-guide-more"><summary>{i18n::t(l,"guide_open")}</summary>{self.reply_guidance(l,false)}</details>
            <div class="row reply-send-bar">
                <small class="muted work-hint" role="status">{match missing{Some(m)=>i18n::t(l,m),None=>i18n::t(l,if to==To::Sender{"to_sender_hint"}else{"to_players_hint"})}}</small>
                <div class="grow"/>
                <button class="primary" id="respond-submit" data-reply-send="" data-respond={(to==To::Sender).then_some("")} data-send-conversation={(to==To::Players).then_some("")} disabled={!ready} onclick={send.reform(|_: MouseEvent| ())}>
                    {if recipients.is_empty(){i18n::t(l,"send_response").to_string()}else{i18n::tf(l,"send_to",&[&recipients.join(", ")])}}</button>
            </div>
        </footer>}
    }

    /// A message of conversation `place` cited by the reply (author, excerpt).
    fn cited(&self, place: &Place, id: u64) -> Option<(String, String)> {
        let Place::Conversation { c } = place else { return None };
        let l = self.lang();
        if c["delivery"].as_u64() == Some(id) {
            return Some((i18n::t(&l, "inject").to_string(), text(&c["quote"]["title"]).to_string()));
        }
        list(&c["messages"]).iter().find(|m| m["id"].as_u64() == Some(id)).map(|m| {
            let mine = m["from"] == self.own.as_str();
            (self.name(text(&m["from"])), crate::inject_panel::excerpt(text(if mine && m["original"].is_string() { &m["original"] } else { &m["text"] })))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_reply_carries_its_basis_reference_and_rationale() {
        let b = reply_body("Evacuiamo F4", 7, None, "", "ignored", " ");
        assert_eq!(b["kind"], "communication");
        assert!(b.get("basis").is_none() && b.get("basis_note").is_none() && b.get("rationale").is_none());
        let b = reply_body("Evacuiamo F4", 7, Some(2), "plan", "  Piano comunale, scheda 4 ", " Il fuoco arriva entro un'ora ");
        assert_eq!((b["basis"].as_str(), b["basis_note"].as_str()), (Some("plan"), Some("Piano comunale, scheda 4")));
        assert_eq!(b["rationale"], "Il fuoco arriva entro un'ora");
        assert_eq!(b["conversation"], 2);
        // Operational judgement has no reference line: a leftover one is not sent.
        let b = reply_body("x", 7, None, "operational_judgment", "Piano", "Perché");
        assert!(b.get("basis_note").is_none());
        assert_eq!(b["rationale"], "Perché");
    }

    /// Send waits for text, basis, the reference of a plan or law and the rationale.
    #[test]
    fn a_reply_to_the_sender_needs_basis_reference_and_rationale() {
        assert_eq!(sender_missing(" ", "plan", "x", "y"), Some("need_text"));
        assert_eq!(sender_missing("F4", "", "", "y"), Some("need_reply_basis"));
        assert_eq!(sender_missing("F4", "gossip", "", "y"), Some("need_reply_basis"));
        assert_eq!(sender_missing("F4", "plan", " ", "y"), Some("need_reference"));
        assert_eq!(sender_missing("F4", "law", "", "y"), Some("need_reference"));
        assert_eq!(sender_missing("F4", "law", "art. 15", " "), Some("need_rationale"));
        assert_eq!(sender_missing("F4", "operational_judgment", "", " "), Some("need_rationale"));
        assert_eq!(sender_missing("F4", "operational_judgment", "", "Il vento gira"), None);
        assert_eq!(sender_missing("F4", "plan", "Scheda 4", "Il vento gira"), None);
        for key in ["need_text", "need_reply_basis", "need_reference", "need_rationale", "rationale_title", "rationale_ph", "basis_required"] {
            for l in ["it", "en"] {
                assert!(crate::i18n_risposte::lookup(l, key).is_some(), "{l}:{key}");
            }
        }
    }

    #[test]
    fn sending_clears_only_that_draft_and_opens_a_new_conversation() {
        let mut f = BTreeMap::new();
        for k in PER_CONTEXT {
            f.insert(format!("{k}:inj:7"), "x".to_string());
        }
        f.insert("reply:conv:3".into(), "other".into());
        reply_sent(&mut f, "inj:7", &json!({"conversation": 4}));
        assert!(PER_CONTEXT.iter().all(|k| !f.contains_key(&format!("{k}:inj:7"))));
        assert_eq!(f.get("reply:conv:3").map(String::as_str), Some("other"));
        assert_eq!(f.get("inj:conv").map(String::as_str), Some("4"));
        // A message inside a conversation does not move the player.
        reply_sent(&mut f, "conv:3", &json!({"conversation": 9}));
        assert_eq!(f.get("inj:conv").map(String::as_str), Some("4"));
    }

    /// One composer, one Send: no step chooser, no second "respond" button,
    /// no new-conversation form, no action controls.
    #[test]
    fn one_composer_one_send() {
        let panel = include_str!("inject_panel.rs");
        let body = &panel[..panel.find("#[cfg(test)]").unwrap()];
        for gone in ["reply_step_title", "data-reply-direct", "data-respond-from-conversation", "conv_respond", "data-create-conversation", "OpenWork"] {
            assert!(!body.contains(gone), "inject panel still has {gone}");
        }
        let me = include_str!("reply_composer.rs");
        let me = &me[..me.find("#[cfg(test)]").unwrap()];
        assert_eq!(me.matches("data-reply-send").count(), 1);
        for forbidden in ["action_pick::form(", "data-add-action", "suggest-actions", "\"commands\""] {
            assert!(!me.contains(forbidden), "composer has {forbidden}");
        }
        assert!(me.contains("reply_guidance(l,false)") && me.contains("basis_title") && me.contains("data-rationale"));
    }
}
