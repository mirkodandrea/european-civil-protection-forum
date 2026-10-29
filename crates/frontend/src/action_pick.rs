//! The player's action form (kind, place, second point, resource), shared by
//! the player's response composer and the EXCON review of a player action.
//!
//! The form keeps no state: it reads a [`Draft`] and reports changes as field
//! updates (`"work:location"` → value), the same protocol as
//! [`place_pick::click`]. The player page stores them in its fields; the EXCON
//! [`ActionPickModal`] in a local map.
use crate::{
    api::{self, list, text},
    i18n,
    place_pick,
};
use serde_json::Value;
use std::collections::BTreeMap;
use yew::prelude::*;

/// Field updates sent by the form.
pub type Set = Callback<Vec<(&'static str, String)>>;

/// Letters and digits only, lower case: "Pineta Primary-School" == "pineta primary school".
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// The facility called `name` (case and punctuation ignored), if any.
pub fn building_named(stat: &Value, name: &str) -> Option<String> {
    let k = norm(name);
    (!k.is_empty()).then_some(())?;
    list(&stat["buildings"]).iter().map(|b| text(&b["name"])).find(|n| norm(n) == k).map(str::to_string)
}

/// What the form holds, as typed (see the `work:*` keys).
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Draft {
    pub action: String,
    pub location: String,
    pub destination: String,
    pub resource: String,
    pub qty: String,
    pub zone_mode: String,
    pub anchor: String,
    pub target_field: String,
    pub notice: String,
}

impl Draft {
    /// The draft stored under the `work:*` keys of `get(key, default)`.
    pub fn read(get: impl Fn(&str, &str) -> String) -> Self {
        Draft {
            action: get("work:action", ""),
            location: get("work:location", ""),
            destination: get("work:destination", ""),
            resource: get("work:resource", ""),
            qty: get("work:qty", "1"),
            zone_mode: get("work:zone_mode", "cells"),
            anchor: get("work:anchor", ""),
            target_field: get("work:target_field", "work:location"),
            notice: get("work:notice", ""),
        }
    }

    /// The fields that rebuild `cmd` (a player command) in the form.
    pub fn from_command(stat: &Value, cmd: &Value) -> Self {
        let loc = |v: &Value| match (v["x"].as_f64(), v["y"].as_f64()) {
            (Some(x), Some(y)) => format!("{x},{y}"),
            _ => text(v).to_string(),
        };
        let mut d = Draft { action: text(&cmd["type"]).to_string(), qty: "1".into(), zone_mode: "cells".into(), target_field: "work:location".into(), ..Default::default() };
        match d.action.as_str() {
            "IssueWarning" | "EvacuateZone" | "Reconnoitre" => d.location = text(&cmd["zone"]).to_string(),
            "MonitorArea" => d.location = list(&cmd["zones"]).iter().map(text).collect::<Vec<_>>().join(", "),
            "EvacuateBuilding" | "OpenShelter" => d.location = place_pick::building_label(stat, text(&cmd["building"])),
            "CloseRoad" | "ReopenRoad" => {
                // The form lists roads by id; a command may name the road instead.
                let r = text(&cmd["road"]);
                d.location = list(&stat["roads"]).iter().find(|x| text(&x["id"]) == r || norm(text(&x["name"])) == norm(r)).map(|x| text(&x["id"]).to_string()).unwrap_or_else(|| r.to_string());
            }
            "WaterDrop" | "DeployEngine" => d.location = loc(&cmd["at"]),
            "CutFireLine" if cmd["along"].is_string() => d.location = text(&cmd["along"]).to_string(),
            "CutFireLine" => {
                d.location = loc(&cmd["from"]);
                d.destination = loc(&cmd["to"]);
            }
            "RequestReinforcement" => {
                d.resource = text(&cmd["resource"]).to_string();
                d.qty = cmd["qty"].as_u64().unwrap_or(1).to_string();
            }
            _ => {}
        }
        d
    }

    /// The draft as `work:*` field values.
    pub fn fields(&self) -> BTreeMap<String, String> {
        [
            ("work:action", &self.action),
            ("work:location", &self.location),
            ("work:destination", &self.destination),
            ("work:resource", &self.resource),
            ("work:qty", &self.qty),
            ("work:zone_mode", &self.zone_mode),
            ("work:anchor", &self.anchor),
            ("work:target_field", &self.target_field),
            ("work:notice", &self.notice),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
    }
}

/// The draft checked against the role and the scenario: what the form shows,
/// what the map highlights and the commands it makes.
#[derive(Clone, PartialEq, Debug)]
pub struct Resolved {
    pub allowed: Vec<String>,
    pub action: String,
    pub pick: &'static str,
    pub location: String,
    pub resource: String,
    pub qty: u32,
    pub commands: Vec<Value>,
    pub selected_zones: Vec<String>,
    /// What the form is still waiting for (an `i18n` key).
    pub missing: Option<&'static str>,
}

pub fn resolve(stat: &Value, allowed: &[String], d: &Draft) -> Resolved {
    // A stale choice from another role or scenario falls back to the first one.
    let action = if allowed.contains(&d.action) { d.action.clone() } else { allowed.first().cloned().unwrap_or_default() };
    let pick = place_pick::pick_for(&action);
    let options = location_options(stat, &action);
    let location = if !options.is_empty() && !options.contains(&d.location) { String::new() } else { d.location.clone() };
    let resource = if d.resource.is_empty() {
        list(&stat["resource_types"]).first().map(|r| text(&r["id"])).unwrap_or("ground_crews").to_string()
    } else {
        d.resource.clone()
    };
    let qty = d.qty.parse::<u32>().unwrap_or(1).max(1);
    let commands = place_pick::commands(stat, &action, &location, &d.destination, &resource, qty);
    let missing = if allowed.is_empty() {
        Some("work_need_action")
    } else if pick != "none" && location.trim().is_empty() {
        Some("work_need_place")
    } else if pick == "line" && commands.is_empty() {
        Some("work_need_second")
    } else if commands.is_empty() {
        Some("work_need_place")
    } else {
        None
    };
    let selected_zones = if pick == "zones" { place_pick::zones_of(stat, &location) } else { Vec::new() };
    Resolved { allowed: allowed.to_vec(), action, pick, location, resource, qty, commands, selected_zones, missing }
}

/// Roads and buildings are picked from a list; other places are typed.
fn location_options(stat: &Value, action: &str) -> Vec<String> {
    if ["CloseRoad", "ReopenRoad"].contains(&action) {
        list(&stat["roads"]).iter().map(|r| text(&r["id"]).to_string()).collect()
    } else if ["EvacuateBuilding", "OpenShelter"].contains(&action) {
        list(&stat["buildings"])
            .iter()
            .map(|b| if text(&b["name"]).is_empty() { format!("#{}", b["id"]) } else { text(&b["name"]).to_string() })
            .collect()
    } else {
        Vec::new()
    }
}

/// Field updates for a map click (the clicked point fills the right field).
pub fn map_click(stat: &Value, r: &Resolved, d: &Draft, set: &Set) -> Callback<Value> {
    let (stat, action, location, target, mode, anchor, set) =
        (stat.clone(), r.action.clone(), d.location.clone(), d.target_field.clone(), d.zone_mode.clone(), d.anchor.clone(), set.clone());
    Callback::from(move |point: Value| {
        let mut v = place_pick::click(&stat, &action, &point, &location, &target, &mode, &anchor);
        v.push(("work:notice", String::new()));
        set.emit(v);
    })
}

/// What the map highlights: selected zones and points/line.
pub fn map_preview(stat: &Value, r: &Resolved, d: &Draft) -> (Vec<String>, Option<Value>) {
    place_pick::preview(stat, &r.action, &r.location, &d.destination, &d.anchor)
}

/// For a line: which end the next map click sets.
pub fn endpoint_picker(l: &str, r: &Resolved, d: &Draft, set: &Set) -> Html {
    if r.pick != "line" {
        return html! {};
    }
    let to = |f: &'static str| {
        let set = set.clone();
        Callback::from(move |_: MouseEvent| set.emit(vec![("work:target_field", f.to_string())]))
    };
    html! {<div class="endpoint-picker"><button aria-pressed={(d.target_field=="work:location").to_string()} onclick={to("work:location")}>{i18n::t(l,"first_point")}</button><button aria-pressed={(d.target_field=="work:destination").to_string()} onclick={to("work:destination")}>{i18n::t(l,"second_point")}</button></div>}
}

/// The form: action kind, place (list, zones, points), second point, resource.
/// `id` prefixes the element ids; `after_action` goes under the kind select.
pub fn form(l: &str, id: &str, stat: &Value, r: &Resolved, d: &Draft, set: &Set, after_action: Html) -> Html {
    let field = |f: &'static str| {
        let set = set.clone();
        move |v: String| set.emit(vec![(f, v)])
    };
    let on_input = |f: &'static str| {
        let s = field(f);
        Callback::from(move |e: InputEvent| s(api::input(e)))
    };
    let on_select = |f: &'static str| {
        let s = field(f);
        Callback::from(move |e: Event| s(api::select(e)))
    };
    let on_action = {
        let (old, loc, set) = (r.action.clone(), r.location.clone(), set.clone());
        Callback::from(move |e: Event| {
            let new = api::select(e);
            let mut m = vec![("work:notice", String::new()), ("work:anchor", String::new())];
            if place_pick::pick_for(&new) != place_pick::pick_for(&old) && !loc.is_empty() {
                m.extend(["work:location", "work:destination"].map(|k| (k, String::new())));
                m.push(("work:target_field", "work:location".into()));
            }
            m.push(("work:action", new));
            set.emit(m)
        })
    };
    // Zone-type actions take grid codes, rectangles or named places; a building
    // typed there switches the action to the building one instead of being refused later.
    let zone_input = {
        let (stat, action, set) = (stat.clone(), r.action.clone(), set.clone());
        let building_allowed = r.allowed.iter().any(|a| a == "EvacuateBuilding");
        Callback::from(move |e: InputEvent| {
            let v = api::input(e);
            set.emit(match building_named(&stat, &v) {
                Some(name) if action == "EvacuateZone" && building_allowed => vec![
                    ("work:action", "EvacuateBuilding".into()),
                    ("work:location", name.clone()),
                    ("work:notice", name),
                ],
                _ => vec![("work:notice", String::new()), ("work:location", v)],
            })
        })
    };
    let zone_options: Vec<String> = if r.pick == "zones" {
        let (cols, rows) = (stat["zones"]["cols"].as_u64().unwrap_or(0) as u8, stat["zones"]["rows"].as_u64().unwrap_or(0) as u8);
        (1..=rows)
            .flat_map(|row| (0..cols).map(move |c| format!("{}{}", (b'A' + c) as char, row)))
            .chain(list(&stat["places"]).iter().map(|p| text(&p["name"]).to_string()).filter(|n| !n.is_empty()))
            .collect()
    } else {
        Vec::new()
    };
    let unknown = if r.pick == "zones" { place_pick::unknown_entries(stat, &r.location) } else { Vec::new() };
    let zone_notice = if !d.notice.is_empty() && r.pick == "building" {
        i18n::tf(l, "place_switched", &[&d.notice, i18n::action(l, "EvacuateBuilding")])
    } else if let Some(bad) = unknown.first() {
        if building_named(stat, bad).is_some() {
            i18n::tf(l, "place_is_building", &[bad.trim()])
        } else {
            i18n::tf(l, "place_not_zone", &[bad.trim()])
        }
    } else {
        String::new()
    };
    let order_count = r.commands.len();
    let (action, location) = (r.action.clone(), r.location.clone());
    let loc_id = format!("{id}-location");
    let zone_list = format!("{id}-zone-options");
    html! {<>
        <label for={format!("{id}-action")}>{i18n::t(l,"action_type")}</label>
        <select id={format!("{id}-action")} value={action.clone()} onchange={on_action}>
            {for r.allowed.iter().map(|v|html!{<option value={v.clone()} selected={*v==action}>{i18n::action(l,v)}</option>})}
        </select>
        {after_action}
        <label for={loc_id.clone()}>{format!("{} *",i18n::t(l,"place"))}</label>
        if action=="CloseRoad"||action=="ReopenRoad" {
            <select id={loc_id.clone()} required=true aria-required="true" value={location.clone()} onchange={on_select("work:location")}><option value="" selected={location.is_empty()}>{i18n::pair(l,"Seleziona sulla mappa o nell’elenco","Select on the map or from the list")}</option>{for list(&stat["roads"]).iter().map(|x|html!{<option value={text(&x["id"]).to_string()} selected={text(&x["id"])==location}>{text(&x["name"])}</option>})}</select>
        } else if action=="EvacuateBuilding"||action=="OpenShelter" {
            <select id={loc_id.clone()} required=true aria-required="true" value={location.clone()} onchange={on_select("work:location")}><option value="" selected={location.is_empty()}>{i18n::pair(l,"Seleziona sulla mappa o nell’elenco","Select on the map or from the list")}</option>{for list(&stat["buildings"]).iter().map(|b|{let bid=b["id"].as_u64().unwrap_or(0);let name=text(&b["name"]);let value=if name.is_empty(){format!("#{bid}")}else{name.to_string()};html!{<option value={value.clone()} selected={value==location}>{if name.is_empty(){value}else{name.to_string()}}</option>}})}</select>
        } else if r.pick=="zones" {
            <input id={loc_id.clone()} list={zone_list.clone()} required=true aria-required="true" placeholder="F4, C3-D4" value={location.clone()} oninput={zone_input}/>
            <datalist id={zone_list}>{for zone_options.iter().map(|z|html!{<option value={z.clone()}/>})}</datalist>
            {zone_tools(l, stat, d, &location, &r.selected_zones, set)}
            if order_count>1 && action!="MonitorArea" {<small class="muted work-hint" data-order-count={order_count.to_string()}>{i18n::tf(l,"pick_orders",&[&order_count.to_string()])}</small>}
        } else if r.pick=="line" {
            <input id={loc_id.clone()} required=true aria-required="true" placeholder={i18n::t(l,"first_point")} value={location.clone()} oninput={on_input("work:location")}/>
        } else {
            <input id={loc_id.clone()} required=true aria-required="true" value={location.clone()} oninput={on_input("work:location")}/>
        }
        if !zone_notice.is_empty() {<small class="muted work-hint" role="status" data-place-notice="">{zone_notice}</small>}
        if r.pick=="line" && !place_pick::is_zone_edge(stat,&location) {<label for={format!("{id}-destination")}>{i18n::t(l,"second_point")}</label><input id={format!("{id}-destination")} value={d.destination.clone()} oninput={on_input("work:destination")}/>}
        if r.pick=="line" {<small class="muted work-hint">{i18n::t(l,"pick_hint_line")}</small>}
        if r.pick=="point" {<small class="muted work-hint">{i18n::t(l,"pick_hint_point")}</small>}
        if action=="RequestReinforcement" {<label for={format!("{id}-resource")}>{i18n::t(l,"resource_req")}</label><select id={format!("{id}-resource")} value={r.resource.clone()} onchange={on_select("work:resource")}>{for list(&stat["resource_types"]).iter().map(|x|html!{<option value={text(&x["id"]).to_string()} selected={text(&x["id"])==r.resource}>{text(&x["name"])}</option>})}</select><label for={format!("{id}-qty")}>{i18n::t(l,"quantity")}</label><input id={format!("{id}-qty")} type="number" min="1" value={d.qty.clone()} oninput={on_input("work:qty")}/>}
    </>}
}

