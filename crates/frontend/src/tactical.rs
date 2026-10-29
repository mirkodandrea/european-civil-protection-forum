use crate::{
    api::{self, list, text},
    i18n,
    map::SituationMap,
    widgets, Action, ActionKind, Msg,
};
use serde_json::{json, Value};

/// Largest PDF the server accepts (`exercise::files::MAX_PDF_BYTES`).
const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
use yew::prelude::*;
/// Letters and digits only, lower case: "Pineta Primary-School" == "pineta primary school".
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// What kind of place an action takes: a grid zone, a building, a road, a point.
fn place_kind(action: &str) -> &'static str {
    match action {
        "IssueWarning" | "EvacuateZone" | "MonitorArea" => "zone",
        "EvacuateBuilding" | "OpenShelter" => "building",
        "CloseRoad" | "ReopenRoad" => "road",
        _ => "point",
    }
}

/// `F4`-style code inside the scenario grid.
fn is_zone_code(stat: &Value, s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let (cols, rows) = (stat["zones"]["cols"].as_u64().unwrap_or(0), stat["zones"]["rows"].as_u64().unwrap_or(0));
    let mut it = s.chars();
    let (Some(letter), Ok(row)) = (it.next(), it.as_str().parse::<u64>()) else { return false };
    letter.is_ascii_uppercase() && ((letter as u8 - b'A') as u64) < cols && (1..=rows).contains(&row)
}

/// The facility called `name` (case and punctuation ignored), if any.
fn building_named(stat: &Value, name: &str) -> Option<String> {
    let k = norm(name);
    (!k.is_empty()).then_some(())?;
    list(&stat["buildings"]).iter().map(|b| text(&b["name"])).find(|n| norm(n) == k).map(str::to_string)
}

