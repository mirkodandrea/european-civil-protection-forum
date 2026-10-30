//! "Controllo": the EXCON control surface. The map is the main state view; every possible action
//! is an entry of a searchable, grouped list. Selecting an entry opens its own modal (with a map
//! to pick locations when the tool needs one, otherwise a short form).
use crate::excon_queue::{command_body, ExconQueue, When, WhenPicker};
use crate::{
    advanced::Forecast,
    api::{self, list, text},
    i18n::ex,
    map::SituationMap,
    widgets::{self, ApiForm, Proposal},
    Action, ActionKind,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use yew::prelude::*;

// ------------------------------------------------------------------ tool catalogue

pub struct Tool {
    pub id: &'static str,
    pub group: &'static str,
    pub icon: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
}
const GROUPS: [&str; 6] = [
    "Fuoco e interventi",
    "Meteo",
    "Risorse e squadre",
    "Comunicazioni e previsioni",
    "Tempo e fasi",
    "Esercitazione",
];
pub const TOOLS: &[Tool] = &[
    Tool { id: "ignite", group: "Fuoco e interventi", icon: "🔥", label: "Innesco", tip: "Accende un nuovo focolaio nel punto scelto sulla mappa (almeno 40 m di raggio)." },
    Tool { id: "line", group: "Fuoco e interventi", icon: "✂", label: "Linea tagliafuoco", tip: "Apre una fascia senza combustibile tra due punti scelti sulla mappa." },
    Tool { id: "drop", group: "Fuoco e interventi", icon: "💧", label: "Lancio d’acqua", tip: "Un lancio aereo di acqua nel punto scelto." },
    Tool { id: "extzone", group: "Fuoco e interventi", icon: "🧯", label: "Spegni zona", tip: "Spegne il fuoco in un’intera zona della griglia." },
    Tool { id: "extcircle", group: "Fuoco e interventi", icon: "◎", label: "Spegni area circolare", tip: "Spegne il fuoco entro un raggio dal punto scelto." },
    Tool { id: "extarea", group: "Fuoco e interventi", icon: "⬠", label: "Spegni area disegnata", tip: "Disegna sulla mappa un’area (almeno 3 vertici): il fuoco al suo interno si spegne subito in tutte le simulazioni. La vegetazione non cambia: il terreno non bruciato può riaccendersi se il fuoco lo raggiunge di nuovo." },
    Tool { id: "evac", group: "Fuoco e interventi", icon: "🚌", label: "Evacua zona", tip: "Ordina l’evacuazione di tutti gli edifici occupati di una zona." },
    Tool { id: "evacborgo", group: "Fuoco e interventi", icon: "🏘", label: "Evacua borgo", tip: "Ordina l’evacuazione di tutte le case di un borgo, case sparse comprese: partenze scaglionate verso l’area di raccolta del borgo. Clicca una casa del borgo." },
    Tool { id: "warn", group: "Fuoco e interventi", icon: "📢", label: "Avvisa zona", tip: "Avvisa la popolazione di una zona: dimezza il tempo di evacuazione." },
    Tool { id: "drone", group: "Risorse e squadre", icon: "🛰", label: "Drone di monitoraggio", tip: "Invia il drone del comandante antincendio sulla zona scelta: finché è in posizione tiene aggiornata la sua mappa. Per più zone usa il comando JSON (es. \"F3-G4\")." },
    Tool { id: "bstatus", group: "Fuoco e interventi", icon: "🏠", label: "Stato edificio", tip: "Forza lo stato di danno di un edificio scelto sulla mappa." },
    Tool { id: "correct", group: "Fuoco e interventi", icon: "↶", label: "Correggi ultimo intervento", tip: "Annulla l’ultimo intervento fisico, se nulla è cambiato dopo di esso." },
    Tool { id: "weather", group: "Meteo", icon: "🌬", label: "Condizioni a contorno", tip: "Imposta vento (direzione da cui soffia e velocità) e umidità del combustibile." },
    Tool { id: "missions", group: "Risorse e squadre", icon: "🚒", label: "Missioni e ritorni", tip: "Stato delle missioni, risorse impegnate e conferma del ritorno delle squadre." },
    Tool { id: "limits", group: "Risorse e squadre", icon: "📦", label: "Limiti risorse", tip: "Sostituisce per intero i limiti di risorse disponibili." },
    Tool { id: "knowledge", group: "Comunicazioni e previsioni", icon: "👁", label: "Informazioni ai ruoli", tip: "Rivela a un ruolo le zone scelte sulla mappa (fog of war)." },
    Tool { id: "forecast", group: "Comunicazioni e previsioni", icon: "🌦", label: "Previsione", tip: "Calcola una bozza privata di previsione, revisionala e pubblicala ai ruoli." },
    Tool { id: "inject", group: "Comunicazioni e previsioni", icon: "✉", label: "Inject personalizzato", tip: "Invia un messaggio dell’EXCON a tutti o a un ruolo." },
    Tool { id: "prompt", group: "Comunicazioni e previsioni", icon: "🤖", label: "Prompt all’AI EXCON", tip: "Chiede all’AI di proporre azioni, senza eseguirle." },
    Tool { id: "proposals", group: "Comunicazioni e previsioni", icon: "🗳", label: "Proposte da revisionare", tip: "Proposte dei ruoli in attesa di approvazione." },
    Tool { id: "references", group: "Comunicazioni e previsioni", icon: "📚", label: "Piani e procedure ai valutatori", tip: "Pubblica piani e procedure di riferimento ai valutatori." },
    Tool { id: "clock", group: "Tempo e fasi", icon: "⏱", label: "Clock", tip: "Avvia o ferma il clock simulato e ne imposta il rapporto." },
    Tool { id: "speed", group: "Tempo e fasi", icon: "⏩", label: "Velocità di risoluzione", tip: "Fattore di velocità della risoluzione del turno (0 = massima)." },
    Tool { id: "advance", group: "Tempo e fasi", icon: "⏭", label: "Avanza il tempo", tip: "Salta in avanti di alcuni minuti simulati." },
    Tool { id: "phase", group: "Tempo e fasi", icon: "🏁", label: "Fase di esercitazione", tip: "Cambia la fase dell’esercitazione." },
    Tool { id: "rollback", group: "Tempo e fasi", icon: "⏮", label: "Rollback a inizio turno", tip: "Riporta l’esercitazione all’inizio di un turno." },
    Tool { id: "msel", group: "Esercitazione", icon: "🗓", label: "MSEL e programmazione", tip: "Inject pianificati: invio immediato o consegna programmata." },
    Tool { id: "design", group: "Esercitazione", icon: "📐", label: "Disegno formativo", tip: "Obiettivi, fasi e punti decisionali; percorsi e adattamenti." },
    Tool { id: "log", group: "Esercitazione", icon: "📜", label: "Messaggi e log", tip: "Messaggi visibili a EXCON ed eventi dell’esercitazione." },
    Tool { id: "links", group: "Esercitazione", icon: "🔗", label: "Collegamenti partecipanti", tip: "Link di accesso dei partecipanti." },
    Tool { id: "json", group: "Esercitazione", icon: "{ }", label: "Comando JSON e catalogo", tip: "Anteprima di comandi in JSON e catalogo completo dei comandi." },
    Tool { id: "end", group: "Esercitazione", icon: "🏁", label: "Fine ed evidenze", tip: "Conclude l’esercitazione o scarica il debrief JSON." },
];
pub(crate) fn tool_label(id: &str) -> &'static str {
    TOOLS.iter().find(|t| t.id == id).map(|t| ex(t.label)).unwrap_or("")
}
fn matches_query(t: &Tool, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    [t.label, t.tip, t.group, ex(t.label), ex(t.tip), ex(t.group)]
        .iter()
        .any(|s| s.to_lowercase().contains(&q))
}

// ------------------------------------------------------------------ pure payload builders

