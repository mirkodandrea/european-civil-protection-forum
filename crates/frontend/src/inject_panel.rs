//! Player "Inject e risposte" panel. The list has one compact row per inject
//! (title, time, sender, outcome badge and one line about the last event of
//! its conversation; items needing the player come first). A row opens a
//! modal with the whole conversation as one chronological chat (the inject,
//! the player's proposals "Tu", EXCON decisions and clarification questions)
//! and the composer at the bottom.
use crate::{
    api::{list, text},
    i18n,
    timeline::format_offset,
    App, Msg,
};
use serde_json::Value;
use yew::prelude::*;


/// One list row: an inject with everything that belongs to it, or the
/// proposals that answer no inject.
struct Item<'a> {
    key: String,
    msg: Option<&'a Value>,
    proposals: Vec<&'a Value>,
    threads: Vec<&'a Value>,
}

/// Badge text key, CSS tone and whether the player has to act.
struct Outcome {
    label: &'static str,
    tone: &'static str,
    action: bool,
}

fn has_status(t: &Value, s: &str) -> bool {
    t["status"] == s
}

fn outcome(l: &str, it: &Item) -> Outcome {
    let o = |key, tone, action| Outcome { label: i18n::t(l, key), tone, action };
    if it.threads.iter().any(|t| has_status(t, "awaiting_player")) {
        return o("clarif_needed", "action", true);
    }
    let Some(latest) = it.proposals.iter().rev().find(|p| p["status"] != "superseded").or(it.proposals.last()) else {
        return o("inj_todo", "action", true);
    };
    match text(&latest["status"]) {
        "rejected" => o("st_rejected", "bad", true),
        "failed" => o("st_failed", "bad", false),
        "approved" | "applied" => o("st_approved", "done", false),
        "needs_clarification" => o("clarif_needed", "action", true),
        _ if it.threads.iter().any(|t| has_status(t, "awaiting_excon")) => o("inj_clar_wait", "wait", false),
        _ if !it.threads.is_empty() && it.threads.iter().all(|t| has_status(t, "closed")) => o("thread_closed", "wait", false),
        _ => o("inj_sent", "wait", false),
    }
}

/// What kind of event a chat line is.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Inject,
    Proposal,
    /// EXCON note on an approved proposal.
    Decision,
    /// EXCON rejection reason.
    Reason,
    Question,
    Closed,
}

/// One line of the conversation of an inject.
struct Ev {
    kind: Kind,
    who: String,
    clock: String,
    text: String,
    /// Proposal status (proposals only).
    status: String,
    executions: Value,
}

/// The conversation of an inject in order: each proposal is followed by
/// EXCON's decision note and by the questions attached to it, so the order
/// holds even when every timestamp is equal.
fn events(l: &str, it: &Item, name: &dyn Fn(&str) -> String) -> Vec<Ev> {
    let entries: Vec<&Value> = it.threads.iter().flat_map(|t| list(&t["entries"]).iter()).collect();
    let ev = |kind, who: String, clock: String, text: &str| Ev { kind, who, clock, text: text.to_string(), status: String::new(), executions: Value::Null };
    let mut out = Vec::new();
    if let Some(m) = it.msg {
        out.push(ev(Kind::Inject, name(text(&m["from"])), text(&m["clock"]).to_string(), text(&m["text"])));
    }
    for p in &it.proposals {
        let status = text(&p["status"]);
        // The player's answers are proposals too: take their time from the thread.
        let when = entries.iter().find(|e| e["author"] == "player" && e["proposal"] == p["id"]).map(|e| text(&e["clock"]).to_string()).unwrap_or_else(|| format_offset(p["t_s"].as_i64().unwrap_or(0)));
        out.push(Ev { status: status.to_string(), executions: p["executions"].clone(), ..ev(Kind::Proposal, i18n::t(l, "thread_you").to_string(), when, text(&p["original"])) });
        let note = text(&p["decision"]["note"]);
        if !note.is_empty() {
            let kind = if status == "rejected" { Kind::Reason } else { Kind::Decision };
            out.push(ev(kind, "EXCON".into(), format_offset(p["decision"]["t_s"].as_i64().unwrap_or(0)), note));
        }
        for e in entries.iter().filter(|e| e["author"] == "excon" && e["proposal"] == p["id"]) {
            out.push(ev(Kind::Question, "EXCON".into(), text(&e["clock"]).to_string(), text(&e["text"])));
        }
    }
    for _ in it.threads.iter().filter(|t| has_status(t, "closed")) {
        out.push(ev(Kind::Closed, String::new(), String::new(), i18n::t(l, "thread_closed")));
    }
    out
}

