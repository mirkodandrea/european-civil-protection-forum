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
use serde_json::{json, Value};
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
        {gen_section("Orario", "Quando comincia l’esercitazione, nell’ora simulata dello scenario.", html! {<>
            {gen_number("Giorno di inizio", "Giorno simulato in cui parte l’esercitazione: 1 è il primo giorno. Serve per scenari che durano più di una notte.", "1", None, "1", "", (start / 86_400 + 1).to_string(), day_cb)}
            <label title={ex("Ora del giorno (24 ore) in cui parte la simulazione. Gli inject sono datati rispetto a questo istante.")}>{ex("Ora di inizio (HH:MM)")}
                <input type="time" required=true value={clock_text(start)} oninput={upd(open, |p, v: String| {
                    let day = p.edits.start_clock_s.unwrap_or(0) / 86_400 + 1;
                    if let Some(t) = parse_clock(day, &v) { p.edits.start_clock_s = Some(t); }
                }).reform(api::input)}/>
                {hint("Ora del giorno (24 ore) in cui parte la simulazione. Gli inject sono datati rispetto a questo istante.")}</label>
        </>})}
        {gen_section("Meteo", "Condizioni iniziali che guidano la propagazione del fuoco. Restano costanti durante l’esercitazione.", html! {<>
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

fn roles_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let roles = e.roles.clone().unwrap_or_default();
    let resources = e.resource_types.clone().unwrap_or_default();
    let levels: Vec<(String, String)> = ["national", "regional", "local"]
        .iter()
        .map(|l| (l.to_string(), l.to_string()))
        .collect();
    let languages: Vec<(String, String)> = api::LANGUAGES
        .iter()
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect();
    let actions: Vec<String> = list(&o.reference["actions"]).iter().map(|a| text(a).to_string()).collect();
    html! {<>
        <p class="muted">{ex("Ogni ruolo è una pagina player. Azioni e risorse decidono cosa può proporre; l’EXCON approva comunque ogni azione.")}</p>
        {for roles.iter().enumerate().map(|(i, r)| {
            let r = r.clone();
            html!{<fieldset class="inject-row" key={i}>
                <legend>{format!("{} · {}", if r.id.is_empty() {"nuovo"} else {&r.id}, r.name)}</legend>
                <div class="settings-fields">
                    {field("Identificativo", "", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", "", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {choice("Livello", "", &r.level, &levels, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.level = v }))}
                    {choice("Lingua", "", &r.language, &languages, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.language = v }))}
                </div>
                {area("Descrizione", "", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
                <h4>{ex("Risorse disponibili per turno")}</h4>
                <div class="settings-fields">
                    {for resources.iter().map(|res| {
                        let id = res.id.clone();
                        let qty = r.resources.get(&res.id).copied().unwrap_or(0);
                        number_owned(format!("{} ({})", res.name, res.id), qty.to_string(), upd(open, move |p, v: String| {
                            if let (Some(x), Some(n)) = (p.edits.roles.as_mut().and_then(|l| l.get_mut(i)), num::<u32>(&v)) {
                                if n == 0 { x.resources.remove(&id); } else { x.resources.insert(id.clone(), n); }
                            }
                        }))
                    })}
                </div>
                <h4>{ex("Azioni che può proporre")}</h4>
                <div class="settings-fields">
                    {for actions.iter().map(|a| {
                        let action = a.clone();
                        check(a.clone(), "", r.allowed_actions.contains(a), upd(open, move |p, on: bool| {
                            if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) {
                                x.allowed_actions.retain(|y| *y != action);
                                if on { x.allowed_actions.push(action.clone()); }
                            }
                        }))
                    })}
                </div>
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.roles.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina ruolo")}</button>
            </fieldset>}
        })}
        <button onclick={upd(open, |p, _: MouseEvent| {
            let l = p.edits.roles.get_or_insert_with(Vec::new);
            let n = l.len() + 1;
            l.push(Role {
                id: format!("role_{n}"), name: format!("Ruolo {n}"), level: "local".into(), language: "en".into(),
                description: String::new(), allowed_actions: vec![], resources: Default::default(),
            });
        })}>{ex("Aggiungi ruolo")}</button>
    </>}
}

/// A number field whose label is not a static string.
fn number_owned(label: String, value: String, cb: Callback<String>) -> Html {
    html! {<label>{label}<input type="number" min="0" step="1" value={value} oninput={cb.reform(api::input)}/></label>}
}

fn resources_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let resources = o.pack.edits.resource_types.clone().unwrap_or_default();
    html! {<>
        <p class="muted">{ex("Tipi di risorsa del lancio: acqua, squadre, mezzi. La velocità è quella con cui raggiungono il luogo dalla base di partenza.")}</p>
        {for resources.iter().enumerate().map(|(i, r)| {
            let r = r.clone();
            html!{<fieldset class="inject-row" key={i}>
                <legend>{format!("{} · {}", r.id, r.name)}</legend>
                <div class="settings-fields">
                    {field("Identificativo", "", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", "", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {number("Velocità di spostamento (m/s, vuoto = predefinita)", "", r.speed_mps.map(|s| s.to_string()).unwrap_or_default(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.speed_mps = num(&v) }))}
                </div>
                {area("Descrizione", "", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.resource_types.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina risorsa")}</button>
            </fieldset>}
        })}
        <button onclick={upd(open, |p, _: MouseEvent| {
            let l = p.edits.resource_types.get_or_insert_with(Vec::new);
            let n = l.len() + 1;
            l.push(ResourceType { id: format!("resource_{n}"), name: format!("Risorsa {n}"), description: String::new(), speed_mps: None });
        })}>{ex("Aggiungi risorsa")}</button>
    </>}
}

/// Zone grid and ignitions; a click on the map moves the selected ignition.
fn ignitions_tab(o: &Open, open: &UseStateHandle<Option<Open>>, selected: &UseStateHandle<usize>, map: &NodeRef) -> Html {
    let ignitions = o.pack.edits.initial_ignitions.clone().unwrap_or_default();
    let width = o.reference["grid"]["width_m"].as_f64().unwrap_or(1.0);
    let height = o.reference["grid"]["height_m"].as_f64().unwrap_or(1.0);
    let zone_size = o.reference["zone_grid"]["size_m"].as_f64().unwrap_or(500.0);
    let cols = o.reference["zone_grid"]["cols"].as_u64().unwrap_or(0) as usize;
    let rows = o.reference["zone_grid"]["rows"].as_u64().unwrap_or(0) as usize;
    let sel = (*selected).min(ignitions.len().saturating_sub(1));
    let stroke = width / 400.0;
    let onclick = {
        let (open, map) = (open.clone(), map.clone());
        Callback::from(move |e: MouseEvent| {
            let Some(cur) = &*open else { return };
            // Yew delegates events: `current_target` is not the map, so keep its node.
            let Some(svg) = map.cast::<web_sys::Element>() else { return };
            let rect = svg.get_bounding_client_rect();
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                return;
            }
            let x = (e.client_x() as f64 - rect.left()) / rect.width() * width;
            let y = height - (e.client_y() as f64 - rect.top()) / rect.height() * height;
            let mut next = cur.clone();
            if let Some(i) = next.pack.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(sel)) {
                i.at = [x.round() as f32, y.round() as f32];
                next.dirty = true;
                open.set(Some(next));
            }
        })
    };
    let zones: Vec<(String, f64)> = list(&o.reference["zones"]).iter().map(|z| (text(z).to_string(), 0.0)).collect();
    let centres = &o.reference["zone_centres"];
    let zone_options: Vec<(String, String)> = std::iter::once((String::new(), ex("Imposta al centro della zona…").to_string()))
        .chain(zones.iter().map(|(z, _)| (z.clone(), z.clone())))
        .collect();
    html! {<>
        <p class="muted">{ex("Clic sulla mappa per spostare l’innesco selezionato. Sotto i 40 m di raggio l’innesco si spegne da solo.")}</p>
        <svg ref={map.clone()} class="ignition-map" viewBox={format!("0 0 {width} {height}")} role="img" aria-label={ex("Mappa degli inneschi")} onclick={onclick}>
            <rect x="0" y="0" width={width.to_string()} height={height.to_string()} fill="#e9efe6"/>
            {for (0..=cols).map(|c| html!{<line x1={(c as f64 * zone_size).to_string()} y1="0" x2={(c as f64 * zone_size).to_string()} y2={(rows as f64 * zone_size).to_string()} stroke="#9db3a6" stroke-width={stroke.to_string()}/>})}
            {for (0..=rows).map(|r| html!{<line x1="0" y1={(height - r as f64 * zone_size).to_string()} x2={(cols as f64 * zone_size).to_string()} y2={(height - r as f64 * zone_size).to_string()} stroke="#9db3a6" stroke-width={stroke.to_string()}/>})}
            {for zones.iter().map(|(z, _)| {
                let c = &centres[z.as_str()];
                html!{<text x={c[0].as_f64().unwrap_or(0.0).to_string()} y={(height - c[1].as_f64().unwrap_or(0.0)).to_string()} text-anchor="middle" font-size={(zone_size * 0.18).to_string()} fill="#7d9487">{z.clone()}</text>}
            })}
            {for o.pack.edits.places.iter().flatten().map(|p| html!{<circle cx={p.pos[0].to_string()} cy={(height - p.pos[1] as f64).to_string()} r={(zone_size * 0.04).to_string()} fill="#617673"><title>{p.name.clone()}</title></circle>})}
            {for ignitions.iter().enumerate().map(|(i, g)| html!{
                <circle cx={g.at[0].to_string()} cy={(height - g.at[1] as f64).to_string()} r={g.radius_m.max(zone_size as f32 * 0.06).to_string()}
                    fill={if i == sel {"rgba(214,69,20,.65)"} else {"rgba(214,120,20,.4)"}} stroke="#a3312c" stroke-width={(stroke * 2.0).to_string()}/>
            })}
        </svg>
        {for ignitions.iter().enumerate().map(|(i, g)| {
            let g = g.clone();
            let selected = selected.clone();
            let centres = centres.clone();
            html!{<fieldset class={classes!("inject-row", (i == sel).then_some("selected"))} key={i} onfocusin={Callback::from(move |_| selected.set(i))}>
                <legend>{format!("{} {}", ex("Innesco"), i + 1)}</legend>
                <div class="settings-fields">
                    {number("Est x (m)", "", g.at[0].to_string(), upd(open, move |p, v: String| if let (Some(x), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[0] = x }))}
                    {number("Nord y (m)", "", g.at[1].to_string(), upd(open, move |p, v: String| if let (Some(y), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[1] = y }))}
                    {number("Raggio (m)", "", g.radius_m.to_string(), upd(open, move |p, v: String| if let (Some(r), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.radius_m = r }))}
                    {choice("Zona", "", "", &zone_options, upd(open, move |p, zone: String| {
                        let c = &centres[zone.as_str()];
                        if let (Some(x), Some(y), Some(g)) = (c[0].as_f64(), c[1].as_f64(), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at = [x as f32, y as f32]; }
                    }))}
                </div>
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.initial_ignitions.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina innesco")}</button>
            </fieldset>}
        })}
        <button onclick={upd(open, |p, _: MouseEvent| {
            p.edits.initial_ignitions.get_or_insert_with(Vec::new).push(IgnitionSpec { at: [1000.0, 1000.0], radius_m: 100.0 });
        })}>{ex("Aggiungi innesco")}</button>
    </>}
}

fn reinforcements_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let e = &o.pack.edits;
    let list_ = e.reinforcements.clone().unwrap_or_default();
    let resources: Vec<(String, String)> = e.resource_types.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    let roles: Vec<(String, String)> = e.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    html! {<>
        <p class="muted">{ex("Capacità esterna che un ruolo può richiedere: arriva dopo il ritardo indicato.")}</p>
        {for list_.iter().enumerate().map(|(i, r)| {
            let r = r.clone();
            html!{<fieldset class="inject-row" key={i}>
                <legend>{format!("{} → {}", r.resource, r.assigned_to)}</legend>
                <div class="settings-fields">
                    {choice("Risorsa", "", &r.resource, &resources, upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.resource = v }))}
                    {number("Quantità massima", "", r.max_qty.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.max_qty = n }))}
                    {number("Ritardo (min)", "", r.delay_min.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.delay_min = n }))}
                    {field("Provenienza", "", r.source.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.source = v }))}
                    {choice("Riceve il ruolo", "", &r.assigned_to, &roles, upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.assigned_to = v }))}
                </div>
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.reinforcements.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina rinforzo")}</button>
            </fieldset>}
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
    </>}
}

