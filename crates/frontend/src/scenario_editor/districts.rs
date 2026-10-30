//! "Borghi" tab: the localities `EvacuateDistrict` evacuates. Membership is a
//! list of building ids, picked by clicking the houses on the map.
use super::*;
use scenario::{District, DistrictEvacuation};

/// One building of the territory as the reference lists it:
/// `[id, x, y, occupants, name, kind, shelter_capacity]`.
#[derive(Clone, PartialEq)]
struct Bld {
    id: u32,
    pos: [f64; 2],
    occupants: u32,
    name: Option<String>,
    shelter: u32,
}

fn buildings(o: &Open) -> Vec<Bld> {
    list(&o.reference["buildings"])
        .iter()
        .filter_map(|b| {
            Some(Bld {
                id: b[0].as_u64()? as u32,
                pos: [b[1].as_f64()?, b[2].as_f64()?],
                occupants: b[3].as_u64().unwrap_or(0) as u32,
                name: b[4].as_str().map(str::to_string),
                shelter: b[6].as_u64().unwrap_or(0) as u32,
            })
        })
        .collect()
}

/// Distinguishable fills for the boroughs, cycled.
const HUES: [&str; 8] = ["#d9480f", "#1c7ed6", "#7048e8", "#c2255c", "#0b7285", "#a8721c", "#5c940d", "#e8590c"];

fn hue(i: usize) -> &'static str {
    HUES[i % HUES.len()]
}

// ------------------------------------------------------------ pure logic

/// What a click did to one house.
#[derive(Debug, PartialEq)]
enum Toggled {
    /// Added to the borgo; `from` is the borgo it was taken from, if any.
    Added { from: Option<usize> },
    Removed,
}

/// Adds or removes `id` in district `i`; a house belongs to one borgo only,
/// so adding it takes it away from any other.
fn toggle(districts: &mut [District], i: usize, id: u32) -> Toggled {
    let had = districts.get(i).is_some_and(|d| d.buildings.contains(&id));
    let from = districts.iter().position(|d| d.buildings.contains(&id));
    for d in districts.iter_mut() {
        d.buildings.retain(|b| *b != id);
    }
    if had {
        return Toggled::Removed;
    }
    if let Some(d) = districts.get_mut(i) {
        d.buildings.push(id);
    }
    Toggled::Added { from }
}

/// Houses added to a borgo by one box selection and how many of them were
/// taken from other boroughs.
#[derive(Debug, PartialEq)]
struct Assigned {
    added: usize,
    stolen: usize,
}

/// Adds `ids` to district `i`. With `only_free`, houses that already belong to
/// another borgo are left where they are.
fn assign(districts: &mut [District], i: usize, ids: &[u32], only_free: bool) -> Assigned {
    let mut out = Assigned { added: 0, stolen: 0 };
    if i >= districts.len() {
        return out;
    }
    for id in ids {
        if districts[i].buildings.contains(id) {
            continue;
        }
        let owner = districts.iter().position(|d| d.buildings.contains(id));
        if owner.is_some() && only_free {
            continue;
        }
        for d in districts.iter_mut() {
            d.buildings.retain(|b| b != id);
        }
        districts[i].buildings.push(*id);
        out.added += 1;
        out.stolen += owner.is_some() as usize;
    }
    out
}

/// Removes `ids` from district `i`; returns how many were in it.
fn release(districts: &mut [District], i: usize, ids: &[u32]) -> usize {
    let Some(d) = districts.get_mut(i) else { return 0 };
    let before = d.buildings.len();
    d.buildings.retain(|b| !ids.contains(b));
    before - d.buildings.len()
}

/// Whether the world point `p` (y up) falls in the SVG rectangle `r`
/// (`[x0, y0, x1, y1]`, y down, normalised) of a map `height` metres tall.
fn in_rect(p: [f64; 2], r: [f64; 4], height: f64) -> bool {
    let y = height - p[1];
    (r[0]..=r[2]).contains(&p[0]) && (r[1]..=r[3]).contains(&y)
}

/// Convex hull of `points` (monotone chain), counter-clockwise.
fn hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = h.len();
        let iter: Vec<[f64; 2]> = if pass == 0 { p.clone() } else { p.iter().rev().cloned().collect() };
        for q in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

/// Visible part of the map: `[x, y, w, h]` in SVG units (y down), always 4:3.
type View = [f64; 4];
const ASPECT: f64 = 4.0 / 3.0;

