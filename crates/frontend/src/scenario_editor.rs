//! "Scenario" view of the EXCON: the scenarios of the library and an editor
//! for what a designer controls: roles, resources, ignitions and weather,
//! reinforcements, places and the injects. The territory (terrain, buildings,
//! roads) is not edited here: a scenario is a *pack* over a base territory.
//!
//! The same component serves the server UI and the standalone app; requests go
//! through `api::request`, which the standalone build answers in-process.
use crate::{
    api::{self, list, text},
    i18n::ex,
};
use scenario::{
    IgnitionSpec, InjectTemplate, Place, ReinforcementOffer, ResourceType, Role, ScenarioPack,
};
use crate::timeline_plan as plan;
use serde_json::{json, Value};
use std::rc::Rc;
use yew::prelude::*;

fn token() -> String {
    #[cfg(feature = "standalone")]
    return "master".into();
    #[cfg(not(feature = "standalone"))]
    api::token()
}

async fn call(path: &str, body: Option<Value>) -> Result<Value, String> {
    api::request(&token(), path, body).await
}

/// A scenario being edited.
#[derive(Clone)]
struct Open {
    pack: ScenarioPack,
    /// Names of the territory, for the pickers.
    reference: Value,
    /// Stored under this id; `None` until the first save.
    saved_id: Option<String>,
    dirty: bool,
}

#[derive(Properties, PartialEq)]
pub struct Props {
    /// Exercise generation: the library is read again when it changes.
    pub revision: u64,
}

// ------------------------------------------------------------ small widgets

/// Applies `f` to the open scenario when `cb` fires.
fn upd<E: 'static>(
    open: &UseStateHandle<Option<Open>>,
    f: impl Fn(&mut ScenarioPack, E) + 'static,
) -> Callback<E> {
    let open = open.clone();
    Callback::from(move |e: E| {
        if let Some(cur) = &*open {
            let mut next = cur.clone();
            f(&mut next.pack, e);
            next.dirty = true;
            open.set(Some(next));
        }
    })
}

/// Label text, tooltip and, when there is one, a visible explanation.
fn hint(tip: &'static str) -> Html {
    if tip.is_empty() {
        Html::default()
    } else {
        html! {<small class="hint">{ex(tip)}</small>}
    }
}

fn field(label: &'static str, tip: &'static str, value: String, cb: Callback<String>) -> Html {
    html! {<label title={ex(tip)}>{ex(label)}<input value={value} oninput={cb.reform(api::input)}/>{hint(tip)}</label>}
}

fn number(label: &'static str, tip: &'static str, value: String, cb: Callback<String>) -> Html {
    html! {<label title={ex(tip)}>{ex(label)}<input type="number" step="any" value={value} oninput={cb.reform(api::input)}/>{hint(tip)}</label>}
}

fn area(label: &'static str, tip: &'static str, value: String, rows: &'static str, cb: Callback<String>) -> Html {
    html! {<label class="wide" title={ex(tip)}>{ex(label)}<textarea rows={rows} value={value} oninput={cb.reform(api::input)}/>{hint(tip)}</label>}
}

fn check(label: String, tip: &'static str, on: bool, cb: Callback<bool>) -> Html {
    html! {<label class="check" title={ex(tip)}><input type="checkbox" checked={on} onchange={cb.reform(|e: Event| {
        use yew::TargetCast;
        e.target_unchecked_into::<web_sys::HtmlInputElement>().checked()
    })}/>{label}</label>}
}

fn choice(label: &'static str, tip: &'static str, value: &str, options: &[(String, String)], cb: Callback<String>) -> Html {
    html! {<label title={ex(tip)}>{ex(label)}<select onchange={cb.reform(api::select)}>
        {for options.iter().map(|(v, name)| html!{<option value={v.clone()} selected={v == value}>{name.clone()}</option>})}
    </select>{hint(tip)}</label>}
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.trim().replace(',', ".").parse().ok()
}

fn fmt(v: f64) -> String {
    format!("{v}")
}

// ------------------------------------------------------------ sections

fn clock_text(s: i64) -> String {
    format!("{:02}:{:02}", s / 3600 % 24, s / 60 % 60)
}

fn parse_clock(day: i64, hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.split_once(':')?;
    let (h, m): (i64, i64) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((day - 1).max(0) * 86_400 + h * 3600 + m * 60)
}

fn general(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let start = e.start_clock_s.unwrap_or(0);
    let tm = e.turn_minutes.unwrap_or(plan::DEFAULT_TURN_MINUTES);
    let weather = e.weather.unwrap_or(scenario::WeatherSpec {
        wind_dir_deg: 0.0,
        wind_speed_kmh: 0.0,
        moisture_pct: 10.0,
    });
    let wind_name = compass(weather.wind_dir_deg);
    let mut places: Vec<(String, String)> = vec![(String::new(), ex("Centro della mappa").to_string())];
    places.extend(e.places.iter().flatten().map(|p| (p.name.clone(), p.name.clone())));
    places.extend(
        list(&o.reference["facilities"])
            .iter()
            .map(|f| (text(f).to_string(), text(f).to_string())),
    );
    let day_cb = {
        let open = open.clone();
        Callback::from(move |v: String| {
            if let (Some(cur), Some(day)) = (&*open, num::<i64>(&v)) {
                let mut next = cur.clone();
                let t = next.pack.edits.start_clock_s.unwrap_or(0);
                next.pack.edits.start_clock_s = Some((day - 1).max(0) * 86_400 + t % 86_400);
                next.dirty = true;
                open.set(Some(next));
            }
        })
    };
    let weather_edit = |f: fn(&mut scenario::WeatherSpec, f64)| {
        upd(open, move |p, v: String| {
            if let Some(x) = num(&v) {
                f(p.edits.weather.get_or_insert(scenario::WeatherSpec { wind_dir_deg: 0.0, wind_speed_kmh: 0.0, moisture_pct: 10.0 }), x)
            }
        })
    };
    html! {<>
        {gen_section("Identità", "Come l’esercitazione si presenta nell’elenco e ai partecipanti.", html! {<>
            {field("Nome", "Titolo dello scenario, mostrato nell’elenco e nelle schermate EXCON.", o.pack.name.clone(), upd(open, |p, v: String| p.name = v))}
            <label title={ex("Territorio su cui si basa lo scenario (mappa, edifici, strade). Non si modifica qui.")}>{ex("Territorio")}<input value={o.pack.base.clone()} disabled=true/>{hint("Territorio su cui si basa lo scenario (mappa, edifici, strade). Non si modifica qui.")}</label>
            {area("Descrizione", "Breve sintesi dell’evento e degli obiettivi di addestramento. Compare nell’elenco degli scenari.", o.pack.description.clone(), "3", upd(open, |p, v: String| p.description = v))}
        </>})}
        {gen_section("Orario", "Quando comincia l’esercitazione, nell’ora simulata dello scenario, e quanto dura un turno.", html! {<>
            {gen_number("Giorno di inizio", "Giorno simulato in cui parte l’esercitazione: 1 è il primo giorno. Serve per scenari che durano più di una notte.", "1", None, "1", "", (start / 86_400 + 1).to_string(), day_cb)}
            <label title={ex("Ora del giorno (24 ore) in cui parte la simulazione. Gli inject sono datati rispetto a questo istante.")}>{ex("Ora di inizio (HH:MM)")}
                <input type="time" required=true value={clock_text(start)} oninput={upd(open, |p, v: String| {
                    let day = p.edits.start_clock_s.unwrap_or(0) / 86_400 + 1;
                    if let Some(t) = parse_clock(day, &v) { p.edits.start_clock_s = Some(t); }
                }).reform(api::input)}/>
                {hint("Ora del giorno (24 ore) in cui parte la simulazione. Gli inject sono datati rispetto a questo istante.")}</label>
            {gen_number("Durata del turno (min)", "Lunghezza di un turno in minuti (1–1440). Divide la timeline in fasce e fissa l’orario degli inject che non ne hanno uno proprio (inizio del loro turno). Durante l’esercitazione l’EXCON può cambiarla senza spostare gli inject pianificati.", "1", Some("1440"), "1", "", tm.to_string(), upd(open, |p, v: String| if let Some(x) = num::<u32>(&v) { plan::set_turn_minutes(&mut p.edits, x) }))}
        </>})}
        {gen_section("Meteo", "Condizioni iniziali che guidano la propagazione del fuoco. Possono cambiare durante l’esercitazione con le azioni pianificate (scheda Timeline).", html! {<>
            <div class="gen-wind">
                {gen_number("Vento: direzione di provenienza (gradi)", "Direzione DA CUI soffia il vento, in gradi dal nord in senso orario: 0 = da nord, 90 = da est, 180 = da sud, 270 = da ovest. Il fuoco avanza nella direzione opposta. Valori da 0 a 360.", "0", Some("360"), "1", "", fmt(weather.wind_dir_deg), weather_edit(|w, x| w.wind_dir_deg = x))}
                <small class="hint live">{format!("{} {} ({}°)", ex("Vento da"), ex(wind_name), fmt(weather.wind_dir_deg))}</small>
            </div>
            {gen_number("Vento: velocità (km/h)", "Velocità media del vento. Ordini di grandezza: 10 brezza, 30 vento teso, 60 o più vento forte che rende il fuoco molto veloce e irregolare.", "0", Some("150"), "1", "", fmt(weather.wind_speed_kmh), weather_edit(|w, x| w.wind_speed_kmh = x))}
            {gen_number("Umidità del combustibile morto (%)", "Umidità della vegetazione secca fine. Più è bassa, più il fuoco è veloce: sotto il 10% condizioni estreme, 15–20% moderate, oltre il 30% il fuoco si propaga a fatica. Valori da 1 a 60.", "1", Some("60"), "1", "", fmt(weather.moisture_pct), weather_edit(|w, x| w.moisture_pct = x))}
        </>})}
        {gen_section("Risorse", "Da dove partono i mezzi quando un dispiegamento viene approvato.", html! {<>
            {choice("Base di partenza delle risorse", "Luogo da cui partono i dispiegamenti approvati: i mezzi percorrono la distanza fino all’obiettivo e gli effetti sul fuoco iniziano solo all’arrivo. Se non si sceglie un luogo si parte dal centro della mappa.", e.staging_base.as_deref().unwrap_or(""), &places, upd(open, |p, v: String| p.edits.staging_base = Some(v)))}
        </>})}
    </>}
}

/// A titled group of controls with a one-line introduction.
fn gen_section(title: &'static str, intro: &'static str, body: Html) -> Html {
    html! {<section class="gen-sec"><h4>{ex(title)}</h4><p class="muted">{ex(intro)}</p><div class="gen-grid">{body}</div></section>}
}

/// Number input with limits: the browser marks values outside them.
#[allow(clippy::too_many_arguments)]
fn gen_number(
    label: &'static str,
    tip: &'static str,
    min: &'static str,
    max: Option<&'static str>,
    step: &'static str,
    placeholder: &'static str,
    value: String,
    cb: Callback<String>,
) -> Html {
    html! {<label title={ex(tip)}>{ex(label)}
        <input type="number" inputmode="decimal" min={min} max={max} step={step} placeholder={ex(placeholder)} value={value} oninput={cb.reform(api::input)}/>
        {hint(tip)}</label>}
}

/// Checkbox with a visible explanation.
fn gen_flag(label: &'static str, tip: &'static str, on: bool, cb: Callback<bool>) -> Html {
    html! {<div class="gen-flag">{check(ex(label).to_string(), tip, on, cb)}{hint(tip)}</div>}
}

