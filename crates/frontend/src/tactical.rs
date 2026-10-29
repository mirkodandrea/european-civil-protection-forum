use crate::{
    api::{self, list, text},
    i18n,
    map::SituationMap,
    widgets, Action, ActionKind, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;

/// Where a map click points, as words for the reply: a named building, else
/// the zone code (`F4`), else the coordinates.
pub fn place_words(point: &Value) -> String {
    let building = text(&point["building"]);
    let zone = text(&point["zone"]);
    if !building.is_empty() {
        building.to_string()
    } else if !zone.is_empty() {
        zone.to_string()
    } else {
        format!("{:.0},{:.0}", point["x"].as_f64().unwrap_or(0.0), point["y"].as_f64().unwrap_or(0.0))
    }
}

/// `text` with `place` appended, separated as a writer would.
pub fn append_place(text: &str, place: &str) -> String {
    let t = text.trim_end();
    if t.is_empty() {
        place.to_string()
    } else {
        format!("{t} {place}")
    }
}

impl crate::player_view::PlayerView {
    /// How to reply so the exercise control can act on it, next to the reply
    /// box: what, where, with which resources, by when, who is informed; then
    /// what the role decides with (competences, resources).
    pub fn reply_guidance(&self, l: &str, compact: bool) -> Html {
        let tips = [
            ("guide_what", "guide_what_hint"),
            ("guide_where", "guide_where_hint"),
            ("guide_with", "guide_with_hint"),
            ("guide_when", "guide_when_hint"),
            ("guide_who", "guide_who_hint"),
        ];
        let resource_name = |id: &str| {
            list(&self.stat["resource_types"]).iter().find(|r| r["id"] == id).map(|r| text(&r["name"]).to_string()).unwrap_or(id.into())
        };
        let resources: Vec<String> = self.state["resource_accounting"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, a)| format!("{} {}", a["available"], resource_name(id)))
            .collect();
        let allowed = list(&self.state["role"]["allowed_actions"]);
        if compact {
            return html! {<p class="reply-guide-line" data-reply-guide="compact">{i18n::t(l,"guide_compact")}</p>};
        }
        html! {<aside class="reply-guide" data-reply-guide="" aria-label={i18n::t(l,"guide_title")}>
            <h4>{i18n::t(l,"guide_title")}</h4>
            <ul class="reply-guide-list">{for tips.iter().map(|(k,h)|html!{<li><b>{i18n::t(l,k)}</b><span>{i18n::t(l,h)}</span></li>})}</ul>
            <p class="reply-guide-example"><small>{i18n::t(l,"guide_example")}</small></p>
            <h4>{i18n::t(l,"guide_support")}</h4>
            <dl class="reply-guide-support">
                <dt>{i18n::t(l,"guide_competences")}</dt>
                <dd>{if allowed.is_empty(){i18n::t(l,"function_no_actions").to_string()}else{allowed.iter().map(|a|i18n::action(l,text(a)).to_string()).collect::<Vec<_>>().join(" · ")}}</dd>
                <dt>{i18n::t(l,"guide_resources")}</dt>
                <dd>{if resources.is_empty(){i18n::t(l,"function_no_resources").to_string()}else{resources.join(" · ")}}</dd>
            </dl>
            <small class="muted">{i18n::t(l,"guide_more")}</small>
        </aside>}
    }

    pub fn view_tactical(&self) -> Html {
        let own = self.own.clone();
        let l = self.lang();
        let l = l.as_str();
        let work_reply_to = self.field("work:reply_to", "");
        let work_conversation = self.field("work:conversation", "").parse::<u64>().ok();
        let narrative_text = self.field("work:narrative", "");
        // A map click writes the place into the reply (zone code or building name).
        let map_click = {
            let link = self.link.clone();
            let current = narrative_text.clone();
            let open = self.field("work:open", "false") == "true";
            Callback::from(move |point: Value| {
                if open {
                    link.emit(Msg::Field("work:narrative".into(), append_place(&current, &place_words(&point))));
                }
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
        // Recipient: the fictitious sender of the inject; only a reply from a conversation
        // can instead go to its participants (talking to other players is a conversation).
        let to_players_mode = work_conversation.is_some() && self.field("work:to", "persona") == "players";
        let to_players: Vec<String> = self.field("work:to_players", "").split(',').filter(|p| !p.is_empty()).map(String::from).collect();
        let sender_name = selected_context
            .map(|m| if m["persona"].is_string() { text(&m["persona"]).to_string() } else { self.name(text(&m["from"])) })
            .unwrap_or_default();
        // From a conversation only its participants can be addressed.
        let player_choices: Vec<(String, String)> = match work_conversation.and_then(|cid| list(&self.state["linked_conversations"]).iter().find(|c| c["id"].as_u64() == Some(cid))) {
            Some(c) => list(&c["participants"]).iter().map(text).filter(|p| *p != own).map(|p| (p.to_string(), self.name(p))).collect(),
            None => self.contacts().iter().filter(|r| r["id"] != "master").map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string())).collect(),
        };
        // What the disabled "send" button is waiting for.
        let respond_missing = if self.state["phase"] != "orders" { Some("work_need_orders") }
            else if !answers_inject { Some("work_need_inject") }
            else if !has_narrative { Some("need_text") }
            else if to_players_mode && to_players.is_empty() { Some("need_players") }
            else { None };
        // Players reply in words only: the exercise control decides what happens.
        let respond = {
            let own = own.clone();
            let narrative = narrative_text.clone();
            let parent = work_reply_to.parse::<u64>().ok();
            let players = to_players_mode.then(|| to_players.clone());
            self.cb(move |_| {
                let mut body = json!({"kind":"communication","narrative":narrative,"reply_to":parent,"conversation":work_conversation});
                if let Some(players) = &players { body["to_players"] = json!(players); }
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
            if work_conversation.is_some() {<div class="work-block" data-work-block="recipient">
                <h4 class="work-section">{i18n::t(l,"resp_to")}</h4>
                <div class="basis-options" role="radiogroup" aria-label={i18n::t(l,"resp_to")}>
                    <label class={classes!("basis-option",(!to_players_mode).then_some("is-selected"))}><input type="radio" name="work-to" id="to-sender" data-to="persona" checked={!to_players_mode} onchange={self.cb(|_|Msg::Field("work:to".into(),"persona".into()))}/><span>{i18n::tf(l,"resp_to_sender",&[&sender_name])}</span></label>
                    <label class={classes!("basis-option",to_players_mode.then_some("is-selected"))}><input type="radio" name="work-to" id="to-players" data-to="players" checked={to_players_mode} disabled={player_choices.is_empty()} onchange={self.cb(|_|Msg::Field("work:to".into(),"players".into()))}/><span>{i18n::t(l,"resp_to_players")}</span></label>
                </div>
                if to_players_mode {<div class="work-players">{for player_choices.iter().map(|(id,name)|{let on=to_players.contains(id);html!{<label class="mt-check" key={id.clone()}><input type="checkbox" data-to-player={id.clone()} checked={on} onchange={pick_players(id.clone(),on)}/>{name.clone()}</label>}})}</div>}
            </div>}
            <div class="work-block reply-block" data-work-block="narrative">
                <div class="reply-write">
                    <h4 class="work-section"><label for="response-narrative">{i18n::t(l,"reply_text_label")}</label></h4>
                    <p class="work-help" data-narrative-help="">{i18n::t(l,"reply_text_help")}</p>
                    <textarea id="response-narrative" data-response-narrative="" maxlength="8000" placeholder={i18n::t(l,"reply_ph")} value={narrative_text.clone()} oninput={self.cb(|e|Msg::Field("work:narrative".into(),api::input(e)))}/>
                    <small class="muted">{i18n::t(l,"map_inserts_place")}</small>
                </div>
                {self.reply_guidance(l, false)}
            </div>
            <div class="work-submit-bar">
                <button class="primary" id="respond-submit" data-respond="" disabled={self.busy||respond_missing.is_some()} onclick={respond}>{i18n::t(l,"send_response")}</button>
                if let Some(missing)=respond_missing{<small class="muted work-hint" role="status">{i18n::t(l,missing)}</small>}else{<small class="muted work-hint">{i18n::t(l,"reply_hint")}</small>}
            </div>
        </section>};
        let mut view = self.state["map"].clone();
        view["deployments"] = self.state["deployments"].clone();
        let selection = String::new();
        let (map_zones, pending): (Vec<String>, Option<Value>) = (Vec::new(), None);
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
                if self.maps{<SituationMap id="player-map" stat={self.stat.clone()} view={view.clone()} simple=true onclick={map_click.clone()} pending={pending.clone()} selected_zones={map_zones.clone()} lang={l.to_string()}/>}<p class="map-instruction">{i18n::t(l,"map_hint_reply")}</p>
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
                <p class="work-approval-note">{i18n::t(l,"reply_note")}</p>
                if !self.error.is_empty(){<p class="error" role="alert">{&self.error}</p>}
                <div class="work-modal-layout"><div class="work-modal-map"><p>{i18n::t(l,"map_inserts_place")}</p>
                    if self.maps{<SituationMap id="work-map" stat={self.stat.clone()} view={view.clone()} simple=true onclick={map_click} pending={pending.clone()} selected_zones={map_zones.clone()} lang={l.to_string()}/>}</div><div class="work-modal-form">{work_editor}</div></div>
            </section></crate::inject_panel::ModalShell>}
        </>}
    }
}

#[cfg(test)]
mod place_tests {
    use super::*;

    #[test]
    fn building_names_are_not_zone_codes() {
        let stat = json!({"zones":{"cols":8,"rows":8},"buildings":[{"id":1,"name":"Pineta Primary School"},{"id":2,"name":""}]});
        assert!(crate::place_pick::parse_zone(&stat, "f4").is_some() && crate::place_pick::parse_zone(&stat, "F 4").is_some() && !crate::place_pick::parse_zone(&stat, "I1").is_some() && !crate::place_pick::parse_zone(&stat, "F9").is_some());
        assert!(!crate::place_pick::parse_zone(&stat, "Pineta Primary School").is_some() && !crate::place_pick::parse_zone(&stat, "Pineta").is_some());
        assert_eq!(crate::action_pick::building_named(&stat, "pineta primary-school").as_deref(), Some("Pineta Primary School"));
        assert_eq!(crate::action_pick::building_named(&stat, "Pineta"), None);
        assert_eq!(crate::action_pick::building_named(&stat, ""), None);
        assert_ne!(crate::place_pick::pick_for("EvacuateZone"), crate::place_pick::pick_for("EvacuateBuilding"));
    }

    #[test]
    fn a_map_click_writes_the_place_into_the_reply() {
        assert_eq!(place_words(&json!({"zone":"F4","x":2750.0,"y":2250.0})), "F4");
        assert_eq!(place_words(&json!({"zone":"F4","building":"Valverde Sports Centre"})), "Valverde Sports Centre");
        assert_eq!(place_words(&json!({"x":10.4,"y":20.6})), "10,21");
        assert_eq!(append_place("Mandiamo le squadre in ", "F4"), "Mandiamo le squadre in F4");
        assert_eq!(append_place("", "F4"), "F4");
    }

    /// Only the EXCON acts on the world: the player's composer has no action
    /// form, no communication/decision choice, no basis, no AI action
    /// suggestions; a reply is words only.
    #[test]
    fn the_player_page_has_no_action_controls() {
        let src = include_str!("tactical.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        for forbidden in ["action_pick::form(", "data-add-action", "name=\"work-kind\"", "work-basis", "suggest-actions", "StageActions", "\"commands\""] {
            assert!(!body.contains(forbidden), "player composer still has {forbidden}");
        }
        assert!(body.contains(r#"json!({"kind":"communication","narrative""#));
        assert!(body.contains("reply_guidance(l, false)"), "the reply guidance sits next to the reply box");
    }
}
