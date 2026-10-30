//! EXCON viewer of the conversations players opened on an inject (Risposte
//! tab): a vertical selector on the left, one conversation at a time on the
//! right, drawn with the player's conversation widgets (quoted inject,
//! message bubbles, response lines). Read-only: EXCON supervises, it never
//! writes in a player conversation.
use crate::{
    api::{list, text},
    inject_panel::{conv_bubble, excerpt, quote_card},
    timeline::role_name,
};
use serde_json::Value;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ConvViewerProps {
    /// The `inject_responses` row whose `conversations` are shown.
    pub row: Value,
    /// Master proposals: the state of the responses born in a conversation.
    pub proposals: Value,
    pub roles: Vec<Value>,
    /// Conversation shown first (default: the first one).
    #[prop_or_default]
    pub selected: Option<u64>,
    pub onclose: Callback<()>,
}

/// Number of conversations linked to an inject row (badge in the list).
pub fn count(row: &Value) -> usize {
    list(&row["conversations"]).len()
}

/// Display names of the participants, creator first.
fn people(c: &Value, roles: &[Value]) -> String {
    list(&c["participants"]).iter().map(|x| role_name(roles, text(x))).collect::<Vec<_>>().join(", ")
}

/// The text EXCON reads: English when the console is in English, else the original.
fn body(m: &Value) -> &str {
    if crate::i18n_excon::is_en() || text(&m["original"]).is_empty() {
        text(&m["text_en"])
    } else {
        text(&m["original"])
    }
}

fn state_label(proposals: &Value, pid: u64) -> Option<&'static str> {
    let lang = if crate::i18n_excon::is_en() { "en" } else { "it" };
    list(proposals).iter().find(|p| p["id"] == pid).map(|p| crate::i18n::t(lang, crate::i18n_risposte::response_state(p)))
}

#[function_component(ConversationViewer)]
pub fn conversation_viewer(props: &ConvViewerProps) -> Html {
    let convs = list(&props.row["conversations"]).to_vec();
    let current = use_state(|| props.selected.or_else(|| convs.first().and_then(|c| c["id"].as_u64())));
    let roles = &props.roles;
    let on_key = {
        let onclose = props.onclose.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Escape" {
                e.stop_propagation();
                onclose.emit(())
            }
        })
    };
    let close = {
        let onclose = props.onclose.clone();
        Callback::from(move |_| onclose.emit(()))
    };
    let shown = convs.iter().find(|c| c["id"].as_u64() == *current).or(convs.first());
    let title = if text(&props.row["title"]).is_empty() { crate::i18n::ex("Inject") } else { text(&props.row["title"]) };
    html! {<crate::inject_panel::ModalShell class={classes!("modal-backdrop","conv-viewer-backdrop")} onkeydown={on_key}><section class="inject-modal conv-viewer" role="dialog" aria-modal="true" aria-labelledby="conv-viewer-title">
        <header class="response-head"><div><h2 id="conv-viewer-title">{crate::exfmt!("Conversazioni · {}","Conversations · {}",title)}</h2>
            <p class="muted">{if convs.len()==1{crate::exfmt!("1 conversazione tra player su questo inject. Sola lettura.","1 player conversation on this inject. Read only.")}else{crate::exfmt!("{} conversazioni tra player su questo inject. Sola lettura.","{} player conversations on this inject. Read only.",convs.len())}}</p></div><div class="grow"/>
            <button data-close-conv-viewer="" onclick={close}>{crate::i18n::ex("Chiudi")}</button></header>
        <div class="conv-viewer-cols">
            <nav class="conv-viewer-list" aria-label={crate::exfmt!("Conversazioni","Conversations")}>{for convs.iter().map(|c|{
                let cid=c["id"].as_u64().unwrap_or(0);
                let active=shown.is_some_and(|s|s["id"]==c["id"]);
                let responses=list(&c["responses"]).len();
                let last=list(&c["messages"]).last().map(|m|text(&m["clock"]).to_string()).unwrap_or_default();
                let onclick={let current=current.clone();Callback::from(move |_|current.set(Some(cid)))};
                html!{<button key={cid} class={classes!("conv-viewer-item",active.then_some("active"))} data-conv-viewer-item={cid.to_string()} aria-current={active.then_some("true")} {onclick}>
                    <b>{people(c,roles)}</b>
                    <small>{crate::exfmt!("#{} · {} messaggi","#{} · {} messages",cid,list(&c["messages"]).len())}{if last.is_empty(){String::new()}else{format!(" · {last}")}}</small>
                    if responses>0{<span class="pill conv-response-tag">{if responses==1{crate::exfmt!("1 risposta","1 response")}else{crate::exfmt!("{} risposte","{} responses",responses)}}</span>}
                </button>}
            })}</nav>
            <div class="conv-viewer-thread">{match shown{
                Some(c)=>conversation(props,c),
                None=>html!{<p class="muted">{crate::exfmt!("Nessuna conversazione su questo inject.","No conversation on this inject.")}</p>},
            }}</div>
        </div>
    </section></crate::inject_panel::ModalShell>}
}

