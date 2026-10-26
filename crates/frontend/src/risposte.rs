//! EXCON "Risposte" tab: sent injects with the per-recipient reply state and
//! the pending approvals that come from the player's replies.
use crate::{
    api::{list, text},
    timeline::{format_offset, role_name},
    widgets::Proposal,
    Action,
};
use serde_json::Value;
use std::collections::BTreeMap;
use yew::prelude::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ReplyState {
    None,
    Replied,
    /// Every open proposal waits for the player's answer to an EXCON question.
    Clarifying,
    Pending,
}

/// Proposals replaced by a newer player reply are history, never pending.
fn is_open(p: &Value) -> bool {
    p["status"] != "superseded"
}

/// Only injects actually delivered to the recipient appear here: a row needs
/// its delivered message, never a draft, a pending proposal or an unsent plan.
pub fn is_sent(r: &Value) -> bool {
    r["inject_message_id"].is_number() && !text(&r["clock"]).is_empty()
}

pub fn reply_state(r: &Value) -> ReplyState {
    let open = list(&r["pending_proposals"]).iter().filter(|p| is_open(p)).collect::<Vec<_>>();
    if open.iter().any(|p| p["awaiting_clarification"] != true) {
        ReplyState::Pending
    } else if !open.is_empty() {
        ReplyState::Clarifying
    } else if !list(&r["replies"]).is_empty() {
        ReplyState::Replied
    } else {
        ReplyState::None
    }
}

fn state_label(s: ReplyState) -> &'static str {
    match s {
        ReplyState::None => crate::i18n::ex("nessuna risposta"),
        ReplyState::Replied => crate::i18n::ex("risposta ricevuta"),
        ReplyState::Clarifying => crate::i18n::ex("in attesa di chiarimento"),
        ReplyState::Pending => crate::i18n::ex("in attesa di approvazione"),
    }
}

/// Number of proposals still waiting for an EXCON decision (nav badge).
pub fn pending_count(master: &Value) -> usize {
    list(&master["inject_responses"])
        .iter()
        .map(|r| {
            list(&r["pending_proposals"])
                .iter()
                .filter(|p| is_open(p) && p["awaiting_clarification"] != true)
                .count()
        })
        .sum()
}

/// Number of proposals sent back to the player and awaiting their answer.
pub fn clarifying_count(master: &Value) -> usize {
    list(&master["inject_responses"])
        .iter()
        .map(|r| {
            list(&r["pending_proposals"])
                .iter()
                .filter(|p| is_open(p) && p["awaiting_clarification"] == true)
                .count()
        })
        .sum()
}

#[derive(Properties, PartialEq)]
pub struct RisposteProps {
    pub master: Value,
    pub state: Value,
    pub stat: Value,
    pub responses: BTreeMap<String, Value>,
    pub action: Callback<Action>,
    pub busy: bool,
}