/// Parameters shared by the map tools.
#[derive(Clone, PartialEq, Debug)]
pub struct CmdParams {
    pub radius_m: f64,
    pub damage: String,
}
impl Default for CmdParams {
    fn default() -> Self {
        Self { radius_m: 80., damage: "destroyed".into() }
    }
}
/// Number of map clicks a tool needs.
pub fn points_needed(tool: &str) -> usize {
    match tool {
        "line" => 2,
        // Polygon: every click adds a vertex (see [`MAX_AREA_VERTICES`]).
        "extarea" => MAX_AREA_VERTICES,
        _ => 1,
    }
}
/// Most vertices of a drawn area (`sim::MAX_POLYGON_VERTICES`).
pub const MAX_AREA_VERTICES: usize = 200;
/// Area of a drawn polygon in hectares (`points` are map picks `{x, y}`).
pub fn polygon_ha(points: &[Value]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.;
    }
    let p = |i: usize| (points[i % n]["x"].as_f64().unwrap_or(0.), points[i % n]["y"].as_f64().unwrap_or(0.));
    (0..n).map(|i| p(i).0 * p(i + 1).1 - p(i + 1).0 * p(i).1).sum::<f64>().abs() / 2. / 10_000.
}
/// Builds the `Command` JSON of a map tool from the picked map points (payloads of the map
/// `onclick`: `{x, y, zone, building_id?}`). `None` while the selection is incomplete.
pub fn build_command(tool: &str, points: &[Value], p: &CmdParams, stat: &Value) -> Option<Value> {
    let pt = |i: usize| -> Option<Value> {
        let v = points.get(i)?;
        Some(json!({"x": v["x"].as_f64()?, "y": v["y"].as_f64()?}))
    };
    let zone = || -> Option<Value> {
        let z = points.first()?["zone"].as_str()?;
        (!z.is_empty()).then(|| json!(z))
    };
    Some(match tool {
        "ignite" => json!({"type":"Ignite","at":pt(0)?,"radius_m":p.radius_m}),
        "drop" => json!({"type":"WaterDrop","at":pt(0)?}),
        "extcircle" => json!({"type":"Extinguish","area":{"at":pt(0)?,"radius_m":p.radius_m}}),
        "extzone" => json!({"type":"Extinguish","area":zone()?}),
        "warn" => json!({"type":"IssueWarning","zone":zone()?}),
        "evac" => json!({"type":"EvacuateZone","zone":zone()?}),
        "evacborgo" => json!({"type":"EvacuateDistrict","district":district_at(stat, points.first()?)?}),
        "drone" => json!({"type":"MonitorArea","zones":[zone()?]}),
        "line" => json!({"type":"CutFireLine","from":pt(0)?,"to":pt(1)?}),
        "extarea" => {
            if points.len() < 3 || polygon_ha(points) <= 0. {
                return None;
            }
            json!({"type":"ExtinguishArea","polygon":(0..points.len()).map(pt).collect::<Option<Vec<_>>>()?})
        }
        "bstatus" => {
            let v = points.first()?;
            let building = if let Some(id) = v["building_id"].as_u64() {
                format!("#{id}")
            } else {
                let (x, y) = (v["x"].as_f64()?, v["y"].as_f64()?);
                let b = list(&stat["buildings"])
                    .iter()
                    .filter_map(|b| {
                        let d = (b["pos"][0].as_f64()? - x).powi(2) + (b["pos"][1].as_f64()? - y).powi(2);
                        (d < 3600.).then_some((d, b))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))?
                    .1;
                format!("#{}", b["id"])
            };
            json!({"type":"SetBuildingStatus","building":building,"damage":p.damage})
        }
        _ => return None,
    })
}
/// The borgo of the building clicked (or nearest the click, within 200 m).
fn district_at(stat: &Value, pick: &Value) -> Option<String> {
    let id = match pick["building_id"].as_u64() {
        Some(id) => id,
        None => {
            let (x, y) = (pick["x"].as_f64()?, pick["y"].as_f64()?);
            list(&stat["buildings"])
                .iter()
                .filter_map(|b| {
                    let d = (b["pos"][0].as_f64()? - x).powi(2) + (b["pos"][1].as_f64()? - y).powi(2);
                    (d < 40_000.).then_some((d, b["id"].as_u64()?))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))?
                .1
        }
    };
    list(&stat["districts"])
        .iter()
        .find(|d| list(&d["buildings"]).iter().any(|b| b.as_u64() == Some(id)))
        .map(|d| text(&d["name"]).to_string())
}
/// Which map click hint applies.
pub(crate) fn map_hint(tool: &str, picked: usize) -> &'static str {
    match (tool, picked) {
        ("line", 0) => "Clicca sulla mappa il primo punto della linea.",
        ("line", 1) => "Clicca il secondo punto della linea.",
        ("line", _) => "Linea pronta. Un nuovo clic ricomincia dal primo punto.",
        ("extarea", 0) => "Clicca sulla mappa i vertici dell’area, in ordine.",
        ("extarea", 1 | 2) => "Aggiungi altri vertici: servono almeno 3.",
        ("extarea", _) => "Area pronta. Ogni clic aggiunge un vertice; l’area si chiude da sola.",
        ("extzone" | "evac" | "warn" | "drone", _) => "Clicca una zona della mappa.",
        ("bstatus", _) => "Clicca un edificio sulla mappa.",
        ("evacborgo", _) => "Clicca una casa del borgo sulla mappa.",
        _ => "Clicca sulla mappa il punto dell’intervento.",
    }
}

/// The map tool that edits a command, when one does (`SetWeather` uses the weather form).
pub fn tool_for(cmd: &Value) -> Option<&'static str> {
    Some(match text(&cmd["type"]) {
        "Ignite" => "ignite",
        "WaterDrop" => "drop",
        "Extinguish" if cmd["area"].is_object() => "extcircle",
        "Extinguish" => "extzone",
        "ExtinguishArea" => "extarea",
        "IssueWarning" => "warn",
        "EvacuateZone" => "evac",
        "EvacuateDistrict" => "evacborgo",
        "MonitorArea" if list(&cmd["zones"]).len() <= 1 => "drone",
        "CutFireLine" if cmd["along"].is_null() => "line",
        "SetBuildingStatus" => "bstatus",
        "SetWeather" => "weather",
        _ => return None,
    })
}

/// The map picks that rebuild `cmd` with its tool (see [`build_command`]):
/// points `{x, y, zone}`, zones `{zone}`, buildings `{x, y, building_id}`.
/// Named locations (a place name instead of a point) give no pick.
pub fn initial_points(cmd: &Value, stat: &Value) -> Vec<Value> {
    let point = |v: &Value| -> Option<Value> {
        let (x, y) = (v["x"].as_f64()?, v["y"].as_f64()?);
        Some(json!({"x": x, "y": y, "zone": crate::place_pick::zone_at(stat, x, y).unwrap_or_default()}))
    };
    let zone = |v: &Value| v.as_str().filter(|z| !z.is_empty()).map(|z| json!({"zone": z}));
    let picks = match text(&cmd["type"]) {
        "Ignite" | "WaterDrop" => vec![point(&cmd["at"])],
        "Extinguish" if cmd["area"].is_object() => vec![point(&cmd["area"]["at"])],
        "Extinguish" => vec![zone(&cmd["area"])],
        "ExtinguishArea" => list(&cmd["polygon"]).iter().map(point).collect(),
        "IssueWarning" | "EvacuateZone" => vec![zone(&cmd["zone"])],
        "EvacuateDistrict" => vec![list(&stat["districts"])
            .iter()
            .find(|d| text(&d["name"]) == text(&cmd["district"]) || text(&d["id"]) == text(&cmd["district"]))
            .and_then(|d| list(&stat["places"]).iter().find(|p| p["name"] == d["name"]))
            .and_then(|p| Some(json!({"x": p["pos"][0].as_f64()?, "y": p["pos"][1].as_f64()?})))],
        "MonitorArea" => vec![zone(&cmd["zones"][0])],
        "CutFireLine" => vec![point(&cmd["from"]), point(&cmd["to"])],
        "SetBuildingStatus" => {
            let id = text(&cmd["building"]).trim_start_matches('#');
            vec![list(&stat["buildings"]).iter().find(|b| b["id"].to_string() == id).and_then(|b| {
                Some(json!({"x": b["pos"][0].as_f64()?, "y": b["pos"][1].as_f64()?, "building_id": b["id"]}))
            })]
        }
        _ => vec![],
    };
    // Partial picks would mix an old point with a new one: all or nothing.
    picks.iter().all(Option::is_some).then(|| picks.into_iter().flatten().collect()).unwrap_or_default()
}

