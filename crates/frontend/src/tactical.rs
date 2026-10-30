use crate::{
    api::{self, list, text},
    i18n,
    map::SituationMap,
    widgets, Msg,
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
    /// How to reply so the exercise control can act on it, under the reply
    /// box: what, where, with which resources, by when, who is informed, what
    /// it rests on (plan, procedure, law or operational judgement) and why
    /// (rationale); then what
    /// the role decides with (competences, resources).
    pub fn reply_guidance(&self, l: &str, compact: bool) -> Html {
        let tips = [
            ("guide_what", "guide_what_hint"),
            ("guide_where", "guide_where_hint"),
            ("guide_with", "guide_with_hint"),
            ("guide_when", "guide_when_hint"),
            ("guide_who", "guide_who_hint"),
            ("guide_basis", "guide_basis_hint"),
            ("guide_why", "guide_why_hint"),
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
        // A map click writes the place into the open reply (zone code or building name).
        let map_click = {
            let link = self.link.clone();
            let target = self.active_reply_ctx().map(|ctx| crate::reply_composer::draft_key(&ctx));
            let current = target.as_ref().map(|k| self.field(k, "")).unwrap_or_default();
            Callback::from(move |point: Value| {
                if let Some(key) = &target {
                    link.emit(Msg::Field(key.clone(), append_place(&current, &place_words(&point))));
                }
            })
        };
        let view = crate::map::player_view(&self.state["map"], &self.state["deployments"]);
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

    /// Only the EXCON acts on the world: the player's page has no action
    /// form, no communication/decision choice and no AI action suggestions;
    /// the reply composer (`reply_composer`) is words plus their basis.
    #[test]
    fn the_player_page_has_no_action_controls() {
        let src = include_str!("tactical.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        for forbidden in ["action_pick::form(", "data-add-action", "name=\"work-kind\"", "suggest-actions", "StageActions", "\"commands\"", "work-modal"] {
            assert!(!body.contains(forbidden), "player page still has {forbidden}");
        }
        assert!(body.contains("guide_basis"), "the guide asks for plans, procedures or operational judgement");
        assert!(body.contains("guide_why"), "the guide asks for the rationale");
    }
}