/// Tools to select several zones: one by one (cells), as a rectangle (two
/// corner clicks), or by a named place that expands to the cells it covers.
fn zone_tools(l: &str, stat: &Value, d: &Draft, location: &str, zones: &[String], set: &Set) -> Html {
    let (mode, anchor) = (d.zone_mode.as_str(), d.anchor.as_str());
    let set_mode = |m: &'static str| {
        let set = set.clone();
        Callback::from(move |_: MouseEvent| set.emit(vec![("work:zone_mode", m.into()), ("work:anchor", String::new())]))
    };
    let add_place = {
        let (stat, location, set) = (stat.clone(), location.to_string(), set.clone());
        Callback::from(move |e: Event| {
            use yew::TargetCast;
            // The picker adds and goes back to its prompt.
            let el = e.target_unchecked_into::<web_sys::HtmlSelectElement>();
            let name = el.value();
            el.set_value("");
            let cells = place_pick::place_cells(&stat, &name);
            if cells.is_empty() {
                return;
            }
            let all = place_pick::add_all(&place_pick::zones_of(&stat, &location), &cells);
            set.emit(vec![("work:location", all.join(", ")), ("work:notice", String::new())])
        })
    };
    let clear = {
        let set = set.clone();
        Callback::from(move |_: MouseEvent| set.emit(vec![("work:location", String::new()), ("work:anchor", String::new())]))
    };
    let places: Vec<String> = list(&stat["places"]).iter().map(|p| text(&p["name"]).to_string()).filter(|n| !n.is_empty()).collect();
    let hint = if mode == "rect" && !anchor.is_empty() { i18n::tf(l, "pick_hint_corner", &[anchor]) } else { i18n::t(l, "pick_hint_zones").to_string() };
    html! {<div class="zone-tools" data-zone-tools={mode.to_string()}>
        <div class="zone-tool-row" role="group" aria-label={i18n::t(l, "place")}>
            <button type="button" aria-pressed={(mode != "rect").to_string()} data-zone-mode="cells" onclick={set_mode("cells")}>{i18n::t(l, "pick_cells")}</button>
            <button type="button" aria-pressed={(mode == "rect").to_string()} data-zone-mode="rect" onclick={set_mode("rect")}>{i18n::t(l, "pick_rect")}</button>
            if !places.is_empty() {
                <select aria-label={i18n::t(l, "pick_add_place")} data-zone-place="" value="" onchange={add_place}>
                    <option value="" selected=true>{i18n::t(l, "pick_add_place")}</option>
                    {for places.iter().map(|p| html!{<option value={p.clone()}>{p.clone()}</option>})}
                </select>
            }
            <button type="button" data-zone-clear="" disabled={zones.is_empty() && anchor.is_empty()} onclick={clear}>{i18n::t(l, "pick_clear")}</button>
        </div>
        if !zones.is_empty() {<div class="zone-chips" data-selected-zones={zones.join(",")}>{for zones.iter().map(|z| {
            let (z2, stat, location, set) = (z.clone(), stat.clone(), location.to_string(), set.clone());
            html!{<button type="button" class="chip" title={i18n::t(l, "remove")} onclick={Callback::from(move |_: MouseEvent| set.emit(vec![("work:location", place_pick::toggle(&place_pick::zones_of(&stat, &location), &z2).join(", "))]))}>{format!("{z} ×")}</button>}
        })}</div>}
        <small class="muted work-hint">{hint}</small>
    </div>}
}

