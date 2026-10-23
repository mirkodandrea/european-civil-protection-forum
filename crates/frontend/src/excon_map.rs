//! EXCON map workspace: one interactive map with tabs for physical interventions, per-zone
//! disclosure to roles (fog of war), forecasts and missions.
use crate::{
    advanced::Forecast,
    api::{self, list, text},
    i18n::ex,
    map::SituationMap,
    widgets::{self, ApiForm},
    Action,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ExconMapProps {
    pub stat: Value,
    pub state: Value,
    pub master: Value,
    pub responses: std::collections::BTreeMap<String, Value>,
    pub busy: bool,
    pub action: Callback<Action>,
}
const LAYERS: [(&str, &str); 5] = [
    ("fire", "Incendio"),
    ("buildings", "Edifici"),
    ("roads", "Strade"),
    ("lines", "Linee e lanci"),
    ("units", "Squadre"),
];
fn zone_names(stat: &Value) -> Vec<String> {
    let (c, r) = (
        stat["zones"]["cols"].as_u64().unwrap_or(0) as u8,
        stat["zones"]["rows"].as_u64().unwrap_or(0) as u8,
    );
    (0..r)
        .flat_map(|row| (0..c).map(move |col| format!("{}{}", (b'A' + col) as char, row + 1)))
        .collect()
}
/// `names` plus the zones touching them (8-neighbourhood).
fn with_neighbours(stat: &Value, names: &[String]) -> BTreeSet<String> {
    let (cols, rows) = (
        stat["zones"]["cols"].as_i64().unwrap_or(0),
        stat["zones"]["rows"].as_i64().unwrap_or(0),
    );
    let mut out = BTreeSet::new();
    for n in names {
        let b = n.as_bytes();
        let (Some(c), Ok(r)) = (b.first().map(|c| (c - b'A') as i64), n[1..].parse::<i64>()) else {
            continue;
        };
        for dc in -1..=1 {
            for dr in -1..=1 {
                let (c, r) = (c + dc, r + dr);
                if (0..cols).contains(&c) && (1..=rows).contains(&r) {
                    out.insert(format!("{}{}", (b'A' + c as u8) as char, r));
                }
            }
        }
    }
    out
}
#[function_component(ExconMap)]
pub fn excon_map(p: &ExconMapProps) -> Html {
    let tab = use_state(|| "act".to_string());
    let tool = use_state(|| "none".to_string());
    let pending = use_state(|| None::<Value>);
    let damage = use_state(|| "destroyed".to_string());
    let reason = use_state(String::new);
    let role = use_state(String::new);
    let source = use_state(String::new);
    let layers = use_state(|| LAYERS.iter().map(|l| l.0.to_string()).collect::<BTreeSet<_>>());
    let zones = use_state(BTreeSet::<String>::new);
    let roles = list(&p.stat["roles"]);
    let role_id = if role.is_empty() {
        roles.first().map(|r| text(&r["id"]).to_string()).unwrap_or_default()
    } else {
        (*role).clone()
    };
    let known: Vec<String> = list(&p.master["role_knowledge"][role_id.as_str()]["zones"])
        .iter()
        .map(|z| text(z).to_string())
        .collect();
    let preview = p.responses.get("intervention").cloned().unwrap_or_default();
    let preview_valid = !preview.is_null()
        && preview["generation"] == p.state["generation"]
        && list(&preview["results"]).iter().all(|r| r["result"]["Err"].is_null());
    let paused = p.state["clock_control"]["running"] != true && p.state["phase"] != "resolving";
    let info_tab = *tab == "info";
    let onmap = {
        let stat = p.stat.clone();
        let tool = tool.clone();
        let pending = pending.clone();
        let cb = p.action.clone();
        let damage = damage.clone();
        let zones = zones.clone();
        let info_tab = info_tab;
        Callback::from(move |point: Value| {
            if info_tab {
                let z = text(&point["zone"]).to_string();
                let mut next = (*zones).clone();
                if !next.remove(&z) {
                    next.insert(z);
                }
                zones.set(next);
                return;
            }
            let at = json!({"x":point["x"],"y":point["y"]});
            let cmd = match tool.as_str() {
                "ignite" => json!({"type":"Ignite","at":at,"radius_m":80}),
                "drop" => json!({"type":"WaterDrop","at":at}),
                "ext" => json!({"type":"Extinguish","area":{"at":at,"radius_m":150}}),
                "extzone" => json!({"type":"Extinguish","area":point["zone"]}),
                "warn" => json!({"type":"IssueWarning","zone":point["zone"]}),
                "evac" => json!({"type":"EvacuateZone","zone":point["zone"]}),
                "line" => {
                    if let Some(from) = &*pending {
                        let cmd = json!({"type":"CutFireLine","from":from,"to":at});
                        pending.set(None);
                        cmd
                    } else {
                        pending.set(Some(at));
                        return;
                    }
                }
                "bstatus" => {
                    let x = point["x"].as_f64().unwrap_or(0.);
                    let y = point["y"].as_f64().unwrap_or(0.);
                    let nearest = list(&stat["buildings"])
                        .iter()
                        .filter_map(|b| {
                            let d = (b["pos"][0].as_f64()? - x).powi(2) + (b["pos"][1].as_f64()? - y).powi(2);
                            (d < 3600.).then_some((d, b))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    let Some((_, b)) = nearest else { return };
                    json!({"type":"SetBuildingStatus","building":format!("#{}",b["id"]),"damage":*damage})
                }
                _ => return,
            };
            cb.emit(Action::store("/api/master/preview", Some(json!({"commands":[cmd]})), "intervention"));
        })
    };
    let tab_button = |id: &'static str, label: &'static str| {
        let tab = tab.clone();
        let pending = pending.clone();
        html! {<button role="tab" aria-selected={(*tab == id).to_string()} class={classes!((*tab == id).then_some("active"))} onclick={Callback::from(move |_| { tab.set(id.to_string()); pending.set(None); })}>{ex(label)}</button>}
    };
    let tools = [
        ("none", "Ispeziona"),
        ("ignite", "Innesco"),
        ("line", "Linea tagliafuoco"),
        ("drop", "Lancio acqua"),
        ("extzone", "Spegni zona"),
        ("ext", "Spegni 150 m"),
        ("evac", "Evacua zona"),
        ("warn", "Avvisa zona"),
        ("bstatus", "Stato edificio"),
    ];
    let hint = match tool.as_str() {
        "line" if pending.is_some() => "Clicca il secondo punto della linea.",
        "line" => "Clicca il primo punto della linea.",
        "none" => "Passa il mouse o tocca la mappa per ispezionare.",
        _ => "Il clic prepara un’anteprima: nulla cambia finché non la applichi.",
    };
    let burning: Vec<String> = list(&p.state["world"]["burning_zones"]).iter().map(|z| text(z).to_string()).collect();
    let preset = |label: &'static str, set: BTreeSet<String>| {
        let zones = zones.clone();
        html! {<button type="button" onclick={Callback::from(move |_| zones.set(set.clone()))}>{ex(label)}</button>}
    };
    let can_publish = !p.busy && !zones.is_empty() && !layers.is_empty() && !source.trim().is_empty() && !role_id.is_empty();
    let publish = {
        let cb = p.action.clone();
        let (role_id, source, zones, layers) = (role_id.clone(), source.clone(), zones.clone(), layers.clone());
        Callback::from(move |_| {
            cb.emit(Action::post(
                "/api/master/knowledge",
                json!({"role":role_id,"source":source.trim(),"zones":*zones,"layers":*layers}),
            ))
        })
    };
    let publish_all = {
        let cb = p.action.clone();
        let (role_id, source) = (role_id.clone(), source.clone());
        Callback::from(move |_| {
            cb.emit(Action::post(
                "/api/master/knowledge",
                json!({"role":role_id,"source":source.trim(),"current_perimeter":true}),
            ))
        })
    };
    let form = |title: &str, endpoint: &str, initial: Value| html! {<ApiForm title={title.to_string()} endpoint={endpoint.to_string()} {initial} action={p.action.clone()} busy={p.busy}/>};
    let selected_zones: Vec<String> = if info_tab { zones.iter().cloned().collect() } else { Vec::new() };
    let marked_zones = if info_tab { known.clone() } else { Vec::new() };
    html! {<main class="page excon-map">
        <section class="card em-map">
            <div class="em-tools" id="tools" role="toolbar" aria-label={ex("Strumenti di intervento")}>
                {for tools.iter().map(|(id, label)| { let id = id.to_string(); let tool = tool.clone(); let pending = pending.clone(); let tab = tab.clone(); html!{<button type="button" class={classes!((*tool == id && !info_tab).then_some("primary"))} onclick={Callback::from(move |_| { tool.set(id.clone()); pending.set(None); if *tab == "info" { tab.set("act".into()); } })}>{ex(label)}</button>} })}
                if *tool == "bstatus" {<select aria-label={ex("Stato edificio")} onchange={{let damage=damage.clone();Callback::from(move|e|damage.set(api::select(e)))}}><option value="destroyed" selected={*damage=="destroyed"}>{ex("Distrutto")}</option><option value="damaged" selected={*damage=="damaged"}>{ex("Danneggiato")}</option><option value="intact" selected={*damage=="intact"}>{ex("Integro")}</option></select>}
                if pending.is_some() {<button type="button" onclick={{let pending=pending.clone();Callback::from(move |_|pending.set(None))}}>{ex("Annulla primo punto")}</button>}
            </div>
            <p class="muted">{if info_tab {ex("Clicca le zone da rivelare al ruolo: il clic le aggiunge o le toglie dalla selezione.")} else {ex(hint)}}</p>
            <SituationMap stat={p.stat.clone()} view={p.state["world"].clone()} onclick={onmap} pending={(*pending).clone()} {selected_zones} {marked_zones}/>
            {widgets::stats(&p.state["world"]["stats"])}
        </section>
        <aside class="em-side">
            <div class="work-modes em-tabs" role="tablist" aria-label={ex("Pannello mappa")}>{tab_button("act","Intervento")}{tab_button("info","Informazioni ai ruoli")}{tab_button("forecast","Previsione")}{tab_button("missions","Missioni")}</div>
            <div hidden={*tab != "act"}>
                <section class="card" id="physicalpreview"><h2>{ex("Anteprima intervento fisico")}</h2>
                if preview.is_null() {<p class="muted">{ex("Scegli uno strumento e clicca la mappa per calcolare l’anteprima.")}</p>}
                if !preview.is_null(){{widgets::preview(&preview)}<p class="muted">{if preview_valid{ex("Anteprima corrente")}else{ex("Anteprima scaduta o non applicabile: calcola una nuova anteprima.")}}</p>}
                <label>{ex("Motivazione dell’intervento")}<input id="interventionreason" value={(*reason).clone()} oninput={{let reason=reason.clone();Callback::from(move|e|reason.set(api::input(e)))}}/></label>
                <button id="applypreview" class="primary" disabled={p.busy||!preview_valid||!paused||reason.trim().is_empty()} onclick={{let cb=p.action.clone();let preview=preview.clone();let reason=reason.clone();Callback::from(move |_|cb.emit(Action::post("/api/master/preview/apply",json!({"generation":preview["generation"],"commands":preview["_request"]["commands"],"reason":*reason}))))}}>{ex("Applica anteprima revisionata")}</button>
                <p class="muted">{ex("Richiede clock fermo e generazione ancora corrente.")}</p>{widgets::details(ex("Correzione disponibile"),&p.master["correction"])}</section>
                {form(ex("Correggi ultimo intervento"),"/api/master/correct-intervention",json!({"reason":""}))}
            </div>
            <div hidden={!info_tab}>
                <section class="card" id="role-knowledge"><h2>{ex("Informazioni ai ruoli · fog of war")}</h2>
                <p class="muted">{ex("Il ruolo conosce solo ciò che pubblichi qui, come fotografia al momento della consegna. Fuori dalle zone rivelate la sua mappa resta velata.")}</p>
                <label>{ex("Ruolo")}<select id="report-role" value={role_id.clone()} onchange={{let role=role.clone();Callback::from(move|e|role.set(api::select(e)))}}>{for roles.iter().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==role_id}>{text(&r["name"])}</option>})}</select></label>
                <p class="muted">{if known.is_empty() {ex("Questo ruolo non conosce ancora nessuna zona.").to_string()} else {format!("{} {} · {}: {}", ex("Zone già note:"), known.len(), text(&p.master["role_knowledge"][role_id.as_str()]["last_clock"]), known.join(", "))}}</p>
                <fieldset><legend>{ex("Livelli da rivelare")}</legend>{for LAYERS.iter().map(|(id,label)|{let id=id.to_string();let layers=layers.clone();let checked=layers.contains(&id);html!{<label class="mt-check"><input type="checkbox" {checked} onchange={Callback::from(move |_|{let mut next=(*layers).clone();if !next.remove(&id){next.insert(id.clone());}layers.set(next);})}/>{ex(label)}</label>}})}</fieldset>
                <div class="row">{preset("Nessuna", BTreeSet::new())}{preset("Zone in fiamme", burning.iter().cloned().collect())}{preset("In fiamme + adiacenti", with_neighbours(&p.stat,&burning))}{preset("Tutta la mappa", zone_names(&p.stat).into_iter().collect())}</div>
                <p>{if zones.is_empty(){ex("Nessuna zona selezionata. Clicca la mappa.").to_string()}else{format!("{} {}: {}", zones.len(), ex("zone selezionate"), zones.iter().cloned().collect::<Vec<_>>().join(", "))}}</p>
                <label>{ex("Fonte del rapporto")}<input id="report-source" value={(*source).clone()} oninput={{let source=source.clone();Callback::from(move|e|source.set(api::input(e)))}}/></label>
                <button id="publish" class="primary" disabled={!can_publish} onclick={publish}>{ex("Rivela le zone selezionate")}</button>
                <button id="publish-all" disabled={p.busy||source.trim().is_empty()||role_id.is_empty()} onclick={publish_all}>{ex("Pubblica l’intero perimetro")}</button>
                </section>
            </div>
            <div hidden={*tab != "forecast"}>
                <Forecast stat={p.stat.clone()} state={p.state.clone()} draft={p.responses.get("forecast").cloned().unwrap_or_default()} action={p.action.clone()} busy={p.busy}/>
            </div>
            <div hidden={*tab != "missions"}>
                <section class="card"><h2>{ex("Modello operativo e missioni effettive")}</h2>{widgets::details(ex("Stato operativo"),&p.state["world"]["operations"])}{widgets::details(ex("Missioni: stato, posizione, arrivo ed esiti"),&p.state["world"]["deployments"])}{widgets::details(ex("Risorse e impegni"),&p.state["resource_accounting"])}
                {for list(&p.state["world"]["allocations"]).iter().filter(|a|a["returned"]!=a["qty"]).map(|a|{let cb=p.action.clone();let id=a["id"].clone();let qty=a["qty"].as_u64().unwrap_or(0).saturating_sub(a["returned"].as_u64().unwrap_or(0));html!{<button disabled={p.busy||qty==0} onclick={Callback::from(move |_|cb.emit(Action::post("/api/master/command",json!({"type":"ReturnResource","allocation":id,"qty":qty}))))}>{crate::exfmt!("Conferma ritorno #{} · {} unità","Confirm return #{} · {} units",a["id"],qty)}</button>}})}</section>
            </div>
        </aside>
    </main>}
}
