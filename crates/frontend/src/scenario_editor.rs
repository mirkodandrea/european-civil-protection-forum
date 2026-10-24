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

fn field(label: &'static str, value: String, cb: Callback<String>) -> Html {
    html! {<label>{ex(label)}<input value={value} oninput={cb.reform(api::input)}/></label>}
}

fn number(label: &'static str, value: String, cb: Callback<String>) -> Html {
    html! {<label>{ex(label)}<input type="number" step="any" value={value} oninput={cb.reform(api::input)}/></label>}
}

fn area(label: &'static str, value: String, rows: &'static str, cb: Callback<String>) -> Html {
    html! {<label class="wide">{ex(label)}<textarea rows={rows} value={value} oninput={cb.reform(api::input)}/></label>}
}

fn check(label: String, on: bool, cb: Callback<bool>) -> Html {
    html! {<label class="check"><input type="checkbox" checked={on} onchange={cb.reform(|e: Event| {
        use yew::TargetCast;
        e.target_unchecked_into::<web_sys::HtmlInputElement>().checked()
    })}/>{label}</label>}
}

fn choice(label: &'static str, value: &str, options: &[(String, String)], cb: Callback<String>) -> Html {
    html! {<label>{ex(label)}<select onchange={cb.reform(api::select)}>
        {for options.iter().map(|(v, name)| html!{<option value={v.clone()} selected={v == value}>{name.clone()}</option>})}
    </select></label>}
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
    html! {<div class="settings-fields">
        {field("Nome", o.pack.name.clone(), upd(open, |p, v: String| p.name = v))}
        {area("Descrizione", o.pack.description.clone(), "3", upd(open, |p, v: String| p.description = v))}
        <label>{ex("Territorio")}<input value={o.pack.base.clone()} disabled=true/></label>
        {field("Ora di inizio (HH:MM)", clock_text(start), upd(open, |p, v: String| {
            let day = p.edits.start_clock_s.unwrap_or(0) / 86_400 + 1;
            if let Some(t) = parse_clock(day, &v) { p.edits.start_clock_s = Some(t); }
        }))}
        {number("Giorno", (start / 86_400 + 1).to_string(), day_cb)}
        {choice("Base di partenza delle risorse", e.staging_base.as_deref().unwrap_or(""), &places, upd(open, |p, v: String| p.edits.staging_base = Some(v)))}
        {number("Vento da (gradi)", fmt(weather.wind_dir_deg), upd(open, |p, v: String| if let Some(x) = num(&v) { if let Some(w) = p.edits.weather.as_mut() { w.wind_dir_deg = x } }))}
        {number("Vento (km/h)", fmt(weather.wind_speed_kmh), upd(open, |p, v: String| if let Some(x) = num(&v) { if let Some(w) = p.edits.weather.as_mut() { w.wind_speed_kmh = x } }))}
        {number("Umidità del combustibile (%)", fmt(weather.moisture_pct), upd(open, |p, v: String| if let Some(x) = num(&v) { if let Some(w) = p.edits.weather.as_mut() { w.moisture_pct = x } }))}
    </div>}
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
                    {field("Identificativo", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {choice("Livello", &r.level, &levels, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.level = v }))}
                    {choice("Lingua", &r.language, &languages, upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.language = v }))}
                </div>
                {area("Descrizione", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.roles.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
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
                        check(a.clone(), r.allowed_actions.contains(a), upd(open, move |p, on: bool| {
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
                    {field("Identificativo", r.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {field("Nome", r.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {number("Velocità di spostamento (m/s, vuoto = predefinita)", r.speed_mps.map(|s| s.to_string()).unwrap_or_default(), upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.speed_mps = num(&v) }))}
                </div>
                {area("Descrizione", r.description.clone(), "2", upd(open, move |p, v: String| if let Some(x) = p.edits.resource_types.as_mut().and_then(|l| l.get_mut(i)) { x.description = v }))}
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
                    {number("Est x (m)", g.at[0].to_string(), upd(open, move |p, v: String| if let (Some(x), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[0] = x }))}
                    {number("Nord y (m)", g.at[1].to_string(), upd(open, move |p, v: String| if let (Some(y), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.at[1] = y }))}
                    {number("Raggio (m)", g.radius_m.to_string(), upd(open, move |p, v: String| if let (Some(r), Some(g)) = (num(&v), p.edits.initial_ignitions.as_mut().and_then(|l| l.get_mut(i))) { g.radius_m = r }))}
                    {choice("Zona", "", &zone_options, upd(open, move |p, zone: String| {
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
                    {choice("Risorsa", &r.resource, &resources, upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.resource = v }))}
                    {number("Quantità massima", r.max_qty.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.max_qty = n }))}
                    {number("Ritardo (min)", r.delay_min.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i))) { x.delay_min = n }))}
                    {field("Provenienza", r.source.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.source = v }))}
                    {choice("Riceve il ruolo", &r.assigned_to, &roles, upd(open, move |p, v: String| if let Some(x) = p.edits.reinforcements.as_mut().and_then(|l| l.get_mut(i)) { x.assigned_to = v }))}
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
                    {field("Nome", pl.name.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.name = v }))}
                    {field("Alias", pl.aliases.join(", "), upd(open, move |p, v: String| if let Some(x) = p.edits.places.as_mut().and_then(|l| l.get_mut(i)) { x.aliases = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() }))}
                    {number("Est x (m)", pl.pos[0].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[0] = n }))}
                    {number("Nord y (m)", pl.pos[1].to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.places.as_mut().and_then(|l| l.get_mut(i))) { x.pos[1] = n }))}
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
                    {field("Identificativo", m.id.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.id = v }))}
                    {number("Turno", m.turn.to_string(), upd(open, move |p, v: String| if let (Some(n), Some(x)) = (num(&v), p.edits.msel.as_mut().and_then(|l| l.get_mut(i))) { x.turn = n }))}
                    {field("Mittente", m.sender.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.sender = v }))}
                    {field("Evento (raggruppa nella timeline)", m.event.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.event = v }))}
                </div>
                <div class="settings-fields">
                    {check(ex("tutti").to_string(), m.recipients.iter().any(|r| r == "all"), upd(open, move |p, on: bool| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) {
                        x.recipients.retain(|r| r != "all");
                        if on { x.recipients = vec!["all".into()]; }
                    }))}
                    {for ro.iter().map(|(id, name)| {
                        let id2 = id.clone();
                        check(name.clone(), m.recipients.contains(id), upd(open, move |p, on: bool| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) {
                            x.recipients.retain(|r| *r != id2 && r != "all");
                            if on { x.recipients.push(id2.clone()); }
                        }))
                    })}
                </div>
                {field("Titolo", m.title.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.title = v }))}
                {area("Testo", m.text.clone(), "3", upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.text = v }))}
                {field("Obiettivo formativo", m.objective.clone(), upd(open, move |p, v: String| if let Some(x) = p.edits.msel.as_mut().and_then(|l| l.get_mut(i)) { x.objective = v }))}
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
    type Flag = (&'static str, fn(&scenario::Operations) -> bool, fn(&mut scenario::Operations, bool));
    let flags: [Flag; 10] = [
        ("Percorsi stradali", |o| o.road_routing, |o, v| o.road_routing = v),
        ("Soppressione progressiva", |o| o.progressive_suppression, |o, v| o.progressive_suppression = v),
        ("Traffico aggregato", |o| o.traffic, |o, v| o.traffic = v),
        ("Esposizione all’intensità", |o| o.intensity_exposure, |o, v| o.intensity_exposure = v),
        ("Comunicazioni", |o| o.communications, |o, v| o.communications = v),
        ("Spotting (brace a distanza)", |o| o.spotting, |o, v| o.spotting = v),
        ("Chiusure di tratto per fuoco", |o| o.segment_fire_closures, |o, v| o.segment_fire_closures = v),
        ("Spostamenti fuori strada", |o| o.terrain_travel, |o, v| o.terrain_travel = v),
        ("Coda dei mezzi di emergenza", |o| o.emergency_congestion, |o, v| o.emergency_congestion = v),
        ("Autonomia locale delle squadre", |o| o.local_autonomy, |o, v| o.local_autonomy = v),
    ];
    let run = &o.pack.run;
    html! {<>
        <h4>{ex("Modello operativo")}</h4>
        <div class="settings-fields">
            {for flags.iter().map(|(label, get, set)| {
                let set = *set;
                check(ex(label).to_string(), get(&ops), upd(open, move |p, on: bool| set(p.edits.operations.get_or_insert_with(Default::default), on)))
            })}
            {check(ex("Avvia con il modello operativo attivo").to_string(), run.operational_model == Some(true), upd(open, |p, on: bool| p.run.operational_model = on.then_some(true)))}
        </div>
        <h4>{ex("Politica tattica (distanze in metri, tempi in secondi simulati)")}</h4>
        <div class="settings-fields">
            {number("Raggio del settore", policy.sector_radius_m.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).sector_radius_m = n }))}
            {number("Portata della manichetta", policy.hose_reach_m.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).hose_reach_m = n }))}
            {number("Limite di bagnatura (punti)", policy.wet_limit_pts.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).wet_limit_pts = n }))}
            {number("Rivalutazione (s)", policy.reassess_s.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).reassess_s = n }))}
            {number("Pericolo per sosta (0–1)", policy.parking_danger.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).parking_danger = n }))}
            {number("Pericolo per lavoro (0–1)", policy.work_danger.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).work_danger = n }))}
            {number("Riorganizzazione (s)", policy.regroup_s.to_string(), upd(open, |p, v: String| if let Some(n) = num(&v) { p.edits.tactical_policy.get_or_insert_with(Default::default).regroup_s = n }))}
            {check(ex("Lavoro condiviso tra squadre").to_string(), policy.share_work, upd(open, |p, on: bool| p.edits.tactical_policy.get_or_insert_with(Default::default).share_work = on))}
            {check(ex("Riprende dopo il ritiro").to_string(), policy.resume_after_withdrawal, upd(open, |p, on: bool| p.edits.tactical_policy.get_or_insert_with(Default::default).resume_after_withdrawal = on))}
        </div>
        <h4>{ex("Simulazione")}</h4>
        <p class="muted">{ex("Vuoto = valore predefinito. Le opzioni sulla riga di comando hanno la precedenza.")}</p>
        <div class="settings-fields">
            {number("Seme casuale", run.seed.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.seed = num(&v)))}
            {number("Realizzazioni dell’ensemble", run.realizations.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.realizations = num(&v)))}
            {number("Soglia del fuoco «reale» (0–1)", run.cutoff.map(|s| s.to_string()).unwrap_or_default(), upd(open, |p, v: String| p.run.cutoff = num(&v)))}
        </div>
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
                        {field("Identificativo (a-z, 0-9, _ -)", o.pack.id.clone(), upd(&open, |p, v: String| p.id = v))}
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
