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
    /// Replies without any EXCON decision (e.g. superseded or informational).
    Replied,
    /// Every open proposal waits for the player's answer to an EXCON question.
    Clarifying,
    Pending,
    /// Nothing open: the EXCON verdict on the latest decided response.
    Decided(Verdict),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Verdict {
    Approved,
    Partial,
    Rejected,
    Acknowledged,
}

fn verdict(review: &str) -> Verdict {
    match review {
        "partially_approved" => Verdict::Partial,
        "rejected" => Verdict::Rejected,
        "acknowledged" => Verdict::Acknowledged,
        _ => Verdict::Approved,
    }
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
    } else if let Some(d) = list(&r["decided_proposals"]).last() {
        ReplyState::Decided(verdict(text(&d["review"])))
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
        ReplyState::Decided(Verdict::Approved) => crate::i18n::ex("approvata"),
        ReplyState::Decided(Verdict::Partial) => crate::i18n::ex("approvata in parte"),
        ReplyState::Decided(Verdict::Rejected) => crate::i18n::ex("rifiutata"),
        ReplyState::Decided(Verdict::Acknowledged) => crate::i18n::ex("presa visione"),
    }
}

/// CSS tone of a reply state pill.
fn state_tone(s: ReplyState) -> &'static str {
    match s {
        ReplyState::Pending | ReplyState::Clarifying => "pending",
        ReplyState::Replied | ReplyState::Decided(Verdict::Approved | Verdict::Acknowledged) => "sent",
        ReplyState::Decided(Verdict::Partial) => "partial",
        ReplyState::Decided(Verdict::Rejected) => "rejected",
        ReplyState::None => "todo",
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
    /// Identity the PDF viewer reads attachments with.
    pub token: String,
}

#[function_component(Risposte)]
pub fn risposte(props: &RisposteProps) -> Html {
    let opened = use_state(|| Option::<(u64, String)>::None);
    let only_pending = use_state(|| false);
    // PDF being read in the open dialog: (document id, file name).
    let pdf = use_state(|| Option::<(u64, String)>::None);
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
            html!{<li key={format!("{}:{}",k.0,k.1)} data-response={format!("{}:{}",k.0,k.1)}><button class={classes!("response-row",match st{ReplyState::Pending=>"is-pending",ReplyState::Replied|ReplyState::Decided(_)=>"is-replied",ReplyState::Clarifying=>"is-pending",ReplyState::None=>"is-none"})} {onclick}>
                <span class="planned-times"><b>{text(&r["clock"])}</b><small>{format_offset(r["t_s"].as_i64().unwrap_or(0))}</small></span>
                <span class="planned-body"><b>{if text(&r["title"]).is_empty(){crate::i18n::ex("Inject")}else{text(&r["title"])}}</b><span class="planned-text">{text(&r["inject_text"])}</span></span>
                <span class="response-who"><span class="pill rcpt sent">{role_name(&roles,text(&r["role"]))}</span>if let ReplyState::Decided(_)=st{{for list(&r["decided_proposals"]).iter().map(|d|{let v=ReplyState::Decided(verdict(text(&d["review"])));html!{<span key={d["id"].to_string()} class={classes!("pill","reply-state",state_tone(v))} data-reply-state={format!("{v:?}")} title={format!("#{}",d["id"])}>{state_label(v)}</span>}})}}else{<span class={classes!("pill","reply-state",state_tone(st))} data-reply-state={format!("{st:?}")}>{state_label(st)}</span>}</span>
            </button></li>}
        })}</ol>
        if let Some(r)=selected{{dialog(props,&r,&roles,&pdf,{let o=opened.clone();let pdf=pdf.clone();Callback::from(move |_|{pdf.set(None);o.set(None)})})}}
    </section>}
}

