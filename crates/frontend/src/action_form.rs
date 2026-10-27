//! The form of an inject's actions (world changes that go with an inject),
//! shared by the scenario editor and the custom inject of the EXCON console.
//! An action is an EXCON command (built with the same tools as the Controllo
//! page, or typed as JSON), a time offset from the inject and a note.
use crate::{
    api,
    controllo::{self, CommonProps, MapFactory},
    i18n::ex,
};
use scenario::InjectAction;
use serde_json::{json, Value};
use yew::prelude::*;

/// Seconds from text typed in minutes (decimals allowed). Negative values
/// (before the inject's planned time) only when `allow_pre`.
pub fn parse_offset(text: &str, allow_pre: bool) -> Option<i64> {
    let m: f64 = text.trim().replace(',', ".").parse().ok()?;
    let s = (m * 60.0).round();
    (m.is_finite() && (allow_pre || s >= 0.0) && s.abs() < 1.0e7).then_some(s as i64)
}

/// Minutes as shown in the field.
pub fn offset_text(offset_s: i64) -> String {
    let m = offset_s as f64 / 60.0;
    if m.fract() == 0.0 {
        format!("{}", m as i64)
    } else {
        format!("{m:.1}")
    }
}

/// When the action happens, in words.
pub fn offset_label(offset_s: i64) -> String {
    let min = offset_text(offset_s.abs());
    match offset_s.signum() {
        0 => ex("All’invio dell’inject").to_string(),
        1 => crate::exfmt!("{} min dopo l’invio dell’inject", "{} min after the inject is sent", min),
        _ => crate::exfmt!("{} min prima dell’inject (dall’ora prevista)", "{} min before the inject (from its planned time)", min),
    }
}

/// A new action with an id that is not in `taken`: `<prefix>-NN`.
pub fn new_action(taken: &[String], prefix: &str) -> InjectAction {
    let mut k = taken.len() + 1;
    while taken.iter().any(|t| *t == format!("{prefix}-{k:02}")) {
        k += 1;
    }
    InjectAction {
        id: format!("{prefix}-{k:02}"),
        title: String::new(),
        command: json!({"type": "SetWeather", "wind_speed_kmh": 30.0}),
        offset_s: 0,
        note: String::new(),
    }
}

const TOOL_IDS: [&str; 8] = ["ignite", "line", "drop", "extcircle", "extzone", "evac", "warn", "weather"];

#[derive(Properties, PartialEq)]
pub struct ActionEditorProps {
    pub action: InjectAction,
    pub onchange: Callback<InjectAction>,
    pub ondelete: Callback<()>,
    /// Negative offsets (an action before its inject) are allowed.
    pub allow_pre: bool,
    /// Show the editable identifier (scenario editor).
    #[prop_or_default]
    pub show_id: bool,
    /// Where the tool forms pick points: the live situation (`common` with the
    /// exercise state) or this factory's map (the scenario being edited).
    pub common: CommonProps,
    #[prop_or_default]
    pub map_for: Option<MapFactory>,
    /// `(wind direction, speed, moisture)` the weather form starts from.
    #[prop_or((0.0, 10.0, 10.0))]
    pub weather: (f64, f64, f64),
}

