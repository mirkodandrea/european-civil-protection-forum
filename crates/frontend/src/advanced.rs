//! EXCON forecast review (private draft -> review -> publish), shown inside the "Controllo" forecast modal.
use crate::{
    api::{self, list, text},
    map::SituationMap,
    Action,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use yew::prelude::*;

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
    let horizons = use_state(|| "60,180".to_string());
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
        let horizons = horizons.clone();
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
                    json!({"seed":1,"members":4,"horizons_s":horizons,"direction_spread_deg":10.0,"speed_spread_fraction":0.1,"bulletin":[]}),
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
        {field(crate::i18n::ex("Orizzonti (minuti crescenti)"),&horizons)}
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