fn common(props: &RisposteProps) -> crate::controllo::CommonProps {
    crate::controllo::CommonProps {
        stat: props.stat.clone(),
        state: props.state.clone(),
        master: props.master.clone(),
        responses: props.responses.clone(),
        busy: props.busy,
        error: "".into(),
        done: 0,
        action: props.action.clone(),
        onclose: Callback::noop(),
    }
}

fn dialog(props: &RisposteProps, r: &Value, roles: &[Value], pdf: &UseStateHandle<Option<(u64, String)>>, onclose: Callback<()>) -> Html {
    let st = reply_state(r);
    let on_key = {
        let onclose = onclose.clone();
        Callback::from(move |e: KeyboardEvent| if e.key() == "Escape" { onclose.emit(()) })
    };
    let on_pdf = {
        let pdf = pdf.clone();
        Callback::from(move |doc: (u64, String)| pdf.set(Some(doc)))
    };
    let has_reply_column = list(&r["pending_proposals"]).iter().any(is_open)
        || list(&props.master["proposals"]).iter().any(|p| approved_for(r, p));
    html! {<crate::inject_panel::ModalShell class={classes!("modal-backdrop")} onkeydown={on_key}><section class="inject-modal response-dialog" role="dialog" aria-modal="true" aria-labelledby="response-dialog-title">
        <header class="response-head"><div><h2 id="response-dialog-title">{format!("{} · {}",text(&r["title"]),role_name(roles,text(&r["role"])))}</h2>
        <p class="muted">{format!("{} · {}",text(&r["clock"]),state_label(st))}</p></div><div class="grow"/><button onclick={Callback::from(move |_|onclose.emit(()))}>{crate::i18n::ex("Chiudi")}</button></header>
        <div class="response-cols">
            <div class="response-col response-col-thread" data-response-col="thread">{thread_view(props,r,roles,&on_pdf)}</div>
            <div class="response-col response-col-reply" data-response-col="reply"><h3 class="response-col-title">{crate::i18n::ex("Risposta da gestire")}</h3>
                {decisions_view(r)}
                if let Some((id,name))=pdf.as_ref(){<crate::pdf_view::PdfViewer key={*id} token={props.token.clone()} id={*id} name={name.clone()} onclose={{let pdf=pdf.clone();Callback::from(move |_|pdf.set(None))}}/>}
                if !has_reply_column&&list(&r["decided_proposals"]).is_empty(){<p class="muted">{crate::i18n::ex("Nessuna risposta da gestire.")}</p>}
                {for list(&props.master["proposals"]).iter().filter(|p|approved_for(r,p)).map(|p|html!{<>
                    {via_conversation(r,p,roles)}
                    {basis_line(r,&p["id"],&on_pdf)}
                    <Proposal key={p["id"].to_string()} proposal={p.clone()} generation={props.state["generation"].clone()} preview={Value::Null} busy={props.busy} action={props.action.clone()} roles={roles.to_vec()} common={Some(common(props))}/>
                </>})}
                if list(&r["pending_proposals"]).iter().any(is_open){<h3>{if st==ReplyState::Clarifying{crate::i18n::ex("In attesa di chiarimento")}else{crate::i18n::ex("In attesa di approvazione")}}</h3>}
                {for list(&r["pending_proposals"]).iter().filter(|pp|is_open(pp)).map(|pp|{
                    let full=list(&props.master["proposals"]).iter().find(|p|p["id"]==pp["id"]).cloned();
                    match full{
                        Some(p)=>{let key=format!("proposal:{}",p["id"]);
                            html!{<>{via_conversation(r,&p,roles)}{basis_line(r,&p["id"],&on_pdf)}<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={props.state["generation"].clone()} preview={props.responses.get(&key).cloned().unwrap_or_default()} busy={props.busy} action={props.action.clone()} roles={roles.to_vec()} common={Some(common(props))}/></>}}
                        None=>html!{<p class="muted">{text(&pp["summary"])}</p>},
                    }
                })}
            </div>
        </div>
    </section></crate::inject_panel::ModalShell>}
}