/// One conversation, as the player sees it but with EXCON's language and names.
fn conversation(props: &ConvViewerProps, c: &Value) -> Html {
    let roles = &props.roles;
    let r = &props.row;
    let cid = c["id"].as_u64().unwrap_or(0);
    let messages = list(&c["messages"]);
    let delivery = c["delivery"].as_u64();
    let cited = |id: u64| -> Option<(String, String)> {
        if Some(id) == delivery || r["inject_message_id"].as_u64() == Some(id) {
            Some((crate::i18n::ex("Inject").to_string(), text(&r["title"]).to_string()))
        } else {
            messages.iter().find(|m| m["id"].as_u64() == Some(id)).map(|m| (role_name(roles, text(&m["from"])), excerpt(body(m))))
        }
    };
    html! {<div key={cid} class="inj-chat conv-thread" data-conversation={cid.to_string()}>
        <div class="conv-head"><span><b>{people(c,roles)}</b><small>{crate::exfmt!("Conversazione #{} avviata da {}","Conversation #{} started by {}",cid,role_name(roles,text(&c["created_by"])))}</small></span></div>
        {quote_card(&r["inject_message_id"],crate::i18n::ex("Inject"),&format!(" · {} · {}",text(&r["clock"]),role_name(roles,text(&r["role"]))),text(&r["title"]),text(&r["inject_text"]))}
        <ol class="inj-chat-inner" role="log">
            if messages.is_empty(){<li class="inj-sys">{crate::exfmt!("Nessun messaggio","No messages")}</li>}
            {for messages.iter().map(|m|{
                let quote=m["reply_to"].as_u64().and_then(cited);
                let original=text(&m["original"]);
                let extra=html!{<>
                    if let Some(pid)=m["proposal"].as_u64(){<small class="pill conv-response-tag" data-conv-response-tag={pid.to_string()}>{match state_label(&props.proposals,pid){Some(s)=>crate::exfmt!("Risposta registrata #{} · {}","Recorded response #{} · {}",pid,s),None=>crate::exfmt!("Risposta registrata #{}","Recorded response #{}",pid)}}</small>}
                    if crate::i18n_excon::is_en()&&!original.is_empty()&&original!=text(&m["text_en"]){<details><summary>{crate::exfmt!("Originale","Original")}</summary>{original}</details>}
                    if m["translation_failed"]==true{<small class="danger">{crate::exfmt!("Traduzione non riuscita","Translation failed")}</small>}
                </>};
                conv_bubble(false,m,&role_name(roles,text(&m["from"])),quote,body(m),extra)
            })}
            {for list(&c["responses"]).iter().filter_map(|id|id.as_u64()).map(|pid|{
                let by=list(&props.proposals).iter().find(|p|p["id"]==pid).map(|p|role_name(roles,text(&p["source"]["role"]))).unwrap_or_default();
                html!{<li key={format!("r{pid}")} class="inj-sys" data-conv-response={pid.to_string()}>{crate::exfmt!("Risposta #{} di {} · {}","Response #{} by {} · {}",pid,by,state_label(&props.proposals,pid).unwrap_or(""))}</li>}
            })}
        </ol>
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn counts_and_names() {
        let roles = vec![json!({"id":"mayor","name":"Sindaco"}), json!({"id":"police","name":"Polizia"})];
        let row = json!({"conversations":[{"id":4,"participants":["mayor","police"]},{"id":5,"participants":["police"]}]});
        assert_eq!(count(&row), 2);
        assert_eq!(count(&json!({})), 0);
        assert_eq!(people(&row["conversations"][0], &roles), "Sindaco, Polizia");
        let proposals = json!([{"id":9,"status":"pending"}]);
        assert!(state_label(&proposals, 9).is_some());
        assert!(state_label(&proposals, 1).is_none());
    }
}