/// One line for the list: who, when and a short text of the last event.
fn last_line(l: &str, e: &Ev) -> (String, String, String) {
    match e.kind {
        Kind::Proposal => (e.who.clone(), e.clock.clone(), format!("{} ({})", e.text, i18n::status(l, &e.status).to_lowercase())),
        // The row already names the sender and the time of the inject.
        Kind::Inject => (String::new(), String::new(), e.text.clone()),
        Kind::Reason => (i18n::t(l, "inj_rejected_by").to_string(), e.clock.clone(), e.text.clone()),
        _ => (e.who.clone(), e.clock.clone(), e.text.clone()),
    }
}

#[derive(Properties, PartialEq)]
pub struct ModalShellProps {
    pub class: Classes,
    #[prop_or_default]
    pub onkeydown: Callback<KeyboardEvent>,
    #[prop_or_default]
    pub onclick: Callback<MouseEvent>,
    pub children: Children,
}

/// Modal backdrop that takes the keyboard focus when it opens, so Escape reaches it.
#[function_component(ModalShell)]
pub fn modal_shell(props: &ModalShellProps) -> Html {
    let node = use_node_ref();
    {
        let node = node.clone();
        use_effect_with((), move |_| {
            if let Some(el) = node.cast::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
            || ()
        });
    }
    html! {<div ref={node} class={props.class.clone()} tabindex="-1" onkeydown={props.onkeydown.clone()} onclick={props.onclick.clone()}>{props.children.clone()}</div>}
}

impl App {
    /// Inject rows of a turn (most urgent first) and the live proposals they are built from.
    fn inject_items(&self, l: &str, turn: u64) -> (Vec<Item<'_>>, Vec<&Value>) {
        let injects: Vec<&Value> = self.messages().iter().filter(|m| m["kind"] == "inject").collect();
        let proposals: Vec<&Value> = list(&self.state["proposals"]).iter().filter(|p| p["status"] != "rolled_back").collect();
        let all_threads = list(&self.state["clarification_threads"]);
        let linked = |m: &Value, p: &Value| p["reply_to"] == m["id"] || m["inject_id"].is_number() && p["inject_id"] == m["inject_id"];
        let mut items: Vec<Item> = injects
            .iter()
            .filter(|m| m["turn"] == turn)
            .map(|m| Item {
                key: m["id"].to_string(),
                msg: Some(*m),
                proposals: proposals.iter().filter(|p| linked(m, p)).copied().collect(),
                threads: all_threads.iter().filter(|t| m["inject_id"].is_number() && t["inject_id"] == m["inject_id"]).collect(),
            })
            .collect();
        // Newest first; the stable sort then lifts open questions, then what else needs the player.
        items.sort_by_key(|it| std::cmp::Reverse(it.msg.map(|m| (m["t_s"].as_i64().unwrap_or(0), m["id"].as_u64().unwrap_or(0)))));
        let free: Vec<&Value> = proposals.iter().filter(|p| p["turn"] == turn && !injects.iter().any(|m| linked(m, p))).copied().collect();
        if !free.is_empty() {
            items.push(Item { key: "free".into(), msg: None, proposals: free, threads: all_threads.iter().filter(|t| t["inject_id"].is_null()).collect() });
        }
        items.sort_by_key(|it| if it.threads.iter().any(|t| has_status(t, "awaiting_player")) { 0 } else if outcome(l, it).action { 1 } else { 2 });
        (items, proposals)
    }

    /// Injects of the current turn that wait for the player (nav badge).
    pub fn inject_todo(&self) -> usize {
        let l = self.lang();
        let turn = self.state["turn"].as_u64().unwrap_or(0);
        self.inject_items(&l, turn).0.iter().filter(|it| outcome(&l, it).action).count()
    }

