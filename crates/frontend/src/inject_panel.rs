//! Player "Risposte" panel. The list has one compact row per inject
//! (title, time, sender, outcome badge and one line about the last event of
//! its thread; items needing the player come first). A row opens a drill-down
//! in place of the list (back to the list, like the conversations view): the
//! whole thread as one chronological chat (the inject, the player's responses
//! "Tu", decisions and clarification questions), the conversations linked to
//! the inject, and the actions: respond, or talk it over with other players.
//! A conversation shows the quoted inject; a response can come out of it, even
//! from a player who never received the inject.
use crate::{
    api::{list, text},
    i18n,
    timeline::format_offset,
    Msg,
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
    /// Conversations of this player linked to the inject.
    convs: Vec<&'a Value>,
    /// The inject was never delivered to this player: it is read through a conversation.
    guest: bool,
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
        return if it.guest { o("conv_via_guest", "wait", false) } else { o("inj_todo", "action", true) };
    };
    if latest["narrative"] == true && latest["carries_actions"] != true && latest["status"] == "approved" {
        return o(if list(&latest["narrative_outcomes"]).is_empty() {"narrative_approved"} else {"narrative_resolved"}, "done", false);
    }
    match (text(&latest["review"]), text(&latest["status"])) {
        ("acknowledged", _) => return o("st_acknowledged", "done", false),
        ("partially_approved", "approved" | "applied") => return o("st_partially_approved", "partial", false),
        _ => {}
    }
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
    Answer,
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
    /// Actions declared next to a narrative (a plain action response is its own
    /// text), or each action's verdict after a partial approval (`false` = not approved).
    actions: Vec<(String, bool)>,
}

