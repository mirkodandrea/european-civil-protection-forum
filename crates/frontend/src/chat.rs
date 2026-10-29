use crate::{
    api::{self, list, text},
    i18n,
    widgets::Proposal,
    Action, ActionKind, App, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;

impl App {
    pub fn latest_inject(&self) -> Option<&Value> {
        self.messages()
            .iter()
            .filter(|m| m["kind"] == "inject" && m["to"] == self.own)
            .max_by_key(|m| m["id"].as_u64().unwrap_or(0))
    }
    fn chat_reference(&self, draft: &crate::Draft) -> Option<u64> {
        draft
            .reply_to
            .filter(|id| {
                self.messages()
                    .iter()
                    .any(|m| m["id"] == *id && self.in_thread(m, &self.selected))
            })
            .or_else(|| {
                draft.inject_ref.filter(|id| {
                    self.messages()
                        .iter()
                        .any(|m| m["id"] == *id && m["kind"] == "inject" && m["to"] == self.own)
                })
            })
            .or_else(|| self.latest_inject().and_then(|m| m["id"].as_u64()))
    }

    pub fn view_chat(&self, ctx: &Context<Self>) -> Html {
        self.view_chat_content(ctx, false)
    }
    pub fn view_inject_form(&self, ctx: &Context<Self>) -> Html {
        self.view_chat_content(ctx, true)
    }
    fn view_chat_content(&self, ctx: &Context<Self>, composer_only: bool) -> Html {
        let draft = self.draft();
        let master = self.is_master();
        let l = self.lang();
        let l = l.as_str();
        let field = |key: &str| {
            let key = key.to_string();
            ctx.link()
                .callback(move |e| Msg::Field(key.clone(), api::input(e)))
        };
        let select = |key: &str| {
            let key = key.to_string();
            ctx.link()
                .callback(move |e| Msg::Field(key.clone(), api::select(e)))
        };
        let selected = self.selected.clone();
        let own = self.own.clone();
        let props = if master {
            list(&self.master["proposals"])
        } else {
            list(&self.state["proposals"])
        };
        let items = self
            .messages()
            .iter()
            .filter(|m| self.in_thread(m, &selected))
            .collect::<Vec<_>>();
        let reply_quote = if !master {
            draft.reply_to.and_then(|id| {
                items
                    .iter()
                    .find(|m| {
                        m["id"].as_u64() == Some(id)
                            && m["kind"] == "chat"
                            && m["from"] != "master"
                            && m["to"] != "master"
                    })
                    .copied()
            })
        } else {
            None
        };
        let visible = props
            .iter()
            .filter(|p| {
                p["source"]["role"] == selected
                    && ["parsing", "pending", "needs_clarification", "approved"]
                        .contains(&text(&p["status"]))
            })
            .collect::<Vec<_>>();
        let reference = self.chat_reference(&draft);
        let mut received_injects = self
            .messages()
            .iter()
            .filter(|m| m["kind"] == "inject" && m["to"] == own)
            .collect::<Vec<_>>();
        received_injects.sort_by_key(|m| std::cmp::Reverse(m["id"].as_u64().unwrap_or(0)));
        let send_disabled =
            (self.busy && !master) || self.state["phase"] == "debrief" || (!master && reference.is_none());
        let contact = self
            .contacts()
            .into_iter()
            .find(|c| c["id"] == selected)
            .unwrap_or(Value::Null);
        let submit = {
            let link = ctx.link().clone();
            let action = self.chat_action();
            Callback::from(move |e: SubmitEvent| {
                e.prevent_default();
                // Buttons of the action forms inside the composer must not send the inject.
                if e.submitter().is_some_and(|b| b.id() != "send") {
                    return;
                }
                if let Some(a) = action.clone() {
                    link.send_message(Msg::Action(a));
                }
            })
        };
        let keydown = {
            let link = ctx.link().clone();
            let action = self.chat_action();
            Callback::from(move |e: KeyboardEvent| {
                if e.key() == "Enter" && !e.shift_key() && !e.is_composing() {
                    e.prevent_default();
                    if let Some(a) = action.clone() {
                        link.send_message(Msg::Action(a));
                    }
                }
            })
        };
        let composer = html! {                    if master || selected!="injects" {<form class="composer" id="composer" onsubmit={submit}>
            if master{<div id="conversation-injects"><div class="row conversation-message-kind"><b>{crate::i18n::ex("Inject dell’esercitazione")}</b></div>
            <div id="inject-options" hidden={draft.mode!="inject"}>
                <div class="inject-compose-row"><label for="inject-template">{crate::i18n::ex("Da scenario")}</label><select key={format!("template:{selected}:{}",draft.template)} id="inject-template" value={draft.template.clone()} onchange={select("draft:template")}><option value="" selected={draft.template.is_empty()}>{crate::i18n::ex("Inject personalizzato")}</option>{for list(&self.stat["msel"]).iter().filter(|t|selected=="all"||list(&t["recipients"]).contains(&json!("all"))||list(&t["recipients"]).contains(&json!(selected))).map(|t|html!{<option value={text(&t["id"]).to_string()} selected={t["id"]==draft.template}>{crate::exfmt!("{} · {} · turno {}","{} · {} · turn {}",text(&t["id"]),text(&t["title"]),t["turn"])}</option>})}</select></div>
                <div class="inject-compose-grid"><label>{crate::i18n::ex("Titolo")}<input id="inject-title" value={draft.title.clone()} oninput={field("draft:title")} required={draft.mode=="inject"}/></label><label>{crate::i18n::ex("Mittente simulato")}<input id="inject-sender" list="inject-senders" maxlength="80" placeholder={crate::exfmt!("Predefinito: {}","Default: {}",text(&self.stat["personas"]["default"]))} value={if draft.sender=="EXCON"{String::new()}else{draft.sender.clone()}} oninput={field("draft:sender")}/><datalist id="inject-senders">{for list(&self.stat["personas"]["senders"]).iter().map(|s|text(s).to_string()).chain(list(&self.stat["personas"]["roles"]).iter().map(|r|text(&r["name"]).to_string())).map(|n|html!{<option value={n}/>})}</datalist></label></div>
                <div class="inject-compose-row"><label>{crate::i18n::ex("Destinatari")}</label><select key={format!("scope:{selected}:{}",draft.scope)} id="inject-scope" value={draft.scope.clone()} onchange={select("draft:scope")}><option value="conversation" selected={draft.scope=="conversation"}>{if selected=="all"{crate::i18n::ex("Tutti i player").to_string()}else{crate::exfmt!("Solo {}","Only {}",self.name(&selected))}}</option><option value="custom" selected={draft.scope=="custom"}>{crate::i18n::ex("Scegli i destinatari…")}</option>if !draft.template.is_empty(){<option value="scenario" selected={draft.scope=="scenario"}>{crate::i18n::ex("Destinatari dello scenario")}</option>}</select></div>
                if draft.scope=="custom"{<fieldset class="inject-compose-row ctl-multi">{for list(&self.stat["roles"]).iter().map(|r|{let id=text(&r["id"]).to_string();let chosen:Vec<String>=draft.recipients.split(',').filter(|k|!k.is_empty()).map(String::from).collect();let on=chosen.contains(&id);let link=ctx.link().clone();let id2=id.clone();html!{<label class="mt-check"><input type="checkbox" checked={on} onchange={Callback::from(move|_|{let mut next:Vec<String>=chosen.iter().filter(|c|**c!=id2).cloned().collect();if !on{next.push(id2.clone())};link.send_message(Msg::Field("draft:recipients".into(),next.join(",")))})}/>{text(&r["name"]).to_string()}</label>}})}</fieldset>}
                <p class="muted">{crate::i18n::ex("Il player risponde dalla vista di lavoro; la proposta appare nella timeline per l’approvazione. L’invio dell’inject non cambia la fisica.")}</p>
                if draft.template.is_empty()||draft.scope!="scenario"{<div class="composer-actions" onkeydown={Callback::from(|e:KeyboardEvent|{
                    // Enter in a field of the action form must not send the inject.
                    if e.key()=="Enter"&&e.target_dyn_into::<web_sys::HtmlTextAreaElement>().is_none(){e.prevent_default();}
                })}>
                    <crate::action_form::ActionList actions={draft.actions.clone()} onchange={ctx.link().callback(|v:Vec<scenario::InjectAction>|Msg::Field("draft:actions".into(),serde_json::to_string(&v).unwrap_or_default()))}
                        allow_pre=false prefix="C" common={crate::controllo::CommonProps{stat:self.stat.clone(),state:self.state.clone(),master:self.master.clone(),responses:self.responses.clone(),busy:self.busy,error:self.error.clone().into(),done:self.done,action:self.action(ctx),onclose:Callback::noop()}}
                        weather={{let w=&self.state["world"]["weather"];(w["wind_dir_deg"].as_f64().unwrap_or(0.),w["wind_speed_kmh"].as_f64().unwrap_or(10.),w["moisture_pct"].as_f64().unwrap_or(10.))}}/>
                </div>}
            </div></div>}
            if self.busy{<div class="activity" role="status" aria-live="polite"><span class="loader" aria-hidden="true"></span><span>{&self.notice}</span></div>}
                if let Some(quoted)=reply_quote {<div class="chat-reply-composer" data-reply-preview={quoted["id"].to_string()}><div><b>{i18n::tf(l,"replying_to",&[&self.name(text(&quoted["from"]))])}</b><p>{chat_excerpt(reply_text(quoted,text(&quoted["from"])==own.as_str()))}</p></div><button type="button" class="chat-reply-cancel" aria-label={i18n::t(l,"cancel_reply")} onclick={ctx.link().callback(|_|Msg::CancelChatReply)}>{i18n::t(l,"cancel_reply")}</button></div>}
                if !master && reply_quote.is_none() {<div class="chat-inject-reference"><label for={format!("chat-inject-reference-{}",own)}>{crate::i18n::pair(l, "In riferimento all’inject", "In reference to inject")}</label><select id={format!("chat-inject-reference-{}",own)} data-chat-inject-reference="true" disabled={received_injects.is_empty() || self.busy} value={reference.map(|id|id.to_string()).unwrap_or_default()} onchange={ctx.link().batch_callback(|e|api::select(e).parse::<u64>().ok().map(Msg::ChatInject))}>
                    if received_injects.is_empty(){<option value="">{crate::i18n::pair(l, "Nessun inject ricevuto", "No received inject")}</option>}
                    {for received_injects.iter().map(|inject| html!{<option value={inject["id"].to_string()} selected={inject["id"].as_u64()==reference}>{format!("{} · {}",text(&inject["clock"]),text(&inject["title"]))}</option>})}
                </select></div>}
                if !master && reference.is_none() {<p class="muted" role="status">{crate::i18n::pair(l, "Per inviare, rispondi a un messaggio oppure attendi un inject.", "To send, reply to a message or wait for an inject.")}</p>}
                <textarea id="text" maxlength="8000" aria-label={i18n::t(l,"message")} placeholder={i18n::t(l,"write")} required=true value={draft.text.clone()} oninput={field("draft:text")} onkeydown={keydown}/>
                if !master {<small class="literal-translation-hint">{i18n::t(l,"literal_translation_hint")}</small>}
            <div class="row">if master{<small>{crate::i18n::ex("Testo EXCON in inglese · tradotto per il destinatario")}</small>}<div class="grow"/><span id="send-status" class="muted" role="status">{if self.busy{self.notice.as_str()}else{""}}</span>if master && !self.pending.is_empty(){<span class="activity"><span class="loader" aria-hidden="true"></span>{crate::i18n::ex("Invio in corso…")}</span>}<button id="send" class="primary" disabled={send_disabled}>{if self.busy{i18n::t(l,"sending")}else if master{crate::i18n::ex("Invia inject →")}else{i18n::t(l,"send")}}</button></div>
        </form>}else{<div class="composer"><p class="muted">{i18n::t(l,"inject_info")}</p></div>}};
        if composer_only {
            return composer;
        }
        html! {<>
            <main id="chat-view" class="workspace" hidden={self.active!="chat"}>
                <aside class="contacts"><h3>{i18n::t(l,"conversations")}</h3><div id="contact-list" style="display:contents">{for self.contacts().iter().map(|r|{
                    let id=text(&r["id"]).to_string();let seen=self.seen.get(&id).copied().unwrap_or(0);let unread=self.messages().iter().filter(|m|self.in_thread(m,&id)&&m["from"]!=own&&m["id"].as_u64().unwrap_or(0)>seen).count();let role=id.clone();
                    html!{<button class={classes!("contact",(selected==id).then_some("active"))} data-contact={id.clone()} onclick={ctx.link().callback(move |_|Msg::Select(role.clone()))}><span>{text(&r["name"])}<small>{if r["id"]=="master"{self.name("master")}else{i18n::t(l,"player_direct").to_string()}}</small></span>if unread>0{<span class="unread">{unread}</span>}</button>}
                })}</div></aside>
                <section class="conversation"><div class="thread-head"><div><h2 id="thread-name">{text(&contact["name"])}</h2><small id="thread-description">{text(&contact["description"])}</small>{if self.peer_typing{html!{<div class="activity peer-activity" role="status"><span class="loader" aria-hidden="true"></span><span>{i18n::tf(l,"typing",&[text(&contact["name"])])}</span></div>}}else{Html::default()}}</div><div class="grow"/><span class="pill" title={if master{""}else{i18n::t(l,"literal_translation_hint")}}>{if master{crate::i18n::ex("Supervisione EXCON")}else{i18n::t(l,"literal_translation_short")}}</span></div>
                <MessageList thread={selected.clone()}>
                if items.is_empty()&&visible.is_empty(){<div class="empty"><h3>{i18n::t(l,"conv_starts")}</h3><p>{i18n::t(l,"conv_hint")}</p></div>}
                {for items.iter().enumerate().map(|(idx,m)|{
                    let proposal=props.iter().find(|p|p["id"]==m["proposal"]).filter(|p|!master||items.iter().find(|other|other["proposal"]==p["id"]).is_some_and(|first|first["id"]==m["id"]));let from=text(&m["from"]);let body=if from==own{if m["original"].is_string(){&m["original"]}else{&m["text"]}}else if master{&m["text_en"]}else{&m["text"]};
                    let is_chat = m["kind"] == "chat";
                    let is_peer_chat = is_chat && m["from"] != "master" && m["to"] != "master";
                    let reply_target = m["reply_to"].as_u64().and_then(|id| items.iter().find(|other|other["id"].as_u64()==Some(id) && other["kind"]=="chat" && other["from"]!="master" && other["to"]!="master").copied());
                    let is_new_turn = idx == 0 || items[idx-1]["turn"] != m["turn"];
                    html!{<div class="chat-turn-group" key={m["id"].to_string()}>
                    if is_new_turn {<div class="chat-turn-separator" data-turn={m["turn"].to_string()}><span>{i18n::tf(l,"turn_label_short",&[&m["turn"].to_string()])}</span></div>}
                    <article id={format!("chat-message-{}-{}",own,m["id"])} data-chat-message={m["id"].to_string()} key={m["id"].to_string()} class={classes!("message",(from==own).then_some("own"),(m["kind"]=="inject").then_some("inject"))}>
                        <div class="meta">{i18n::tf(l,"meta",&[&self.name(from),text(&m["clock"]),&m["turn"].to_string(),if m["kind"]=="inject"{i18n::t(l,"private_inject")}else{""}])}</div>
                        if let Some(reply)=reply_target {<a class="message-quote-link" data-reply-target={reply["id"].to_string()} href={format!("#chat-message-{}-{}",own,reply["id"])}><b>{self.name(text(&reply["from"]))}</b><span>{chat_excerpt(reply_text(reply,text(&reply["from"])==own.as_str()))}</span></a>}
                        if !master && reply_target.is_none() && m["inject_id"].is_number() {
                            <small class="chat-inject-context">{if let Some(inject)=self.messages().iter().find(|inject| inject["kind"]=="inject" && inject["to"]==own && inject["inject_id"]==m["inject_id"]){format!("{} · {}",i18n::t(l,"inject"),text(&inject["title"]))}else{crate::i18n::pair(l, "Thread collegato a un inject", "Thread linked to an inject").to_string()}}</small>
                        }
                        if !text(&m["title"]).is_empty(){<b>{text(&m["title"])}</b>}<div class="text">{text(body)}</div>
                        if m["original"]!=*body&&!m["original"].is_null(){<details><summary>{i18n::t(l,"original")}</summary>{text(&m["original"])}</details>}
                        if m["translation_failed"]==true{<p class="danger">{i18n::t(l,"translation_failed")}</p>}
                        if let Some(p)=proposal{if master {<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)} roles={self.roles()}/>}else{<div class="status">{i18n::status(l,text(&p["status"]))}</div>}}
                        if !master {{for props.iter().filter(|p|p["reply_to"]==m["id"]).map(|p|html!{<p class="status" data-chat-proposal={p["id"].to_string()}>{format!("{} · {}",crate::i18n::pair(l, "Azione proposta", "Proposed action"),i18n::status(l,text(&p["status"])))}</p>})}}
                        if !master && is_peer_chat {<div class="message-response-actions"><button type="button" data-propose-message={m["id"].to_string()} disabled={self.busy||self.state["phase"]!="orders"} onclick={ctx.link().callback({let id=m["id"].to_string();move |_|Msg::OpenWork(Some(id.clone()))})}>{crate::i18n::pair(l, "Proponi azione", "Propose action")}</button><button type="button" class="message-reply-button" data-reply-message={m["id"].to_string()} disabled={self.busy||self.state["phase"]=="debrief"} onclick={ctx.link().callback({let id=m["id"].as_u64().unwrap_or(0);move |_|Msg::ReplyChat(id)})}>{i18n::t(l,"reply_message")}</button></div>}
                    </article></div>}
                })}
                if master {{for visible.iter().filter(|p|!items.iter().any(|m|m["proposal"]==p["id"])).map(|p|html!{<article class="message" key={format!("orphan:{}",p["id"])}><Proposal proposal={(*p).clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)} roles={self.roles()}/></article>})}}
                </MessageList>
                    {composer}
                </section>
            </main>
            if master {<div id="map-view" hidden={self.active!="map"}><crate::controllo::Controllo stat={self.stat.clone()} state={self.state.clone()} master={self.master.clone()} links={self.links.clone()} catalogue={self.catalogue.clone()} responses={self.responses.clone()} busy={self.busy} error={self.error.clone()} done={self.done} action={self.action(ctx)}/></div>}else{{self.player_view(ctx).view_tactical()}{self.view_function()}}
        </>}
    }
    fn chat_action(&self) -> Option<Action> {
        // Master injects do not wait for other actions: see `ActionKind::Inject`.
        if (self.busy && !self.is_master()) || self.state["phase"] == "debrief" {
            return None;
        }
        let draft = self.draft();
        if self.is_master() && draft.scope == "custom" && draft.recipients.is_empty() {
            return None;
        }
        let to = self.selected.clone();
        let message = draft.text.trim();
        let reference = self.chat_reference(&draft);
        if message.is_empty() || (!self.is_master() && reference.is_none()) {
            return None;
        }
        let body = json!({"to":to,"text":message,"interpret":false,"reply_to":if self.is_master(){None}else{reference}});
        let mut action = Action::post(
            if self.is_master() {
                "/api/master/chat".into()
            } else {
                format!("/api/player/{}/chat", self.own)
            },
            body,
        );
        action.kind = ActionKind::Send(to.clone(), draft.text.clone());
        // Master injects run in their own slot: a new one can be sent while another is in flight.
        let mut slot = format!("compose:{}", js_sys::Date::now());
        if self.is_master() {
            let template = list(&self.stat["msel"])
                .iter()
                .find(|t| t["id"] == draft.template);
            let scenario = draft.scope == "scenario" && template.is_some();
            if let Some(t) = template.filter(|t| {
                scenario
                    && !list(&self.state["sent_injects"]).contains(&t["id"])
                    && t["text"] == message
                    && t["title"] == draft.title.trim()
                    && t["sender"] == draft.sender.trim()
            }) {
                action.path = format!("/api/master/msel/{}/send", api::encode(text(&t["id"])));
                action.body = Some(json!({}));
                slot = crate::timeline::send_slot(text(&t["id"]));
            } else {
                action.path = "/api/master/inject".into();
                action.body = Some(
                    json!({"title":draft.title.trim(),"sender":draft.sender.trim(),"text":message,"actions":draft.actions,"recipients":if scenario{template.unwrap()["recipients"].clone()}else if draft.scope=="custom"{json!(draft.recipients.split(',').filter(|k|!k.is_empty()).collect::<Vec<_>>())}else{json!([to])}}),
                );
            }
            return Some(action.in_slot(slot));
        }
        Some(action)
    }
}

