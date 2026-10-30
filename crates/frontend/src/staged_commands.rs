//! EXCON commands collected in a form and sent later with it (for example the
//! interventions that accompany a narrative outcome). They are built with the
//! same tool forms as the Controllo page (map picking included) and applied
//! by the server as ordinary EXCON interventions.
use crate::{
    api,
    controllo::{self, CommonProps},
    i18n::ex,
    widgets::{command_name, command_summary},
};
use serde_json::Value;
use yew::prelude::*;

/// Controllo tools that produce a world-changing command.
const TOOL_IDS: [&str; 10] = ["ignite", "line", "drop", "extcircle", "extzone", "evac", "evacborgo", "warn", "bstatus", "weather"];

#[derive(Properties, PartialEq)]
pub struct StagedProps {
    pub commands: Vec<Value>,
    pub onchange: Callback<Vec<Value>>,
    pub common: CommonProps,
    #[prop_or_default]
    pub disabled: bool,
}

#[function_component(StagedCommands)]
pub fn staged_commands(p: &StagedProps) -> Html {
    let tool = use_state(|| Option::<&'static str>::None);
    let raw = use_state(String::new);
    let raw_error = use_state(String::new);
    let add = {
        let (commands, onchange, tool) = (p.commands.clone(), p.onchange.clone(), tool.clone());
        Callback::from(move |v: Value| {
            let mut next = commands.clone();
            next.push(v);
            onchange.emit(next);
            tool.set(None);
        })
    };
    let common = CommonProps { onclose: { let tool = tool.clone(); Callback::from(move |_| tool.set(None)) }, ..p.common.clone() };
    let modal = match *tool {
        Some("weather") => {
            let w = &p.common.state["world"]["weather"];
            html! {<controllo::WeatherModal title={controllo::tool_label("weather")} common={common}
                dir={w["wind_dir_deg"].as_f64().unwrap_or(0.)} speed={w["wind_speed_kmh"].as_f64().unwrap_or(10.)}
                moisture={w["moisture_pct"].as_f64().unwrap_or(10.)} oncommand={Some(add.clone())}/>}
        }
        Some(id) => html! {<controllo::MapToolModal key={id.to_string()} tool={id.to_string()} common={common} oncommand={Some(add.clone())}/>},
        None => Html::default(),
    };
    let lang = if crate::i18n_excon::is_en() { "en" } else { "it" };
    let add_json = {
        let (raw, raw_error, commands, onchange) = (raw.clone(), raw_error.clone(), p.commands.clone(), p.onchange.clone());
        Callback::from(move |_: MouseEvent| match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(o)) if o.contains_key("type") => {
                let mut next = commands.clone();
                next.push(Value::Object(o));
                onchange.emit(next);
                raw.set(String::new());
                raw_error.set(String::new());
            }
            Ok(_) => raw_error.set(ex("Serve un oggetto JSON con il campo type.").to_string()),
            Err(e) => raw_error.set(e.to_string()),
        })
    };
    html! {<fieldset class="staged-commands" data-staged-commands="">
        <legend>{crate::exfmt!("Azioni EXCON con l’esito ({})", "EXCON actions with the outcome ({})", p.commands.len())}</legend>
        <small class="hint">{ex("Applicate subito come interventi EXCON, con le stesse verifiche degli strumenti del Controllo. Facoltative.")}</small>
        <ol class="staged-list">{for p.commands.iter().enumerate().map(|(i, c)| {
            let remove = {
                let (commands, onchange) = (p.commands.clone(), p.onchange.clone());
                Callback::from(move |_: MouseEvent| {
                    let mut next = commands.clone();
                    if i < next.len() { next.remove(i); }
                    onchange.emit(next);
                })
            };
            html!{<li key={i} data-staged-command={i.to_string()}><b>{command_name(api::text(&c["type"]))}</b>{" · "}<span class="muted">{command_summary(lang, c)}</span>
                <button type="button" disabled={p.disabled} onclick={remove}>{ex("Rimuovi")}</button></li>}
        })}</ol>
        <div class="chips">{for controllo::TOOLS.iter().filter(|t| TOOL_IDS.contains(&t.id)).map(|t| {
            let (tool, id) = (tool.clone(), t.id);
            html!{<button type="button" class="chip" data-staged-tool={t.id} disabled={p.disabled} title={ex(t.tip)} onclick={Callback::from(move |_| tool.set(Some(id)))}>{format!("{} {}", t.icon, ex(t.label))}</button>}
        })}</div>
        {modal}
        <details class="manual-commands"><summary>{ex("Avanzate: comando in JSON (ad esempio CloseRoad)")}</summary>
            <textarea rows="3" aria-label={ex("Comando JSON")} value={(*raw).clone()} oninput={{let raw=raw.clone();Callback::from(move |e| raw.set(api::input(e)))}}/>
            <button type="button" disabled={p.disabled || raw.trim().is_empty()} onclick={add_json}>{ex("Aggiungi comando")}</button>
            if !raw_error.is_empty() {<p class="error">{(*raw_error).clone()}</p>}
        </details>
    </fieldset>}
}
