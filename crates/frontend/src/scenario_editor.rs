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
    InjectTemplate, ReinforcementOffer, ResourceType, Role, ScenarioPack,
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

pub(crate) async fn call(path: &str, body: Option<Value>) -> Result<Value, String> {
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

/// Settings content supplied by the host (the standalone app: AI key and stored
/// session). Shown above the scenario list; never equal, so it always refreshes.
#[derive(Clone)]
pub struct SettingsSlot(pub Html);
impl PartialEq for SettingsSlot {
    fn eq(&self, _: &Self) -> bool {
        false
    }
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

/// Selection in the combined "Ruoli e risorse" list: (kind, index) with
/// kind `K_ROLE`, `K_RESOURCE` or `K_REINF`.
type Sel = UseStateHandle<(u8, usize)>;
const K_ROLE: u8 = 0;
const K_RESOURCE: u8 = 1;
const K_REINF: u8 = 2;

/// List entries, "add" button and editor of the roles.
fn roles_parts(o: &Open, open: &UseStateHandle<Option<Open>>, selected: &Sel) -> (Html, Html, Html) {
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
    let sel = if selected.0 == K_ROLE { selected.1.min(n.saturating_sub(1)) } else { usize::MAX };
    let items = html! {<>
        <h5 class="md-group">{ex("Ruoli")}</h5>
        {for roles.iter().enumerate().map(|(i, r)| {
            let stock: Vec<String> = resources.iter().filter_map(|res| r.resources.get(&res.id).map(|q| format!("{} ×{q}", res.name))).collect();
            let level = levels.iter().find(|(v, _)| *v == r.level).map(|(_, n)| n.clone()).unwrap_or_else(|| r.level.clone());
            let bad = r.id.is_empty() || roles.iter().filter(|x| x.id == r.id).count() > 1;
            md_item(i == sel, { let selected = selected.clone(); Callback::from(move |_| selected.set((K_ROLE, i))) }, html!{<>
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
            Callback::from(move |e| { selected.set((K_ROLE, n)); add.emit(e) })
        }}>{ex("+ Ruolo")}</button>
    };
    let detail = match roles.get(sel) {
        Some(r) if selected.0 == K_ROLE => {
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
                    { if resources.is_empty() { html!{<small class="muted">{ex("Nessun tipo di risorsa: aggiungilo tra le risorse.")}</small>} } else { Html::default() } }
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
        _ => Html::default(),
    };
    (items, tools, detail)
}

/// The parts the EXCON plays: one row per name, reorderable.
fn excon_roles_list(names: Vec<String>, default_persona: String, open: &UseStateHandle<Option<Open>>) -> Html {
    let n = names.len();
    fn edit_fn(open: &UseStateHandle<Option<Open>>, f: impl Fn(&mut Vec<String>) + 'static) -> Callback<MouseEvent> {
        upd(open, move |p, _: MouseEvent| f(p.edits.excon_roles.get_or_insert_with(Vec::new)))
    }
    let edit = |f: Box<dyn Fn(&mut Vec<String>)>| edit_fn(open, f);
    html! {<section class="excon-roles" aria-label={ex("Ruoli interpretati dall’EXCON")}>
        <h4>{ex("Ruoli interpretati dall’EXCON")}</h4>
        <p class="muted">{ex("Le parti che l’EXCON recita negli inject (ad esempio «Centrale 112», «Sindaco»). Non sono ruoli player: compaiono come mittenti quando si costruisce un inject.")}</p>
        <label class="wide default-persona" title={ex("Il nome con cui l’EXCON si rivolge ai player quando non sceglie un altro personaggio e l’inject non ha un mittente.")}>{ex("Personaggio predefinito dell’EXCON")}
            <input maxlength="80" placeholder={ex("Sala operativa")} value={default_persona} oninput={upd(open, move |p, e: InputEvent| p.edits.default_persona = Some(api::input(e)))}/></label>
        <ul class="excon-role-list">
        {for names.iter().enumerate().map(|(i, name)| {
            let dup = !name.trim().is_empty() && names.iter().filter(|x| x.trim() == name.trim()).count() > 1;
            html!{<li key={i} class="excon-role">
                <input value={name.clone()} placeholder={ex("Nome del ruolo")} aria-label={ex("Nome del ruolo EXCON")}
                    oninput={upd(open, move |p, e: InputEvent| if let Some(x) = p.edits.excon_roles.as_mut().and_then(|l| l.get_mut(i)) { *x = api::input(e) })}/>
                {if dup { html!{<span class="pill pending" title={ex("Nome duplicato.")}>{"!"}</span>} } else { Html::default() }}
                <button type="button" title={ex("Sposta su")} disabled={i == 0} onclick={edit(Box::new(move |l: &mut Vec<String>| if i > 0 && i < l.len() { l.swap(i, i - 1) }))}>{"↑"}</button>
                <button type="button" title={ex("Sposta giù")} disabled={i + 1 >= n} onclick={edit(Box::new(move |l: &mut Vec<String>| if i + 1 < l.len() { l.swap(i, i + 1) }))}>{"↓"}</button>
                <button type="button" title={ex("Elimina")} onclick={edit(Box::new(move |l: &mut Vec<String>| if i < l.len() { l.remove(i); }))}>{"✕"}</button>
            </li>}
        })}
        </ul>
        {if n == 0 { html!{<p class="muted">{ex("Nessun ruolo EXCON: il mittente degli inject resta libero.")}</p>} } else { Html::default() }}
        <button type="button" class="primary" onclick={edit(Box::new(|l: &mut Vec<String>| l.push(String::new())))}>{ex("Aggiungi ruolo EXCON")}</button>
    </section>}
}

/// List entries, "add" button and editor of the resource types.
fn resources_parts(o: &Open, open: &UseStateHandle<Option<Open>>, selected: &Sel) -> (Html, Html, Html) {
    let resources = o.pack.edits.resource_types.clone().unwrap_or_default();
    let roles = o.pack.edits.roles.clone().unwrap_or_default();
    let n = resources.len();
    let sel = if selected.0 == K_RESOURCE { selected.1.min(n.saturating_sub(1)) } else { usize::MAX };
    let users_of = |id: &str| -> Vec<String> { roles.iter().filter(|x| x.resources.contains_key(id)).map(|x| x.id.clone()).collect() };
    let items = html! {<>
        <h5 class="md-group">{ex("Tipi di risorsa")}</h5>
        {for resources.iter().enumerate().map(|(i, r)| {
            let bad = r.id.is_empty() || resources.iter().filter(|x| x.id == r.id).count() > 1;
            md_item(i == sel, { let selected = selected.clone(); Callback::from(move |_| selected.set((K_RESOURCE, i))) }, html!{<>
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
                l.push(ResourceType { id: format!("resource_{n}"), name: format!("Risorsa {n}"), description: String::new(), speed_mps: None, drone: None });
            }));
            Callback::from(move |e| { selected.set((K_RESOURCE, n)); add.emit(e) })
        }}>{ex("+ Risorsa")}</button>
    };
    let detail = match resources.get(sel) {
        Some(r) if selected.0 == K_RESOURCE => {
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
        _ => Html::default(),
    };
    (items, tools, detail)
}

/// Plain-words explanation of a command kind, for the role action checkboxes.
fn action_tip(action: &str) -> &'static str {
    match action {
        "CutFireLine" => "Apre una linea tagliafuoco (senza vegetazione) lungo un percorso: ferma l’avanzata se è completata prima del fuoco.",
        "WaterDrop" => "Lancio d’acqua da aerei o elicotteri su una zona: rallenta o spegne il fuoco sotto il passaggio.",
        "MonitorArea" => "Manda il drone del comandante su alcune zone (fino a 4): finché è in posizione tiene aggiornata la sua mappa ogni pochi minuti, poi rientra e si ricarica.",
        "Reconnoitre" => "Manda una squadra a terra in ricognizione su una zona: all’arrivo riferisce una volta cosa vede e la mappa del ruolo si aggiorna a quell’ora. La squadra resta impiegata finché l’EXCON non ne conferma il rientro.",
        "DeployEngine" => "Invia un’autobotte o una squadra a terra in un luogo: agisce dopo il viaggio dalla base, se le strade sono praticabili.",
        "EvacuateZone" => "Ordina di evacuare tutta una zona della griglia, con i suoi edifici e abitanti.",
        "EvacuateBuilding" => "Ordina di evacuare un singolo edificio, ad esempio una scuola o una casa di riposo.",
        "IssueWarning" => "Allerta la popolazione di una zona: la zona resta segnata come allertata (con l’ora) e un’evacuazione successiva lì è due volte più rapida.",
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

/// `options` plus the current value when it is not among them (a dangling reference stays visible).
fn with_current(options: &[(String, String)], value: &str) -> Vec<(String, String)> {
    let mut v = options.to_vec();
    if !options.iter().any(|(id, _)| id == value) {
        v.insert(0, (value.to_string(), if value.is_empty() { ex("— scegli —").to_string() } else { format!("{value} ⚠") }));
    }
    v
}

/// List entries, "add" button and editor of the reinforcement offers.
fn reinforcements_parts(o: &Open, open: &UseStateHandle<Option<Open>>, selected: &Sel) -> (Html, Html, Html) {
    let e = &o.pack.edits;
    let list_ = e.reinforcements.clone().unwrap_or_default();
    let resources: Vec<(String, String)> = e.resource_types.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let roles: Vec<(String, String)> = e.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let n = list_.len();
    let sel = if selected.0 == K_REINF { selected.1.min(n.saturating_sub(1)) } else { usize::MAX };
    let items = html! {<>
        <h5 class="md-group" title={ex("Capacità esterna che un ruolo può richiedere durante l’esercitazione.")}>{ex("Rinforzi")}</h5>
        {for list_.iter().enumerate().map(|(i, r)| {
            md_item(i == sel, { let selected = selected.clone(); Callback::from(move |_| selected.set((K_REINF, i))) }, html!{<>
                <b class="md-id">{r.resource.clone()}</b>
                <span class="md-name">{format!("×{} → {}", r.max_qty, r.assigned_to)}</span>
                <small class="md-sub">{crate::exfmt!("dopo {} min", "after {} min", r.delay_min)}{" · "}{r.source.clone()}</small>
            </>})
        })}
        { if n == 0 { html!{<p class="muted">{ex("Nessun rinforzo definito.")}</p>} } else { Html::default() } }
    </>};
    let tools = html! {
        <button class="primary" onclick={{
            let first_resource = resources.first().map(|r| r.0.clone()).unwrap_or_default();
            let first_role = roles.first().map(|r| r.0.clone()).unwrap_or_default();
            let (selected, add) = (selected.clone(), upd(open, move |p, _: MouseEvent| {
                p.edits.reinforcements.get_or_insert_with(Vec::new).push(ReinforcementOffer {
                    resource: first_resource.clone(), max_qty: 1, delay_min: 60, source: String::new(), assigned_to: first_role.clone(),
                });
            }));
            Callback::from(move |e| { selected.set((K_REINF, n)); add.emit(e) })
        }}>{ex("+ Rinforzo")}</button>
    };
    let detail = match list_.get(sel) {
        Some(r) if selected.0 == K_REINF => {
            let i = sel;
            let r = r.clone();
            html! {<div class="rr-body" key={i}>
                <p class="muted">{ex("Capacità esterna che un ruolo può richiedere: arriva dopo il ritardo indicato. L’EXCON approva comunque ogni richiesta e ogni arrivo.")}</p>
                if resources.is_empty() || roles.is_empty() {<p class="warn" role="alert">{ex("Servono almeno una risorsa e un ruolo per definire un rinforzo.")}</p>}
                <div class="settings-fields">
                    {choice("Risorsa", "Che cosa arriva: uno dei tipi di risorsa definiti.", &r.resource, &with_current(&resources, &r.resource), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.resource = v }))}
                    {choice("Riceve il ruolo", "Il ruolo (pagina player) che può richiedere e che riceve le unità all’arrivo.", &r.assigned_to, &with_current(&roles, &r.assigned_to), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.assigned_to = v }))}
                    {number("Quantità massima", "Capacità massima che il ruolo può richiedere in totale da questa fonte (unità del tipo di risorsa).", r.max_qty.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.max_qty = n }))}
                    {number("Ritardo (min)", "Minuti simulati tra l’approvazione della richiesta e l’arrivo alla base di partenza.", r.delay_min.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.delay_min = n }))}
                    {field("Provenienza", "Testo libero: da dove arrivano (es. «Colonna mobile regionale»). Compare nei messaggi ai giocatori.", r.source.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.source = v }))}
                </div>
                {confirm_delete("Elimina rinforzo", "Elimina questa offerta di rinforzo.",
                    upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.reinforcements.as_mut() { if i < l.len() { l.remove(i); } } }))}
            </div>}
        }
        _ => Html::default(),
    };
    (items, tools, detail)
}