impl crate::player_view::PlayerView {
    pub fn view_tactical(&self) -> Html {
        let own = self.own.clone();
        let l = self.lang();
        let l = l.as_str();
        let click_action = self.field(
            "work:action",
            list(&self.state["role"]["allowed_actions"])
                .first()
                .map(text)
                .unwrap_or(""),
        );
        let click_field = self.field("work:target_field", "work:location");
        let monitor_zones = self.field("work:location", "");
        let map_click = self.cb(move |point: Value| {
            let (field, value) = match click_action.as_str() {
                "CloseRoad" | "ReopenRoad" => ("work:location", text(&point["road"]).to_string()),
                "EvacuateBuilding" | "OpenShelter" => {
                    ("work:location", text(&point["building"]).to_string())
                }
                "DeployEngine" | "WaterDrop" => (
                    "work:location",
                    format!(
                        "{},{}",
                        point["x"].as_f64().unwrap_or(0.),
                        point["y"].as_f64().unwrap_or(0.)
                    ),
                ),
                "CutFireLine" if point["x"].is_number() => (
                    if click_field == "work:destination" {
                        "work:destination"
                    } else {
                        "work:location"
                    },
                    format!(
                        "{},{}",
                        point["x"].as_f64().unwrap_or(0.),
                        point["y"].as_f64().unwrap_or(0.)
                    ),
                ),
                "MonitorArea" => {
                    // Clicking zones builds the list; clicking one again removes it.
                    let mut zones: Vec<String> = monitor_zones.split(',').map(|z| z.trim().to_string()).filter(|z| !z.is_empty()).collect();
                    let clicked = text(&point["zone"]).to_string();
                    match zones.iter().position(|z| *z == clicked) {
                        Some(i) => { zones.remove(i); }
                        None => zones.push(clicked),
                    }
                    ("work:location", zones.join(", "))
                }
                _ => ("work:location", text(&point["zone"]).to_string()),
            };
            Msg::Field(field.into(), value)
        });
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
                String::new()
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
        let work_reply_to = self.field("work:reply_to", "");
        let staged = self.staged();
        let work_conversation = self.field("work:conversation", "").parse::<u64>().ok();
        let narrative_text = self.field("work:narrative", "");
        let work_command = {
            let action = work_action.clone();
            let location = work_location.clone();
            let destination = work_destination.clone();
            let resource = work_resource.clone();
            let qty = work_qty.parse::<u32>().unwrap_or(1).max(1);
            self.batch(move |_| {
                let loc = |value: &str| -> Value {
                    let mut pair = value.split(',').filter_map(|part| part.parse::<f32>().ok());
                    match (pair.next(), pair.next()) {
                        (Some(x), Some(y)) => json!({"x":x,"y":y}),
                        _ => json!(value),
                    }
                };
                let command = match action.as_str() {
                    "IssueWarning" | "EvacuateZone" => json!({"type":action,"zone":location}),
                    "MonitorArea" => json!({"type":action,"zones":location.split(',').map(str::trim).filter(|z| !z.is_empty()).collect::<Vec<_>>()}),
                    "EvacuateBuilding" | "OpenShelter" => {
                        json!({"type":action,"building":location})
                    }
                    "WaterDrop" | "DeployEngine" => json!({"type":action,"at":loc(&location)}),
                    "CutFireLine" => json!({"type":action,"from":loc(&location),"to":loc(&destination)}),
                    "CloseRoad" | "ReopenRoad" => json!({"type":action,"road":location}),
                    "RequestReinforcement" => json!({"type":action,"resource":resource,"qty":qty}),
                    _ => Value::Null,
                };
                (!command.is_null()).then(|| Msg::StageActions(vec![command]))
            })
        };
        let selected_context = self
            .messages()
            .iter()
            .find(|m| m["id"].to_string() == work_reply_to)
            .or_else(|| {
                // An inject the player never received is read through the conversation.
                work_conversation.and_then(|cid| {
                    list(&self.state["linked_conversations"])
                        .iter()
                        .find(|c| c["id"].as_u64() == Some(cid))
                        .map(|c| &c["quote"])
                })
            });
        let answers_inject = selected_context.is_some_and(|m| m["kind"] == "inject");
        let has_narrative = !narrative_text.trim().is_empty();
        // Communication or decision: an explicit choice (an action makes it a decision).
        let work_kind = if staged.is_empty() { self.field("work:kind", "") } else { "decision".into() };
        let decision = work_kind == "decision";
        // Recipient: the fictitious sender of the inject, or real players.
        let to_players_mode = self.field("work:to", "persona") == "players";
        let to_players: Vec<String> = self.field("work:to_players", "").split(',').filter(|p| !p.is_empty()).map(String::from).collect();
        let sender_name = selected_context
            .map(|m| if m["persona"].is_string() { text(&m["persona"]).to_string() } else { self.name(text(&m["from"])) })
            .unwrap_or_default();
        // From a conversation only its participants can be addressed.
        let player_choices: Vec<(String, String)> = match work_conversation.and_then(|cid| list(&self.state["linked_conversations"]).iter().find(|c| c["id"].as_u64() == Some(cid))) {
            Some(c) => list(&c["participants"]).iter().map(text).filter(|p| *p != own).map(|p| (p.to_string(), self.name(p))).collect(),
            None => self.contacts().iter().filter(|r| r["id"] != "master").map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string())).collect(),
        };
        let work_basis_chosen = self.field("work:basis", "");
        // What the disabled "send" button is waiting for.
        let respond_missing = if self.state["phase"] != "orders" { Some("work_need_orders") }
            else if !answers_inject { Some("work_need_inject") }
            else if work_kind.is_empty() { Some("need_kind") }
            else if !has_narrative && (!decision || staged.is_empty() || to_players_mode) { Some("need_text") }
            else if decision && work_basis_chosen.is_empty() { Some("need_basis") }
            else if to_players_mode && to_players.is_empty() { Some("need_players") }
            else { None };
        // What the disabled "add action" button is waiting for.
        let work_missing = if work_allowed.is_empty() { Some("work_need_action") } else if work_action != "RequestReinforcement" && work_location.trim().is_empty() { Some("work_need_place") } else if work_action == "CutFireLine" && work_destination.trim().is_empty() { Some("work_need_second") } else { None };
        // Zone-type actions take a grid code (or a named place); a building typed
        // there switches the action to the building one instead of being refused later.
        let zone_options: Vec<String> = if place_kind(&work_action) == "zone" {
            let (cols, rows) = (self.stat["zones"]["cols"].as_u64().unwrap_or(0) as u8, self.stat["zones"]["rows"].as_u64().unwrap_or(0) as u8);
            (1..=rows).flat_map(|r| (0..cols).map(move |c| format!("{}{}", (b'A' + c) as char, r)))
                .chain(list(&self.stat["places"]).iter().map(|p| text(&p["name"]).to_string()).filter(|n| !n.is_empty()))
                .collect()
        } else { Vec::new() };
        let zone_input = {
            let (stat, action) = (self.stat.clone(), work_action.clone());
            let building_allowed = work_allowed.iter().any(|a| text(a) == "EvacuateBuilding");
            self.batch(move |e: InputEvent| {
                let v = api::input(e);
                match building_named(&stat, &v) {
                    Some(name) if action == "EvacuateZone" && building_allowed => vec![
                        Msg::Field("work:action".into(), "EvacuateBuilding".into()),
                        Msg::Field("work:location".into(), name.clone()),
                        Msg::Field("work:notice".into(), name),
                    ],
                    _ => vec![Msg::Field("work:notice".into(), String::new()), Msg::Field("work:location".into(), v)],
                }
            })
        };
        let notice = self.field("work:notice", "");
        let zone_notice = if !notice.is_empty() && place_kind(&work_action) == "building" {
            i18n::tf(l, "place_switched", &[&notice, i18n::action(l, "EvacuateBuilding")])
        } else if work_action != "MonitorArea" && place_kind(&work_action) == "zone" && !work_location.trim().is_empty() && !is_zone_code(&self.stat, &work_location) {
            if building_named(&self.stat, &work_location).is_some() {
                i18n::tf(l, "place_is_building", &[work_location.trim()])
            } else if zone_options.iter().any(|o| norm(o) == norm(&work_location)) {
                String::new()
            } else {
                i18n::tf(l, "place_not_zone", &[work_location.trim()])
            }
        } else { String::new() };
        // What the response rests on; a PDF only goes with plans or laws.
        let work_basis = work_basis_chosen.clone();
        let file_name = self.field("work:file_name", "");
        let file_size = self.field("work:file_size", "0").parse::<usize>().unwrap_or(0);
        let file_error = self.field("work:file_error", "");
        let doc_allowed = work_basis == "plan" || work_basis == "law";
        let on_file = {
            let link = self.link.clone();
            Callback::from(move |e: Event| {
                use yew::TargetCast;
                let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
                let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
                // Choosing the same file again must fire a change event.
                input.set_value("");
                let link = link.clone();
                let set = move |k: &str, v: String| link.emit(Msg::Field(k.into(), v));
                wasm_bindgen_futures::spawn_local(async move {
                    let name = file.name();
                    let size = file.size() as usize;
                    let error = if size > MAX_PDF_BYTES {
                        Some("doc_too_big")
                    } else if !(file.type_() == "application/pdf" || name.to_lowercase().ends_with(".pdf")) {
                        Some("doc_not_pdf")
                    } else {
                        None
                    };
                    if let Some(error) = error {
                        set("work:file_error", error.into());
                        return;
                    }
                    match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        Ok(buffer) => {
                            use base64::Engine;
                            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                            set("work:file_data", base64::engine::general_purpose::STANDARD.encode(&bytes));
                            set("work:file_name", name);
                            set("work:file_size", bytes.len().to_string());
                            set("work:file_error", String::new());
                        }
                        Err(_) => set("work:file_error", "doc_read_fail".into()),
                    }
                });
            })
        };
        let drop_file = self.batch(|_| {
            ["work:file_data", "work:file_name", "work:file_size", "work:file_error"]
                .into_iter()
                .map(|k| Msg::Field(k.into(), String::new()))
                .collect::<Vec<_>>()
        });
        let respond = {
            let own = own.clone();
            let narrative = narrative_text.clone();
            let commands = staged.clone();
            let parent = work_reply_to.parse::<u64>().ok();
            let basis = work_basis.clone();
            let attachment = (doc_allowed && !file_name.is_empty()).then(|| {
                json!({"name": file_name.clone(), "content_type": "application/pdf", "data": self.field("work:file_data", "")})
            });
            let summary = staged.iter().map(|c| widgets::command_summary("en", c)).collect::<Vec<_>>().join("; ");
            let (kind, players) = (work_kind.clone(), to_players_mode.then(|| to_players.clone()));
            self.cb(move |_| {
                let decision = kind == "decision";
                let mut body = json!({"kind":kind,"commands":if decision {commands.clone()} else {vec![]},"text":summary,"reply_to":parent,"conversation":work_conversation});
                if !narrative.trim().is_empty() { body["narrative"] = json!(narrative); }
                if decision { body["basis"] = json!(basis); }
                if let Some(players) = &players { body["to_players"] = json!(players); }
                if let Some(attachment) = attachment.as_ref().filter(|_| decision) { body["attachment"] = attachment.clone(); }
                let mut request = Action::post(format!("/api/player/{own}/actions"), body);
                request.kind = ActionKind::WorkSubmit;
                Msg::Action(request)
            })
        };
        let pick_players = |id: String, on: bool| {
            let current = to_players.clone();
            self.cb(move |_| {
                let mut next: Vec<String> = current.iter().filter(|p| **p != id).cloned().collect();
                if !on { next.push(id.clone()) }
                Msg::Field("work:to_players".into(), next.join(","))
            })
        };
        let work_editor = html! {            <section class="card" id="player-work">
            <div class="work-block" data-work-block="recipient">
                <h4 class="work-section">{i18n::t(l,"resp_to")}</h4>
                <div class="basis-options" role="radiogroup" aria-label={i18n::t(l,"resp_to")}>
                    <label class={classes!("basis-option",(!to_players_mode).then_some("is-selected"))}><input type="radio" name="work-to" id="to-sender" data-to="persona" checked={!to_players_mode} onchange={self.cb(|_|Msg::Field("work:to".into(),"persona".into()))}/><span>{i18n::tf(l,"resp_to_sender",&[&sender_name])}</span></label>
                    <label class={classes!("basis-option",to_players_mode.then_some("is-selected"))}><input type="radio" name="work-to" id="to-players" data-to="players" checked={to_players_mode} disabled={player_choices.is_empty()} onchange={self.cb(|_|Msg::Field("work:to".into(),"players".into()))}/><span>{i18n::t(l,"resp_to_players")}</span></label>
                </div>
                if to_players_mode {<div class="work-players">{for player_choices.iter().map(|(id,name)|{let on=to_players.contains(id);html!{<label class="mt-check" key={id.clone()}><input type="checkbox" data-to-player={id.clone()} checked={on} onchange={pick_players(id.clone(),on)}/>{name.clone()}</label>}})}</div>}
            </div>
            <div class="work-block" data-work-block="kind">
                <h4 class="work-section">{i18n::t(l,"resp_kind")}</h4>
                <div class="basis-options kind-options" role="radiogroup" aria-label={i18n::t(l,"resp_kind")}>
                    <label class={classes!("basis-option",(work_kind=="communication").then_some("is-selected"))}><input type="radio" name="work-kind" id="kind-communication" data-kind="communication" checked={work_kind=="communication"} disabled={!staged.is_empty()} onchange={self.cb(|_|Msg::Field("work:kind".into(),"communication".into()))}/><span><b>{i18n::t(l,"kind_comm")}</b><small>{i18n::t(l,"kind_comm_hint")}</small></span></label>
                    <label class={classes!("basis-option",decision.then_some("is-selected"))}><input type="radio" name="work-kind" id="kind-decision" data-kind="decision" checked={decision} onchange={self.cb(|_|Msg::Field("work:kind".into(),"decision".into()))}/><span><b>{i18n::t(l,"kind_dec")}</b><small>{i18n::t(l,"kind_dec_hint")}</small></span></label>
                </div>
            </div>
            <div class="work-block" data-work-block="narrative">
                <h4 class="work-section"><label for="response-narrative">{i18n::t(l,"resp_text")}</label></h4>
                <p class="work-help" data-narrative-help="">{i18n::t(l,"resp_text_help")}</p>
                <textarea id="response-narrative" data-response-narrative="" maxlength="8000" placeholder={i18n::t(l,"narr_ph")} value={narrative_text.clone()} oninput={self.cb(|e|Msg::Field("work:narrative".into(),api::input(e)))}/>
                if work_kind != "communication" {{self.view_suggestions(l, &staged)}}
            </div>
            if decision {
            <div class="work-block" data-work-block="basis">
                <h4 class="work-section">{format!("{} *",i18n::t(l,"step_basis"))}</h4>
                <p class="work-help">{i18n::t(l,"basis_hint")}</p>
                <div class="basis-options" role="radiogroup" aria-required="true" aria-label={i18n::t(l,"step_basis")}>{for [("plan","basis_plan"),("law","basis_law"),("operational_judgment","basis_judgment")].into_iter().map(|(value,label)|html!{<label key={value} class={classes!("basis-option",(work_basis==value).then_some("is-selected"))}><input type="radio" name="work-basis" id={format!("basis-{value}")} value={value} checked={work_basis==value} onchange={self.cb(move |_|Msg::Field("work:basis".into(),value.into()))}/><span>{i18n::t(l,label)}</span></label>})}</div>
                <div class="work-attach" data-work-attach="">
                    <label for="work-pdf">{i18n::t(l,"doc_label")}</label>
                    if doc_allowed {
                        <p class="work-help">{i18n::t(l,"doc_hint")}</p>
                        if file_name.is_empty() {
                            <input id="work-pdf" type="file" accept="application/pdf,.pdf" disabled={self.busy} onchange={on_file}/>
                        } else {
                            <div class="work-file" data-work-file=""><span class="work-file-name">{file_name.clone()}</span><small class="muted">{if file_size<1_048_576{format!("{} KB",file_size.div_ceil(1024))}else{format!("{:.1} MB",file_size as f64/1_048_576.0)}}</small><button type="button" disabled={self.busy} onclick={drop_file}>{i18n::t(l,"doc_remove")}</button></div>
                        }
                    } else {
                        <p class="work-help muted">{i18n::t(l,"doc_off")}</p>
                    }
                    if !file_error.is_empty() {<p class="error" role="alert">{i18n::t(l,match file_error.as_str(){"doc_too_big"=>"doc_too_big","doc_not_pdf"=>"doc_not_pdf",_=>"doc_read_fail"})}</p>}
                </div>
            </div>
            <div class="work-block" data-work-block="actions">
            <h4 class="work-section">{i18n::t(l,"sim_action")}</h4>
            if staged.is_empty() {<p class="muted" data-staged-empty="">{i18n::t(l,"sim_action_none")}</p>}
            <ol class="staged-actions">{for staged.iter().enumerate().map(|(i,c)|html!{<li data-staged-action=""><span>{widgets::command_summary(l,c)}</span><button type="button" aria-label={i18n::t(l,"remove")} disabled={self.busy} onclick={self.cb(move |_|Msg::Unstage(i))}>{"×"}</button></li>})}</ol>
            <details class="work-add" open={staged.is_empty()}><summary>{i18n::t(l,"add_action_title")}</summary>
            <label for="work-action">{i18n::t(l,"action_type")}</label>
            <select id="work-action" value={work_action.clone()} onchange={{let old=work_action.clone();let loc=work_location.clone();self.batch(move |e|{let new=api::select(e);let mut m=vec![Msg::Field("work:notice".into(),String::new())];if place_kind(&new)!=place_kind(&old)&&!loc.is_empty(){m.push(Msg::Field("work:location".into(),String::new()));}m.push(Msg::Field("work:action".into(),new));m})}}>
                {for work_allowed.iter().map(|v|html!{<option value={text(v).to_string()} selected={text(v)==work_action}>{i18n::action(l,text(v))}</option>})}
            </select>
            <label for="work-location">{format!("{} *",i18n::t(l,"place"))}</label>
            if work_action=="CloseRoad"||work_action=="ReopenRoad" {
                <select id="work-location" required=true aria-required="true" value={work_location.clone()} onchange={self.cb(|e|Msg::Field("work:location".into(),api::select(e)))}><option value="" selected={work_location.is_empty()}>{if l=="it"{"Seleziona sulla mappa o nell’elenco"}else{"Select on the map or from the list"}}</option>{for self.stat["roads"].as_array().into_iter().flatten().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==work_location}>{text(&r["name"])}</option>})}</select>
            } else if work_action=="EvacuateBuilding"||work_action=="OpenShelter" {
                <select id="work-location" required=true aria-required="true" value={work_location.clone()} onchange={self.cb(|e|Msg::Field("work:location".into(),api::select(e)))}><option value="" selected={work_location.is_empty()}>{if l=="it"{"Seleziona sulla mappa o nell’elenco"}else{"Select on the map or from the list"}}</option>{for self.stat["buildings"].as_array().into_iter().flatten().map(|b|{let id=b["id"].as_u64().unwrap_or(0);let name=text(&b["name"]);let value=if name.is_empty(){format!("#{id}")}else{name.to_string()};html!{<option value={value.clone()} selected={value==work_location}>{if name.is_empty(){value}else{name.to_string()}}</option>}})}</select>
            } else if place_kind(&work_action)=="zone" {
                <input id="work-location" list="work-zone-options" required=true aria-required="true" placeholder="F4" value={work_location.clone()} oninput={zone_input}/>
                <datalist id="work-zone-options">{for zone_options.iter().map(|z|html!{<option value={z.clone()}/>})}</datalist>
            } else {
                <input id="work-location" required=true aria-required="true" value={work_location.clone()} oninput={self.cb(|e|Msg::Field("work:location".into(),api::input(e)))}/>
            }
            if !zone_notice.is_empty() {<small class="muted work-hint" role="status" data-place-notice="">{zone_notice.clone()}</small>}
            if work_action=="CutFireLine" {<label for="work-destination">{i18n::t(l,"second_point")}</label><input id="work-destination" value={work_destination.clone()} oninput={self.cb(|e|Msg::Field("work:destination".into(),api::input(e)))}/>}
            if work_action=="RequestReinforcement" {<label for="work-resource">{i18n::t(l,"resource_req")}</label><select id="work-resource" value={work_resource.clone()} onchange={self.cb(|e|Msg::Field("work:resource".into(),api::select(e)))}>{for self.stat["resource_types"].as_array().into_iter().flatten().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==work_resource}>{text(&r["name"])}</option>})}</select><label for="work-qty">{i18n::t(l,"quantity")}</label><input id="work-qty" type="number" min="1" value={work_qty} oninput={self.cb(|e|Msg::Field("work:qty".into(),api::input(e)))}/>}
            <button data-add-action="" disabled={self.busy||work_missing.is_some()} onclick={work_command}>{i18n::t(l,"add_action")}</button>
            if let Some(missing)=work_missing{<small class="muted work-hint" role="status">{i18n::t(l,missing)}</small>}
            </details>
            </div>
            }
            <div class="work-submit-bar">
                <button class="primary" id="respond-submit" data-respond="" disabled={self.busy||respond_missing.is_some()} onclick={respond}>{i18n::t(l,"send_response")}</button>
                if let Some(missing)=respond_missing{<small class="muted work-hint" role="status">{i18n::t(l,missing)}</small>}else{<small class="muted work-hint">{i18n::t(l,if decision{"respond_hint"}else{"kind_comm_hint"})}</small>}
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
        let context_id = selected_context
            .map(|m| m["id"].as_u64().unwrap_or(0))
            .unwrap_or(0);
        let response_count = proposals
            .iter()
            .filter(|p| {
                (p["reply_to"].as_u64() == Some(context_id)
                    || selected_context.is_some_and(|m| {
                        m["kind"] == "inject"
                            && m["inject_id"].is_number()
                            && p["inject_id"] == m["inject_id"]
                    }))
                    && p["status"] != "rolled_back"
            })
            .count();
        // EXCON's open question for the proposal being answered (explicit or via the replied-to message).
        let clarify_field = self.field("work:clarify", "");
        let clarifying_proposal = proposals
            .iter()
            .find(|p| {
                p["status"] == "needs_clarification"
                    && p["clarification_request"].is_string()
                    && if clarify_field.is_empty() {
                        context_id != 0 && p["reply_to"].as_u64() == Some(context_id)
                    } else {
                        p["id"].to_string() == clarify_field
                    }
            })
            .cloned();
        // Who asked: the persona of the latest question of that proposal, else the default.
        let asker = clarifying_proposal
            .as_ref()
            .and_then(|p| {
                list(&self.state["clarification_threads"])
                    .iter()
                    .flat_map(|t| list(&t["entries"]).iter())
                    .filter(|e| e["author"] == "excon" && e["proposal"] == p["id"])
                    .last()
                    .map(|e| text(&e["persona"]).to_string())
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.name("master"));
        let resource_name = |id: &str| {
            list(&self.stat["resource_types"])
                .iter()
                .find(|r| r["id"] == id)
                .map(|r| text(&r["name"]).to_string())
                .unwrap_or(id.into())
        };
        let can_act = self.state["phase"] == "orders" && !self.busy;
        html! {<>
        <main id="map-view" class="page tactical-view" hidden={self.active!="map"}>
            <section class="tactical-toolbar"><div><strong>{i18n::t(l,"tactical")}</strong><span class="muted">{self.name(&own)}</span></div></section>
            <section class="tactical-resources" id="resources" aria-label={i18n::t(l,"resources")}>{for self.state["resources"].as_object().into_iter().flatten().map(|(id,pool)|{let account=&self.state["resource_accounting"][id];html!{<div class="resource-token"><strong>{account["available"].to_string()}</strong><span>{resource_name(id)}<small>{i18n::tf(l,"res_of",&[&pool["total"].to_string(),&(account["committed"].as_u64().unwrap_or(0)+account["deployed"].as_u64().unwrap_or(0)).to_string()])}</small></span></div>}})}</section>
            <div class="tactical-grid"><section class="tactical-map card"><div class="tactical-map-heading"><h3>{i18n::t(l,"sit_map")}</h3>if !selection.is_empty(){<span class="selection-chip">{i18n::tf(l,"selection",&[&selection])}</span>}</div><p id="map-status" class={classes!("observation-status",(!self.state["map"]["last_observation_clock"].is_string()).then_some("unknown"))}>{if let Some(clock)=self.state["map"]["last_observation_clock"].as_str(){let age=(self.state["time_s"].as_i64().unwrap_or(0)-self.state["map"]["last_observation_s"].as_i64().unwrap_or(0))/60;let source=self.state["map"]["last_observation_source"].as_str().map(String::from).unwrap_or_else(||self.name("master"));let live=list(&self.state["map"]["live_zones"]).iter().map(text).collect::<Vec<_>>().join(", ");format!("{}{}{}",i18n::tf(l,"map_updated",&[clock,&source]),if age>0{i18n::tf(l,"map_age",&[&age.to_string()])}else{String::new()},if live.is_empty(){String::new()}else{i18n::tf(l,"map_drone_live",&[&live])})}else{i18n::t(l,"no_map_data").into()}}</p>
                if self.maps{<SituationMap id="player-map" stat={self.stat.clone()} view={view.clone()} simple=true onclick={map_click.clone()} pending={pending.clone()} lang={l.to_string()}/>}<p class="map-instruction">{i18n::t(l,"map_hint")}</p>
                if !list(&self.state["map"]["forecast_products"]).is_empty(){<details><summary>{i18n::t(l,"forecasts")}</summary>{for list(&self.state["map"]["forecast_products"]).iter().map(|f|widgets::details(i18n::tf(l,"issued",&[&f["issued_s"].to_string()]),&json!({"assumptions":f["assumptions"],"horizons":f["horizons"]})))}</details>}
                if !list(&self.state["deployments"]).is_empty(){<details open=true class="tactical-missions"><summary>{i18n::t(l,"missions")}</summary>{for list(&self.state["deployments"]).iter().map(|d|{let drone=list(&self.state["map"]["drones"]).iter().find(|x|x["task"]==d["task"]["id"]).cloned();let detail=match &drone{Some(x)=>i18n::tf(l,"mission_drone_detail",&[&list(&x["zones"]).iter().map(|z|text(z).to_string()).collect::<Vec<_>>().join(", ")]),None=>text(&d["task"]["detail"]).to_string()};html!{<p><b>{resource_name(text(&d["task"]["resource"]))}</b>{format!(" · {}",i18n::mission_status(l,text(&d["task"]["status"])))}<br/><small>{detail}</small></p>}})}</details>}
            </section>
            <section class="tactical-timeline card" id="player-timeline"><header><div><h3>{i18n::t(l,"injects_replies")}</h3><small>{i18n::t(l,"own_events")}</small></div><label>{i18n::t(l,"turn_label")}<select aria-label={i18n::t(l,"player_turn")} value={turn} onchange={self.cb(|e|Msg::Field("work:turn".into(),api::select(e)))}>{for turns.iter().rev().map(|t|html!{<option value={t.to_string()} selected={*t==current_turn}>{t}</option>})}</select></label></header>
                {self.view_inject_panel(can_act,current_turn)}
            </section></div>
        </main>
            if self.field("work:open","false")=="true"{<crate::inject_panel::ModalShell class={classes!("modal-backdrop","work-backdrop")} onkeydown={{let link=self.link.clone();let busy=self.busy;Callback::from(move |e:KeyboardEvent|{if e.key()=="Escape"&&!busy{e.prevent_default();link.emit(Msg::Field("work:open".into(),"false".into()));}})}}><section class="work-modal" role="dialog" aria-modal="true" aria-labelledby="work-modal-title"><header><h2 id="work-modal-title">{i18n::t(l,"respond_title")}</h2><button aria-label={i18n::t(l,"close_reply")} disabled={self.busy} onclick={self.cb(|_|Msg::Field("work:open".into(),"false".into()))}>{i18n::t(l,"close")}</button></header>
                if let Some(message)=selected_context{<div class="work-context"><strong>{if message["kind"]=="inject"{text(&message["title"]).to_string()}else{self.name(text(&message["from"]))}}</strong><small>{text(&message["clock"])}</small><p>{text(if message["from"]==own && message["original"].is_string(){&message["original"]}else{&message["text"]})}</p>if message["kind"]=="inject"{<small>{format!("{}: {}",i18n::t(l,"reply_count"),response_count)}</small>}</div>}
                if let Some(question)=clarifying_proposal.as_ref().and_then(|p|p["clarification_request"].as_str()){<div class="work-clarification" role="note" data-work-clarification={clarifying_proposal.as_ref().map(|p|p["id"].to_string()).unwrap_or_default()}><b>{format!("{} · {}",asker,i18n::t(l,"clarif_needed"))}</b><p>{question}</p>if let Some(p)=clarifying_proposal.as_ref(){<small class="muted">{text(&p["original"])}</small>}</div>}
                <p class="work-approval-note">{i18n::t(l,"response_note")}</p>
                if !self.error.is_empty(){<p class="error" role="alert">{&self.error}</p>}
                <div class="work-modal-layout"><div class="work-modal-map"><p>{i18n::t(l,"select_map_target")}</p>
                    if work_action=="CutFireLine" {<div class="endpoint-picker"><button aria-pressed={(self.field("work:target_field","work:location")=="work:location").to_string()} onclick={self.cb(|_|Msg::Field("work:target_field".into(),"work:location".into()))}>{i18n::t(l,"first_point")}</button><button aria-pressed={(self.field("work:target_field","work:location")=="work:destination").to_string()} onclick={self.cb(|_|Msg::Field("work:target_field".into(),"work:destination".into()))}>{i18n::t(l,"second_point")}</button></div>}
                    if self.maps{<SituationMap id="work-map" stat={self.stat.clone()} view={view.clone()} simple=true onclick={map_click} pending={pending.clone()} lang={l.to_string()}/>}</div><div class="work-modal-form">{work_editor}</div></div>
            </section></crate::inject_panel::ModalShell>}
        </>}
    }
}