/// Compass point (Italian initials) of a bearing.
fn compass(deg: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SO", "O", "NO"];
    POINTS[((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

#[derive(Properties, Clone)]
struct ListProps {
    o: Open,
    open: UseStateHandle<Option<Open>>,
}

impl PartialEq for ListProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

/// Master-detail layout shared by the Ruoli, Risorse and Inject tabs: a compact
/// selectable list (items, then tools such as "Aggiungi") and the editor of the
/// selected item only. The classes are the `md-*` block at the end of interface.css.
fn master_detail(items: Html, tools: Html, detail: Html) -> Html {
    html! {<div class="md">
        <div class="md-list">
            <div class="md-items">{items}</div>
            <div class="md-tools">{tools}</div>
        </div>
        <div class="md-detail">{detail}</div>
    </div>}
}

/// One selectable line of the master list.
fn md_item(active: bool, onclick: Callback<MouseEvent>, content: Html) -> Html {
    html! {<button type="button" class={classes!("md-item", active.then_some("active"))} aria-pressed={active.to_string()} onclick={onclick}>{content}</button>}
}

fn roles_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<RolesTab o={o.clone()} open={open.clone()}/>}
}

#[function_component(RolesTab)]
fn roles_view(props: &ListProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected = use_state(|| 0usize);
    let e = &o.pack.edits;
    let roles = e.roles.clone().unwrap_or_default();
    let resources = e.resource_types.clone().unwrap_or_default();
    let levels: Vec<(String, String)> = [("national", "Nazionale"), ("regional", "Regionale"), ("local", "Locale")]
        .iter()
        .map(|(l, name)| (l.to_string(), ex(name).to_string()))
        .collect();
    let languages: Vec<(String, String)> = api::LANGUAGES
        .iter()
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect();
    let actions: Vec<String> = list(&o.reference["actions"]).iter().map(|a| text(a).to_string()).collect();
    let n = roles.len();
    let sel = (*selected).min(n.saturating_sub(1));
    let items = html! {<>
        {for roles.iter().enumerate().map(|(i, r)| {
            let stock: Vec<String> = resources.iter().filter_map(|res| r.resources.get(&res.id).map(|q| format!("{} ×{q}", res.name))).collect();
            let level = levels.iter().find(|(v, _)| *v == r.level).map(|(_, n)| n.clone()).unwrap_or_else(|| r.level.clone());
            let bad = r.id.is_empty() || roles.iter().filter(|x| x.id == r.id).count() > 1;
            md_item(i == sel, { let selected = selected.clone(); Callback::from(move |_| selected.set(i)) }, html!{<>
                <b class="md-id">{ if r.id.is_empty() { ex("nuovo").to_string() } else { r.id.clone() } }</b>
                <span class="pill">{level}</span>
                { if bad { html!{<span class="pill pending" title={ex("Identificativo non valido o duplicato.")}>{"!"}</span>} } else { Html::default() } }
                <span class="md-name">{r.name.clone()}</span>
                <small class="md-sub">{crate::exfmt!("{} azioni", "{} actions", r.allowed_actions.len())}
                    {" · "}{ if stock.is_empty() { ex("nessuna risorsa").to_string() } else { stock.join(", ") } }</small>
            </>})
        })}
        { if n == 0 { html!{<p class="muted">{ex("Nessun ruolo definito.")}</p>} } else { Html::default() } }
    </>};
    let tools = html! {
        <button class="primary" onclick={{
            let (selected, add) = (selected.clone(), upd(open, |p, _: MouseEvent| {
                let l = p.edits.roles.get_or_insert_with(Vec::new);
                let n = l.len() + 1;
                l.push(Role {
                    id: format!("role_{n}"), name: format!("Ruolo {n}"), level: "local".into(), language: "en".into(),
                    description: String::new(), allowed_actions: vec![], resources: Default::default(),
                });
            }));
            Callback::from(move |e| { selected.set(n); add.emit(e) })
        }}>{ex("Aggiungi ruolo")}</button>
    };
    let detail = match roles.get(sel) {
        Some(r) => {
            let i = sel;
            let r = r.clone();
            let duplicate = roles.iter().filter(|x| x.id == r.id).count() > 1;
            let bad_id = r.id.is_empty() || !r.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
            html! {<div class="rr-body" key={i}>
                <div class="settings-fields">
                    {field("Identificativo", "Codice breve del ruolo: compare negli indirizzi delle pagine player e come destinatario degli inject. Solo a-z, 0-9, _ e -; deve essere unico.", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", "Nome mostrato ai partecipanti e all’EXCON, ad esempio «Protezione civile regionale».", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {choice("Livello", "Livello istituzionale del ruolo: nazionale, regionale o locale. Orienta il perimetro delle richieste e il modo in cui l’AI contestualizza i messaggi del ruolo.", &r.level, &levels, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.level = v }))}
                    {choice("Lingua", "Lingua predefinita della pagina del giocatore; i messaggi vengono tradotti per l’EXCON.", &r.language, &languages, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.language = v }))}
                </div>
                { if duplicate { html!{<p class="error">{ex("Identificativo già usato da un altro ruolo.")}</p>} } else if bad_id { html!{<p class="error">{ex("Identificativo non valido: usa solo a-z, 0-9, _ e -.")}</p>} } else { Html::default() } }
                {area("Descrizione", "Responsabilità e contesto del ruolo, letti dal giocatore e usati dall’AI per interpretare i messaggi.", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
                <h4>{ex("Risorse disponibili per turno")}</h4>
                <p class="muted">{ex("Unità che il ruolo può impiegare in ogni turno. 0 = nessuna.")}</p>
                <div class="rr-qty">
                    {for resources.iter().map(|res| {
                        let id = res.id.clone();
                        let qty = r.resources.get(&res.id).copied().unwrap_or(0);
                        qty_box(format!("{} ({})", res.name, res.id), qty.to_string(), upd(open, move |p, v: String| {
                            if let (Some(x), Some(n)) = (p.edits.roles.as_mut().and_then(|l| l.get_mut(i)), num::<u32>(&v)) {
                                if n == 0 { x.resources.remove(&id); } else { x.resources.insert(id.clone(), n); }
                            }
                        }))
                    })}
                    { if resources.is_empty() { html!{<small class="muted">{ex("Nessun tipo di risorsa: aggiungilo nella scheda Risorse.")}</small>} } else { Html::default() } }
                </div>
                <h4>{ex("Azioni che può proporre")}
                    <span class="rr-bulk">
                        <button type="button" title={ex("Abilita tutte le azioni per questo ruolo")} onclick={{
                            let all = actions.clone();
                            upd(open, move |p, _: MouseEvent| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.allowed_actions = all.clone() })
                        }}>{ex("Tutte")}</button>
                        <button type="button" title={ex("Toglie tutte le azioni: il ruolo potrà solo scrivere messaggi")} onclick={upd(open, move |p, _: MouseEvent| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.allowed_actions.clear() })}>{ex("Nessuna")}</button>
                    </span></h4>
                <div class="rr-actions">
                    {for actions.iter().map(|a| {
                        let action = a.clone();
                        action_box(a, r.allowed_actions.contains(a), upd(open, move |p, on: bool| {
                            if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) {
                                x.allowed_actions.retain(|y| *y != action);
                                if on { x.allowed_actions.push(action.clone()); }
                            }
                        }))
                    })}
                </div>
                {confirm_delete("Elimina ruolo", "Elimina il ruolo e la sua pagina player. Se un inject, un rinforzo o un luogo lo cita ancora, «Controlla» lo segnala.",
                    upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.roles.as_mut() { if i < l.len() { l.remove(i); } } }))}
            </div>}
        }
        None => html! {<p class="muted">{ex("Aggiungi un ruolo per iniziare.")}</p>},
    };
    html! {<div class="rr-tab">
        <p class="muted">{ex("Ogni ruolo è una pagina player. Azioni e risorse decidono cosa può proporre; l’EXCON approva comunque ogni azione.")}</p>
        {master_detail(items, tools, detail)}
        {area("Ruoli interpretati dall’EXCON", "Le parti che l’EXCON recita negli inject (ad esempio «Centrale 112», «Sindaco»), una per riga. Sono distinti dai ruoli dei player e compaiono come mittenti nella costruzione degli inject.", e.excon_roles.clone().unwrap_or_default().join("\n"), "4",
            upd(open, |p, v: String| p.edits.excon_roles = Some(v.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())))}
    </div>}
}

fn resources_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<ResourcesTab o={o.clone()} open={open.clone()}/>}
}

#[function_component(ResourcesTab)]
fn resources_view(props: &ListProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected = use_state(|| 0usize);
    let resources = o.pack.edits.resource_types.clone().unwrap_or_default();
    let roles = o.pack.edits.roles.clone().unwrap_or_default();
    let n = resources.len();
    let sel = (*selected).min(n.saturating_sub(1));
    let users_of = |id: &str| -> Vec<String> { roles.iter().filter(|x| x.resources.contains_key(id)).map(|x| x.id.clone()).collect() };
    let items = html! {<>
        {for resources.iter().enumerate().map(|(i, r)| {
            let bad = r.id.is_empty() || resources.iter().filter(|x| x.id == r.id).count() > 1;
            md_item(i == sel, { let selected = selected.clone(); Callback::from(move |_| selected.set(i)) }, html!{<>
                <b class="md-id">{r.id.clone()}</b>
                { if bad { html!{<span class="pill pending" title={ex("Identificativo vuoto o duplicato.")}>{"!"}</span>} } else { Html::default() } }
                <span class="md-name">{r.name.clone()}</span>
                <small class="md-sub">{ match r.speed_mps { Some(s) => format!("{s} m/s"), None => ex("velocità predefinita").to_string() } }
                    {" · "}{crate::exfmt!("{} ruoli", "{} roles", users_of(&r.id).len())}</small>
            </>})
        })}
        { if n == 0 { html!{<p class="muted">{ex("Nessun tipo di risorsa definito.")}</p>} } else { Html::default() } }
    </>};
    let tools = html! {
        <button class="primary" onclick={{
            let (selected, add) = (selected.clone(), upd(open, |p, _: MouseEvent| {
                let l = p.edits.resource_types.get_or_insert_with(Vec::new);
                let n = l.len() + 1;
                l.push(ResourceType { id: format!("resource_{n}"), name: format!("Risorsa {n}"), description: String::new(), speed_mps: None });
            }));
            Callback::from(move |e| { selected.set(n); add.emit(e) })
        }}>{ex("Aggiungi risorsa")}</button>
    };
    let detail = match resources.get(sel) {
        Some(r) => {
            let i = sel;
            let r = r.clone();
            let duplicate = resources.iter().filter(|x| x.id == r.id).count() > 1;
            let users = users_of(&r.id);
            html! {<div class="rr-body" key={i}>
                <div class="settings-fields">
                    {field("Identificativo", "Codice breve della risorsa, usato nelle quantità dei ruoli, nei rinforzi e negli ordini. Deve essere unico.", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", "Nome leggibile mostrato ai giocatori, ad esempio «Squadra AIB».", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {number("Velocità di spostamento (m/s, vuoto = predefinita)", "Velocità con cui la risorsa raggiunge il luogo dalla base di partenza; i suoi effetti iniziano all’arrivo. Esempio: 10 m/s = 36 km/h. Vuoto = valore predefinito del simulatore.", r.speed_mps.map(|s| s.to_string()).unwrap_or_default(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.speed_mps = num(&v) }))}
                </div>
                { if duplicate { html!{<p class="error">{ex("Identificativo già usato da un’altra risorsa.")}</p>} } else { Html::default() } }
                {area("Descrizione", "Cosa rappresenta e cosa può fare la risorsa; aiuta l’AI e i giocatori a usarla in modo coerente.", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
                <p class="muted">{ if users.is_empty() { ex("Nessun ruolo la possiede.").to_string() } else { crate::exfmt!("Assegnata ai ruoli: {}", "Assigned to roles: {}", users.join(", ")) } }</p>
                {confirm_delete("Elimina risorsa", "Elimina il tipo di risorsa. Se un ruolo o un rinforzo la usa ancora, «Controlla» lo segnala.",
                    upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.resource_types.as_mut() { if i < l.len() { l.remove(i); } } }))}
            </div>}
        }
        None => html! {<p class="muted">{ex("Aggiungi un tipo di risorsa per iniziare.")}</p>},
    };
    html! {<div class="rr-tab">
        <p class="muted">{ex("Tipi di risorsa del lancio: acqua, squadre, mezzi. La velocità è quella con cui raggiungono il luogo dalla base di partenza.")}</p>
        {master_detail(items, tools, detail)}
    </div>}
}

/// Plain-words explanation of a command kind, for the role action checkboxes.
fn action_tip(action: &str) -> &'static str {
    match action {
        "CutFireLine" => "Apre una linea tagliafuoco (senza vegetazione) lungo un percorso: ferma l’avanzata se è completata prima del fuoco.",
        "WaterDrop" => "Lancio d’acqua da aerei o elicotteri su una zona: rallenta o spegne il fuoco sotto il passaggio.",
        "DeployEngine" => "Invia un’autobotte o una squadra a terra in un luogo: agisce dopo il viaggio dalla base, se le strade sono praticabili.",
        "EvacuateZone" => "Ordina di evacuare tutta una zona della griglia, con i suoi edifici e abitanti.",
        "EvacuateBuilding" => "Ordina di evacuare un singolo edificio, ad esempio una scuola o una casa di riposo.",
        "IssueWarning" => "Emette un’allerta alla popolazione di un luogo; ha effetto dopo il tempo di diffusione.",
        "CloseRoad" => "Chiude una strada o un tratto: il traffico civile non passa, i mezzi di soccorso sì.",
        "ReopenRoad" => "Riapre una strada chiusa in precedenza.",
        "OpenShelter" => "Apre un centro di accoglienza dove portare gli evacuati.",
        "RequestReinforcement" => "Chiede rinforzi esterni, che arrivano dopo un ritardo se previsti dallo scenario.",
        _ => "Azione operativa che il ruolo può proporre all’EXCON.",
    }
}

/// An action checkbox with its plain-words explanation, as title and visible hint.
fn action_box(action: &str, on: bool, cb: Callback<bool>) -> Html {
    let tip = action_tip(action);
    html! {<label class="rr-action" title={ex(tip)}>
        <input type="checkbox" checked={on} onchange={cb.reform(|e: Event| {
            use yew::TargetCast;
            e.target_unchecked_into::<web_sys::HtmlInputElement>().checked()
        })}/>
        <span><b>{action.to_string()}</b><small class="hint">{ex(tip)}</small></span>
    </label>}
}

/// Quantity input for one resource of a role.
fn qty_box(label: String, value: String, cb: Callback<String>) -> Html {
    html! {<label class="rr-q" title={ex("Unità al turno (0 = nessuna)")}>
        <span>{label}</span>
        <input type="number" min="0" step="1" inputmode="numeric" value={value} oninput={cb.reform(api::input)}/>
    </label>}
}

/// Two-step delete: opening the disclosure explains the consequence, the button inside confirms.
fn confirm_delete(label: &'static str, tip: &'static str, cb: Callback<MouseEvent>) -> Html {
    html! {<details class="rr-del">
        <summary class="danger" title={ex(tip)}>{ex(label)}{"…"}</summary>
        <p class="muted">{ex(tip)}</p>
        <button type="button" class="danger" onclick={cb}>{ex("Conferma eliminazione")}</button>
    </details>}
}

/// Geometry of the territory for the maps (metres, origin south-west).
#[derive(Clone, Copy)]
struct Geom {
    width: f64,
    height: f64,
    zone: f64,
    cols: usize,
    rows: usize,
}

fn geom(o: &Open) -> Geom {
    Geom {
        width: o.reference["grid"]["width_m"].as_f64().unwrap_or(1.0),
        height: o.reference["grid"]["height_m"].as_f64().unwrap_or(1.0),
        zone: o.reference["zone_grid"]["size_m"].as_f64().unwrap_or(500.0),
        cols: o.reference["zone_grid"]["cols"].as_u64().unwrap_or(0) as usize,
        rows: o.reference["zone_grid"]["rows"].as_u64().unwrap_or(0) as usize,
    }
}

/// Zone code containing `at`, from the territory's zone centres.
fn zone_at(o: &Open, g: Geom, at: [f32; 2]) -> Option<String> {
    o.reference["zone_centres"].as_object()?.iter().find_map(|(name, c)| {
        let (cx, cy) = (c[0].as_f64()?, c[1].as_f64()?);
        ((cx - at[0] as f64).abs() <= g.zone / 2.0 && (cy - at[1] as f64).abs() <= g.zone / 2.0).then(|| name.clone())
    })
}

/// World position of a click on the map and, when it landed on a marker, the
/// index in its `data-hit` attribute.
fn map_click(map: &NodeRef, g: Geom, on: Callback<([f32; 2], Option<usize>)>) -> Callback<MouseEvent> {
    use wasm_bindgen::JsCast;
    let map = map.clone();
    Callback::from(move |e: MouseEvent| {
        // Yew delegates events: `current_target` is not the map, so keep its node.
        let Some(svg) = map.cast::<web_sys::Element>() else { return };
        let rect = svg.get_bounding_client_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let x = ((e.client_x() as f64 - rect.left()) / rect.width() * g.width).clamp(0.0, g.width);
        let y = (g.height - (e.client_y() as f64 - rect.top()) / rect.height() * g.height).clamp(0.0, g.height);
        let hit = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("[data-hit]").ok().flatten())
            .and_then(|el| el.get_attribute("data-hit"))
            .and_then(|s| s.parse().ok());
        on.emit(([x.round() as f32, y.round() as f32], hit));
    })
}