/// One action: title, when, command (tool forms or JSON) and note.
#[function_component(ActionEditor)]
pub fn action_editor(p: &ActionEditorProps) -> Html {
    let a = &p.action;
    let tool = use_state(|| Option::<&'static str>::None);
    // Text being typed, kept apart so partial input ("-", "1.") is not rewritten.
    let raw_json = use_state(|| Option::<String>::None);
    let raw_offset = use_state(|| Option::<String>::None);
    let change = |f: fn(&mut InjectAction, String)| {
        let (a, onchange) = (a.clone(), p.onchange.clone());
        Callback::from(move |v: String| {
            let mut n = a.clone();
            f(&mut n, v);
            onchange.emit(n);
        })
    };
    let set_command = {
        let (a, onchange, raw_json) = (a.clone(), p.onchange.clone(), raw_json.clone());
        Callback::from(move |v: Value| {
            let mut n = a.clone();
            n.command = v;
            raw_json.set(None);
            onchange.emit(n);
        })
    };
    let on_json = {
        let (a, onchange, raw_json) = (a.clone(), p.onchange.clone(), raw_json.clone());
        Callback::from(move |e: InputEvent| {
            let t = api::input(e);
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                if v.is_object() {
                    let mut n = a.clone();
                    n.command = v;
                    onchange.emit(n);
                }
            }
            raw_json.set(Some(t));
        })
    };
    let on_offset = {
        let (a, onchange, raw, allow) = (a.clone(), p.onchange.clone(), raw_offset.clone(), p.allow_pre);
        Callback::from(move |e: InputEvent| {
            let t = api::input(e);
            if let Some(s) = parse_offset(&t, allow) {
                let mut n = a.clone();
                n.offset_s = s;
                onchange.emit(n);
            }
            raw.set(Some(t));
        })
    };
    let shown_json = (*raw_json).clone().unwrap_or_else(|| serde_json::to_string_pretty(&a.command).unwrap_or_default());
    let json_ok = serde_json::from_str::<Value>(&shown_json).is_ok_and(|v| v.is_object());
    let shown_offset = (*raw_offset).clone().filter(|t| parse_offset(t, p.allow_pre).is_some_and(|s| s == a.offset_s)).unwrap_or_else(|| offset_text(a.offset_s));
    let common = CommonProps { onclose: { let tool = tool.clone(); Callback::from(move |_| tool.set(None)) }, ..p.common.clone() };
    let tool_modal = match *tool {
        Some("weather") => {
            let cur = |k: &str, d: f64| if a.command["type"] == "SetWeather" { a.command[k].as_f64() } else { None }.unwrap_or(d);
            html! {<controllo::WeatherModal title={controllo::tool_label("weather")} common={common}
                dir={cur("wind_dir_deg", p.weather.0)} speed={cur("wind_speed_kmh", p.weather.1)}
                moisture={cur("moisture_pct", p.weather.2)} oncommand={Some(set_command.clone())}/>}
        }
        Some(id) => html! {<controllo::MapToolModal key={id.to_string()} tool={id.to_string()} common={common} oncommand={Some(set_command.clone())} map_for={p.map_for.clone()}/>},
        None => Html::default(),
    };
    html! {<div class="action-editor">
        <div class="settings-fields">
            {if p.show_id {html!{<label title={ex("Chiave stabile dell’azione, usata dai report: non riutilizzarla.")}>{ex("Identificativo")}<input value={a.id.clone()} oninput={change(|x, v| x.id = v).reform(api::input)}/></label>}} else {Html::default()}}
            <label title={ex("Nome breve mostrato nell’elenco della timeline.")}>{ex("Titolo")}<input value={a.title.clone()} oninput={change(|x, v| x.title = v).reform(api::input)}/></label>
            <label title={if p.allow_pre {ex("Minuti rispetto all’inject. Zero o positivo: dopo l’invio. Negativo: prima, contando dall’ora prevista dell’inject (l’azione scatta da sola durante l’avanzamento del tempo).")} else {ex("Minuti dopo l’invio dell’inject (0 = subito).")}}>{ex("Quando (min)")}
                <input type="number" step="1" min={if p.allow_pre {None} else {Some("0")}} value={shown_offset} oninput={on_offset}/></label>
            <small class="hint action-when">{offset_label(a.offset_s)}</small>
        </div>
        <div class="inject-recipients" title={ex("Gli stessi strumenti del Controllo EXCON: costruiscono il comando dell’azione.")}>
            <span class="label">{ex("Imposta con lo strumento")}</span>
            <div class="chips">{for controllo::TOOLS.iter().filter(|t| TOOL_IDS.contains(&t.id)).map(|t| {
                let (tool, id) = (tool.clone(), t.id);
                html!{<button type="button" class="chip" title={ex(t.tip)} onclick={Callback::from(move |_| tool.set(Some(id)))}>{format!("{} {}", t.icon, ex(t.label))}</button>}
            })}</div>
            <small class="hint">{format!("{}: {}", ex("Comando attuale"), a.command["type"].as_str().unwrap_or("?"))}</small>
        </div>
        {tool_modal}
        <label class="wide" title={ex("Comando EXCON in JSON, ad esempio SetWeather, Ignite, CloseRoad, SetBuildingStatus, AdjustResource.")}>{ex("Avanzate: comando (JSON)")}
            <textarea rows="5" value={shown_json} oninput={on_json}/>
            <small class="hint">{if json_ok { ex("JSON valido: i campi sono controllati all’esecuzione. Esempio: {\"type\":\"SetWeather\",\"wind_dir_deg\":270}") } else { ex("JSON non valido: l’ultima versione valida resta salvata.") }}</small>
        </label>
        <label class="wide" title={ex("Appunto per l’EXCON, mostrato accanto all’azione nella timeline.")}>{ex("Nota")}<textarea rows="2" value={a.note.clone()} oninput={change(|x, v| x.note = v).reform(api::input)}/></label>
        <div class="inject-actions"><button type="button" class="danger" onclick={{let d=p.ondelete.clone();Callback::from(move |_| d.emit(()))}}>{ex("Elimina azione")}</button></div>
    </div>}
}

