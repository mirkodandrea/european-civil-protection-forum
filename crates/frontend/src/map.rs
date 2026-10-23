//! Interactive map: every dynamic layer comes from an authorized server view.
//! Zoom/pan, inspection and layer toggles are presentation only; they never change the world.
use crate::api::{list, text, window};
use base64::Engine;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{closure::Closure, Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, PointerEvent, WheelEvent};
use yew::prelude::*;

const SIZE: f64 = 1200.;
const MAX_ZOOM: f64 = 16.;
/// Layer bits; a set bit in `hidden` switches the layer off.
const L_FIRE: u8 = 1;
const L_ROADS: u8 = 2;
const L_BUILDINGS: u8 = 4;
const L_LABELS: u8 = 8;
const L_GRID: u8 = 16;
const L_FORECAST: u8 = 32;
const L_UNITS: u8 = 64;
const L_FOG: u8 = 128;
const LAYERS: [(u8, &str); 8] = [
    (L_FIRE, "l_fire"),
    (L_ROADS, "l_roads"),
    (L_BUILDINGS, "l_buildings"),
    (L_LABELS, "l_labels"),
    (L_GRID, "l_grid"),
    (L_FORECAST, "l_forecast"),
    (L_UNITS, "l_units"),
    (L_FOG, "l_fog"),
];

/// Visible window in bitmap pixels: top-left `(ox, oy)`, width `SIZE / z`.
#[derive(Clone, Copy, PartialEq)]
pub struct View {
    z: f64,
    ox: f64,
    oy: f64,
}
impl View {
    const HOME: View = View {
        z: 1.,
        ox: 0.,
        oy: 0.,
    };
    fn clamp(&mut self, bh: f64) {
        self.z = self.z.clamp(1., MAX_ZOOM);
        self.ox = self.ox.clamp(0., SIZE - SIZE / self.z);
        self.oy = self.oy.clamp(0., bh - bh / self.z);
    }
    /// Zoom by `f` keeping the bitmap point under the fractional cursor `(fx, fy)` fixed.
    fn zoom_at(&mut self, f: f64, fx: f64, fy: f64, bh: f64) {
        let px = self.ox + fx * SIZE / self.z;
        let py = self.oy + fy * bh / self.z;
        self.z = (self.z * f).clamp(1., MAX_ZOOM);
        self.ox = px - fx * SIZE / self.z;
        self.oy = py - fy * bh / self.z;
        self.clamp(bh);
    }
    fn pan(&mut self, dfx: f64, dfy: f64, bh: f64) {
        self.ox -= dfx * SIZE / self.z;
        self.oy -= dfy * bh / self.z;
        self.clamp(bh);
    }
}
#[derive(Default)]
struct Drag {
    pts: Vec<(i32, f64, f64)>,
    moved: f64,
}
#[derive(Clone, PartialEq)]
struct Tick(u32);
impl Reducible for Tick {
    type Action = ();
    fn reduce(self: Rc<Self>, _: ()) -> Rc<Self> {
        Rc::new(Tick(self.0 + 1))
    }
}
/// `(width_m, height_m, bitmap px per metre, bitmap height)`.
fn geom(stat: &Value) -> Option<(f64, f64, f64, f64)> {
    let w = n(&stat["grid"]["cols"]) * n(&stat["grid"]["cellsize"]);
    let h = n(&stat["grid"]["rows"]) * n(&stat["grid"]["cellsize"]);
    (w > 0. && h > 0.).then(|| (w, h, SIZE / w, h * SIZE / w))
}