#[function_component(TeamTab)]
fn team_view(props: &ListProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected: Sel = use_state(|| (K_ROLE, 0usize));
    let (ri, rt, rd) = roles_parts(o, open, &selected);
    let (si, st, sd) = resources_parts(o, open, &selected);
    let (fi, ft, fd) = reinforcements_parts(o, open, &selected);
    let items = html! {<>{ri}{si}{fi}</>};
    let tools = html! {<>{rt}{st}{ft}</>};
    let detail = match selected.0 {
        K_RESOURCE => sd,
        K_REINF => fd,
        _ => rd,
    };
    let detail = if matches!(&detail, Html::VList(v) if v.is_empty()) {
        html! {<p class="muted">{ex("Scegli una voce dall’elenco o aggiungine una.")}</p>}
    } else {
        detail
    };
    html! {<div class="rr-tab">
        <p class="muted">{ex("Ruoli (pagine player), tipi di risorsa e rinforzi in un unico elenco. Azioni e risorse decidono cosa ogni ruolo può proporre; l’EXCON approva comunque ogni azione.")}</p>
        {master_detail(items, tools, detail)}
        {excon_roles_list(o.pack.edits.excon_roles.clone().unwrap_or_default(), o.pack.edits.default_persona.clone().unwrap_or_default(), open)}
    </div>}
}

