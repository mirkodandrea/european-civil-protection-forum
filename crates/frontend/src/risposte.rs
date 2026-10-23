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
    Pending,
}

pub fn reply_state(r: &Value) -> ReplyState {
    if !list(&r["pending_proposals"]).is_empty() {
        ReplyState::Pending
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
        ReplyState::Pending => crate::i18n::ex("in attesa di approvazione"),
    }
}

/// Number of proposals still waiting for an EXCON decision (nav badge).
pub fn pending_count(master: &Value) -> usize {
    list(&master["inject_responses"])
        .iter()
        .map(|r| list(&r["pending_proposals"]).len())
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
        .filter(|r| !*only_pending || reply_state(r) == ReplyState::Pending)
        .collect::<Vec<_>>();
    html! {<section class="card" id="risposte"><div class="row"><h2>{crate::i18n::ex("Risposte")}</h2><span class="pill pending-count" hidden={pending_count(&props.master)==0}>{crate::exfmt!("{} in attesa di approvazione","{} awaiting approval",pending_count(&props.master))}</span><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="only-pending" checked={*only_pending} onchange={{let o=only_pending.clone();Callback::from(move |e:Event|{use yew::TargetCast;o.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("solo in attesa")}</label></div>
        if shown.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject inviato da mostrare")}</p>}
        <ol class="response-list">{for shown.iter().map(|r|{
            let st=reply_state(r);
            let k=key(r);
            let onclick={let opened=opened.clone();let k=k.clone();Callback::from(move |_|opened.set(Some(k.clone())))};
            html!{<li key={format!("{}:{}",k.0,k.1)} data-response={format!("{}:{}",k.0,k.1)}><button class={classes!("response-row",match st{ReplyState::Pending=>"is-pending",ReplyState::Replied=>"is-replied",ReplyState::None=>"is-none"})} {onclick}>
                <span class="planned-times"><b>{text(&r["clock"])}</b><small>{format_offset(r["t_s"].as_i64().unwrap_or(0))}</small></span>
                <span class="planned-body"><b>{if text(&r["title"]).is_empty(){crate::i18n::ex("Inject")}else{text(&r["title"])}}</b><span class="planned-text">{text(&r["inject_text"])}</span></span>
                <span class="response-who"><span class="pill rcpt sent">{role_name(&roles,text(&r["role"]))}</span><span class={classes!("pill","reply-state",match st{ReplyState::Pending=>"pending",ReplyState::Replied=>"sent",ReplyState::None=>"todo"})}>{state_label(st)}</span></span>
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
        <h3>{crate::i18n::ex("Inject inviato")}</h3><p class="inject-text">{text(&r["inject_text"])}</p>
        <h3>{crate::i18n::ex("Risposte del player")}</h3>
        if list(&r["replies"]).is_empty(){<p class="muted">{crate::i18n::ex("Nessuna risposta")}</p>}
        <ol class="reply-list">{for list(&r["replies"]).iter().map(|m|html!{<li key={m["id"].to_string()}><small>{text(&m["clock"])}</small><p>{text(&m["text"])}</p>
            if !text(&m["original"]).is_empty()&&text(&m["original"])!=text(&m["text"]){<small class="muted">{crate::exfmt!("Originale: {}","Original: {}",text(&m["original"]))}</small>}</li>})}</ol>
        if !list(&r["pending_proposals"]).is_empty(){<h3>{crate::i18n::ex("In attesa di approvazione")}</h3>}
        {for list(&r["pending_proposals"]).iter().map(|pp|{
            let full=list(&props.master["proposals"]).iter().find(|p|p["id"]==pp["id"]).cloned();
            match full{
                Some(p)=>{let key=format!("proposal:{}",p["id"]);
                    html!{<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={props.state["generation"].clone()} preview={props.responses.get(&key).cloned().unwrap_or_default()} busy={props.busy} action={props.action.clone()} roles={roles.to_vec()}/>}}
                None=>html!{<p class="muted">{text(&pp["summary"])}</p>},
            }
        })}
    </section></div>}
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
    }
    #[test]
    fn counts_pending() {
        let m = json!({"inject_responses":[{"pending_proposals":[{"id":1},{"id":2}]},{"pending_proposals":[]},{"pending_proposals":[{"id":3}]}]});
        assert_eq!(pending_count(&m), 3);
    }
}