/// A narrative proposal already decided that belongs to this row (the model
/// lists each response under exactly one row, see `decided_proposals`).
fn approved_for(r: &Value, p: &Value) -> bool {
    p["narrative"] == true
        && ["approved", "applied", "failed"].contains(&text(&p["status"]))
        && list(&r["decided_proposals"]).iter().any(|d| d["id"] == p["id"])
}

/// State, kind and recipient of the response a thread message carries.
fn response_line(r: &Value, m: &Value, roles: &[Value]) -> Html {
    let Some(reply) = list(&r["replies"]).iter().find(|x| x["id"] == m["id"]) else { return html! {} };
    let resp = &reply["response"];
    if resp.is_null() {
        return html! {};
    }
    let en = crate::i18n_excon::is_en();
    let lang = if en { "en" } else { "it" };
    let key = crate::i18n_risposte::response_state(resp);
    let to = &resp["addressed_to"];
    let recipient = if list(&to["players"]).is_empty() {
        text(&to["persona"]).to_string()
    } else {
        list(&to["players"]).iter().map(|x| role_name(roles, text(x))).collect::<Vec<_>>().join(", ")
    };
    html! {<div class="response-line" data-response-line={m["id"].to_string()}>
        <span class="pill kind">{crate::i18n::t(lang, if resp["communication"] == true { "kind_tag_comm" } else { "kind_tag_dec" })}</span>
        if !recipient.is_empty() {<span class="pill">{crate::i18n::tf(lang, "resp_to_label", &[&recipient])}</span>}
        if reply["from"] != r["role"] {<span class="pill">{crate::exfmt!("da {}", "from {}", role_name(roles, text(&reply["from"])))}</span>}
        <span class={classes!("pill","response-state",crate::i18n_risposte::state_tone(key))} data-response-state={key}>{crate::i18n::t(lang, key)}</span>
    </div>}
}

/// Same wording as the player's composer: Piano o procedura / Legge o norma / Giudizio operativo.
fn basis_name(kind: &str) -> &'static str {
    let lang = if crate::i18n_excon::is_en() { "en" } else { "it" };
    crate::i18n::t(lang, match kind {
        "plan" | "procedure" => "basis_plan",
        "law" => "basis_law",
        _ => "basis_judgment",
    })
}

/// What a reply rests on and the PDF behind it, with the button that opens it.
fn basis_chip(basis: &Value, document: &Value, on_pdf: &Callback<(u64, String)>) -> Html {
    if !basis.is_string() && !document.is_object() {
        return html! {};
    }
    let doc = document.as_object().map(|_| (document["id"].as_u64().unwrap_or(0), text(&document["name"]).to_string()));
    html! {<div class="reply-basis" data-reply-basis={text(basis).to_string()}>
        <span class="pill reply-basis-kind">{crate::exfmt!("Fondamento: {}","Basis: {}",basis_name(text(basis)))}</span>
        if let Some((id,name))=doc{<button type="button" class="reply-doc" data-open-pdf={id.to_string()} onclick={{let on_pdf=on_pdf.clone();let name=name.clone();Callback::from(move |_|on_pdf.emit((id,name.clone())))}}>{format!("PDF · {name}")}</button>}
    </div>}
}

/// The basis line of the reply that opened proposal `pid`.
fn basis_line(r: &Value, pid: &Value, on_pdf: &Callback<(u64, String)>) -> Html {
    match list(&r["replies"]).iter().find(|m| m["proposal"] == *pid) {
        Some(m) => basis_chip(&m["basis"], &m["document"], on_pdf),
        None => html! {},
    }
}

