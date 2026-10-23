use crate::{
    api::{self, list, text},
    map::SituationMap,
    widgets, Action, ActionKind, App, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;
impl App {
    pub fn view_tactical(&self, ctx: &Context<Self>) -> Html {
        let own = self.own.clone();
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
        let entered_location = self.field("work:location", "");
        let work_location =
            if !location_options.is_empty() && !location_options.contains(&entered_location) {
                location_options[0].clone()
            } else {
                entered_location
            };
        let work_destination = self.field("work:destination", "");
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
        let work_mode = self.field("work:mode", "action");
        let work_reply = self.field("work:reply", "");
        let work_clarifying = work_private["status"] == "needs_clarification";
        let work_draft_id = work_private["id"].clone();
        let work_revision = work_private["revision"].clone();
        let work_submit = {
            let parent = work_reply_to.parse::<u64>().ok();
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
                let mut body = json!({"to":"master","text":reply,"reply_to":parent});
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
                let mut body = json!({"commands":[command],"text":format!("{} · {}",widgets::command_name(&action_name),summary_location)});
                if let Some(message) = reply_to { body["reply_to"] = json!(message); }
                let mut request=Action::post(format!("/api/player/{own}/actions"),body);request.kind=ActionKind::WorkSubmit;Msg::Action(request)
            })
        };
        let work_editor = html! {            <section class="card" id="player-work">

            <div class="work-modes" role="tablist" aria-label="Modalità proposta"><button role="tab" aria-selected={(work_mode=="action").to_string()} onclick={ctx.link().callback(|_|Msg::Field("work:mode".into(),"action".into()))}>{"Azione operativa"}</button><button role="tab" aria-selected={(work_mode=="ai").to_string()} onclick={ctx.link().callback(|_|Msg::Field("work:mode".into(),"ai".into()))}>{"Assistente AI"}</button></div>
            <div hidden={work_mode!="action"}>
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
            if work_action=="CutFireLine" {<label for="work-destination">{"Secondo punto"}</label><input id="work-destination" value={work_destination.clone()} oninput={ctx.link().callback(|e|Msg::Field("work:destination".into(),api::input(e)))}/>}
            if work_action=="RequestReinforcement" {<label for="work-resource">{"Risorsa richiesta"}</label><select id="work-resource" value={work_resource.clone()} onchange={ctx.link().callback(|e|Msg::Field("work:resource".into(),api::select(e)))}>{for self.stat["resource_types"].as_array().into_iter().flatten().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==work_resource}>{text(&r["name"])}</option>})}</select><label for="work-qty">{"Quantità"}</label><input id="work-qty" type="number" min="1" value={work_qty} oninput={ctx.link().callback(|e|Msg::Field("work:qty".into(),api::input(e)))}/>}
            <button class="primary" disabled={self.busy||self.state["phase"]!="orders"||work_allowed.is_empty()||(work_action!="RequestReinforcement"&&work_location.trim().is_empty())||(work_action=="CutFireLine"&&work_destination.trim().is_empty())} onclick={work_command}>{"Invia proposta operativa a EXCON →"}</button>
            </div><div hidden={work_mode!="ai"}>
            <p class="muted">{"Descrivi l’intervento: rivedi la bozza prima di inviarla alla revisione EXCON."}</p>
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
            </div>
        </section>};
        let mut view = self.state["map"].clone();
        view["deployments"] = self.state["deployments"].clone();
        let selection = self.field("work:location", "");
        let pending = (!selection.is_empty()).then(|| json!({"zone":selection}));
        let turn = self.field("work:turn", &self.state["turn"].to_string());
        let current_turn = turn.parse::<u64>().unwrap_or(0);
        let injects = self
            .messages()
            .iter()
            .filter(|m| m["kind"] == "inject")
            .collect::<Vec<_>>();
        let proposals = list(&self.state["proposals"]);
        let turns = injects
            .iter()
            .map(|m| m["turn"].as_u64().unwrap_or(0))
            .chain(proposals.iter().map(|p| p["turn"].as_u64().unwrap_or(0)))
            .chain(std::iter::once(self.state["turn"].as_u64().unwrap_or(0)))
            .collect::<std::collections::BTreeSet<_>>();
        let mut events = injects
            .iter()
            .filter(|m| m["turn"] == current_turn)
            .map(|m| {
                (
                    m["t_s"].as_i64().unwrap_or(0),
                    m["id"].as_u64().unwrap_or(0),
                    true,
                    *m,
                )
            })
            .chain(
                proposals
                    .iter()
                    .filter(|p| p["turn"] == current_turn)
                    .map(|p| {
                        (
                            p["t_s"].as_i64().unwrap_or(0),
                            p["id"].as_u64().unwrap_or(0),
                            false,
                            p,
                        )
                    }),
            )
            .collect::<Vec<_>>();
        events.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        let selected_inject = injects
            .iter()
            .find(|m| m["id"].to_string() == work_reply_to);
        let resource_name = |id: &str| {
            list(&self.stat["resource_types"])
                .iter()
                .find(|r| r["id"] == id)
                .map(|r| text(&r["name"]).to_string())
                .unwrap_or(id.into())
        };
        let open = ctx.link().callback(Msg::OpenWork);
        let can_act = self.state["phase"] == "orders" && !self.busy;
        html! {<main id="map-view" class="page tactical-view" hidden={self.active!="map"}>
            <section class="tactical-toolbar"><div><strong>{"Quadro tattico"}</strong><span class="muted">{self.name(&own)}</span></div><div class="grow"/><button class="primary" id="new-player-action" disabled={!can_act} onclick={{let open=open.clone();Callback::from(move |_|open.emit(None))}}>{"+ Nuova azione"}</button></section>
            <section class="tactical-resources" id="resources" aria-label="Risorse disponibili">{for self.state["resources"].as_object().into_iter().flatten().map(|(id,pool)|{let account=&self.state["resource_accounting"][id];html!{<div class="resource-token"><strong>{account["available"].to_string()}</strong><span>{resource_name(id)}<small>{format!("su {} · impegnate {}",pool["total"],account["committed"].as_u64().unwrap_or(0)+account["deployed"].as_u64().unwrap_or(0))}</small></span></div>}})}</section>
            <div class="tactical-grid"><section class="tactical-map card"><div class="tactical-map-heading"><h3>{"Mappa di situazione"}</h3>if !selection.is_empty(){<span class="selection-chip">{format!("Selezione: {selection}")}</span>}</div><p id="map-status" class={classes!("observation-status",(!self.state["map"]["last_observation_clock"].is_string()).then_some("unknown"))}>{if let Some(clock)=self.state["map"]["last_observation_clock"].as_str(){format!("Rapporto EXCON · {clock}")}else{"Nessun rapporto sull’incendio ricevuto · situazione non nota".into()}}</p>
                <SituationMap stat={self.stat.clone()} view={view} simple=true onclick={map_click} {pending}/><p class="map-instruction">{"Seleziona una zona sulla mappa, poi apri un’azione. La selezione non invia ordini."}</p>
                if !list(&self.state["map"]["forecast_products"]).is_empty(){<details><summary>{"Previsioni ricevute"}</summary>{for list(&self.state["map"]["forecast_products"]).iter().map(|f|widgets::details(format!("Emissione t={}s",f["issued_s"]),&json!({"assumptions":f["assumptions"],"horizons":f["horizons"]})))}</details>}
                if !list(&self.state["deployments"]).is_empty(){<details open=true class="tactical-missions"><summary>{"Missioni in corso"}</summary>{for list(&self.state["deployments"]).iter().map(|d|html!{<p><b>{resource_name(text(&d["task"]["resource"]))}</b>{format!(" · {}",text(&d["task"]["status"]))}<br/><small>{text(&d["task"]["detail"])}</small></p>})}</details>}
            </section>
            <section class="tactical-timeline card" id="player-timeline"><header><div><h3>{"Inject e risposte"}</h3><small>{"Solo gli eventi del tuo ruolo"}</small></div><label>{"Turno "}<select aria-label="Turno del player" value={turn} onchange={ctx.link().callback(|e|Msg::Field("work:turn".into(),api::select(e)))}>{for turns.iter().rev().map(|t|html!{<option value={t.to_string()} selected={*t==current_turn}>{t}</option>})}</select></label></header>
                if events.is_empty(){<div class="tactical-empty"><b>{if self.state["phase"]=="briefing"{"In attesa dell’avvio EXCON"}else{"Nessun evento in questo turno"}}</b><p>{"Qui compariranno gli inject ricevuti e lo stato delle tue risposte. Puoi anche proporre un’azione indipendente."}</p></div>}
                <ol class="tactical-events">{for events.iter().map(|(_,_,inject,event)|{
                    if *inject {let id=event["id"].to_string();let latest=proposals.iter().filter(|p|p["reply_to"]==event["id"]&&p["status"]!="rolled_back").last();let replied=latest.is_some();let rejected=latest.is_some_and(|p|p["status"]=="rejected");html!{<li class="tactical-event inject-event"><div class="event-meta"><span>{"INJECT"}</span><time>{text(&event["clock"])}</time>if replied{<span class={classes!("event-replied",rejected.then_some("rejected"))}>{if rejected{"Risposta rifiutata da EXCON"}else{"Risposta inviata"}}</span>}</div><h4>{text(&event["title"])}</h4><p>{text(&event["text"])}</p><div class="event-footer"><small>{format!("Da {}",self.name(text(&event["from"])))}</small><button class="reply-inject" disabled={!can_act} data-reply-inject={id.clone()} onclick={{let open=open.clone();Callback::from(move |_|open.emit(Some(id.clone())))}}>{if rejected{"Rispondi di nuovo"}else if replied{"Altra risposta"}else{"Rispondi →"}}</button></div></li>}}
                    else {let parent=injects.iter().find(|m|m["id"]==event["reply_to"]);html!{<li class={classes!("tactical-event","response-event",(event["status"]=="rejected").then_some("rejected"))} data-player-proposal={event["id"].to_string()}><div class="event-meta"><span>{"TUA PROPOSTA"}</span><span class="pill">{api::status(text(&event["status"]))}</span></div>if let Some(parent)=parent{<small>{format!("In risposta a: {}",text(&parent["title"]))}</small>}<p>{text(&event["original"])}</p>if !text(&event["decision"]["note"]).is_empty(){<p class={classes!("decision-note",(event["status"]=="rejected").then_some("decision-reason"))}>{if event["status"]=="rejected"{format!("EXCON · motivo: {}",text(&event["decision"]["note"]))}else{format!("EXCON · {}",text(&event["decision"]["note"]))}}</p>}if !list(&event["executions"]).is_empty(){<details><summary>{"Esiti di esecuzione"}</summary><pre>{api::pretty(&event["executions"])}</pre></details>}</li>}}
                })}</ol>
            </section></div>
            if self.field("work:open","false")=="true"{<div class="modal-backdrop"><section class="work-modal" role="dialog" aria-modal="true" aria-labelledby="work-modal-title" onkeydown={{let link=ctx.link().clone();let busy=self.busy;Callback::from(move |e:KeyboardEvent|{if e.key()=="Escape"&&!busy{e.prevent_default();link.send_message(Msg::Field("work:open".into(),"false".into()));}})}}><header><h2 id="work-modal-title">{if selected_inject.is_some(){"Rispondi all’inject"}else{"Nuova azione"}}</h2><button aria-label="Chiudi risposta" disabled={self.busy} onclick={ctx.link().callback(|_|Msg::Field("work:open".into(),"false".into()))}>{"Chiudi"}</button></header>
                if let Some(inject)=selected_inject{<div class="work-context"><strong>{text(&inject["title"])}</strong><small>{text(&inject["clock"])}</small><p>{text(&inject["text"])}</p></div>}
                <p class="work-approval-note">{"La proposta viene inviata a EXCON per approvazione prima di produrre effetti."}</p>
                if !self.error.is_empty(){<p class="error" role="alert">{&self.error}</p>}
                {work_editor}
            </section></div>}
        </main>}
    }
}
