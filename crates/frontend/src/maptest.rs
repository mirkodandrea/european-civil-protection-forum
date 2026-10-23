//! Standalone test page for the interactive map (`maptest.html`): synthetic data, feature
//! switches and a fog-of-war simulation. It never talks to the server.
use crate::map::SituationMap;
use base64::Engine;
use serde_json::{json, Value};
use yew::prelude::*;

const COLS: usize = 80;
const CELL: f64 = 25.;
const ZONE_CELLS: usize = 10;
const ZONES: usize = COLS / ZONE_CELLS;
const SIDE: f64 = COLS as f64 * CELL;

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}
fn zone_name(zc: usize, zr: usize) -> String {
    format!("{}{}", (b'A' + zc as u8) as char, zr + 1)
}
fn stat() -> Value {
    let fuel: Vec<u8> = (0..COLS * COLS)
        .map(|i| match ((i % COLS) / 13 + (i / COLS) / 17) % 5 {
            0 => 1,
            1 => 5,
            2 => 2,
            3 => 6,
            _ => 7,
        })
        .collect();
    let road = |id: &str, name: &str, pts: Value| json!({"id":id,"name":name,"points":pts,"drivable":true});
    json!({
        "id":"maptest","name":"Valverde · prova mappa",
        "grid":{"cols":COLS,"rows":COLS,"cellsize":CELL},
        "zones":{"size_m":ZONE_CELLS as f64*CELL,"cols":ZONES,"rows":ZONES},
        "fuel":b64(&fuel),
        "roads":[
            road("r1","SP12 Valverde",json!([[100,300],[600,700],[1100,950],[1900,1500]])),
            road("r2","Strada del Bosco",json!([[1000,100],[1050,900],[1200,1700]])),
        ],
        "road_segments":[],
        "buildings":[
            {"id":1,"name":"Scuola Valverde","pos":[900,950],"size":[60,40]},
            {"id":2,"name":"Casa di riposo","pos":[1250,1150],"size":[50,40]},
            {"id":3,"name":"Rifugio Pineta","pos":[1500,1600],"size":[40,30]},
            {"id":4,"name":"","pos":[1100,800],"size":[25,25]},
        ],
        "places":[
            {"name":"Piazza del Municipio","pos":[800,700]},
            {"name":"Base di staging","pos":[300,400]},
        ],
    })
}
/// Live fire state of a cell: 1 burning, 2 burnt, 3 line, 0 none.
fn live_state(col: usize, row: usize) -> u8 {
    let x = (col as f64 + 0.5) * CELL;
    let y = SIDE - (row as f64 + 0.5) * CELL;
    let d = (x - 1000.).hypot(y - 1050.);
    if (1190.0..1215.).contains(&x) && (800.0..1300.).contains(&y) {
        3
    } else if d < 110. {
        2
    } else if d < 210. {
        1
    } else {
        0
    }
}
fn live_raster() -> Vec<u8> {
    (0..COLS * COLS).map(|i| live_state(i % COLS, i / COLS)).collect()
}
fn forecast() -> Value {
    let p: Vec<f64> = (0..COLS * COLS)
        .map(|i| {
            let x = ((i % COLS) as f64 + 0.5) * CELL;
            let y = SIDE - ((i / COLS) as f64 + 0.5) * CELL;
            if (x - 1100.).hypot(y - 1100.) < 330. { 0.8 } else { 0.1 }
        })
        .collect();
    json!({"issued_s":600,"assumptions":"prova","horizons":[{"probability":p}]})
}
fn extras(v: &mut Value, data: bool) {
    if !data {
        return;
    }
    v["lines"] = json!([{"from":[1200,800],"to":[1200,1300]}]);
    v["drops"] = json!([{"at":[1040,1250],"radius_m":60}]);
    v["deployments"] = json!([
        {"active_qty":2,"position_m":[1150,1000],"task":{"id":7,"status":"working","resource":"ground_crews","detail":"Squadra a terra sulla linea","target":null}},
        {"active_qty":1,"position_m":[500,520],"task":{"id":8,"status":"en_route","resource":"engines","detail":"Autobotte in arrivo","target":[900,900]}},
    ]);
    v["forecast_products"] = json!([forecast()]);
}
/// Authoritative (EXCON) view with the live raster and dynamic building/road state.
fn live_view(data: bool) -> Value {
    let mut v = json!({
        "fire":b64(&live_raster()),
        "cells":[],
        "roads":{"r1":{"closed":false,"cut_by_fire":false},"r2":{"closed":false,"cut_by_fire":true}},
        "buildings":[
            {"damage":0,"threatened":true,"inside":120,"evacuated":0,"evacuating":false},
            {"damage":0,"inside":40,"evacuated":0,"evacuating":true},
            {"damage":1,"inside":0,"evacuated":12},
            {"damage":2,"inside":0,"evacuated":0},
        ],
        "warned":["D5"],"reported_locations":[],
    });
    extras(&mut v, data);
    v
}
/// Player view under fog of war: no live raster, no dynamic state; only cells of the enabled
/// zones are reported (so a clear cell there reads "no fire", elsewhere "not reported").
fn fog_view(enabled: &[bool], data: bool) -> Value {
    let mut cells = Vec::new();
    for row in 0..COLS {
        for col in 0..COLS {
            if enabled[(row / ZONE_CELLS) * ZONES + col / ZONE_CELLS] {
                cells.push(json!({"row":row,"col":col,"state":live_state(col, row)}));
            }
        }
    }
    let mut v = json!({
        "cells":cells,"roads":{},"buildings":[],"lines":[],"drops":[],"warned":[],
        "reported_locations":[[1000,1050]],"forecast_products":[],"deployments":[],
    });
    if data {
        v["forecast_products"] = json!([forecast()]);
    }
    v
}
#[function_component(MapTest)]
pub fn map_test() -> Html {
    let fog = use_state(|| false);
    let pan_zoom = use_state(|| true);
    let inspect = use_state(|| true);
    let tools = use_state(|| true);
    let simple = use_state(|| false);
    let data = use_state(|| true);
    let lang = use_state(|| "it".to_string());
    let zones = use_state(|| {
        let mut z = vec![false; ZONES * ZONES];
        for r in 3..6 {
            for c in 3..6 {
                z[r * ZONES + c] = true;
            }
        }
        z
    });
    let stat = stat();
    let view = if *fog { fog_view(&zones, *data) } else { live_view(*data) };
    let toggle = |label: &'static str, state: &UseStateHandle<bool>| {
        let state = state.clone();
        html! {<label class="mt-check"><input type="checkbox" checked={*state} onchange={Callback::from(move |_| state.set(!*state))}/>{label}</label>}
    };
    let preset = |label: &'static str, f: fn(usize, usize) -> bool| {
        let zones = zones.clone();
        html! {<button type="button" onclick={Callback::from(move |_| zones.set((0..ZONES * ZONES).map(|i| f(i % ZONES, i / ZONES)).collect()))}>{label}</button>}
    };
    html! {<main class="page maptest">
        <h2>{"Prova mappa interattiva"}</h2>
        <p class="muted">{"Dati sintetici, nessun collegamento al server. Attiva o disattiva le funzioni e simula la fog of war di un giocatore."}</p>
        <div class="mt-layout">
            <section class="card mt-controls">
                <h3>{"Funzioni"}</h3>
                {toggle("Zoom / pan", &pan_zoom)}
                {toggle("Ispezione (hover / tocco)", &inspect)}
                {toggle("Barra strumenti e livelli", &tools)}
                {toggle("Modalità giocatore (solo edifici con nome)", &simple)}
                {toggle("Previsione, squadre, linee, lanci", &data)}
                <label class="mt-check">{"Lingua "}<select onchange={{let lang=lang.clone();Callback::from(move |e| lang.set(crate::api::select(e)))}}><option value="it" selected={*lang=="it"}>{"it"}</option><option value="en" selected={*lang=="en"}>{"en"}</option></select></label>
                <h3>{"Fog of war"}</h3>
                {toggle("Vista giocatore (nebbia attiva)", &fog)}
                <p class="muted">{"Zone abilitate: il giocatore conosce l’incendio solo lì. Altrove l’ispezione dice «non riferito»."}</p>
                <div class="mt-zones" aria-label="Zone abilitate" style={format!("grid-template-columns:repeat({ZONES},1fr)")}>
                    {for (0..ZONES*ZONES).map(|i| {
                        let (zc, zr) = (i % ZONES, i / ZONES);
                        let zones = zones.clone();
                        html!{<button type="button" class={classes!("mt-zone", zones[i].then_some("on"))} aria-pressed={zones[i].to_string()} disabled={!*fog} onclick={Callback::from(move |_| { let mut z=(*zones).clone(); z[i]=!z[i]; zones.set(z); })}>{zone_name(zc, zr)}</button>}
                    })}
                </div>
                <div class="row">{preset("Nessuna", |_, _| false)}{preset("Intorno all’incendio", |c, r| (3..6).contains(&c) && (3..6).contains(&r))}{preset("Tutte", |_, _| true)}</div>
            </section>
            <section class="card">
                <SituationMap stat={stat} view={view} simple={*simple || *fog} pan_zoom={*pan_zoom} inspect={*inspect} tools={*tools} lang={(*lang).clone()}/>
            </section>
        </div>
    </main>}
}