/// Background, zone grid and zone codes (never interactive).
fn map_base(o: &Open, g: Geom) -> Html {
    let stroke = g.width / 500.0;
    let zones: Vec<String> = list(&o.reference["zones"]).iter().map(|z| text(z).to_string()).collect();
    let centres = &o.reference["zone_centres"];
    html! {<>
        <rect x="0" y="0" width={g.width.to_string()} height={g.height.to_string()} fill="#e9efe6"/>
        {for (0..=g.cols).map(|c| html!{<line x1={(c as f64 * g.zone).to_string()} y1="0" x2={(c as f64 * g.zone).to_string()} y2={(g.rows as f64 * g.zone).to_string()} stroke="#9db3a6" stroke-width={stroke.to_string()}/>})}
        {for (0..=g.rows).map(|r| html!{<line x1="0" y1={(g.height - r as f64 * g.zone).to_string()} x2={(g.cols as f64 * g.zone).to_string()} y2={(g.height - r as f64 * g.zone).to_string()} stroke="#9db3a6" stroke-width={stroke.to_string()}/>})}
        {for zones.iter().map(|z| {
            let c = &centres[z.as_str()];
            html!{<text class="zone-lbl" x={c[0].as_f64().unwrap_or(0.0).to_string()} y={(g.height - c[1].as_f64().unwrap_or(0.0)).to_string()}
                text-anchor="middle" dominant-baseline="central" font-size={(g.zone * 0.2).to_string()}>{z.clone()}</text>}
        })}
    </>}
}

/// Named places as small diamonds. `always` writes the name; otherwise it shows on hover.
fn map_places(places: &[Place], g: Geom, always: bool, selected: Option<usize>, hit: bool) -> Html {
    let r = g.zone * 0.07;
    html! {<>{for places.iter().enumerate().map(|(i, p)| {
        let (cx, cy) = (p.pos[0] as f64, g.height - p.pos[1] as f64);
        let sel = selected == Some(i);
        let d = format!("M{} {}L{} {}L{} {}L{} {}Z", cx, cy - r, cx + r, cy, cx, cy + r, cx - r, cy);
        html!{<g class={classes!("map-place", always.then_some("always"), sel.then_some("selected"))} data-hit={hit.then(|| i.to_string())}>
            <title>{if p.aliases.is_empty() { p.name.clone() } else { format!("{} ({})", p.name, p.aliases.join(", ")) }}</title>
            <circle cx={cx.to_string()} cy={cy.to_string()} r={(r * 2.2).to_string()} fill="rgba(0,0,0,.001)"/>
            <path d={d} fill={if sel {"#0f6b4d"} else {"#617673"}} stroke="#fff" stroke-width={(g.width / 700.0).to_string()}/>
            <text class="map-lbl" x={cx.to_string()} y={(cy - r * 1.6).to_string()} text-anchor="middle" font-size={(g.zone * 0.15).to_string()} stroke-width={(g.zone * 0.035).to_string()}>{p.name.clone()}</text>
        </g>}
    })}</>}
}

/// A delete button that needs a second click, so a stray tap cannot lose data.
#[derive(Properties, PartialEq)]
struct ConfirmProps {
    label: &'static str,
    onconfirm: Callback<MouseEvent>,
}

#[function_component(ConfirmDelete)]
fn confirm_delete(p: &ConfirmProps) -> Html {
    let armed = use_state(|| false);
    if *armed {
        let (a, cb) = (armed.clone(), p.onconfirm.clone());
        let b = armed.clone();
        html! {<span class="confirm-delete">
            <button class="danger" onclick={Callback::from(move |e| { a.set(false); cb.emit(e) })}>{ex("Conferma eliminazione")}</button>
            <button onclick={Callback::from(move |_| b.set(false))}>{ex("Annulla")}</button>
        </span>}
    } else {
        let a = armed.clone();
        html! {<button class="danger" title={ex("Chiede una conferma prima di eliminare")} onclick={Callback::from(move |_| a.set(true))}>{ex(p.label)}</button>}
    }
}

/// `options` plus the current value when it is not among them (a dangling reference stays visible).
fn with_current(options: &[(String, String)], value: &str) -> Vec<(String, String)> {
    let mut v = options.to_vec();
    if !options.iter().any(|(id, _)| id == value) {
        v.insert(0, (value.to_string(), if value.is_empty() { ex("— scegli —").to_string() } else { format!("{value} ⚠") }));
    }
    v
}

/// Zone grid and ignitions; a click on the map moves the selected ignition.
fn ignitions_tab(o: &Open, open: &UseStateHandle<Option<Open>>, selected: &UseStateHandle<usize>, map: &NodeRef) -> Html {
    let ignitions = o.pack.edits.initial_ignitions.clone().unwrap_or_default();
    let g = geom(o);
    let sel = (*selected).min(ignitions.len().saturating_sub(1));
    let onclick = {
        let (open, selected) = (open.clone(), selected.clone());
        map_click(map, g, Callback::from(move |(at, hit): ([f32; 2], Option<usize>)| {
            if let Some(i) = hit {
                selected.set(i);
                return;
            }
            let Some(cur) = &*open else { return };
            let mut next = cur.clone();
            if let Some(i) = next.pack.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(sel)) {
                i.at = at;
                next.dirty = true;
                open.set(Some(next));
            }
        }))
    };
    let zones: Vec<String> = list(&o.reference["zones"]).iter().map(|z| text(z).to_string()).collect();
    let centres = o.reference["zone_centres"].clone();
    let places = o.pack.edits.places.clone().unwrap_or_default();
    let unit = g.zone;
    html! {<div class="sc-ign">
        <div class="sc-split">
            <div class="sc-map">
                <p class="map-help"><strong>{ex("Mappa: clic su un punto vuoto = sposta l’innesco selezionato · clic su un cerchio = lo seleziona.")}</strong>{" "}
                    {ex("Origine in basso a sinistra (sud-ovest); i cerchi sono disegnati in scala col raggio. Passa il mouse sui rombi per il nome dei luoghi.")}</p>
                <svg ref={map.clone()} class="ignition-map" viewBox={format!("0 0 {} {}", g.width, g.height)} style={format!("aspect-ratio:{}/{}", g.width, g.height)}
                    role="img" aria-label={ex("Mappa degli inneschi")} onclick={onclick}>
                    {map_base(o, g)}
                    {map_places(&places, g, false, None, false)}
                    {for ignitions.iter().enumerate().map(|(i, g_)| {
                        let (cx, cy) = (g_.at[0] as f64, g.height - g_.at[1] as f64);
                        let is_sel = i == sel;
                        let r = g_.radius_m as f64;
                        let hit_r = r.max(unit * 0.07);
                        let tiny = g_.radius_m < 40.0;
                        html!{<g class={classes!("map-ign", is_sel.then_some("selected"), tiny.then_some("tiny"))} data-hit={i.to_string()}>
                            <title>{format!("{} {} · {} m{}", ex("Innesco"), i + 1, g_.radius_m, if tiny { format!(" · {}", ex("troppo piccolo")) } else { String::new() })}</title>
                            <circle cx={cx.to_string()} cy={cy.to_string()} r={hit_r.to_string()} fill="rgba(0,0,0,.001)"/>
                            <circle class="ign-disc" cx={cx.to_string()} cy={cy.to_string()} r={r.to_string()} stroke-width={(g.width / 450.0 * if is_sel {2.0} else {1.0}).to_string()}/>
                            if is_sel {
                                <circle class="ign-ring" cx={cx.to_string()} cy={cy.to_string()} r={(hit_r + unit * 0.03).to_string()} stroke-width={(g.width / 300.0).to_string()} stroke-dasharray={format!("{} {}", unit * 0.05, unit * 0.03)}/>
                            }
                            <circle cx={cx.to_string()} cy={cy.to_string()} r={(unit * 0.012).to_string()} fill="#7a1d12"/>
                            <text class="map-lbl ign-num" x={cx.to_string()} y={(cy - hit_r - unit * 0.05).to_string()} text-anchor="middle"
                                font-size={(unit * if is_sel {0.2} else {0.15}).to_string()} stroke-width={(unit * 0.04).to_string()}>{i + 1}</text>
                        </g>}
                    })}
                </svg>
                <p class="muted map-legend">{ex("Cerchio pieno = innesco, con anello tratteggiato se selezionato · tratteggio = raggio sotto i 40 m (si spegne) · rombo = luogo con nome.")}</p>
            </div>
            <div class="sc-list">
                {for ignitions.iter().enumerate().map(|(i, g_)| {
                    let g_ = g_.clone();
                    let sel_cb = { let selected = selected.clone(); Callback::from(move |_: MouseEvent| selected.set(i)) };
                    let zone_now = zone_at(o, g, g_.at);
                    if i != sel {
                        return html!{<button class="sc-summary" key={i} onclick={sel_cb} title={ex("Seleziona questo innesco (si evidenzia sulla mappa)")}>
                            <span class="badge">{i + 1}</span>
                            {format!("{} {} · {} m · E {} N {}{}", ex("Innesco"), i + 1, g_.radius_m, g_.at[0], g_.at[1], zone_now.map(|z| format!(" · {z}")).unwrap_or_default())}
                        </button>};
                    }
                    let mut zone_options: Vec<(String, String)> = vec![(String::new(), match &zone_now {
                        Some(z) => format!("{} {z} · {}", ex("Ora in zona"), ex("sposta al centro di…")),
                        None => ex("Sposta al centro della zona…").to_string(),
                    })];
                    zone_options.extend(zones.iter().map(|z| (z.clone(), z.clone())));
                    let centres = centres.clone();
                    html!{<fieldset class="inject-row selected" key={i}>
                        <legend><span class="badge sel">{i + 1}</span>{format!(" {} {}", ex("Innesco"), i + 1)}</legend>
                        <div class="settings-fields">
                            {number("Est x (m)", "Metri verso est dall’angolo sud-ovest del territorio (origine 0,0). Si imposta anche con un clic sulla mappa.", g_.at[0].to_string(), upd(open, move |p, v: String| if let (Some(x), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[0] = x }))}
                            {number("Nord y (m)", "Metri verso nord dall’angolo sud-ovest del territorio (origine 0,0). Si imposta anche con un clic sulla mappa.", g_.at[1].to_string(), upd(open, move |p, v: String| if let (Some(y), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[1] = y }))}
                            {number("Raggio (m)", "Raggio iniziale del fuoco, disegnato in scala sulla mappa. Con meno di 40 m l’innesco si spegne da solo (una sola cella non si propaga): usa 40 m o più.", g_.radius_m.to_string(), upd(open, move |p, v: String| if let (Some(r), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.radius_m = r }))}
                            {choice("Zona", "Scorciatoia: porta l’innesco al centro di una zona della griglia 8×8 (es. F4), il nome che i giocatori usano nei messaggi.", "", &zone_options, upd(open, move |p, zone: String| {
                                let c = &centres[zone.as_str()];
                                if let (Some(x), Some(y), Some(g)) = (c[0].as_f64(), c[1].as_f64(), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at = [x as f32, y as f32]; }
                            }))}
                        </div>
                        if g_.radius_m < 40.0 {<p class="warn" role="alert">{ex("Raggio sotto i 40 m: l’innesco si spegne da solo. Portalo a 40 m o più.")}</p>}
                        <ConfirmDelete label="Elimina innesco" onconfirm={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.initial_ignitions.as_mut() { if i < l.len() { l.remove(i); } } })}/>
                    </fieldset>}
                })}
                if ignitions.is_empty() {<p class="muted">{ex("Nessun innesco: l’esercitazione parte senza fuoco. Aggiungine uno e clicca sulla mappa.")}</p>}
                <button class="primary" title={ex("Crea un nuovo innesco al centro della mappa e lo seleziona")} onclick={{
                    let (selected, n) = (selected.clone(), ignitions.len());
                    let add = upd(open, |p, _: MouseEvent| {
                        p.edits.initial_ignitions.get_or_insert_with(Vec::new).push(IgnitionSpec { at: [1000.0, 1000.0], radius_m: 100.0 });
                    });
                    Callback::from(move |e: MouseEvent| { selected.set(n); add.emit(e) })
                }}>{ex("Aggiungi innesco")}</button>
            </div>
        </div>
    </div>}
}

