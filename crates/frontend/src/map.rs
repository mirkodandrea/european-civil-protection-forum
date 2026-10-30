//! Interactive map: every dynamic layer comes from an authorized server view.
//! Zoom/pan, inspection and layer toggles are presentation only; they never change the world.
use crate::api::{list, text, window};
use base64::Engine;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{closure::Closure, Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, PointerEvent, ResizeObserver, WheelEvent,
};
use yew::prelude::*;

const SIZE: f64 = 1200.;
const MAX_ZOOM: f64 = 16.;
use crate::map_layers::{self as ml, Hidden};

/// Visible window in territory units: top-left `(ox, oy)`, width `Frame::fw / z`.
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
    /// Keep the territory in view: an axis wider than the territory centres
    /// it, a narrower one stays inside it.
    fn clamp(&mut self, f: Frame) {
        fn axis(o: f64, extent: f64, visible: f64) -> f64 {
            if visible >= extent {
                (extent - visible) / 2.
            } else {
                o.clamp(0., extent - visible)
            }
        }
        self.z = self.z.clamp(1., MAX_ZOOM);
        self.ox = axis(self.ox, SIZE, f.fw / self.z);
        self.oy = axis(self.oy, f.th, f.fh / self.z);
    }
    /// Zoom by `f` keeping the territory point under the fractional cursor `(fx, fy)` fixed.
    fn zoom_at(&mut self, f: f64, fx: f64, fy: f64, fr: Frame) {
        let px = self.ox + fx * fr.fw / self.z;
        let py = self.oy + fy * fr.fh / self.z;
        self.z = (self.z * f).clamp(1., MAX_ZOOM);
        self.ox = px - fx * fr.fw / self.z;
        self.oy = py - fy * fr.fh / self.z;
        self.clamp(fr);
    }
    fn pan(&mut self, dfx: f64, dfy: f64, fr: Frame) {
        self.ox -= dfx * fr.fw / self.z;
        self.oy -= dfy * fr.fh / self.z;
        self.clamp(fr);
    }
}
/// The map viewport in territory units (`SIZE` wide, `th` tall): `(fw, fh)` is
/// what the whole canvas shows at zoom 1. The canvas is any rectangle; the
/// territory keeps its proportions, fits at zoom 1 and the spare width or
/// height shows more terrain once zoomed in.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Frame {
    th: f64,
    fw: f64,
    fh: f64,
}
impl Frame {
    fn new(th: f64, css_w: f64, css_h: f64) -> Frame {
        if css_w <= 0. || css_h <= 0. || th <= 0. {
            return Frame { th, fw: SIZE, fh: th.max(1.) };
        }
        let a = css_w / css_h;
        if a >= SIZE / th {
            Frame { th, fw: th * a, fh: th }
        } else {
            Frame { th, fw: SIZE, fh: SIZE / a }
        }
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
    /// Centre and zoom on `(x, y)` metres whenever this changes (the third
    /// value only makes the same point trigger again).
    #[prop_or_default]
    pub focus: Option<(f64, f64, u32)>,
}
/// What a player's situation map draws: the role's `map` from the server
/// (fog of war, reports, estimates) with its own missions. The player page and
/// EXCON's "view as role" build it the same way.
pub fn player_view(map: &Value, deployments: &Value) -> Value {
    let mut view = map.clone();
    if view.is_object() {
        view["deployments"] = deployments.clone();
    }
    view
}

#[function_component(SituationMap)]
pub fn situation_map(props: &MapProps) -> Html {
    let canvas = use_node_ref();
    let frame = use_node_ref();
    let view = use_mut_ref(|| View::HOME);
    let drag = use_mut_ref(Drag::default);
    let tick = use_reducer(|| Tick(0));
    let hidden = use_state({
        let id = props.id.clone();
        move || ml::load(&id)
    });
    let hover = use_state(|| None::<(f64, f64)>);
    let pinned = use_state(|| None::<(f64, f64)>);
    let geometry = geom(&props.stat);
    let bh = geometry.map(|g| g.3).unwrap_or(SIZE);
    let bh_ref = use_mut_ref(|| Frame::new(bh, 0., 0.));
    if bh_ref.borrow().th != bh {
        *bh_ref.borrow_mut() = Frame::new(bh, 0., 0.);
    }
    let current = *view.borrow();
    let pan_zoom = props.pan_zoom;
    let pz_ref = use_mut_ref(|| pan_zoom);
    *pz_ref.borrow_mut() = pan_zoom;
    {
        let view = view.clone();
        let redraw = tick.dispatcher();
        let stat = props.stat.clone();
        let frame_ref = bh_ref.clone();
        use_effect_with(props.focus, move |focus| {
            if let (Some((x, y, _)), Some((_, h, k, _))) = (*focus, geom(&stat)) {
                let fr = *frame_ref.borrow();
                let mut v = view.borrow_mut();
                v.z = 3.;
                v.ox = x * k - fr.fw / (2. * v.z);
                v.oy = (h - y) * k - fr.fh / (2. * v.z);
                v.clamp(fr);
                drop(v);
                redraw.dispatch(());
            }
            || ()
        });
    }
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
                let frame_ref = bh_ref.clone();
                move |(stat, v, simple, pending, lang, _, hidden, (selected, marked))| {
                    if let Some(canvas) = canvas.cast::<HtmlCanvasElement>() {
                        // The viewport follows the canvas box; the view stays valid in it.
                        let rect = canvas.get_bounding_client_rect();
                        let fr = Frame::new(frame_ref.borrow().th, rect.width(), rect.height());
                        *frame_ref.borrow_mut() = fr;
                        view.borrow_mut().clamp(fr);
                        let _ = draw(
                            &canvas,
                            stat,
                            v,
                            *simple,
                            pending.as_ref(),
                            lang,
                            *view.borrow(),
                            fr,
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
                    let _ = el
                        .remove_event_listener_with_callback("wheel", cb.as_ref().unchecked_ref());
                }
            }
        });
    }
    {
        // Recompute screen-space labels when responsive CSS changes the canvas size.
        let canvas = canvas.clone();
        let redraw = tick.dispatcher();
        use_effect_with((), move |_| {
            let mut observer: Option<ResizeObserver> = None;
            let mut callback: Option<Closure<dyn FnMut(js_sys::Array, ResizeObserver)>> = None;
            if let Some(el) = canvas.cast::<HtmlCanvasElement>() {
                let rect = el.get_bounding_client_rect();
                let last_size = Rc::new(RefCell::new((rect.width(), rect.height())));
                let target = el.clone();
                let last = last_size.clone();
                let redraw = redraw.clone();
                let cb = Closure::<dyn FnMut(js_sys::Array, ResizeObserver)>::new(move |_, _| {
                    let rect = target.get_bounding_client_rect();
                    let mut previous = last.borrow_mut();
                    if (rect.width() - previous.0).abs() > 0.5
                        || (rect.height() - previous.1).abs() > 0.5
                    {
                        *previous = (rect.width(), rect.height());
                        redraw.dispatch(());
                    }
                });
                if let Ok(obs) = ResizeObserver::new(cb.as_ref().unchecked_ref()) {
                    obs.observe(el.unchecked_ref());
                    observer = Some(obs);
                    callback = Some(cb);
                }
            }
            let win = window();
            let fallback =
                Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(move |_| redraw.dispatch(()));
            let _ =
                win.add_event_listener_with_callback("resize", fallback.as_ref().unchecked_ref());
            move || {
                let _ = win.remove_event_listener_with_callback(
                    "resize",
                    fallback.as_ref().unchecked_ref(),
                );
                if let Some(obs) = observer {
                    obs.disconnect();
                }
                drop(callback);
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
            let fr = Frame::new(bh, rect.width(), rect.height());
            let v = *view.borrow();
            let bx = v.ox + (cx - rect.left()) / rect.width() * fr.fw / v.z;
            let by = v.oy + (cy - rect.top()) / rect.height() * fr.fh / v.z;
            if !(0. ..=SIZE).contains(&bx) || !(0. ..=bh).contains(&by) {
                return None;
            }
            Some((
                (bx / k).round().clamp(0., w - 1.),
                (h - by / k).round().clamp(0., h - 1.),
            ))
        }
    };
    let onclick = {
        let cb = props.onclick.clone();
        let stat = props.stat.clone();
        let drag = drag.clone();
        let pinned = pinned.clone();
        let locate = locate.clone();
        let canvas = canvas.clone();
        let view = view.clone();
        let hidden = *hidden;
        let simple = props.simple;
        Callback::from(move |e: MouseEvent| {
            if std::mem::take(&mut drag.borrow_mut().moved) > 5. {
                return;
            }
            if let Some((x, y)) = locate(e.client_x() as f64, e.client_y() as f64) {
                let h = n(&stat["grid"]["rows"]) * n(&stat["grid"]["cellsize"]);
                pinned.set(Some((x, y)));
                let mut payload = json!({"x":x,"y":y,"zone":zone_name(&stat, x, y, h)});
                let tol = canvas
                    .cast::<HtmlCanvasElement>()
                    .and_then(|c| {
                        let rect = c.get_bounding_client_rect();
                        let (_, _, k, bh) = geom(&stat)?;
                        let fr = Frame::new(bh, rect.width(), rect.height());
                        (rect.width() > 0.)
                            .then_some(12. / (k * view.borrow().z * rect.width() / fr.fw))
                    })
                    .unwrap_or(0.);
                let (building, road) = pick_static(&stat, x, y, tol, hidden, simple);
                if let Some((id, name)) = building {
                    payload["building"] = json!(name);
                    payload["building_id"] = json!(id);
                }
                if let Some(id) = road {
                    payload["road"] = json!(id);
                }
                cb.emit(payload);
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
            d.pts
                .push((e.pointer_id(), e.client_x() as f64, e.client_y() as f64));
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
        let bh_ref = bh_ref.clone();
        Callback::from(move |_: MouseEvent| {
            let bh = *bh_ref.borrow();
            if let Some((x0, y0, x1, y1)) = fire_box {
                let (cx, cy) = ((x0 + x1) / 2., (y0 + y1) / 2.);
                let pad = 1.4;
                let mut v = view.borrow_mut();
                v.z = (bh.fw / ((x1 - x0) * pad).max(1.)).min(bh.fh / ((y1 - y0) * pad).max(1.));
                v.z = v.z.clamp(1., MAX_ZOOM.min(6.));
                v.ox = cx - bh.fw / v.z / 2.;
                v.oy = cy - bh.fh / v.z / 2.;
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
    let probe = if props.inspect {
        (*hover).or(*pinned)
    } else {
        None
    };
    let lines = probe
        .map(|(x, y)| {
            probe_lines(
                &props.stat,
                &props.view,
                x,
                y,
                lang,
                props.simple,
                current.z,
            )
        })
        .unwrap_or_default();
    let clear_pin = {
        let pinned = pinned.clone();
        Callback::from(move |_: MouseEvent| pinned.set(None))
    };
    html! {<div class="mapbox"><div class="mapview" ref={frame}>
        <canvas id={props.id.clone()} ref={canvas} tabindex="0" {onclick} {ondblclick} {onpointerdown} {onpointermove} {onpointerup} onpointercancel={onpointerup_cancel(&drag)} {onpointerleave} {onkeydown}
            aria-label={if props.simple {t("map_tactical")} else {t("map_situation")}}/>
        if props.tools {<div class="map-tools" role="toolbar" aria-label={t("map_tools")}>
            <button type="button" title={t("map_zoom_in")} aria-label={t("map_zoom_in")} onclick={zoom_button(1.5)} disabled={current.z >= MAX_ZOOM}>{"+"}</button>
            <button type="button" title={t("map_zoom_out")} aria-label={t("map_zoom_out")} onclick={zoom_button(1. / 1.5)} disabled={current.z <= 1.}>{"−"}</button>
            <button type="button" title={t("map_reset")} aria-label={t("map_reset")} onclick={reset} disabled={current.z <= 1.0001}>{"⟲"}</button>
            <button type="button" title={t("map_fit_fire")} aria-label={t("map_fit_fire")} onclick={fit_fire} disabled={fire_box.is_none()}>{"🔥"}</button>
            <button type="button" title={t("map_fullscreen")} aria-label={t("map_fullscreen")} onclick={fullscreen}>{"⛶"}</button>
            <ml::LayerControl hidden={*hidden} lang={props.lang.clone()} onchange={{
                let (hidden, id) = (hidden.clone(), props.id.clone());
                Callback::from(move |h: Hidden| { ml::save(&id, h); hidden.set(h) })
            }}/>
        </div>}
        if !lines.is_empty() {<div class="map-probe" aria-live="polite">{for lines.iter().map(|l| html!{<div>{l.clone()}</div>})}if pinned.is_some() && hover.is_none() {<button type="button" aria-label={t("map_close_probe")} onclick={clear_pin}>{"✕"}</button>}</div>}
        if current.z > 1. {<div class="map-zoom-badge">{format!("×{:.1}", current.z)}</div>}
    </div><crate::map_legend::MapLegend lang={props.lang.clone()}/><p class="map-legend map-keys">{t("map_controls")}</p></div>}
}
fn onpointerup_cancel(drag: &Rc<RefCell<Drag>>) -> Callback<PointerEvent> {
    let drag = drag.clone();
    Callback::from(move |e: PointerEvent| {
        drag.borrow_mut().pts.retain(|p| p.0 != e.pointer_id());
    })
}
/// Localized fuel class of a raster byte, from the scenario's `fuel_names`
/// ("conifers", "shrubs - high susceptibility"); unknown names pass through.
fn fuel_label(lang: &str, stat: &Value, code: u8) -> String {
    use crate::i18n::pair;
    let Some(name) = stat["fuel_names"][code.to_string()].as_str() else {
        return code.to_string();
    };
    let (class, level) = name.split_once(" - ").unwrap_or((name, ""));
    let class = match class.trim() {
        "grassland" => pair(lang, "Prateria", "Grassland"),
        "broadleaves" => pair(lang, "Latifoglie", "Broadleaves"),
        "shrubs" => pair(lang, "Arbusteti", "Shrubs"),
        "conifers" => pair(lang, "Conifere", "Conifers"),
        "non-vegetated" | "not vegetated" => pair(lang, "Non vegetato", "Non-vegetated"),
        "agro-forestry" => pair(lang, "Agroforestale", "Agro-forestry"),
        "non-fire-prone forest" => pair(lang, "Bosco poco infiammabile", "Non-fire-prone forest"),
        _ => return name.to_string(),
    };
    let level = match level.trim() {
        "" => return class.to_string(),
        "low susceptibility" => pair(lang, "suscettività bassa", "low susceptibility"),
        "medium susceptibility" => pair(lang, "suscettività media", "medium susceptibility"),
        "high susceptibility" => pair(lang, "suscettività alta", "high susceptibility"),
        other => other,
    };
    format!("{class}, {level}")
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
    let (cols, rows) = (
        stat["zones"]["cols"].as_u64()?,
        stat["zones"]["rows"].as_u64()?,
    );
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
/// Pick the nearest public static feature within a screen-space tolerance.
fn pick_static(
    stat: &Value,
    x: f64,
    y: f64,
    tol: f64,
    hidden: Hidden,
    simple: bool,
) -> (Option<(String, String)>, Option<String>) {
    if tol <= 0. {
        return (None, None);
    }
    let mut best_building: Option<(String, String, f64)> = None;
    for b in list(&stat["buildings"]) {
        if hidden & ml::BUILDINGS != 0 || (simple && text(&b["name"]).is_empty()) {
            continue;
        }
        let (bx, by) = (n(&b["pos"][0]), n(&b["pos"][1]));
        let (hw, hh) = (n(&b["size"][0]) / 2., n(&b["size"][1]) / 2.);
        let dx = (x - bx).abs() - hw;
        let dy = (y - by).abs() - hh;
        let d = dx.max(0.).hypot(dy.max(0.));
        let id = b["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| b["id"].to_string());
        let name = if text(&b["name"]).is_empty() {
            format!("#{id}")
        } else {
            text(&b["name"]).to_string()
        };
        if d <= tol && best_building.as_ref().map_or(true, |item| d < item.2) {
            best_building = Some((id, name, d));
        }
    }
    let mut best_road: Option<(String, f64)> = None;
    for road in list(&stat["roads"]) {
        if hidden & ml::ROADS != 0 || (simple && road["drivable"] == false) {
            continue;
        }
        let points = list(&road["points"]);
        let mut d = f64::INFINITY;
        for pair in points.windows(2) {
            d = d.min(seg_dist(
                (x, y),
                (n(&pair[0][0]), n(&pair[0][1])),
                (n(&pair[1][0]), n(&pair[1][1])),
            ));
        }
        let id = road["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| road["id"].to_string());
        if d <= tol && best_road.as_ref().map_or(true, |item| d < item.1) {
            best_road = Some((id, d));
        }
    }
    (
        best_building.map(|(id, name, _)| (id, name)),
        best_road.map(|(id, _)| id),
    )
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
            tf(lang, "insp_fuel", &[&fuel_label(lang, stat, fuel)]),
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
                line += &format!(
                    " · {}",
                    tf(
                        lang,
                        "insp_inside",
                        &[&s["inside"].to_string(), &s["evacuated"].to_string()]
                    )
                );
            }
            let flag = match (
                s["damage"].as_u64(),
                s["evacuating"] == true,
                s["threatened"] == true,
            ) {
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
        let pts: Vec<(f64, f64)> = list(&road["points"])
            .iter()
            .map(|p| (n(&p[0]), n(&p[1])))
            .collect();
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
    for d in list(&v["deployments"])
        .iter()
        .chain(list(&v["observed_units"]))
    {
        let tk = &d["task"];
        if d["active_qty"] == 0 || tk["status"] == "cancelled" {
            continue;
        }
        if (n(&d["position_m"][0]) - x).hypot(n(&d["position_m"][1]) - y) <= tol * 1.5 {
            out.push(tf(
                lang,
                "insp_unit",
                &[
                    &tk["id"].to_string(),
                    text(&tk["status"]),
                    text(&tk["detail"]),
                ],
            ));
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
/// Fill the cells in `state` with a screen-constant texture (see `map_legend` swatches).
fn texture(
    ctx: &CanvasRenderingContext2d,
    states: &[u8],
    state: u8,
    cols: usize,
    rows: usize,
    width: f64,
    height: f64,
    u: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    if !states.contains(&state) {
        return Ok(());
    }
    let doc = window().document().unwrap();
    let new_canvas = |w: u32, h: u32| -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), wasm_bindgen::JsValue> {
        let c = doc.create_element("canvas")?.dyn_into::<HtmlCanvasElement>()?;
        c.set_width(w);
        c.set_height(h);
        let x = c.get_context("2d")?.unwrap().dyn_into::<CanvasRenderingContext2d>()?;
        Ok((c, x))
    };
    let tile = (10. * u).round().max(2.);
    let (tc, tx) = new_canvas(tile as u32, tile as u32)?;
    let lw = (1.6 * u).max(1.);
    tx.set_line_width(lw);
    match state {
        1 => {
            tx.set_fill_style_str("rgba(255,244,170,.95)");
            tx.begin_path();
            tx.arc(tile / 2., tile / 2., (1.8 * u).max(1.), 0., std::f64::consts::TAU)?;
            tx.fill();
        }
        2 => {
            tx.set_stroke_style_str("rgba(255,255,255,.5)");
            tx.begin_path();
            tx.move_to(0., tile);
            tx.line_to(tile, 0.);
            tx.stroke();
        }
        _ => {
            tx.set_stroke_style_str("rgba(255,255,255,.8)");
            tx.begin_path();
            tx.move_to(0., tile / 2.);
            tx.line_to(tile, tile / 2.);
            tx.stroke();
        }
    }
    let (mc, mx) = new_canvas(cols as u32, rows as u32)?;
    let mut mask = vec![0u8; cols * rows * 4];
    for (i, st) in states.iter().enumerate().take(cols * rows) {
        if *st == state {
            mask[i * 4 + 3] = 255;
        }
    }
    mx.put_image_data(
        &web_sys::ImageData::new_with_u8_clamped_array_and_sh(Clamped(&mask), cols as u32, rows as u32)?,
        0.,
        0.,
    )?;
    let (oc, ox) = new_canvas(width as u32, height as u32)?;
    if let Some(pattern) = ox.create_pattern_with_html_canvas_element(&tc, "repeat")? {
        ox.set_fill_style_canvas_pattern(&pattern);
        ox.fill_rect(0., 0., width, height);
    }
    ox.set_image_smoothing_enabled(false);
    ox.set_global_composite_operation("destination-in")?;
    ox.draw_image_with_html_canvas_element_and_dw_and_dh(&mc, 0., 0., width, height)?;
    ctx.draw_image_with_html_canvas_element(&oc, 0., 0.)?;
    Ok(())
}
/// Amber diagonal hatching and outline over one warned zone (territory units).
fn warned_zone(
    ctx: &CanvasRenderingContext2d,
    x0: f64,
    y0: f64,
    zs: f64,
    s: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    ctx.save();
    ctx.begin_path();
    ctx.rect(x0, y0, zs, zs);
    ctx.clip();
    ctx.set_fill_style_str("rgba(255,200,0,.14)");
    ctx.fill_rect(x0, y0, zs, zs);
    ctx.set_stroke_style_str("rgba(208,138,0,.6)");
    ctx.set_line_width(2. * s);
    ctx.begin_path();
    let step = zs / 7.;
    let mut d = step;
    while d < 2. * zs {
        ctx.move_to(x0 + d, y0);
        ctx.line_to(x0 + d - zs, y0 + zs);
        d += step;
    }
    ctx.stroke();
    ctx.restore();
    ctx.set_stroke_style_str("#d08a00");
    ctx.set_line_width(3. * s);
    ctx.stroke_rect(x0 + 1.5 * s, y0 + 1.5 * s, zs - 3. * s, zs - 3. * s);
    Ok(())
}
/// "Day 1 14:20" -> "14:20".
fn short_clock(clock: &str) -> &str {
    clock.rsplit(' ').next().unwrap_or(clock)
}
/// A label collected during the world pass; `x`/`y` are bitmap-space anchor coordinates.
struct LabelReq {
    text: String,
    x: f64,
    y: f64,
    /// 0 critical (school, hospital, reported fire, affected buildings), 1 other named
    /// buildings, closures and units, 2 places. Zone codes are priority 3 and always drawn.
    prio: u8,
}
/// Shape badge (not colour alone) marking a building status; bitmap-space centre.
struct Badge {
    x: f64,
    y: f64,
    glyph: &'static str,
    color: &'static str,
}
/// Label font size in CSS pixels: compact at overview, growing gently with map zoom.
fn label_font_px(zoom: f64) -> f64 {
    (12. + 1.1 * zoom.max(1.).ln()).min(14.5)
}
/// Zone codes: small, light, always drawn.
const ZONE_FONT: &str = "600 10px system-ui";
/// A zone cell this wide on screen (CSS px) also shows its own code.
const CELL_CODES_PX: f64 = 150.;
/// A zone code on screen: an axis letter/number, or a cell's own code.
#[derive(Debug, PartialEq)]
struct ZoneMark {
    text: String,
    x: f64,
    y: f64,
    cell: bool,
}
/// Where zone codes go for zones `zs` CSS px wide whose territory starts at
/// `origin` in a `cw`×`ch` viewport: letters along the top edge and numbers
/// along the left edge of the visible territory; once cells are at least
/// [`CELL_CODES_PX`] wide, each visible cell's code in its corner instead.
fn zone_marks(zs: f64, origin: (f64, f64), (cols, rows): (u64, u64), (cw, ch): (f64, f64)) -> Vec<ZoneMark> {
    let mut out = Vec::new();
    if zs <= 0. {
        return out;
    }
    let (right, bottom) = (origin.0 + cols as f64 * zs, origin.1 + rows as f64 * zs);
    let (left, top) = (origin.0.max(0.), origin.1.max(0.));
    let (right, bottom) = (right.min(cw), bottom.min(ch));
    if right - left < 10. || bottom - top < 10. {
        return out;
    }
    let (ay, ax) = (top + 8., left + 8.);
    if zs >= CELL_CODES_PX {
        // Zoomed in: each visible cell names itself; the axes would repeat it.
        for rw in 0..rows {
            for c in 0..cols {
                let (l, t) = ((origin.0 + c as f64 * zs).max(left), (origin.1 + rw as f64 * zs).max(top));
                let (r, b) = ((origin.0 + (c + 1) as f64 * zs).min(right), (origin.1 + (rw + 1) as f64 * zs).min(bottom));
                if r - l >= 60. && b - t >= 40. {
                    out.push(ZoneMark { text: format!("{}{}", (b'A' + c as u8) as char, rw + 1), x: l + 22., y: t + 22., cell: true });
                }
            }
        }
        return out;
    }
    for c in 0..cols {
        let (l, r) = ((origin.0 + c as f64 * zs).max(left), (origin.0 + (c + 1) as f64 * zs).min(right));
        if r - l >= 14. {
            out.push(ZoneMark { text: ((b'A' + c as u8) as char).to_string(), x: (l + r) / 2., y: ay, cell: false });
        }
    }
    for rw in 0..rows {
        let (t, b) = ((origin.1 + rw as f64 * zs).max(top), (origin.1 + (rw + 1) as f64 * zs).min(bottom));
        if b - t >= 14. && (t + b) / 2. > ay + 10. {
            out.push(ZoneMark { text: (rw + 1).to_string(), x: ax, y: (t + b) / 2., cell: false });
        }
    }
    out
}
/// A label as shown at `zoom`: long names are shortened at overview zoom.
fn label_text(text: &str, zoom: f64) -> String {
    let max = if zoom < 1.6 { 18 } else if zoom < 3. { 28 } else { usize::MAX };
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}
/// Fewer labels at overview zoom: descriptive places (more than two words,
/// e.g. "Margine nord di Roverechiare") wait until the map is zoomed in.
fn label_shown(text: &str, prio: u8, zoom: f64) -> bool {
    prio < 2 || zoom >= 1.4 || text.split_whitespace().count() <= 2
}
/// Draw badges, labels (collision-free, by priority) and zone codes in CSS-pixel space.
fn draw_labels(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    vw: View,
    css_per_unit: f64,
    dpr: f64,
    zsize: f64,
    (zcols, zrows): (u64, u64),
    reqs: &[LabelReq],
    badges: &[Badge],
    avoid: &[(f64, f64, f64, f64)],
) -> Result<(), wasm_bindgen::JsValue> {
    use crate::label_layout::{place, Item, Rect};
    let rect = canvas.get_bounding_client_rect();
    if rect.width() <= 0. || rect.height() <= 0. {
        return Ok(());
    }
    let (cw, ch) = (rect.width(), rect.height());
    ctx.set_transform(dpr, 0., 0., dpr, 0., 0.)?;
    let to_screen = |x: f64, y: f64| ((x - vw.ox) * css_per_unit, (y - vw.oy) * css_per_unit);
    let visible = |p: (f64, f64)| p.0 >= 0. && p.1 >= 0. && p.0 <= cw && p.1 <= ch;
    let fs = label_font_px(vw.z);
    let h = fs + 5.;
    let mut items: Vec<Item> = Vec::new();
    let mut texts: Vec<(String, bool)> = Vec::new();
    let mut markers: Vec<Rect> = vec![Rect { x: cw - 140., y: ch - 30., w: 140., h: 30. }];
    for b in badges {
        let p = to_screen(b.x, b.y);
        if visible(p) {
            markers.push(Rect { x: p.0 - 8., y: p.1 - 8., w: 16., h: 16. });
        }
    }
    for &(x0, y0, x1, y1) in avoid {
        let (a, b) = (to_screen(x0, y0), to_screen(x1, y1));
        markers.push(Rect { x: a.0, y: a.1, w: b.0 - a.0, h: b.1 - a.1 });
    }
    let mut seen: Vec<(String, (f64, f64))> = Vec::new();
    for r in reqs {
        let p = to_screen(r.x, r.y);
        if !visible(p) || !label_shown(&r.text, r.prio, vw.z) {
            continue;
        }
        // A place and a building with the same name at the same spot are one label.
        let key = r.text.to_lowercase();
        if seen
            .iter()
            .any(|(t, q)| *t == key && (q.0 - p.0).abs() < 40. && (q.1 - p.1).abs() < 40.)
        {
            continue;
        }
        seen.push((key, p));
        ctx.set_font(&format!("{} {fs}px system-ui", if r.prio == 0 { 650 } else { 500 }));
        let shown = label_text(&r.text, vw.z);
        let w = ctx.measure_text(&shown)?.width() + 8.;
        markers.push(Rect { x: p.0 - 5., y: p.1 - 5., w: 10., h: 10. });
        items.push(Item { w, h, anchor: p, prio: r.prio, forced: false, cands: vec![] });
        texts.push((shown, r.prio == 0));
    }
    // Zone codes: column letters along the top and row numbers along the left
    // edge of the visible territory (like chart axes); per-cell codes only when
    // a cell is large on screen. No plates: small text with a thin halo.
    let origin = to_screen(0., 0.);
    let marks = zone_marks(zsize * css_per_unit, origin, (zcols, zrows), (cw, ch));
    for m in &marks {
        markers.push(Rect { x: m.x - 8., y: m.y - 7., w: 16., h: 14. });
    }
    let placed = place(&items, (cw, ch), &markers);
    ctx.set_font(ZONE_FONT);
    ctx.set_text_baseline("middle");
    ctx.set_line_join("round");
    ctx.set_line_width(3.);
    ctx.set_stroke_style_str("rgba(255,255,255,.8)");
    ctx.set_fill_style_str("rgba(18,43,44,.72)");
    for m in &marks {
        ctx.set_text_align(if m.cell { "left" } else { "center" });
        ctx.stroke_text(&m.text, m.x, m.y)?;
        ctx.fill_text(&m.text, m.x, m.y)?;
    }
    // Painting order: labels by descending priority.
    let mut order: Vec<usize> = (0..items.len()).filter(|&i| placed[i].is_some()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(items[i].prio));
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_line_join("round");
    for &i in &order {
        let Some(pl) = placed[i] else { continue };
        let (txt, strong) = (&texts[i].0, texts[i].1);
        let rc = pl.rect;
        if let Some((ax, ay)) = pl.leader {
            let (nx, ny) = rc.nearest(ax, ay);
            ctx.begin_path();
            ctx.move_to(ax, ay);
            ctx.line_to(nx, ny);
            ctx.set_stroke_style_str("rgba(18,43,44,.7)");
            ctx.set_line_width(1.);
            ctx.stroke();
            ctx.begin_path();
            ctx.arc(ax, ay, 2., 0., std::f64::consts::TAU)?;
            ctx.set_fill_style_str("#122b2c");
            ctx.fill();
        }
        // Light plates: names translucent, only critical names outlined.
        ctx.set_fill_style_str(if strong {
            "rgba(255,255,255,.9)"
        } else {
            "rgba(255,255,255,.72)"
        });
        ctx.fill_rect(rc.x, rc.y, rc.w, rc.h);
        if strong {
            ctx.set_stroke_style_str("rgba(18,43,44,.75)");
            ctx.set_line_width(1.);
            ctx.stroke_rect(rc.x + 0.5, rc.y + 0.5, rc.w - 1., rc.h - 1.);
        }
        ctx.set_fill_style_str("#122b2c");
        ctx.set_font(&format!("{} {fs}px system-ui", if strong { 650 } else { 500 }));
        ctx.fill_text(txt, rc.x + 4., rc.y + rc.h / 2. + 0.5)?;
    }
    // Status badges on top, with a white halo.
    ctx.set_font("700 15px system-ui");
    ctx.set_text_align("center");
    for b in badges {
        let p = to_screen(b.x, b.y);
        if visible(p) {
            ctx.set_stroke_style_str("#fff");
            ctx.set_line_width(4.);
            ctx.stroke_text(b.glyph, p.0, p.1)?;
            ctx.set_fill_style_str(b.color);
            ctx.fill_text(b.glyph, p.0, p.1)?;
        }
    }
    ctx.set_text_align("start");
    Ok(())
}
fn draw(
    canvas: &HtmlCanvasElement,
    stat: &Value,
    v: &Value,
    simple: bool,
    pending: Option<&Value>,
    lang: &str,
    vw: View,
    fr: Frame,
    hidden: Hidden,
    selected: &[String],
    marked: &[String],
) -> Result<(), wasm_bindgen::JsValue> {
    let on = |bit: Hidden| hidden & bit == 0;
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
    // The canvas bitmap matches its box (sharp at any size and pixel ratio);
    // `sc` maps territory units to canvas pixels.
    let rect = canvas.get_bounding_client_rect();
    let (css_w, css_h) = if rect.width() > 0. && rect.height() > 0. {
        (rect.width(), rect.height())
    } else {
        (size, h * k)
    };
    let dpr = window().device_pixel_ratio().clamp(1., 2.);
    canvas.set_width((css_w * dpr).round() as u32);
    canvas.set_height((css_h * dpr).round() as u32);
    let sc = css_w * dpr / fr.fw * z;
    let ctx = canvas
        .get_context("2d")?
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()?;
    // Outside the territory: a quiet neutral margin.
    ctx.set_fill_style_str("#dfe6e1");
    ctx.fill_rect(0., 0., css_w * dpr, css_h * dpr);
    ctx.set_transform(sc, 0., 0., sc, -vw.ox * sc, -vw.oy * sc)?;
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
        let bg = if !on(ml::BASE) {
            ml::PLAIN_BG
        } else {
            match fuel.get(i).copied().unwrap_or(0) {
            1 => [107, 142, 78],
            2 => [154, 165, 90],
            3 => [214, 208, 196],
            4 => [217, 210, 138],
            5 => [63, 107, 58],
            6 => [194, 183, 110],
            7 => [127, 163, 122],
            _ => [200, 200, 190],
            }
        };
        let code = fire.get(i).copied().unwrap_or(0);
        let fg = if ml::fire_code_shown(code, hidden) {
            colour(code)
        } else {
            [0; 4]
        };
        let a = fg[3] as f64 / 255.;
        for c in 0..3 {
            rgba[i * 4 + c] = (bg[c] as f64 * (1. - a) + fg[c] as f64 * a) as u8;
        }
        rgba[i * 4 + 3] = 255;
    }
    let mut states: Vec<u8> = fire
        .iter()
        .map(|&c| if ml::fire_code_shown(c, hidden) { c } else { 0 })
        .collect();
    states.resize(cols * rows, 0);
    for obs in list(&v["cells"])
        .iter()
        .filter(|o| ml::fire_code_shown(o["state"].as_u64().unwrap_or(0) as u8, hidden))
    {
        let i = (obs["row"].as_u64().unwrap_or(rows as u64) as usize * cols
            + obs["col"].as_u64().unwrap_or(cols as u64) as usize)
            * 4;
        if i + 4 <= rgba.len() {
            let fg = colour(obs["state"].as_u64().unwrap_or(0) as u8);
            if fg[3] > 0 {
                rgba[i..i + 4].copy_from_slice(&fg);
                states[i / 4] = obs["state"].as_u64().unwrap_or(0) as u8;
            }
        }
    }
    // Labels must not hide the active front: blocks of cells with active fire become obstacles.
    let mut front: Vec<(f64, f64, f64, f64)> = Vec::new();
    if on(ml::FIRE) && cols > 0 && rows > 0 {
        const B: usize = 6;
        let (cw, chh) = (size / cols as f64, h * k / rows as f64);
        for br in (0..rows).step_by(B) {
            for bc in (0..cols).step_by(B) {
                let hot = (br..(br + B).min(rows))
                    .any(|r| (bc..(bc + B).min(cols)).any(|c| states[r * cols + c] == 1));
                if hot {
                    front.push((bc as f64 * cw, br as f64 * chh, (bc + B).min(cols) as f64 * cw, (br + B).min(rows) as f64 * chh));
                }
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
    {
        // Texture per fire state so the legend does not rely on colour alone:
        // dots = active fire, diagonal hatch = burnt, horizontal lines = water/retardant.
        for state in [1u8, 2, 4] {
            texture(&ctx, &states, state, cols, rows, size, h * k, u)?;
        }
    }
    // Labels are only collected here; they are placed in screen space after the world
    // pass (see `crate::label_layout`) so they never overlap and keep a readable size.
    let reqs = RefCell::new(Vec::<LabelReq>::new());
    let badges = RefCell::new(Vec::<Badge>::new());
    let label = |s: &str, x: f64, y: f64, prio: u8| {
        if on(ml::LABELS) {
            reqs.borrow_mut().push(LabelReq {
                text: s.to_string(),
                x,
                y,
                prio,
            });
        }
    };
    let z = &stat["zones"];
    let zsize = n(&z["size_m"]) * k;
    let (zcols, zrows) = (
        z["cols"].as_u64().unwrap_or(0),
        z["rows"].as_u64().unwrap_or(0),
    );
    if let Some(known) = fog_zones(stat, v).filter(|_| on(ml::FOG)) {
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
    // A drone on station keeps its zones live: tinted, dashed footprint; the rest of what
    // the role knows stays a snapshot, slightly veiled while a drone is flying.
    let live: Vec<(u64, u64)> = list(&v["live_zones"])
        .iter()
        .filter_map(|name| parse_zone(text(name), zcols, zrows))
        .collect();
    if on(ml::UNITS) && !live.is_empty() {
        if let Some(known) = fog_zones(stat, v) {
            ctx.set_fill_style_str("rgba(24,36,40,.14)");
            for row in 0..zrows {
                for col in 0..zcols {
                    if known[(row * zcols + col) as usize] && !live.contains(&(col, row)) {
                        ctx.fill_rect(col as f64 * zsize, row as f64 * zsize, zsize, zsize);
                    }
                }
            }
        }
        ctx.set_fill_style_str("rgba(0,160,190,.12)");
        ctx.set_stroke_style_str("#008c9e");
        ctx.set_line_width(3. * s);
        ctx.set_line_dash(&js_sys::Array::of2(&(10. * s).into(), &(6. * s).into()))?;
        for &(col, row) in &live {
            let (x, y) = (col as f64 * zsize, row as f64 * zsize);
            ctx.fill_rect(x, y, zsize, zsize);
            // Outline only the sides that border a zone the drone does not cover.
            for (dc, dr, x0, y0, x1, y1) in [
                (0i64, -1i64, x, y, x + zsize, y),
                (0, 1, x, y + zsize, x + zsize, y + zsize),
                (-1, 0, x, y, x, y + zsize),
                (1, 0, x + zsize, y, x + zsize, y + zsize),
            ] {
                let next = ((col as i64 + dc) as u64, (row as i64 + dr) as u64);
                if !live.contains(&next) {
                    ctx.begin_path();
                    ctx.move_to(x0, y0);
                    ctx.line_to(x1, y1);
                    ctx.stroke();
                }
            }
        }
        ctx.set_line_dash(&js_sys::Array::new())?;
    }
    for (names, fill, dash) in [
        (marked, "rgba(18,103,82,.10)", true),
        (selected, "rgba(18,103,82,.28)", false),
    ] {
        for name in names {
            if let Some((col, row)) = parse_zone(name, zcols, zrows) {
                let (x, y) = (col as f64 * zsize, row as f64 * zsize);
                ctx.set_fill_style_str(fill);
                ctx.fill_rect(x, y, zsize, zsize);
                if !dash {
                    // Light halo: the selection stays readable over a fogged (dark) player map.
                    ctx.set_stroke_style_str("rgba(255,255,255,.85)");
                    ctx.set_line_width(7. * s);
                    ctx.stroke_rect(x + 2. * s, y + 2. * s, zsize - 4. * s, zsize - 4. * s);
                }
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
    ctx.set_stroke_style_str("rgba(0,0,0,.12)");
    for col in (0..=z["cols"].as_u64().unwrap_or(0)).filter(|_| on(ml::GRID)) {
        ctx.begin_path();
        ctx.move_to(col as f64 * zsize, 0.);
        ctx.line_to(col as f64 * zsize, h * k);
        ctx.stroke();
    }
    for row in (0..=z["rows"].as_u64().unwrap_or(0)).filter(|_| on(ml::GRID)) {
        ctx.begin_path();
        ctx.move_to(0., row as f64 * zsize);
        ctx.line_to(size, row as f64 * zsize);
        ctx.stroke();
    }
    // Warned zones: amber hatching and outline, with a bell label and the
    // time of the warning (persistent until rollback).
    for row in 0..z["rows"].as_u64().unwrap_or(0) {
        for col in 0..z["cols"].as_u64().unwrap_or(0) {
            let name = format!("{}{}", (b'A' + col as u8) as char, row + 1);
            if on(ml::CLOSURES) && list(&v["warned"]).contains(&json!(name)) {
                let (x0, y0) = (col as f64 * zsize, row as f64 * zsize);
                warned_zone(&ctx, x0, y0, zsize, s)?;
                let since = list(&v["warnings"])
                    .iter()
                    .find(|w| w["zone"] == json!(name))
                    .map(|w| short_clock(text(&w["clock"])).to_string())
                    .unwrap_or_default();
                // Compact at overview (labels are cut at 18 characters); the word when zoomed.
                let head = if vw.z >= 1.6 {
                    format!("{} ", crate::map_legend::lg(lang, "warned_short"))
                } else {
                    String::new()
                };
                let txt = if since.is_empty() {
                    format!("🔔 {head}{name}")
                } else {
                    format!("🔔 {head}{name} · {since}")
                };
                label(&txt, x0 + zsize / 2., y0 + zsize / 2., 0);
            }
        }
    }
    ctx.set_line_join("round");
    ctx.set_line_cap("round");
    for road in list(&stat["roads"])
        .iter()
        .filter(|_| on(ml::ROADS) || on(ml::CLOSURES))
    {
        if simple && road["drivable"] == false {
            continue;
        }
        let points = list(&road["points"]);
        trace(&ctx, points, k, h);
        if on(ml::ROADS) {
            ctx.set_stroke_style_str("#29363d");
            ctx.set_line_width(6. * s);
            ctx.stroke();
            ctx.set_stroke_style_str("#e6e4d8");
            ctx.set_line_width(3. * s);
            ctx.stroke();
        }
        if !on(ml::CLOSURES) {
            continue;
        }
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
                    &format!(
                        "{}: {}",
                        if cut {
                            crate::i18n::t(lang, "map_road_cut")
                        } else {
                            crate::map_legend::lg(lang, "police_closure")
                        },
                        text(&road["name"])
                    ),
                    n(&segment["from"][0]) * k,
                    (h - n(&segment["from"][1])) * k,
                    1,
                );
                marked = true;
            }
        }
    }
    for (i, b) in list(&stat["buildings"]).iter().enumerate() {
        if !on(ml::BUILDINGS) || (simple && text(&b["name"]).is_empty()) {
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
        // Shape badges (drawn in screen space) repeat the status without relying on colour.
        let glyph = if state["damage"] == 2 {
            Some(("✕", "#000"))
        } else if state["damage"] == 1 {
            Some(("◆", "#b34700"))
        } else if state["evacuating"] == true {
            Some(("➜", "#00668f"))
        } else if state["threatened"] == true {
            Some(("▲", "#8a5200"))
        } else if fill == "#2e6bff" {
            Some(("○", "#1a45c8"))
        } else {
            None
        };
        if let Some((glyph, color)) = glyph {
            badges.borrow_mut().push(Badge { x, y, glyph, color });
        }
        if state["threatened"] == true {
            ctx.set_stroke_style_str("#ffb000");
            ctx.set_line_width(2. * s);
            ctx.set_line_dash(&js_sys::Array::of2(&(4. * s).into(), &(3. * s).into()))?;
            ctx.stroke_rect(
                x - bw / 2. - 2. * s,
                y - bh / 2. - 2. * s,
                bw + 4. * s,
                bh + 4. * s,
            );
            ctx.set_line_dash(&js_sys::Array::new())?;
        }
    }
    for line in list(&v["lines"]).iter().filter(|_| on(ml::WORKS)) {
        trace(&ctx, &[line["from"].clone(), line["to"].clone()], k, h);
        ctx.set_stroke_style_str("#ffe000");
        ctx.set_line_width(3. * s);
        ctx.set_line_dash(&js_sys::Array::of2(&(7. * s).into(), &(4. * s).into()))?;
        ctx.stroke();
        ctx.set_line_dash(&js_sys::Array::new())?;
    }
    for drop in list(&v["drops"]).iter().filter(|_| on(ml::WORKS)) {
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
    for location in list(&v["reported_locations"]).iter().filter(|_| on(ml::ESTIMATES)) {
        let x = n(&location[0]) * k;
        let y = (h - n(&location[1])) * k;
        ctx.begin_path();
        ctx.arc(x, y, 9. * s, 0., std::f64::consts::TAU)?;
        ctx.set_stroke_style_str("#e33");
        ctx.stroke();
        label(crate::i18n::t(lang, "map_reported"), x, y, 0);
    }
    // Unverified fire estimates: a dashed uncertainty circle, the spread direction and who said it.
    for e in list(&v["estimates"]).iter().filter(|_| on(ml::ESTIMATES)) {
        let (x, y, r) = (n(&e["at"][0]) * k, (h - n(&e["at"][1])) * k, (n(&e["radius_m"]) * k).max(14. * s));
        ctx.set_stroke_style_str("#e33");
        ctx.set_line_width(2.5 * s);
        ctx.set_line_dash(&js_sys::Array::of2(&(9. * s).into(), &(6. * s).into()))?;
        ctx.begin_path();
        ctx.arc(x, y, r, 0., std::f64::consts::TAU)?;
        ctx.stroke();
        ctx.set_line_dash(&js_sys::Array::new())?;
        if let Some(bearing) = e["spread_toward_deg"].as_f64().map(f64::to_radians) {
            let (dx, dy) = (bearing.sin(), -bearing.cos());
            let len = (r * 0.8).max(26. * s);
            let (tx, ty) = (x + dx * len, y + dy * len);
            ctx.begin_path();
            ctx.move_to(x, y);
            ctx.line_to(tx, ty);
            for side in [-0.45f64, 0.45] {
                let (c, sn) = (side.cos(), side.sin());
                ctx.move_to(tx, ty);
                ctx.line_to(tx - (dx * c - dy * sn) * 11. * s, ty - (dx * sn + dy * c) * 11. * s);
            }
            ctx.stroke();
        }
        let area = n(&e["area_ha"]);
        label(
            &format!("{} · ~{} ha", crate::i18n::tf(lang, "map_estimate", &[text(&e["source"]), text(&e["clock"])]), if area < 10. { format!("{area:.1}") } else { format!("{area:.0}") }),
            x,
            y - r,
            0,
        );
    }
    if let Some(forecast) = list(&v["forecast_products"])
        .last()
        .filter(|_| on(ml::FORECAST))
    {
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
    // The role's own drone: position known from its telemetry, whatever the fog.
    for d in list(&v["drones"]).iter().filter(|_| on(ml::UNITS)) {
        let (x, y) = (n(&d["position_m"][0]) * k, (h - n(&d["position_m"][1])) * k);
        let status = text(&d["status"]);
        let color = match status {
            "on_station" => "#008c9e",
            "en_route" => "#ad6912",
            _ => "#7a7f85",
        };
        ctx.set_stroke_style_str(color);
        ctx.set_fill_style_str("rgba(255,255,255,.85)");
        ctx.set_line_width(2.5 * s);
        ctx.begin_path();
        ctx.arc(x, y, 7. * s, 0., std::f64::consts::TAU)?;
        ctx.fill();
        ctx.stroke();
        for (dx, dy) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
            ctx.begin_path();
            ctx.arc(x + dx * 11. * s, y + dy * 11. * s, 4. * s, 0., std::f64::consts::TAU)?;
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(x + dx * 5. * s, y + dy * 5. * s);
            ctx.line_to(x + dx * 8. * s, y + dy * 8. * s);
            ctx.stroke();
        }
        let status_key = match status {
            "on_station" => "map_drone_on_station",
            "en_route" => "map_drone_en_route",
            _ => "map_drone_returning",
        };
        label(&format!("{} · {}", crate::i18n::t(lang, "map_drone"), crate::i18n::t(lang, status_key)), x, y, 1);
    }
    // Drones have their own marker above; do not draw them again as a generic unit.
    let drone_tasks: Vec<&Value> = list(&v["drones"]).iter().map(|d| &d["task"]).collect();
    for d in list(&v["deployments"])
        .iter()
        .chain(list(&v["observed_units"]))
        .filter(|_| on(ml::UNITS))
    {
        let t = &d["task"];
        if d["active_qty"] == 0 || t["status"] == "cancelled" || drone_tasks.contains(&&t["id"]) {
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
        label(&format!("#{} {}", t["id"], text(&t["status"])), x, y, 1);
    }
    for (i, b) in list(&stat["buildings"]).iter().enumerate() {
        if let Some(name) = b["name"].as_str().filter(|s| !s.is_empty()) {
            let st = &v["buildings"][i];
            let critical = matches!(
                text(&b["kind"]),
                "school" | "hospital" | "fuel_station" | "town_hall"
            ) || st["damage"].as_u64().unwrap_or(0) > 0
                || st["threatened"] == true
                || st["evacuating"] == true;
            label(
                name,
                n(&b["pos"][0]) * k,
                (h - n(&b["pos"][1])) * k,
                if critical { 0 } else { 1 },
            );
        }
    }
    for place in list(&stat["places"]) {
        let name = text(&place["name"]);
        if !name.contains(" edge") && !name.contains("junction") {
            label(
                name,
                n(&place["pos"][0]) * k,
                (h - n(&place["pos"][1])) * k,
                2,
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
    // Points picked by an action form (one point, the two ends of a line, or
    // the vertices of a drawn area when `closed`).
    if let Some(p) = pending.filter(|p| p["points"].is_array()) {
        let pts: Vec<(f64, f64)> = list(&p["points"])
            .iter()
            .filter_map(|q| Some((q[0].as_f64()? * k, (h - q[1].as_f64()?) * k)))
            .collect();
        let closed = p["closed"] == true && pts.len() >= 3;
        if pts.len() >= 2 {
            ctx.begin_path();
            ctx.move_to(pts[0].0, pts[0].1);
            for q in &pts[1..] {
                ctx.line_to(q.0, q.1);
            }
            if closed {
                ctx.close_path();
                ctx.set_fill_style_str("rgba(40,120,220,.22)");
                ctx.fill();
            }
            ctx.set_stroke_style_str("#122b2c");
            ctx.set_line_width(7. * s);
            ctx.stroke();
            ctx.set_stroke_style_str("#ffe000");
            ctx.set_line_width(4. * s);
            ctx.stroke();
        }
        for (i, (x, y)) in pts.iter().enumerate() {
            ctx.begin_path();
            ctx.arc(*x, *y, 8. * s, 0., std::f64::consts::TAU)?;
            ctx.set_fill_style_str("#ffe000");
            ctx.fill();
            ctx.set_stroke_style_str("#122b2c");
            ctx.set_line_width(2. * s);
            ctx.stroke();
            label(&(i + 1).to_string(), *x, *y, 0);
        }
    }
    // A zone edge (`F4:N`) picked for a fire line.
    if let Some(edge) = pending.and_then(|p| p["edge"].as_str()) {
        if let Some((zone, side)) = edge.split_once(':') {
            if let Some((col, row)) = parse_zone(zone, zcols, zrows) {
                let (x0, y0) = (col as f64 * zsize, row as f64 * zsize);
                let (x1, y1) = (x0 + zsize, y0 + zsize);
                let (a, b) = match side.trim() {
                    "N" => ((x0, y0), (x1, y0)),
                    "S" => ((x0, y1), (x1, y1)),
                    "E" => ((x1, y0), (x1, y1)),
                    _ => ((x0, y0), (x0, y1)),
                };
                ctx.begin_path();
                ctx.move_to(a.0, a.1);
                ctx.line_to(b.0, b.1);
                ctx.set_stroke_style_str("#ffe000");
                ctx.set_line_width(6. * s);
                ctx.stroke();
            }
        }
    }
    if let Some(p) = pending.filter(|p| p["x"].is_number() && p["y"].is_number()) {
        // The disc a radius tool acts on (circle extinguish, ignition).
        if let Some(r) = p["radius_m"].as_f64().filter(|r| r.is_finite() && *r > 0.) {
            ctx.begin_path();
            ctx.arc(n(&p["x"]) * k, (h - n(&p["y"])) * k, r * k, 0., std::f64::consts::TAU)?;
            ctx.set_fill_style_str("rgba(40,120,220,.22)");
            ctx.fill();
            ctx.set_stroke_style_str("#122b2c");
            ctx.set_line_width(4. * s);
            ctx.stroke();
        }
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
    scale_bar(&ctx, k * sc / dpr, (css_w, css_h), dpr)?;
    draw_labels(
        &ctx,
        canvas,
        vw,
        sc / dpr,
        dpr,
        zsize,
        // Hidden zone codes: no edge axes and no per-cell codes.
        if on(ml::CODES) { (zcols, zrows) } else { (0, 0) },
        &reqs.into_inner(),
        &badges.into_inner(),
        &front,
    )
}
/// Metric scale bar in screen space (bitmap pixels), drawn after the world transform is reset.
fn scale_bar(
    ctx: &CanvasRenderingContext2d,
    px_per_m: f64,
    (size, bh): (f64, f64),
    dpr: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    let len_m = [10., 20., 50., 100., 200., 500., 1000., 2000., 5000., 10000.]
        .into_iter()
        .rev()
        .find(|m| m * px_per_m <= 110.)
        .unwrap_or(10.);
    ctx.set_transform(dpr, 0., 0., dpr, 0., 0.)?;
    let l = len_m * px_per_m;
    let (x, y) = (size - 16. - l, bh - 10.);
    ctx.set_fill_style_str("rgba(255,255,255,.7)");
    ctx.fill_rect(x - 4., y - 16., l + 8., 22.);
    ctx.set_stroke_style_str("#122b2c");
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.move_to(x, y - 6.);
    ctx.line_to(x, y);
    ctx.line_to(x + l, y);
    ctx.line_to(x + l, y - 6.);
    ctx.stroke();
    ctx.set_fill_style_str("#122b2c");
    ctx.set_font("11px system-ui");
    let label = if len_m >= 1000. {
        format!("{} km", len_m / 1000.)
    } else {
        format!("{len_m} m")
    };
    ctx.fill_text(&label, x + 2., y - 7.)
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn player_view_is_the_role_map_with_its_missions() {
        let map = json!({"known_zones":["G5"],"cells":[{"row":1,"col":2,"state":1}]});
        let deployments = json!([{"task":{"id":3}}]);
        let v = player_view(&map, &deployments);
        assert_eq!(v["known_zones"], json!(["G5"]));
        assert_eq!(v["deployments"], deployments);
        // Still fogged: a player view is never an authoritative one.
        assert!(fog_zones(&json!({"zones":{"cols":8,"rows":8}}), &v).is_some());
        assert_eq!(player_view(&Value::Null, &deployments), Value::Null);
    }

    #[test]
    fn labels_are_compact_at_overview_and_complete_when_zoomed() {
        assert!(label_font_px(1.) < 13. && label_font_px(16.) <= 14.5);
        assert_eq!(label_text("Presidio sanitario di Selvariva", 1.), "Presidio sanitari…");
        assert_eq!(label_text("Presidio sanitario di Selvariva", 4.), "Presidio sanitario di Selvariva");
        assert_eq!(label_text("Pianalto", 1.), "Pianalto");
        // Descriptive places wait for zoom; towns and critical facilities never do.
        assert!(!label_shown("Margine nord di Roverechiare", 2, 1.));
        assert!(label_shown("Margine nord di Roverechiare", 2, 2.));
        assert!(label_shown("Pianalto", 2, 1.) && label_shown("Scuola di Roverechiare", 0, 1.));
    }

    #[test]
    fn zone_codes_run_along_the_edges_and_reach_cells_only_when_zoomed() {
        // Overview: 8 letters on top, 8 numbers on the left, no per-cell codes.
        let overview = zone_marks(80., (100., 0.), (8, 8), (840., 640.));
        assert_eq!(overview.iter().filter(|m| !m.cell).count(), 16);
        assert!(overview.iter().all(|m| !m.cell));
        assert_eq!(overview[0], ZoneMark { text: "A".into(), x: 140., y: 8., cell: false });
        assert!(overview.iter().any(|m| m.text == "8" && (m.x - 108.).abs() < 0.1));
        // Panned at mid zoom: axes only for the visible columns.
        let mid = zone_marks(120., (-200., 0.), (8, 8), (500., 600.));
        let letters: Vec<&str> = mid.iter().filter(|m| m.text.chars().all(|c| c.is_ascii_uppercase())).map(|m| m.text.as_str()).collect();
        assert_eq!(letters, ["B", "C", "D", "E", "F"]);
        // Zoomed: codes in the visible cells, no axes.
        let zoomed = zone_marks(400., (-500., -300.), (8, 8), (800., 600.));
        assert!(zoomed.iter().all(|m| m.cell));
        assert!(zoomed.iter().any(|m| m.text == "C2"));
        assert!(zoomed.iter().all(|m| m.x >= 0. && m.x <= 800. && m.y >= 0. && m.y <= 600.));
    }

    #[test]
    fn hidden_zone_codes_draw_neither_axes_nor_cell_codes() {
        // `draw` passes an empty zone grid when the codes layer is off.
        assert!(zone_marks(80., (100., 0.), (0, 0), (840., 640.)).is_empty());
        assert!(zone_marks(400., (-500., -300.), (0, 0), (800., 600.)).is_empty());
    }
}