/// `edited` (built by a tool) with the fields of `original` the tool does not
/// set, such as an evacuation destination or a fire-line width.
pub fn keep_extra_fields(original: &Value, edited: &Value) -> Value {
    let mut out = edited.clone();
    if let (Some(o), Some(e)) = (original.as_object(), out.as_object_mut()) {
        for (k, v) in o {
            if !e.contains_key(k) {
                e.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

#[derive(Clone, PartialEq)]
pub enum FieldKind {
    Text,
    Area,
    Number,
    Check,
    Select(Vec<(String, String)>),
    /// Checkbox list; the value is the selected keys joined by commas and the body gets an array.
    Multi(Vec<(String, String)>),
}
#[derive(Clone, PartialEq)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub init: String,
    pub unit: &'static str,
}
impl Field {
    fn new(key: &'static str, label: &'static str, kind: FieldKind, init: impl Into<String>) -> Self {
        Self { key, label, kind, init: init.into(), unit: "" }
    }
    fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }
}
/// Builds a request body from form field values; `arrays` wraps those keys in a one-item list.
pub fn fields_body(fields: &[Field], values: &BTreeMap<String, String>, extra: &Value, arrays: &[&str]) -> Result<Value, String> {
    let mut body = if extra.is_object() { extra.clone() } else { json!({}) };
    for f in fields {
        let raw = values.get(f.key).cloned().unwrap_or_else(|| f.init.clone());
        let v = match f.kind {
            FieldKind::Number => {
                let n = raw
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("{}: {}", ex(f.label), ex("numero non valido")))?;
                // Whole numbers go out as integers: integer fields (AdvanceTime.minutes: u32) reject 40.0.
                if n.fract() == 0.0 && n.abs() < 9e15 { json!(n as i64) } else { json!(n) }
            }
            FieldKind::Check => json!(raw == "true"),
            FieldKind::Multi(_) => json!(raw.split(',').filter(|k| !k.is_empty()).collect::<Vec<_>>()),
            _ => json!(raw),
        };
        body[f.key] = if arrays.contains(&f.key) { json!([v]) } else { v };
    }
    Ok(body)
}

// ------------------------------------------------------------------ modal frame

#[derive(Properties, PartialEq)]
pub struct ModalProps {
    pub title: AttrValue,
    pub onclose: Callback<()>,
    #[prop_or_default]
    pub onapply: Option<Callback<()>>,
    #[prop_or(ex("Applica").into())]
    pub apply_label: AttrValue,
    #[prop_or_default]
    pub apply_disabled: bool,
    #[prop_or_default]
    pub busy: bool,
    #[prop_or_default]
    pub done: u64,
    #[prop_or_default]
    pub error: AttrValue,
    /// Map shown beside the form (wide layout).
    #[prop_or_default]
    pub map: Option<Html>,
    #[prop_or_default]
    pub extra_actions: Html,
    #[prop_or_default]
    pub children: Html,
}
#[function_component(Modal)]
pub fn modal(p: &ModalProps) -> Html {
    let root = use_node_ref();
    // Close after our own request completes without error. `done` counts finished actions.
    let submitted = use_state(|| None::<u64>);
    {
        let root = root.clone();
        use_effect_with((), move |_| {
            if let Some(el) = root.cast::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        });
    }
    {
        let (submitted, onclose, error) = (submitted.clone(), p.onclose.clone(), p.error.clone());
        use_effect_with(p.done, move |done| {
            if let Some(at) = *submitted {
                if *done > at && error.is_empty() {
                    onclose.emit(());
                }
            }
        });
    }
    let onkeydown = {
        let (onclose, busy) = (p.onclose.clone(), p.busy);
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Escape" && !busy {
                e.prevent_default();
                // A tool opened from another dialog (e.g. Risposte) closes alone.
                e.stop_propagation();
                onclose.emit(());
            }
        })
    };
    let apply = p.onapply.clone().map(|cb| {
        let (submitted, done) = (submitted.clone(), p.done);
        Callback::from(move |_| {
            submitted.set(Some(done));
            cb.emit(());
        })
    });
    let wide = p.map.is_some();
    let close = {
        let c = p.onclose.clone();
        Callback::from(move |_| c.emit(()))
    };
    let footer = html! {<div class="row modal-actions">
        if let Some(apply) = apply {<button type="button" class="primary" data-apply="" disabled={p.busy || p.apply_disabled} onclick={apply}>{if p.busy {ex("Elaborazione…").to_string()} else {p.apply_label.to_string()}}</button>}
        {p.extra_actions.clone()}
        <div class="grow"/>
        <button type="button" data-cancel="" disabled={p.busy} onclick={close.clone()}>{if p.onapply.is_some() {ex("Annulla")} else {ex("Chiudi")}}</button>
    </div>};
    html! {<div class={classes!("modal-backdrop", wide.then_some("work-backdrop"))}>
        <section ref={root} tabindex="-1" class={if wide {"work-modal ctl-modal"} else {"inject-modal ctl-modal"}} role="dialog" aria-modal="true" aria-label={p.title.clone()} {onkeydown}>
            <header class="row"><h2>{p.title.clone()}</h2><div class="grow"/><button type="button" aria-label={ex("Chiudi")} title={ex("Chiudi")} disabled={p.busy} onclick={close}>{"✕"}</button></header>
            if !p.error.is_empty() {<p class="error" role="alert">{p.error.clone()}</p>}
            if let Some(map) = &p.map {
                <div class="work-modal-layout"><div class="work-modal-map">{map.clone()}</div><div class="work-modal-form">{p.children.clone()}{footer}</div></div>
            } else {
                <div class="ctl-body">{p.children.clone()}</div>{footer}
            }
        </section>
    </div>}
}

#[derive(Properties, PartialEq, Clone)]
pub struct CommonProps {
    pub stat: Value,
    pub state: Value,
    pub master: Value,
    pub responses: BTreeMap<String, Value>,
    pub busy: bool,
    pub error: AttrValue,
    pub done: u64,
    pub action: Callback<Action>,
    pub onclose: Callback<()>,
}

fn pick_map(stat: &Value, state: &Value, onclick: Callback<Value>, pending: Option<Value>, selected: Vec<String>, marked: Vec<String>) -> Html {
    html! {<>
        <SituationMap id="ctl-modal-map" stat={stat.clone()} view={state["world"].clone()} {onclick} {pending} selected_zones={selected} marked_zones={marked}/>
    </>}
}

// ------------------------------------------------------------------ map-command modal

#[derive(Properties, PartialEq)]
pub struct MapToolProps {
    pub tool: AttrValue,
    pub common: CommonProps,
    /// Hand the finished command to the caller instead of posting it to the
    /// running exercise (the scenario editor uses the tool forms this way).
    #[prop_or_default]
    pub oncommand: Option<Callback<Value>>,
    /// Replaces the situation map: gets the map-click callback and the pending point.
    #[prop_or_default]
    pub map_for: Option<MapFactory>,
    /// Command being edited: the form starts from its picks and parameters.
    #[prop_or_default]
    pub initial: Option<Value>,
}