fn reinforcements_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let list_ = e.reinforcements.clone().unwrap_or_default();
    let resources: Vec<(String, String)> = e.resource_types.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let roles: Vec<(String, String)> = e.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    html! {<div class="sc-reinf">
        <p class="muted">{ex("Capacità esterna che un ruolo può richiedere: arriva dopo il ritardo indicato. L’EXCON approva comunque ogni richiesta e ogni arrivo.")}</p>
        if resources.is_empty() || roles.is_empty() {<p class="warn" role="alert">{ex("Servono almeno una risorsa e un ruolo (schede Risorse e Ruoli) per definire un rinforzo.")}</p>}
        {for list_.iter().enumerate().map(|(i, r)| {
            let r = r.clone();
            html!{<div class="inject-row sc-reinf-row" role="group" aria-label={format!("{} {} → {}", ex("Rinforzo"), r.resource, r.assigned_to)} key={i}>
                {choice("Risorsa", "Che cosa arriva: uno dei tipi definiti nella scheda Risorse.", &r.resource, &with_current(&resources, &r.resource), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.resource = v }))}
                {number("Quantità massima", "Capacità massima che il ruolo può richiedere in totale da questa fonte (unità del tipo di risorsa).", r.max_qty.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.max_qty = n }))}
                {number("Ritardo (min)", "Minuti simulati tra l’approvazione della richiesta e l’arrivo alla base di partenza.", r.delay_min.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.delay_min = n }))}
                {field("Provenienza", "Testo libero: da dove arrivano (es. «Colonna mobile regionale»). Compare nei messaggi ai giocatori.", r.source.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.source = v }))}
                {choice("Riceve il ruolo", "Il ruolo (pagina player) che può richiedere e che riceve le unità all’arrivo.", &r.assigned_to, &with_current(&roles, &r.assigned_to), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.assigned_to = v }))}
                <div class="sc-del"><ConfirmDelete label="Elimina rinforzo" onconfirm={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.reinforcements.as_mut() { if i < l.len() { l.remove(i); } } })}/></div>
            </div>}
        })}
        <button onclick={{
            let first_resource = resources.first().map(|r| r.0.clone()).unwrap_or_default();
            let first_role = roles.first().map(|r| r.0.clone()).unwrap_or_default();
            upd(open, move |p, _: MouseEvent| {
                p.edits.reinforcements.get_or_insert_with(Vec::new).push(ReinforcementOffer {
                    resource: first_resource.clone(), max_qty: 1, delay_min: 60, source: String::new(), assigned_to: first_role.clone(),
                });
            })
        }}>{ex("Aggiungi rinforzo")}</button>
    </div>}
}

#[derive(Properties, Clone)]
struct PlacesProps {
    o: Open,
    open: UseStateHandle<Option<Open>>,
}

impl PartialEq for PlacesProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

fn places_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<PlacesTab o={o.clone()} open={open.clone()}/>}
}

#[function_component(PlacesTab)]
fn places_view(props: &PlacesProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected = use_state(|| 0usize);
    let map = use_node_ref();
    let places = o.pack.edits.places.clone().unwrap_or_default();
    let ignitions = o.pack.edits.initial_ignitions.clone().unwrap_or_default();
    let g = geom(o);
    let sel = (*selected).min(places.len().saturating_sub(1));
    let onclick = {
        let (open, selected) = (open.clone(), selected.clone());
        map_click(&map, g, Callback::from(move |(at, hit): ([f32; 2], Option<usize>)| {
            if let Some(i) = hit {
                selected.set(i);
                return;
            }
            let Some(cur) = &*open else { return };
            let mut next = cur.clone();
            if let Some(p) = next.pack.edits.places.as_mut().and_then(|l| l.get_mut(sel)) {
                p.pos = at;
                next.dirty = true;
                open.set(Some(next));
            }
        }))
    };
    html! {<div class="sc-places">
        <p class="muted">{ex("Luoghi con nome: l’AI li usa per capire «la cresta» o «il municipio». Gli alias sono separati da virgola. I luoghi servono anche come base di partenza delle risorse (scheda Generale).")}</p>
        <div class="sc-split">
            <div class="sc-map">
                <p class="map-help"><strong>{ex("Mappa: clic su un punto vuoto = sposta il luogo selezionato · clic su un rombo = lo seleziona.")}</strong>{" "}
                    {ex("Origine in basso a sinistra (sud-ovest). I cerchi arancioni sono gli inneschi, per orientarti.")}</p>
                <svg ref={map.clone()} class="ignition-map" viewBox={format!("0 0 {} {}", g.width, g.height)} style={format!("aspect-ratio:{}/{}", g.width, g.height)}
                    role="img" aria-label={ex("Mappa dei luoghi")} onclick={onclick}>
                    {map_base(o, g)}
                    {for ignitions.iter().map(|i| html!{<circle class="ign-disc faint" cx={i.at[0].to_string()} cy={(g.height - i.at[1] as f64).to_string()} r={i.radius_m.to_string()}/>})}
                    {map_places(&places, g, true, Some(sel), true)}
                </svg>
            </div>
            <div class="sc-list">
                {for places.iter().enumerate().map(|(i, pl)| {
                    let pl = pl.clone();
                    if i != sel {
                        let selected = selected.clone();
                        return html!{<button class="sc-summary" key={i} onclick={Callback::from(move |_| selected.set(i))} title={ex("Seleziona questo luogo (si evidenzia sulla mappa)")}>
                            <span class="badge">{"◆"}</span>
                            {format!("{} · E {} N {}{}", pl.name, pl.pos[0], pl.pos[1], zone_at(o, g, pl.pos).map(|z| format!(" · {z}")).unwrap_or_default())}
                        </button>};
                    }
                    html!{<fieldset class="inject-row selected" key={i}>
                        <legend><span class="badge sel">{"◆"}</span>{format!(" {}", pl.name)}</legend>
                        <div class="settings-fields">
                            {field("Nome", "Nome ufficiale del luogo, come lo chiamano i giocatori (es. «Cresta di Valverde»).", pl.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                            {field("Alias", "Altri modi di chiamarlo, separati da virgola (es. «la cresta, il crinale»). L’AI li usa per riconoscere il luogo nei messaggi.", pl.aliases.join(", "), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.aliases = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() }))}
                            {number("Est x (m)", "Metri verso est dall’angolo sud-ovest del territorio. Si imposta anche con un clic sulla mappa.", pl.pos[0].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[0] = n }))}
                            {number("Nord y (m)", "Metri verso nord dall’angolo sud-ovest del territorio. Si imposta anche con un clic sulla mappa.", pl.pos[1].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[1] = n }))}
                        </div>
                        <ConfirmDelete label="Elimina luogo" onconfirm={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.places.as_mut() { if i < l.len() { l.remove(i); } } })}/>
                    </fieldset>}
                })}
                if places.is_empty() {<p class="muted">{ex("Nessun luogo definito.")}</p>}
                <button class="primary" title={ex("Crea un nuovo luogo e lo seleziona: poi clicca sulla mappa per posizionarlo")} onclick={{
                    let (selected, n) = (selected.clone(), places.len());
                    let add = upd(open, |p, _: MouseEvent| {
                        let l = p.edits.places.get_or_insert_with(Vec::new);
                        let n = l.len() + 1;
                        l.push(Place { name: format!("Luogo {n}"), aliases: vec![], pos: [1000.0, 1000.0] });
                    });
                    Callback::from(move |e: MouseEvent| { selected.set(n); add.emit(e) })
                }}>{ex("Aggiungi luogo")}</button>
            </div>
        </div>
    </div>}
}

#[derive(Properties, Clone)]
struct InjectsProps {
    o: Open,
    open: UseStateHandle<Option<Open>>,
    json: UseStateHandle<String>,
    message: UseStateHandle<String>,
}

impl PartialEq for InjectsProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

fn injects_tab(o: &Open, open: &UseStateHandle<Option<Open>>, json: &UseStateHandle<String>, message: &UseStateHandle<String>) -> Html {
    html! {<InjectsTab o={o.clone()} open={open.clone()} json={json.clone()} message={message.clone()}/>}
}