fn places_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    let places = o.pack.edits.places.clone().unwrap_or_default();
    html! {<>
        <p class="muted">{ex("Luoghi con nome: l’AI li usa per capire «la cresta» o «il municipio». Gli alias sono separati da virgola.")}</p>
        {for places.iter().enumerate().map(|(i, pl)| {
            let pl = pl.clone();
            html!{<fieldset class="inject-row" key={i}>
                <legend>{pl.name.clone()}</legend>
                <div class="settings-fields">
                    {field("Nome", "", pl.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {field("Alias", "", pl.aliases.join(", "), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.aliases = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() }))}
                    {number("Est x (m)", "", pl.pos[0].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[0] = n }))}
                    {number("Nord y (m)", "", pl.pos[1].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[1] = n }))}
                </div>
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.places.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina luogo")}</button>
            </fieldset>}
        })}
        <button onclick={upd(open, |p, _: MouseEvent| {
            let l = p.edits.places.get_or_insert_with(Vec::new);
            let n = l.len() + 1;
            l.push(Place { name: format!("Luogo {n}"), aliases: vec![], pos: [1000.0, 1000.0] });
        })}>{ex("Aggiungi luogo")}</button>
    </>}
}

fn injects_tab(o: &Open, open: &UseStateHandle<Option<Open>>, json: &UseStateHandle<String>, message: &UseStateHandle<String>) -> Html {
    let msel = o.pack.edits.msel.clone().unwrap_or_default();
    let roles: Vec<(String, String)> = o.pack.edits.roles.iter().flatten().map(|r| (r.id.clone(), r.name.clone())).collect();
    html! {<>
        <p class="muted">{ex("Inject preparati (MSEL). L’EXCON decide quando inviarli; il turno è solo un suggerimento.")}</p>
        {for msel.iter().enumerate().map(|(i, m)| {
            let m = m.clone();
            let ro = roles.clone();
            html!{<fieldset class="inject-row" key={i}>
                <legend>{format!("{} · {} {}", m.id, ex("turno"), m.turn)}</legend>
                <div class="settings-fields">
                    {field("Identificativo", "", m.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {number("Turno", "", m.turn.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.msel.as_mut().and_then(|l| l.get_mut(i))) { x.turn = n }))}
                    {field("Mittente", "", m.sender.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.sender = v }))}
                    {field("Evento (raggruppa nella timeline)", "", m.event.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.event = v }))}
                </div>
                <div class="settings-fields">
                    {check(ex("tutti").to_string(), "", m.recipients.iter().any(|r| r == "all"), upd(open, move |p, on: bool| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) {
                        x.recipients.retain(|r| r != "all");
                        if on { x.recipients = vec!["all".into()]; }
                    }))}
                    {for ro.iter().map(|(id, name)| {
                        let id2 = id.clone();
                        check(name.clone(), "", m.recipients.contains(id), upd(open, move |p, on: bool| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) {
                            x.recipients.retain(|r| *r != id2 && r != "all");
                            if on { x.recipients.push(id2.clone()); }
                        }))
                    })}
                </div>
                {field("Titolo", "", m.title.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.title = v }))}
                {area("Testo", "", m.text.clone(), "3", upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.text = v }))}
                {field("Obiettivo formativo", "", m.objective.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.objective = v }))}
                <button class="danger" onclick={upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.msel.as_mut() { if i < l.len() { l.remove(i); } } })}>{ex("Elimina inject")}</button>
            </fieldset>}
        })}
        <div class="settings-fields">
            <button onclick={upd(open, |p, _: MouseEvent| {
                let l = p.edits.msel.get_or_insert_with(Vec::new);
                let turn = l.iter().map(|m| m.turn).max().unwrap_or(1);
                let n = l.len() + 1;
                l.push(InjectTemplate {
                    id: format!("I-{n:02}"), turn, title: String::new(), sender: String::new(),
                    recipients: vec!["all".into()], text: String::new(), objective: String::new(), event: String::new(),
                });
            })}>{ex("Aggiungi inject")}</button>
            <button onclick={{
                let (json, msel) = (json.clone(), msel.clone());
                Callback::from(move |_| json.set(serde_json::to_string_pretty(&msel).unwrap_or_default()))
            }}>{ex("Esporta JSON")}</button>
            <button onclick={{
                let (json, message) = (json.clone(), message.clone());
                upd(open, move |p, _: MouseEvent| match serde_json::from_str::<Vec<InjectTemplate>>(&json) {
                    Ok(v) => { p.edits.msel = Some(v); message.set(ex("Inject importati nell’editor · salva per conservarli").to_string()); }
                    Err(e) => message.set(format!("JSON non valido: {e}")),
                })
            }}>{ex("Importa JSON")}</button>
        </div>
        <textarea aria-label={ex("JSON degli inject")} rows="4" value={(**json).clone()} oninput={{
            let json = json.clone();
            Callback::from(move |e: InputEvent| json.set(api::input(e)))
        }}/>
    </>}
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