#[derive(Properties, PartialEq)]
pub struct MapProps {
    pub stat: Value,
    pub view: Value,
    #[prop_or_default]
    pub simple: bool,
    #[prop_or_default]
    pub onclick: Callback<Value>,
    #[prop_or_default]
    pub pending: Option<Value>,
    #[prop_or("map".into())]
    pub id: AttrValue,
    #[prop_or("it".into())]
    pub lang: AttrValue,
    /// Wheel, drag, pinch, double-click and keyboard navigation.
    #[prop_or(true)]
    pub pan_zoom: bool,
    /// Hover/tap inspection panel.
    #[prop_or(true)]
    pub inspect: bool,
    /// Zoom/fit/fullscreen buttons and the layer menu.
    #[prop_or(true)]
    pub tools: bool,
    /// Zones EXCON is composing a selection of (solid teal).
    #[prop_or_default]
    pub selected_zones: Vec<String>,
    /// Zones already known to the role EXCON is looking at (dashed teal).
    #[prop_or_default]
    pub marked_zones: Vec<String>,
}
#[function_component(SituationMap)]
pub fn situation_map(props: &MapProps) -> Html {
    let canvas = use_node_ref();
    let frame = use_node_ref();
    let view = use_mut_ref(|| View::HOME);
    let drag = use_mut_ref(Drag::default);
    let tick = use_reducer(|| Tick(0));
    let hidden = use_state(|| 0u8);
    let hover = use_state(|| None::<(f64, f64)>);
    let pinned = use_state(|| None::<(f64, f64)>);
    let geometry = geom(&props.stat);
    let bh = geometry.map(|g| g.3).unwrap_or(SIZE);
    let bh_ref = use_mut_ref(|| bh);
    *bh_ref.borrow_mut() = bh;
    let current = *view.borrow();
    let pan_zoom = props.pan_zoom;
    let pz_ref = use_mut_ref(|| pan_zoom);
    *pz_ref.borrow_mut() = pan_zoom;
    {
        let canvas = canvas.clone();
        use_effect_with(
            (
                props.stat.clone(),
                props.view.clone(),
                props.simple,
                props.pending.clone(),
                props.lang.clone(),
                tick.0,
                *hidden,
                (props.selected_zones.clone(), props.marked_zones.clone()),
            ),
            {
                let view = view.clone();
                move |(stat, v, simple, pending, lang, _, hidden, (selected, marked))| {
                    if let Some(canvas) = canvas.cast::<HtmlCanvasElement>() {
                        let _ = draw(
                            &canvas,
                            stat,
                            v,
                            *simple,
                            pending.as_ref(),
                            lang,
                            *view.borrow(),
                            *hidden,
                            selected,
                            marked,
                        );
                    }
                }
            },
        );
    }
    {
        // A wheel listener must be non-passive to stop the page from scrolling while zooming.
        let canvas = canvas.clone();
        let view = view.clone();
        let bh_ref = bh_ref.clone();
        let pz_ref = pz_ref.clone();
        let redraw = tick.dispatcher();
        use_effect_with((), move |_| {
            let mut cleanup: Option<(web_sys::HtmlCanvasElement, Closure<dyn FnMut(WheelEvent)>)> =
                None;
            if let Some(el) = canvas.cast::<HtmlCanvasElement>() {
                let target = el.clone();
                let cb = Closure::<dyn FnMut(WheelEvent)>::new(move |e: WheelEvent| {
                    if !*pz_ref.borrow() {
                        return;
                    }
                    e.prevent_default();
                    let rect = target.get_bounding_client_rect();
                    if rect.width() <= 0. || rect.height() <= 0. {
                        return;
                    }
                    let unit = if e.delta_mode() == 1 { 16. } else { 1. };
                    let f = (-e.delta_y() * unit * 0.0015).exp();
                    view.borrow_mut().zoom_at(
                        f,
                        (e.client_x() as f64 - rect.left()) / rect.width(),
                        (e.client_y() as f64 - rect.top()) / rect.height(),
                        *bh_ref.borrow(),
                    );
                    redraw.dispatch(());
                });
                let _ = el.add_event_listener_with_callback("wheel", cb.as_ref().unchecked_ref());
                cleanup = Some((el, cb));
            }
            move || {
                if let Some((el, cb)) = cleanup {
                    let _ = el.remove_event_listener_with_callback("wheel", cb.as_ref().unchecked_ref());
                }
            }
        });
    }
    // Client coordinates → world metres under the current view.
    let locate = {
        let canvas = canvas.clone();
        let view = view.clone();
        let stat = props.stat.clone();
        move |cx: f64, cy: f64| -> Option<(f64, f64)> {
            let canvas = canvas.cast::<HtmlCanvasElement>()?;
            let (w, h, k, bh) = geom(&stat)?;
            let rect = canvas.get_bounding_client_rect();
            if rect.width() <= 0. || rect.height() <= 0. {
                return None;
            }
            let v = *view.borrow();
            let bx = v.ox + (cx - rect.left()) / rect.width() * SIZE / v.z;
            let by = v.oy + (cy - rect.top()) / rect.height() * bh / v.z;
            Some(((bx / k).round().clamp(0., w - 1.), (h - by / k).round().clamp(0., h - 1.)))
        }
    };
    let onclick = {
        let cb = props.onclick.clone();
        let stat = props.stat.clone();
        let drag = drag.clone();
        let pinned = pinned.clone();
        let locate = locate.clone();
        Callback::from(move |e: MouseEvent| {
            if std::mem::take(&mut drag.borrow_mut().moved) > 5. {
                return;
            }
            if let Some((x, y)) = locate(e.client_x() as f64, e.client_y() as f64) {
                let h = n(&stat["grid"]["rows"]) * n(&stat["grid"]["cellsize"]);
                pinned.set(Some((x, y)));
                cb.emit(json!({"x":x,"y":y,"zone":zone_name(&stat, x, y, h)}));
            }
        })
    };
    let ondblclick = {
        let view = view.clone();
        let canvas = canvas.clone();
        let redraw = tick.dispatcher();
        let bh_ref = bh_ref.clone();
        Callback::from(move |e: MouseEvent| {
            if !pan_zoom {
                return;
            }
            if let Some(canvas) = canvas.cast::<HtmlCanvasElement>() {
                let rect = canvas.get_bounding_client_rect();
                if rect.width() > 0. && rect.height() > 0. {
                    view.borrow_mut().zoom_at(
                        2.,
                        (e.client_x() as f64 - rect.left()) / rect.width(),
                        (e.client_y() as f64 - rect.top()) / rect.height(),
                        *bh_ref.borrow(),
                    );
                    redraw.dispatch(());
                }
            }
        })
    };
    let onpointerdown = {
        let drag = drag.clone();
        let canvas = canvas.clone();
        Callback::from(move |e: PointerEvent| {
            if !pan_zoom {
                return;
            }
            let mut d = drag.borrow_mut();
            if d.pts.is_empty() {
                d.moved = 0.;
            }
            d.pts.retain(|p| p.0 != e.pointer_id());
            d.pts.push((e.pointer_id(), e.client_x() as f64, e.client_y() as f64));
            if let Some(c) = canvas.cast::<HtmlCanvasElement>() {
                let _ = c.set_pointer_capture(e.pointer_id());
            }
        })
    };
    let onpointermove = {
        let drag = drag.clone();
        let view = view.clone();
        let canvas = canvas.clone();
        let redraw = tick.dispatcher();
        let bh_ref = bh_ref.clone();
        let hover = hover.clone();
        let locate = locate.clone();
        Callback::from(move |e: PointerEvent| {
            let Some(c) = canvas.cast::<HtmlCanvasElement>() else {
                return;
            };
            let rect = c.get_bounding_client_rect();
            if rect.width() <= 0. || rect.height() <= 0. {
                return;
            }
            let (x, y) = (e.client_x() as f64, e.client_y() as f64);
            let mut d = drag.borrow_mut();
            let Some(i) = d.pts.iter().position(|p| p.0 == e.pointer_id()) else {
                if e.pointer_type() == "mouse" {
                    hover.set(locate(x, y));
                }
                return;
            };
            let old = d.pts.clone();
            d.pts[i].1 = x;
            d.pts[i].2 = y;
            let bh = *bh_ref.borrow();
            let mut v = view.borrow_mut();
            if old.len() == 1 {
                let (dx, dy) = (x - old[i].1, y - old[i].2);
                d.moved += dx.abs() + dy.abs();
                v.pan(dx / rect.width(), dy / rect.height(), bh);
            } else {
                // Pinch: scale by the change in finger distance, follow the midpoint.
                let (a, b) = (old[0], old[1]);
                let (na, nb) = (d.pts[0], d.pts[1]);
                let dist = |p: (i32, f64, f64), q: (i32, f64, f64)| (p.1 - q.1).hypot(p.2 - q.2);
                let (d0, d1) = (dist(a, b), dist(na, nb));
                if d0 > 1. && d1 > 1. {
                    let mid0 = ((a.1 + b.1) / 2., (a.2 + b.2) / 2.);
                    let mid1 = ((na.1 + nb.1) / 2., (na.2 + nb.2) / 2.);
                    v.zoom_at(
                        d1 / d0,
                        (mid0.0 - rect.left()) / rect.width(),
                        (mid0.1 - rect.top()) / rect.height(),
                        bh,
                    );
                    v.pan(
                        (mid1.0 - mid0.0) / rect.width(),
                        (mid1.1 - mid0.1) / rect.height(),
                        bh,
                    );
                    d.moved += 10.;
                }
            }
            drop(v);
            redraw.dispatch(());
        })
    };
    let onpointerup = {
        let drag = drag.clone();
        Callback::from(move |e: PointerEvent| {
            drag.borrow_mut().pts.retain(|p| p.0 != e.pointer_id());
        })
    };
    let onpointerleave = {
        let hover = hover.clone();
        Callback::from(move |e: PointerEvent| {
            if e.pointer_type() == "mouse" {
                hover.set(None);
            }
        })
    };
    let onkeydown = {
        let view = view.clone();
        let redraw = tick.dispatcher();
        let bh_ref = bh_ref.clone();
        Callback::from(move |e: KeyboardEvent| {
            if !pan_zoom {
                return;
            }
            let bh = *bh_ref.borrow();
            let mut v = view.borrow_mut();
            match e.key().as_str() {
                "+" | "=" => v.zoom_at(1.4, 0.5, 0.5, bh),
                "-" | "_" => v.zoom_at(1. / 1.4, 0.5, 0.5, bh),
                "0" | "Home" => *v = View::HOME,
                "ArrowLeft" => v.pan(0.1, 0., bh),
                "ArrowRight" => v.pan(-0.1, 0., bh),
                "ArrowUp" => v.pan(0., 0.1, bh),
                "ArrowDown" => v.pan(0., -0.1, bh),
                _ => return,
            }
            e.prevent_default();
            drop(v);
            redraw.dispatch(());
        })
    };
    let zoom_button = |f: f64| {
        let view = view.clone();
        let redraw = tick.dispatcher();
        let bh_ref = bh_ref.clone();
        Callback::from(move |_: MouseEvent| {
            view.borrow_mut().zoom_at(f, 0.5, 0.5, *bh_ref.borrow());
            redraw.dispatch(());
        })
    };
    let reset = {
        let view = view.clone();
        let redraw = tick.dispatcher();
        Callback::from(move |_: MouseEvent| {
            *view.borrow_mut() = View::HOME;
            redraw.dispatch(());
        })
    };
    let fire_box = fire_bounds(&props.stat, &props.view);
    let fit_fire = {
        let view = view.clone();
        let redraw = tick.dispatcher();
        Callback::from(move |_: MouseEvent| {
            if let Some((x0, y0, x1, y1)) = fire_box {
                let (cx, cy) = ((x0 + x1) / 2., (y0 + y1) / 2.);
                let pad = 1.4;
                let mut v = view.borrow_mut();
                v.z = (SIZE / ((x1 - x0) * pad).max(1.)).min(bh / ((y1 - y0) * pad).max(1.));
                v.z = v.z.clamp(1., MAX_ZOOM.min(6.));
                v.ox = cx - SIZE / v.z / 2.;
                v.oy = cy - bh / v.z / 2.;
                v.clamp(bh);
                drop(v);
                redraw.dispatch(());
            }
        })
    };
    let fullscreen = {
        let frame = frame.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(el) = frame.cast::<web_sys::Element>() {
                let _ = el.request_fullscreen();
            }
        })
    };
    let lang = props.lang.as_str();
    let t = |k: &'static str| crate::i18n::t(lang, k);
    let probe = if props.inspect { (*hover).or(*pinned) } else { None };
    let lines = probe
        .map(|(x, y)| probe_lines(&props.stat, &props.view, x, y, lang, props.simple, current.z))
        .unwrap_or_default();
    let clear_pin = {
        let pinned = pinned.clone();
        Callback::from(move |_: MouseEvent| pinned.set(None))
    };
    html! {<div class="mapbox"><div class="mapview" ref={frame}>
        <canvas id={props.id.clone()} ref={canvas} tabindex="0" {onclick} {ondblclick} {onpointerdown} {onpointermove} {onpointerup} onpointercancel={onpointerup_cancel(&drag)} {onpointerleave} {onkeydown}
            style={if props.simple {"max-width:min(100%, calc(100dvh - 280px))"}else{""}}
            aria-label={if props.simple {t("map_tactical")} else {t("map_situation")}}/>
        if props.tools {<div class="map-tools" role="toolbar" aria-label={t("map_tools")}>
            <button type="button" title={t("map_zoom_in")} aria-label={t("map_zoom_in")} onclick={zoom_button(1.5)} disabled={current.z >= MAX_ZOOM}>{"+"}</button>
            <button type="button" title={t("map_zoom_out")} aria-label={t("map_zoom_out")} onclick={zoom_button(1. / 1.5)} disabled={current.z <= 1.}>{"−"}</button>
            <button type="button" title={t("map_reset")} aria-label={t("map_reset")} onclick={reset} disabled={current == View::HOME}>{"⟲"}</button>
            <button type="button" title={t("map_fit_fire")} aria-label={t("map_fit_fire")} onclick={fit_fire} disabled={fire_box.is_none()}>{"🔥"}</button>
            <button type="button" title={t("map_fullscreen")} aria-label={t("map_fullscreen")} onclick={fullscreen}>{"⛶"}</button>
            <details class="map-layers"><summary title={t("map_layers")} aria-label={t("map_layers")}>{"☰"}</summary><div>
                {for LAYERS.iter().map(|(bit, key)| { let bit = *bit; let hidden = hidden.clone(); html!{<label><input type="checkbox" checked={*hidden & bit == 0} onchange={Callback::from(move |_| hidden.set(*hidden ^ bit))}/>{t(key)}</label>} })}
            </div></details>
        </div>}
        if !lines.is_empty() {<div class="map-probe" aria-live="polite">{for lines.iter().map(|l| html!{<div>{l.clone()}</div>})}if pinned.is_some() && hover.is_none() {<button type="button" aria-label={t("map_close_probe")} onclick={clear_pin}>{"✕"}</button>}</div>}
        if current.z > 1. {<div class="map-zoom-badge">{format!("×{:.1}", current.z)}</div>}
    </div><p class="map-legend">{t("map_legend")}</p><p class="map-legend map-keys">{t("map_controls")}</p></div>}
}
fn onpointerup_cancel(drag: &Rc<RefCell<Drag>>) -> Callback<PointerEvent> {
    let drag = drag.clone();
    Callback::from(move |e: PointerEvent| {
        drag.borrow_mut().pts.retain(|p| p.0 != e.pointer_id());
    })
}
fn zone_name(stat: &Value, x: f64, y: f64, h: f64) -> String {
    let size = n(&stat["zones"]["size_m"]);
    if size <= 0. {
        return String::new();
    }
    format!(
        "{}{}",
        (b'A' + (x / size).floor().clamp(0., 25.) as u8) as char,
        ((h - y) / size).floor() as u32 + 1
    )
}
/// Bitmap-space box `(x0, y0, x1, y1)` around burning/burnt cells, if any.
fn fire_bounds(stat: &Value, v: &Value) -> Option<(f64, f64, f64, f64)> {
    let (_, _, k, _) = geom(stat)?;
    let cols = stat["grid"]["cols"].as_u64()? as usize;
    let cs = n(&stat["grid"]["cellsize"]) * k;
    let fire = bytes(&v["fire"]);
    let mut cells: Vec<(usize, usize)> = fire
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(**s, 1 | 2))
        .map(|(i, _)| (i % cols, i / cols))
        .collect();
    for obs in list(&v["cells"]) {
        if matches!(obs["state"].as_u64(), Some(1 | 2)) {
            if let (Some(r), Some(c)) = (obs["row"].as_u64(), obs["col"].as_u64()) {
                cells.push((c as usize, r as usize));
            }
        }
    }
    let x0 = cells.iter().map(|c| c.0).min()? as f64 * cs;
    let x1 = (cells.iter().map(|c| c.0).max()? + 1) as f64 * cs;
    let y0 = cells.iter().map(|c| c.1).min()? as f64 * cs;
    let y1 = (cells.iter().map(|c| c.1).max()? + 1) as f64 * cs;
    Some((x0, y0, x1, y1))
}
fn parse_zone(name: &str, cols: u64, rows: u64) -> Option<(u64, u64)> {
    let name = name.trim().to_uppercase();
    let col = name.as_bytes().first()?.checked_sub(b'A')? as u64;
    let row = name.get(1..)?.parse::<u64>().ok()?;
    (col < cols && row > 0 && row <= rows).then_some((col, row - 1))
}
/// Per-zone "known" flags (row-major) for player views, `None` for authoritative views.
/// A zone is known when disclosed (`known_zones`) or when it holds a reported cell.
fn fog_zones(stat: &Value, v: &Value) -> Option<Vec<bool>> {
    let known = v["known_zones"].as_array()?;
    let (cols, rows) = (stat["zones"]["cols"].as_u64()?, stat["zones"]["rows"].as_u64()?);
    let mut out = vec![false; (cols * rows) as usize];
    for name in known {
        if let Some((c, r)) = parse_zone(text(name), cols, rows) {
            out[(r * cols + c) as usize] = true;
        }
    }
    Some(out)
}
fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0. {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0., 1.)
    } else {
        0.
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}
/// Text lines describing what is at world point `(x, y)`; the tolerance follows the zoom.
fn probe_lines(
    stat: &Value,
    v: &Value,
    x: f64,
    y: f64,
    lang: &str,
    simple: bool,
    zoom: f64,
) -> Vec<String> {
    use crate::i18n::{t, tf};
    let Some((_, h, k, _)) = geom(stat) else {
        return Vec::new();
    };
    let tol = 14. / (k * zoom);
    let cols = stat["grid"]["cols"].as_u64().unwrap_or(1) as usize;
    let rows = stat["grid"]["rows"].as_u64().unwrap_or(1) as usize;
    let cell = n(&stat["grid"]["cellsize"]);
    let mut out = vec![format!(
        "{} · {:.0}, {:.0} m",
        tf(lang, "insp_zone", &[&zone_name(stat, x, y, h)]),
        x,
        y
    )];
    let col = (x / cell).floor() as usize;
    let row = ((h - y) / cell).floor() as usize;
    if col < cols && row < rows {
        let i = row * cols + col;
        let fuel = bytes(&stat["fuel"]).get(i).copied().unwrap_or(0);
        // Player views have no live raster (fog of war): only reported cells are known.
        let live = !bytes(&v["fire"]).is_empty();
        let mut state = if live {
            bytes(&v["fire"]).get(i).copied().unwrap_or(0)
        } else {
            u8::MAX
        };
        for obs in list(&v["cells"]) {
            if obs["row"].as_u64() == Some(row as u64) && obs["col"].as_u64() == Some(col as u64) {
                let s = obs["state"].as_u64().unwrap_or(0) as u8;
                if colour(s)[3] > 0 {
                    state = s;
                } else if !live {
                    state = 0;
                }
            }
        }
        let name = match state {
            1 => "fs_burning",
            2 => "fs_burnt",
            3 => "fs_line",
            4 => "fs_water",
            5 => "fs_possible",
            u8::MAX => "fs_unknown",
            _ => "fs_none",
        };
        out.push(format!(
            "{} · {}",
            tf(lang, "insp_fuel", &[&fuel.to_string()]),
            t(lang, name)
        ));
    }
    for (i, b) in list(&stat["buildings"]).iter().enumerate() {
        if simple && text(&b["name"]).is_empty() {
            continue;
        }
        let (bx, by) = (n(&b["pos"][0]), n(&b["pos"][1]));
        let (hw, hh) = (n(&b["size"][0]) / 2. + tol, n(&b["size"][1]) / 2. + tol);
        if (x - bx).abs() <= hw && (y - by).abs() <= hh {
            let s = &v["buildings"][i];
            let name = if text(&b["name"]).is_empty() {
                format!("#{}", b["id"])
            } else {
                text(&b["name"]).to_string()
            };
            let mut line = tf(lang, "insp_building", &[&name]);
            if s["inside"].is_number() {
                line += &format!(" · {}", tf(lang, "insp_inside", &[&s["inside"].to_string(), &s["evacuated"].to_string()]));
            }
            let flag = match (s["damage"].as_u64(), s["evacuating"] == true, s["threatened"] == true) {
                (Some(2), ..) => Some("insp_destroyed"),
                (Some(1), ..) => Some("insp_damaged"),
                (_, true, _) => Some("insp_evacuating"),
                (_, _, true) => Some("insp_threatened"),
                _ if s["fully_evacuated"] == true => Some("insp_evacuated"),
                _ => None,
            };
            if let Some(f) = flag {
                line += &format!(" · {}", t(lang, f));
            }
            out.push(line);
        }
    }
    for road in list(&stat["roads"]) {
        if simple && road["drivable"] == false {
            continue;
        }
        let pts: Vec<(f64, f64)> = list(&road["points"]).iter().map(|p| (n(&p[0]), n(&p[1]))).collect();
        if pts.windows(2).any(|w| seg_dist((x, y), w[0], w[1]) <= tol) {
            let s = &v["roads"][text(&road["id"])];
            let mut line = tf(lang, "insp_road", &[text(&road["name"])]);
            if s["cut_by_fire"] == true {
                line += &format!(" · {}", t(lang, "map_road_cut"));
            } else if s["closed"] == true {
                line += &format!(" · {}", t(lang, "insp_closed"));
            }
            out.push(line);
        }
    }
    for d in list(&v["deployments"]).iter().chain(list(&v["observed_units"])) {
        let tk = &d["task"];
        if d["active_qty"] == 0 || tk["status"] == "cancelled" {
            continue;
        }
        if (n(&d["position_m"][0]) - x).hypot(n(&d["position_m"][1]) - y) <= tol * 1.5 {
            out.push(tf(lang, "insp_unit", &[&tk["id"].to_string(), text(&tk["status"]), text(&tk["detail"])]));
        }
    }
    for place in list(&stat["places"]) {
        let name = text(&place["name"]);
        if !name.contains(" edge")
            && !name.contains("junction")
            && (n(&place["pos"][0]) - x).hypot(n(&place["pos"][1]) - y) <= tol * 2.
        {
            out.push(name.to_string());
        }
    }
    out
}
fn n(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
fn bytes(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(text(v))
        .unwrap_or_default()
}
fn colour(v: u8) -> [u8; 4] {
    match v {
        1 => [255, 70, 20, 235],
        2 => [35, 30, 28, 215],
        3 => [150, 90, 30, 230],
        4 => [60, 150, 255, 110],
        5 => [255, 150, 40, 80],
        _ => [0; 4],
    }
}
fn trace(ctx: &CanvasRenderingContext2d, points: &[Value], k: f64, h: f64) {
    ctx.begin_path();
    for (i, p) in points.iter().enumerate() {
        let x = n(&p[0]) * k;
        let y = (h - n(&p[1])) * k;
        if i == 0 {
            ctx.move_to(x, y)
        } else {
            ctx.line_to(x, y)
        }
    }
}
fn draw(
    canvas: &HtmlCanvasElement,
    stat: &Value,
    v: &Value,
    simple: bool,
    pending: Option<&Value>,
    lang: &str,
    vw: View,
    hidden: u8,
    selected: &[String],
    marked: &[String],
) -> Result<(), wasm_bindgen::JsValue> {
    let on = |bit: u8| hidden & bit == 0;
    let (z, u) = (vw.z, 1. / vw.z);
    // Strokes and markers grow only with the square root of the zoom.
    let s = u.sqrt();
    let cols = stat["grid"]["cols"].as_u64().unwrap_or(1) as usize;
    let rows = stat["grid"]["rows"].as_u64().unwrap_or(1) as usize;
    let cell = n(&stat["grid"]["cellsize"]);
    let w = cols as f64 * cell;
    let h = rows as f64 * cell;
    if w <= 0. || h <= 0. {
        return Ok(());
    }
    let size = SIZE;
    let k = size / w;
    canvas.set_width(size as u32);
    canvas.set_height((h * k) as u32);
    let ctx = canvas
        .get_context("2d")?
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()?;
    ctx.set_transform(z, 0., 0., z, -vw.ox * z, -vw.oy * z)?;
    let raster = window()
        .document()
        .unwrap()
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()?;
    raster.set_width(cols as u32);
    raster.set_height(rows as u32);
    let rctx = raster
        .get_context("2d")?
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()?;
    let fuel = bytes(&stat["fuel"]);
    let fire = bytes(&v["fire"]);
    let mut rgba = vec![0u8; cols * rows * 4];
    for i in 0..cols * rows {
        let bg = match fuel.get(i).copied().unwrap_or(0) {
            1 => [107, 142, 78],
            2 => [154, 165, 90],
            3 => [214, 208, 196],
            4 => [217, 210, 138],
            5 => [63, 107, 58],
            6 => [194, 183, 110],
            7 => [127, 163, 122],
            _ => [200, 200, 190],
        };
        let fg = if on(L_FIRE) {
            colour(fire.get(i).copied().unwrap_or(0))
        } else {
            [0; 4]
        };
        let a = fg[3] as f64 / 255.;
        for c in 0..3 {
            rgba[i * 4 + c] = (bg[c] as f64 * (1. - a) + fg[c] as f64 * a) as u8;
        }
        rgba[i * 4 + 3] = 255;
    }
    for obs in list(&v["cells"]).iter().filter(|_| on(L_FIRE)) {
        let i = (obs["row"].as_u64().unwrap_or(rows as u64) as usize * cols
            + obs["col"].as_u64().unwrap_or(cols as u64) as usize)
            * 4;
        if i + 4 <= rgba.len() {
            let fg = colour(obs["state"].as_u64().unwrap_or(0) as u8);
            if fg[3] > 0 {
                rgba[i..i + 4].copy_from_slice(&fg);
            }
        }
    }
    rctx.put_image_data(
        &web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&rgba),
            cols as u32,
            rows as u32,
        )?,
        0.,
        0.,
    )?;
    ctx.set_image_smoothing_enabled(false);
    ctx.draw_image_with_html_canvas_element_and_dw_and_dh(&raster, 0., 0., size, h * k)?;
    ctx.set_font(&format!(
        "{}px system-ui",
        if simple { 19. } else { 16. } * u
    ));
    let mut labels: Vec<(f64, f64, f64, f64)> = Vec::new();
    let label = |ctx: &CanvasRenderingContext2d,
                 s: &str,
                 x: f64,
                 y: f64,
                 labels: &mut Vec<(f64, f64, f64, f64)>| {
        if !on(L_LABELS) {
            return;
        }
        let width = (s.chars().count() as f64 * 8.5 + 8.) * u;
        let x = x.clamp(3. * u, (size - width - 3. * u).max(3. * u));
        let y = y.clamp(22. * u, h * k - 5. * u);
        let candidate = [0., -24., 24., -48., 48.]
            .into_iter()
            .map(|dy| (x, y + (dy - 18.) * u, width, 23. * u))
            .find(|b| {
                b.1 >= 0.
                    && b.1 + b.3 <= h * k
                    && (!simple
                        || !labels.iter().any(|a| {
                            b.0 < a.0 + a.2 && b.0 + b.2 > a.0 && b.1 < a.1 + a.3 && b.1 + b.3 > a.1
                        }))
            });
        if let Some(b) = candidate {
            labels.push(b);
            ctx.set_fill_style_str("rgba(255,255,255,.85)");
            ctx.fill_rect(b.0, b.1, b.2, b.3);
            ctx.set_fill_style_str("#122b2c");
            let _ = ctx.fill_text(s, b.0 + 3. * u, b.1 + 18. * u);
        }
    };
    let z = &stat["zones"];
    let zsize = n(&z["size_m"]) * k;
    let (zcols, zrows) = (z["cols"].as_u64().unwrap_or(0), z["rows"].as_u64().unwrap_or(0));
    if let Some(known) = fog_zones(stat, v).filter(|_| on(L_FOG)) {
        // Fog of war: zones the role knows nothing about are veiled.
        ctx.set_fill_style_str("rgba(24,36,40,.55)");
        for row in 0..zrows {
            for col in 0..zcols {
                if !known[(row * zcols + col) as usize] {
                    ctx.fill_rect(col as f64 * zsize, row as f64 * zsize, zsize, zsize);
                }
            }
        }
    }
    for (names, fill, dash) in [(marked, "rgba(18,103,82,.10)", true), (selected, "rgba(18,103,82,.28)", false)] {
        for name in names {
            if let Some((col, row)) = parse_zone(name, zcols, zrows) {
                let (x, y) = (col as f64 * zsize, row as f64 * zsize);
                ctx.set_fill_style_str(fill);
                ctx.fill_rect(x, y, zsize, zsize);
                ctx.set_stroke_style_str("#126752");
                ctx.set_line_width(3. * s);
                if dash {
                    ctx.set_line_dash(&js_sys::Array::of2(&(10. * s).into(), &(6. * s).into()))?;
                }
                ctx.stroke_rect(x + 2. * s, y + 2. * s, zsize - 4. * s, zsize - 4. * s);
                ctx.set_line_dash(&js_sys::Array::new())?;
            }
        }
    }
    ctx.set_line_width(u);
    ctx.set_stroke_style_str("rgba(0,0,0,.25)");
    for col in (0..=z["cols"].as_u64().unwrap_or(0)).filter(|_| on(L_GRID)) {
        ctx.begin_path();
        ctx.move_to(col as f64 * zsize, 0.);
        ctx.line_to(col as f64 * zsize, h * k);
        ctx.stroke();
    }
    for row in (0..=z["rows"].as_u64().unwrap_or(0)).filter(|_| on(L_GRID)) {
        ctx.begin_path();
        ctx.move_to(0., row as f64 * zsize);
        ctx.line_to(size, row as f64 * zsize);
        ctx.stroke();
    }
    for row in 0..z["rows"].as_u64().unwrap_or(0) {
        for col in 0..z["cols"].as_u64().unwrap_or(0) {
            let name = format!("{}{}", (b'A' + col as u8) as char, row + 1);
            if list(&v["warned"]).contains(&json!(name)) {
                ctx.set_fill_style_str("rgba(255,200,0,.15)");
                ctx.fill_rect(col as f64 * zsize, row as f64 * zsize, zsize, zsize);
            }
            ctx.set_fill_style_str("#203c3d");
            if on(L_GRID) {
                ctx.fill_text(&name, col as f64 * zsize + 5. * u, row as f64 * zsize + 21. * u)?;
            }
        }
    }
    ctx.set_line_join("round");
    ctx.set_line_cap("round");
    for road in list(&stat["roads"]).iter().filter(|_| on(L_ROADS)) {
        if simple && road["drivable"] == false {
            continue;
        }
        let points = list(&road["points"]);
        trace(&ctx, points, k, h);
        ctx.set_stroke_style_str("#29363d");
        ctx.set_line_width(6. * s);
        ctx.stroke();
        ctx.set_stroke_style_str("#e6e4d8");
        ctx.set_line_width(3. * s);
        ctx.stroke();
        let state = &v["roads"][text(&road["id"])];
        let segments = list(&stat["road_segments"])
            .iter()
            .filter(|s| s["road"] == road["id"])
            .collect::<Vec<_>>();
        if segments.is_empty() && (state["closed"] == true || state["cut_by_fire"] == true) {
            ctx.set_stroke_style_str(if state["cut_by_fire"] == true {
                "#b72e26"
            } else {
                "#b87512"
            });
            ctx.stroke();
        }
        let mut marked = false;
        for segment in segments {
            let cut = if state["fire_closed_links"].is_null() {
                state["cut_by_fire"] == true
            } else {
                list(&segment["links"])
                    .iter()
                    .any(|id| list(&state["fire_closed_links"]).contains(id))
            };
            if !cut && state["closed"] != true {
                continue;
            }
            let points = if segment["points"].is_array() {
                segment["points"].clone()
            } else {
                json!([segment["from"], segment["to"]])
            };
            trace(&ctx, list(&points), k, h);
            ctx.set_stroke_style_str(if cut { "#b72e26" } else { "#b87512" });
            ctx.set_line_width(5. * s);
            if !cut {
                ctx.set_line_dash(&js_sys::Array::of2(&(8. * s).into(), &(5. * s).into()))?;
            }
            ctx.stroke();
            ctx.set_line_dash(&js_sys::Array::new())?;
            if !marked {
                label(
                    &ctx,
                    &format!(
                        "{}: {}",
                        if cut {
                            crate::i18n::t(lang, "map_road_cut")
                        } else {
                            "— Chiusura polizia"
                        },
                        text(&road["name"])
                    ),
                    n(&segment["from"][0]) * k,
                    (h - n(&segment["from"][1])) * k - 6. * u,
                    &mut labels,
                );
                marked = true;
            }
        }
    }
    for (i, b) in list(&stat["buildings"]).iter().enumerate() {
        if !on(L_BUILDINGS) || (simple && text(&b["name"]).is_empty()) {
            continue;
        }
        let state = &v["buildings"][i];
        let x = n(&b["pos"][0]) * k;
        let y = (h - n(&b["pos"][1])) * k;
        let bw = (n(&b["size"][0]) * k).max(4. * s);
        let bh = (n(&b["size"][1]) * k).max(4. * s);
        let fill = if state["damage"] == 2 {
            "#000"
        } else if state["damage"] == 1 {
            "#ff7a00"
        } else if state["evacuating"] == true {
            "#00c8ff"
        } else if state["fully_evacuated"] == true
            || (state["inside"] == 0 && n(&state["evacuated"]) > 0.)
        {
            "#2e6bff"
        } else {
            "#334144"
        };
        ctx.set_fill_style_str(fill);
        ctx.fill_rect(x - bw / 2., y - bh / 2., bw, bh);
        if state["damage"] == 2 {
            ctx.begin_path();
            ctx.move_to(x - bw / 2., y - bh / 2.);
            ctx.line_to(x + bw / 2., y + bh / 2.);
            ctx.move_to(x + bw / 2., y - bh / 2.);
            ctx.line_to(x - bw / 2., y + bh / 2.);
            ctx.set_stroke_style_str("#f33");
            ctx.set_line_width(2. * s);
            ctx.stroke();
        }
        if state["threatened"] == true {
            ctx.set_stroke_style_str("#ffb000");
            ctx.stroke_rect(x - bw / 2. - 2. * s, y - bh / 2. - 2. * s, bw + 4. * s, bh + 4. * s);
        }
    }
    for line in list(&v["lines"]) {
        trace(&ctx, &[line["from"].clone(), line["to"].clone()], k, h);
        ctx.set_stroke_style_str("#ffe000");
        ctx.set_line_width(3. * s);
        ctx.set_line_dash(&js_sys::Array::of2(&(7. * s).into(), &(4. * s).into()))?;
        ctx.stroke();
        ctx.set_line_dash(&js_sys::Array::new())?;
    }
    for drop in list(&v["drops"]) {
        ctx.begin_path();
        ctx.arc(
            n(&drop["at"][0]) * k,
            (h - n(&drop["at"][1])) * k,
            n(&drop["radius_m"]) * k,
            0.,
            std::f64::consts::TAU,
        )?;
        ctx.set_stroke_style_str("#1e90ff");
        ctx.set_line_width(3. * s);
        ctx.stroke();
    }
    for location in list(&v["reported_locations"]) {
        let x = n(&location[0]) * k;
        let y = (h - n(&location[1])) * k;
        ctx.begin_path();
        ctx.arc(x, y, 9. * s, 0., std::f64::consts::TAU)?;
        ctx.set_stroke_style_str("#e33");
        ctx.stroke();
        label(&ctx, crate::i18n::t(lang, "map_reported"), x + 12. * u, y, &mut labels);
    }
    if let Some(forecast) = list(&v["forecast_products"]).last().filter(|_| on(L_FORECAST)) {
        for (idx, horizon) in list(&forecast["horizons"]).iter().enumerate() {
            let probability = list(&horizon["probability"]);
            if probability.len() != rows * cols {
                continue;
            }
            let reached = |i: usize| n(&probability[i]) >= 0.5;
            let cs = cell * k;
            ctx.begin_path();
            for row in 0..rows {
                for col in 0..cols {
                    let i = row * cols + col;
                    if !reached(i) {
                        continue;
                    }
                    let x = col as f64 * cs;
                    let y = row as f64 * cs;
                    for (edge, show) in [
                        (0, col == 0 || !reached(i.saturating_sub(1))),
                        (
                            1,
                            col + 1 == cols || !reached((i + 1).min(probability.len() - 1)),
                        ),
                        (2, row == 0 || !reached(i.saturating_sub(cols))),
                        (
                            3,
                            row + 1 == rows || !reached((i + cols).min(probability.len() - 1)),
                        ),
                    ] {
                        if show {
                            match edge {
                                0 => {
                                    ctx.move_to(x, y);
                                    ctx.line_to(x, y + cs)
                                }
                                1 => {
                                    ctx.move_to(x + cs, y);
                                    ctx.line_to(x + cs, y + cs)
                                }
                                2 => {
                                    ctx.move_to(x, y);
                                    ctx.line_to(x + cs, y)
                                }
                                _ => {
                                    ctx.move_to(x, y + cs);
                                    ctx.line_to(x + cs, y + cs)
                                }
                            }
                        }
                    }
                }
            }
            ctx.set_stroke_style_str(if idx == 0 { "#713b72" } else { "#9d5576" });
            ctx.set_line_width(3. * s);
            ctx.set_line_dash(&js_sys::Array::of2(&(8. * s).into(), &(5. * s).into()))?;
            ctx.stroke();
            ctx.set_line_dash(&js_sys::Array::new())?;
        }
    }
    for d in list(&v["deployments"])
        .iter()
        .chain(list(&v["observed_units"]))
        .filter(|_| on(L_UNITS))
    {
        let t = &d["task"];
        if d["active_qty"] == 0 || t["status"] == "cancelled" {
            continue;
        }
        let x = n(&d["position_m"][0]) * k;
        let y = (h - n(&d["position_m"][1])) * k;
        let color = match text(&t["status"]) {
            "working" => "#008c9e",
            "completed" => "#247d42",
            "failed" => "#b72e26",
            _ => "#ad6912",
        };
        ctx.set_fill_style_str(color);
        ctx.fill_rect(x - 7. * s, y - 5. * s, 14. * s, 10. * s);
        ctx.set_stroke_style_str("#112632");
        ctx.set_line_width(s);
        ctx.stroke_rect(x - 7. * s, y - 5. * s, 14. * s, 10. * s);
        if ["en_route", "withdrawing", "refilling"].contains(&text(&t["status"]))
            && t["target"].is_array()
        {
            let tx = n(&t["target"][0]) * k;
            let ty = (h - n(&t["target"][1])) * k;
            ctx.begin_path();
            ctx.move_to(tx, ty - 8. * s);
            ctx.line_to(tx + 8. * s, ty);
            ctx.line_to(tx, ty + 8. * s);
            ctx.line_to(tx - 8. * s, ty);
            ctx.close_path();
            ctx.set_stroke_style_str(color);
            ctx.stroke();
        }
        label(
            &ctx,
            &format!("#{} {}", t["id"], text(&t["status"])),
            x + 10. * u,
            y - 5. * u,
            &mut labels,
        );
    }
    for b in list(&stat["buildings"]) {
        if let Some(name) = b["name"].as_str().filter(|s| !s.is_empty()) {
            label(
                &ctx,
                name,
                n(&b["pos"][0]) * k + 8. * u,
                (h - n(&b["pos"][1])) * k - 6. * u,
                &mut labels,
            );
        }
    }
    for place in list(&stat["places"]) {
        let name = text(&place["name"]);
        if !name.contains(" edge") && !name.contains("junction") {
            label(
                &ctx,
                name,
                n(&place["pos"][0]) * k,
                (h - n(&place["pos"][1])) * k,
                &mut labels,
            );
        }
    }
    if let Some(p) = pending.filter(|p| p["zone"].is_string()) {
        let zone = text(&p["zone"]).trim().to_uppercase();
        if let Some(col) = zone.as_bytes().first().and_then(|c| c.checked_sub(b'A')) {
            if let Ok(row) = zone.get(1..).unwrap_or("").parse::<u64>() {
                if (col as u64) < z["cols"].as_u64().unwrap_or(0)
                    && row > 0
                    && row <= z["rows"].as_u64().unwrap_or(0)
                {
                    let x = col as f64 * zsize;
                    let y = (row - 1) as f64 * zsize;
                    ctx.set_fill_style_str("rgba(18,103,82,.16)");
                    ctx.fill_rect(x, y, zsize, zsize);
                    ctx.set_stroke_style_str("#126752");
                    ctx.set_line_width(4. * s);
                    ctx.stroke_rect(x + 2. * s, y + 2. * s, zsize - 4. * s, zsize - 4. * s);
                }
            }
        }
    }
    if let Some(p) = pending.filter(|p| p["x"].is_number() && p["y"].is_number()) {
        ctx.begin_path();
        ctx.arc(
            n(&p["x"]) * k,
            (h - n(&p["y"])) * k,
            8. * s,
            0.,
            std::f64::consts::TAU,
        )?;
        ctx.set_fill_style_str("#ffe000");
        ctx.fill();
    }
    scale_bar(&ctx, k * vw.z, h * k, size)?;
    Ok(())
}
/// Metric scale bar in screen space (bitmap pixels), drawn after the world transform is reset.
fn scale_bar(
    ctx: &CanvasRenderingContext2d,
    px_per_m: f64,
    bh: f64,
    size: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    let len_m = [10., 20., 50., 100., 200., 500., 1000., 2000., 5000., 10000.]
        .into_iter()
        .rev()
        .find(|m| m * px_per_m <= 170.)
        .unwrap_or(10.);
    ctx.set_transform(1., 0., 0., 1., 0., 0.)?;
    let l = len_m * px_per_m;
    let (x, y) = (size - 24. - l, bh - 18.);
    ctx.set_fill_style_str("rgba(255,255,255,.8)");
    ctx.fill_rect(x - 6., y - 22., l + 12., 32.);
    ctx.set_stroke_style_str("#122b2c");
    ctx.set_line_width(2.);
    ctx.begin_path();
    ctx.move_to(x, y - 6.);
    ctx.line_to(x, y);
    ctx.line_to(x + l, y);
    ctx.line_to(x + l, y - 6.);
    ctx.stroke();
    ctx.set_fill_style_str("#122b2c");
    ctx.set_font("14px system-ui");
    let label = if len_m >= 1000. {
        format!("{} km", len_m / 1000.)
    } else {
        format!("{len_m} m")
    };
    ctx.fill_text(&label, x + 2., y - 9.)
}