#[function_component(InjectsTab)]
fn injects_view(props: &InjectsProps) -> Html {
    use plan::{Key, Kind, Target};
    let (o, open, json, message) = (&props.o, &props.open, &props.json, &props.message);
    let selected = use_state(|| 0usize);
    // `Some(i)`: the i-th scripted action is selected instead of an inject.
    let act = use_state(|| Option::<usize>::None);
    let cmd_edit = use_state(|| (usize::MAX, String::new()));
    // The EXCON control tool (see `controllo`) currently building an action's command.
    let tool = use_state(|| Option::<&'static str>::None);
    // Band whose header was clicked: where "Aggiungi" puts new items.
    let band_sel = use_state(|| Option::<usize>::None);
    // Row being dragged (kept outside the render so a drop always sees it) and the drop target under the pointer.
    let drag_src = use_mut_ref(|| Option::<Key>::None);
    let dragging = use_state_eq(|| Option::<Key>::None);
    let hover = use_state_eq(|| Option::<Target>::None);
    let e = &o.pack.edits;
    let msel = e.msel.clone().unwrap_or_default();
    let actions = e.timeline.clone().unwrap_or_default();
    let (n, na) = (msel.len(), actions.len());
    let tm = e.turn_minutes.unwrap_or(plan::DEFAULT_TURN_MINUTES).clamp(1, 1440);
    let start_s = e.start_clock_s.unwrap_or(0);
    let roles: Vec<(String, String)> = e.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let excon_roles: Vec<String> = e.excon_roles.clone().unwrap_or_default();
    let sel = (*selected).min(n.saturating_sub(1));
    let entries = Rc::new(plan::sorted_entries(&msel, &actions, tm));
    let bands = Rc::new(plan::bands(&entries, tm));
    let warns = plan::action_warnings(&msel, &actions, tm);
    let implicit = entries.iter().filter(|x| !x.explicit).count();
    let current: Option<Key> = match *act {
        Some(i) if i < na => Some(Key { kind: Kind::Action, idx: i }),
        _ if n > 0 => Some(Key { kind: Kind::Inject, idx: sel }),
        _ => None,
    };
    let select = {
        let (selected, act, band_sel) = (selected.clone(), act.clone(), band_sel.clone());
        Callback::from(move |k: Key| {
            band_sel.set(None);
            match k.kind {
                Kind::Inject => {
                    act.set(None);
                    selected.set(k.idx)
                }
                Kind::Action => act.set(Some(k.idx)),
            }
        })
    };
    // The resulting time of the drop under the pointer, shown while dragging.
    let hint = match (*dragging, *hover) {
        (Some(k), Some(t)) => plan::resolve_drop(&entries, &bands, k, t, tm)
            .map(|d| format!("→ {} · {} {}", scenario::format_clock(start_s + d.at_s), ex("turno"), plan::turn_of(d.at_s, tm))),
        _ => None,
    };
    let do_drop = {
        let (open, drag_src, dragging, hover, selected, act, entries, bands) =
            (open.clone(), drag_src.clone(), dragging.clone(), hover.clone(), selected.clone(), act.clone(), entries.clone(), bands.clone());
        Callback::from(move |target: Target| {
            let key = drag_src.borrow_mut().take();
            dragging.set(None);
            hover.set(None);
            let (Some(key), Some(cur)) = (key, &*open) else { return };
            let Some(d) = plan::resolve_drop(&entries, &bands, key, target, tm) else { return };
            if d.anchor.is_none() && entries.iter().any(|x| x.key == key && x.at_s == d.at_s) {
                return;
            }
            let mut next = cur.clone();
            let idx = plan::apply_drop(&mut next.pack.edits, key, &d);
            next.dirty = true;
            open.set(Some(next));
            match key.kind {
                Kind::Inject => {
                    act.set(None);
                    selected.set(idx)
                }
                Kind::Action => act.set(Some(idx)),
            }
        })
    };
    let nudge = |key: Key, at_s: i64, minutes: i64| upd(open, move |p, _: ()| plan::set_time(&mut p.edits, key, at_s + minutes * 60));
    let band_title = |b: &plan::Band| {
        if b.first_turn == b.last_turn {
            format!("{} {}", ex("Turno"), b.first_turn)
        } else {
            format!("{} {}–{}", ex("Turni"), b.first_turn, b.last_turn)
        }
    };
    let items = html! {<>
        {for bands.iter().enumerate().map(|(b, band)| {
            let (ni, nact) = band.items.iter().fold((0, 0), |(i, a), &p| if entries[p].key.kind == Kind::Inject { (i + 1, a) } else { (i, a + 1) });
            let counts = if band.items.is_empty() {
                ex("vuoto").to_string()
            } else {
                crate::exfmt!("{} inject · {} azioni", "{} injects · {} actions", ni, nact)
            };
            let on = *hover == Some(Target::Band(b));
            let over = {
                let (hover, drag_src) = (hover.clone(), drag_src.clone());
                Callback::from(move |ev: DragEvent| {
                    if drag_src.borrow().is_none() { return; }
                    ev.prevent_default();
                    hover.set(Some(Target::Band(b)));
                })
            };
            let drop = { let do_drop = do_drop.clone(); Callback::from(move |ev: DragEvent| { ev.prevent_default(); do_drop.emit(Target::Band(b)) }) };
            let header_click = { let band_sel = band_sel.clone(); Callback::from(move |_| band_sel.set(if *band_sel == Some(b) { None } else { Some(b) })) };
            html!{<>
                <div class={classes!("tl-band", (*band_sel == Some(b)).then_some("active"), on.then_some("drop-on"))}
                    data-drop={on.then(|| hint.clone()).flatten()} title={ex("Clic: i nuovi elementi partono da questo turno. Trascina qui una riga per portarla all’inizio del turno.")}
                    onclick={header_click} ondragover={over.clone()} ondrop={drop.clone()}>
                    <b>{band_title(band)}</b>
                    <span class="tl-range">{plan::band_range(start_s, band)}</span>
                    <span class={classes!("tl-count", band.items.is_empty().then_some("none"))}>{counts}</span>
                </div>
                {if band.items.is_empty() { html!{<div class="tl-empty" ondragover={over} ondrop={drop}>{ex("nessun elemento")}</div>} } else { Html::default() }}
                {for band.items.iter().map(|&pos| {
                    let en = &entries[pos];
                    let key = en.key;
                    let nr = NodeRef::default();
                    let abs = start_s + en.at_s;
                    let (id, title, sub, glyph, tip) = match key.kind {
                        Kind::Inject => {
                            let m = &msel[key.idx];
                            (m.id.clone(), m.title.clone(), m.event.clone(), "●", ex("Inject: messaggio ai giocatori"))
                        }
                        Kind::Action => {
                            let a = &actions[key.idx];
                            (a.id.clone(), a.title.clone(), a.command["type"].as_str().unwrap_or("?").to_string(), "◆", ex("Azione pianificata: cambia il mondo"))
                        }
                    };
                    let title = if title.trim().is_empty() { ex("(senza titolo)").to_string() } else { title };
                    let warn = if key.kind == Kind::Action { warns.get(key.idx).cloned().flatten() } else { None };
                    let after = if key.kind == Kind::Action { actions[key.idx].after.clone() } else { None };
                    let link = match (&after, &warn) {
                        (_, Some(plan::Warn::BeforeInject(i, t))) => Some(format!("⚠ {} {i} ({})", ex("prima dell’inject"), plan::hhmm(start_s + t))),
                        (_, Some(plan::Warn::UnknownInject(i))) => Some(format!("⚠ {} {i}", ex("inject sconosciuto"))),
                        (Some(i), None) => Some(format!("↳ {} {i}", ex("segue"))),
                        _ => None,
                    };
                    let target_for = {
                        let nr = nr.clone();
                        move |ev: &DragEvent| {
                            let upper = nr.cast::<web_sys::Element>().is_none_or(|el| {
                                let r = el.get_bounding_client_rect();
                                (ev.client_y() as f64) < r.top() + r.height() / 2.0
                            });
                            if upper { Target::Before(pos) } else { Target::After(pos) }
                        }
                    };
                    let over_row = {
                        let (hover, drag_src, target_for) = (hover.clone(), drag_src.clone(), target_for.clone());
                        Callback::from(move |ev: DragEvent| {
                            if drag_src.borrow().is_none() { return; }
                            ev.prevent_default();
                            hover.set(Some(target_for(&ev)));
                        })
                    };
                    let drop_row = { let do_drop = do_drop.clone(); Callback::from(move |ev: DragEvent| { ev.prevent_default(); ev.stop_propagation(); do_drop.emit(target_for(&ev)) }) };
                    let start = {
                        let (drag_src, dragging, id) = (drag_src.clone(), dragging.clone(), id.clone());
                        Callback::from(move |ev: DragEvent| {
                            if let Some(dt) = ev.data_transfer() {
                                let _ = dt.set_data("text/plain", &id);
                                dt.set_effect_allowed("move");
                            }
                            *drag_src.borrow_mut() = Some(key);
                            dragging.set(Some(key));
                        })
                    };
                    let end = { let (drag_src, dragging, hover) = (drag_src.clone(), dragging.clone(), hover.clone()); Callback::from(move |_: DragEvent| { *drag_src.borrow_mut() = None; dragging.set(None); hover.set(None); }) };
                    let keys = {
                        let (select, nudge_up, nudge_down) = (select.clone(), nudge(key, en.at_s, -5), nudge(key, en.at_s, 5));
                        Callback::from(move |ev: KeyboardEvent| match (ev.key().as_str(), ev.alt_key()) {
                            ("ArrowUp", true) => { ev.prevent_default(); nudge_up.emit(()) }
                            ("ArrowDown", true) => { ev.prevent_default(); nudge_down.emit(()) }
                            ("Enter" | " ", false) => { ev.prevent_default(); select.emit(key) }
                            _ => {}
                        })
                    };
                    let (is_before, is_after) = (*hover == Some(Target::Before(pos)), *hover == Some(Target::After(pos)));
                    let shows = (is_before || is_after) && *dragging != Some(key);
                    let elapsed = en.at_s / 60;
                    html!{<div ref={nr} role="button" tabindex="0" draggable="true" data-key={format!("{}:{}", if key.kind == Kind::Inject { "inject" } else { "action" }, key.idx)}
                        class={classes!("tl-row", if key.kind == Kind::Inject { "inject" } else { "action" }, (current == Some(key)).then_some("active"),
                            (*dragging == Some(key)).then_some("dragging"), (shows && is_before).then_some("drop-before"), (shows && is_after).then_some("drop-after"),
                            warn.is_some().then_some("warn"))}
                        data-drop={shows.then(|| hint.clone()).flatten()}
                        title={ex("Trascina per cambiare l’orario (metà superiore: dopo la riga precedente; metà inferiore: stesso orario di questa riga). Alt+↑/↓ sposta di 5 minuti.")}
                        onclick={{ let select = select.clone(); Callback::from(move |_| select.emit(key)) }}
                        onkeydown={keys} ondragstart={start} ondragend={end} ondragover={over_row} ondrop={drop_row}>
                        <span class="tl-kind" title={tip}>{glyph}</span>
                        <span class={classes!("tl-time", (!en.explicit).then_some("implied"))}
                            title={if en.explicit { scenario::format_clock(abs) } else { format!("{} · {}", scenario::format_clock(abs), ex("implicito: calcolato dal turno suggerito")) }}>
                            {format!("{}{}", if en.explicit { "" } else { "~" }, scenario::format_clock(abs))}
                            <small>{format!(" +{elapsed}m")}</small>
                        </span>
                        <span class="md-id">{id}</span>
                        <span class="md-name">{title}</span>
                        {if sub.trim().is_empty() { Html::default() } else { html!{<small class="md-sub">{sub}</small>} }}
                        {if let Some(l) = link { html!{<small class={classes!("tl-link", warn.is_some().then_some("bad"))}>{l}</small>} } else { Html::default() }}
                    </div>}
                })}
            </>}
        })}
        {if n + na == 0 { html!{<p class="muted">{ex("Nessun inject: aggiungine uno per iniziare.")}</p>} } else { Html::default() }}
    </>};
    let new_time = plan::new_item_time(&entries, &bands, *band_sel);
    let tools = html! {<>
        <span class="muted">{format!("{n} inject · {na} {}", ex("azioni"))}</span>
        {add_inject_button(open, &selected, &act, &band_sel, n, new_time, tm)}
        <button class="primary" title={ex("Aggiunge un’azione pianificata: un cambiamento del mondo (es. vento, ignizione) che l’EXCON decide quando il tempo avanza oltre il suo orario.")} onclick={{
            let (act, band_sel, add) = (act.clone(), band_sel.clone(), upd(open, move |p, _: MouseEvent| {
                let l = p.edits.timeline.get_or_insert_with(Vec::new);
                let mut k = l.len() + 1;
                while l.iter().any(|a| a.id == format!("A-{k:02}")) { k += 1; }
                l.push(scenario::TimelineAction {
                    id: format!("A-{k:02}"), title: String::new(), at_s: new_time,
                    command: serde_json::json!({"type": "SetWeather", "wind_speed_kmh": 30.0}),
                    after: None, note: String::new(),
                });
            }));
            Callback::from(move |e| { band_sel.set(None); act.set(Some(na)); add.emit(e) })
        }}>{ex("Aggiungi azione")}</button>
        <button title={ex("Riordina i due elenchi per orario (inject e azioni), mantenendo l’ordine attuale a parità di orario.")} onclick={{
            let (selected, act, open2) = (selected.clone(), act.clone(), open.clone());
            let (cur_sel, cur_act) = (sel, *act);
            let sort = upd(open, move |p, _: MouseEvent| { plan::sort_by_time(&mut p.edits); });
            Callback::from(move |e| {
                // The selection follows the item it points at.
                if let Some(cur) = &*open2 {
                    let mut probe = cur.pack.edits.clone();
                    let (im, am) = plan::sort_by_time(&mut probe);
                    if let Some(&k) = im.get(cur_sel) { selected.set(k); }
                    if let Some(k) = cur_act.and_then(|i| am.get(i).copied()) { act.set(Some(k)); }
                }
                sort.emit(e)
            })
        }}>{ex("Ordina per orario")}</button>
        <button disabled={implicit == 0} title={ex("Scrive l’orario che il turno implica in ogni inject che non ne ha uno proprio: da quel momento non dipendono più dalla durata del turno.")} onclick={upd(open, |p, _: MouseEvent| { plan::make_explicit(&mut p.edits); })}>{crate::exfmt!("Rendi espliciti i tempi ({})", "Make times explicit ({})", implicit)}</button>
    </>};
    let detail = match (act.and_then(|i| actions.get(i).map(|a| (i, a))), msel.get(sel)) {
        (Some((i, a)), _) => action_detail(i, a, &msel, warns.get(i).cloned().flatten(), tm, o, open, &act, &cmd_edit, &tool),
        (None, Some(m)) => inject_detail(sel, n, m, tm, start_s, &roles, &excon_roles, open, &selected),
        (None, None) => html! {<p class="muted">{ex("Nessun inject: aggiungine uno per iniziare.")}</p>},
    };
    html! {<div class="inject-tab">
        <p class="muted">{ex("Una sola timeline in ordine di tempo, divisa in turni. Gli inject (●) sono messaggi preparati: l’EXCON decide quando inviarli e, quando il tempo li raggiunge, li trova proposti. Le azioni pianificate (◆) cambiano il mondo a un istante preciso: a ogni avanzamento del tempo che le attraversa l’EXCON sceglie se eseguirle, saltarle o rinviarle. Trascina le righe per cambiarne l’orario.")}</p>
        <div class="md tl-md">
            <div class="md-list tl-list">
                <div class="md-tools tl-tools">{tools}</div>
                <div class="md-items tl-items" ondragleave={{ let hover = hover.clone(); Callback::from(move |ev: DragEvent| if ev.related_target().is_none() { hover.set(None) }) }}>{items}</div>
            </div>
            <div class="md-detail">{detail}</div>
        </div>
        <details class="inject-json">
            <summary title={ex("Copia o sostituisci l’intera lista degli inject come testo JSON.")}>{ex("Avanzate: JSON (solo inject)")}</summary>
            <p class="hint">{ex("Riguarda solo la lista degli inject, non l’intero scenario (per quello usa «Avanzate: JSON» in fondo all’editor). Esporta mostra gli inject qui sotto per copiarli. Importa sostituisce l’intera lista nell’editor: non viene salvato nulla finché non premi Salva.")}</p>
            <div class="settings-fields">
                <button title={ex("Scrive nel riquadro il JSON degli inject attuali.")} onclick={{
                    let (json, msel) = (json.clone(), msel.clone());
                    Callback::from(move |_| json.set(serde_json::to_string_pretty(&msel).unwrap_or_default()))
                }}>{ex("Esporta JSON")}</button>
                <button title={ex("Sostituisce tutti gli inject dell’editor con il JSON nel riquadro. Nulla è salvato finché non premi Salva.")} onclick={{
                    let (json, message) = (json.clone(), message.clone());
                    upd(open, move |p, _: MouseEvent| match serde_json::from_str::<Vec<InjectTemplate>>(&json) {
                        Ok(v) => { p.edits.msel = Some(v); message.set(ex("Inject importati nell’editor · salva per conservarli").to_string()); }
                        Err(e) => message.set(format!("JSON non valido: {e}")),
                    })
                }}>{ex("Importa JSON")}</button>
            </div>
            <textarea aria-label={ex("JSON degli inject")} rows="6" value={(**json).clone()} oninput={{
                let json = json.clone();
                Callback::from(move |e: InputEvent| json.set(api::input(e)))
            }}/>
        </details>
    </div>}
}

/// Time controls shared by the inject and action editors: HH:MM, minutes from
/// the start, ±5 min and the start of the turn. The turn is derived.
fn time_block(key: plan::Key, at_s: i64, explicit: bool, tm: u32, start_s: i64, open: &UseStateHandle<Option<Open>>) -> Html {
    let abs = start_s + at_s;
    let set = upd(open, move |p, t: i64| plan::set_time(&mut p.edits, key, t));
    let clock_cb = {
        let set = set.clone();
        Callback::from(move |v: String| {
            if let Some(t) = parse_clock(abs.div_euclid(86_400) + 1, &v).map(|t| t - start_s).filter(|t| *t >= 0) {
                set.emit(t)
            }
        })
    };
    let min_cb = {
        let set = set.clone();
        Callback::from(move |v: String| {
            if let Some(m) = num::<f64>(&v).filter(|m| *m >= 0.0) {
                set.emit((m * 60.0).round() as i64)
            }
        })
    };
    let step = |d: i64| {
        let set = set.clone();
        Callback::from(move |_: MouseEvent| set.emit((at_s + d * 60).max(0)))
    };
    let to_turn = {
        let set = set.clone();
        Callback::from(move |_: MouseEvent| set.emit(plan::turn_start(at_s, tm)))
    };
    let turn = plan::turn_of(at_s, tm);
    let day = abs.div_euclid(86_400) + 1;
    let help = ex("Istante pianificato, nell’ora simulata dello scenario. L’EXCON decide cosa farne quando il tempo avanza oltre questo istante.");
    html! {<div class="tl-time-block">
        <div class="tl-time-fields">
            <label title={help}>{ex("Ora (HH:MM)")}<input type="time" required=true value={clock_text(abs)} oninput={clock_cb.reform(api::input)}/></label>
            <span class="tl-day">{format!("{} {day}", ex("Giorno"))}</span>
            <label title={ex("Minuti trascorsi dall’inizio dell’esercitazione.")}>{ex("Minuti dall’inizio")}<input type="number" min="0" step="5" value={fmt(at_s as f64 / 60.0)} oninput={min_cb.reform(api::input)}/></label>
            <div class="tl-nudge">
                <button type="button" title={ex("Anticipa di 5 minuti")} disabled={at_s < 300} onclick={step(-5)}>{"−5"}</button>
                <button type="button" title={ex("Posticipa di 5 minuti")} onclick={step(5)}>{"+5"}</button>
                <button type="button" title={ex("Porta l’elemento all’inizio del suo turno.")} disabled={at_s == plan::turn_start(at_s, tm)} onclick={to_turn}>{ex("Inizio turno")}</button>
            </div>
        </div>
        <small class="hint">{crate::exfmt!("Turno {turn}, derivato dall’orario (turni da {tm} min, durata nella scheda Generale).", "Turn {turn}, derived from the time (turns of {tm} min, set in the General tab).")}</small>
        {if explicit { Html::default() } else {
            html!{<small class="hint tl-implied">{ex("Orario implicito: calcolato dal turno suggerito e quindi legato alla durata del turno. Diventa esplicito appena lo modifichi.")}
                {" "}<button type="button" class="tl-link-btn" onclick={{ let set = set.clone(); Callback::from(move |_: MouseEvent| set.emit(at_s)) }}>{ex("Rendi esplicito")}</button></small>}
        }}
    </div>}
}

/// The editor of the selected scripted action. The command is raw JSON of an
/// EXCON command; it is stored only while it parses as a JSON object.
fn action_detail(
    i: usize,
    a: &scenario::TimelineAction,
    msel: &[InjectTemplate],
    warn: Option<plan::Warn>,
    tm: u32,
    o: &Open,
    open: &UseStateHandle<Option<Open>>,
    act: &UseStateHandle<Option<usize>>,
    cmd_edit: &UseStateHandle<(usize, String)>,
    tool: &UseStateHandle<Option<&'static str>>,
) -> Html {
    let start_s = o.pack.edits.start_clock_s.unwrap_or(0);
    let set = |f: fn(&mut scenario::TimelineAction, String)| upd(open, move |p, v: String| if let Some(x) = p.edits.timeline.as_mut().and_then(|l| l.get_mut(i)) { f(x, v) });
    let shown = if cmd_edit.0 == i { cmd_edit.1.clone() } else { serde_json::to_string_pretty(&a.command).unwrap_or_default() };
    let valid = serde_json::from_str::<Value>(&shown).is_ok_and(|v| v.is_object());
    let on_cmd = {
        let (cmd_edit, store) = (cmd_edit.clone(), upd(open, move |p, v: Value| if let Some(x) = p.edits.timeline.as_mut().and_then(|l| l.get_mut(i)) { x.command = v }));
        Callback::from(move |e: InputEvent| {
            let t = api::input(e);
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                if v.is_object() { store.emit(v); }
            }
            cmd_edit.set((i, t));
        })
    };
    // The same tool forms as the EXCON Controllo; the command they build is stored here.
    const TOOL_IDS: [&str; 8] = ["ignite", "line", "drop", "extcircle", "extzone", "evac", "warn", "weather"];
    let pick = {
        let (store, cmd_edit) = (upd(open, move |p, v: Value| if let Some(x) = p.edits.timeline.as_mut().and_then(|l| l.get_mut(i)) { x.command = v }), cmd_edit.clone());
        Callback::from(move |v: Value| {
            store.emit(v);
            cmd_edit.set((usize::MAX, String::new()));
        })
    };
    let common = crate::controllo::CommonProps {
        stat: Value::Null, state: Value::Null, master: Value::Null, responses: Default::default(),
        busy: false, error: "".into(), done: 0, action: Callback::noop(),
        onclose: { let tool = tool.clone(); Callback::from(move |_| tool.set(None)) },
    };
    let tool_modal = match **tool {
        Some("weather") => {
            let w = o.pack.edits.weather;
            let cur = |k: &str, d: f64| if a.command["type"] == "SetWeather" { a.command[k].as_f64() } else { None }.or(Some(d)).unwrap_or(d);
            html! {<crate::controllo::WeatherModal title={crate::controllo::tool_label("weather")} common={common}
                dir={cur("wind_dir_deg", w.map_or(0., |w| w.wind_dir_deg))} speed={cur("wind_speed_kmh", w.map_or(0., |w| w.wind_speed_kmh))}
                moisture={cur("moisture_pct", w.map_or(10., |w| w.moisture_pct))} oncommand={Some(pick.clone())}/>}
        }
        Some(id) => {
            let (g, o2) = (geom(o), o.clone());
            let map_for = crate::controllo::MapFactory(std::rc::Rc::new(move |onpick: Callback<Value>, pending: Option<Value>| {
                let map = NodeRef::default();
                let o3 = o2.clone();
                let onclick = map_click(&map, g, Callback::from(move |(at, _): ([f32; 2], Option<usize>)| {
                    let zone = zone_at(&o3, g, at).unwrap_or_default();
                    onpick.emit(json!({"x": at[0], "y": at[1], "zone": zone}));
                }));
                let mark = pending.and_then(|p| Some((p["x"].as_f64()?, p["y"].as_f64()?)));
                html! {<svg ref={map} class="ignition-map" viewBox={format!("0 0 {} {}", g.width, g.height)} style={format!("aspect-ratio:{}/{}", g.width, g.height)} role="img" aria-label={ex("Mappa del territorio")} {onclick}>
                    {map_base(&o2, g)}
                    {if let Some((x, y)) = mark { html!{<circle cx={x.to_string()} cy={(g.height - y).to_string()} r={(g.zone * 0.06).to_string()} fill="#d9480f"/>} } else { Html::default() }}
                </svg>}
            }));
            html! {<crate::controllo::MapToolModal key={id.to_string()} tool={id.to_string()} common={common} oncommand={Some(pick.clone())} {map_for}/>}
        }
        None => Html::default(),
    };
    let links: Vec<(String, String)> = std::iter::once((String::new(), ex("(nessuno)").to_string()))
        .chain(msel.iter().map(|m| (m.id.clone(), format!("{} · {}", m.id, m.title))))
        .collect();
    html! {<div class="inject-body" key={format!("action-{i}")}>
        {time_block(plan::Key { kind: plan::Kind::Action, idx: i }, a.at_s, true, tm, start_s, open)}
        {match &warn {
            Some(plan::Warn::BeforeInject(id, t)) => html!{<p class="tl-warn">{crate::exfmt!("⚠ L’azione scatta alle {} ma segue l’inject {id}, previsto alle {}: l’inject ne è l’annuncio e dovrebbe venire prima.", "⚠ The action fires at {} but follows inject {id}, due at {}: the inject announces it and should come first.", plan::hhmm(start_s + a.at_s), plan::hhmm(start_s + t))}</p>},
            Some(plan::Warn::UnknownInject(id)) => html!{<p class="tl-warn">{crate::exfmt!("⚠ L’inject «{id}» collegato non esiste.", "⚠ The linked inject «{id}» does not exist.")}</p>},
            None => Html::default(),
        }}
        <div class="settings-fields">
            {field("Identificativo", "Chiave stabile dell’azione, usata dai report: non riutilizzarla.", a.id.clone(), set(|x, v| x.id = v))}
            {choice("Segue l’inject", "Collegamento informativo a un inject che annuncia l’azione: non cambia quando scatta, ma l’editor segnala se l’azione cade prima dell’inject.", a.after.as_deref().unwrap_or(""), &links, upd(open, move |p, v: String| if let Some(x) = p.edits.timeline.as_mut().and_then(|l| l.get_mut(i)) { x.after = Some(v).filter(|v| !v.is_empty()) }))}
        </div>
        {field("Titolo", "Nome breve mostrato nell’elenco e nella revisione prima di far avanzare il tempo.", a.title.clone(), set(|x, v| x.title = v))}
        <div class="inject-recipients" title={ex("Gli stessi strumenti del Controllo EXCON: costruiscono il comando dell’azione.")}>
            <span class="label">{ex("Imposta con lo strumento")}</span>
            <div class="chips">{for crate::controllo::TOOLS.iter().filter(|t| TOOL_IDS.contains(&t.id)).map(|t| {
                let (tool, id) = (tool.clone(), t.id);
                html!{<button type="button" class="chip" title={ex(t.tip)} onclick={Callback::from(move |_| tool.set(Some(id)))}>{format!("{} {}", t.icon, ex(t.label))}</button>}
            })}</div>
            <small class="hint">{format!("{}: {}", ex("Comando attuale"), a.command["type"].as_str().unwrap_or("?"))}</small>
        </div>
        {tool_modal}
        <label class="wide" title={ex("Comando EXCON in JSON, ad esempio SetWeather, Ignite, CloseRoad, SetBuildingStatus, AdjustResource.")}>{ex("Avanzate: comando (JSON)")}
            <textarea rows="7" value={shown} oninput={on_cmd}/>
            <small class="hint">{if valid { ex("JSON valido: i campi sono controllati all’esecuzione. Esempio: {\"type\":\"SetWeather\",\"wind_dir_deg\":270}") } else { ex("JSON non valido: l’ultima versione valida resta salvata.") }}</small>
        </label>
        {area("Nota", "Appunto per l’EXCON, mostrato quando l’azione è da decidere (revisione prima di far avanzare il tempo).", a.note.clone(), "2", set(|x, v| x.note = v))}
        <div class="inject-actions">
            <ConfirmDelete label="Elimina azione" onconfirm={{
                let act = act.clone();
                let del = upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.timeline.as_mut() { if i < l.len() { l.remove(i); } } });
                Callback::from(move |e| { act.set(None); del.emit(e) })
            }}/>
        </div>
    </div>}
}