/// The conversation of an inject in order: each proposal is followed by
/// EXCON's decision note and by the questions attached to it, so the order
/// holds even when every timestamp is equal.
fn events(l: &str, it: &Item, name: &dyn Fn(&str) -> String) -> Vec<Ev> {
    let entries: Vec<&Value> = it.threads.iter().flat_map(|t| list(&t["entries"]).iter()).collect();
    // Who speaks for exercise control: the persona stored with the item, else the default one.
    let persona = |v: &Value| -> String {
        let p = text(&v["persona"]);
        if p.is_empty() { name("master") } else { p.to_string() }
    };
    let ev = |kind, who: String, clock: String, text: &str| Ev { kind, who, clock, text: text.to_string(), status: String::new(), executions: Value::Null, actions: Vec::new() };
    let mut out = Vec::new();
    if let Some(m) = it.msg {
        let from = if m["persona"].is_string() { persona(m) } else { name(text(&m["from"])) };
        out.push(ev(Kind::Inject, from, text(&m["clock"]).to_string(), text(&m["text"])));
    }
    for p in &it.proposals {
        let status = text(&p["status"]);
        // The player's answers are proposals too: take their time from the thread.
        let when = entries.iter().find(|e| e["author"] == "player" && e["proposal"] == p["id"]).map(|e| text(&e["clock"]).to_string()).unwrap_or_else(|| format_offset(p["t_s"].as_i64().unwrap_or(0)));
        let rejected = list(&p["decision"]["rejected_actions"]);
        let partial = p["review"] == "partially_approved";
        let mut actions: Vec<(String, bool)> = if p["carries_actions"] == true || partial {
            list(&p["commands"]).iter().map(|c| {
                let s = crate::widgets::command_summary(l, c);
                (if partial { format!("✓ {s} · {}", i18n::t(l, "act_approved")) } else { s }, true)
            }).collect()
        } else { Vec::new() };
        // Which actions were not approved, and why (a whole rejection has the reason below).
        if partial {
            actions.extend(rejected.iter().map(|r| {
                let note = text(&r["note"]);
                (format!("✗ {} · {}{}", crate::widgets::command_summary(l, &r["command"]), i18n::t(l, "act_not_approved"), if note.is_empty() { String::new() } else { format!(": {note}") }), false)
            }));
        }
        let shown = if p["narrative"]==true && p["carries_actions"]!=true && status=="approved" {if list(&p["narrative_outcomes"]).is_empty(){"narrative_approved"}else{"narrative_resolved"}} else if p["review"]=="acknowledged" {"acknowledged"} else if partial && status=="approved" {"partially_approved"} else {status};
        out.push(Ev { status: shown.to_string(), executions: p["executions"].clone(), actions, ..ev(Kind::Proposal, i18n::t(l, "thread_you").to_string(), when, text(&p["original"])) });
        let note = text(&p["decision"]["note"]);
        if !note.is_empty() {
            let kind = if status == "rejected" { Kind::Reason } else { Kind::Decision };
            out.push(ev(kind, persona(&p["decision"]), format_offset(p["decision"]["t_s"].as_i64().unwrap_or(0)), note));
        }
        if p["narrative"]==true {
            for e in entries.iter().filter(|e|e["proposal"]==p["id"]).skip(1) {
                let (kind,who)=if e["author"]=="excon"{(Kind::Question,persona(e))}else{(Kind::Answer,i18n::t(l,"thread_you").into())};
                out.push(ev(kind,who,text(&e["clock"]).into(),text(&e["text"])));
            }
        } else {
            for e in entries.iter().filter(|e| e["author"] == "excon" && e["proposal"] == p["id"]) {
                out.push(ev(Kind::Question, persona(e), text(&e["clock"]).to_string(), text(&e["text"])));
            }
        }
        if p["status"] != "rolled_back" {
            for outcome in list(&p["narrative_outcomes"]) {
                // What EXCON changed in the world is shown with the narrative.
                let effects = list(&outcome["effects"]).iter().map(|x| text(x).to_string()).collect::<Vec<_>>().join("; ");
                let body = if effects.is_empty() || effects == text(&outcome["text"]) { text(&outcome["text"]).to_string() } else { format!("{} ({})", text(&outcome["text"]), effects) };
                out.push(ev(Kind::Decision, persona(outcome), format_offset(outcome["t_s"].as_i64().unwrap_or(0)), &body));
            }
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
        Kind::Reason => (i18n::tf(l, "inj_rejected_by", &[&e.who]), e.clock.clone(), e.text.clone()),
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

impl crate::player_view::PlayerView {
    /// Inject rows of a turn (most urgent first) and the live proposals they are built from.
    /// Injects the player never received but talks about in a conversation appear as guest rows.
    fn inject_items(&self, l: &str, turn: u64) -> (Vec<Item<'_>>, Vec<&Value>) {
        let own_injects: Vec<&Value> = self.messages().iter().filter(|m| m["kind"] == "inject").collect();
        let proposals: Vec<&Value> = list(&self.state["proposals"]).iter().filter(|p| p["status"] != "rolled_back").collect();
        let all_threads = list(&self.state["clarification_threads"]);
        let all_convs = list(&self.state["linked_conversations"]);
        let linked = |m: &Value, p: &Value| p["reply_to"] == m["id"] || m["inject_id"].is_number() && p["inject_id"] == m["inject_id"];
        let convs_of = |m: &Value| -> Vec<&Value> { all_convs.iter().filter(|c| m["inject_id"].is_number() && c["inject_id"] == m["inject_id"]).collect() };
        let mut guests: Vec<&Value> = Vec::new();
        for c in all_convs {
            if !own_injects.iter().any(|m| m["inject_id"] == c["inject_id"]) && !guests.iter().any(|q| q["inject_id"] == c["inject_id"]) {
                guests.push(&c["quote"]);
            }
        }
        let injects: Vec<&Value> = own_injects.iter().copied().chain(guests.iter().copied()).collect();
        let mut items: Vec<Item> = injects
            .iter()
            .filter(|m| m["turn"] == turn)
            .map(|m| Item {
                key: m["id"].to_string(),
                msg: Some(*m),
                proposals: proposals.iter().filter(|p| linked(m, p)).copied().collect(),
                threads: all_threads.iter().filter(|t| m["inject_id"].is_number() && t["inject_id"] == m["inject_id"]).collect(),
                convs: convs_of(m),
                guest: guests.iter().any(|q| q["id"] == m["id"]),
            })
            .collect();
        // Newest first; the stable sort then lifts open questions, then what else needs the player.
        items.sort_by_key(|it| std::cmp::Reverse(it.msg.map(|m| (m["t_s"].as_i64().unwrap_or(0), m["id"].as_u64().unwrap_or(0)))));
        let free: Vec<&Value> = proposals.iter().filter(|p| p["turn"] == turn && !injects.iter().any(|m| linked(m, p))).copied().collect();
        if !free.is_empty() {
            items.push(Item { key: "free".into(), msg: None, proposals: free, threads: all_threads.iter().filter(|t| t["inject_id"].is_null()).collect(), convs: Vec::new(), guest: false });
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

    /// The list of injects of the selected turn; a row opens the drill-down in
    /// place of the list (thread on the right, back to the list), like the
    /// conversations view.
    pub fn view_inject_panel(&self, can_act: bool, turn: u64) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let (items, _) = self.inject_items(l, turn);
        if items.is_empty() {
            return html! {<div class="tactical-empty"><b>{if self.state["phase"]=="briefing"{i18n::t(l,"awaiting_start")}else{i18n::t(l,"no_events")}}</b><p>{i18n::t(l,"empty_hint")}</p></div>};
        }
        let todo = items.iter().filter(|it| outcome(l, it).action).count();
        // The open detail follows the live data: it is looked up by key on every render.
        let open_key = self.field("inj:open", "");
        if let Some(it) = items.iter().find(|it| it.key == open_key) {
            return self.view_inject_detail(can_act, it);
        }
        let name = |id: &str| self.name(id);
        html! {<div class="inject-md">
            if todo>0{<p class="inj-count" role="status">{i18n::tf(l,"inj_todo_count",&[&todo.to_string()])}</p>}
            <ul class="inject-list">{for items.iter().map(|it|{
                let o=outcome(l,it);
                let pick={let key=it.key.clone();self.cb(move |_|Msg::OpenInjectDetail(key.clone()))};
                let (title,meta)=match it.msg{
                    Some(m)=>(text(&m["title"]).to_string(),format!("{} · {}",text(&m["clock"]),i18n::tf(l,"from",&[&name(text(&m["from"]))]))),
                    None=>(i18n::t(l,"inj_free").to_string(),String::new()),
                };
                let last=events(l,it,&name).last().map(|e|last_line(l,e));
                html!{<li key={it.key.clone()}><button class={classes!("inj-row",o.action.then_some("needs-action"))} data-inject-row={it.key.clone()} onclick={pick}>
                    <span class="inj-main"><b>{title}</b><small>{meta}</small>
                        if it.guest{<small class="inj-via">{i18n::t(l,"conv_via_guest")}</small>}
                        if let Some((who,when,body))=last{<small class="inj-last">{if who.is_empty(){body}else{format!("{}{}: {}",who,if when.is_empty(){String::new()}else{format!(" · {when}")},body)}}</small>}
                    </span>
                    if !it.convs.is_empty(){<span class="pill kind" title={i18n::t(l,"conv_linked")}>{format!("{} {}",i18n::t(l,"conv_short"),it.convs.len())}</span>}
                    <span class={classes!("pill","inj-badge",o.tone)}>{o.label}</span></button></li>}
            })}</ul>
        </div>}
    }

    /// The inject, its thread and its conversations; or one conversation; or the new-conversation form.
    fn view_inject_detail(&self, can_act: bool, it: &Item) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let o = outcome(l, it);
        let back_to_list = self.cb(|_| Msg::OpenInjectDetail(String::new()));
        let conv_open = self.field("inj:conv", "");
        let conv = it.convs.iter().find(|c| c["id"].to_string() == conv_open).copied();
        let creating = self.field("inj:newconv", "") == "true" && !it.guest && it.msg.is_some();
        let head = match it.msg {
            Some(m) => html! {<><h3 id="inj-modal-title">{text(&m["title"])}</h3><small>{format!("{} · {}",text(&m["clock"]),i18n::tf(l,"from",&[&self.name(text(&m["from"]))]))}</small></>},
            None => html! {<h3 id="inj-modal-title">{i18n::t(l,"inj_free")}</h3>},
        };
        let body = if let Some(c) = conv {
            self.view_conversation(can_act, it, c)
        } else if creating {
            self.view_new_conversation(it)
        } else {
            self.view_inject_thread(can_act, it)
        };
        let on_key = Callback::from(|_: KeyboardEvent| {});
        html! {<section class="inj-detail" role="region" aria-labelledby="inj-modal-title" data-inject-detail={it.key.clone()} onkeydown={on_key}>
            <header class="inj-detail-head"><button class="inj-back" data-inject-back="" onclick={back_to_list}>{format!("← {}",i18n::t(l,"back_to_list"))}</button><div class="grow">{head}</div><span class={classes!("pill","inj-badge",o.tone)}>{o.label}</span></header>
            {body}
        </section>}
    }

    fn view_inject_thread(&self, can_act: bool, it: &Item) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let o = outcome(l, it);
        let chat: Vec<Html> = events(l, it, &|id: &str| self.name(id))
            .into_iter()
            .map(|e| {
                let (mine, cls, tag): (bool, String, Option<Html>) = match e.kind {
                    Kind::Inject => (false, "inject".into(), Some(html! {<span class="pill kind">{i18n::t(l,"inject")}</span>})),
                    Kind::Proposal => (true, format!("proposal st-{}", e.status), Some(html! {<span class={classes!("pill",(e.status=="rejected").then_some("bad"))}>{i18n::status(l,&e.status)}</span>})),
                    Kind::Decision => (false, "decision".into(), None),
                    Kind::Reason => (false, "decision rejected".into(), Some(html! {<span class="pill bad">{i18n::t(l,"reason")}</span>})),
                    Kind::Answer => (true, "reply".into(), None),
                    Kind::Question => (false, "question".into(), Some(html! {<span class="pill kind">{i18n::t(l,"inj_clar_label")}</span>})),
                    Kind::Closed => return html! {<li class="inj-sys">{e.text}</li>},
                };
                let extra = if list(&e.executions).is_empty() { html! {} } else { html! {<details><summary>{i18n::t(l,"exec_results")}</summary>{crate::render::render_value("executions",&e.executions)}</details>} };
                html! {<li class={classes!("inj-msg",if mine{"mine"}else{"theirs"},cls)}>
                    <small><b>{e.who}</b>{format!(" · {}",e.clock)}{tag}</small>
                    if !e.text.is_empty(){<p>{e.text}</p>}
                    if !e.actions.is_empty(){<ul class="inj-actions">{for e.actions.iter().map(|(a,ok)|html!{<li class={classes!((!*ok).then_some("not-approved"))}>{a}</li>})}</ul>}
                    {extra}</li>}
            })
            .collect();
        let awaiting = it.threads.iter().find(|t| has_status(t, "awaiting_player")).copied();
        let incoming = it.threads.iter().any(|t| has_status(t, "awaiting_excon") && !list(&t["entries"]).iter().any(|e| e["author"] == "excon"));
        // Conversations linked to this inject: the group work that can lead to a response.
        let convs = if it.convs.is_empty() {
            html! {}
        } else {
            html! {<section class="inj-convs" aria-label={i18n::t(l,"conv_linked")}><h4>{i18n::t(l,"conv_linked")}</h4><ul>{for it.convs.iter().map(|c|{
                let id=c["id"].to_string();
                let last=list(&c["messages"]).last();
                let others=self.conv_names(c);
                let open={let id=id.clone();self.cb(move |_|Msg::Field("inj:conv".into(),id.clone()))};
                html!{<li><button class="conv-row" data-conv-row={id} onclick={open}><b>{others}</b>
                    if let Some(m)=last{<small>{format!("{}: {}",self.name(text(&m["from"])),excerpt(text(if m["from"]==self.own&&m["original"].is_string(){&m["original"]}else{&m["text"]})))}</small>}
                    <span class="pill">{list(&c["messages"]).len()}</span></button></li>}
            })}</ul></section>}
        };
        let composer = if let Some(t) = awaiting {
            // The answer is a plain message to the exercise controllers; a changed action is a new action.
            html! {<footer class="inj-composer">{self.clarify_composer(can_act,t,html!{})}</footer>}
        } else if let Some(m) = it.msg {
            let label = if o.tone == "bad" { "reply_again" } else if it.proposals.is_empty() { "reply" } else { "another_reply" };
            let open = self.cb(Msg::OpenWork);
            let open_via = self.cb(|(parent, conv): (String, String)| Msg::OpenWorkConv(parent, conv));
            let id = m["id"].to_string();
            // A guest answers through a conversation; the most recent one carries the response.
            let via = if it.guest { it.convs.last().map(|c| (c["delivery"].to_string(), c["id"].to_string())) } else { None };
            let start = self.cb(|_| Msg::Field("inj:newconv".into(), "true".into()));
            html! {<footer class="inj-composer row">
                if incoming{<small class="muted" role="status">{i18n::t(l,"thread_incoming")}</small>}
                else if it.threads.iter().any(|t| has_status(t,"awaiting_excon")){<small class="muted" role="status">{i18n::t(l,"thread_wait")}</small>}
                <div class="grow"/>
                if !it.guest {<button data-new-conversation="" onclick={start}>{i18n::t(l,"conv_new")}</button>}
                <button class={classes!("reply-inject",it.proposals.is_empty().then_some("primary"))} disabled={!can_act||(it.guest&&via.is_none())} data-reply-inject={id.clone()} onclick={Callback::from(move |_|match &via{Some((p,c))=>open_via.emit((p.clone(),c.clone())),None=>open.emit(Some(id.clone()))})}>{i18n::t(l,label)}</button></footer>}
        } else {
            html! {}
        };
        html! {<>
            <div class="inj-chat"><ol class="inj-chat-inner" role="log" aria-live="polite">{for chat.into_iter()}</ol></div>
            {convs}
            {composer}
        </>}
    }

    /// Display names of the other participants of a conversation.
    fn conv_names(&self, c: &Value) -> String {
        list(&c["participants"])
            .iter()
            .map(text)
            .filter(|p| *p != self.own)
            .map(|p| self.name(p))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn view_new_conversation(&self, it: &Item) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let people: Vec<String> = self.field("inj:conv_people", "").split(',').filter(|p| !p.is_empty()).map(String::from).collect();
        let message = self.field("inj:conv_text", "");
        let (delivery, own) = (it.msg.map(|m| m["id"].as_u64().unwrap_or(0)).unwrap_or(0), self.own.clone());
        let ready = !people.is_empty() && !message.trim().is_empty() && !self.busy;
        let cancel = self.cb(|_| Msg::Field("inj:newconv".into(), String::new()));
        let send = {
            let (people, message) = (people.clone(), message.clone());
            self.cb(move |_| {
                let mut a = crate::Action::post(format!("/api/player/{own}/conversations"), serde_json::json!({"delivery":delivery,"participants":people,"text":message}));
                a.kind = crate::ActionKind::ConvStart;
                Msg::Action(a)
            })
        };
        html! {<>
            <div class="inj-chat conv-new">
                {self.quoted_inject(l, it.msg)}
                <fieldset class="conv-people"><legend>{i18n::t(l,"conv_pick")}</legend>
                    {for self.contacts().iter().filter(|r|r["id"]!="master").map(|r|{
                        let id=text(&r["id"]).to_string();
                        let on=people.contains(&id);
                        let link=self.link.clone();
                        let (id2,people2)=(id.clone(),people.clone());
                        html!{<label class="mt-check"><input type="checkbox" data-conv-person={id.clone()} checked={on} onchange={Callback::from(move |_|{let mut next:Vec<String>=people2.iter().filter(|p|**p!=id2).cloned().collect();if !on{next.push(id2.clone())};link.emit(Msg::Field("inj:conv_people".into(),next.join(",")))})}/>{text(&r["name"]).to_string()}</label>}
                    })}
                </fieldset>
                <label for="conv-first">{i18n::t(l,"conv_first")}</label>
                <textarea id="conv-first" maxlength="8000" value={message.clone()} oninput={self.cb(|e|Msg::Field("inj:conv_text".into(),crate::api::input(e)))}/>
                <small class="muted">{i18n::t(l,"conv_private_hint")}</small>
            </div>
            <footer class="inj-composer row"><div class="grow"/><button onclick={cancel}>{i18n::t(l,"cancel")}</button><button class="primary" data-create-conversation="" disabled={!ready} onclick={send}>{i18n::t(l,"conv_create")}</button></footer>
        </>}
    }

    /// The inject as a quoted card (conversation context).
    fn quoted_inject(&self, l: &str, m: Option<&Value>) -> Html {
        let Some(m) = m else { return html! {} };
        html! {<blockquote class="conv-quote" data-conv-quote={m["id"].to_string()}><small><b>{i18n::t(l,"inject")}</b>{format!(" · {} · {}",text(&m["clock"]),i18n::tf(l,"from",&[&self.name(text(&m["from"]))]))}</small><b>{text(&m["title"])}</b><p>{text(&m["text"])}</p></blockquote>}
    }

    fn view_conversation(&self, can_act: bool, it: &Item, c: &Value) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let own = self.own.clone();
        let cid = c["id"].as_u64().unwrap_or(0);
        let messages = list(&c["messages"]);
        let reply = self.field("inj:conv_reply", "");
        let reply_id = reply.parse::<u64>().ok();
        let delivery = c["delivery"].as_u64().unwrap_or(0);
        let draft = self.field("inj:conv_text", "");
        let cited = |id: u64| -> Option<(String, String)> {
            if id == delivery {
                Some((i18n::t(l, "inject").to_string(), text(&c["quote"]["title"]).to_string()))
            } else {
                messages.iter().find(|m| m["id"].as_u64() == Some(id)).map(|m| (self.name(text(&m["from"])), excerpt(text(if m["from"] == own && m["original"].is_string() { &m["original"] } else { &m["text"] }))))
            }
        };
        let back = self.cb(|_| Msg::Field("inj:conv".into(), String::new()));
        let send = {
            let (own, draft) = (own.clone(), draft.clone());
            self.cb(move |_| {
                let mut a = crate::Action::post(format!("/api/player/{own}/conversations/{cid}/messages"), serde_json::json!({"text":draft,"reply_to":reply_id}));
                a.kind = crate::ActionKind::ConvSend;
                Msg::Action(a)
            })
        };
        let respond = {
            let (parent, conv) = (delivery.to_string(), cid.to_string());
            self.cb(move |_| Msg::OpenWorkConv(parent.clone(), conv.clone()))
        };
        html! {<>
            <div class="inj-chat conv-thread" data-conversation={cid.to_string()}>
                <div class="conv-head"><button class="inj-back" onclick={back}>{format!("← {}",i18n::t(l,"conv_back"))}</button><span><b>{self.conv_names(c)}</b><small>{i18n::t(l,"conv_private_hint")}</small></span></div>
                <div class="conv-quote-cite" data-cite-target={delivery.to_string()}>{self.quoted_inject(l, Some(&c["quote"]))}
                    <button type="button" class="conv-cite" onclick={self.cb(move |_|Msg::Field("inj:conv_reply".into(),delivery.to_string()))}>{i18n::t(l,"conv_cite")}</button></div>
                <ol class="inj-chat-inner" role="log" aria-live="polite">{for messages.iter().map(|m|{
                    let mine=m["from"]==own;
                    let id=m["id"].as_u64().unwrap_or(0);
                    let quote=m["reply_to"].as_u64().and_then(cited);
                    let body=if mine&&m["original"].is_string(){text(&m["original"])}else{text(&m["text"])};
                    html!{<li class={classes!("inj-msg",if mine{"mine"}else{"theirs"})} data-conv-message={id.to_string()}>
                        <small><b>{if mine{i18n::t(l,"thread_you").to_string()}else{self.name(text(&m["from"]))}}</b>{format!(" · {}",text(&m["clock"]))}</small>
                        if let Some((who,what))=quote{<a class="message-quote-link" data-conv-cites={m["reply_to"].to_string()}><b>{who}</b><span>{what}</span></a>}
                        <p>{body}</p>
                        <button type="button" class="conv-cite" onclick={self.cb(move |_|Msg::Field("inj:conv_reply".into(),id.to_string()))}>{i18n::t(l,"conv_cite")}</button></li>}
                })}
                {for list(&c["responses"]).iter().map(|r|{
                    let by=if r["by"]==own{i18n::t(l,"thread_you").to_string()}else{self.name(text(&r["by"]))};
                    html!{<li class="inj-sys" data-conv-response={r["id"].to_string()}>{i18n::tf(l,"conv_response_line",&[&by,i18n::status(l,text(&r["status"]))])}</li>}
                })}</ol>
            </div>
            <footer class="inj-composer conv-composer">
                if let Some((who,what))=reply_id.and_then(cited){<div class="chat-reply-composer" data-reply-preview={reply.clone()}><div><b>{i18n::tf(l,"replying_to",&[&who])}</b><p>{what}</p></div><button type="button" class="chat-reply-cancel" onclick={self.cb(|_|Msg::Field("inj:conv_reply".into(),String::new()))}>{i18n::t(l,"cancel_reply")}</button></div>}
                <textarea id="conv-text" maxlength="8000" aria-label={i18n::t(l,"message")} placeholder={i18n::t(l,"write")} value={draft.clone()} oninput={self.cb(|e|Msg::Field("inj:conv_text".into(),crate::api::input(e)))}/>
                <div class="row"><small class="literal-translation-hint">{i18n::t(l,"literal_translation_hint")}</small><div class="grow"/>
                    <button data-respond-from-conversation="" disabled={!can_act||it.msg.is_none()} onclick={respond}>{i18n::t(l,"conv_respond")}</button>
                    <button class="primary" data-send-conversation="" disabled={self.busy||draft.trim().is_empty()} onclick={send}>{i18n::t(l,"send")}</button></div>
            </footer>
        </>}
    }
}

fn excerpt(message: &str) -> String {
    let mut chars = message.chars();
    let head = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn item<'a>(proposals: Vec<&'a Value>, threads: Vec<&'a Value>) -> Item<'a> {
        Item { key: "1".into(), msg: None, proposals, threads, convs: vec![], guest: false }
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
    fn verdicts_are_explicit_and_per_action() {
        let partial = json!({"id":4,"status":"approved","review":"partially_approved","original":"warn F4, G4","t_s":0,
            "commands":[{"type":"IssueWarning","zone":"F4"}],
            "decision":{"review":"partially_approved","t_s":0,"rejected_actions":[{"index":1,"command":{"type":"IssueWarning","zone":"G4"},"note":"not threatened"}]}});
        let ack = json!({"id":5,"status":"acknowledged","review":"acknowledged","original":"how many buses?","t_s":0});
        let o = outcome("it", &item(vec![&partial], vec![]));
        assert_eq!((o.label, o.tone, o.action), ("Approvata in parte", "partial", false));
        assert_eq!(outcome("en", &item(vec![&ack], vec![])).label, "Acknowledged · nothing to execute");
        let name = |id: &str| id.to_string();
        let ev = events("it", &item(vec![&partial], vec![]), &name);
        assert_eq!(ev[0].status, "partially_approved");
        assert_eq!(ev[0].actions.len(), 2);
        assert!(ev[0].actions[0].1 && ev[0].actions[0].0.contains("Approvata"));
        assert!(!ev[0].actions[1].1 && ev[0].actions[1].0.contains("not threatened"), "{:?}", ev[0].actions);
        assert_eq!(i18n::status("fr", "partially_approved"), "Approuvée en partie");
    }
    #[test]
    fn last_line_names_the_last_event() {
        let name = |id: &str| id.to_string();
        let inject = json!({"from":"dispatch","clock":"Day 1 13:00","text":"Fire"});
        let p1 = json!({"id":1,"status":"superseded","original":"F4","t_s":0});
        let p2 = json!({"id":2,"status":"pending","original":"2 bus","t_s":0});
        let rej = json!({"id":3,"status":"rejected","original":"D6","t_s":0,"decision":{"note":"too few buses","t_s":0,"persona":"Prefettura"}});
        let th = json!({"id":1,"status":"awaiting_player","entries":[{"author":"excon","persona":"Sala operativa","proposal":1,"clock":"13:05","text":"How many buses?"}]});
        let line = |it: &Item| last_line("it", events("it", it, &name).last().unwrap());
        // Only the inject: its own text (the row shows sender and time).
        let mut it = Item { key: "1".into(), msg: Some(&inject), proposals: vec![], threads: vec![], convs: vec![], guest: false };
        assert_eq!(line(&it), (String::new(), String::new(), "Fire".into()));
        // EXCON's question follows the proposal it is attached to, the answer comes after it.
        it.proposals = vec![&p1];
        it.threads = vec![&th];
        assert_eq!(line(&it), ("Sala operativa".into(), "13:05".into(), "How many buses?".into()));
        it.proposals = vec![&p1, &p2];
        let (who, _, body) = line(&it);
        assert_eq!((who.as_str(), body.contains("2 bus")), ("Tu", true));
        // A rejection reports EXCON's reason.
        it.proposals = vec![&rej];
        it.threads = vec![];
        let (who, _, body) = line(&it);
        assert_eq!((who.as_str(), body.as_str()), ("Prefettura ha rifiutato", "too few buses"));
        assert_eq!(events("it", &it, &name).len(), 3);
        // A closed thread ends the conversation.
        let closed = json!({"id":1,"status":"closed","entries":[]});
        it.threads = vec![&closed];
        assert_eq!(line(&it).2, "Chiarimento chiuso");
    }
}