/// Builds a point-picking map for [`MapToolModal`] when no live situation exists.
#[derive(Clone)]
pub struct MapFactory(pub std::rc::Rc<dyn Fn(Callback<Value>, Option<Value>) -> Html>);
impl PartialEq for MapFactory {
    fn eq(&self, _: &Self) -> bool {
        false // closes over the scenario being edited
    }
}
#[function_component(MapToolModal)]
pub fn map_tool_modal(p: &MapToolProps) -> Html {
    let c = &p.common;
    let tool = p.tool.as_str();
    let initial = p.initial.clone().unwrap_or_default();
    let points = use_state(|| initial_points(&initial, &c.stat));
    let radius = use_state(|| {
        let r = if tool == "extcircle" { initial["area"]["radius_m"].as_f64() } else { initial["radius_m"].as_f64() };
        r.map(|r| format!("{r:.0}")).unwrap_or_else(|| if tool == "extcircle" { "150".to_string() } else { "80".to_string() })
    });
    let damage = use_state(|| initial["damage"].as_str().unwrap_or("destroyed").to_string());
    let when = use_state(|| When::Play);
    let params = CmdParams { radius_m: radius.trim().parse().unwrap_or(0.), damage: (*damage).clone() };
    let cmd = build_command(tool, &points, &params, &c.stat);
    let radius_bad = matches!(tool, "ignite" | "extcircle") && !(if tool == "ignite" { 40.0..=2000.0 } else { 10.0..=5000.0 }).contains(&params.radius_m);
    let need = points_needed(tool);
    let onmap = {
        let points = points.clone();
        Callback::from(move |pt: Value| {
            let mut next = if points.len() >= need { Vec::new() } else { (*points).clone() };
            next.push(pt);
            points.set(next);
        })
    };
    let zone_tool = matches!(tool, "extzone" | "evac" | "warn" | "drone");
    let pending = if zone_tool {
        points.first().map(|v| json!({"zone": v["zone"]}))
    } else if tool == "line" && !points.is_empty() {
        // Both ends, joined once the second is picked.
        Some(json!({"points": points.iter().map(|v| json!([v["x"], v["y"]])).collect::<Vec<_>>()}))
    } else if tool == "extarea" && !points.is_empty() {
        // Every vertex, closed into a shaded polygon from the third on.
        Some(json!({"points": points.iter().map(|v| json!([v["x"], v["y"]])).collect::<Vec<_>>(), "closed": true}))
    } else {
        points.last().cloned()
    };
    let summary = if let Some(first) = points.first() {
        if zone_tool {
            crate::exfmt!("Zona selezionata: {}", "Selected zone: {}", text(&first["zone"]))
        } else if tool == "bstatus" {
            match &cmd {
                Some(cmd) => crate::exfmt!("Edificio selezionato: {}", "Selected building: {}", text(&cmd["building"])),
                None => ex("Nessun edificio vicino al punto: riprova.").to_string(),
            }
        } else if tool == "extarea" {
            crate::exfmt!("Area di {} vertici · {:.1} ha", "Area of {} vertices · {:.1} ha", points.len(), polygon_ha(&points))
        } else if tool == "line" && points.len() == 2 {
            let d = ((points[1]["x"].as_f64().unwrap_or(0.) - points[0]["x"].as_f64().unwrap_or(0.)).powi(2)
                + (points[1]["y"].as_f64().unwrap_or(0.) - points[0]["y"].as_f64().unwrap_or(0.)).powi(2))
            .sqrt();
            crate::exfmt!("Linea di {:.0} m", "Line of {:.0} m", d)
        } else {
            crate::exfmt!("Punto selezionato: {}", "Selected point: {}", crate::place_pick::point_label(&c.stat, first["x"].as_f64().unwrap_or(0.), first["y"].as_f64().unwrap_or(0.)))
        }
    } else {
        ex(map_hint(tool, 0)).to_string()
    };
    let hint = if points.len() < need && !points.is_empty() { ex(map_hint(tool, points.len())).to_string() } else { String::new() };
    let undo_vertex = {
        let points = points.clone();
        Callback::from(move |_| {
            let mut next = (*points).clone();
            next.pop();
            points.set(next);
        })
    };
    let onapply = {
        let (cb, cmd, oncommand, onclose, when) = (c.action.clone(), cmd.clone(), p.oncommand.clone(), c.onclose.clone(), (*when).clone());
        Callback::from(move |_| {
            if let Some(cmd) = &cmd {
                match &oncommand {
                    Some(f) => {
                        f.emit(cmd.clone());
                        onclose.emit(());
                    }
                    None => cb.emit(Action::post("/api/master/command", command_body(cmd.clone(), &when))),
                }
            }
        })
    };
    let map = match &p.map_for {
        Some(f) => (f.0)(onmap, pending),
        None => pick_map(&c.stat, &c.state, onmap, pending, Vec::new(), Vec::new()),
    };
    let apply_label = apply_label(p.oncommand.is_some(), &c.state, &when);
    let when_bad = p.oncommand.is_none() && matches!(&*when, When::At(t) if !crate::excon_queue::valid_clock(t));
    let reset = {
        let points = points.clone();
        Callback::from(move |_| points.set(Vec::new()))
    };
    html! {<Modal title={tool_label(tool)} onclose={c.onclose.clone()} {onapply} apply_disabled={cmd.is_none() || radius_bad || when_bad} busy={c.busy} done={c.done} error={c.error.clone()} {apply_label}
        {map}
        extra_actions={html!{<>
            if tool == "extarea" {<button type="button" disabled={points.is_empty() || c.busy} onclick={undo_vertex}>{ex("Togli ultimo vertice")}</button>}
            <button type="button" disabled={points.is_empty() || c.busy} onclick={reset}>{ex("Azzera selezione")}</button>
        </>}}>
        <p class="muted">{ex(TOOLS.iter().find(|t| t.id == tool).map(|t| t.tip).unwrap_or(""))}</p>
        <p class="ctl-pick" aria-live="polite">{summary}</p>
        if !hint.is_empty() {<p class="muted">{hint}</p>}
        if matches!(tool, "ignite" | "extcircle") {<label>{ex("Raggio")}<span class="ctl-unit"><input type="number" min="10" step="10" value={(*radius).clone()} oninput={{let radius=radius.clone();Callback::from(move|e|radius.set(api::input(e)))}}/>{" m"}</span></label>}
        if radius_bad {<p class="error">{if tool == "ignite" {ex("Il raggio dell’innesco deve essere tra 40 e 2000 m.")} else {ex("Il raggio deve essere tra 10 e 5000 m.")}}</p>}
        if tool == "bstatus" {<label>{ex("Stato")}<select aria-label={ex("Stato edificio")} onchange={{let damage=damage.clone();Callback::from(move|e|damage.set(api::select(e)))}}><option value="destroyed" selected={*damage=="destroyed"}>{ex("Distrutto")}</option><option value="damaged" selected={*damage=="damaged"}>{ex("Danneggiato")}</option><option value="intact" selected={*damage=="intact"}>{ex("Integro")}</option></select></label>}
        if p.oncommand.is_none() {{when_picker(&c.state, &when)}}
    </Modal>}
}

/// "Use this command" in the scenario editor; otherwise "Queue" while the
/// action will wait for play, "Apply" when it acts at once.
fn apply_label(editor: bool, state: &Value, when: &When) -> &'static str {
    if editor {
        ex("Usa questo comando")
    } else if *when == When::Now || !started(state) && *when == When::Play {
        ex("Applica")
    } else {
        ex("Metti in coda")
    }
}
fn started(state: &Value) -> bool {
    matches!(text(&state["phase"]), "orders" | "resolving")
}
fn when_picker(state: &Value, when: &UseStateHandle<When>) -> Html {
    let onchange = {
        let when = when.clone();
        Callback::from(move |w| when.set(w))
    };
    html! {<WhenPicker value={(**when).clone()} {onchange} now_clock={text(&state["world"]["stats"]["clock"]).to_string()} started={started(state)}/>}
}

// ------------------------------------------------------------------ generic short-form modal

#[derive(Properties, PartialEq)]
pub struct FieldsProps {
    pub title: AttrValue,
    pub tip: AttrValue,
    pub endpoint: AttrValue,
    pub fields: Vec<Field>,
    #[prop_or(json!({}))]
    pub extra: Value,
    #[prop_or_default]
    pub arrays: Vec<&'static str>,
    /// Keys that must be non-empty.
    #[prop_or_default]
    pub required: Vec<&'static str>,
    #[prop_or_default]
    pub store: Option<String>,
    pub common: CommonProps,
}
#[function_component(FieldsModal)]
pub fn fields_modal(p: &FieldsProps) -> Html {
    let values = use_state(BTreeMap::<String, String>::new);
    let local_err = use_state(String::new);
    let val = |f: &Field| values.get(f.key).cloned().unwrap_or_else(|| f.init.clone());
    let missing = p.required.iter().any(|k| p.fields.iter().find(|f| f.key == *k).map(|f| val(f).trim().is_empty()).unwrap_or(false));
    let onapply = {
        let (fields, values, extra, arrays, ep, store, cb, local_err) =
            (p.fields.clone(), values.clone(), p.extra.clone(), p.arrays.clone(), p.endpoint.to_string(), p.store.clone(), p.common.action.clone(), local_err.clone());
        Callback::from(move |_| match fields_body(&fields, &values, &extra, &arrays) {
            Ok(body) => {
                local_err.set(String::new());
                cb.emit(match &store {
                    Some(k) => Action::store(&ep, Some(body), k),
                    None => Action::post(&ep, body),
                });
            }
            Err(e) => local_err.set(e),
        })
    };
    let set = |key: &'static str| {
        let values = values.clone();
        move |v: String| {
            let mut next = (*values).clone();
            next.insert(key.to_string(), v);
            values.set(next);
        }
    };
    let error = if local_err.is_empty() { p.common.error.clone() } else { AttrValue::from((*local_err).clone()) };
    html! {<Modal title={p.title.clone()} onclose={p.common.onclose.clone()} {onapply} apply_disabled={missing} busy={p.common.busy} done={p.common.done} {error}>
        <p class="muted">{p.tip.clone()}</p>
        {for p.fields.iter().map(|f| {
            let s = set(f.key);
            let v = val(f);
            let label = ex(f.label);
            match &f.kind {
                FieldKind::Text => html!{<label>{label}<input value={v} oninput={Callback::from(move|e|s(api::input(e)))}/></label>},
                FieldKind::Area => html!{<label>{label}<textarea rows="5" value={v} oninput={Callback::from(move|e|s(api::input(e)))}/></label>},
                FieldKind::Number => html!{<label>{label}<span class="ctl-unit"><input type="number" step="any" value={v} oninput={Callback::from(move|e|s(api::input(e)))}/>{f.unit}</span></label>},
                FieldKind::Check => { let on = v == "true"; html!{<label class="mt-check"><input type="checkbox" checked={on} onchange={Callback::from(move|_|s((!on).to_string()))}/>{label}</label>} }
                FieldKind::Multi(opts) => {
                    let chosen: Vec<String> = v.split(',').filter(|k| !k.is_empty()).map(String::from).collect();
                    html!{<fieldset class="ctl-multi"><legend>{label}</legend>{for opts.iter().map(|(k,n)| {
                        let (on, k2, chosen, s) = (chosen.contains(k), k.clone(), chosen.clone(), s.clone());
                        // "all" excludes the single roles, and the other way round.
                        let toggle = Callback::from(move |_| {
                            let mut next: Vec<String> = chosen.iter().filter(|c| **c != k2 && (k2 != "all" && *c != "all")).cloned().collect();
                            if !on { next.push(k2.clone()); }
                            s(next.join(","));
                        });
                        html!{<label class="mt-check"><input type="checkbox" checked={on} onchange={toggle}/>{n.clone()}</label>}
                    })}</fieldset>}
                }
                FieldKind::Select(opts) => html!{<label>{label}<select onchange={Callback::from(move|e|s(api::select(e)))}>{for opts.iter().map(|(k,n)|html!{<option value={k.clone()} selected={*k==v}>{n.clone()}</option>})}</select></label>},
            }
        })}
    </Modal>}
}