impl crate::player_view::PlayerView {
    /// Actions the AI suggests from the narrative being written (player-known
    /// context only; see `/api/player/<role>/suggest-actions`), to add by hand.
    fn view_suggestions(&self, l: &str, staged: &[Value]) -> Html {
        let working = !self.field("work:sugg_busy", "").is_empty();
        let result: Value = serde_json::from_str(&self.field("work:sugg", "null")).unwrap_or_default();
        let stale = !result.is_null() && self.field("work:sugg_for", "") != self.field("work:narrative", "");
        if !working && result.is_null() {
            return html! {};
        }
        let open = fresh_suggestions(&result, staged);
        html! {<div class={classes!("work-suggestions",stale.then_some("is-stale"))} data-work-suggestions={text(&result["status"]).to_string()} aria-live="polite">
            <b>{i18n::t(l,"sugg_title")}</b>
            if working {<p class="muted" data-suggestions-working="">{i18n::t(l,"sugg_working")}</p>}
            if result["status"]=="ai_unavailable" {<p class="muted">{i18n::t(l,"sugg_unavailable")}</p>}
            else if !result.is_null() && open.is_empty() && !working {<p class="muted">{i18n::t(l,"sugg_none")}</p>}
            if !open.is_empty() {
                <p class="work-help">{i18n::t(l,"sugg_hint")}</p>
                <ol class="suggested-actions">{for open.iter().enumerate().map(|(i,a)|{
                    let cmd=a["command"].clone();
                    html!{<li data-suggested-action={i.to_string()}>
                        <span>{widgets::command_summary(l,&cmd)}</span>
                        if !text(&a["reason"]).is_empty(){<small class="muted">{text(&a["reason"])}</small>}
                        if a["verdict"]["verdict"]=="rejected"{<small class="danger">{i18n::tf(l,"sugg_check",&[text(&a["verdict"]["reason"])])}</small>}
                        <button type="button" data-add-suggestion={i.to_string()} disabled={self.busy} onclick={self.cb(move |_|Msg::StageActions(vec![cmd.clone()]))}>{i18n::t(l,"sugg_add")}</button>
                    </li>}
                })}</ol>
            }
        </div>}
    }
}