/// The EXCON verdicts already given on this recipient's responses, newest last.
fn decisions_view(r: &Value) -> Html {
    let decided = list(&r["decided_proposals"]);
    if decided.is_empty() {
        return html! {};
    }
    html! {<div class="response-decisions"><h3>{crate::i18n::ex("Decisioni EXCON")}</h3><ul>{for decided.iter().map(|d|{
        let st=ReplyState::Decided(verdict(text(&d["review"])));
        html!{<li key={d["id"].to_string()} data-decided-proposal={d["id"].to_string()}><span class={classes!("pill","reply-state",state_tone(st))}>{state_label(st)}</span>
            {format!(" #{}",d["id"])}
            if d["rejected"].as_u64().unwrap_or(0)>0{<small>{crate::exfmt!(" · {} approvate, {} non approvate"," · {} approved, {} not approved",d["approved"],d["rejected"])}</small>}
            if !text(&d["note"]).is_empty(){<small class="muted">{format!(" · {}",text(&d["note"]))}</small>}</li>}
    })}</ul></div>}
}

/// Who answered through which player conversation, when it is not the inject's recipient.
fn via_conversation(r: &Value, p: &Value, roles: &[Value]) -> Html {
    let Some(cid) = p["via_conversation"].as_u64() else { return html! {} };
    let conv = list(&r["conversations"]).iter().find(|c| c["id"] == cid);
    let people = conv.map(|c| list(&c["participants"]).iter().map(|x| role_name(roles, text(x))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    html! {<div class="response-via-block"><p class="muted response-via" data-via-conversation={cid.to_string()}>{crate::exfmt!("Risposta di {} tramite la conversazione #{} ({}).","Response by {} through conversation #{} ({}).",role_name(roles,text(&p["source"]["role"])),cid,people)}</p>
        {transcript(cid,conv,roles)}</div>}
}

/// Collapsible transcript of a linked conversation.
fn transcript(cid: u64, conv: Option<&Value>, roles: &[Value]) -> Html {
    let en = crate::i18n_excon::is_en();
    let messages = list(&conv.map(|c| c["messages"].clone()).unwrap_or_default()).to_vec();
    html! {<details class="conversation-transcript" data-conversation-transcript={cid.to_string()}><summary>{crate::exfmt!("Trascrizione ({} messaggi)","Transcript ({} messages)",messages.len())}</summary>
        <ol>{for messages.iter().map(|m|html!{<li key={m["id"].to_string()} data-conv-message={m["id"].to_string()}><small>{format!("{} · {}",text(&m["clock"]),role_name(roles,text(&m["from"])))}</small>
            <p>{if en||text(&m["original"]).is_empty(){text(&m["text_en"])}else{text(&m["original"])}}</p>
            if m["proposal"].is_number(){<small class="pill conv-response-tag">{crate::exfmt!("Risposta registrata #{}","Recorded response #{}",m["proposal"])}</small>}
            if m["reply_to"].is_number(){<small>{crate::exfmt!("In risposta a #{}","In reply to #{}",m["reply_to"])}</small>}</li>})}</ol></details>}
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

fn thread_view(props: &RisposteProps, r: &Value, roles: &[Value], on_pdf: &Callback<(u64, String)>) -> Html {
    let messages = thread_messages(r);
    let player = text(&r["role"]).to_string();
    html! {<div class="response-thread" data-response-thread={r["inject_message_id"].to_string()}>
        <h3>{format!("Thread · {}",crate::i18n_excon::count_messages(messages.len()))}</h3>
        if list(&r["replies"]).is_empty(){<p class="muted">{crate::i18n::ex("Nessuna risposta")}</p>}
        <ol class="thread-list">{for messages.iter().map(|m|{
            let kind=thread_kind(m);
            let from=text(&m["from"]);
            let own=from==player;
            let original=text(&m["original"]);
            html!{<li key={m["id"].to_string()} data-thread-message={m["id"].to_string()} data-clarification-thread={m["clarification_thread"].as_u64().map(|id|id.to_string())} class={classes!("thread-msg",format!("kind-{kind}"),own.then_some("from-player"),m["clarification_thread"].is_number().then_some("in-clarification"))}>
                <div class="thread-head"><b class="thread-from">{crate::render::role_label(roles,from)}</b><span class="thread-arrow" aria-hidden="true">{"→"}</span><span class="thread-to">{crate::render::role_label(roles,text(&m["to"]))}</span><span class="pill thread-kind">{kind_label(kind)}</span><div class="grow"/><time>{text(&m["clock"])}</time></div>
                if !text(&m["title"]).is_empty(){<b class="thread-title">{text(&m["title"])}</b>}
                <p class="thread-text">{text(&m["text"])}</p>
                {response_line(r,m,roles)}
                {basis_chip(&m["basis"],&m["document"],on_pdf)}
                if !original.is_empty()&&original!=text(&m["text"]){<small class="muted">{crate::exfmt!("Originale: {}","Original: {}",original)}</small>}
                if m["reply_to"].is_number(){<small class="thread-reply-to">{crate::exfmt!("In risposta al messaggio #{}","In reply to message #{}",m["reply_to"])}</small>}
            </li>}
        })}</ol>
        {for list(&r["conversations"]).iter().map(|c|{
            let cid=c["id"].as_u64().unwrap_or(0);
            let people=list(&c["participants"]).iter().map(|x|role_name(roles,text(x))).collect::<Vec<_>>().join(", ");
            html!{<div key={cid} class="response-via-block" data-linked-conversation={cid.to_string()}>
                <p class="muted response-via">{crate::exfmt!("Conversazione #{} nata dall’inject ({})","Conversation #{} started from the inject ({})",cid,people)}</p>
                {transcript(cid,Some(c),roles)}</div>}
        })}
        {for list(&r["clarification_threads"]).iter().map(|c|{
            let id=c["id"].as_u64().unwrap_or(0);
            let closed=c["status"]=="closed";
            let close={let a=props.action.clone();Callback::from(move |_|a.emit(Action::post(format!("/api/master/clarifications/{id}/close"),serde_json::json!({}))))};
            html!{<div key={id} class={classes!("clarification-bar",closed.then_some("is-closed"))} data-clarification-bar={id.to_string()}>
                <span class={classes!("pill",(c["status"]=="awaiting_player").then_some("pending"))}>{match text(&c["status"]){"awaiting_player"=>crate::i18n::ex("Chiarimento: attende il player"),"closed"=>crate::i18n::ex("Chiarimento chiuso"),_ if !messages.iter().any(|m|m["clarification_thread"]==c["id"]&&m["kind"]=="clarification")=>crate::i18n::ex("Chiarimento: domanda in consegna al player"),_=>crate::i18n::ex("Chiarimento: attende EXCON")}}</span>
                if !closed{<button data-close-clarification={id.to_string()} disabled={props.busy} onclick={close}>{crate::i18n::ex("Chiudi chiarimento")}</button>}
            </div>}
        })}
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
        // After a decision the row shows the verdict, not just "reply received".
        let decided = |review: &str| json!({"replies":[{}],"pending_proposals":[],"decided_proposals":[{"id":1,"review":"approved"},{"id":2,"review":review}]});
        assert_eq!(reply_state(&decided("rejected")), ReplyState::Decided(Verdict::Rejected));
        assert_eq!(reply_state(&decided("partially_approved")), ReplyState::Decided(Verdict::Partial));
        assert_eq!(reply_state(&decided("acknowledged")), ReplyState::Decided(Verdict::Acknowledged));
        assert_eq!(state_label(reply_state(&decided("rejected"))), "rifiutata");
        // An open proposal still comes first.
        let reopened = json!({"replies":[{}],"pending_proposals":[{"id":3}],"decided_proposals":[{"id":2,"review":"rejected"}]});
        assert_eq!(reply_state(&reopened), ReplyState::Pending);
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
        // Clarification entries keep their thread tag inside the same list.
        let tagged = json!({"inject_message_id":1,"role":"mayor","thread":[{"id":1,"kind":"inject"},{"id":2,"kind":"clarification","clarification_thread":7}]});
        assert_eq!(thread_messages(&tagged)[1]["clarification_thread"], 7);
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
