use crate::{
    api::{self, list, pretty, text},
    map::SituationMap,
    widgets::{self, ApiForm, Proposal},
    Action, ActionKind,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct AdvancedProps {
    pub stat: Value,
    pub state: Value,
    pub master: Value,
    pub links: Value,
    pub catalogue: Value,
    pub responses: BTreeMap<String, Value>,
    pub busy: bool,
    pub action: Callback<Action>,
}
#[function_component(Advanced)]
pub fn advanced(props: &AdvancedProps) -> Html {
    let p = props;
    let form = |title: &str, endpoint: &str, initial: Value| html! {<ApiForm title={title.to_string()} endpoint={endpoint.to_string()} {initial} action={p.action.clone()} busy={p.busy}/>};
    html! {<main class="page advanced-grid">
        <section>
        <section class="card"><h2>{crate::i18n::ex("Collegamenti partecipanti")}</h2>{widgets::participant_links(&p.links,list(&p.stat["roles"]))}</section>
        </section>
        <section>
        <section class="card"><h2>{crate::i18n::ex("Proposte da revisionare")}</h2>{for list(&p.master["proposals"]).iter().rev().map(|proposal|html!{<Proposal key={proposal["id"].to_string()} proposal={proposal.clone()} generation={p.state["generation"].clone()} preview={p.responses.get(&format!("proposal:{}",proposal["id"])).cloned().unwrap_or_default()} busy={p.busy} action={p.action.clone()}/>})}</section>
        {form(crate::i18n::ex("Clock indipendente dal turno"),"/api/master/clock",json!({"running":false,"ratio":60,"reason":crate::i18n::ex("EXCON controlla il clock")}))}
        <p class="muted">{crate::i18n::ex("running = true avvia il clock; false lo ferma. Cambi di fase e turni conservano lo stato accumulato.")}</p>
        {form(crate::i18n::ex("Fase di esercitazione"),"/api/master/phase",json!({"phase":text(&p.state["exercise_phase"]),"reason":""}))}
        {form(crate::i18n::ex("Velocità di risoluzione"),"/api/master/settings",json!({"speed":0}))}
        {form(crate::i18n::ex("Rollback all’inizio del turno"),"/api/master/rollback",json!({"turn":p.state["turn"]}))}
        <section class="card"><h2>{crate::i18n::ex("Fine ed evidenze")}</h2><button disabled={p.busy||p.state["phase"]=="resolving"} onclick={{let cb=p.action.clone();Callback::from(move |_|cb.emit(Action::post("/api/master/end",json!({}))))}}>{crate::i18n::ex("Concludi esercitazione")}</button><button id="debriefdownload" disabled={p.busy} onclick={{let cb=p.action.clone();Callback::from(move |_|cb.emit(Action{path:"/api/debrief".into(),body:None,kind:ActionKind::Download}))}}>{crate::i18n::ex("Scarica debrief JSON")}</button></section>
        <ApiForm title={crate::i18n::ex("Comandi · anteprima prima dell’applicazione")} endpoint="/api/master/preview" initial={json!({"commands":[{"type":"Extinguish","area":"G2"}]})} button="Calcola anteprima" store={Some("intervention".to_string())} action={p.action.clone()} busy={p.busy}/>
        {widgets::details(crate::i18n::ex("Catalogo completo comandi e autorità"),&p.catalogue)}
        {form(crate::i18n::ex("Prompt EXCON · propone senza eseguire"),"/api/master/prompt",json!({"text":""}))}
        {form(crate::i18n::ex("Inject personalizzato e rilevazione opzionale"),"/api/master/inject",json!({"title":"Inject","sender":"EXCON","recipients":["all"],"text":"","observation":{}}))}
        {form(crate::i18n::ex("Pubblica perimetro a un ruolo"),"/api/master/knowledge",json!({"role":"mayor","source":"","current_perimeter":true}))}
        <section class="card"><h2>{crate::i18n::ex("MSEL e programmazione")}</h2>{for list(&p.stat["msel"]).iter().map(|i|{let id=text(&i["id"]).to_string();let cb=p.action.clone();html!{<details key={id.clone()}><summary>{crate::exfmt!("{} · {} · turno {}","{} · {} · turn {}",id,text(&i["title"]),i["turn"])}</summary><p>{text(&i["text"])}</p><p class="muted">{text(&i["objective"])}</p><button disabled={p.busy} onclick={Callback::from(move |_|cb.emit(Action::post(format!("/api/master/msel/{}/send",api::encode(&id)),json!({}))))}>{crate::i18n::ex("Invia MSEL")}</button>{form(crate::i18n::ex("Programma consegna"),"/api/master/schedules",json!({"msel_id":i["id"],"at_clock":"14:00"}))}</details>}})}
        {for list(&p.state["scheduled_injects"]).iter().map(|s|{let cb=p.action.clone();let id=s["id"].clone();html!{<div><p>{pretty(s)}</p><button disabled={p.busy} onclick={Callback::from(move |_|cb.emit(Action::post(format!("/api/master/schedules/{id}/cancel"),json!({"reason":crate::i18n::ex("Annullamento EXCON")}))))}>{crate::i18n::ex("Annulla consegna programmata")}</button></div>}})}</section>
        {form(crate::i18n::ex("Limiti risorse · sostituzione completa"),"/api/master/resource-limits",json!({"limits":p.state["resource_limits"],"reason":""}))}
        {form(crate::i18n::ex("Disegno formativo · modificabile nel briefing"),"/api/master/design",p.master["training_design"]["original"].as_object().map(|_|p.master["training_design"]["original"].clone()).unwrap_or(json!({"objectives":[],"phases":{},"decision_points":[]})))}
        {form(crate::i18n::ex("Approva percorso o adattamento"),"/api/master/path",json!({"point":"","path":"","reason":"","adaptation":false}))}
        {widgets::details(crate::i18n::ex("Disegno, condizioni e percorsi disponibili"),&p.master["training_design"])}
        {form(crate::i18n::ex("Pubblica piani e procedure ai valutatori"),"/api/master/references",json!({"references":[]}))}
        <section class="card"><h2>{crate::i18n::ex("Messaggi e log EXCON")}</h2>{widgets::details(crate::i18n::ex("Messaggi visibili a EXCON"),&p.master["messages"])}{widgets::details("Eventi",&p.master["log"])}</section>
        </section>
        if !cfg!(feature = "standalone") {<crate::server_settings::ServerSettings revision={p.state["generation"].as_u64().unwrap_or(0)}/>}
    </main>}
}

#[derive(Properties, PartialEq)]
pub struct ForecastProps {
    pub stat: Value,
    pub state: Value,
    pub draft: Value,
    pub action: Callback<Action>,
    pub busy: bool,
}
#[function_component(Forecast)]
pub fn forecast(props: &ForecastProps) -> Html {
    let members = use_state(|| "4".to_string());
    let horizons = use_state(|| "60,180".to_string());
    let direction = use_state(|| "10".to_string());
    let speed = use_state(|| "0.1".to_string());
    let bulletin = use_state(|| "[]".to_string());
    let source = use_state(String::new);
    let reviewed = use_state(|| false);
    let recipients = use_state(BTreeSet::<String>::new);
    let error = use_state(String::new);
    {
        let reviewed = reviewed.clone();
        use_effect_with(props.draft["id"].clone(), move |_| reviewed.set(false));
    }
    let draft = &props.draft;
    let valid = !draft.is_null() && draft["generation"] == props.state["generation"];
    let compute = {
        let cb = props.action.clone();
        let members = members.clone();
        let horizons = horizons.clone();
        let direction = direction.clone();
        let speed = speed.clone();
        let bulletin = bulletin.clone();
        let error = error.clone();
        Callback::from(move |_| {
            let body = (|| -> Result<Value, String> {
                let horizons = horizons
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .parse::<u32>()
                            .map(|v| v * 60)
                            .map_err(|_| crate::i18n::ex("Orizzonti in minuti non validi").to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(
                    json!({"seed":1,"members":members.parse::<u32>().map_err(|_|crate::i18n::ex("Gruppi meteo non validi"))?,"horizons_s":horizons,"direction_spread_deg":direction.parse::<f64>().map_err(|_|crate::i18n::ex("Direzione non valida"))?,"speed_spread_fraction":speed.parse::<f64>().map_err(|_|crate::i18n::ex("Velocità non valida"))?,"bulletin":serde_json::from_str::<Value>(&bulletin).map_err(|e|e.to_string())?}),
                )
            })();
            match body {
                Ok(body) => {
                    error.set(String::new());
                    cb.emit(Action::store(
                        "/api/master/forecast",
                        Some(body),
                        "forecast",
                    ));
                }
                Err(e) => error.set(e),
            }
        })
    };
    let mut view = props.state["world"].clone();
    view["forecast_products"] = json!([draft["forecast"]]);
    let field = |label: &str, value: &UseStateHandle<String>| {
        let value = value.clone();
        let cb = value.clone();
        html! {<label>{label}<input value={(*value).clone()} oninput={Callback::from(move |e|cb.set(api::input(e)))}/></label>}
    };
    html! {<section class="card" id="forecast-review"><h2>{crate::i18n::ex("Revisione previsione EXCON")}</h2><p class="muted">{crate::i18n::ex("Proiezione separata dallo stato corrente. Conserva gli interventi presenti, esclude il lavoro futuro delle squadre. Ferma il clock prima del calcolo.")}</p>
        {field(crate::i18n::ex("Gruppi meteo"),&members)}{field(crate::i18n::ex("Orizzonti (minuti crescenti)"),&horizons)}{field(crate::i18n::ex("Perturbazione direzione (gradi)"),&direction)}{field(crate::i18n::ex("Perturbazione velocità (frazione)"),&speed)}<label>{crate::i18n::ex("Bollettino meteo JSON (tempi relativi in secondi)")}<textarea value={(*bulletin).clone()} oninput={{let bulletin=bulletin.clone();Callback::from(move|e|bulletin.set(api::input(e)))}}/></label>
        <button data-compute="" disabled={props.busy||props.state["clock_control"]["running"]==true||props.state["phase"]=="resolving"} onclick={compute}>{crate::i18n::ex("Calcola bozza privata")}</button><p class="error">{&*error}</p>
        if !draft.is_null(){<div data-review=""><p>{crate::exfmt!("Emessa a t={}s · {}","Issued at t={}s · {}",draft["forecast"]["issued_s"],text(&draft["forecast"]["assumptions"]))}</p>
        <p>{if valid{crate::i18n::ex("Bozza privata pronta: esamina gli orizzonti prima della pubblicazione.")}else{crate::i18n::ex("L’esercitazione è cambiata: calcola e revisiona una nuova previsione.")}}</p>
        <SituationMap id="forecast-map" stat={props.stat.clone()} view={view}/>
        {for list(&draft["forecast"]["horizons"]).iter().map(|h|{let area=list(&h["probability"]).iter().filter(|p|p.as_f64().unwrap_or(0.)>=0.5).count() as f64*props.stat["grid"]["cellsize"].as_f64().unwrap_or(0.).powi(2)/10000.;html!{<p>{crate::exfmt!("+{} min · area raggiunta ≥50%: {area:.1} ha","+{} min · area reached ≥50%: {area:.1} ha",h["after_s"].as_u64().unwrap_or(0)/60)}</p>}})}
        <fieldset><legend>{crate::i18n::ex("Destinatari selezionati")}</legend>{for list(&props.stat["roles"]).iter().map(|r|{let id=text(&r["id"]).to_string();let recipients=recipients.clone();let checked=recipients.contains(&id);html!{<label><input type="checkbox" data-role={id.clone()} {checked} onchange={Callback::from(move |_|{let mut next=(*recipients).clone();if !next.remove(&id){next.insert(id.clone());}recipients.set(next);})}/>{text(&r["name"])}</label>}})}</fieldset>
        {field(crate::i18n::ex("Fonte revisionata e motivazione"),&source)}
        <label><input type="checkbox" data-reviewed="" checked={*reviewed} onchange={{let reviewed=reviewed.clone();Callback::from(move |_|reviewed.set(!*reviewed))}}/>{crate::i18n::ex(" Ho esaminato la bozza e le sue assunzioni.")}</label>
        <button data-publish="" class="primary" disabled={props.busy||!valid||!*reviewed||recipients.is_empty()||source.trim().is_empty()} onclick={{let cb=props.action.clone();let draft=draft.clone();let source=source.clone();let recipients=recipients.clone();let reviewed=reviewed.clone();Callback::from(move |_|{cb.emit(Action::post("/api/master/forecast/publish",json!({"id":draft["id"],"generation":draft["generation"],"roles":*recipients,"source":source.trim()})));reviewed.set(false);})}}>{crate::i18n::ex("Pubblica previsione revisionata")}</button>
        </div>}
    </section>}
}