#[function_component(Risposte)]
pub fn risposte(props: &RisposteProps) -> Html {
    let opened = use_state(|| Option::<(u64, String)>::None);
    let only_pending = use_state(|| false);
    let roles = list(&props.stat["roles"]).to_vec();
    let rows = list(&props.master["inject_responses"]).to_vec();
    let key = |r: &Value| (r["inject_message_id"].as_u64().unwrap_or(0), text(&r["role"]).to_string());
    let selected = opened
        .as_ref()
        .and_then(|k| rows.iter().find(|r| key(r) == *k).cloned());
    let shown = rows
        .iter()
        .filter(|r| is_sent(r))
        .filter(|r| !*only_pending || matches!(reply_state(r), ReplyState::Pending | ReplyState::Clarifying))
        .collect::<Vec<_>>();
    html! {<section class="card" id="risposte"><div class="row"><h2>{crate::i18n::ex("Risposte")}</h2><span class="pill pending-count" hidden={pending_count(&props.master)==0}>{crate::exfmt!("{} in attesa di approvazione","{} awaiting approval",pending_count(&props.master))}</span><span class="pill clarifying-count" hidden={clarifying_count(&props.master)==0}>{crate::exfmt!("{} in attesa di chiarimento","{} awaiting clarification",clarifying_count(&props.master))}</span><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="only-pending" checked={*only_pending} onchange={{let o=only_pending.clone();Callback::from(move |e:Event|{use yew::TargetCast;o.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("solo in attesa")}</label></div>
        if shown.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject inviato da mostrare")}</p>}
        <ol class="response-list">{for shown.iter().map(|r|{
            let st=reply_state(r);
            let k=key(r);
            let onclick={let opened=opened.clone();let k=k.clone();Callback::from(move |_|opened.set(Some(k.clone())))};
            html!{<li key={format!("{}:{}",k.0,k.1)} data-response={format!("{}:{}",k.0,k.1)}><button class={classes!("response-row",match st{ReplyState::Pending=>"is-pending",ReplyState::Replied=>"is-replied",ReplyState::Clarifying=>"is-pending",ReplyState::None=>"is-none"})} {onclick}>
                <span class="planned-times"><b>{text(&r["clock"])}</b><small>{format_offset(r["t_s"].as_i64().unwrap_or(0))}</small></span>
                <span class="planned-body"><b>{if text(&r["title"]).is_empty(){crate::i18n::ex("Inject")}else{text(&r["title"])}}</b><span class="planned-text">{text(&r["inject_text"])}</span></span>
                <span class="response-who"><span class="pill rcpt sent">{role_name(&roles,text(&r["role"]))}</span><span class={classes!("pill","reply-state",match st{ReplyState::Pending|ReplyState::Clarifying=>"pending",ReplyState::Replied=>"sent",ReplyState::None=>"todo"})}>{state_label(st)}</span></span>
            </button></li>}
        })}</ol>
        if let Some(r)=selected{{dialog(props,&r,&roles,{let o=opened.clone();Callback::from(move |_|o.set(None))})}}
    </section>}
}

fn dialog(props: &RisposteProps, r: &Value, roles: &[Value], onclose: Callback<()>) -> Html {
    let st = reply_state(r);
    html! {<div class="modal-backdrop"><section class="inject-modal response-dialog" role="dialog" aria-modal="true" aria-labelledby="response-dialog-title">
        <div class="row"><h2 id="response-dialog-title">{format!("{} · {}",text(&r["title"]),role_name(roles,text(&r["role"])))}</h2><div class="grow"/><button onclick={Callback::from(move |_|onclose.emit(()))}>{crate::i18n::ex("Chiudi")}</button></div>
        <p class="muted">{format!("{} · {}",text(&r["clock"]),state_label(st))}</p>
        {thread_view(r,roles)}
        if list(&r["pending_proposals"]).iter().any(is_open){<h3>{if st==ReplyState::Clarifying{crate::i18n::ex("In attesa di chiarimento")}else{crate::i18n::ex("In attesa di approvazione")}}</h3>}
        {for list(&r["pending_proposals"]).iter().filter(|pp|is_open(pp)).map(|pp|{
            let full=list(&props.master["proposals"]).iter().find(|p|p["id"]==pp["id"]).cloned();
            match full{
                Some(p)=>{let key=format!("proposal:{}",p["id"]);
                    html!{<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={props.state["generation"].clone()} preview={props.responses.get(&key).cloned().unwrap_or_default()} busy={props.busy} action={props.action.clone()} roles={roles.to_vec()}/>}}
                None=>html!{<p class="muted">{text(&pp["summary"])}</p>},
            }
        })}
    </section></div>}
}

/// Visual kind of a thread message: who speaks decides the side and the colour.
fn thread_kind(m: &Value) -> &'static str {
    match text(&m["kind"]) {
        "inject" => "inject",
        "clarification" => "question",
        "reply" => "reply",
        _ => "chat",
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "inject" => crate::i18n::ex("Inject"),
        "question" => crate::i18n::ex("Chiarimento EXCON"),
        "reply" => crate::i18n::ex("Risposta"),
        _ => crate::i18n::ex("Messaggio"),
    }
}

/// The whole conversation of a delivery in message order. Older payloads
/// without `thread` fall back to the inject plus the player replies.
fn thread_messages(r: &Value) -> Vec<Value> {
    let t = list(&r["thread"]);
    if !t.is_empty() {
        return t.to_vec();
    }
    let mut v = vec![serde_json::json!({"id":r["inject_message_id"],"kind":"inject","from":"master","to":r["role"],
        "clock":r["clock"],"title":r["title"],"text":r["inject_text"]})];
    v.extend(list(&r["replies"]).iter().map(|m| {
        serde_json::json!({"id":m["id"],"kind":"reply","from":r["role"],"to":"master","clock":m["clock"],
            "text":m["text"],"original":m["original"],"reply_to":m["reply_to"]})
    }));
    v
}

fn thread_view(r: &Value, roles: &[Value]) -> Html {
    let messages = thread_messages(r);
    let player = text(&r["role"]).to_string();
    html! {<div class="response-thread" data-response-thread={r["inject_message_id"].to_string()}>
        <h3>{crate::exfmt!("Thread · {} messaggi","Thread · {} messages",messages.len())}</h3>
        if list(&r["replies"]).is_empty(){<p class="muted">{crate::i18n::ex("Nessuna risposta")}</p>}
        <ol class="thread-list">{for messages.iter().map(|m|{
            let kind=thread_kind(m);
            let from=text(&m["from"]);
            let own=from==player;
            let original=text(&m["original"]);
            html!{<li key={m["id"].to_string()} data-thread-message={m["id"].to_string()} class={classes!("thread-msg",format!("kind-{kind}"),own.then_some("from-player"))}>
                <div class="thread-head"><b class="thread-from">{role_name(roles,from)}</b><span class="thread-arrow" aria-hidden="true">{"→"}</span><span class="thread-to">{role_name(roles,text(&m["to"]))}</span><span class="pill thread-kind">{kind_label(kind)}</span><div class="grow"/><time>{text(&m["clock"])}</time></div>
                if !text(&m["title"]).is_empty(){<b class="thread-title">{text(&m["title"])}</b>}
                <p class="thread-text">{text(&m["text"])}</p>
                if !original.is_empty()&&original!=text(&m["text"]){<small class="muted">{crate::exfmt!("Originale: {}","Original: {}",original)}</small>}
                if m["reply_to"].is_number(){<small class="thread-reply-to">{crate::exfmt!("In risposta al messaggio #{}","In reply to message #{}",m["reply_to"])}</small>}
            </li>}
        })}</ol>
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn states() {
        assert_eq!(reply_state(&json!({"replies":[],"pending_proposals":[]})), ReplyState::None);
        assert_eq!(reply_state(&json!({"replies":[{}],"pending_proposals":[]})), ReplyState::Replied);
        assert_eq!(reply_state(&json!({"replies":[{}],"pending_proposals":[{"id":1}]})), ReplyState::Pending);
        let c = json!({"replies":[{}],"pending_proposals":[{"id":1,"awaiting_clarification":true}]});
        assert_eq!(reply_state(&c), ReplyState::Clarifying);
        let mixed = json!({"replies":[],"pending_proposals":[{"id":1,"awaiting_clarification":true},{"id":2}]});
        assert_eq!(reply_state(&mixed), ReplyState::Pending);
        let old = json!({"replies":[{}],"pending_proposals":[{"id":1,"status":"superseded"}]});
        assert_eq!(reply_state(&old), ReplyState::Replied);
        assert_eq!(pending_count(&json!({"inject_responses":[old]})), 0);
    }
    #[test]
    fn only_sent_injects_and_whole_thread() {
        assert!(is_sent(&json!({"inject_message_id":3,"clock":"10:00"})));
        assert!(!is_sent(&json!({"title":"draft"})));
        assert!(!is_sent(&json!({"inject_message_id":null,"clock":"10:00"})));
        let r = json!({"inject_message_id":1,"role":"mayor","clock":"10:00","inject_text":"Fire",
            "replies":[{"id":2,"clock":"10:05","text":"ok","original":"va bene"}],
            "thread":[{"id":1,"kind":"inject"},{"id":2,"kind":"reply"},{"id":3,"kind":"clarification"}]});
        assert_eq!(thread_messages(&r).len(), 3);
        // Without `thread` the inject and its replies are rebuilt in order.
        let old = json!({"inject_message_id":1,"role":"mayor","clock":"10:00","inject_text":"Fire","replies":[{"id":2,"text":"ok"}]});
        let t = thread_messages(&old);
        assert_eq!((thread_kind(&t[0]), thread_kind(&t[1])), ("inject", "reply"));
        assert_eq!(thread_kind(&json!({"kind":"clarification"})), "question");
    }
    #[test]
    fn counts_pending() {
        let m = json!({"inject_responses":[{"pending_proposals":[{"id":1},{"id":2}]},{"pending_proposals":[]},{"pending_proposals":[{"id":3}]}]});
        assert_eq!(pending_count(&m), 3);
        assert_eq!(clarifying_count(&m), 0);
        let m = json!({"inject_responses":[{"pending_proposals":[{"id":1,"awaiting_clarification":true},{"id":2}]}]});
        assert_eq!((pending_count(&m), clarifying_count(&m)), (1, 1));
    }
}