/// The view that frames `points` (world coordinates) with some margin.
fn fit(points: &[[f64; 2]], width: f64, height: f64) -> View {
    if points.is_empty() {
        let w = width.max(height * ASPECT);
        return [(width - w) / 2.0, (height - w / ASPECT) / 2.0, w, w / ASPECT];
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in points {
        let y = height - p[1];
        (x0, x1, y0, y1) = (x0.min(p[0]), x1.max(p[0]), y0.min(y), y1.max(y));
    }
    let w = ((x1 - x0).max((y1 - y0) * ASPECT) * 1.15).max(80.0);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    [cx - w / 2.0, cy - w / ASPECT / 2.0, w, w / ASPECT]
}

/// Zooms by `factor` (< 1 zooms in) keeping the point at fraction `(fx, fy)`
/// of the view still.
fn zoom_at(v: View, factor: f64, fx: f64, fy: f64, width: f64, height: f64) -> View {
    let max_w = width.max(height * ASPECT) * 1.3;
    let w = (v[2] * factor).clamp(60.0, max_w);
    let h = w / ASPECT;
    [v[0] + fx * v[2] - fx * w, v[1] + fy * v[3] - fy * h, w, h]
}

/// Lower-case identifier (a-z, 0-9, `_`) from a display name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let c = match c {
            'à' | 'á' | 'â' | 'ä' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

/// The identifier after a rename: it follows the name while it is still the
/// automatic one (empty, derived from the old name, or the default `borgo_N`).
fn renamed_id(old_name: &str, old_id: &str, new_name: &str) -> String {
    let generic = old_id.strip_prefix("borgo_").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let new = slug(new_name);
    if (old_id.is_empty() || old_id == slug(old_name) || generic) && !new.is_empty() {
        new
    } else {
        old_id.to_string()
    }
}

/// First `borgo_N` identifier not used yet.
fn next_default(districts: &[District]) -> (String, String) {
    let k = (1..).find(|k| !districts.iter().any(|d| d.id == format!("borgo_{k}"))).unwrap_or(1);
    (format!("borgo_{k}"), format!("Borgo {k}"))
}

// ------------------------------------------------------------ the tab

#[derive(Properties, Clone)]
pub(super) struct DistrictsProps {
    pub o: Open,
    pub open: UseStateHandle<Option<Open>>,
    /// Problems found by the live validation, to show next to the fields.
    pub problems: Vec<String>,
}

impl PartialEq for DistrictsProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

pub(super) fn districts_tab(o: &Open, open: &UseStateHandle<Option<Open>>, problems: &[String]) -> Html {
    html! {<DistrictsTab o={o.clone()} open={open.clone()} problems={problems.to_vec()}/>}
}

/// A drag that started on the map.
#[derive(Clone, Copy)]
struct Drag {
    pan: bool,
    remove: bool,
    moved: bool,
    start_px: [f64; 2],
    start_view: View,
    start_svg: [f64; 2],
}

type History = Rc<std::cell::RefCell<Vec<Vec<District>>>>;

/// Applies `f` to the districts, remembering the previous list for undo, and
/// shows what `f` returns as the status line.
fn commit(open: &UseStateHandle<Option<Open>>, history: &History, note: &UseStateHandle<String>, f: impl FnOnce(&mut Vec<District>) -> String) {
    let Some(cur) = &**open else { return };
    let mut next = cur.clone();
    let l = next.pack.edits.districts.get_or_insert_with(Vec::new);
    {
        let mut h = history.borrow_mut();
        h.push(l.clone());
        if h.len() > 50 {
            h.remove(0);
        }
    }
    let msg = f(l);
    next.dirty = true;
    open.set(Some(next));
    note.set(msg);
}

/// Bounding box of the map element on screen: left, top, width, height.
fn rect(map: &NodeRef) -> Option<(f64, f64, f64, f64)> {
    let r = map.cast::<web_sys::Element>()?.get_bounding_client_rect();
    (r.width() > 0.0 && r.height() > 0.0).then(|| (r.left(), r.top(), r.width(), r.height()))
}

/// SVG point under a screen position and its fraction of the view.
fn svg_at(map: &NodeRef, v: View, cx: f64, cy: f64) -> Option<([f64; 2], [f64; 2])> {
    let (l, t, w, h) = rect(map)?;
    let (fx, fy) = ((cx - l) / w, (cy - t) / h);
    Some(([v[0] + fx * v[2], v[1] + fy * v[3]], [fx, fy]))
}

fn label(b: &Bld) -> String {
    b.name.clone().unwrap_or_else(|| format!("#{}", b.id))
}

#[function_component(DistrictsTab)]
fn districts_view(props: &DistrictsProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected = use_state(|| 0usize);
    let map = use_node_ref();
    let boxref = use_node_ref();
    let view = use_state(|| None::<View>);
    let pan_mode = use_state(|| false);
    let only_free = use_state(|| true);
    let band = use_state(|| None::<[f64; 4]>);
    let hover = use_state_eq(|| None::<(u32, i32, i32)>);
    let note = use_state(String::new);
    let drag = use_mut_ref(|| None::<Drag>);
    let history: History = use_mut_ref(Vec::new);

    let districts = o.pack.edits.districts.clone().unwrap_or_default();
    let pace = o.pack.edits.district_evacuation.unwrap_or_default();
    let all = Rc::new(buildings(o));
    let g = geom(o);
    let n = districts.len();
    let sel = (*selected).min(n.saturating_sub(1));
    let owner = |id: u32| districts.iter().position(|d| d.buildings.contains(&id));
    let people = |d: &District| -> u32 { d.buildings.iter().filter_map(|id| all.iter().find(|b| b.id == *id)).map(|b| b.occupants).sum() };
    let gatherings: Vec<(String, String)> = std::iter::once((String::new(), ex("Area di raccolta più vicina").to_string()))
        .chain(all.iter().filter(|b| b.shelter > 0).filter_map(|b| b.name.clone()).map(|n| (n.clone(), n)))
        .collect();
    let problems_of = |i: usize| -> Vec<String> {
        let prefix = format!("Borgo {}:", i + 1);
        props.problems.iter().filter_map(|p| p.strip_prefix(&prefix)).map(|p| p.trim().to_string()).collect()
    };

    // The framed part of the map: fitted to the houses until the user moves it.
    // (shelters far from every borgo would zoom the map out for nothing)
    let mut points: Vec<[f64; 2]> = all.iter().filter(|b| owner(b.id).is_some()).map(|b| b.pos).collect();
    if points.is_empty() {
        points = all.iter().map(|b| b.pos).collect();
    }
    let vb: View = (*view).unwrap_or_else(|| fit(&points, g.width, g.height));
    let u = vb[2] / 740.0; // one screen pixel, in SVG units

    let undo = {
        let (open, history, note) = (open.clone(), history.clone(), note.clone());
        Callback::from(move |_: ()| {
            let Some(prev) = history.borrow_mut().pop() else { return };
            if let Some(cur) = &*open {
                let mut next = cur.clone();
                next.pack.edits.districts = Some(prev);
                next.dirty = true;
                open.set(Some(next));
                note.set(ex("Ultima modifica annullata.").to_string());
            }
        })
    };

    let items = html! {<>
        <h5 class="md-group" title={ex("Località evacuabili in blocco con «Evacua borgo».")}>{ex("Borghi")}</h5>
        {for districts.iter().enumerate().map(|(i, d)| {
            let bad = d.id.is_empty() || d.buildings.is_empty() || districts.iter().filter(|x| x.id == d.id).count() > 1 || !problems_of(i).is_empty();
            md_item(i == sel && n > 0, { let selected = selected.clone(); Callback::from(move |_| selected.set(i)) }, html!{<>
                <span class="dist-swatch" style={format!("background:{}", hue(i))}/>
                <b class="md-id">{d.id.clone()}</b>
                { if bad { html!{<span class="pill pending" title={problems_of(i).join(" · ")}>{"!"}</span>} } else { Html::default() } }
                {if i < 9 { html!{<kbd class="dist-key" title={ex("Con la mappa a fuoco, questo tasto sceglie il borgo.")}>{i + 1}</kbd>} } else { Html::default() }}
                <span class="md-name">{d.name.clone()}</span>
                <small class="md-sub">{crate::exfmt!("{} case", "{} houses", d.buildings.len())}{" · "}{crate::exfmt!("{} persone", "{} people", people(d))}</small>
            </>})
        })}
        { if n == 0 { html!{<p class="muted">{ex("Nessun borgo definito: il comando «Evacua borgo» non è disponibile.")}</p>} } else { Html::default() } }
        {{
            let free: Vec<&Bld> = all.iter().filter(|b| owner(b.id).is_none()).collect();
            html!{<div class="dist-free" title={ex("Case che nessun borgo evacua: restano grigie sulla mappa.")}>
                <span class="dist-swatch free"/>
                <span>{crate::exfmt!("Senza borgo: {} case · {} persone", "No borgo: {} houses · {} people", free.len(), free.iter().map(|b| b.occupants).sum::<u32>())}</span>
            </div>}
        }}
    </>};
    let tools = html! {
        <button class="primary" onclick={{
            let (selected, add) = (selected.clone(), upd(open, |p, _: MouseEvent| {
                let l = p.edits.districts.get_or_insert_with(Vec::new);
                let (id, name) = next_default(l);
                l.push(District { id, name, buildings: vec![], gathering: None });
            }));
            Callback::from(move |e| { selected.set(n); add.emit(e) })
        }}>{ex("+ Borgo")}</button>
    };

    let set = |f: fn(&mut District, String)| {
        upd(open, move |p, v: String| if let Some(d) = p.edits.districts.as_mut().and_then(|l| l.get_mut(sel)) { f(d, v) })
    };
    let name_cb = upd(open, move |p, v: String| {
        if let Some(d) = p.edits.districts.as_mut().and_then(|l| l.get_mut(sel)) {
            d.id = renamed_id(&d.name, &d.id, &v);
            d.name = v;
        }
    });
    let zones: Vec<String> = list(&o.reference["zones"]).iter().map(|z| text(z).to_string()).collect();

    let fields = match districts.get(sel) {
        None => html! {<p class="muted">{ex("Aggiungi un borgo, poi seleziona sulla mappa le case che ne fanno parte.")}</p>},
        Some(d) => {
            let found = problems_of(sel);
            html! {<div class="rr-body dist-fields" key={sel}>
                <div class="settings-fields">
                    {field("Nome", "Nome del borgo, come lo scrive l’EXCON: «Evacua borgo di …» lo trova per nome o per identificativo. L’identificativo lo segue finché non lo cambi a mano.", d.name.clone(), name_cb)}
                    {field("Identificativo", "Codice breve unico: solo a-z, 0-9 e _.", d.id.clone(), set(|d, v| d.id = v))}
                    {choice("Area di raccolta", "Edificio con capienza dove si radunano gli evacuati quando l’ordine non indica una destinazione. Senza, vale il rifugio aperto o l’uscita dalla mappa più vicini.", d.gathering.as_deref().unwrap_or(""), &with_current(&gatherings, d.gathering.as_deref().unwrap_or("")), set(|d, v| d.gathering = Some(v).filter(|v| !v.is_empty())))}
                </div>
                { if found.is_empty() { Html::default() } else { html!{<ul class="field-errors" role="alert">{for found.iter().map(|p| html!{<li>{p.clone()}</li>})}</ul>} } }
                <p class="muted"><b>{crate::exfmt!("{} case · {} persone", "{} houses · {} people", d.buildings.len(), people(d))}</b></p>
                <div class="row">
                    <button type="button" disabled={d.buildings.is_empty()} onclick={{
                        let (open, history, note) = (open.clone(), history.clone(), note.clone());
                        Callback::from(move |_: MouseEvent| commit(&open, &history, &note, |l| {
                            let c = l.get(sel).map(|d| d.buildings.len()).unwrap_or(0);
                            if let Some(d) = l.get_mut(sel) { d.buildings.clear() }
                            crate::exfmt!("{} case tolte dal borgo.", "{} houses removed from the borgo.", c)
                        }))
                    }}>{ex("Svuota il borgo")}</button>
                </div>
                {confirm_delete("Elimina borgo", "Elimina il borgo: le sue case tornano non assegnate.",
                    {
                        let (open, history, note) = (open.clone(), history.clone(), note.clone());
                        Callback::from(move |_: MouseEvent| commit(&open, &history, &note, |l| {
                            if sel < l.len() { l.remove(sel); }
                            ex("Borgo eliminato.").to_string()
                        }))
                    })}
            </div>}
        }
    };

    // ---- map interaction
    let reach_px = 9.0;
    let nearest = {
        let all = all.clone();
        move |at: [f64; 2], reach: f64| -> Option<Bld> {
            all.iter()
                .map(|b| (((b.pos[0] - at[0]).powi(2) + (g.height - b.pos[1] - at[1]).powi(2)).sqrt(), b))
                .filter(|(d, _)| *d <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, b)| b.clone())
        }
    };
    let onmousedown = {
        let (map, boxref, drag) = (map.clone(), boxref.clone(), drag.clone());
        let pan_mode = *pan_mode;
        Callback::from(move |e: MouseEvent| {
            if e.button() > 1 {
                return;
            }
            e.prevent_default();
            let (cx, cy) = (e.client_x() as f64, e.client_y() as f64);
            let Some((svg, _)) = svg_at(&map, vb, cx, cy) else { return };
            *drag.borrow_mut() = Some(Drag { pan: e.button() == 1 || e.alt_key() || pan_mode, remove: e.shift_key(), moved: false, start_px: [cx, cy], start_view: vb, start_svg: svg });
            if let Some(el) = boxref.cast::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        })
    };
    let onmousemove = {
        let (map, drag, view, band, hover, nearest) = (map.clone(), drag.clone(), view.clone(), band.clone(), hover.clone(), nearest.clone());
        Callback::from(move |e: MouseEvent| {
            let (cx, cy) = (e.client_x() as f64, e.client_y() as f64);
            let current = *drag.borrow();
            if let Some(mut d) = current {
                if !d.moved && ((cx - d.start_px[0]).powi(2) + (cy - d.start_px[1]).powi(2)).sqrt() > 4.0 {
                    d.moved = true;
                    *drag.borrow_mut() = Some(d);
                }
                if d.moved {
                    let Some((_, _, w, h)) = rect(&map) else { return };
                    if d.pan {
                        let v = d.start_view;
                        view.set(Some([v[0] - (cx - d.start_px[0]) / w * v[2], v[1] - (cy - d.start_px[1]) / h * v[3], v[2], v[3]]));
                    } else if let Some((now, _)) = svg_at(&map, vb, cx, cy) {
                        band.set(Some([d.start_svg[0], d.start_svg[1], now[0], now[1]]));
                    }
                }
                return;
            }
            let Some((svg, _)) = svg_at(&map, vb, cx, cy) else { return };
            let Some((_, _, w, _)) = rect(&map) else { return };
            let next = nearest(svg, reach_px / w * vb[2]).map(|b| {
                let px = ((b.pos[0] - vb[0]) / vb[2] * w).round() as i32;
                let py = (((g.height - b.pos[1]) - vb[1]) / vb[3] * (w / ASPECT)).round() as i32;
                (b.id, px, py)
            });
            hover.set(next);
        })
    };
    let onmouseup = {
        let (map, drag, band, open, history, note, all, nearest) = (map.clone(), drag.clone(), band.clone(), open.clone(), history.clone(), note.clone(), all.clone(), nearest.clone());
        let free_only = *only_free;
        let names: Vec<String> = districts.iter().map(|d| d.name.clone()).collect();
        Callback::from(move |e: MouseEvent| {
            let taken = drag.borrow_mut().take();
            band.set(None);
            let Some(d) = taken else { return };
            if d.pan {
                return;
            }
            if n == 0 {
                note.set(ex("Aggiungi prima un borgo con «+ Borgo».").to_string());
                return;
            }
            let target = names[sel].clone();
            if d.moved {
                let Some((now, _)) = svg_at(&map, vb, e.client_x() as f64, e.client_y() as f64) else { return };
                let r = [d.start_svg[0].min(now[0]), d.start_svg[1].min(now[1]), d.start_svg[0].max(now[0]), d.start_svg[1].max(now[1])];
                let ids: Vec<u32> = all.iter().filter(|b| in_rect(b.pos, r, g.height)).map(|b| b.id).collect();
                if d.remove {
                    commit(&open, &history, &note, |l| {
                        let c = release(l, sel, &ids);
                        crate::exfmt!("{} case tolte da «{}».", "{} houses removed from «{}».", c, target)
                    });
                } else {
                    commit(&open, &history, &note, |l| {
                        let a = assign(l, sel, &ids, free_only);
                        match (a.added, a.stolen) {
                            (0, _) => ex("Nessuna casa nuova nel rettangolo.").to_string(),
                            (c, 0) => crate::exfmt!("{} case aggiunte a «{}».", "{} houses added to «{}».", c, target),
                            (c, s) => crate::exfmt!("{} case aggiunte a «{}», di cui {} prese ad altri borghi.", "{} houses added to «{}», {} of them taken from other boroughs.", c, target, s),
                        }
                    });
                }
            } else if let Some(b) = nearest(d.start_svg, reach_px / rect(&map).map(|r| r.2).unwrap_or(740.0) * vb[2]) {
                let who = names.clone();
                commit(&open, &history, &note, |l| match toggle(l, sel, b.id) {
                    Toggled::Removed => crate::exfmt!("Casa {} tolta da «{}».", "House {} removed from «{}».", label(&b), target),
                    Toggled::Added { from: None } => crate::exfmt!("Casa {} aggiunta a «{}».", "House {} added to «{}».", label(&b), target),
                    Toggled::Added { from: Some(j) } => crate::exfmt!("Casa {} spostata da «{}» a «{}».", "House {} moved from «{}» to «{}».", label(&b), who.get(j).cloned().unwrap_or_default(), target),
                });
            }
        })
    };
    let onmouseleave = {
        let (drag, band, hover) = (drag.clone(), band.clone(), hover.clone());
        Callback::from(move |_: MouseEvent| {
            *drag.borrow_mut() = None;
            band.set(None);
            hover.set(None);
        })
    };
    let zoom = {
        let view = view.clone();
        move |factor: f64, fx: f64, fy: f64| view.set(Some(zoom_at(vb, factor, fx, fy, g.width, g.height)))
    };
    let onwheel = {
        let (map, zoom) = (map.clone(), zoom.clone());
        Callback::from(move |e: WheelEvent| {
            e.prevent_default();
            if let Some((_, [fx, fy])) = svg_at(&map, vb, e.client_x() as f64, e.client_y() as f64) {
                zoom(if e.delta_y() < 0.0 { 0.8 } else { 1.25 }, fx, fy);
            }
        })
    };
    let onkeydown = {
        let (zoom, view, drag, band, selected, undo) = (zoom.clone(), view.clone(), drag.clone(), band.clone(), selected.clone(), undo.clone());
        Callback::from(move |e: KeyboardEvent| {
            let k = e.key();
            let step = vb[2] * 0.15;
            match k.as_str() {
                "z" | "Z" if e.ctrl_key() || e.meta_key() => undo.emit(()),
                "+" | "=" => zoom(0.8, 0.5, 0.5),
                "-" | "_" => zoom(1.25, 0.5, 0.5),
                "0" => view.set(None),
                "ArrowLeft" => view.set(Some([vb[0] - step, vb[1], vb[2], vb[3]])),
                "ArrowRight" => view.set(Some([vb[0] + step, vb[1], vb[2], vb[3]])),
                "ArrowUp" => view.set(Some([vb[0], vb[1] - step, vb[2], vb[3]])),
                "ArrowDown" => view.set(Some([vb[0], vb[1] + step, vb[2], vb[3]])),
                "Escape" => {
                    *drag.borrow_mut() = None;
                    band.set(None);
                }
                d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => {
                    let i = (d.as_bytes()[0] - b'1') as usize;
                    if i < n {
                        selected.set(i);
                    }
                }
                _ => return,
            }
            e.prevent_default();
        })
    };

    // ---- drawing
    let mut order: Vec<&Bld> = all.iter().collect();
    order.sort_by_key(|b| match owner(b.id) {
        None => 0,
        Some(i) if i == sel => 2,
        Some(_) => 1,
    });
    let dots = html! {<>{for order.iter().map(|b| {
        let own = owner(b.id);
        let (fill, r) = match own {
            Some(i) if i == sel => (hue(i), 4.6 * u),
            Some(i) => (hue(i), 3.2 * u),
            None => ("#8a948c", 2.6 * u),
        };
        let (cx, cy) = (b.pos[0], g.height - b.pos[1]);
        let hot = hover.is_some_and(|h| h.0 == b.id);
        let stroke = if hot { "#000" } else if b.shelter > 0 { "#0f6b4d" } else { "#222" };
        let sw = if hot { 2.0 * u } else if b.shelter > 0 { 1.6 * u } else { 0.6 * u };
        let o = if own.is_none() { "0.75" } else { "1" };
        if b.name.is_some() {
            let s = (r * 2.0).max(6.0 * u);
            html!{<rect x={(cx - s / 2.0).to_string()} y={(cy - s / 2.0).to_string()} width={s.to_string()} height={s.to_string()} fill={fill} stroke={stroke} stroke-width={sw.to_string()} opacity={o}/>}
        } else {
            html!{<circle cx={cx.to_string()} cy={cy.to_string()} r={r.to_string()} fill={fill} stroke={if hot { "#000" } else { "none" }} stroke-width={sw.to_string()} opacity={o}/>}
        }
    })}</>};
    let areas = html! {<>{for districts.iter().enumerate().filter_map(|(i, d)| {
        let pts: Vec<[f64; 2]> = d.buildings.iter().filter_map(|id| all.iter().find(|b| b.id == *id)).map(|b| [b.pos[0], g.height - b.pos[1]]).collect();
        if pts.is_empty() {
            return None;
        }
        let h = hull(&pts);
        let on = i == sel;
        let c = (h.iter().map(|p| p[0]).sum::<f64>() / h.len() as f64, h.iter().map(|p| p[1]).sum::<f64>() / h.len() as f64);
        let poly = h.iter().map(|p| format!("{:.0},{:.0}", p[0], p[1])).collect::<Vec<_>>().join(" ");
        Some(html!{<g class="dist-area" pointer-events="none">
            <polygon points={poly} fill={hue(i)} fill-opacity={if on { "0.16" } else { "0.07" }} stroke={hue(i)} stroke-opacity={if on { "0.55" } else { "0.28" }} stroke-width={(if on { 10.0 } else { 7.0 } * u).to_string()} stroke-linejoin="round"/>
            <text class="map-lbl" x={c.0.to_string()} y={(c.1 - 9.0 * u).to_string()} text-anchor="middle" font-size={(if on { 13.0 } else { 11.0 } * u).to_string()} style={format!("fill:{}", hue(i))}>{d.name.clone()}</text>
        </g>})
    })}</>};
    let gather = districts.get(sel).and_then(|d| d.gathering.as_deref()).and_then(|name| all.iter().find(|b| b.name.as_deref() == Some(name))).map(|b| {
        let (cx, cy) = (b.pos[0], g.height - b.pos[1]);
        html! {<g pointer-events="none">
            <circle cx={cx.to_string()} cy={cy.to_string()} r={(11.0 * u).to_string()} fill="none" stroke="#0f6b4d" stroke-width={(2.5 * u).to_string()} stroke-dasharray={format!("{} {}", 4.0 * u, 3.0 * u)}/>
            <text class="map-lbl" x={cx.to_string()} y={(cy + 22.0 * u).to_string()} text-anchor="middle" font-size={(11.0 * u).to_string()}>{format!("{}: {}", ex("Raccolta"), b.name.clone().unwrap_or_default())}</text>
        </g>}
    });
    let band_rect = band.map(|r| {
        let (x, y) = (r[0].min(r[2]), r[1].min(r[3]));
        html! {<rect class="dist-band" x={x.to_string()} y={y.to_string()} width={(r[0] - r[2]).abs().to_string()} height={(r[1] - r[3]).abs().to_string()} stroke-width={u.to_string()} stroke-dasharray={format!("{} {}", 4.0 * u, 3.0 * u)}/>}
    });
    let tip = hover.and_then(|(id, px, py)| all.iter().find(|b| b.id == id).map(|b| {
        let who = owner(b.id).and_then(|i| districts.get(i)).map(|d| d.name.clone());
        html! {<div class="dist-tip" style={format!("left:{px}px;top:{py}px")}>
            <b>{b.name.clone().unwrap_or_else(|| format!("{} {}", ex("Casa"), b.id))}</b>
            {" · "}{crate::exfmt!("{} persone", "{} people", b.occupants)}
            {if b.shelter > 0 { format!(" · {} {}", ex("capienza"), b.shelter) } else { String::new() }}
            {" · "}{who.unwrap_or_else(|| ex("senza borgo").to_string())}
        </div>}
    }));

    let zone_pick = {
        let (all, open, history, note, free_only) = (all.clone(), open.clone(), history.clone(), note.clone(), *only_free);
        let (centres, name) = (o.reference["zone_centres"].clone(), districts.get(sel).map(|d| d.name.clone()).unwrap_or_default());
        Callback::from(move |e: Event| {
            let el = e.target_unchecked_into::<web_sys::HtmlSelectElement>();
            let z = el.value();
            el.set_value("");
            let Some(c) = centres.get(&z) else { return };
            let (cx, cy) = (c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0));
            let ids: Vec<u32> = all.iter().filter(|b| (b.pos[0] - cx).abs() <= g.zone / 2.0 && (b.pos[1] - cy).abs() <= g.zone / 2.0).map(|b| b.id).collect();
            if n == 0 {
                return;
            }
            commit(&open, &history, &note, |l| {
                let a = assign(l, sel, &ids, free_only);
                crate::exfmt!("Zona {}: {} case aggiunte a «{}».", "Zone {}: {} houses added to «{}».", z, a.added, name)
            });
        })
    };

    let btn = |label: &'static str, tip: &'static str, on: bool, cb: Callback<MouseEvent>| {
        html! {<button type="button" class={classes!("dist-tool", on.then_some("on"))} aria-pressed={on.to_string()} title={ex(tip)} onclick={cb}>{ex(label)}</button>}
    };
    let nav = |label: &'static str, tip: &'static str, f: f64| {
        let zoom = zoom.clone();
        html! {<button type="button" class="dist-tool" title={ex(tip)} aria-label={ex(tip)} onclick={Callback::from(move |_| zoom(f, 0.5, 0.5))}>{label}</button>}
    };

    html! {<div class="rr-tab dist-tab">
        <p class="muted">{ex("I borghi sono le località che l’EXCON può evacuare con un solo ordine, case sparse comprese. Le case si scelgono sulla mappa.")}</p>
        <div class="dist-pace">
            <label title={ex("Quante persone del borgo partono ogni minuto, dalle case più vicine alla strada.")}>{ex("Persone al minuto (partenze)")}
                <input type="number" step="any" min="0" value={fmt(pace.per_min as f64)} oninput={upd(open, move |p, v: String| if let Some(x) = num::<f32>(&v) { let e = p.edits.district_evacuation.get_or_insert_with(DistrictEvacuation::default); e.per_min = x }).reform(api::input)}/></label>
            <label title={ex("Minuti tra l’ordine e le prime partenze (massimo 240).")}>{ex("Preparazione (min)")}
                <input type="number" step="1" min="0" max="240" value={pace.prep_min.to_string()} oninput={upd(open, move |p, v: String| if let Some(x) = num::<u32>(&v) { let e = p.edits.district_evacuation.get_or_insert_with(DistrictEvacuation::default); e.prep_min = x }).reform(api::input)}/></label>
            { if let Some(p) = props.problems.iter().find(|p| p.starts_with("Ritmo di evacuazione")) { html!{<span class="error field-error">{p.clone()}</span>} } else { Html::default() } }
        </div>
        {master_detail(items, tools, html! {<div class="dist-work">
            <div class="dist-side">{fields}
                <small class="muted dist-legend">
                    <span class="lg own"/>{ex("casa del borgo scelto")}<br/><span class="lg other"/>{ex("di un altro borgo")}<br/><span class="lg free"/>{ex("senza borgo")}<br/><span class="lg sq"/>{ex("edificio con nome")}<br/><span class="lg ring"/>{ex("area di raccolta")}
                </small>
            </div>
            <div class="dist-mapcol">
                <div class="dist-bar" role="toolbar" aria-label={ex("Strumenti della mappa")}>
                    {btn("Scegli", "Clic su una casa la aggiunge o la toglie; trascina per selezionare un rettangolo (Maiusc: toglie).", !*pan_mode, { let p = pan_mode.clone(); Callback::from(move |_| p.set(false)) })}
                    {btn("Sposta", "Trascina per spostare la mappa (anche con Alt o il tasto centrale).", *pan_mode, { let p = pan_mode.clone(); Callback::from(move |_| p.set(true)) })}
                    {nav("+", "Ingrandisci (rotella o +)", 0.8)}
                    {nav("−", "Riduci (rotella o −)", 1.25)}
                    <button type="button" class="dist-tool" title={ex("Inquadra tutte le case (tasto 0).")} onclick={{ let v = view.clone(); Callback::from(move |_| v.set(None)) }}>{ex("Adatta")}</button>
                    <label class="check dist-free-only" title={ex("Se attivo, la selezione a rettangolo e per zona non toglie case agli altri borghi.")}>
                        <input type="checkbox" checked={*only_free} onchange={{ let f = only_free.clone(); Callback::from(move |e: Event| f.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())) }}/>{ex("Solo case senza borgo")}</label>
                    <select class="dist-zone" aria-label={ex("Aggiungi le case di una zona")} title={ex("Aggiunge al borgo scelto le case di una zona della griglia.")} onchange={zone_pick}>
                        <option value="" selected=true>{ex("Aggiungi zona…")}</option>
                        {for zones.iter().map(|z| html!{<option value={z.clone()}>{z.clone()}</option>})}
                    </select>
                    <button type="button" class="dist-tool" disabled={history.borrow().is_empty()} title={ex("Annulla l’ultima modifica delle case (Ctrl+Z con la mappa a fuoco).")} onclick={{ let u = undo.clone(); Callback::from(move |_| u.emit(())) }}>{ex("↶ Annulla")}</button>
                </div>
                <div class="dist-mapbox" tabindex="0" ref={boxref} {onkeydown} aria-label={ex("Mappa dei borghi: frecce spostano, + e − ingrandiscono, 0 inquadra, numeri scelgono il borgo")}>
                    <svg ref={map} class={classes!("ignition-map", "dist-map", pan_mode.then_some("pan"))} viewBox={format!("{} {} {} {}", vb[0], vb[1], vb[2], vb[3])} role="img" aria-label={ex("Mappa delle case dei borghi")} {onmousedown} {onmousemove} {onmouseup} {onmouseleave} {onwheel}>
                        {map_base(o, g)}
                        {areas}
                        {dots}
                        {gather}
                        {band_rect}
                    </svg>
                    <div class="dist-tipslot">{tip}</div>
                    <p class="dist-note" role="status" aria-live="polite" hidden={note.is_empty()}>{(*note).clone()}</p>
                </div>
            </div>
        </div>})}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, b: &[u32]) -> District {
        District { id: id.into(), name: id.into(), buildings: b.to_vec(), gathering: None }
    }

    #[test]
    fn a_house_moves_between_boroughs_and_toggles_off() {
        let mut l = vec![d("a", &[1, 2]), d("b", &[3])];
        assert_eq!(toggle(&mut l, 1, 2), Toggled::Added { from: Some(0) });
        assert_eq!((l[0].buildings.clone(), l[1].buildings.clone()), (vec![1], vec![3, 2]));
        assert_eq!(toggle(&mut l, 1, 2), Toggled::Removed);
        assert_eq!(l[1].buildings, vec![3]);
        assert_eq!(toggle(&mut l, 0, 9), Toggled::Added { from: None });
    }

    #[test]
    fn box_selection_can_leave_other_boroughs_alone() {
        let mut l = vec![d("a", &[1]), d("b", &[2, 3])];
        assert_eq!(assign(&mut l, 0, &[1, 2, 4], true), Assigned { added: 1, stolen: 0 });
        assert_eq!(l[0].buildings, vec![1, 4]);
        assert_eq!(l[1].buildings, vec![2, 3]);
        assert_eq!(assign(&mut l, 0, &[2, 3], false), Assigned { added: 2, stolen: 2 });
        assert!(l[1].buildings.is_empty());
        assert_eq!(release(&mut l, 0, &[1, 2, 99]), 2);
        assert_eq!(l[0].buildings, vec![4, 3]);
        assert_eq!(assign(&mut l, 5, &[1], false), Assigned { added: 0, stolen: 0 });
    }

    #[test]
    fn rectangles_use_the_flipped_map_axis() {
        // A house 100 m north of the south edge of a 1000 m map is at svg y 900.
        assert!(in_rect([50.0, 100.0], [0.0, 800.0, 100.0, 1000.0], 1000.0));
        assert!(!in_rect([50.0, 100.0], [0.0, 0.0, 100.0, 200.0], 1000.0));
    }

    #[test]
    fn hull_keeps_only_the_outline() {
        let h = hull(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [5.0, 5.0], [5.0, 0.0]]);
        assert_eq!(h.len(), 4);
        assert!(!h.contains(&[5.0, 5.0]));
        assert_eq!(hull(&[[1.0, 1.0], [1.0, 1.0]]).len(), 1);
    }

    #[test]
    fn view_fits_and_zooms_around_the_cursor() {
        let v = fit(&[[100.0, 100.0], [300.0, 200.0]], 1000.0, 1000.0);
        assert!((v[2] / v[3] - ASPECT).abs() < 1e-9);
        assert!(v[2] >= 200.0 * 1.15 && v[0] < 100.0 && v[0] + v[2] > 300.0);
        // The point under the cursor does not move when zooming.
        let z = zoom_at(v, 0.5, 0.25, 0.75, 1000.0, 1000.0);
        let before = (v[0] + 0.25 * v[2], v[1] + 0.75 * v[3]);
        let after = (z[0] + 0.25 * z[2], z[1] + 0.75 * z[3]);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
        assert!(zoom_at(v, 1e-6, 0.5, 0.5, 1000.0, 1000.0)[2] >= 60.0);
        assert!(fit(&[], 1000.0, 1000.0)[2] >= 1000.0);
    }

    #[test]
    fn identifier_follows_the_name_until_edited_by_hand() {
        assert_eq!(slug("Pian dei Grilli"), "pian_dei_grilli");
        assert_eq!(slug("  Città  nuova! "), "citta_nuova");
        assert_eq!(renamed_id("Borgo 1", "borgo_1", "Le Terrazze"), "le_terrazze");
        assert_eq!(renamed_id("Le Terrazze", "le_terrazze", "Le Terrazze Alte"), "le_terrazze_alte");
        assert_eq!(renamed_id("Le Terrazze", "terrazze", "Altro"), "terrazze");
        assert_eq!(renamed_id("A", "a", ""), "a", "an empty name keeps the identifier");
        assert_eq!(renamed_id("", "", "Fornaci"), "fornaci");
    }

    #[test]
    fn default_names_skip_the_ones_in_use() {
        assert_eq!(next_default(&[d("borgo_1", &[]), d("borgo_3", &[])]).0, "borgo_2");
        assert_eq!(next_default(&[]).1, "Borgo 1");
    }
}
