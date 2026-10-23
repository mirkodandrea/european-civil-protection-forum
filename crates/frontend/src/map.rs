//! Read-only map: every dynamic layer comes from an authorized server view.
use crate::api::{list, text, window};
use base64::Engine;
use serde_json::{json, Value};
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use yew::prelude::*;

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
}
#[function_component(SituationMap)]
pub fn situation_map(props: &MapProps) -> Html {
    let canvas = use_node_ref();
    {
        let canvas = canvas.clone();
        use_effect_with(
            (
                props.stat.clone(),
                props.view.clone(),
                props.simple,
                props.pending.clone(),
                props.lang.clone(),
            ),
            move |(stat, view, simple, pending, lang)| {
                if let Some(canvas) = canvas.cast::<HtmlCanvasElement>() {
                    let _ = draw(&canvas, stat, view, *simple, pending.as_ref(), lang);
                }
            },
        );
    }
    let onclick = {
        let canvas = canvas.clone();
        let stat = props.stat.clone();
        let cb = props.onclick.clone();
        Callback::from(move |e: MouseEvent| {
            if let Some(canvas) = canvas.cast::<HtmlCanvasElement>() {
                let rect = canvas.get_bounding_client_rect();
                let w = n(&stat["grid"]["cols"]) * n(&stat["grid"]["cellsize"]);
                let h = n(&stat["grid"]["rows"]) * n(&stat["grid"]["cellsize"]);
                let x = ((e.client_x() as f64 - rect.left()) / rect.width() * w)
                    .round()
                    .clamp(0., w - 1.);
                let y = (h - (e.client_y() as f64 - rect.top()) / rect.height() * h)
                    .round()
                    .clamp(0., h - 1.);
                let zone = format!(
                    "{}{}",
                    (b'A' + (x / n(&stat["zones"]["size_m"])).floor() as u8) as char,
                    ((h - y) / n(&stat["zones"]["size_m"])).floor() as u32 + 1
                );
                cb.emit(json!({"x":x,"y":y,"zone":zone}));
            }
        })
    };
    html! {<div class="mapbox"><canvas id={props.id.clone()} ref={canvas} {onclick} style={if props.simple {"max-width:min(100%, calc(100dvh - 280px))"}else{""}} aria-label={if props.simple {crate::i18n::t(&props.lang, "map_tactical")} else {crate::i18n::t(&props.lang, "map_situation")}}/><p class="map-legend">{crate::i18n::t(&props.lang, "map_legend")}</p></div>}
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
) -> Result<(), wasm_bindgen::JsValue> {
    let cols = stat["grid"]["cols"].as_u64().unwrap_or(1) as usize;
    let rows = stat["grid"]["rows"].as_u64().unwrap_or(1) as usize;
    let cell = n(&stat["grid"]["cellsize"]);
    let w = cols as f64 * cell;
    let h = rows as f64 * cell;
    if w <= 0. || h <= 0. {
        return Ok(());
    }
    let size = 1200.;
    let k = size / w;
    canvas.set_width(size as u32);
    canvas.set_height((h * k) as u32);
    let ctx = canvas
        .get_context("2d")?
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()?;
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
        let fg = colour(fire.get(i).copied().unwrap_or(0));
        let a = fg[3] as f64 / 255.;
        for c in 0..3 {
            rgba[i * 4 + c] = (bg[c] as f64 * (1. - a) + fg[c] as f64 * a) as u8;
        }
        rgba[i * 4 + 3] = 255;
    }
    for obs in list(&v["cells"]) {
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
    ctx.set_font(if simple {
        "19px system-ui"
    } else {
        "16px system-ui"
    });
    let mut labels: Vec<(f64, f64, f64, f64)> = Vec::new();
    let label = |ctx: &CanvasRenderingContext2d,
                 s: &str,
                 x: f64,
                 y: f64,
                 labels: &mut Vec<(f64, f64, f64, f64)>| {
        let width = s.chars().count() as f64 * 8.5 + 8.;
        let x = x.clamp(3., (size - width - 3.).max(3.));
        let y = y.clamp(22., h * k - 5.);
        let candidate = [0., -24., 24., -48., 48.]
            .into_iter()
            .map(|dy| (x, y + dy - 18., width, 23.))
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
            let _ = ctx.fill_text(s, b.0 + 3., b.1 + 18.);
        }
    };
    let z = &stat["zones"];
    let zsize = n(&z["size_m"]) * k;
    ctx.set_line_width(1.);
    ctx.set_stroke_style_str("rgba(0,0,0,.25)");
    for col in 0..=z["cols"].as_u64().unwrap_or(0) {
        ctx.begin_path();
        ctx.move_to(col as f64 * zsize, 0.);
        ctx.line_to(col as f64 * zsize, h * k);
        ctx.stroke();
    }
    for row in 0..=z["rows"].as_u64().unwrap_or(0) {
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
            ctx.fill_text(&name, col as f64 * zsize + 5., row as f64 * zsize + 21.)?;
        }
    }
    ctx.set_line_join("round");
    ctx.set_line_cap("round");
    for road in list(&stat["roads"]) {
        if simple && road["drivable"] == false {
            continue;
        }
        let points = list(&road["points"]);
        trace(&ctx, points, k, h);
        ctx.set_stroke_style_str("#29363d");
        ctx.set_line_width(6.);
        ctx.stroke();
        ctx.set_stroke_style_str("#e6e4d8");
        ctx.set_line_width(3.);
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
            ctx.set_line_width(5.);
            if !cut {
                ctx.set_line_dash(&js_sys::Array::of2(&8.into(), &5.into()))?;
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
                    (h - n(&segment["from"][1])) * k - 6.,
                    &mut labels,
                );
                marked = true;
            }
        }
    }
    for (i, b) in list(&stat["buildings"]).iter().enumerate() {
        if simple && text(&b["name"]).is_empty() {
            continue;
        }
        let state = &v["buildings"][i];
        let x = n(&b["pos"][0]) * k;
        let y = (h - n(&b["pos"][1])) * k;
        let bw = (n(&b["size"][0]) * k).max(4.);
        let bh = (n(&b["size"][1]) * k).max(4.);
        let fill = if state["damage"] == 2 {
            "#000"
        } else if state["damage"] == 1 {
            "#ff7a00"
        } else if state["evacuating"] == true {
            "#00c8ff"
        } else if state["inside"] == 0 && n(&state["evacuated"]) > 0. {
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
            ctx.set_line_width(2.);
            ctx.stroke();
        }
        if state["threatened"] == true {
            ctx.set_stroke_style_str("#ffb000");
            ctx.stroke_rect(x - bw / 2. - 2., y - bh / 2. - 2., bw + 4., bh + 4.);
        }
    }
    for line in list(&v["lines"]) {
        trace(&ctx, &[line["from"].clone(), line["to"].clone()], k, h);
        ctx.set_stroke_style_str("#ffe000");
        ctx.set_line_width(3.);
        ctx.set_line_dash(&js_sys::Array::of2(&7.into(), &4.into()))?;
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
        ctx.set_line_width(3.);
        ctx.stroke();
    }
    for location in list(&v["reported_locations"]) {
        let x = n(&location[0]) * k;
        let y = (h - n(&location[1])) * k;
        ctx.begin_path();
        ctx.arc(x, y, 9., 0., std::f64::consts::TAU)?;
        ctx.set_stroke_style_str("#e33");
        ctx.stroke();
        label(&ctx, crate::i18n::t(lang, "map_reported"), x + 12., y, &mut labels);
    }
    if let Some(forecast) = list(&v["forecast_products"]).last() {
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
            ctx.set_line_width(3.);
            ctx.set_line_dash(&js_sys::Array::of2(&8.into(), &5.into()))?;
            ctx.stroke();
            ctx.set_line_dash(&js_sys::Array::new())?;
        }
    }
    for d in list(&v["deployments"]) {
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
        ctx.fill_rect(x - 7., y - 5., 14., 10.);
        ctx.set_stroke_style_str("#112632");
        ctx.stroke_rect(x - 7., y - 5., 14., 10.);
        if ["en_route", "withdrawing", "refilling"].contains(&text(&t["status"]))
            && t["target"].is_array()
        {
            let tx = n(&t["target"][0]) * k;
            let ty = (h - n(&t["target"][1])) * k;
            ctx.begin_path();
            ctx.move_to(tx, ty - 8.);
            ctx.line_to(tx + 8., ty);
            ctx.line_to(tx, ty + 8.);
            ctx.line_to(tx - 8., ty);
            ctx.close_path();
            ctx.set_stroke_style_str(color);
            ctx.stroke();
        }
        label(
            &ctx,
            &format!("#{} {}", t["id"], text(&t["status"])),
            x + 10.,
            y - 5.,
            &mut labels,
        );
    }
    for b in list(&stat["buildings"]) {
        if let Some(name) = b["name"].as_str().filter(|s| !s.is_empty()) {
            label(
                &ctx,
                name,
                n(&b["pos"][0]) * k + 8.,
                (h - n(&b["pos"][1])) * k - 6.,
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
                    ctx.set_line_width(4.);
                    ctx.stroke_rect(x + 2., y + 2., zsize - 4., zsize - 4.);
                }
            }
        }
    }
    if let Some(p) = pending.filter(|p| p["x"].is_number() && p["y"].is_number()) {
        ctx.begin_path();
        ctx.arc(
            n(&p["x"]) * k,
            (h - n(&p["y"])) * k,
            8.,
            0.,
            std::f64::consts::TAU,
        )?;
        ctx.set_fill_style_str("#ffe000");
        ctx.fill();
    }
    Ok(())
}