fn team_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<TeamTab o={o.clone()} open={open.clone()}/>}
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
    use plan::{Key, Target};
    let (o, open, json, message) = (&props.o, &props.open, &props.json, &props.message);
    let selected = use_state(|| 0usize);
    // Band whose header was clicked: where "Aggiungi" puts new items.
    let band_sel = use_state(|| Option::<usize>::None);
    // Row being dragged (kept outside the render so a drop always sees it) and the drop target under the pointer.
    let drag_src = use_mut_ref(|| Option::<Key>::None);
    let dragging = use_state_eq(|| Option::<Key>::None);
    let hover = use_state_eq(|| Option::<Target>::None);
    let e = &o.pack.edits;
    let msel = e.msel.clone().unwrap_or_default();
    let n = msel.len();
    let na: usize = msel.iter().map(|m| m.actions.len()).sum();
    let tm = e.turn_minutes.unwrap_or(plan::DEFAULT_TURN_MINUTES).clamp(1, 1440);
    let start_s = e.start_clock_s.unwrap_or(0);
    let roles: Vec<(String, String)> = e.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let excon_roles: Vec<String> = e.excon_roles.iter().flatten().filter(|r| !r.trim().is_empty()).cloned().collect();
    let sel = (*selected).min(n.saturating_sub(1));
    let entries = Rc::new(plan::sorted_entries(&msel, tm));
    let bands = Rc::new(plan::bands(&entries, tm));
    let implicit = entries.iter().filter(|x| !x.explicit).count();
    let current: Option<Key> = (n > 0).then_some(Key { idx: sel });
    let select = {
        let (selected, band_sel) = (selected.clone(), band_sel.clone());
        Callback::from(move |k: Key| {
            band_sel.set(None);
            selected.set(k.idx)
        })
    };
    // The resulting time of the drop under the pointer, shown while dragging.
    let hint = match (*dragging, *hover) {
        (Some(k), Some(t)) => plan::resolve_drop(&entries, &bands, k, t, tm)
            .map(|d| format!("→ {} · {} {}", scenario::format_clock(start_s + d.at_s), ex("turno"), plan::turn_of(d.at_s, tm))),
        _ => None,
    };
    let do_drop = {
        let (open, drag_src, dragging, hover, selected, entries, bands) =
            (open.clone(), drag_src.clone(), dragging.clone(), hover.clone(), selected.clone(), entries.clone(), bands.clone());
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
            selected.set(idx);
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
            let (ni, nact) = (band.items.len(), band.items.iter().map(|&p| msel[entries[p].key.idx].actions.len()).sum::<usize>());
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
                    let m = &msel[key.idx];
                    let (id, title, sub, glyph, tip) = (m.id.clone(), m.title.clone(), m.event.clone(), "●", ex("Inject: messaggio ai giocatori"));
                    let title = if title.trim().is_empty() { ex("(senza titolo)").to_string() } else { title };
                    let warns = plan::inject_warnings(m, tm);
                    let warn = warns.first().cloned();
                    let link = match &warn {
                        Some(plan::Warn::BeforeStart(a, t)) => Some(format!("⚠ {} {a} ({})", ex("azione prima dell’inizio"), plan::hhmm(start_s + t))),
                        None if !m.actions.is_empty() => {
                            let pre = m.actions.iter().filter(|a| a.offset_s < 0).count();
                            Some(if pre > 0 {
                                crate::exfmt!("◆ {} azioni ({} prima)", "◆ {} actions ({} before)", m.actions.len(), pre)
                            } else {
                                crate::exfmt!("◆ {} azioni", "◆ {} actions", m.actions.len())
                            })
                        }
                        None => None,
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
                    html!{<div ref={nr} role="button" tabindex="0" draggable="true" data-key={format!("inject:{}", key.idx)}
                        class={classes!("tl-row", "inject", (current == Some(key)).then_some("active"),
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
        {if n == 0 { html!{<p class="muted">{ex("Nessun inject: aggiungine uno per iniziare.")}</p>} } else { Html::default() }}
    </>};
    let new_time = plan::new_item_time(&entries, &bands, *band_sel);
    let tools = html! {<>
        <span class="muted">{format!("{n} inject · {na} {}", ex("azioni"))}</span>
        {add_inject_button(open, &selected, &band_sel, n, new_time, tm)}
        <button title={ex("Riordina gli inject per orario, mantenendo l’ordine attuale a parità di orario.")} onclick={{
            let (selected, open2) = (selected.clone(), open.clone());
            let cur_sel = sel;
            let sort = upd(open, move |p, _: MouseEvent| { plan::sort_by_time(&mut p.edits); });
            Callback::from(move |e| {
                // The selection follows the item it points at.
                if let Some(cur) = &*open2 {
                    let mut probe = cur.pack.edits.clone();
                    let im = plan::sort_by_time(&mut probe);
                    if let Some(&k) = im.get(cur_sel) { selected.set(k); }
                }
                sort.emit(e)
            })
        }}>{ex("Ordina per orario")}</button>
        <button disabled={implicit == 0} title={ex("Scrive l’orario che il turno implica in ogni inject che non ne ha uno proprio: da quel momento non dipendono più dalla durata del turno.")} onclick={upd(open, |p, _: MouseEvent| { plan::make_explicit(&mut p.edits); })}>{crate::exfmt!("Rendi espliciti i tempi ({})", "Make times explicit ({})", implicit)}</button>
    </>};
    let detail = match msel.get(sel) {
        Some(m) => inject_detail(sel, n, m, &msel, tm, start_s, &roles, &excon_roles, o, open, &selected),
        None => html! {<p class="muted">{ex("Nessun inject: aggiungine uno per iniziare.")}</p>},
    };
    html! {<div class="inject-tab">
        <p class="muted">{ex("Una sola timeline in ordine di tempo, divisa in turni. Gli inject (●) sono messaggi preparati: l’EXCON decide quando inviarli e, quando il tempo li raggiunge, li trova proposti. Ogni inject porta le sue azioni (◆), i cambiamenti del mondo che lo accompagnano: partono quando l’EXCON lo invia (o dopo qualche minuto), oppure, se anticipate, da sole prima dell’inject. Si modificano dentro l’inject. Trascina le righe per cambiarne l’orario.")}</p>
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

/// A point-picking map of the scenario for the action tool forms.
fn scenario_map_factory(o: &Open) -> crate::controllo::MapFactory {
    let (g, o2) = (geom(o), o.clone());
    crate::controllo::MapFactory(std::rc::Rc::new(move |onpick: Callback<Value>, pending: Option<Value>| {
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
    }))
}

fn add_inject_button(
    open: &UseStateHandle<Option<Open>>,
    selected: &UseStateHandle<usize>,
    band_sel: &UseStateHandle<Option<usize>>,
    n: usize,
    at_s: i64,
    tm: u32,
) -> Html {
    let (selected, band_sel) = (selected.clone(), band_sel.clone());
    let add = upd(open, move |p, _: MouseEvent| {
        let l = p.edits.msel.get_or_insert_with(Vec::new);
        let mut k = l.len() + 1;
        while l.iter().any(|m| m.id == format!("I-{k:02}")) { k += 1; }
        l.push(InjectTemplate {
            id: format!("I-{k:02}"), turn: plan::turn_of(at_s, tm), title: String::new(), sender: String::new(),
            recipients: vec!["all".into()], text: String::new(), objective: String::new(), event: String::new(), at_s: Some(at_s), actions: vec![],
        });
    });
    html! {<button class="primary" title={ex("Aggiunge un inject con orario esplicito: all’inizio del turno scelto (clic sull’intestazione) oppure in coda alla timeline. Destinatari «tutti».")} onclick={Callback::from(move |e| { band_sel.set(None); selected.set(n); add.emit(e) })}>{ex("Aggiungi inject")}</button>}
}

/// The editor of the selected inject.
fn inject_detail(i: usize, n: usize, m: &InjectTemplate, msel: &[InjectTemplate], tm: u32, start_s: i64, roles: &[(String, String)], excon_roles: &[String], o: &Open, open: &UseStateHandle<Option<Open>>, selected: &UseStateHandle<usize>) -> Html {
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
                // Action ids are unique across the scenario: the copy gets new ones.
                let mut used: Vec<String> = l.iter().flat_map(|x| x.actions.iter().map(|a| a.id.clone())).collect();
                for a in c.actions.iter_mut() {
                    let mut k = 1;
                    while used.contains(&format!("{}-copia{k}", a.id)) { k += 1; }
                    a.id = format!("{}-copia{k}", a.id);
                    used.push(a.id.clone());
                }
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
        {time_block(plan::Key { idx: i }, plan::inject_s(m, tm), m.at_s.is_some(), tm, start_s, open)}
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
        {inject_actions(i, m, msel, o, open)}
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

/// The «Azioni» section of an inject: the shared action form for each of its actions.
fn inject_actions(i: usize, m: &InjectTemplate, msel: &[InjectTemplate], o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let onchange = upd(open, move |p, v: Vec<scenario::InjectAction>| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.actions = v });
    let taken: Vec<String> = msel.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, x)| x.actions.iter().map(|a| a.id.clone())).collect();
    let w = o.pack.edits.weather;
    let common = crate::controllo::CommonProps {
        stat: Value::Null, state: Value::Null, master: Value::Null, responses: Default::default(),
        busy: false, error: "".into(), done: 0, action: Callback::noop(), onclose: Callback::noop(),
    };
    html! {<crate::action_form::ActionList key={format!("actions-{i}")} actions={m.actions.clone()} {onchange} allow_pre=true show_id=true {taken}
        prefix="A" {common} map_for={Some(scenario_map_factory(o))} weather={w.map_or((0.0, 10.0, 10.0), |w| (w.wind_dir_deg, w.wind_speed_kmh, w.moisture_pct))}/>}
}

fn advanced_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let ops = e.operations.clone().unwrap_or_default();
    let policy = e.tactical_policy.clone().unwrap_or_default();
    type Flag = (&'static str, &'static str, fn(&scenario::Operations) -> bool, fn(&mut scenario::Operations, bool));
    let flags: [Flag; 9] = [
        ("Percorsi stradali", "I mezzi seguono la rete stradale reale. Senza un percorso possibile restano in attesa o falliscono: non c’è mai una linea retta di ripiego.", |o| o.road_routing, |o, v| o.road_routing = v),
        ("Soppressione progressiva", "Il lavoro di squadre e mezzi si accumula nel tempo (linee che si completano a poco a poco) invece di avere effetto istantaneo.", |o| o.progressive_suppression, |o, v| o.progressive_suppression = v),
        ("Traffico aggregato", "Simula il traffico dei veicoli civili come flusso complessivo: può rallentare le strade e gli spostamenti, senza modellare ogni singola auto.", |o| o.traffic, |o, v| o.traffic = v),
        ("Esposizione all’intensità", "Squadre e veicoli vicini al fronte accumulano esposizione in base all’intensità del fuoco; oltre certe soglie devono ritirarsi.", |o| o.intensity_exposure, |o, v| o.intensity_exposure = v),
        ("Spotting (braci che accendono nuovi focolai)", "Spotting: il vento trasporta braci che accendono nuovi focolai davanti al fronte. Rende il fuoco più imprevedibile: da attivare solo se lo scenario lo richiede.", |o| o.spotting, |o, v| o.spotting = v),
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
        {gen_section("Modello operativo", "Quanto realistica è la logistica: strade, tempi, comando sul posto. Ogni opzione è indipendente; tutte spente = regole semplificate originali.", html! {<>
            <div class="gen-group wide">{gen_flag("Avvia con il modello operativo attivo", "Parte con le opzioni sotto attive. Se spento, ci si affida alle regole semplificate o all’opzione --operational-model della riga di comando.", run.operational_model == Some(true), upd(open, |p, on: bool| p.run.operational_model = on.then_some(true)))}</div>
            <div class="gen-group wide"><h5>{ex("Strade e spostamenti")}</h5>{for [0, 5, 6, 7, 2].map(flag)}</div>
            <div class="gen-group wide"><h5>{ex("Fuoco e rischio")}</h5>{for [1, 3, 4].map(flag)}</div>
            <div class="gen-group wide"><h5>{ex("Comando sul posto")}</h5>{for [8].map(flag)}</div>
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
    ("team", "Ruoli e risorse", "Chi partecipa, quali mezzi ha e quali rinforzi può richiedere."),
    ("injects", "Timeline", "Inject (messaggi) con le loro azioni (cambiamenti del mondo, inneschi compresi) che l’EXCON invia durante l’esercitazione."),
    ("advanced", "Avanzate", "Impostazioni tecniche: modello operativo, seme casuale, realizzazioni dell’ensemble."),
];

#[function_component(ScenarioEditor)]
pub fn scenario_editor(props: &Props) -> Html {
    let settings_slot = use_context::<SettingsSlot>();
    let library = use_state(|| None::<Value>);
    let territory = use_state(String::new);
    let open = use_state(|| None::<Open>);
    let tab = use_state(|| "general".to_string());
    let message = use_state(String::new);
    let problems = use_state(Vec::<String>::new);
    let busy = use_state(|| false);
    let confirm = use_state(String::new);
    let json = use_state(String::new);
    let pack_json = use_state(String::new);
    let pack_json_open = use_state(|| false);

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

    // WebMCP tools exist only while a scenario is open here, and act on this
    // draft. `current` mirrors the latest state for the tools' closures.
    let current = use_mut_ref(|| None::<Open>);
    *current.borrow_mut() = (*open).clone();
    {
        let (open_h, current, refresh) = (open.clone(), current.clone(), refresh.clone());
        use_effect_with(open.is_some(), move |editing| {
            use wasm_bindgen::JsCast;
            let mut handler = None;
            if *editing {
                let (snap, rep, sav) = (current.clone(), current.clone(), current.clone());
                let (open_r, open_s) = (open_h.clone(), open_h.clone());
                crate::webmcp::attach(crate::webmcp::Editor {
                    snapshot: Box::new(move || {
                        snap.borrow().as_ref().map(|o| crate::webmcp::Draft {
                            pack: serde_json::to_value(&o.pack).unwrap_or_default(),
                            reference: o.reference.clone(),
                            saved_id: o.saved_id.clone(),
                            dirty: o.dirty,
                        })
                    }),
                    replace: Box::new(move |v| {
                        let pack: ScenarioPack = serde_json::from_value(v).map_err(|e| format!("not a valid pack: {e}"))?;
                        let mut cur = rep.borrow().clone().ok_or("no scenario is open")?;
                        cur.pack = pack;
                        cur.dirty = true;
                        *rep.borrow_mut() = Some(cur.clone());
                        open_r.set(Some(cur));
                        Ok(())
                    }),
                    saved: Box::new(move |id| {
                        let held = sav.borrow().clone();
                        if let Some(mut cur) = held {
                            cur.saved_id = Some(id);
                            cur.dirty = false;
                            *sav.borrow_mut() = Some(cur.clone());
                            open_s.set(Some(cur));
                        }
                    }),
                });
                let h = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || refresh.emit(()));
                let _ = api::window().add_event_listener_with_callback(crate::webmcp::CHANGED_EVENT, h.as_ref().unchecked_ref());
                handler = Some(h);
            } else {
                crate::webmcp::detach();
            }
            move || {
                if let Some(h) = handler {
                    let _ = api::window().remove_event_listener_with_callback(crate::webmcp::CHANGED_EVENT, h.as_ref().unchecked_ref());
                }
            }
        });
    }
    // Leaving the view while editing also withdraws the tools.
    use_effect_with((), |_| || crate::webmcp::detach());

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
    html! {<main class={classes!("page","scenario-editor",open.is_some().then_some("editing"))} id="scenario-editor">
        if open.is_none() {<div class="settings-split"><section class="card settings-card" id="app-settings">
            <h2>{ex("Impostazioni")}</h2>
            if let Some(slot) = &settings_slot {{slot.0.clone()}}
            if !cfg!(feature = "standalone") {<crate::server_settings::ServerSettings revision={props.revision}/>}
        </section>
        <section class="card settings-scenarios">
            <h2>{ex("Scenari")}</h2>
            if cfg!(feature="standalone") {<a class="button" href="?editor=1">{"Apri editor scenario 3D"}</a>}
            <p class="muted">{ex("Uno scenario è un territorio (mappa, edifici, strade) più ciò che lo rende un’esercitazione: ruoli, risorse, meteo e inject con le loro azioni. «Avvia» inizia una nuova esercitazione e scarta la sessione in corso.")}</p>
            <ul class="scenario-legend muted">
                <li><span class="pill">{ex("in corso")}</span> {ex("è l’esercitazione che sta girando ora.")}</li>
                <li><span class="pill">{ex("prossimo avvio")}</span> {ex("verrà usato al prossimo riavvio dell’applicazione.")}</li>
                <li><span class="pill">{ex("incluso")}</span> {ex("fa parte dell’applicazione e non si sovrascrive: usa Personalizza per ottenerne una copia modificabile.")}</li>
            </ul>
            <label>{ex("Filtra per territorio")}
                <select id="scenario-territory" value={(*territory).clone()} onchange={{let territory=territory.clone(); Callback::from(move |event: Event| territory.set(event.target_unchecked_into::<web_sys::HtmlSelectElement>().value()))}}>
                    <option value="" selected={territory.is_empty()}>{ex("Tutti i territori")}</option>
                    {for list(&lib["bases"]).iter().map(|base| {let id=text(base); let name=packs.iter().find(|p| text(&p["id"])==id).map(|p|text(&p["name"])).unwrap_or(id); html!{<option value={id.to_string()} selected={territory.as_str()==id}>{name}</option>}})}
                </select>
            </label>
            <div class="row">
                <label class="button" title={ex("Legge uno scenario da un file JSON e lo apre nell’editor come nuovo scenario; salva poi per aggiungerlo alla libreria.")}>{ex("Importa scenario da file")}
                    <input id="import-scenario-file" type="file" accept=".json,application/json" hidden=true onchange={{
                        let (open, message, problems, tab) = (open.clone(), message.clone(), problems.clone(), tab.clone());
                        Callback::from(move |e: Event| {
                            let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
                            let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
                            input.set_value("");
                            let (open, message, problems, tab) = (open.clone(), message.clone(), problems.clone(), tab.clone());
                            wasm_bindgen_futures::spawn_local(async move {
                                match read_pack(file).await {
                                    Ok(pack) => match call(&format!("/api/master/scenarios/{}", api::encode(&pack.base)), None).await {
                                        Ok(v) => {
                                            problems.set(vec![]);
                                            message.set(String::new());
                                            tab.set("general".into());
                                            open.set(Some(Open { pack, reference: v["reference"].clone(), saved_id: None, dirty: true }));
                                        }
                                        Err(e) => message.set(format!("Territorio «{}» non disponibile: {e}", pack.base)),
                                    },
                                    Err(e) => message.set(e),
                                }
                            });
                        })
                    }}/>
                </label>
            </div>
            <table class="scenario-list"><thead><tr>
                <th title={ex("Nome e descrizione dello scenario.")}>{ex("Nome")}</th>
                <th title={ex("La mappa su cui si svolge: terreno, edifici e strade. Non si modifica da qui.")}>{ex("Territorio")}</th>
                <th><span class="sr-only">{ex("Azioni")}</span></th></tr></thead><tbody>
            {for packs.iter().filter(|p| territory.is_empty() || text(&p["base"]) == territory.as_str()).map(|p| {
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
                        <button class="clone-scenario" disabled={*busy} title={ex("Apre una copia di questo scenario con un nuovo identificativo.")} onclick={{let load = load.clone(); let id = id.clone(); Callback::from(move |_| load.emit((id.clone(), true)))}}>{ex("Duplica")}</button>
                        {twice(format!("start:{id}"), ex("Avvia").to_string(), "primary", "Avvia una nuova esercitazione da questo scenario. La sessione in corso (turni, messaggi, decisioni) viene scartata.", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("start", id.clone())))})}
                        if !builtin && !is_running {{twice(format!("delete:{id}"), ex("Elimina").to_string(), "danger", "Elimina definitivamente questo scenario dalla libreria. Non si può annullare.", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("/api/master/scenarios/delete", id.clone())))})}}
                    </td>
                </tr>}
            })}
            </tbody></table>
            {busy_note}
            if !message.is_empty() {<p role="status" id="scenario-message">{(*message).clone()}</p>}
        </section></div>}
        if let Some(o) = &*open {
            <section class="card" id="scenario-form">
                <div class="row scenario-head">
                    <h2>{o.pack.name.clone()}</h2>
                    <span class="pill" title={if o.saved_id.is_some() {ex("Identificativo con cui lo scenario è salvato nella libreria.")} else {ex("Questo scenario non è ancora nella libreria: salvalo per conservarlo.")}}>{if o.saved_id.is_some() {o.pack.id.clone()} else {ex("non salvato").to_string()}}</span>
                    if o.dirty {<span class="pill pending" title={ex("Hai cambiato qualcosa che non è ancora stato salvato.")}>{ex("modifiche non salvate")}</span>}
                    if o.saved_id.is_none() {
                        <label class="scenario-id" title={ex("Nome breve che identifica lo scenario nella libreria: solo lettere minuscole, cifre, «_» e «-». Dopo il primo salvataggio non cambia più.")}>{ex("Identificativo")}
                            <input value={o.pack.id.clone()} placeholder="a-z 0-9 _ -" oninput={upd(&open, |p, v: String| p.id = v).reform(api::input)}/></label>
                    }
                    <div class="grow"/>
                    if o.dirty {
                        {twice("close".into(), ex("← Scenari").to_string(), "", "Chiude l’editor e perde le modifiche non salvate.", {let open = open.clone(); let confirm = confirm.clone(); Callback::from(move |_| { confirm.set(String::new()); open.set(None) })})}
                    } else {
                        <button title={ex("Chiude l’editor e torna all’elenco.")} onclick={{let open = open.clone(); Callback::from(move |_| open.set(None))}}>{ex("← Scenari")}</button>
                    }
                </div>
                <div class="scenario-layout"><nav class="scenario-tabs" role="tablist" aria-orientation="vertical" aria-label={ex("Sezioni dello scenario")}>
                    {for TABS.iter().map(|(id, label, tip)| {
                        let tab = tab.clone();
                        let id = id.to_string();
                        let on = *tab == id;
                        html!{<button role="tab" aria-selected={on.to_string()} title={ex(tip)} class={classes!(on.then_some("active"))} data-tab={id.clone()} onclick={Callback::from(move |_| tab.set(id.clone()))}>{ex(label)}</button>}
                    })}
                </nav>
                <div class="scenario-tab" role="tabpanel">
                    {match tab.as_str() {
                        "team" => team_tab(o, &open),
                        "injects" => injects_tab(o, &open, &json, &message),
                        "advanced" => advanced_tab(o, &open),
                        _ => general(o, &open),
                    }}
                </div></div>
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
                        <button id="export-scenario" title={ex("Scarica lo scenario intero come file JSON.")} onclick={{
                            let (message, pack) = (message.clone(), o.pack.clone());
                            Callback::from(move |_| {
                                let value = serde_json::to_value(&pack).unwrap_or_default();
                                match api::download_as(&value, &format!("{}.json", pack.id)) {
                                    Ok(()) => message.set(ex("Scenario esportato").to_string()),
                                    Err(e) => message.set(e),
                                }
                            })
                        }}>{ex("Esporta scenario (JSON)")}</button>
                        <label class="button" title={ex("Sostituisce il contenuto dell’editor con lo scenario letto da un file JSON; salva poi per conservarlo. L’identificativo dello scenario resta quello attuale.")}>{ex("Importa scenario (JSON)")}
                            <input id="import-scenario" type="file" accept=".json,application/json" hidden=true onchange={{
                                let (open, message) = (open.clone(), message.clone());
                                Callback::from(move |e: Event| {
                                    let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
                                    let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
                                    input.set_value("");
                                    let (open, message) = (open.clone(), message.clone());
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match read_pack(file).await {
                                            Ok(mut pack) => {
                                                if let Some(cur) = &*open {
                                                    // An imported pack never renames the scenario it is imported into.
                                                    pack.id = cur.pack.id.clone();
                                                    open.set(Some(Open { pack, dirty: true, ..cur.clone() }));
                                                    message.set(ex("Scenario importato nell’editor · salva per conservarlo").to_string());
                                                }
                                            }
                                            Err(e) => message.set(e),
                                        }
                                    });
                                })
                            }}/>
                        </label>
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

/// Reads a scenario pack from a JSON file chosen by the user.
async fn read_pack(file: web_sys::File) -> Result<ScenarioPack, String> {
    let text = wasm_bindgen_futures::JsFuture::from(file.text())
        .await
        .map_err(|_| ex("File non leggibile").to_string())?
        .as_string()
        .unwrap_or_default();
    serde_json::from_str(&text).map_err(|e| format!("JSON non valido: {e}"))
}

/// Begin a new exercise from `id`. In the browser the page reloads into it.
pub(crate) async fn start_scenario(id: &str) -> Result<String, String> {
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