// ------------------------------------------------------------------ boundary conditions (wind + moisture)

const WIND_MAX_KMH: f64 = 100.;
const WIND_R: f64 = 90.;
#[derive(Properties, PartialEq)]
pub struct WeatherProps {
    pub title: AttrValue,
    /// Current direction the wind blows from, degrees clockwise from north.
    pub dir: f64,
    pub speed: f64,
    /// Fuel moisture, percent.
    pub moisture: f64,
    pub common: CommonProps,
    /// Hand the `SetWeather` command to the caller instead of posting it.
    #[prop_or_default]
    pub oncommand: Option<Callback<Value>>,
}
/// Boundary conditions: wind as a direction/speed vector editor, plus fuel moisture.
/// Wind vector: drag the arrow tip on the dial. The arrow points where the wind blows to.
#[function_component(WeatherModal)]
pub fn wind_modal(p: &WeatherProps) -> Html {
    let state = use_state(|| (p.dir.rem_euclid(360.).round(), p.speed.clamp(0., WIND_MAX_KMH).round()));
    let (dir, speed) = *state;
    let moisture = use_state(|| (p.moisture * 10.).round() / 10.);
    let when = use_state(|| When::Play);
    let to = (dir + 180.).to_radians();
    let len = speed / WIND_MAX_KMH * WIND_R;
    let (tx, ty) = (len * to.sin(), -len * to.cos());
    // The dial element itself: with Yew's event delegation `current_target` can be a child shape.
    let dial = use_node_ref();
    let pick = {
        let (state, dial) = (state.clone(), dial.clone());
        move |e: &PointerEvent| {
            let Some(svg) = dial.cast::<web_sys::Element>() else { return };
            let r = svg.get_bounding_client_rect();
            // The viewBox (200 units) is fitted into the smaller side and centred.
            let k = 200. / r.width().min(r.height()).max(1.);
            let x = (e.client_x() as f64 - r.left() - r.width() / 2.) * k;
            let y = (e.client_y() as f64 - r.top() - r.height() / 2.) * k;
            let to_deg = x.atan2(-y).to_degrees().rem_euclid(360.);
            let v = (x.hypot(y) / WIND_R * WIND_MAX_KMH).clamp(0., WIND_MAX_KMH).round();
            state.set(((to_deg + 180.).rem_euclid(360.).round(), v));
        }
    };
    let ondown = {
        let (pick, dial) = (pick.clone(), dial.clone());
        Callback::from(move |e: PointerEvent| {
            if let Some(el) = dial.cast::<web_sys::Element>() {
                let _ = el.set_pointer_capture(e.pointer_id());
            }
            pick(&e);
        })
    };
    // Dragging = the dial holds the pointer capture taken in `ondown`.
    let onmove = {
        let dial = dial.clone();
        Callback::from(move |e: PointerEvent| {
            if dial.cast::<web_sys::Element>().is_some_and(|el| el.has_pointer_capture(e.pointer_id())) {
                pick(&e);
            }
        })
    };
    let onapply = {
        let (cb, moisture, oncommand, onclose, when) = (p.common.action.clone(), moisture.clone(), p.oncommand.clone(), p.common.onclose.clone(), (*when).clone());
        Callback::from(move |_| {
            let cmd = json!({"type":"SetWeather","wind_dir_deg":dir,"wind_speed_kmh":speed,"moisture_pct":*moisture});
            match &oncommand {
                Some(f) => {
                    f.emit(cmd);
                    onclose.emit(());
                }
                None => cb.emit(Action::post("/api/master/command", command_body(cmd, &when))),
            }
        })
    };
    // Typed values; an unparsable field leaves the state untouched.
    let on_dir = {
        let state = state.clone();
        Callback::from(move |e: InputEvent| {
            if let Ok(d) = api::input(e).trim().parse::<f64>() {
                state.set((d.rem_euclid(360.), state.1));
            }
        })
    };
    let on_speed = {
        let state = state.clone();
        Callback::from(move |e: InputEvent| {
            if let Ok(v) = api::input(e).trim().parse::<f64>() {
                state.set((state.0, v.clamp(0., WIND_MAX_KMH)));
            }
        })
    };
    let on_moisture = {
        let moisture = moisture.clone();
        Callback::from(move |e: InputEvent| {
            if let Ok(m) = api::input(e).trim().parse::<f64>() {
                moisture.set(m.clamp(0., 100.));
            }
        })
    };
    let rings = [25., 50., 75., 100.];
    let apply_label = apply_label(p.oncommand.is_some(), &p.common.state, &when);
    let when_bad = p.oncommand.is_none() && matches!(&*when, When::At(t) if !crate::excon_queue::valid_clock(t));
    html! {<Modal title={p.title.clone()} onclose={p.common.onclose.clone()} {onapply} {apply_label} apply_disabled={when_bad} busy={p.common.busy} done={p.common.done} error={p.common.error.clone()}>
        <p class="muted">{ex("Trascina la punta della freccia: indica dove soffia il vento, la lunghezza è la velocità.")}</p>
        <svg class="wind-dial" ref={dial} viewBox="-100 -100 200 200" onpointerdown={ondown} onpointermove={onmove}>
            {for rings.iter().map(|&v| html!{<circle r={format!("{}", v / WIND_MAX_KMH * WIND_R)} class="wd-ring"/>})}
            <line x1="-95" y1="0" x2="95" y2="0" class="wd-ring"/><line x1="0" y1="-95" x2="0" y2="95" class="wd-ring"/>
            <text y="-91" class="wd-n">{"N"}</text><text x="93" y="3" class="wd-n">{"E"}</text><text y="99" class="wd-n">{"S"}</text><text x="-93" y="3" class="wd-n">{"O"}</text>
            if speed > 0. {
                <line x1="0" y1="0" x2={format!("{tx:.1}")} y2={format!("{ty:.1}")} class="wd-arrow"/>
                <circle cx={format!("{tx:.1}")} cy={format!("{ty:.1}")} r="6" class="wd-tip"/>
            }
            <circle r="3" class="wd-tip"/>
        </svg>
        <div class="wd-fields">
            <label>{ex("Direzione (gradi, da cui soffia)")}<span class="ctl-unit"><input type="number" step="any" value={format!("{dir:.0}")} oninput={on_dir}/>{" °"}</span></label>
            <label>{ex("Velocità")}<span class="ctl-unit"><input type="number" step="any" value={format!("{speed:.0}")} oninput={on_speed}/>{" km/h"}</span></label>
        </div>
        <label>{ex("Umidità del combustibile")}<span class="ctl-unit"><input type="number" step="any" min="0" max="100" value={moisture.to_string()} oninput={on_moisture}/>{" %"}</span></label>
        <p class="muted">{ex("Umidità del combustibile in percentuale (0–100).")}</p>
        if p.oncommand.is_none() {{when_picker(&p.common.state, &when)}}
    </Modal>}
}

// ------------------------------------------------------------------ knowledge (fog of war)