fn add_inject_button(
    open: &UseStateHandle<Option<Open>>,
    selected: &UseStateHandle<usize>,
    act: &UseStateHandle<Option<usize>>,
    band_sel: &UseStateHandle<Option<usize>>,
    n: usize,
    at_s: i64,
    tm: u32,
) -> Html {
    let (selected, act, band_sel) = (selected.clone(), act.clone(), band_sel.clone());
    let add = upd(open, move |p, _: MouseEvent| {
        let l = p.edits.msel.get_or_insert_with(Vec::new);
        let mut k = l.len() + 1;
        while l.iter().any(|m| m.id == format!("I-{k:02}")) { k += 1; }
        l.push(InjectTemplate {
            id: format!("I-{k:02}"), turn: plan::turn_of(at_s, tm), title: String::new(), sender: String::new(),
            recipients: vec!["all".into()], text: String::new(), objective: String::new(), event: String::new(), at_s: Some(at_s),
        });
    });
    html! {<button class="primary" title={ex("Aggiunge un inject con orario esplicito: all’inizio del turno scelto (clic sull’intestazione) oppure in coda alla timeline. Destinatari «tutti».")} onclick={Callback::from(move |e| { band_sel.set(None); act.set(None); selected.set(n); add.emit(e) })}>{ex("Aggiungi inject")}</button>}
}

/// The editor of the selected inject.
fn inject_detail(i: usize, n: usize, m: &InjectTemplate, tm: u32, start_s: i64, roles: &[(String, String)], excon_roles: &[String], open: &UseStateHandle<Option<Open>>, selected: &UseStateHandle<usize>) -> Html {
    let set = |f: fn(&mut InjectTemplate, String)| upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { f(x, v) });
    let all = m.recipients.is_empty() || m.recipients.iter().any(|r| r == "all");
    let text_len = m.text.chars().count();
    let toggle_all = upd(open, move |p, _: MouseEvent| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.recipients = vec!["all".into()]; });
    let mv = |d: isize| {
        let j = i as isize + d;
        let selected = selected.clone();
        let cb = upd(open, move |p, _: MouseEvent| if let Some(l) = p.edits.msel.as_mut() {
            if j >= 0 && (j as usize) < l.len() { l.swap(i, j as usize); }
        });
        Callback::from(move |e| { if j >= 0 { selected.set(j as usize); } cb.emit(e) })
    };
    let dup = {
        let selected = selected.clone();
        let cb = upd(open, move |p, _: MouseEvent| if let Some(l) = p.edits.msel.as_mut() {
            if let Some(src) = l.get(i).cloned() {
                let mut c = src.clone();
                let mut k = 1;
                let id = |k: usize| if k == 1 { format!("{}-copia", src.id) } else { format!("{}-copia{k}", src.id) };
                while l.iter().any(|x| x.id == id(k)) { k += 1; }
                c.id = id(k);
                l.insert(i + 1, c);
            }
        });
        Callback::from(move |e| { selected.set(i + 1); cb.emit(e) })
    };
    html!{<div class="inject-body" key={i}>
        {time_block(plan::Key { kind: plan::Kind::Inject, idx: i }, plan::inject_s(m, tm), m.at_s.is_some(), tm, start_s, open)}
        <div class="settings-fields">
            {field("Identificativo", "Chiave stabile dell’inject, usata da pianificazioni e report: non riutilizzarla.", m.id.clone(), set(|x, v| x.id = v))}
            {if excon_roles.is_empty() {
                field("Mittente", "Chi appare come mittente per i giocatori. Definisci i ruoli dell’EXCON nella scheda Ruoli per sceglierlo da un elenco.", m.sender.clone(), set(|x, v| x.sender = v))
            } else {
                let mut opts: Vec<(String, String)> = vec![(String::new(), "—".into())];
                opts.extend(excon_roles.iter().map(|r| (r.clone(), r.clone())));
                if !m.sender.is_empty() && !excon_roles.contains(&m.sender) { opts.push((m.sender.clone(), m.sender.clone())); }
                choice("Mittente", "Ruolo interpretato dall’EXCON, definito nella scheda Ruoli.", &m.sender, &opts, set(|x, v| x.sender = v))
            }}
            {field("Evento (raggruppa nella timeline)", "Gli inject con lo stesso evento sono raggruppati nella timeline dell’EXCON.", m.event.clone(), set(|x, v| x.event = v))}
        </div>
        <div class="inject-recipients" title={ex("Chi riceve l’inject: tutti i ruoli oppure solo quelli scelti.")}>
            <span class="label">{ex("Destinatari")}</span>
            <div class="chips">
                <button type="button" class={classes!("chip", all.then_some("on"))} aria-pressed={all.to_string()} onclick={toggle_all}>{ex("tutti")}</button>
                {for roles.iter().map(|(id, name)| {
                    let on = !all && m.recipients.contains(id);
                    let id2 = id.clone();
                    html!{<button type="button" class={classes!("chip", on.then_some("on"))} aria-pressed={on.to_string()} onclick={upd(open, move |p, _: MouseEvent| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) {
                        x.recipients.retain(|r| r != "all");
                        if let Some(k) = x.recipients.iter().position(|r| *r == id2) { x.recipients.remove(k); } else { x.recipients.push(id2.clone()); }
                        if x.recipients.is_empty() { x.recipients = vec!["all".into()]; }
                    })}>{name.clone()}</button>}
                })}
            </div>
            <small class="hint">{ex("«tutti» invia a ogni ruolo. Scegliendo un ruolo specifico «tutti» si disattiva; senza ruoli scelti torna «tutti».")}</small>
        </div>
        {field("Titolo", "Oggetto breve mostrato nell’elenco e nella casella dei giocatori.", m.title.clone(), set(|x, v| x.title = v))}
        {area("Testo", "Scrivilo in inglese, la lingua di lavoro: i giocatori lo ricevono tradotto. Meglio 2–4 frasi con fatti concreti (luogo, ora, numeri).", m.text.clone(), "4", set(|x, v| x.text = v))}
        <small class="hint char-count">{format!("{text_len} {}", ex("caratteri"))}{if text_len > 600 { format!(" · {}", ex("testo lungo: valuta di accorciarlo")) } else { String::new() }}</small>
        {field("Obiettivo formativo", "Che cosa il formatore vuole mettere alla prova con questo inject; guida il debriefing.", m.objective.clone(), set(|x, v| x.objective = v))}
        <div class="inject-actions">
            <button title={ex("Sposta l’inject prima nell’elenco: conta solo a parità di orario.")} disabled={i == 0} onclick={mv(-1)}>{"▲"}</button>
            <button title={ex("Sposta l’inject dopo nell’elenco: conta solo a parità di orario.")} disabled={i + 1 >= n} onclick={mv(1)}>{"▼"}</button>
            <button title={ex("Crea una copia di questo inject subito dopo, con un nuovo identificativo.")} onclick={dup}>{ex("Duplica")}</button>
            <details class="confirm-inline">
                <summary class="danger" title={ex("Elimina l’inject: chiede conferma.")}>{ex("Elimina inject")}</summary>
                <span>{ex("Eliminare davvero? Puoi ancora uscire senza salvare.")}</span>
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.msel.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Conferma eliminazione")}</button>
            </details>
        </div>
    </div>}
}

