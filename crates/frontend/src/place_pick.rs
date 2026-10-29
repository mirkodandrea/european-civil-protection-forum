//! Where a player's action takes place, picked to match the action type
//! (pure logic, no browser types; the tactical view keeps the state in fields).
//!
//! - `point`: one map point (`"x,y"`), e.g. an engine post or a water drop;
//! - `line`: two points, filled in order (first, then second) or a zone edge
//!   (`"F4:N"`) for a fire line;
//! - `zones`: several cells of the 8×8 grid, added one by one, as a rectangle
//!   (two corners) or by a named place that expands to the cells it covers;
//! - `building` / `road`: one named facility or road;
//! - `none`: no place (reinforcement requests).
//!
//! Places are named places, zone codes (`F4`), zone edges (`F4:N`) or coordinates.
use crate::api::{list, text};
use serde_json::{json, Value};

/// The selection tool an action needs.
pub fn pick_for(action: &str) -> &'static str {
    match action {
        "IssueWarning" | "EvacuateZone" | "MonitorArea" | "Reconnoitre" => "zones",
        "EvacuateBuilding" | "OpenShelter" => "building",
        "CloseRoad" | "ReopenRoad" => "road",
        "CutFireLine" => "line",
        "DeployEngine" | "WaterDrop" => "point",
        "RequestReinforcement" => "none",
        _ => "point",
    }
}

/// EXCON-only command kinds (`sim::MASTER_ONLY`; the server build has no `sim`).
const MASTER_ONLY: [&str; 9] = ["Ignite", "Extinguish", "ExtinguishArea", "SetWeather", "SetBuildingStatus", "AdjustResource", "ReturnResource", "SetDeploymentTravel", "AdvanceTime"];

/// Action kinds a player may choose: the role's `allowed_actions`, in order,
/// without duplicates and without EXCON-only kinds.
pub fn menu_actions(role: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in list(&role["allowed_actions"]).iter().map(text) {
        if !a.is_empty() && !MASTER_ONLY.contains(&a) && !out.iter().any(|x| x == a) {
            out.push(a.to_string());
        }
    }
    out
}

fn grid(stat: &Value) -> (u64, u64, f64) {
    let z = &stat["zones"];
    (z["cols"].as_u64().unwrap_or(0), z["rows"].as_u64().unwrap_or(0), z["size_m"].as_f64().unwrap_or(0.))
}

/// `(col, row)` of a zone code (`row` from 1 at the top), case and spaces ignored.
pub fn parse_zone(stat: &Value, s: &str) -> Option<(u64, u64)> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let (cols, rows, _) = grid(stat);
    let mut it = s.chars();
    let letter = it.next()?;
    let row = it.as_str().parse::<u64>().ok()?;
    let col = (letter as u64).checked_sub(b'A' as u64)?;
    (letter.is_ascii_uppercase() && col < cols && (1..=rows).contains(&row)).then_some((col, row))
}

pub fn zone_code(col: u64, row: u64) -> String {
    format!("{}{}", (b'A' + col as u8) as char, row)
}

/// A zone edge such as `F4:N`.
pub fn is_zone_edge(stat: &Value, s: &str) -> bool {
    match s.trim().split_once(':') {
        Some((z, side)) => parse_zone(stat, z).is_some() && ["N", "S", "E", "W"].contains(&side.trim().to_uppercase().as_str()),
        None => false,
    }
}

/// Every zone of the rectangle with corners `a` and `b`.
pub fn rectangle(stat: &Value, a: &str, b: &str) -> Vec<String> {
    let (Some((c0, r0)), Some((c1, r1))) = (parse_zone(stat, a), parse_zone(stat, b)) else { return Vec::new() };
    let mut out = Vec::new();
    for r in r0.min(r1)..=r0.max(r1) {
        for c in c0.min(c1)..=c0.max(c1) {
            out.push(zone_code(c, r));
        }
    }
    out
}