/// Player action kinds the form builds.
pub const PLAYER_ACTIONS: [&str; 12] =
    ["CutFireLine", "WaterDrop", "DeployEngine", "MonitorArea", "Reconnoitre", "EvacuateZone", "EvacuateBuilding", "IssueWarning", "CloseRoad", "ReopenRoad", "OpenShelter", "RequestReinforcement"];

/// The actions a role may propose, else every player action kind.
pub fn role_actions(stat: &Value, role: &str) -> Vec<String> {
    let own = list(&stat["roles"]).iter().find(|r| text(&r["id"]) == role).map(place_pick::menu_actions).unwrap_or_default();
    if own.is_empty() {
        PLAYER_ACTIONS.map(String::from).to_vec()
    } else {
        own
    }
}

#[derive(Properties, PartialEq)]
pub struct ActionPickProps {
    /// The command being edited.
    pub command: Value,
    /// Role whose action kinds the menu offers.
    pub role: AttrValue,
    pub common: crate::controllo::CommonProps,
    /// The commands that replace the edited one (several zones = several orders).
    pub oncommands: Callback<Vec<Value>>,
}

/// EXCON edits a player action with the player's own form and map.
#[function_component(ActionPickModal)]
pub fn action_pick_modal(p: &ActionPickProps) -> Html {
    let c = &p.common;
    let lang = if crate::i18n_excon::is_en() { "en" } else { "it" };
    let fields = use_state(|| Draft::from_command(&c.stat, &p.command).fields());
    let d = Draft::read(|k, def| fields.get(k).cloned().unwrap_or_else(|| def.to_string()));
    let mut allowed = role_actions(&c.stat, &p.role);
    // The edited kind stays available even when the role's menu lacks it.
    let own = text(&p.command["type"]).to_string();
    if PLAYER_ACTIONS.contains(&own.as_str()) && !allowed.contains(&own) {
        allowed.insert(0, own);
    }
    let r = resolve(&c.stat, &allowed, &d);
    let set: Set = {
        let fields = fields.clone();
        Callback::from(move |v: Vec<(&'static str, String)>| {
            let mut next = (*fields).clone();
            for (k, x) in v {
                next.insert(k.to_string(), x);
            }
            fields.set(next);
        })
    };
    let (zones, pending) = map_preview(&c.stat, &r, &d);
    let map = html! {<>
        {endpoint_picker(lang, &r, &d, &set)}
        <crate::map::SituationMap id="excon-action-map" stat={c.stat.clone()} view={c.state["world"].clone()} onclick={map_click(&c.stat, &r, &d, &set)} {pending} selected_zones={zones}/>
    </>};
    let onapply = {
        let (cb, commands, onclose) = (p.oncommands.clone(), r.commands.clone(), c.onclose.clone());
        Callback::from(move |_| {
            cb.emit(commands.clone());
            onclose.emit(());
        })
    };
    html! {<crate::controllo::Modal title={i18n::action(lang, &r.action).to_string()} onclose={c.onclose.clone()} {onapply} apply_disabled={r.missing.is_some()} busy={c.busy} apply_label={crate::i18n::ex("Usa questa azione")} map={Some(map)}>
        <div class="excon-action-form" data-excon-action-form="">
            {form(lang, "excon-work", &c.stat, &r, &d, &set, Html::default())}
            if let Some(m) = r.missing {<small class="muted work-hint" role="status">{i18n::t(lang, m)}</small>}
            else if r.commands.len() > 1 {<small class="muted work-hint">{crate::exfmt!("Sostituisce l’azione con {} ordini.", "Replaces the action with {} orders.", r.commands.len())}</small>}
        </div>
    </crate::controllo::Modal>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stat() -> Value {
        json!({"zones":{"cols":8,"rows":8,"size_m":750.0},
            "buildings":[{"id":7,"name":"Scuola","pos":[3900.0,5900.0]},{"id":8,"name":"","pos":[0.0,0.0]}],
            "roads":[{"id":"sp12","name":"SP12"}],
            "resource_types":[{"id":"engines","name":"Autobotti"}],
            "places":[{"name":"Selvariva","pos":[3500.0,2280.0],"radius_m":500.0}],
            "roles":[{"id":"mayor","allowed_actions":["IssueWarning","EvacuateZone","EvacuateBuilding"]}]})
    }

    #[test]
    fn a_command_comes_back_from_its_draft() {
        let s = stat();
        let all = role_actions(&s, "nobody");
        for cmd in [
            json!({"type":"IssueWarning","zone":"F4"}),
            json!({"type":"MonitorArea","zones":["C3","D3"]}),
            json!({"type":"EvacuateBuilding","building":"Scuola"}),
            json!({"type":"OpenShelter","building":"#8"}),
            json!({"type":"CloseRoad","road":"sp12"}),
            json!({"type":"DeployEngine","at":{"x":10.0,"y":20.0}}),
            json!({"type":"CutFireLine","from":{"x":10.0,"y":20.0},"to":{"x":30.0,"y":40.0}}),
            json!({"type":"CutFireLine","along":"F4:N"}),
            json!({"type":"RequestReinforcement","resource":"engines","qty":3}),
        ] {
            let r = resolve(&s, &all, &Draft::from_command(&s, &cmd));
            assert_eq!(r.commands, vec![cmd.clone()], "{cmd}");
            assert_eq!(r.missing, None);
        }
        // A building given by id comes back by name when it has one.
        let r = resolve(&s, &all, &Draft::from_command(&s, &json!({"type":"OpenShelter","building":"#7"})));
        assert_eq!(r.commands, vec![json!({"type":"OpenShelter","building":"Scuola"})]);
        // A road given by name is listed by id.
        assert_eq!(Draft::from_command(&s, &json!({"type":"CloseRoad","road":"SP12"})).location, "sp12");
    }

    #[test]
    fn the_menu_follows_the_role() {
        let s = stat();
        assert_eq!(role_actions(&s, "mayor"), ["IssueWarning", "EvacuateZone", "EvacuateBuilding"]);
        assert!(role_actions(&s, "nobody").contains(&"DeployEngine".to_string()));
        // An action the role cannot take falls back to its first one.
        let r = resolve(&s, &role_actions(&s, "mayor"), &Draft { action: "DeployEngine".into(), ..Default::default() });
        assert_eq!((r.action.as_str(), r.missing), ("IssueWarning", Some("work_need_place")));
    }

    #[test]
    fn draft_fields_round_trip() {
        let d = Draft::from_command(&stat(), &json!({"type":"CutFireLine","from":{"x":1.0,"y":2.0},"to":{"x":3.0,"y":4.0}}));
        let f = d.fields();
        assert_eq!(Draft::read(|k, def| f.get(k).cloned().unwrap_or_else(|| def.to_string())), d);
    }
}
