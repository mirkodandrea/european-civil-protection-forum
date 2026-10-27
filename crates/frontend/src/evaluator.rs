//! Read-only evidence UI. It never constructs operational POST requests.
use crate::{
    api::{self, list, text},
    map::SituationMap,
    widgets, Action, ActionKind,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct EvaluatorProps {
    pub stat: Value,
    pub state: Value,
    pub evidence: Value,
    pub assessment: Value,
    pub busy: bool,
    pub action: Callback<Action>,
    pub onfilter: Callback<String>,
}
#[function_component(Evaluator)]
pub fn evaluator(props: &EvaluatorProps) -> Html {
    let filters = use_state(BTreeMap::<String, String>::new);
    let offset = use_state(|| 0usize);
    let query = query(&filters, *offset);
    {
        let cb = props.onfilter.clone();
        use_effect_with(query.clone(), move |query| cb.emit(query.clone()));
    }
    let v = &props.evidence;
    let assessment = &props.assessment;
    let filter = |key: &str, label: &str, options: Vec<(String, String)>| {
        let key = key.to_string();
        let value = filters.get(&key).cloned().unwrap_or_default();
        let chosen = value.clone();
        let filters = filters.clone();
        let offset = offset.clone();
        let input = options.is_empty();
        let onchange = {
            let key = key.clone();
            let filters = filters.clone();
            let offset = offset.clone();
            Callback::from(move |e: Event| {
                use yew::TargetCast;
                let value = if input {
                    e.target_unchecked_into::<web_sys::HtmlInputElement>()
                        .value()
                } else {
                    api::select(e)
                };
                let mut next = (*filters).clone();
                next.insert(key.clone(), value);
                filters.set(next);
                offset.set(0);
            })
        };
        html! {<label>{label} if input{<input id={key} {value} {onchange}/>}else{<select key={chosen.clone()} id={key} {value} {onchange}><option value="" selected={chosen.is_empty()}>{"Tutti"}</option>{for options.into_iter().map(|(id,label)|html!{<option selected={chosen==id} value={id.clone()}>{label}</option>})}</select>}</label>}
    };
    let opts = |v: &[&str]| {
        v.iter()
            .map(|s| (s.to_string(), s.replace('_', " ")))
            .collect::<Vec<_>>()
    };
    let mut roles = list(&props.stat["roles"])
        .iter()
        .map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string()))
        .collect::<Vec<_>>();
    roles.push(("master".into(), "EXCON".into()));
    html! {<main class="page advanced-grid evaluator">
        <section><section class="card"><h2>{"Situazione autorevole · sola lettura"}</h2><p id="context">{format!("{} · turno {} · {} / {}",text(&v["context"]["clock"]),v["context"]["turn"],text(&v["context"]["phase"]),text(&v["context"]["exercise_phase"]))}</p><SituationMap stat={props.stat.clone()} view={props.state["world"].clone()}/>{widgets::stats(&v["context"]["stats"])}{widgets::details("Risorse: limiti, disponibilità e impegni",&json!({"limits":v["resource_limits"],"ledger":v["resource_accounting"]}))}</section>
        <section class="card"><h2>{"Missioni effettive"}</h2><p class="muted">{"Una partenza non è un completamento: consulta arrivo, stato ed esito registrati."}</p>{widgets::details("Missioni e tempi",&v["deployments"])}</section>
        <section class="card"><h2>{"Partecipanti"}</h2>{for list(&v["role_cards"]).iter().map(|c|html!{<div class="card"><h3>{text(&c["player"]["name"])}</h3><p>{format!("Ultimo inject: {} · risposta attesa: {}",text(&c["last_inject"]["clock"]),c["awaiting_reply"])}</p>{crate::render::render_value("",&c["counts"])}{widgets::details("Evidenze del ruolo",c)}</div>})}</section>
        <section class="card"><h2>{"Disegno, percorsi e scostamenti"}</h2>{widgets::details("Disegno formativo e percorso corrente",&v["training_design"])}</section>
        <section class="card"><h2>{"Obiettivi formativi"}</h2>{for list(&v["objectives"]).iter().map(|o|html!{<details key={o["msel_id"].to_string()}><summary>{format!("{} · {}",text(&o["msel_id"]),text(&o["title"]))}</summary><p>{text(&o["objective"])}</p><p>{format!("Turno previsto {} · {} consegne effettive",o["planned_turn"],list(&o["deliveries"]).len())}</p>{crate::render::render_with_raw("",&o["deliveries"])}</details>})}<h2>{"Piani e procedure di riferimento"}</h2>{for list(&v["reference_materials"]).iter().map(evidence)}</section>
        <section class="card"><button id="export" disabled={props.busy} onclick={{let cb=props.action.clone();Callback::from(move |_|cb.emit(Action{path:"/api/debrief".into(),body:None,kind:ActionKind::Download}))}}>{"Scarica debrief completo"}</button></section>
        </section>
        <section><section class="card"><h2>{"Filtri delle evidenze"}</h2><div class="filters">{filter("role","Player",roles)}{filter("inject","ID consegna inject",vec![])}{filter("turn","Turno dichiarazione",vec![])}{filter("phase","Fase dichiarazione",opts(&["briefing","orders","resolving","debrief"]))}{filter("exercise_phase","Fase formativa",opts(&["early_warning","early_action","escalation","full_response"]))}{filter("status","Stato decisione",opts(&["parsing","pending","needs_clarification","approved","rejected","superseded","applied","failed","rolled_back"]))}{filter("kind","Tipo evento",vec![])}</div><p class="muted">{"Le catene mantengono la cronologia completa. I filtri della timeline si riferiscono alla fase e al turno di ciascun evento."}</p></section>
        <section class="card"><h2>{"Valutazione basata sulle evidenze"}</h2><p class="muted">{"Interpretazione AI facoltativa per revisione umana. Non invia messaggi e non modifica l’esercitazione."}</p><button id="assess" disabled={props.busy} onclick={{let cb=props.action.clone();let query=query.clone();Callback::from(move |_|cb.emit(Action::store(format!("/api/evaluator/assessment?{query}"),None,"assessment")))}}>{"Valuta evidenze filtrate"}</button>
        if !assessment.is_null(){<div id="assessment"><h3>{"Valutazione derivata · richiede revisione umana"}</h3><p>{format!("Snapshot generazione {} · {} record omessi · catturato a {} ms",assessment["snapshot"]["generation"],assessment["snapshot"]["omitted_records"],assessment["captured_wall_ms"])}</p><p>{text(&assessment["error"])}</p>
        {for list(&assessment["snapshot"]["limitations"]).iter().map(|l|html!{<p class="muted">{text(l)}</p>})}
        {for list(&assessment["derived_assessment"]["findings"]).iter().map(|f|html!{<div class="card"><b>{text(&f["category"])}</b><p>{text(&f["assessment"])}</p><p>{format!("Incertezza: {}",text(&f["uncertainty"]))}</p><p>{format!("Domanda per il debrief: {}",text(&f["debrief_question"]))}</p>{for list(&f["evidence"]).iter().map(|id|html!{<p><a href={format!("#assessment-{}",text(id))}>{text(id)}</a></p>})}</div>})}
        {widgets::details("Filtri catturati",&assessment["snapshot"]["filters"])}
        {for assessment["snapshot"]["evidence"].as_object().into_iter().flatten().map(|(id,value)|html!{<details id={format!("assessment-{id}")}><summary>{id}</summary>{crate::render::render_with_raw("",value)}</details>})}</div>}
        </section>
        <section class="card"><h2>{"Inject → risposta → approvazione → esecuzione"}</h2>{for list(&v["chains"]).iter().map(|c|{let p=&c["proposal"];html!{<details key={format!("proposal:{}",p["id"])} class="card" data-record={format!("proposal-{}",p["id"])}><summary>{format!("#{} · {} · {} · turno {}",p["id"],text(&p["source"]["role"]),api::status(text(&p["status"])),p["turn"])}</summary><p>{text(&p["original"])}</p><p>{text(&p["translation_en"])}</p>{evidence(&p["evidence"])}<p>{format!("Risposta dopo consegna: {} ms reali / {} s simulati · coda EXCON: {} ms / {} s",c["reply_delay_wall_ms"],c["reply_delay_sim_s"],c["master_queue_wall_ms"],c["master_queue_sim_s"])}</p><h3>{"Cronologia, decisione ed esiti effettivi"}</h3>{crate::render::render_with_raw("",&json!({"history":p["history"],"decision":p["decision"],"executions":p["executions"],"narrative":p["narrative"],"narrative_outcomes":p["narrative_outcomes"],"resource_limit_draft":p["resource_limit_draft"]}))}<h3>{"Catena di comunicazione"}</h3>{for list(&c["messages"]).iter().map(message)}</details>}})}</section>
        <section class="card"><h2>{"Messaggi trasmessi"}</h2>{for list(&v["messages"]).iter().map(message)}</section>
        <section class="card"><h2>{"Timeline delle evidenze"}</h2><div class="row"><button id="newer" disabled={*offset==0} onclick={{let offset=offset.clone();Callback::from(move |_|offset.set((*offset).saturating_sub(200)))}}>{"Più recenti"}</button><button id="older" disabled={*offset+200>=v["timeline_total"].as_u64().unwrap_or(0) as usize} onclick={{let offset=offset.clone();Callback::from(move |_|offset.set(*offset+200))}}>{"Precedenti"}</button><span id="range">{format!("Da {} · {} eventi totali",*offset+1,v["timeline_total"])}</span></div>{for list(&v["timeline"]).iter().map(|l|html!{<details key={format!("log:{}",l["seq"])} data-record={format!("log-{}",l["seq"])}><summary>{format!("#{} · {} · {} · {} · turno {}",l["seq"],text(&l["kind"]),text(&l["actor"]),text(&l["clock"]),l["turn"])}</summary>{crate::render::render_with_raw("",l)}</details>})}</section>
        </section>
    </main>}
}
fn query(filters: &BTreeMap<String, String>, offset: usize) -> String {
    let mut q = format!("offset={offset}");
    for (key, value) in filters {
        if !value.trim().is_empty() {
            q.push_str(&format!("&{}={}", api::encode(key), api::encode(value)));
        }
    }
    q
}
fn evidence(e: &Value) -> Html {
    html! {<div><p><b>{text(&e["basis"]["kind"])}</b>{": "}{text(&e["basis"]["reference"])}</p>{for list(&e["attachments"]).iter().map(|a|{let url=text(&a["url"]);let safe=url.starts_with("https://")||url.starts_with("http://");html!{<p>if safe{<a href={url.to_string()} target="_blank" rel="noopener noreferrer">{text(&a["title"])}</a>}else{{text(&a["title"])}}</p>}})}</div>}
}
fn message(m: &Value) -> Html {
    html! {<details key={format!("message:{}",m["id"])} class="card" data-record={format!("message-{}",m["id"])}><summary>{format!("#{} · {} → {} · {}",m["id"],text(&m["from"]),text(&m["to"]),text(&m["clock"]))}</summary><p>{text(&m["text_en"])}</p>if m["text"]!=m["text_en"]{<p>{text(&m["text"])}</p>}{crate::render::render_with_raw("",m)}</details>}
}