fn advanced_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let ops = e.operations.clone().unwrap_or_default();
    let policy = e.tactical_policy.clone().unwrap_or_default();
    type Flag = (&'static str, &'static str, fn(&scenario::Operations) -> bool, fn(&mut scenario::Operations, bool));
    let flags: [Flag; 10] = [
        ("Percorsi stradali", "I mezzi seguono la rete stradale reale. Senza un percorso possibile restano in attesa o falliscono: non c’è mai una linea retta di ripiego.", |o| o.road_routing, |o, v| o.road_routing = v),
        ("Soppressione progressiva", "Il lavoro di squadre e mezzi si accumula nel tempo (linee che si completano a poco a poco) invece di avere effetto istantaneo.", |o| o.progressive_suppression, |o, v| o.progressive_suppression = v),
        ("Traffico aggregato", "Simula il traffico dei veicoli civili come flusso complessivo: può rallentare le strade e gli spostamenti, senza modellare ogni singola auto.", |o| o.traffic, |o, v| o.traffic = v),
        ("Esposizione all’intensità", "Squadre e veicoli vicini al fronte accumulano esposizione in base all’intensità del fuoco; oltre certe soglie devono ritirarsi.", |o| o.intensity_exposure, |o, v| o.intensity_exposure = v),
        ("Comunicazioni", "Gli avvisi e gli ordini dipendono dalla copertura radio/telefonica: dove i siti di comunicazione cadono o mancano, i messaggi arrivano in ritardo o non arrivano.", |o| o.communications, |o, v| o.communications = v),
        ("Spotting (brace a distanza)", "Il vento trasporta braci che accendono nuovi focolai davanti al fronte. Rende il fuoco più imprevedibile: da attivare solo se lo scenario lo richiede.", |o| o.spotting, |o, v| o.spotting = v),
        ("Chiusure di tratto per fuoco", "Un tratto di strada raggiunto o minacciato dal fuoco viene chiuso e i percorsi vengono ricalcolati.", |o| o.segment_fire_closures, |o, v| o.segment_fire_closures = v),
        ("Spostamenti fuori strada", "Alcuni mezzi e squadre possono attraversare il terreno aperto, più lentamente, quando la strada non basta.", |o| o.terrain_travel, |o, v| o.terrain_travel = v),
        ("Coda dei mezzi di emergenza", "I mezzi di soccorso che usano lo stesso tratto si mettono in coda e si rallentano a vicenda.", |o| o.emergency_congestion, |o, v| o.emergency_congestion = v),
        ("Autonomia locale delle squadre", "Le squadre sul posto possono adattare il proprio lavoro (spostarsi, ritirarsi, riprendere) entro la missione approvata, senza chiedere ogni volta all’EXCON.", |o| o.local_autonomy, |o, v| o.local_autonomy = v),
    ];
    let flag = |i: usize| {
        let (label, tip, get, set) = flags[i];
        gen_flag(label, tip, get(&ops), upd(open, move |p, on: bool| set(p.edits.operations.get_or_insert_with(Default::default), on)))
    };
    let run = &o.pack.run;
    let pol = |f: fn(&mut scenario::TacticalPolicy, f64)| {
        upd(open, move |p, v: String| if let Some(n) = num(&v) { f(p.edits.tactical_policy.get_or_insert_with(Default::default), n) })
    };
    html! {<>
        {gen_section("Modello operativo", "Quanto realistica è la logistica: strade, tempi, comunicazioni. Ogni opzione è indipendente; tutte spente = regole semplificate originali.", html! {<>
            <div class="gen-group wide">{gen_flag("Avvia con il modello operativo attivo", "Parte con le opzioni sotto attive. Se spento, ci si affida alle regole semplificate o all’opzione --operational-model della riga di comando.", run.operational_model == Some(true), upd(open, |p, on: bool| p.run.operational_model = on.then_some(true)))}</div>
            <div class="gen-group wide"><h5>{ex("Strade e spostamenti")}</h5>{for [0, 6, 7, 8, 2].map(flag)}</div>
            <div class="gen-group wide"><h5>{ex("Fuoco e rischio")}</h5>{for [1, 3, 5].map(flag)}</div>
            <div class="gen-group wide"><h5>{ex("Comando e comunicazioni")}</h5>{for [4, 9].map(flag)}</div>
        </>})}
        {gen_section("Politica tattica", "Regole con cui le squadre agiscono sul posto. Distanze in metri, tempi in secondi simulati. Sono ipotesi di addestramento, non indicazioni di sicurezza operativa.", html! {<>
            {gen_number("Raggio del settore (m)", "Distanza entro cui una squadra considera suo il settore attorno al punto di missione. Tipico: 200–500 m.", "0", None, "any", "", policy.sector_radius_m.to_string(), pol(|p, n| p.sector_radius_m = n as f32))}
            {gen_number("Portata della manichetta (m)", "Distanza dal mezzo entro cui si può bagnare. Tipico: 40–100 m.", "0", None, "any", "", policy.hose_reach_m.to_string(), pol(|p, n| p.hose_reach_m = n as f32))}
            {gen_number("Limite di bagnatura (punti)", "Quantità massima di bagnatura che una squadra applica a un punto prima di passare oltre: valore più alto = più acqua per punto.", "0", None, "any", "", policy.wet_limit_pts.to_string(), pol(|p, n| p.wet_limit_pts = n as f32))}
            {gen_number("Rivalutazione (s)", "Ogni quanti secondi simulati le squadre rivalutano la situazione. 60 = una volta al minuto.", "1", None, "1", "", policy.reassess_s.to_string(), pol(|p, n| p.reassess_s = n as i64))}
            {gen_number("Riorganizzazione (s)", "Secondi simulati persi per riorganizzarsi dopo un ritiro o un cambio di compito.", "0", None, "1", "", policy.regroup_s.to_string(), pol(|p, n| p.regroup_s = n as i64))}
            {gen_number("Pericolo per sosta (0–1)", "Livello di pericolo (0 nessuno, 1 massimo) oltre il quale un mezzo non sosta e si sposta. Valori bassi = più prudenti.", "0", Some("1"), "0.05", "", policy.parking_danger.to_string(), pol(|p, n| p.parking_danger = n as f32))}
            {gen_number("Pericolo per lavoro (0–1)", "Livello di pericolo (0 nessuno, 1 massimo) oltre il quale una squadra smette di lavorare e si ritira. Valori bassi = più prudenti.", "0", Some("1"), "0.05", "", policy.work_danger.to_string(), pol(|p, n| p.work_danger = n as f32))}
            <div class="gen-group wide">
            {gen_flag("Lavoro condiviso tra squadre", "Più squadre vicine si dividono lo stesso lavoro invece di ripeterlo.", policy.share_work, upd(open, |p, on: bool| p.edits.tactical_policy.get_or_insert_with(Default::default).share_work = on))}
            {gen_flag("Riprende dopo il ritiro", "Dopo un ritiro per pericolo, la squadra riprende da sola il compito quando le condizioni lo permettono.", policy.resume_after_withdrawal, upd(open, |p, on: bool| p.edits.tactical_policy.get_or_insert_with(Default::default).resume_after_withdrawal = on))}
            </div>
        </>})}
        {gen_section("Simulazione", "Opzioni di calcolo. Lasciare vuoto = valore predefinito. Le opzioni sulla riga di comando hanno la precedenza su queste.", html! {<>
            {gen_number("Seme casuale", "Numero intero che rende ripetibile il caso: stesso seme, stessa evoluzione del fuoco. Vuoto = scelto dal programma.", "0", None, "1", "predefinito", run.seed.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.seed = num(&v)))}
            {gen_number("Realizzazioni dell’ensemble", "Quante simulazioni del fuoco vengono eseguite in parallelo per stimarne l’incertezza. Predefinito 32. Più realizzazioni = stima più stabile ma più lenta.", "1", None, "1", "32", run.realizations.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.realizations = num(&v)))}
            {gen_number("Soglia del fuoco «reale» (0–1)", "Frazione di realizzazioni che devono raggiungere una cella perché conti come in fiamme. 0.5 = scenario centrale (predefinito), 0.25 = scenario pessimistico.", "0", Some("1"), "0.05", "0.5", run.cutoff.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.cutoff = num(&v)))}
        </>})}
    </>}
}

// ------------------------------------------------------------ the view

const TABS: &[(&str, &str, &str)] = &[
    ("general", "Generale", "Nome, descrizione, ora di inizio, durata dei turni e meteo iniziale."),
    ("roles", "Ruoli", "Chi partecipa (squadre, enti, funzioni) e quali azioni ciascuno può proporre."),
    ("resources", "Risorse", "Tipi di mezzi e squadre disponibili, con quantità e base di partenza."),
    ("ignitions", "Inneschi", "Dove e quando parte l’incendio, scelti sulla mappa."),
    ("reinforcements", "Rinforzi", "Risorse extra che i giocatori possono richiedere durante l’esercitazione."),
    ("places", "Luoghi", "Nomi dei luoghi che giocatori e AI possono citare nei messaggi."),
    ("injects", "Timeline", "Inject (messaggi) e azioni pianificate (cambiamenti del mondo) che l’EXCON consegna o decide durante l’esercitazione."),
    ("advanced", "Avanzate", "Impostazioni tecniche: modello operativo, seme casuale, realizzazioni dell’ensemble."),
];