    /// The list of injects (and, if one is opened, its modal). `turn` is the selected turn.
    pub fn view_inject_panel(&self, ctx: &Context<Self>, can_act: bool, turn: u64) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let (items, proposals) = self.inject_items(l, turn);
        if items.is_empty() {
            return html! {<div class="tactical-empty"><b>{if self.state["phase"]=="briefing"{i18n::t(l,"awaiting_start")}else{i18n::t(l,"no_events")}}</b><p>{i18n::t(l,"empty_hint")}</p></div>};
        }
        let todo = items.iter().filter(|it| outcome(l, it).action).count();
        // The open modal follows the live data: it is looked up by key on every render.
        let open_key = self.field("inj:open", "");
        let opened = items.iter().find(|it| it.key == open_key);
        let name = |id: &str| self.name(id);
        html! {<div class="inject-md">
            if todo>0{<p class="inj-count" role="status">{i18n::tf(l,"inj_todo_count",&[&todo.to_string()])}</p>}
            <ul class="inject-list">{for items.iter().map(|it|{
                let o=outcome(l,it);
                let pick={let key=it.key.clone();ctx.link().callback(move |_|Msg::Field("inj:open".into(),key.clone()))};
                let (title,meta)=match it.msg{
                    Some(m)=>(text(&m["title"]).to_string(),format!("{} · {}",text(&m["clock"]),i18n::tf(l,"from",&[&name(text(&m["from"]))]))),
                    None=>(i18n::t(l,"inj_free").to_string(),String::new()),
                };
                let last=events(l,it,&name).last().map(|e|last_line(l,e));
                html!{<li key={it.key.clone()}><button class={classes!("inj-row",o.action.then_some("needs-action"))} data-inject-row={it.key.clone()} onclick={pick}>
                    <span class="inj-main"><b>{title}</b><small>{meta}</small>
                        if let Some((who,when,body))=last{<small class="inj-last">{if who.is_empty(){body}else{format!("{}{}: {}",who,if when.is_empty(){String::new()}else{format!(" · {when}")},body)}}</small>}
                    </span>
                    <span class={classes!("pill","inj-badge",o.tone)}>{o.label}</span></button></li>}
            })}</ul>
            if let Some(it)=opened{{self.view_inject_modal(ctx,can_act,it,&proposals)}}
        </div>}
    }

    fn view_inject_modal(&self, ctx: &Context<Self>, can_act: bool, it: &Item, all_proposals: &[&Value]) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let o = outcome(l, it);
        let close = ctx.link().callback(|_| Msg::Field("inj:open".into(), String::new()));
        let chat: Vec<Html> = events(l, it, &|id: &str| self.name(id))
            .into_iter()
            .map(|e| {
                let (mine, cls, tag): (bool, String, Option<Html>) = match e.kind {
                    Kind::Inject => (false, "inject".into(), Some(html! {<span class="pill kind">{i18n::t(l,"inject")}</span>})),
                    Kind::Proposal => (true, format!("proposal st-{}", e.status), Some(html! {<span class={classes!("pill",(e.status=="rejected").then_some("bad"))}>{i18n::status(l,&e.status)}</span>})),
                    Kind::Decision => (false, "decision".into(), None),
                    Kind::Reason => (false, "decision rejected".into(), Some(html! {<span class="pill bad">{i18n::t(l,"reason")}</span>})),
                    Kind::Question => (false, "question".into(), Some(html! {<span class="pill kind">{i18n::t(l,"inj_clar_label")}</span>})),
                    Kind::Closed => return html! {<li class="inj-sys">{e.text}</li>},
                };
                let extra = if list(&e.executions).is_empty() { html! {} } else { html! {<details><summary>{i18n::t(l,"exec_results")}</summary>{crate::render::render_value("executions",&e.executions)}</details>} };
                html! {<li class={classes!("inj-msg",if mine{"mine"}else{"theirs"},cls)}>
                    <small><b>{e.who}</b>{format!(" · {}",e.clock)}{tag}</small><p>{e.text}</p>{extra}</li>}
            })
            .collect();
        let awaiting = it.threads.iter().find(|t| has_status(t, "awaiting_player")).copied();
        let incoming = it.threads.iter().any(|t| has_status(t, "awaiting_excon") && !list(&t["entries"]).iter().any(|e| e["author"] == "excon"));
        let composer = if let Some(t) = awaiting {
            // "Rivedi l'azione" re-opens the structured proposal behind the question.
            let pid = t["current_proposal"].to_string();
            let parent = all_proposals.iter().find(|p| p["id"] == t["current_proposal"]).and_then(|p| p["reply_to"].as_u64()).map(|n| n.to_string()).unwrap_or_default();
            let revise = {
                let link = ctx.link().clone();
                Callback::from(move |_| {
                    link.send_message(Msg::OpenWork(Some(parent.clone())));
                    link.send_message(Msg::Field("work:clarify".into(), pid.clone()));
                })
            };
            let revise = html! {<button class="link-button" data-reply-clarification={t["current_proposal"].to_string()} disabled={!can_act} onclick={revise}>{i18n::t(l,"thread_revise")}</button>};
            html! {<footer class="inj-composer">{self.clarify_composer(ctx,can_act,t,revise)}</footer>}
        } else if let Some(m) = it.msg {
            let label = if o.tone == "bad" { "reply_again" } else if it.proposals.is_empty() { "reply" } else { "another_reply" };
            let id = m["id"].to_string();
            let open = ctx.link().callback(Msg::OpenWork);
            html! {<footer class="inj-composer row">
                if incoming{<small class="muted" role="status">{i18n::t(l,"thread_incoming")}</small>}
                else if it.threads.iter().any(|t| has_status(t,"awaiting_excon")){<small class="muted" role="status">{i18n::t(l,"thread_wait")}</small>}
                <div class="grow"/>
                <button class={classes!("reply-inject",it.proposals.is_empty().then_some("primary"))} disabled={!can_act} data-reply-inject={id.clone()} onclick={Callback::from(move |_|open.emit(Some(id.clone())))}>{i18n::t(l,label)}</button></footer>}
        } else {
            html! {}
        };
        let head = match it.msg {
            Some(m) => html! {<><h2 id="inj-modal-title">{text(&m["title"])}</h2><small>{format!("{} · {}",text(&m["clock"]),i18n::tf(l,"from",&[&self.name(text(&m["from"]))]))}</small></>},
            None => html! {<h2 id="inj-modal-title">{i18n::t(l,"inj_free")}</h2>},
        };
        // Escape or a click on the backdrop closes the modal.
        let on_key = { let close = close.clone(); Callback::from(move |e: KeyboardEvent| if e.key() == "Escape" { e.stop_propagation(); close.emit(()) }) };
        let on_backdrop = { let close = close.clone(); Callback::from(move |e: MouseEvent| if e.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok()).is_some_and(|el| el.class_name().split_whitespace().any(|c| c == "inject-backdrop")) { close.emit(()) }) };
        html! {<ModalShell class={classes!("modal-backdrop","inject-backdrop")} onkeydown={on_key} onclick={on_backdrop}>
            <section class="inject-modal inj-modal" role="dialog" aria-modal="true" aria-labelledby="inj-modal-title" data-inject-detail={it.key.clone()}>
                <header class="inj-detail-head"><div>{head}</div><span class={classes!("pill","inj-badge",o.tone)}>{o.label}</span>
                    <button onclick={move |_| close.emit(())}>{i18n::t(l,"close")}</button></header>
                <div class="inj-chat"><ol class="inj-chat-inner" role="log" aria-live="polite">{for chat.into_iter()}</ol></div>
                {composer}
            </section></ModalShell>}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn item<'a>(proposals: Vec<&'a Value>, threads: Vec<&'a Value>) -> Item<'a> {
        Item { key: "1".into(), msg: None, proposals, threads }
    }
    #[test]
    fn outcome_flags_what_needs_the_player() {
        let rejected = json!({"status":"rejected"});
        let pending = json!({"status":"pending"});
        let asked = json!({"status":"awaiting_player"});
        let closed = json!({"status":"closed"});
        assert!(outcome("it", &item(vec![], vec![])).action);
        assert!(outcome("it", &item(vec![&rejected], vec![])).action);
        assert!(outcome("it", &item(vec![&pending], vec![&asked])).action);
        assert!(!outcome("it", &item(vec![&pending], vec![])).action);
        assert_eq!(outcome("it", &item(vec![&pending], vec![&closed])).tone, "wait");
    }
    #[test]
    fn last_line_names_the_last_event() {
        let name = |id: &str| id.to_string();
        let inject = json!({"from":"dispatch","clock":"Day 1 13:00","text":"Fire"});
        let p1 = json!({"id":1,"status":"superseded","original":"F4","t_s":0});
        let p2 = json!({"id":2,"status":"pending","original":"2 bus","t_s":0});
        let rej = json!({"id":3,"status":"rejected","original":"D6","t_s":0,"decision":{"note":"too few buses","t_s":0}});
        let th = json!({"id":1,"status":"awaiting_player","entries":[{"author":"excon","proposal":1,"clock":"13:05","text":"How many buses?"}]});
        let line = |it: &Item| last_line("it", events("it", it, &name).last().unwrap());
        // Only the inject: its own text (the row shows sender and time).
        let mut it = Item { key: "1".into(), msg: Some(&inject), proposals: vec![], threads: vec![] };
        assert_eq!(line(&it), (String::new(), String::new(), "Fire".into()));
        // EXCON's question follows the proposal it is attached to, the answer comes after it.
        it.proposals = vec![&p1];
        it.threads = vec![&th];
        assert_eq!(line(&it), ("EXCON".into(), "13:05".into(), "How many buses?".into()));
        it.proposals = vec![&p1, &p2];
        let (who, _, body) = line(&it);
        assert_eq!((who.as_str(), body.contains("2 bus")), ("Tu", true));
        // A rejection reports EXCON's reason.
        it.proposals = vec![&rej];
        it.threads = vec![];
        let (who, _, body) = line(&it);
        assert_eq!((who.as_str(), body.as_str()), ("EXCON ha rifiutato", "too few buses"));
        assert_eq!(events("it", &it, &name).len(), 3);
        // A closed thread ends the conversation.
        let closed = json!({"id":1,"status":"closed","entries":[]});
        it.threads = vec![&closed];
        assert_eq!(line(&it).2, "Chiarimento chiuso");
    }
}