/// The zone holding a point in world metres (`y` northwards).
pub fn zone_at(stat: &Value, x: f64, y: f64) -> Option<String> {
    let (cols, rows, size) = grid(stat);
    if size <= 0. {
        return None;
    }
    let h = rows as f64 * size;
    let (c, r) = ((x / size).floor(), ((h - y) / size).floor());
    (c >= 0. && r >= 0. && (c as u64) < cols && (r as u64) < rows).then(|| zone_code(c as u64, r as u64 + 1))
}

/// A map point in words: its zone and, when it lies within a named place
/// (its `radius_m`, else 300 m), that place: `"E5 (Selvariva)"`.
pub fn point_label(stat: &Value, x: f64, y: f64) -> String {
    let near = list(&stat["places"])
        .iter()
        .filter(|p| {
            let name = text(&p["name"]);
            !name.is_empty() && !name.contains(" edge") && !name.contains("junction")
        })
        .filter_map(|p| {
            let d = (p["pos"][0].as_f64()? - x).hypot(p["pos"][1].as_f64()? - y);
            (d <= p["radius_m"].as_f64().unwrap_or(300.).max(300.)).then_some((d, text(&p["name"])))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, name)| name);
    let zone = zone_at(stat, x, y).unwrap_or_else(|| format!("{x:.0}, {y:.0} m"));
    match near {
        Some(name) => format!("{zone} ({name})"),
        None => zone,
    }
}

/// A building reference (`#id` or a name) by its name when it has one.
pub fn building_label(stat: &Value, s: &str) -> String {
    let Some(id) = s.strip_prefix('#') else { return s.to_string() };
    list(&stat["buildings"])
        .iter()
        .find(|b| b["id"].to_string() == id || text(&b["id"]) == id)
        .map(|b| text(&b["name"]))
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
        .unwrap_or_else(|| s.to_string())
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// The cells a named place covers: those its circular area (`radius_m`) touches,
/// those holding its buildings, and the one holding its centre.
pub fn place_cells(stat: &Value, name: &str) -> Vec<String> {
    let key = norm(name);
    if key.is_empty() {
        return Vec::new();
    }
    let Some(place) = list(&stat["places"]).iter().find(|p| {
        norm(text(&p["name"])) == key || list(&p["aliases"]).iter().any(|a| norm(text(a)) == key)
    }) else {
        return Vec::new();
    };
    let (cols, rows, size) = grid(stat);
    let (px, py) = (place["pos"][0].as_f64().unwrap_or(0.), place["pos"][1].as_f64().unwrap_or(0.));
    let mut out: Vec<String> = zone_at(stat, px, py).into_iter().collect();
    if let Some(radius) = place["radius_m"].as_f64().filter(|r| *r > 0. && size > 0.) {
        let h = rows as f64 * size;
        for r in 0..rows {
            for c in 0..cols {
                // Closest point of the cell to the centre.
                let (x0, x1) = (c as f64 * size, (c + 1) as f64 * size);
                let (y1, y0) = (h - r as f64 * size, h - (r + 1) as f64 * size);
                let (qx, qy) = (px.clamp(x0, x1), py.clamp(y0, y1));
                if (qx - px).hypot(qy - py) < radius {
                    out.push(zone_code(c, r + 1));
                }
            }
        }
    }
    for id in list(&place["buildings"]) {
        if let Some(b) = list(&stat["buildings"]).iter().find(|b| b["id"] == *id) {
            out.extend(zone_at(stat, b["pos"][0].as_f64().unwrap_or(-1.), b["pos"][1].as_f64().unwrap_or(-1.)));
        }
    }
    sorted(out)
}

/// Row by row, without duplicates.
fn sorted(zones: Vec<String>) -> Vec<String> {
    let mut keyed: Vec<((u64, u64), String)> = Vec::new();
    for z in zones {
        let up = z.trim().to_uppercase();
        let k = (up.get(1..).and_then(|r| r.parse().ok()).unwrap_or(0), up.as_bytes().first().copied().unwrap_or(0) as u64);
        if !keyed.iter().any(|(_, x)| *x == up) {
            keyed.push((k, up));
        }
    }
    keyed.sort();
    keyed.into_iter().map(|(_, z)| z).collect()
}

/// Zones written in a field: codes, rectangles (`C3-D4`) and named places,
/// separated by commas. Unknown entries are left out.
pub fn zones_of(stat: &Value, field: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in field.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if parse_zone(stat, part).is_some() {
            out.push(part.to_string());
        } else if let Some((a, b)) = part.split_once(['-', '–']).filter(|(a, b)| parse_zone(stat, a).is_some() && parse_zone(stat, b).is_some()) {
            out.extend(rectangle(stat, a, b));
        } else {
            out.extend(place_cells(stat, part));
        }
    }
    sorted(out)
}

