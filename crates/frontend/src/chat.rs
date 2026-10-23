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
        self.view_chat_content(ctx, false)
    }
    pub fn view_inject_form(&self, ctx: &Context<Self>) -> Html {
        self.view_chat_content(ctx, true)
    }
    fn view_chat_content(&self, ctx: &Context<Self>, composer_only: bool) -> Html {
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
        let send_disabled = self.busy || self.state["phase"] == "debrief";
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
        let map_click = ctx.link().callback(|point: Value| {
            Msg::Field("work:location".into(), text(&point["zone"]).into())
        });
        let work_draft = self.drafts.get("master").cloned().unwrap_or_default();
        let work_private = &work_draft.private;
        let work_allowed = list(&self.state["role"]["allowed_actions"]);
        let work_action = self.field(
            "work:action",
            work_allowed.first().map(|v| text(v)).unwrap_or(""),
        );
        let location_options: Vec<String> =
            if ["CloseRoad", "ReopenRoad"].contains(&work_action.as_str()) {
                list(&self.stat["roads"])
                    .iter()
                    .map(|r| text(&r["id"]).to_string())
                    .collect()
            } else if ["EvacuateBuilding", "OpenShelter"].contains(&work_action.as_str()) {
                list(&self.stat["buildings"])
                    .iter()
                    .map(|b| {
                        if text(&b["name"]).is_empty() {
                            format!("#{}", b["id"])
                        } else {
                            text(&b["name"]).to_string()
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
        let entered_location = self.field("work:location", "A1");
        let work_location =
            if !location_options.is_empty() && !location_options.contains(&entered_location) {
                location_options[0].clone()
            } else {
                entered_location
            };
        let work_destination = self.field("work:destination", "A2");
        let work_resource = self.field(
            "work:resource",
            list(&self.stat["resource_types"])
                .first()
                .map(|r| text(&r["id"]))
                .unwrap_or("ground_crews"),
        );
        let work_qty = self.field("work:qty", "1");
        let work_free_text = self.field("work:free_text", "");
        let work_reply_to = self.field("work:reply_to", "");
        let work_reply = self.field("work:reply", "");
        let work_clarifying = work_private["status"] == "needs_clarification";
        let work_draft_id = work_private["id"].clone();
        let work_revision = work_private["revision"].clone();
        let work_submit = {
            let own = own.clone();
            let clarification = work_clarifying;
            let reply = if work_clarifying {
                work_reply.clone()
            } else {
                work_free_text.clone()
            };
            let draft_id = work_draft_id.clone();
            let revision = work_revision.clone();
            ctx.link().callback(move |_| {
                let mut body = json!({"to":"master","text":reply});
                if clarification {
                    body["draft"] = draft_id.clone();
                    body["revision"] = revision.clone();
                }
                let mut request = Action::post(format!("/api/player/{own}/draft"), body);
                request.kind = ActionKind::Prepare("master".into());
                Msg::Action(request)
            })
        };
        let work_command = {
            let own = own.clone();
            let action = work_action.clone();
            let location = work_location.clone();
            let destination = work_destination.clone();
            let resource = work_resource.clone();
            let qty = work_qty.parse::<u32>().unwrap_or(1).max(1);
            let reply_to = work_reply_to.parse::<u64>().ok();
            ctx.link().callback(move |_| {
                let action_name = action.clone();
                let summary_location = location.clone();
                let command = match action.as_str() {
                    "IssueWarning" | "EvacuateZone" => json!({"type":action,"zone":location}),
                    "EvacuateBuilding" | "OpenShelter" => {
                        json!({"type":action,"building":location})
                    }
                    "WaterDrop" | "DeployEngine" => json!({"type":action,"at":location}),
                    "CutFireLine" => json!({"type":action,"from":location,"to":destination}),
                    "CloseRoad" | "ReopenRoad" => json!({"type":action,"road":location}),
                    "RequestReinforcement" => json!({"type":action,"resource":resource,"qty":qty}),
                    _ => Value::Null,
                };
                let mut body = json!({"commands":[command],"text":format!("Player submits {action_name} for {summary_location}.")});
                if let Some(message) = reply_to { body["reply_to"] = json!(message); }
                Msg::Action(Action::post(format!("/api/player/{own}/actions"), body))
            })
        };
        let composer = html! {                    if master || selected!="injects" {<form class="composer" id="composer" onsubmit={submit}>
            if master{<div id="conversation-injects"><div class="row conversation-message-kind"><b>{"Inject dell’esercitazione"}</b></div>
            <div id="inject-options" hidden={draft.mode!="inject"}>
                <div class="inject-compose-row"><label for="inject-template">{"Da scenario"}</label><select key={format!("template:{selected}:{}",draft.template)} id="inject-template" value={draft.template.clone()} onchange={select("draft:template")}><option value="" selected={draft.template.is_empty()}>{"Inject personalizzato"}</option>{for list(&self.stat["msel"]).iter().filter(|t|selected=="all"||list(&t["recipients"]).contains(&json!("all"))||list(&t["recipients"]).contains(&json!(selected))).map(|t|html!{<option value={text(&t["id"]).to_string()} selected={t["id"]==draft.template}>{format!("{} · {} · turno {}",text(&t["id"]),text(&t["title"]),t["turn"])}</option>})}</select></div>
                <div class="inject-compose-grid"><label>{"Titolo"}<input id="inject-title" value={draft.title.clone()} oninput={field("draft:title")} required={draft.mode=="inject"}/></label><label>{"Mittente simulato"}<input id="inject-sender" value={draft.sender.clone()} oninput={field("draft:sender")} required={draft.mode=="inject"}/></label></div>
                <div class="inject-compose-row"><label>{"Destinatari"}</label><select key={format!("scope:{selected}:{}",draft.scope)} id="inject-scope" value={draft.scope.clone()} onchange={select("draft:scope")}><option value="conversation" selected={draft.scope=="conversation"}>{if selected=="all"{"Tutti i player".to_string()}else{format!("Solo {}",self.name(&selected))}}</option>if !draft.template.is_empty(){<option value="scenario" selected={draft.scope=="scenario"}>{"Destinatari dello scenario"}</option>}</select></div>
                <p class="muted">{"Il player risponde dalla vista di lavoro; la proposta appare nella timeline per l’approvazione. L’invio dell’inject non cambia la fisica."}</p>
            </div></div>}
            if self.busy{<div class="activity" role="status" aria-live="polite"><span class="loader" aria-hidden="true"></span><span>{&self.notice}</span></div>}
            <textarea id="text" maxlength="8000" aria-label="Messaggio" placeholder="Scrivi un messaggio…" required=true value={draft.text.clone()} oninput={field("draft:text")} onkeydown={keydown}/>
            <div class="row">if master{<small>{"Testo EXCON in inglese · tradotto per il destinatario"}</small>}<div class="grow"/><span id="send-status" class="muted" role="status">{&self.notice}</span><button id="send" class="primary" disabled={send_disabled}>{if self.busy{"Invio…"}else if master{"Invia inject →"}else{"Invia →"}}</button></div>
        </form>}else{<div class="composer"><p class="muted">{"Gli inject sono informazioni di EXCON. Rispondi usando le conversazioni tra partecipanti o le proposte operative dalla mappa."}</p></div>}};
        if composer_only {
            return composer;
        }
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
                    html!{<article key={m["id"].to_string()} class={classes!("message",(from==own).then_some("own"),(m["kind"]=="inject").then_some("inject"))}>
                        <div class="meta">{format!("{} · {} · turno {}{}",self.name(from),text(&m["clock"]),m["turn"],if m["kind"]=="inject"{" · INJECT PRIVATO"}else{""})}</div>
                        if !text(&m["title"]).is_empty(){<b>{text(&m["title"])}</b>}<div class="text">{text(body)}</div>
                        if m["original"]!=*body&&!m["original"].is_null(){<details><summary>{"Testo originale"}</summary>{text(&m["original"])}</details>}
                        if m["translation_failed"]==true{<p class="danger">{"Traduzione non disponibile · testo di origine o inglese"}</p>}
                        if let Some(p)=proposal{if master {<Proposal key={p["id"].to_string()} proposal={p.clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)}/>}else{<div class="status">{api::status(text(&p["status"]))}</div>}}
                    </article>}
                })}
                if master {{for visible.iter().filter(|p|!items.iter().any(|m|m["proposal"]==p["id"])).map(|p|html!{<article class="message" key={format!("orphan:{}",p["id"])}><Proposal proposal={(*p).clone()} generation={self.state["generation"].clone()} preview={self.responses.get(&format!("proposal:{}",p["id"])).cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)}/></article>})}}
                </MessageList>
                    {composer}
                </section>
            </main>
            <main id="map-view" class="page" hidden={self.active!="map"}><div class="map-layout"><section class="card"><div class="eyebrow">{if master{"Stato completo del simulatore"}else{"Informazioni disponibili al tuo ruolo"}}</div><h2>{text(&self.stat["name"])}</h2><SituationMap stat={self.stat.clone()} view={map_view} simple={!master} onclick={map_click}/><p id="map-status" class="muted">{if master{"Stato autorevole completo · osservazioni dei player pubblicate separatamente".into()}else if self.state["map"]["last_observation_clock"].is_string(){format!("Ultimo rapporto: {}. Le aree senza rapporti sono sconosciute.",text(&self.state["map"]["last_observation_clock"]))}else{"Nessuna rilevazione ricevuta. La mappa non segue automaticamente il simulatore.".into()}}</p>
            {for list(&self.state["map"]["forecast_products"]).iter().map(|f|widgets::details(format!("Previsione revisionata · emissione t={}s",f["issued_s"]),&json!({"assumptions":f["assumptions"],"horizons":list(&f["horizons"]).iter().map(|h|json!({"after_s":h["after_s"]})).collect::<Vec<_>>() })))}</section>
            <aside><section class="card"><h3>{if master{"Quadro operativo"}else{"Risorse e missioni"}}</h3><div id="resources" class="resources">if master{{widgets::stats(&self.state["world"]["stats"])}}else{{for self.state["resources"].as_object().into_iter().flatten().map(|(id,r)|html!{<p><b>{self.stat["resource_types"].as_array().and_then(|t|t.iter().find(|t|t["id"]==*id)).map(|r|text(&r["name"])).unwrap_or(id)}</b><br/>{format!("{} disponibili / {}",self.state["resource_accounting"][id]["available"],r["total"])}</p>})}}
            {for list(if master{&self.state["world"]["deployments"]}else{&self.state["deployments"]}).iter().map(|d|html!{<p><b>{text(&d["task"]["resource"])}</b>{format!(" · {} · {}",text(&d["task"]["status"]),text(&d["task"]["detail"]))}</p>})}</div></section>
            if !master {if self.messages().iter().any(|m|m["kind"]=="inject"){<section class="card" id="work-injects"><h3>{"Inject ricevuti"}</h3>{for self.messages().iter().filter(|m|m["kind"]=="inject").rev().take(5).map(|m|html!{<article><b>{format!("Turno {} · {}",m["turn"],text(&m["title"]))}</b><p>{text(&m["text"])}</p></article>})}</section>}
            if !list(&self.state["proposals"]).is_empty(){<section class="card" id="player-proposals"><h3>{"Le tue proposte"}</h3>{for list(&self.state["proposals"]).iter().rev().map(|p|html!{<p>{text(&p["original"])}<br/><b>{api::status(text(&p["status"]))}</b></p>})}</section>}
            <section class="card" id="player-work">
                <h3>{"Proponi un’azione"}</h3>
                <p class="muted">{"Le proposte operative arrivano a EXCON per revisione prima di produrre effetti."}</p>
                <label for="work-action">{"Tipo di intervento"}</label>
                <select id="work-action" value={work_action.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:action".into(),api::select(e)))}>
                    {for work_allowed.iter().map(|v|html!{<option value={text(v).to_string()} selected={text(v)==work_action}>{match text(v){"IssueWarning"=>"Avviso alla popolazione","EvacuateZone"=>"Evacuazione di zona","EvacuateBuilding"=>"Evacuazione di edificio","WaterDrop"=>"Lancio d’acqua","DeployEngine"=>"Invio autobotte","CutFireLine"=>"Linea tagliafuoco","CloseRoad"=>"Chiusura strada","ReopenRoad"=>"Riapertura strada","OpenShelter"=>"Apertura rifugio","RequestReinforcement"=>"Richiesta rinforzi",x=>x}}</option>})}
                </select>
                <label for="work-location">{"Zona / luogo"}</label>
                if work_action=="CloseRoad"||work_action=="ReopenRoad" {
                    <select id="work-location" value={work_location.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:location".into(),api::select(e)))}>{for self.stat["roads"].as_array().into_iter().flatten().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==work_location}>{text(&r["name"])}</option>})}</select>
                } else if work_action=="EvacuateBuilding"||work_action=="OpenShelter" {
                    <select id="work-location" value={work_location.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:location".into(),api::select(e)))}>{for self.stat["buildings"].as_array().into_iter().flatten().map(|b|{let id=b["id"].as_u64().unwrap_or(0);let name=text(&b["name"]);let value=if name.is_empty(){format!("#{id}")}else{name.to_string()};html!{<option value={value.clone()} selected={value==work_location}>{if name.is_empty(){value}else{name.to_string()}}</option>}})}</select>
                } else {
                    <input id="work-location" value={work_location.clone()} oninput={ctx.link().callback(|e|Msg::Field("work:location".into(),api::input(e)))}/>
                }
                <label for="work-reply-to">{"Rispondi a inject (facoltativo)"}</label>
                <select id="work-reply-to" value={work_reply_to.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:reply_to".into(),api::select(e)))}><option value="" selected={work_reply_to.is_empty()}>{"Proposta indipendente"}</option>{for self.messages().iter().filter(|m|m["kind"]=="inject").map(|m|html!{<option value={m["id"].to_string()} selected={m["id"].to_string()==work_reply_to}>{format!("Turno {} · {}",m["turn"],text(&m["title"]))}</option>})}</select>
                if work_action=="CutFireLine" {<label for="work-destination">{"Secondo punto"}</label><input id="work-destination" value={work_destination} oninput={ctx.link().callback(|e|Msg::Field("work:destination".into(),api::input(e)))}/>}
                if work_action=="RequestReinforcement" {<label for="work-resource">{"Risorsa richiesta"}</label><select id="work-resource" value={work_resource.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:resource".into(),api::select(e)))}>{for self.stat["resource_types"].as_array().into_iter().flatten().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==work_resource}>{text(&r["name"])}</option>})}</select><label for="work-qty">{"Quantità"}</label><input id="work-qty" type="number" min="1" value={work_qty} oninput={ctx.link().callback(|e|Msg::Field("work:qty".into(),api::input(e)))}/>}
                <button class="primary" disabled={self.busy||self.state["phase"]!="orders"||work_allowed.is_empty()} onclick={work_command}>{"Invia proposta operativa a EXCON →"}</button>
                <hr/>
                <h4>{"Proposta con testo libero"}</h4>
                <p class="muted">{"L’AI interpreta la richiesta e prepara una bozza privata. Puoi rispondere ai chiarimenti o chiedere una revisione manuale se l’AI non è disponibile."}</p>
                if work_clarifying {<label for="work-reply">{"Rispondi al chiarimento"}</label><textarea id="work-reply" value={work_reply.clone()} oninput={ctx.link().callback(|e|Msg::Field("work:reply".into(),api::input(e)))}/>}else{<label for="work-free-text">{"Descrivi la richiesta"}</label><textarea id="work-free-text" value={work_free_text.clone()} oninput={ctx.link().callback(|e|Msg::Field("work:free_text".into(),api::input(e)))}/>}
                <button disabled={self.busy||self.state["phase"]!="orders"||(work_clarifying&&work_reply.trim().is_empty())||(!work_clarifying&&work_free_text.trim().is_empty())} onclick={work_submit}>{if work_clarifying{"Invia chiarimento →"}else{"Prepara bozza AI →"}}</button>
                if !work_private.is_null() {
                    <div class="proposal-preview">
                        <b>{if work_private["status"]=="needs_clarification"{"Chiarimento richiesto"}else if work_private["status"]=="ai_unavailable"{"AI non disponibile"}else{"Bozza privata"}}</b>
                        if work_private["status"]=="needs_clarification" {<p>{text(&work_private["question"])}</p>}
                        else {<p>{text(&work_private["message"])}</p>}
                        {for list(&work_private["action_descriptions"]).iter().map(|d|html!{<p>{text(d)}</p>})}
                        <button disabled={self.busy||!((work_private["status"]=="ready"&&!list(&work_private["actions"]).is_empty())||work_private["status"]=="ai_unavailable")} onclick={ctx.link().callback({let draft=work_private["id"].clone();let revision=work_private["revision"].clone();let manual=work_private["status"]=="ai_unavailable";let own=own.clone();move |_|{let mut a=Action::post(format!("/api/player/{own}/draft/submit"),json!({"draft":draft,"revision":revision,"manual":manual}));a.kind=ActionKind::Submit("master".into());Msg::Action(a)}})}>{if work_private["status"]=="ai_unavailable"{"Invia a EXCON per revisione manuale"}else{"Conferma e invia a EXCON"}}</button>
                        <button disabled={self.busy} onclick={ctx.link().callback(|_|Msg::Discard)}>{"Scarta"}</button>
                    </div>
                }
            </section>}
            if master {<section class="card"><h3>{"Condividi una rilevazione"}</h3><p class="muted">{"La chat da sola non rivela lo stato del simulatore."}</p><select key={self.field("report-role",&selected)} id="report-role" value={self.field("report-role",&selected)} onchange={select("report-role")}>{for self.roles().iter().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={r["id"]==self.field("report-role",&selected)}>{text(&r["name"])}</option>})}</select><input id="report-source" placeholder="Fonte del rapporto" value={self.field("report-source","")} oninput={field("report-source")}/><button id="publish" disabled={self.busy} onclick={{let role=self.field("report-role",&selected);let source=self.field("report-source","");ctx.link().callback(move |_|Msg::Action(Action::post("/api/master/knowledge",json!({"role":role,"source":source,"current_perimeter":true}))))}}>{"Pubblica perimetro"}</button></section><section class="card"><h3>{"Collegamenti partecipanti"}</h3>{widgets::participant_links(&self.links,&self.roles())}</section>}
            </aside></div></main>
        </>}
    }
    fn chat_action(&self) -> Option<Action> {
        if self.busy || self.state["phase"] == "debrief" {
            return None;
        }
        let draft = self.draft();
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