/// Suggested actions not already in the response being composed.
fn fresh_suggestions(result: &Value, staged: &[Value]) -> Vec<Value> {
    list(&result["actions"]).iter().filter(|a| !staged.contains(&a["command"])).cloned().collect()
}

#[cfg(test)]
mod place_tests {
    use super::*;

    #[test]
    fn building_names_are_not_zone_codes() {
        let stat = json!({"zones":{"cols":8,"rows":8},"buildings":[{"id":1,"name":"Pineta Primary School"},{"id":2,"name":""}]});
        assert!(is_zone_code(&stat, "f4") && is_zone_code(&stat, "F 4") && !is_zone_code(&stat, "I1") && !is_zone_code(&stat, "F9"));
        assert!(!is_zone_code(&stat, "Pineta Primary School") && !is_zone_code(&stat, "Pineta"));
        assert_eq!(building_named(&stat, "pineta primary-school").as_deref(), Some("Pineta Primary School"));
        assert_eq!(building_named(&stat, "Pineta"), None);
        assert_eq!(building_named(&stat, ""), None);
        assert_ne!(place_kind("EvacuateZone"), place_kind("EvacuateBuilding"));
    }

    #[test]
    fn staged_suggestions_are_not_offered_again() {
        let warn = json!({"type":"IssueWarning","zone":"F4"});
        let close = json!({"type":"CloseRoad","road":"r1"});
        let result = json!({"status":"ready","actions":[{"command":warn},{"command":close}]});
        let open = fresh_suggestions(&result, &[warn.clone()]);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0]["command"], close);
        assert!(fresh_suggestions(&Value::Null, &[]).is_empty());
    }
}