/// Entries of the field that are neither zones nor known places.
pub fn unknown_entries(stat: &Value, field: &str) -> Vec<String> {
    field
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty() && zones_of(stat, p).is_empty())
        .map(str::to_string)
        .collect()
}

/// `zones` with `zone` added, or removed when already there.
pub fn toggle(zones: &[String], zone: &str) -> Vec<String> {
    let z = zone.trim().to_uppercase();
    if zones.iter().any(|x| *x == z) {
        zones.iter().filter(|x| **x != z).cloned().collect()
    } else {
        sorted(zones.iter().cloned().chain(std::iter::once(z)).collect())
    }
}

/// `zones` with every zone of `extra` added.
pub fn add_all(zones: &[String], extra: &[String]) -> Vec<String> {
    sorted(zones.iter().chain(extra.iter()).cloned().collect())
}

/// A point field (`"x,y"`) as a command location, else the text itself (a named place or zone).
pub fn loc(value: &str) -> Value {
    let mut pair = value.split(',').filter_map(|part| part.trim().parse::<f32>().ok());
    match (pair.next(), pair.next()) {
        (Some(x), Some(y)) => json!({"x":x,"y":y}),
        _ => json!(value.trim()),
    }
}

/// The commands the form describes. Zone actions that take one zone in the
/// model (warning, evacuation, reconnaissance) give one command per selected
/// zone; the drone takes them all in one order.
pub fn commands(stat: &Value, action: &str, location: &str, destination: &str, resource: &str, qty: u32) -> Vec<Value> {
    match action {
        "IssueWarning" | "EvacuateZone" | "Reconnoitre" => {
            zones_of(stat, location).into_iter().map(|z| json!({"type":action,"zone":z})).collect()
        }
        "MonitorArea" => {
            let zones = zones_of(stat, location);
            if zones.is_empty() { vec![] } else { vec![json!({"type":action,"zones":zones})] }
        }
        "EvacuateBuilding" | "OpenShelter" if !location.trim().is_empty() => vec![json!({"type":action,"building":location.trim()})],
        "CloseRoad" | "ReopenRoad" if !location.trim().is_empty() => vec![json!({"type":action,"road":location.trim()})],
        "WaterDrop" | "DeployEngine" if !location.trim().is_empty() => vec![json!({"type":action,"at":loc(location)})],
        "CutFireLine" if is_zone_edge(stat, location) => {
            let (z, side) = location.trim().split_once(':').unwrap_or_default();
            vec![json!({"type":action,"along":format!("{}:{}", z.trim().to_uppercase(), side.trim().to_uppercase())})]
        }
        "CutFireLine" if !location.trim().is_empty() && !destination.trim().is_empty() => {
            vec![json!({"type":action,"from":loc(location),"to":loc(destination)})]
        }
        "RequestReinforcement" => vec![json!({"type":action,"resource":resource,"qty":qty.max(1)})],
        _ => vec![],
    }
}