fn reply_text(message: &Value, sent_by_self: bool) -> &str {
    if sent_by_self && !text(&message["original"]).is_empty() {
        text(&message["original"])
    } else {
        text(&message["text"])
    }
}

fn chat_excerpt(message: &str) -> String {
    let mut chars = message.chars();
    let excerpt = chars.by_ref().take(160).collect::<String>();
    if chars.next().is_some() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

#[derive(Properties, PartialEq)]
struct MessageListProps {
    thread: String,
    children: Children,
}
#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
    let node = use_node_ref();
    let following = use_mut_ref(|| true);
    let previous_thread = use_mut_ref(String::new);
    {
        let node = node.clone();
        let following = following.clone();
        let previous_thread = previous_thread.clone();
        use_effect_with(
            (props.thread.clone(), props.children.clone()),
            move |(thread, _)| {
                if let Some(element) = node.cast::<web_sys::HtmlElement>() {
                    if *previous_thread.borrow() != *thread || *following.borrow() {
                        element.set_scroll_top(element.scroll_height());
                        *following.borrow_mut() = true;
                    }
                    *previous_thread.borrow_mut() = thread.clone();
                }
            },
        );
    }
    let onscroll = Callback::from(move |event: Event| {
        let element = event.target_unchecked_into::<web_sys::HtmlElement>();
        *following.borrow_mut() =
            element.scroll_height() - element.scroll_top() - element.client_height() < 90;
    });
    html! { <div class="messages" id="messages" aria-live="polite" ref={node} {onscroll}>{props.children.clone()}</div> }
}
