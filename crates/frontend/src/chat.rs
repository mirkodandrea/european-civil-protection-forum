use crate::{
    api::{self, list, text},
    map::SituationMap,
    widgets::{self, Proposal},
    Action, ActionKind, App, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;

impl App {
    pub fn view_chat(&self, ctx: &Context<Self>) -> Html {
        let draft = self.draft();
        let master = self.is_master();
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
        let visible = props
            .iter()
            .filter(|p| {
                p["source"]["role"] == selected
                    && ["parsing", "pending", "needs_clarification", "approved"]
                        .contains(&text(&p["status"]))
            })
            .collect::<Vec<_>>();
        let preparation =
            !master && self.field("interpret", "true") == "true" && self.state["phase"] == "orders";
        let private = &draft.private;
        let ready = private["status"] == "ready";
        let clarification = private["status"] == "needs_clarification";
        let manual_fallback = private["status"] == "ai_unavailable";
        let has_actions = !list(&private["actions"]).is_empty();
        let send_disabled = self.busy
            || self.state["phase"] == "debrief"
            || (!master && self.state["player"]["language_confirmed"] != true)
            || (preparation && ready && !draft.dirty);
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
        let map_view = if master {
            self.state["world"].clone()
        } else {
            let mut v = self.state["map"].clone();
            v["deployments"] = self.state["deployments"].clone();
            v
        };
        html! {<>
            <main id="chat-view" class="workspace" hidden={self.active!="chat"}>
                <aside class="contacts"><h3>{"Conversazioni"}</h3><div id="contact-list" style="display:contents">{for self.contacts().iter().map(|r|{
                    let id=text(&r["id"]).to_string();let seen=self.seen.get(&id).copied().unwrap_or(0);let unread=self.messages().iter().filter(|m|self.in_thread(m,&id)&&m["from"]!=own&&m["id"].as_u64().unwrap_or(0)>seen).count();let role=id.clone();
                    html!{<button class={classes!("contact",(selected==id).then_some("active"))} data-contact={id.clone()} onclick={ctx.link().callback(move |_|Msg::Select(role.clone()))}><span>{text(&r["name"])}<small>{if r["id"]=="master"{"Direzione esercitazione"}else{"Player · conversazione diretta"}}</small></span>if unread>0{<span class="unread">{unread}</span>}</button>}
                })}</div></aside>
                <section class="conversation"><div class="thread-head"><div><h2 id="thread-name">{text(&contact["name"])}</h2><small id="thread-description">{text(&contact["description"])}</small>{if self.peer_typing{html!{<div class="activity peer-activity" role="status"><span class="loader" aria-hidden="true"></span><span>{format!("{} sta scrivendo…",text(&contact["name"]))}</span></div>}}else{Html::default()}}</div><div class="grow"/><span class="pill">{if master{"Supervisione EXCON"}else{"Traduzione AI"}}</span></div>
                <MessageList thread={selected.clone()}>
                if items.is_empty()&&visible.is_empty(){<div class="empty"><h3>{"La conversazione inizia qui"}</h3><p>{"Invia informazioni, richieste o decisioni."}</p></div>}
                {for items.iter().map(|m|{
                    let proposal=props.iter().find(|p|p["id"]==m["proposal"]).filter(|p|!master||items.iter().find(|other|other["proposal"]==p["id"]).is_some_and(|first|first["id"]==m["id"]));let from=text(&m["from"]);let body=if from==own{if m["original"].is_string(){&m["original"]}else{&m["text"]}}else if master{&m["text_en"]}else{&m["text"]};
                    html!{<article key={m["id"].to_string()} class={classes!("message",(from==own).then_some("own"),(master&&m["kind"]=="inject").then_some("inject"))}>
                        <div class="meta">{format!("{} · {} · turno {}{}",self.name(from),text(&m["clock"]),m["turn"],if master&&m["kind"]=="inject"{" · INJECT"}else{""})}</div>
                        if !text(&m["title"]).is_empty(){<b>{text(&m["title"])}</b>}<div class="text">{text(body)}</div>
                        if m["original"]!=*body&&!m["original"].is_null(){<details><summary>{"Testo originale"}</summary>{text(&m["original"])}</details>}
                        if m["translation_failed"]==true{<p class="danger">{"Traduzione non disponibile · testo di origine o inglese"}</p>}
                        if let Some(p)=proposal{if master {<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)}/>}else{<div class="status">{api::status(text(&p["status"]))}</div>}}
                    </article>}
                })}
                if master {{for visible.iter().filter(|p|!items.iter().any(|m|m["proposal"]==p["id"])).map(|p|html!{<article class="message" key={format!("orphan:{}",p["id"])}><Proposal proposal={(*p).clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)}/></article>})}}
                </MessageList>
                <form class="composer" id="composer" onsubmit={submit}>
                    if master{<div id="conversation-injects"><div class="row conversation-message-kind"><label for="message-kind">{"Scrivi come"}</label><select key={format!("kind:{selected}:{}",draft.mode)} id="message-kind" value={draft.mode.clone()} onchange={select("draft:mode")}><option value="chat" selected={draft.mode=="chat"}>{"EXCON · messaggio"}</option><option value="inject" selected={draft.mode=="inject"}>{"Inject · evento dell’esercitazione"}</option></select></div>
                    <div id="inject-options" hidden={draft.mode!="inject"}>
                        <div class="inject-compose-row"><label for="inject-template">{"Da scenario"}</label><select key={format!("template:{selected}:{}",draft.template)} id="inject-template" value={draft.template.clone()} onchange={select("draft:template")}><option value="" selected={draft.template.is_empty()}>{"Inject personalizzato"}</option>{for list(&self.stat["msel"]).iter().filter(|t|list(&t["recipients"]).contains(&json!("all"))||list(&t["recipients"]).contains(&json!(selected))).map(|t|html!{<option value={text(&t["id"]).to_string()} selected={t["id"]==draft.template}>{format!("{} · {} · turno {}",text(&t["id"]),text(&t["title"]),t["turn"])}</option>})}</select></div>
                        <div class="inject-compose-grid"><label>{"Titolo"}<input id="inject-title" value={draft.title.clone()} oninput={field("draft:title")} required={draft.mode=="inject"}/></label><label>{"Mittente simulato"}<input id="inject-sender" value={draft.sender.clone()} oninput={field("draft:sender")} required={draft.mode=="inject"}/></label></div>
                        <div class="inject-compose-row"><label>{"Destinatari"}</label><select key={format!("scope:{selected}:{}",draft.scope)} id="inject-scope" value={draft.scope.clone()} onchange={select("draft:scope")}><option value="conversation" selected={draft.scope=="conversation"}>{format!("Solo {} · questa conversazione",self.name(&selected))}</option>if !draft.template.is_empty(){<option value="scenario" selected={draft.scope=="scenario"}>{"Destinatari dello scenario"}</option>}</select></div>
                        <p class="muted">{"Il player risponde nella stessa conversazione. L’invio non cambia la fisica."}</p>
                    </div></div>}
                    if preparation&&!private.is_null()&&(clarification||manual_fallback||has_actions){<div id="private-preparation"><div class="private-draft-head"><b>{if clarification{"Chiarimento richiesto"}else if manual_fallback{"Interpretazione non riuscita"}else{"Azioni individuate · verifica e conferma"}}</b><span class="pill">{"Bozza privata · non inviata"}</span></div>
                        if clarification{<p class="draft-question">{text(&private["question"])}</p><p class="muted">{"Rispondi nel campo qui sotto. EXCON e il destinatario non hanno ancora ricevuto il messaggio."}</p>}
                        else if manual_fallback{<p>{"La bozza non è stata interpretata. Puoi riprovare oppure inviarla a EXCON per l’interpretazione manuale."}</p>}
                        else{<p class="draft-message">{text(&private["message"])}</p>{for list(&private["action_descriptions"]).iter().map(|s|html!{<p class="draft-action">{text(s)}</p>})}}
                        if draft.dirty{<p class="muted">{"Hai modificato il testo: analizzalo di nuovo prima di confermare."}</p>}
                        <div class="row">if !clarification{<button id="confirm-prepared" type="button" class="primary" disabled={self.busy||draft.dirty} onclick={{let mut a=Action::post(format!("/api/player/{own}/draft/submit"),json!({"draft":private["id"],"revision":private["revision"],"manual":manual_fallback}));a.kind=ActionKind::Submit(selected.clone());ctx.link().callback(move |_|Msg::Action(a.clone()))}}>{if manual_fallback{"Invia a EXCON per interpretazione manuale"}else{"Conferma azioni e invia"}}</button>}<button type="button" id="discard-prepared" disabled={self.busy} onclick={ctx.link().callback(|_|Msg::Discard)}>{"Scarta bozza"}</button></div>
                    </div>}
                    if preparation&&self.busy{<div class="activity" role="status" aria-live="polite"><span class="loader" aria-hidden="true"></span><span>{&self.notice}</span></div>}
                    if self.busy&&!preparation{<div class="activity" role="status" aria-live="polite"><span class="loader" aria-hidden="true"></span><span>{&self.notice}</span></div>}
                    <textarea id="text" maxlength="8000" aria-label="Messaggio" placeholder={if clarification{"Rispondi alla domanda dell’assistente…"}else{"Scrivi un messaggio…"}} required=true value={draft.text.clone()} oninput={field("draft:text")} onkeydown={keydown}/>
                    <div class="row">if !master{<label><input type="checkbox" id="interpret" checked={self.field("interpret","true")=="true"} disabled={self.state["phase"]!="orders"||self.busy} onchange={ctx.link().callback(|e:Event|{use yew::TargetCast;Msg::Field("interpret".into(),e.target_unchecked_into::<web_sys::HtmlInputElement>().checked().to_string())})}/>{" Distingui automaticamente chat e azioni con l’AI"}</label>}else{<small>{"Testo EXCON in inglese · tradotto per il destinatario"}</small>}<div class="grow"/><span id="send-status" class="muted" role="status">{&self.notice}</span><button id="send" class="primary" disabled={send_disabled}>{if self.busy{"Elaborazione…"}else if preparation {if clarification{"Rispondi →"}else if ready&&has_actions{"Rianalizza modifiche"}else{"Invia →"}}else if draft.mode=="inject"{"Invia inject →"}else{"Invia →"}}</button></div>
                    <small>{"Le azioni fisiche richiedono approvazione EXCON. Chat tra player visibili solo ai partecipanti. Le bozze restano private."}</small>
                </form></section>
            </main>
            <main id="map-view" class="page" hidden={self.active!="map"}><div class="map-layout"><section class="card"><div class="eyebrow">{if master{"Stato completo del simulatore"}else{"Informazioni disponibili al tuo ruolo"}}</div><h2>{text(&self.stat["name"])}</h2><SituationMap stat={self.stat.clone()} view={map_view} simple={!master}/><p id="map-status" class="muted">{if master{"Stato autorevole completo · osservazioni dei player pubblicate separatamente".into()}else if self.state["map"]["last_observation_clock"].is_string(){format!("Ultimo rapporto: {}. Le aree senza rapporti sono sconosciute.",text(&self.state["map"]["last_observation_clock"]))}else{"Nessuna rilevazione ricevuta. La mappa non segue automaticamente il simulatore.".into()}}</p>
            {for list(&self.state["map"]["forecast_products"]).iter().map(|f|widgets::details(format!("Previsione revisionata · emissione t={}s",f["issued_s"]),&json!({"assumptions":f["assumptions"],"horizons":list(&f["horizons"]).iter().map(|h|json!({"after_s":h["after_s"]})).collect::<Vec<_>>() })))}</section>
            <aside><section class="card"><h3>{if master{"Quadro operativo"}else{"Risorse e missioni"}}</h3><div id="resources" class="resources">if master{{widgets::stats(&self.state["world"]["stats"])}}else{{for self.state["resources"].as_object().into_iter().flatten().map(|(id,r)|html!{<p><b>{self.stat["resource_types"].as_array().and_then(|t|t.iter().find(|t|t["id"]==*id)).map(|r|text(&r["name"])).unwrap_or(id)}</b><br/>{format!("{} disponibili / {}",self.state["resource_accounting"][id]["available"],r["total"])}</p>})}}
            {for list(if master{&self.state["world"]["deployments"]}else{&self.state["deployments"]}).iter().map(|d|html!{<p><b>{text(&d["task"]["resource"])}</b>{format!(" · {} · {}",text(&d["task"]["status"]),text(&d["task"]["detail"]))}</p>})}</div></section>
            if master {<section class="card"><h3>{"Condividi una rilevazione"}</h3><p class="muted">{"La chat da sola non rivela lo stato del simulatore."}</p><select key={self.field("report-role",&selected)} id="report-role" value={self.field("report-role",&selected)} onchange={select("report-role")}>{for self.roles().iter().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={r["id"]==self.field("report-role",&selected)}>{text(&r["name"])}</option>})}</select><input id="report-source" placeholder="Fonte del rapporto" value={self.field("report-source","")} oninput={field("report-source")}/><button id="publish" disabled={self.busy} onclick={{let role=self.field("report-role",&selected);let source=self.field("report-source","");ctx.link().callback(move |_|Msg::Action(Action::post("/api/master/knowledge",json!({"role":role,"source":source,"current_perimeter":true}))))}}>{"Pubblica perimetro"}</button></section><section class="card"><h3>{"Collegamenti partecipanti"}</h3>{widgets::participant_links(&self.links,&self.roles())}</section>}
            </aside></div></main>
        </>}
    }
    fn chat_action(&self) -> Option<Action> {
        if self.busy
            || self.state["phase"] == "debrief"
            || (!self.is_master() && self.state["player"]["language_confirmed"] != true)
        {
            return None;
        }
        let draft = self.draft();
        if !self.is_master()
            && self.field("interpret", "true") == "true"
            && self.state["phase"] == "orders"
            && draft.private["status"] == "ready"
            && !draft.dirty
        {
            return None;
        }
        let to = self.selected.clone();
        let message = draft.text.trim();
        if message.is_empty() {
            return None;
        }
        let mut action = Action::post(
            if self.is_master() {
                "/api/master/chat".into()
            } else {
                format!("/api/player/{}/chat", self.own)
            },
            json!({"to":to,"text":message,"interpret":false}),
        );
        action.kind = ActionKind::Send(to.clone(), draft.text.clone());
        if !self.is_master()
            && self.field("interpret", "true") == "true"
            && self.state["phase"] == "orders"
        {
            let mut body = json!({"to":to,"text":message});
            if draft.private["status"] == "needs_clarification" && !draft.dirty {
                body["draft"] = draft.private["id"].clone();
                body["revision"] = draft.private["revision"].clone();
            }
            action = Action {
                path: format!("/api/player/{}/draft", self.own),
                body: Some(body),
                kind: ActionKind::Prepare(to),
            };
        } else if self.is_master() && draft.mode == "inject" {
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
            } else {
                action.path = "/api/master/inject".into();
                action.body = Some(
                    json!({"title":draft.title.trim(),"sender":draft.sender.trim(),"text":message,"recipients":if scenario{template.unwrap()["recipients"].clone()}else{json!([to])}}),
                );
            }
        }
        Some(action)
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