const TABS: &[(&str, &str)] = &[
    ("general", "Generale"),
    ("roles", "Ruoli"),
    ("resources", "Risorse"),
    ("ignitions", "Inneschi"),
    ("reinforcements", "Rinforzi"),
    ("places", "Luoghi"),
    ("injects", "Inject"),
    ("advanced", "Avanzate"),
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

    // A destructive or session-dropping button asks twice.
    let twice = |key: String, label: String, class: &'static str, go: Callback<()>| {
        if *confirm == key {
            html! {<span class="confirm-delete">
                <button class="danger" disabled={*busy} onclick={Callback::from(move |_| go.emit(()))}>{format!("{}: {label}", ex("Conferma"))}</button>
                <button onclick={{let confirm = confirm.clone(); Callback::from(move |_| confirm.set(String::new()))}}>{ex("Annulla")}</button>
            </span>}
        } else {
            let confirm = confirm.clone();
            html! {<button class={class} disabled={*busy} onclick={Callback::from(move |_| confirm.set(key.clone()))}>{label}</button>}
        }
    };

    html! {<main class="page scenario-editor" id="scenario-editor">
        <section class="card">
            <h2>{ex("Scenari")}</h2>
            <p class="muted">{ex("Uno scenario è un territorio (mappa, edifici, strade) più ciò che lo rende un’esercitazione: ruoli, risorse, inneschi, meteo e inject. «Avvia» inizia una nuova esercitazione e scarta la sessione in corso.")}</p>
            <table class="scenario-list"><thead><tr><th>{ex("Nome")}</th><th>{ex("Territorio")}</th><th></th></tr></thead><tbody>
            {for packs.iter().map(|p| {
                let id = text(&p["id"]).to_string();
                let builtin = p["builtin"] == true;
                let is_running = id == running;
                html!{<tr key={id.clone()} class={classes!(is_running.then_some("running"))} data-scenario={id.clone()}>
                    <td><strong>{text(&p["name"])}</strong>
                        if is_running {<span class="pill">{ex("in corso")}</span>}
                        else if id == active {<span class="pill">{ex("prossimo avvio")}</span>}
                        if builtin {<span class="pill">{ex("incluso")}</span>}
                        <br/><small class="muted">{text(&p["description"])}</small></td>
                    <td>{text(&p["base"])}</td>
                    <td class="row">
                        <button class="edit-scenario" disabled={*busy} onclick={{let load = load.clone(); let id = id.clone(); Callback::from(move |_| load.emit((id.clone(), false)))}}>{if builtin {ex("Personalizza")} else {ex("Modifica")}}</button>
                        if !builtin {<button disabled={*busy} onclick={{let load = load.clone(); let id = id.clone(); Callback::from(move |_| load.emit((id.clone(), true)))}}>{ex("Duplica")}</button>}
                        {twice(format!("start:{id}"), ex("Avvia").to_string(), "primary", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("start", id.clone())))})}
                        if !builtin && !is_running {{twice(format!("delete:{id}"), ex("Elimina").to_string(), "danger", {let act = act.clone(); let id = id.clone(); Callback::from(move |_| act.emit(("/api/master/scenarios/delete", id.clone())))})}}
                    </td>
                </tr>}
            })}
            </tbody></table>
            if open.is_none() && !message.is_empty() {<p role="status">{(*message).clone()}</p>}
        </section>
        if let Some(o) = &*open {
            <section class="card" id="scenario-form">
                <div class="row">
                    <h2>{o.pack.name.clone()}</h2>
                    <span class="pill">{if o.saved_id.is_some() {o.pack.id.clone()} else {ex("non salvato").to_string()}}</span>
                    if o.dirty {<span class="pill pending">{ex("modifiche non salvate")}</span>}
                    <div class="grow"/>
                    <button onclick={{let open = open.clone(); Callback::from(move |_| open.set(None))}}>{ex("Chiudi")}</button>
                </div>
                if o.saved_id.is_none() {
                    <div class="settings-fields">
                        {field("Identificativo (a-z, 0-9, _ -)", "", o.pack.id.clone(), upd(&open, |p, v: String| p.id = v))}
                    </div>
                }
                <nav class="scenario-tabs" aria-label={ex("Sezioni dello scenario")}>
                    {for TABS.iter().map(|(id, label)| {
                        let tab = tab.clone();
                        let id = id.to_string();
                        html!{<button class={classes!((*tab == id).then_some("active"))} data-tab={id.clone()} onclick={Callback::from(move |_| tab.set(id.clone()))}>{ex(label)}</button>}
                    })}
                </nav>
                <div class="scenario-tab">
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
                    <div class="error" role="alert"><strong>{ex("Da correggere prima di salvare:")}</strong>
                        <ul>{for problems.iter().map(|p| html!{<li>{p.clone()}</li>})}</ul></div>
                }
                if !message.is_empty() {<p role="status" id="scenario-message">{(*message).clone()}</p>}
                <div class="row">
                    <button disabled={*busy} onclick={validate}>{ex("Controlla")}</button>
                    <button id="save-scenario" class="primary" disabled={*busy} onclick={{let save = save.clone(); Callback::from(move |_| save.emit(false))}}>{ex("Salva")}</button>
                    {twice("save-start".into(), ex("Salva e avvia").to_string(), "", {let save = save.clone(); Callback::from(move |_| save.emit(true))})}
                    <button onclick={{
                        let (json, pack) = (json.clone(), o.pack.clone());
                        Callback::from(move |_| json.set(serde_json::to_string_pretty(&pack).unwrap_or_default()))
                    }}>{ex("Esporta scenario (JSON)")}</button>
                    <button onclick={{
                        let (json, open, message) = (json.clone(), open.clone(), message.clone());
                        Callback::from(move |_| match serde_json::from_str::<ScenarioPack>(&json) {
                            Ok(pack) => {
                                if let Some(cur) = &*open {
                                    open.set(Some(Open { pack, dirty: true, ..cur.clone() }));
                                    message.set(ex("Scenario importato nell’editor · salva per conservarlo").to_string());
                                }
                            }
                            Err(e) => message.set(format!("JSON non valido: {e}")),
                        })
                    }}>{ex("Importa scenario (JSON)")}</button>
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