/// Field updates for a click on the map at `point` (`x`, `y`, `zone`,
/// optional `building` / `road`). `mode` is the zone tool (`cells` or `rect`),
/// `anchor` the first corner of a rectangle being drawn.
pub fn click(stat: &Value, action: &str, point: &Value, location: &str, target: &str, mode: &str, anchor: &str) -> Vec<(&'static str, String)> {
    let xy = || format!("{},{}", point["x"].as_f64().unwrap_or(0.), point["y"].as_f64().unwrap_or(0.));
    let zone = text(&point["zone"]).to_string();
    match pick_for(action) {
        "road" => vec![("work:location", text(&point["road"]).to_string())],
        "building" => vec![("work:location", text(&point["building"]).to_string())],
        "point" => vec![("work:location", xy())],
        "line" => {
            // First point, then second; a click after both starts a new line.
            if target == "work:destination" {
                vec![("work:destination", xy()), ("work:target_field", "work:location".into())]
            } else {
                vec![("work:location", xy()), ("work:destination", String::new()), ("work:target_field", "work:destination".into())]
            }
        }
        "zones" => {
            let current = zones_of(stat, location);
            if parse_zone(stat, &zone).is_none() {
                return vec![];
            }
            if mode == "rect" {
                if anchor.is_empty() {
                    vec![("work:anchor", zone)]
                } else {
                    vec![("work:location", add_all(&current, &rectangle(stat, anchor, &zone)).join(", ")), ("work:anchor", String::new())]
                }
            } else {
                vec![("work:location", toggle(&current, &zone).join(", "))]
            }
        }
        _ => vec![],
    }
}