#[derive(Properties, PartialEq)]
pub struct ActionListProps {
    pub actions: Vec<InjectAction>,
    pub onchange: Callback<Vec<InjectAction>>,
    pub allow_pre: bool,
    #[prop_or_default]
    pub show_id: bool,
    /// Ids used elsewhere, to give new actions a free one.
    #[prop_or_default]
    pub taken: Vec<String>,
    #[prop_or_else(|| "A".into())]
    pub prefix: AttrValue,
    pub common: CommonProps,
    #[prop_or_default]
    pub map_for: Option<MapFactory>,
    #[prop_or((0.0, 10.0, 10.0))]
    pub weather: (f64, f64, f64),
}

/// The «Azioni» section of an inject: its actions, add and remove.
#[function_component(ActionList)]
pub fn action_list(p: &ActionListProps) -> Html {
    let add = {
        let (actions, taken, prefix, onchange) = (p.actions.clone(), p.taken.clone(), p.prefix.clone(), p.onchange.clone());
        Callback::from(move |_: MouseEvent| {
            let used: Vec<String> = taken.iter().cloned().chain(actions.iter().map(|a| a.id.clone())).collect();
            let mut n = actions.clone();
            n.push(new_action(&used, &prefix));
            onchange.emit(n);
        })
    };
    html! {<fieldset class="inject-actions-section">
        <legend>{crate::exfmt!("Azioni ({})", "Actions ({})", p.actions.len())}</legend>
        <small class="hint">{if p.allow_pre {ex("Cambiamenti del mondo che accompagnano l’inject: partono all’invio (0 min), dopo (min positivi) oppure prima, da soli, durante l’avanzamento del tempo (min negativi).")} else {ex("Cambiamenti del mondo eseguiti dall’EXCON con l’invio: subito (0 min) oppure dopo i minuti indicati, durante il prossimo avanzamento del tempo.")}}</small>
        {for p.actions.iter().enumerate().map(|(i, a)| {
            let change = {
                let (actions, onchange) = (p.actions.clone(), p.onchange.clone());
                Callback::from(move |n: InjectAction| {
                    let mut l = actions.clone();
                    if let Some(x) = l.get_mut(i) { *x = n; }
                    onchange.emit(l);
                })
            };
            let delete = {
                let (actions, onchange) = (p.actions.clone(), p.onchange.clone());
                Callback::from(move |_: ()| {
                    let mut l = actions.clone();
                    if i < l.len() { l.remove(i); }
                    onchange.emit(l);
                })
            };
            html!{<details class="action-item" key={format!("{i}:{}", a.id)} open={a.title.trim().is_empty()}>
                <summary><span class="pill kind action">{ex("Azione")}</span>{" "}<b>{if a.title.trim().is_empty() { ex("(senza titolo)").to_string() } else { a.title.clone() }}</b>{" · "}<span class="muted">{offset_label(a.offset_s)}</span></summary>
                <ActionEditor action={a.clone()} onchange={change} ondelete={delete} allow_pre={p.allow_pre} show_id={p.show_id}
                    common={p.common.clone()} map_for={p.map_for.clone()} weather={p.weather}/>
            </details>}
        })}
        <button type="button" class="chip" onclick={add}>{ex("Aggiungi azione")}</button>
    </fieldset>}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_minutes_and_negative_only_for_pre_actions() {
        assert_eq!(parse_offset("5", false), Some(300));
        assert_eq!(parse_offset("1,5", false), Some(90));
        assert_eq!(parse_offset("-5", true), Some(-300));
        assert_eq!(parse_offset("-5", false), None);
        assert_eq!(parse_offset("-", true), None);
        assert_eq!(parse_offset("", true), None);
        assert_eq!(offset_text(-300), "-5");
        assert_eq!(offset_text(90), "1.5");
    }

    #[test]
    fn offset_wording_says_before_or_after() {
        assert_eq!(offset_label(0), "All’invio dell’inject");
        assert_eq!(offset_label(300), "5 min dopo l’invio dell’inject");
        assert_eq!(offset_label(-600), "10 min prima dell’inject (dall’ora prevista)");
    }

    #[test]
    fn new_actions_get_free_ids() {
        let taken = vec!["A-01".to_string(), "A-03".to_string()];
        assert_eq!(new_action(&taken, "A").id, "A-04");
        assert_eq!(new_action(&[], "C").id, "C-01");
    }
}