const LAYERS: [(&str, &str); 5] = [("fire", "Incendio"), ("buildings", "Edifici"), ("roads", "Strade"), ("lines", "Linee e lanci"), ("units", "Squadre")];
fn zone_names(stat: &Value) -> Vec<String> {
    let (c, r) = (stat["zones"]["cols"].as_u64().unwrap_or(0) as u8, stat["zones"]["rows"].as_u64().unwrap_or(0) as u8);
    (0..r).flat_map(|row| (0..c).map(move |col| format!("{}{}", (b'A' + col) as char, row + 1))).collect()
}
/// `names` plus the zones touching them (8-neighbourhood).
fn with_neighbours(stat: &Value, names: &[String]) -> BTreeSet<String> {
    let (cols, rows) = (stat["zones"]["cols"].as_i64().unwrap_or(0), stat["zones"]["rows"].as_i64().unwrap_or(0));
    let mut out = BTreeSet::new();
    for n in names {
        let b = n.as_bytes();
        let (Some(c), Ok(r)) = (b.first().map(|c| c.wrapping_sub(b'A') as i64), n.get(1..).unwrap_or("").parse::<i64>()) else { continue };
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
#[function_component(KnowledgeModal)]
pub fn knowledge_modal(p: &MapToolProps) -> Html {
    let c = &p.common;
    let role = use_state(String::new);
    let source = use_state(|| ex("Rapporto EXCON").to_string());
    let layers = use_state(|| LAYERS.iter().map(|l| l.0.to_string()).collect::<BTreeSet<_>>());
    let zones = use_state(BTreeSet::<String>::new);
    let roles = list(&c.stat["roles"]);
    let role_id = if role.is_empty() { roles.first().map(|r| text(&r["id"]).to_string()).unwrap_or_default() } else { (*role).clone() };
    let known: Vec<String> = list(&c.master["role_knowledge"][role_id.as_str()]["zones"]).iter().map(|z| text(z).to_string()).collect();
    let burning: Vec<String> = list(&c.state["world"]["burning_zones"]).iter().map(|z| text(z).to_string()).collect();
    let onmap = {
        let zones = zones.clone();
        Callback::from(move |pt: Value| {
            let z = text(&pt["zone"]).to_string();
            let mut next = (*zones).clone();
            if !next.remove(&z) {
                next.insert(z);
            }
            zones.set(next);
        })
    };
    let preset = |label: &'static str, set: BTreeSet<String>| {
        let zones = zones.clone();
        html! {<button type="button" onclick={Callback::from(move |_| zones.set(set.clone()))}>{ex(label)}</button>}
    };
    let ok = !zones.is_empty() && !layers.is_empty() && !source.trim().is_empty() && !role_id.is_empty();
    let onapply = {
        let (cb, role_id, source, zones, layers) = (c.action.clone(), role_id.clone(), source.clone(), zones.clone(), layers.clone());
        Callback::from(move |_| cb.emit(Action::post("/api/master/knowledge", json!({"role":role_id,"source":source.trim(),"zones":*zones,"layers":*layers}))))
    };
    let all = {
        let (cb, role_id, source) = (c.action.clone(), role_id.clone(), source.clone());
        Callback::from(move |_| cb.emit(Action::post("/api/master/knowledge", json!({"role":role_id,"source":source.trim(),"current_perimeter":true}))))
    };
    html! {<Modal title={tool_label("knowledge")} onclose={c.onclose.clone()} {onapply} apply_label={ex("Rivela le zone selezionate")} apply_disabled={!ok} busy={c.busy} done={c.done} error={c.error.clone()}
        map={pick_map(&c.stat, &c.state, onmap, None, zones.iter().cloned().collect(), known.clone())}
        extra_actions={html!{<button type="button" disabled={c.busy || source.trim().is_empty() || role_id.is_empty()} onclick={all}>{ex("Pubblica l’intero perimetro")}</button>}}>
        <p class="muted">{ex("Clicca le zone da rivelare: il clic le aggiunge o le toglie. Il ruolo conosce solo ciò che pubblichi, come fotografia al momento della consegna.")}</p>
        <label>{ex("Ruolo")}<select id="report-role" onchange={{let role=role.clone();Callback::from(move|e|role.set(api::select(e)))}}>{for roles.iter().map(|r|html!{<option value={text(&r["id"]).to_string()} selected={text(&r["id"])==role_id}>{text(&r["name"])}</option>})}</select></label>
        <p class="muted">{if known.is_empty() {ex("Questo ruolo non conosce ancora nessuna zona.").to_string()} else {format!("{} {}: {}", ex("Zone già note:"), known.len(), known.join(", "))}}</p>
        <fieldset><legend>{ex("Livelli da rivelare")}</legend>{for LAYERS.iter().map(|(id,label)|{let id=id.to_string();let layers=layers.clone();let checked=layers.contains(&id);html!{<label class="mt-check"><input type="checkbox" {checked} onchange={Callback::from(move |_|{let mut next=(*layers).clone();if !next.remove(&id){next.insert(id.clone());}layers.set(next);})}/>{ex(label)}</label>}})}</fieldset>
        <div class="row">{preset("Nessuna", BTreeSet::new())}{preset("Zone in fiamme", burning.iter().cloned().collect())}{preset("In fiamme + adiacenti", with_neighbours(&c.stat,&burning))}{preset("Tutta la mappa", zone_names(&c.stat).into_iter().collect())}</div>
        <p>{if zones.is_empty(){ex("Nessuna zona selezionata. Clicca la mappa.").to_string()}else{format!("{} {}: {}", zones.len(), ex("zone selezionate"), zones.iter().cloned().collect::<Vec<_>>().join(", "))}}</p>
        <label>{ex("Fonte del rapporto")}<input id="report-source" value={(*source).clone()} oninput={{let source=source.clone();Callback::from(move|e|source.set(api::input(e)))}}/></label>
    </Modal>}
}

// ------------------------------------------------------------------ read-only / admin panels

#[derive(Properties, PartialEq)]
pub struct PanelProps {
    pub tool: AttrValue,
    pub common: CommonProps,
    pub links: Value,
    pub catalogue: Value,
}
#[function_component(PanelModal)]
pub fn panel_modal(p: &PanelProps) -> Html {
    let c = &p.common;
    let (stat, state, master) = (&c.stat, &c.state, &c.master);
    let form = |title: &str, endpoint: &str, initial: Value| html! {<ApiForm title={title.to_string()} endpoint={endpoint.to_string()} {initial} action={c.action.clone()} busy={c.busy}/>};
    let body = match p.tool.as_str() {
        "links" => widgets::participant_links(&p.links, list(&stat["roles"])),
        "proposals" => html! {<>{if list(&master["proposals"]).is_empty() {html!{<p class="muted">{ex("Nessuna proposta.")}</p>}} else {html!{}}}
            {for list(&master["proposals"]).iter().rev().map(|proposal|html!{<Proposal key={proposal["id"].to_string()} proposal={proposal.clone()} generation={state["generation"].clone()} preview={c.responses.get(&format!("proposal:{}",proposal["id"])).cloned().unwrap_or_default()} busy={c.busy} action={c.action.clone()} roles={list(&c.stat["roles"]).to_vec()} common={Some(c.clone())}/>})}</>},
        "forecast" => html! {<Forecast stat={stat.clone()} state={state.clone()} draft={c.responses.get("forecast").cloned().unwrap_or_default()} action={c.action.clone()} busy={c.busy}/>},
        "missions" => html! {<>{widgets::details(ex("Stato operativo"),&state["world"]["operations"])}{widgets::details(ex("Missioni: stato, posizione, arrivo ed esiti"),&state["world"]["deployments"])}{widgets::details(ex("Risorse e impegni"),&state["resource_accounting"])}
            {for list(&state["world"]["allocations"]).iter().filter(|a|a["returned"]!=a["qty"]).map(|a|{let cb=c.action.clone();let id=a["id"].clone();let qty=a["qty"].as_u64().unwrap_or(0).saturating_sub(a["returned"].as_u64().unwrap_or(0));html!{<button disabled={c.busy||qty==0} onclick={Callback::from(move |_|cb.emit(Action::post("/api/master/command",json!({"type":"ReturnResource","allocation":id,"qty":qty}))))}>{crate::exfmt!("Conferma ritorno #{} · {} unità","Confirm return #{} · {} units",a["id"],qty)}</button>}})}</>},
        "limits" => form(ex("Limiti risorse · sostituzione completa"), "/api/master/resource-limits", json!({"limits":state["resource_limits"],"reason":""})),
        "references" => form(ex("Pubblica piani e procedure ai valutatori"), "/api/master/references", json!({"references":[]})),
        "msel" => html! {<>{for list(&stat["msel"]).iter().map(|i|{let id=text(&i["id"]).to_string();let cb=c.action.clone();html!{<details key={id.clone()}><summary>{crate::exfmt!("{} · {} · turno {}","{} · {} · turn {}",id,text(&i["title"]),i["turn"])}</summary><p>{text(&i["text"])}</p><p class="muted">{text(&i["objective"])}</p><button disabled={c.busy} onclick={Callback::from(move |_|cb.emit(Action::post(format!("/api/master/msel/{}/send",api::encode(&id)),json!({}))))}>{ex("Invia MSEL")}</button>{form(ex("Programma consegna"),"/api/master/schedules",json!({"msel_id":i["id"],"at_clock":"14:00"}))}</details>}})}
            {for list(&state["scheduled_injects"]).iter().map(|s|{let cb=c.action.clone();let id=s["id"].clone();html!{<div><p>{crate::render::render_value("", s)}</p><button disabled={c.busy} onclick={Callback::from(move |_|cb.emit(Action::post(format!("/api/master/schedules/{id}/cancel"),json!({"reason":ex("Annullamento EXCON")}))))}>{ex("Annulla consegna programmata")}</button></div>}})}</>},
        "design" => html! {<>{form(ex("Disegno formativo · modificabile nel briefing"),"/api/master/design",master["training_design"]["original"].as_object().map(|_|master["training_design"]["original"].clone()).unwrap_or(json!({"objectives":[],"phases":{},"decision_points":[]})))}
            {form(ex("Approva percorso o adattamento"),"/api/master/path",json!({"point":"","path":"","reason":"","adaptation":false}))}
            {widgets::details(ex("Disegno, condizioni e percorsi disponibili"),&master["training_design"])}</>},
        "log" => html! {<>{widgets::details(ex("Messaggi visibili a EXCON"),&master["messages"])}{widgets::details("Eventi",&master["log"])}</>},
        "json" => html! {<>
            <ApiForm title={ex("Comandi · anteprima prima dell’applicazione")} endpoint="/api/master/preview" initial={json!({"commands":[{"type":"Extinguish","area":"G2"}]})} button="Calcola anteprima" store={Some("intervention".to_string())} action={c.action.clone()} busy={c.busy}/>
            {c.responses.get("intervention").filter(|v|!v.is_null()).map(widgets::preview).unwrap_or_default()}
            {widgets::details(ex("Catalogo completo comandi e autorità"),&p.catalogue)}</>},
        "end" => html! {<><button disabled={c.busy||state["phase"]=="resolving"} onclick={{let cb=c.action.clone();Callback::from(move |_|cb.emit(Action::post("/api/master/end",json!({}))))}}>{ex("Concludi esercitazione")}</button>
            <button id="debriefdownload" disabled={c.busy} onclick={{let cb=c.action.clone();Callback::from(move |_|cb.emit(Action{path:"/api/debrief".into(),body:None,kind:ActionKind::Download}))}}>{ex("Scarica debrief JSON")}</button></>},
        _ => html! {},
    };
    html! {<Modal title={tool_label(&p.tool)} onclose={c.onclose.clone()} busy={c.busy} error={c.error.clone()}>
        <p class="muted">{ex(TOOLS.iter().find(|t| t.id == p.tool.as_str()).map(|t| t.tip).unwrap_or(""))}</p>{body}
    </Modal>}
}

// ------------------------------------------------------------------ the tab

#[derive(Properties, PartialEq)]
pub struct ControlloProps {
    pub stat: Value,
    pub state: Value,
    pub master: Value,
    pub links: Value,
    pub catalogue: Value,
    pub responses: BTreeMap<String, Value>,
    pub busy: bool,
    #[prop_or_default]
    pub error: AttrValue,
    #[prop_or_default]
    pub done: u64,
    pub action: Callback<Action>,
}
#[function_component(Controllo)]
pub fn controllo(p: &ControlloProps) -> Html {
    let query = use_state(String::new);
    let open = use_state(|| None::<String>);
    let onclose = {
        let open = open.clone();
        Callback::from(move |_| open.set(None))
    };
    let common = CommonProps {
        stat: p.stat.clone(),
        state: p.state.clone(),
        master: p.master.clone(),
        responses: p.responses.clone(),
        busy: p.busy,
        error: p.error.clone(),
        done: p.done,
        action: p.action.clone(),
        onclose: onclose.clone(),
    };
    let visible: Vec<&Tool> = TOOLS.iter().filter(|t| matches_query(t, &query)).collect();
    let on_enter = {
        let (open, first) = (open.clone(), visible.first().map(|t| t.id.to_string()));
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                if let Some(id) = &first {
                    open.set(Some(id.clone()));
                }
            }
        })
    };
    let w = &p.state["world"]["weather"];
    let num = |v: &Value, d: f64| format!("{}", v.as_f64().map(|x| (x * 10.).round() / 10.).unwrap_or(d));
    let phases: Vec<(String, String)> = [("early_warning", "Allerta precoce"), ("early_action", "Azione precoce"), ("escalation", "Escalation"), ("full_response", "Risposta completa")]
        .iter()
        .map(|&(k, n)| (k.to_string(), ex(n).to_string()))
        .collect();
    let mut senders = vec![("EXCON".to_string(), "EXCON".to_string())];
    senders.extend(list(&p.stat["excon_roles"]).iter().map(|r| (text(r).to_string(), text(r).to_string())));
    let mut recipients = vec![("all".to_string(), ex("Tutti i player").to_string())];
    recipients.extend(list(&p.stat["roles"]).iter().map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string())));
    let cm = common.clone();
    let fields_modal = |title: &str, tip: &str, endpoint: &str, fields: Vec<Field>, extra: Value| {
        html! {<FieldsModal title={title.to_string()} tip={tip.to_string()} endpoint={endpoint.to_string()} {fields} {extra} common={cm.clone()}/>}
    };
    let modal = match open.as_deref() {
        None => html! {},
        Some(id @ ("ignite" | "line" | "drop" | "extzone" | "extcircle" | "extarea" | "evac" | "evacborgo" | "warn" | "drone" | "bstatus")) => {
            html! {<MapToolModal key={id.to_string()} tool={id.to_string()} common={common.clone()}/>}
        }
        Some("knowledge") => html! {<KnowledgeModal tool="knowledge" common={common.clone()}/>},
        Some("weather") => html! {<WeatherModal
            title={tool_label("weather").to_string()}
            dir={w["wind_dir_deg"].as_f64().unwrap_or(0.)}
            speed={w["wind_speed_kmh"].as_f64().unwrap_or(10.)}
            moisture={w["moisture_pct"].as_f64().unwrap_or(10.)}
            common={common.clone()}/>},
        Some("correct") => fields_modal(
            tool_label("correct"), ex("Annulla l’ultimo intervento fisico. Richiede clock fermo."), "/api/master/correct-intervention",
            vec![Field::new("reason", "Motivazione", FieldKind::Text, "")], json!({}),
        ),
        Some("clock") => fields_modal(
            tool_label("clock"), ex("Il clock è indipendente dal turno: fasi e turni conservano lo stato accumulato."), "/api/master/clock",
            vec![
                Field::new("running", "Clock in marcia", FieldKind::Check, (p.state["clock_control"]["running"] == true).to_string()),
                Field::new("ratio", "Rapporto simulato/reale", FieldKind::Number, num(&p.state["clock_control"]["ratio"], 60.)).unit(" ×"),
            ],
            json!({"reason": ex("EXCON controlla il clock")}),
        ),
        Some("speed") => fields_modal(
            tool_label("speed"), ex("0 = massima velocità; 60 = un minuto simulato al secondo."), "/api/master/settings",
            vec![Field::new("speed", "Fattore di velocità", FieldKind::Number, num(&p.state["speed"], 0.)).unit(" ×")], json!({}),
        ),
        Some("advance") => fields_modal(
            tool_label("advance"), ex("Salta in avanti nel tempo simulato, fuori dal ciclo dei turni."), "/api/master/command",
            vec![Field::new("minutes", "Minuti", FieldKind::Number, "10").unit(" min")], json!({"type":"AdvanceTime"}),
        ),
        Some("phase") => fields_modal(
            tool_label("phase"), ex("Cambia la fase dell’esercitazione."), "/api/master/phase",
            vec![
                Field::new("phase", "Fase", FieldKind::Select(phases), text(&p.state["exercise_phase"])),
                Field::new("reason", "Motivazione", FieldKind::Text, ex("Cambio fase EXCON")),
            ],
            json!({}),
        ),
        Some("rollback") => fields_modal(
            tool_label("rollback"), ex("Riporta l’esercitazione all’inizio del turno indicato."), "/api/master/rollback",
            vec![Field::new("turn", "Turno", FieldKind::Number, p.state["turn"].as_u64().unwrap_or(1).to_string())], json!({}),
        ),
        Some("inject") => html! {<FieldsModal title={tool_label("inject")} tip={ex("Il messaggio non cambia la fisica: arriva ai destinatari come inject.")} endpoint="/api/master/inject"
            fields={vec![
                Field::new("recipients", "Destinatari", FieldKind::Multi(recipients), "all"),
                Field::new("title", "Titolo", FieldKind::Text, "Inject"),
                Field::new("sender", "Mittente", FieldKind::Select(senders), "EXCON"),
                Field::new("text", "Testo", FieldKind::Area, ""),
            ]} extra={json!({"observation":{}})} required={vec!["text", "recipients"]} common={cm.clone()}/>},
        Some("prompt") => html! {<FieldsModal title={tool_label("prompt")} tip={ex("L’AI propone azioni; nulla viene eseguito senza la tua approvazione.")} endpoint="/api/master/prompt"
            fields={vec![Field::new("text", "Richiesta", FieldKind::Area, "")]} required={vec!["text"]} common={cm.clone()}/>},
        Some(id) => html! {<PanelModal tool={id.to_string()} common={common.clone()} links={p.links.clone()} catalogue={p.catalogue.clone()}/>},
    };
    let tool_buttons = |group: &str| {
        visible.iter().filter(|t| t.group == group).map(|t| {
            let (open, id) = (open.clone(), t.id.to_string());
            let tip = ex(t.tip);
            html! {<li key={t.id}><button type="button" class="tool-item" data-tool={t.id} title={tip} aria-label={format!("{}: {}", ex(t.label), tip)} onclick={Callback::from(move |_| open.set(Some(id.clone())))}><span class="ti-icon" aria-hidden="true">{t.icon}</span><span class="ti-label">{ex(t.label)}</span></button></li>}
        }).collect::<Html>()
    };
    html! {<main class="page controllo">
        <section class="card ctl-map">
            <SituationMap stat={p.stat.clone()} view={p.state["world"].clone()}/>
            {widgets::stats(&p.state["world"]["stats"])}
        </section>
        <aside class="card ctl-tools" aria-label={ex("Azioni disponibili")}>
            <ExconQueue master={p.master.clone()} state={p.state.clone()} busy={p.busy} action={p.action.clone()}/>
            <input type="search" id="tool-search" class="ctl-search" placeholder={ex("Cerca un’azione…")} aria-label={ex("Cerca un’azione")} value={(*query).clone()}
                oninput={{let query=query.clone();Callback::from(move|e|query.set(api::input(e)))}} onkeydown={on_enter}/>
            if visible.is_empty() {<p class="muted">{ex("Nessuna azione corrisponde alla ricerca.")}</p>}
            {for GROUPS.iter().filter(|g| visible.iter().any(|t| t.group == **g)).map(|g| html!{<section class="ctl-group" key={*g}><h3>{ex(*g)}</h3><ul>{tool_buttons(g)}</ul></section>})}
        </aside>
        {modal}
    </main>}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn click(x: f64, y: f64, zone: &str) -> Value {
        json!({"x": x, "y": y, "zone": zone})
    }
    #[test]
    fn fire_line_needs_two_points_and_has_api_shape() {
        let stat = json!({});
        let p = CmdParams::default();
        assert_eq!(build_command("line", &[click(100., 100., "A1")], &p, &stat), None);
        let cmd = build_command("line", &[click(100., 100., "A1"), click(400., 100., "A1")], &p, &stat).unwrap();
        assert_eq!(cmd, json!({"type":"CutFireLine","from":{"x":100.0,"y":100.0},"to":{"x":400.0,"y":100.0}}));
    }
    #[test]
    fn drawn_area_needs_three_vertices_and_a_surface() {
        let stat = json!({});
        let p = CmdParams::default();
        let a = click(0., 0., "A1");
        let b = click(1000., 0., "A1");
        let c = click(0., 1000., "A1");
        assert_eq!(build_command("extarea", &[a.clone(), b.clone()], &p, &stat), None);
        assert_eq!(build_command("extarea", &[a.clone(), b.clone(), click(500., 0., "A1")], &p, &stat), None);
        let cmd = build_command("extarea", &[a.clone(), b.clone(), c.clone()], &p, &stat).unwrap();
        assert_eq!(cmd, json!({"type":"ExtinguishArea","polygon":[{"x":0.0,"y":0.0},{"x":1000.0,"y":0.0},{"x":0.0,"y":1000.0}]}));
        assert!((polygon_ha(&[a, b, c]) - 50.).abs() < 1e-9);
        assert!(points_needed("extarea") > 3);
    }
    #[test]
    fn map_tools_payloads() {
        let stat = json!({"buildings":[{"id":7,"pos":[500.0,500.0]}]});
        let p = CmdParams::default();
        let c = click(510., 495., "B2");
        assert_eq!(build_command("extzone", &[c.clone()], &p, &stat).unwrap()["area"], json!("B2"));
        assert_eq!(build_command("warn", &[c.clone()], &p, &stat).unwrap()["zone"], json!("B2"));
        assert_eq!(build_command("drone", &[c.clone()], &p, &stat).unwrap(), json!({"type":"MonitorArea","zones":["B2"]}));
        assert_eq!(build_command("drop", &[c.clone()], &p, &stat).unwrap()["at"], json!({"x":510.0,"y":495.0}));
        assert_eq!(build_command("ignite", &[c.clone()], &p, &stat).unwrap()["radius_m"], json!(80.0));
        assert_eq!(build_command("bstatus", &[c.clone()], &p, &stat).unwrap()["building"], json!("#7"));
        assert_eq!(build_command("bstatus", &[click(5000., 5000., "H8")], &p, &stat), None);
        let mut with_id = c;
        with_id["building_id"] = json!(3);
        assert_eq!(build_command("bstatus", &[with_id], &p, &stat).unwrap()["building"], json!("#3"));
    }
    #[test]
    fn edited_commands_start_from_their_own_picks() {
        let stat = json!({"zones":{"cols":8,"rows":8,"size_m":750.0},"buildings":[{"id":7,"pos":[500.0,500.0]}]});
        for cmd in [
            json!({"type":"CutFireLine","from":{"x":100.0,"y":100.0},"to":{"x":400.0,"y":100.0}}),
            json!({"type":"WaterDrop","at":{"x":510.0,"y":495.0}}),
            json!({"type":"Ignite","at":{"x":510.0,"y":495.0},"radius_m":120.0}),
            json!({"type":"Extinguish","area":{"at":{"x":510.0,"y":495.0},"radius_m":150.0}}),
            json!({"type":"Extinguish","area":"B2"}),
            json!({"type":"ExtinguishArea","polygon":[{"x":100.0,"y":100.0},{"x":900.0,"y":100.0},{"x":500.0,"y":800.0}]}),
            json!({"type":"EvacuateZone","zone":"F4"}),
            json!({"type":"IssueWarning","zone":"F4"}),
            json!({"type":"MonitorArea","zones":["F4"]}),
            json!({"type":"SetBuildingStatus","building":"#7","damage":"destroyed"}),
        ] {
            let tool = tool_for(&cmd).unwrap();
            let radius = cmd["radius_m"].as_f64().or(cmd["area"]["radius_m"].as_f64()).unwrap_or(80.);
            let p = CmdParams { radius_m: radius, damage: "destroyed".into() };
            assert_eq!(build_command(tool, &initial_points(&cmd, &stat), &p, &stat).as_ref(), Some(&cmd), "{cmd}");
        }
        // Named places and zone edges keep the parameter fields.
        assert_eq!(tool_for(&json!({"type":"CutFireLine","along":"F4:N"})), None);
        assert!(initial_points(&json!({"type":"WaterDrop","at":"Pineta"}), &stat).is_empty());
        assert_eq!(tool_for(&json!({"type":"DeployEngine","at":{"x":1.0,"y":2.0}})), None);
        let kept = keep_extra_fields(&json!({"type":"EvacuateZone","zone":"F4","destination":"Palestra"}), &json!({"type":"EvacuateZone","zone":"G4"}));
        assert_eq!(kept, json!({"type":"EvacuateZone","zone":"G4","destination":"Palestra"}));
    }
    #[test]
    fn fields_body_types_and_arrays() {
        let fields = vec![
            Field::new("recipients", "d", FieldKind::Text, "all"),
            Field::new("wind_speed_kmh", "v", FieldKind::Number, "12.5"),
            Field::new("running", "r", FieldKind::Check, "true"),
        ];
        let body = fields_body(&fields, &BTreeMap::new(), &json!({"type":"SetWeather"}), &["recipients"]).unwrap();
        assert_eq!(body, json!({"type":"SetWeather","recipients":["all"],"wind_speed_kmh":12.5,"running":true}));
        let mut bad = BTreeMap::new();
        bad.insert("wind_speed_kmh".to_string(), "x".to_string());
        assert!(fields_body(&fields, &bad, &json!({}), &[]).is_err());
    }
}