/// What the map highlights for the form: selected zones and points/line.
pub fn preview(stat: &Value, action: &str, location: &str, destination: &str, anchor: &str) -> (Vec<String>, Option<Value>) {
    let point = |s: &str| match loc(s) {
        Value::Object(o) => Some(json!([o["x"], o["y"]])),
        _ => None,
    };
    match pick_for(action) {
        "zones" => {
            let mut zones = zones_of(stat, location);
            if parse_zone(stat, anchor).is_some() {
                zones = add_all(&zones, &[anchor.trim().to_uppercase()]);
            }
            (zones, None)
        }
        "line" if is_zone_edge(stat, location) => (vec![], Some(json!({"edge": location.trim().to_uppercase()}))),
        "line" | "point" => {
            let points: Vec<Value> = [location, destination].iter().filter_map(|s| point(s)).collect();
            if points.is_empty() { (vec![], None) } else { (vec![], Some(json!({"points": points}))) }
        }
        _ => (vec![], None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat() -> Value {
        json!({"zones":{"cols":8,"rows":8,"size_m":750.0},
            "buildings":[{"id":7,"name":"Scuola","pos":[3900.0,5900.0]}],
            "places":[{"name":"Selvariva","pos":[3500.0,2280.0],"radius_m":500.0},
                      {"name":"Pianalto","aliases":["Alto"],"pos":[4900.0,2400.0],"buildings":[7]},
                      {"name":"Bivio","pos":[100.0,100.0]}]})
    }

    #[test]
    fn points_and_buildings_in_words() {
        let s = stat();
        assert_eq!(point_label(&s, 3550.0, 2300.0), "E5 (Selvariva)");
        assert_eq!(point_label(&s, 5800.0, 300.0), "H8");
        assert_eq!(building_label(&s, "#7"), "Scuola");
        assert_eq!(building_label(&s, "#99"), "#99");
        assert_eq!(building_label(&s, "Municipio"), "Municipio");
    }

    #[test]
    fn the_menu_is_the_roles_list() {
        let role = json!({"allowed_actions":["CutFireLine","Reconnoitre","Ignite","CutFireLine"]});
        assert_eq!(menu_actions(&role), ["CutFireLine", "Reconnoitre"]);
        assert!(menu_actions(&json!({})).is_empty());
    }

    #[test]
    fn tools_match_the_action() {
        assert_eq!(pick_for("CutFireLine"), "line");
        assert_eq!(pick_for("Reconnoitre"), "zones");
        assert_eq!(pick_for("EvacuateZone"), "zones");
        assert_eq!(pick_for("WaterDrop"), "point");
        assert_eq!(pick_for("CloseRoad"), "road");
        assert_eq!(pick_for("RequestReinforcement"), "none");
    }

    #[test]
    fn zones_rectangles_and_places() {
        let s = stat();
        assert_eq!(rectangle(&s, "d4", "C3"), ["C3", "D3", "C4", "D4"]);
        // Selvariva: centre in E5, a 500 m area reaching neighbouring cells.
        let cells = place_cells(&s, "selvariva");
        assert!(cells.contains(&"E5".to_string()) && cells.len() > 1, "{cells:?}");
        // A place without area: its own cell plus its buildings' cells.
        assert_eq!(place_cells(&s, "Alto"), ["F1", "G5"]);
        assert_eq!(zones_of(&s, "B2, C3-C4, Bivio, nowhere"), ["B2", "C3", "C4", "A8"]);
        assert_eq!(unknown_entries(&s, "B2, nowhere"), ["nowhere"]);
        assert_eq!(toggle(&["B2".into()], "b2"), Vec::<String>::new());
        assert_eq!(toggle(&["B2".into()], "A1"), ["A1", "B2"]);
        assert!(is_zone_edge(&s, "f4:n") && !is_zone_edge(&s, "F4") && !is_zone_edge(&s, "Z9:N"));
    }

    #[test]
    fn several_zones_become_one_order_each_except_the_drone() {
        let s = stat();
        let warn = commands(&s, "IssueWarning", "B2, B3", "", "", 1);
        assert_eq!(warn, vec![json!({"type":"IssueWarning","zone":"B2"}), json!({"type":"IssueWarning","zone":"B3"})]);
        assert_eq!(commands(&s, "Reconnoitre", "Bivio", "", "", 1), vec![json!({"type":"Reconnoitre","zone":"A8"})]);
        assert_eq!(commands(&s, "MonitorArea", "C3-D4", "", "", 1), vec![json!({"type":"MonitorArea","zones":["C3","D3","C4","D4"]})]);
        assert_eq!(commands(&s, "CutFireLine", "f4:s", "", "", 1), vec![json!({"type":"CutFireLine","along":"F4:S"})]);
        assert_eq!(commands(&s, "CutFireLine", "10,20", "30,40", "", 1), vec![json!({"type":"CutFireLine","from":{"x":10.0,"y":20.0},"to":{"x":30.0,"y":40.0}})]);
        assert!(commands(&s, "CutFireLine", "10,20", "", "", 1).is_empty());
        assert!(commands(&s, "EvacuateZone", "", "", "", 1).is_empty());
    }

    #[test]
    fn clicks_fill_the_right_field() {
        let s = stat();
        let p = json!({"x":10.0,"y":20.0,"zone":"C3"});
        // Line: the first click is the first point and moves on to the second.
        let first = click(&s, "CutFireLine", &p, "", "work:location", "cells", "");
        assert_eq!(first[0], ("work:location", "10,20".to_string()));
        assert_eq!(first[2], ("work:target_field", "work:destination".to_string()));
        let second = click(&s, "CutFireLine", &p, "10,20", "work:destination", "cells", "");
        assert_eq!(second[0], ("work:destination", "10,20".to_string()));
        // Cells toggle; a rectangle needs two corners.
        assert_eq!(click(&s, "EvacuateZone", &p, "B2", "", "cells", ""), vec![("work:location", "B2, C3".to_string())]);
        assert_eq!(click(&s, "EvacuateZone", &p, "B2", "", "rect", ""), vec![("work:anchor", "C3".to_string())]);
        assert_eq!(
            click(&s, "MonitorArea", &json!({"zone":"D4"}), "", "", "rect", "C3"),
            vec![("work:location", "C3, D3, C4, D4".to_string()), ("work:anchor", String::new())]
        );
        let (zones, marks) = preview(&s, "MonitorArea", "B2", "", "C3");
        assert_eq!((zones, marks), (vec!["B2".to_string(), "C3".to_string()], None));
        assert_eq!(preview(&s, "CutFireLine", "1,2", "3,4", "").1, Some(json!({"points":[[1.0,2.0],[3.0,4.0]]})));
    }
}