#[function_component(ScenarioEditor)]
pub fn scenario_editor(props: &Props) -> Html {
    let library = use_state(|| None::<Value>);
    let open = use_state(|| None::<Open>);
    let tab = use_state(|| "general".to_string());
    let selected = use_state(|| 0usize);
    let message = use_state(String::new);
    let problems = use_state(Vec::<String>::new);
    let busy = use_state(|| false);
    let confirm = use_state(String::new);
    let json = use_state(String::new);
    let pack_json = use_state(String::new);
    let pack_json_open = use_state(|| false);
    let map = use_node_ref();

    let refresh = {
        let library = library.clone();
        let message = message.clone();
        use_callback((), move |(), _| {
            let (library, message) = (library.clone(), message.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match call("/api/master/scenarios", None).await {
                    Ok(v) => library.set(Some(v)),
                    Err(e) => message.set(e),
                }
            });
        })
    };
    {
        let refresh = refresh.clone();
        use_effect_with(props.revision, move |_| {
            refresh.emit(());
            || ()
        });
    }

    // Opens a scenario for editing. A bare territory or an app-supplied
    // scenario cannot be overwritten, so it opens as a new copy.
    let load = {
        let (open, message, problems, tab) = (open.clone(), message.clone(), problems.clone(), tab.clone());
        Callback::from(move |(id, copy): (String, bool)| {
            let (open, message, problems, tab) = (open.clone(), message.clone(), problems.clone(), tab.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match call(&format!("/api/master/scenarios/{}", api::encode(&id)), None).await {
                    Ok(v) => match serde_json::from_value::<ScenarioPack>(v["pack"].clone()) {
                        Ok(mut pack) => {
                            let as_copy = copy || v["builtin"] == true;
                            let saved_id = (!as_copy).then(|| pack.id.clone());
                            if as_copy {
                                pack.id = format!("{}-copia", id.trim_end_matches("-copia"));
                                pack.name = format!("{} (copia)", pack.name);
                            }
                            problems.set(vec![]);
                            message.set(String::new());
                            tab.set("general".into());
                            open.set(Some(Open { pack, reference: v["reference"].clone(), saved_id, dirty: as_copy }));
                        }
                        Err(e) => message.set(format!("Scenario non leggibile: {e}")),
                    },
                    Err(e) => message.set(e),
                }
            });
        })
    };

    // Save (and optionally start). `Ok` shows the outcome, errors list problems.
    let save = {
        let (open, message, problems, busy, refresh) =
            (open.clone(), message.clone(), problems.clone(), busy.clone(), refresh.clone());
        Callback::from(move |start: bool| {
            let Some(cur) = (*open).clone() else { return };
            let (open, message, problems, busy, refresh) =
                (open.clone(), message.clone(), problems.clone(), busy.clone(), refresh.clone());
            busy.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let id = cur.pack.id.clone();
                let saved = call("/api/master/scenarios/save", Some(json!({"pack": cur.pack}))).await;
                match saved {
                    Ok(_) => {
                        problems.set(vec![]);
                        let mut next = cur;
                        next.saved_id = Some(id.clone());
                        next.dirty = false;
                        open.set(Some(next));
                        message.set(ex("Scenario salvato").to_string());
                        if start {
                            message.set(start_scenario(&id).await.unwrap_or_else(|e| e));
                        }
                    }
                    Err(e) => problems.set(e.lines().map(str::to_string).collect()),
                }
                busy.set(false);
                refresh.emit(());
            });
        })
    };

    let validate = {
        let (open, problems, message) = (open.clone(), problems.clone(), message.clone());
        Callback::from(move |_: MouseEvent| {
            let Some(cur) = (*open).clone() else { return };
            let (problems, message) = (problems.clone(), message.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match call("/api/master/scenarios/validate", Some(json!({"pack": cur.pack}))).await {
                    Ok(v) => {
                        let found: Vec<String> = list(&v["problems"]).iter().map(|p| text(p).to_string()).collect();
                        message.set(if found.is_empty() { ex("Nessun problema trovato").to_string() } else { String::new() });
                        problems.set(found);
                    }
                    Err(e) => message.set(e),
                }
            });
        })
    };

    let act = {
        let (message, refresh, busy) = (message.clone(), refresh.clone(), busy.clone());
        Callback::from(move |(path, id): (&'static str, String)| {
            let (message, refresh, busy) = (message.clone(), refresh.clone(), busy.clone());
            busy.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = if path == "start" {
                    start_scenario(&id).await
                } else {
                    call(path, Some(json!({"id": id}))).await.map(|_| ex("Eliminato").to_string())
                };
                message.set(result.unwrap_or_else(|e| e));
                busy.set(false);
                refresh.emit(());
            });
        })
    };

    let Some(lib) = &*library else {
        return html! {<main class="page"><p>{ex("Caricamento…")}</p>if !message.is_empty(){<p class="error" role="alert">{(*message).clone()}</p>}</main>};
    };
    let running = text(&lib["running"]).to_string();
    let active = text(&lib["active"]).to_string();
    let packs = list(&lib["packs"]).to_vec();

    // A destructive or session-dropping button asks twice. The second step
    // says what will happen and can be cancelled.
    let twice = |key: String, label: String, class: &'static str, tip: &'static str, go: Callback<()>| {
        if *confirm == key {
            html! {<span class="confirm-delete" role="group" aria-label={ex("Conferma richiesta")}>
                <button class="danger" autofocus=true disabled={*busy} title={ex(tip)} onclick={Callback::from(move |_| go.emit(()))}>{format!("{}: {label}", ex("Conferma"))}</button>
                <button title={ex("Non fare nulla e tornare indietro.")} onclick={{let confirm = confirm.clone(); Callback::from(move |_| confirm.set(String::new()))}}>{ex("Annulla")}</button>
                <small class="hint warn">{ex(tip)}</small>
            </span>}
        } else {
            let confirm = confirm.clone();
            html! {<button class={class} disabled={*busy} title={ex(tip)} onclick={Callback::from(move |_| confirm.set(key.clone()))}>{label}</button>}
        }
    };

    let busy_note = if *busy { html! {<p class="muted" role="status" aria-live="polite">{ex("Operazione in corso…")}</p>} } else { Html::default() };

    // The list and the editor are two pages of one view: opening a scenario
    // replaces the list, and "← Scenari" goes back.
    html! {<main class="page scenario-editor" id="scenario-editor">
        if open.is_none() {<section class="card">
            <h2>{ex("Scenari")}</h2>
            <p class="muted">{ex("Uno scenario è un territorio (mappa, edifici, strade) più ciò che lo rende un’esercitazione: ruoli, risorse, inneschi, meteo e inject. «Avvia» inizia una nuova esercitazione e scarta la sessione in corso.")}</p>
            <ul class="scenario-legend muted">
                <li><span class="pill">{ex("in corso")}</span> {ex("è l’esercitazione che sta girando ora.")}</li>
                <li><span class="pill">{ex("prossimo avvio")}</span> {ex("verrà usato al prossimo riavvio dell’applicazione.")}</li>
                <li><span class="pill">{ex("incluso")}</span> {ex("fa parte dell’applicazione e non si sovrascrive: usa Personalizza per ottenerne una copia modificabile.")}</li>
            </ul>
            <table class="scenario-list"><thead><tr>
                <th title={ex("Nome e descrizione dello scenario.")}>{ex("Nome")}</th>
                <th title={ex("La mappa su cui si svolge: terreno, edifici e strade. Non si modifica da qui.")}>{ex("Territorio")}</th>
                <th><span class="sr-only">{ex("Azioni")}</span></th></tr></thead><tbody>
            {for packs.iter().map(|p| {
                let id = text(&p["id"]).to_string();
                let builtin = p["builtin"] == true;
                let is_running = id == running;
                let description = text(&p["description"]).to_string();
                html!{<tr key={id.clone()} class={classes!(is_running.then_some("running"))} data-scenario={id.clone()}>
                    <td class="scenario-name"><strong>{text(&p["name"])}</strong>
                        if is_running {<span class="pill" title={ex("È l’esercitazione in corso in questo momento.")}>{ex("in corso")}</span>}
                        else if id == active {<span class="pill" title={ex("Sarà caricato al prossimo riavvio dell’applicazione.")}>{ex("prossimo avvio")}</span>}
                        if builtin {<span class="pill" title={ex("Scenario incluso nell’applicazione: non si può sovrascrivere. Usa Personalizza per salvarne una copia tua.")}>{ex("incluso")}</span>}
                        if !description.is_empty() {<br/><small class="muted">{description}</small>}</td>
                    <td class="scenario-base">{text(&p["base"])}</td>
                    <td class="row">
                        <button class="edit-scenario" disabled={*busy}
                            title={if builtin {ex("Apre una copia modificabile: lo scenario incluso resta com’è.")} else {ex("Apre lo scenario nell’editor. Salvare non tocca l’esercitazione in corso.")}}
                            onclick={{let load = load.clone(); let id = id.clone(); Callback::from(move |_| load.emit((id.clone(), false)))}}>{if builtin {ex("Personalizza")} else {ex("Modifica")}}</button>
                        if !builtin {<button disabled={*busy} title={ex("Apre una copia di questo scenario con un nuovo identificativo.")} onclick={{let load = load.clone(); let id = id.clone(); Callback::from(move |_| load.emit((id.clone(), true)))}}>{ex("Duplica")}</button>}
                        {twice(format!("start:{id}"), ex("Avvia").to_string(), "primary", "Avvia una nuova esercitazione da questo scenario. La sessione in corso (turni, messaggi, decisioni) viene scartata.", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("start", id.clone())))})}
                        if !builtin && !is_running {{twice(format!("delete:{id}"), ex("Elimina").to_string(), "danger", "Elimina definitivamente questo scenario dalla libreria. Non si può annullare.", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("/api/master/scenarios/delete", id.clone())))})}}
                    </td>
                </tr>}
            })}
            </tbody></table>
            {busy_note}
            if !message.is_empty() {<p role="status" id="scenario-message">{(*message).clone()}</p>}
        </section>}
        if let Some(o) = &*open {
            <section class="card" id="scenario-form">
                <div class="row scenario-head">
                    <h2>{o.pack.name.clone()}</h2>
                    <span class="pill" title={if o.saved_id.is_some() {ex("Identificativo con cui lo scenario è salvato nella libreria.")} else {ex("Questo scenario non è ancora nella libreria: salvalo per conservarlo.")}}>{if o.saved_id.is_some() {o.pack.id.clone()} else {ex("non salvato").to_string()}}</span>
                    if o.dirty {<span class="pill pending" title={ex("Hai cambiato qualcosa che non è ancora stato salvato.")}>{ex("modifiche non salvate")}</span>}
                    <div class="grow"/>
                    if o.dirty {
                        {twice("close".into(), ex("← Scenari").to_string(), "", "Chiude l’editor e perde le modifiche non salvate.", {let open = open.clone(); let confirm = confirm.clone(); Callback::from(move |_| { confirm.set(String::new()); open.set(None) })})}
                    } else {
                        <button title={ex("Chiude l’editor e torna all’elenco.")} onclick={{let open = open.clone(); Callback::from(move |_| open.set(None))}}>{ex("← Scenari")}</button>
                    }
                </div>
                if o.saved_id.is_none() {
                    <div class="settings-fields">
                        {field("Identificativo (a-z, 0-9, _ -)", "Nome breve che identifica lo scenario nella libreria: solo lettere minuscole, cifre, «_» e «-». Dopo il primo salvataggio non cambia più.", o.pack.id.clone(), upd(&open, |p, v: String| p.id = v))}
                    </div>
                }
                <nav class="scenario-tabs" role="tablist" aria-label={ex("Sezioni dello scenario")}>
                    {for TABS.iter().map(|(id, label, tip)| {
                        let tab = tab.clone();
                        let id = id.to_string();
                        let on = *tab == id;
                        html!{<button role="tab" aria-selected={on.to_string()} title={ex(tip)} class={classes!(on.then_some("active"))} data-tab={id.clone()} onclick={Callback::from(move |_| tab.set(id.clone()))}>{ex(label)}</button>}
                    })}
                </nav>
                <div class="scenario-tab" role="tabpanel">
                    {match tab.as_str() {
                        "roles" => roles_tab(o, &open),
                        "resources" => resources_tab(o, &open),
                        "ignitions" => ignitions_tab(o, &open, &selected, &map),
                        "reinforcements" => reinforcements_tab(o, &open),
                        "places" => places_tab(o, &open),
                        "injects" => injects_tab(o, &open, &json, &message),
                        "advanced" => advanced_tab(o, &open),
                        _ => general(o, &open),
                    }}
                </div>
                if !problems.is_empty() {
                    <div class="error scenario-problems" role="alert"><strong>{format!("{} ({})", ex("Da correggere prima di salvare:"), problems.len())}</strong>
                        <ul>{for problems.iter().map(|p| html!{<li>{p.clone()}</li>})}</ul></div>
                }
                if !message.is_empty() {<p role="status" id="scenario-message">{(*message).clone()}</p>}
                <div class="scenario-actions">
                    <div class="row">
                        <button id="save-scenario" class="primary" disabled={*busy}
                            title={ex("Salva lo scenario nella libreria. Non tocca l’esercitazione in corso.")}
                            onclick={{let save = save.clone(); Callback::from(move |_| save.emit(false))}}>{if *busy {ex("Attendere…")} else {ex("Salva")}}</button>
                        {twice("save-start".into(), ex("Salva e avvia").to_string(), "", "Salva lo scenario e avvia subito una nuova esercitazione. La sessione in corso viene scartata.", {let save = save.clone(); Callback::from(move |_| save.emit(true))})}
                        <button disabled={*busy} title={ex("Verifica lo scenario senza salvare e segnala gli errori da correggere.")} onclick={validate}>{ex("Controlla")}</button>
                    </div>
                    <div class="row scenario-transfer">
                        <button title={ex("Scrive lo scenario intero come testo JSON nel riquadro «Avanzate: JSON» qui sotto, per copiarlo o archiviarlo.")} onclick={{
                            let (pack_json, pack_json_open, pack) = (pack_json.clone(), pack_json_open.clone(), o.pack.clone());
                            Callback::from(move |_| { pack_json.set(serde_json::to_string_pretty(&pack).unwrap_or_default()); pack_json_open.set(true) })
                        }}>{ex("Esporta scenario (JSON)")}</button>
                        <button title={ex("Sostituisce il contenuto dell’editor con il testo JSON incollato; salva poi per conservarlo. L’identificativo dello scenario resta quello attuale.")} onclick={{
                            let (pack_json, pack_json_open, open, message) = (pack_json.clone(), pack_json_open.clone(), open.clone(), message.clone());
                            Callback::from(move |_| { if pack_json.trim().is_empty() { pack_json_open.set(true); message.set(ex("Incolla prima il testo JSON dello scenario (riquadro «Avanzate: JSON» qui sotto), poi premi Importa.").to_string()); return; } match serde_json::from_str::<ScenarioPack>(&pack_json) {
                                Ok(mut pack) => {
                                    if let Some(cur) = &*open {
                                        // An imported pack never renames the scenario it is imported into.
                                        pack.id = cur.pack.id.clone();
                                        open.set(Some(Open { pack, dirty: true, ..cur.clone() }));
                                        message.set(ex("Scenario importato nell’editor · salva per conservarlo").to_string());
                                    }
                                }
                                Err(e) => message.set(format!("JSON non valido: {e}")),
                            } })
                        }}>{ex("Importa scenario (JSON)")}</button>
                    </div>
                    <details class="scenario-json" open={*pack_json_open} ontoggle={{
                        let pack_json_open = pack_json_open.clone();
                        Callback::from(move |e: Event| {
                            use yew::TargetCast;
                            pack_json_open.set(e.target_unchecked_into::<web_sys::Element>().has_attribute("open"));
                        })
                    }}>
                        <summary title={ex("Testo JSON dell’intero scenario, per Esporta e Importa scenario.")}>{ex("Avanzate: JSON")}</summary>
                        <p class="hint">{ex("Esporta scenario scrive qui il JSON dell’intero scenario; per importarne uno, incollalo qui e premi Importa scenario. Nulla è salvato finché non premi Salva.")}</p>
                        <textarea aria-label={ex("JSON dello scenario")} rows="8" value={(*pack_json).clone()} oninput={{
                            let pack_json = pack_json.clone();
                            Callback::from(move |e: InputEvent| pack_json.set(api::input(e)))
                        }}/>
                    </details>
                </div>
            </section>
        }
    </main>}
}

/// Begin a new exercise from `id`. In the browser the page reloads into it.
async fn start_scenario(id: &str) -> Result<String, String> {
    #[cfg(feature = "standalone")]
    {
        crate::store::activate(id)?;
        return Ok(ex("Nuova esercitazione…").to_string());
    }
    #[cfg(not(feature = "standalone"))]
    {
        call("/api/master/scenarios/activate", Some(json!({"id": id}))).await?;
        Ok(ex("Nuova esercitazione avviata").to_string())
    }
}
