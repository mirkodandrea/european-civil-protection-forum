//! The command catalogue: every change to the world is one of these.
//!
//! The same JSON shape is produced by the LLM (from player replies and EXCON
//! prompts), by the EXCON GUI forms, and by scripts calling the HTTP API
//! directly — commands can always be driven "by code", no LLM needed.
//!
//! Locations accept, interchangeably:
//! - a zone code of the evacuation grid: `"F4"` (its centre),
//! - a named place, facility, sector or road: `"Pine Hill"`, `"Pineta Primary School"`,
//! - a point in world metres: `{"x": 2800, "y": 2400}`.
//!
//! Fire lines can also run along a zone edge: `"along": "F3:S"`.

use serde::{Deserialize, Serialize};

/// A location, see the module docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Loc {
    Point { x: f32, y: f32 },
    Named(String),
}

/// An area: a whole zone (`"F4"`), a named place (its own `radius_m`, a
/// sector's bounds, else 250 m around it), or a circle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Area {
    Circle { at: Loc, radius_m: f32 },
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Damage {
    Intact,
    Damaged,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    // ---- player actions (subject to role authority and resources) ----
    /// Cut a fuel break with ground crews, from/to two locations or along a zone edge.
    CutFireLine {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<Loc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Loc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        along: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_m: Option<f32>,
    },
    /// One air-tanker water drop.
    WaterDrop {
        at: Loc,
    },
    /// Approve a road-engine mission; local posts stay within the authored sector.
    DeployEngine {
        at: Loc,
    },
    /// Send the monitoring drone over zones of the evacuation grid. Once on
    /// station it keeps the owner's map of those zones updated. A zone entry is
    /// a code (`"F3"`) or a rectangle of zones (`"F3-G4"`).
    MonitorArea {
        zones: Vec<String>,
        /// Owner of the drone when EXCON tasks it directly; players always use their own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Ground reconnaissance: one crew travels to a zone of the evacuation grid
    /// (a code like `"F4"` or a named place) and, on arrival, reports once what
    /// it sees there: the owner's map of that zone is updated at that instant.
    Reconnoitre {
        zone: String,
        /// Role that receives the report when EXCON orders it directly;
        /// players always send their own crews.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Evacuate every occupied building in a zone of the evacuation grid.
    EvacuateZone {
        zone: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Evacuate one building (a named facility, or `#id`).
    EvacuateBuilding {
        building: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    },
    /// Warn the population of a zone: halves the time an evacuation takes there.
    IssueWarning {
        zone: String,
    },
    CloseRoad {
        road: String,
    },
    ReopenRoad {
        road: String,
    },
    /// Open a facility as a shelter / assembly area for evacuees.
    OpenShelter {
        building: String,
    },
    /// Request external units; they arrive after the offer's delay.
    RequestReinforcement {
        resource: String,
        qty: u32,
    },

    // ---- EXCON only ----
    Ignite {
        at: Loc,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_m: Option<f32>,
    },
    /// Put the fire out over an area (a whole zone, a place, or a circle).
    /// Same rule as `ExtinguishArea`: every realization, fuel unchanged.
    Extinguish {
        area: Area,
    },
    /// Put the fire out inside a polygon drawn by the EXCON (at least three
    /// vertices). Only the fire is affected: fuel and vegetation stay as
    /// they are, so unburnt ground there can burn again later.
    ExtinguishArea {
        polygon: Vec<Loc>,
    },
    SetWeather {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wind_dir_deg: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wind_speed_kmh: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        moisture_pct: Option<f64>,
    },
    /// Force a building's damage and/or evacuation state. `building` may also be a zone code.
    SetBuildingStatus {
        building: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        damage: Option<Damage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evacuated: Option<bool>,
    },
    /// Release units from a recorded deployment; EXCON confirms their return.
    ReturnResource {
        allocation: u64,
        qty: u32,
    },
    /// Configure a training staging base and maximum travel speed.
    SetDeploymentTravel {
        role: String,
        resource: String,
        at: Loc,
        speed_mps: f32,
    },
    AdjustResource {
        role: String,
        resource: String,
        delta: i32,
    },
    /// Skip simulated time outside the turn loop.
    AdvanceTime {
        minutes: u32,
    },
}

pub const MASTER_ONLY: &[&str] = &[
    "Ignite",
    "Extinguish",
    "ExtinguishArea",
    "SetWeather",
    "SetBuildingStatus",
    "AdjustResource",
    "ReturnResource",
    "SetDeploymentTravel",
    "AdvanceTime",
];

pub const ALL_KINDS: &[&str] = &[
    "CutFireLine",
    "WaterDrop",
    "DeployEngine",
    "MonitorArea",
    "Reconnoitre",
    "EvacuateZone",
    "EvacuateBuilding",
    "IssueWarning",
    "CloseRoad",
    "ReopenRoad",
    "OpenShelter",
    "RequestReinforcement",
    "Ignite",
    "Extinguish",
    "ExtinguishArea",
    "SetWeather",
    "SetBuildingStatus",
    "AdjustResource",
    "ReturnResource",
    "SetDeploymentTravel",
    "AdvanceTime",
];

impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Command::CutFireLine { .. } => "CutFireLine",
            Command::WaterDrop { .. } => "WaterDrop",
            Command::DeployEngine { .. } => "DeployEngine",
            Command::MonitorArea { .. } => "MonitorArea",
            Command::Reconnoitre { .. } => "Reconnoitre",
            Command::EvacuateZone { .. } => "EvacuateZone",
            Command::EvacuateBuilding { .. } => "EvacuateBuilding",
            Command::IssueWarning { .. } => "IssueWarning",
            Command::CloseRoad { .. } => "CloseRoad",
            Command::ReopenRoad { .. } => "ReopenRoad",
            Command::OpenShelter { .. } => "OpenShelter",
            Command::RequestReinforcement { .. } => "RequestReinforcement",
            Command::Ignite { .. } => "Ignite",
            Command::Extinguish { .. } => "Extinguish",
            Command::ExtinguishArea { .. } => "ExtinguishArea",
            Command::SetWeather { .. } => "SetWeather",
            Command::SetBuildingStatus { .. } => "SetBuildingStatus",
            Command::AdjustResource { .. } => "AdjustResource",
            Command::ReturnResource { .. } => "ReturnResource",
            Command::SetDeploymentTravel { .. } => "SetDeploymentTravel",
            Command::AdvanceTime { .. } => "AdvanceTime",
        }
    }

    pub fn is_master_only(&self) -> bool {
        MASTER_ONLY.contains(&self.kind())
    }

    /// One-line English description, for logs, the EXCON queue and debrief.
    pub fn describe(&self) -> String {
        fn loc(l: &Loc) -> String {
            match l {
                Loc::Point { x, y } => format!("({x:.0}, {y:.0})"),
                Loc::Named(s) => s.clone(),
            }
        }
        match self {
            Command::CutFireLine {
                from, to, along, ..
            } => match (along, from, to) {
                (Some(a), _, _) => format!("Cut fire line along {a}"),
                (_, Some(f), Some(t)) => format!("Cut fire line from {} to {}", loc(f), loc(t)),
                _ => "Cut fire line (incomplete)".into(),
            },
            Command::DeployEngine { at } => format!("Deploy engine at {}", loc(at)),
            Command::MonitorArea { zones, .. } => format!("Drone monitoring {}", zones.join(", ")),
            Command::Reconnoitre { zone, .. } => format!("Ground reconnaissance of {zone}"),
            Command::WaterDrop { at } => format!("Water drop on {}", loc(at)),
            Command::EvacuateZone { zone, destination } => {
                format!(
                    "Evacuate zone {zone}{}",
                    destination
                        .as_ref()
                        .map(|d| format!(" to {d}"))
                        .unwrap_or_default()
                )
            }
            Command::EvacuateBuilding {
                building,
                destination,
            } => {
                format!(
                    "Evacuate {building}{}",
                    destination
                        .as_ref()
                        .map(|d| format!(" to {d}"))
                        .unwrap_or_default()
                )
            }
            Command::IssueWarning { zone } => format!("Issue warning to zone {zone}"),
            Command::CloseRoad { road } => format!("Close road {road}"),
            Command::ReopenRoad { road } => format!("Reopen road {road}"),
            Command::OpenShelter { building } => format!("Open shelter at {building}"),
            Command::RequestReinforcement { resource, qty } => {
                format!("Request {qty} × {resource}")
            }
            Command::Ignite { at, .. } => format!("Ignite fire at {}", loc(at)),
            Command::Extinguish { area } => match area {
                Area::Named(s) => format!("Extinguish {s}"),
                Area::Circle { at, radius_m } => {
                    format!("Extinguish {radius_m:.0} m around {}", loc(at))
                }
            },
            Command::ExtinguishArea { polygon } => format!(
                "Extinguish the area {}",
                polygon.iter().map(loc).collect::<Vec<_>>().join(" - ")
            ),
            Command::SetWeather {
                wind_dir_deg,
                wind_speed_kmh,
                moisture_pct,
            } => format!(
                "Set weather{}{}{}",
                wind_dir_deg
                    .map(|v| format!(" wind from {v:.0}°"))
                    .unwrap_or_default(),
                wind_speed_kmh
                    .map(|v| format!(" {v:.0} km/h"))
                    .unwrap_or_default(),
                moisture_pct
                    .map(|v| format!(" moisture {v:.0}%"))
                    .unwrap_or_default()
            ),
            Command::SetBuildingStatus {
                building,
                damage,
                evacuated,
            } => format!(
                "Set {building}{}{}",
                damage.map(|d| format!(" damage={d:?}")).unwrap_or_default(),
                evacuated
                    .map(|e| format!(" evacuated={e}"))
                    .unwrap_or_default()
            ),
            Command::ReturnResource { allocation, qty } => {
                format!("Return {qty} units from allocation #{allocation}")
            }
            Command::SetDeploymentTravel {
                role,
                resource,
                at,
                speed_mps,
            } => format!(
                "Set {role}.{resource} staging at {} and travel {speed_mps} m/s",
                loc(at)
            ),
            Command::AdjustResource {
                role,
                resource,
                delta,
            } => format!("Adjust {role}.{resource} by {delta:+}"),
            Command::AdvanceTime { minutes } => format!("Advance time {minutes} min"),
        }
    }
}

/// Documentation of the catalogue, restricted to `allowed` kinds, for the LLM
/// prompt and the API docs.
pub fn catalogue(allowed: &[String]) -> String {
    let docs: &[(&str, &str)] = &[
        (
            "CutFireLine",
            r#"{"type":"CutFireLine","from":LOC,"to":LOC}  or  {"type":"CutFireLine","along":"F3:S"}  — fuel break cut by ground crews (1 crew per 600 m). "along" = zone edge N/S/E/W. Optional "width_m" (default 6 in the operational model, 40 in the legacy model)."#,
        ),
        (
            "WaterDrop",
            r#"{"type":"WaterDrop","at":LOC}  — one air tanker drop, ~60 m radius. Wets fuel ahead of the front; does not put out burning cells."#,
        ),
        (
            "EvacuateZone",
            r#"{"type":"EvacuateZone","zone":"F4","destination":"Valverde Sports Centre"}  — evacuate all occupied buildings of a grid zone. "zone" is ALWAYS a grid code such as F4, never a building or place name: to evacuate one named facility (a school, hospital, ...) use EvacuateBuilding. destination optional (an open shelter). Schools/hospitals need buses (50 seats each)."#,
        ),
        (
            "EvacuateBuilding",
            r#"{"type":"EvacuateBuilding","building":"Pineta Primary School","destination":"Valverde Sports Centre"}  — evacuate one named facility ("building" is its exact name from the gazetteer, never a zone code). Schools/hospitals need buses."#,
        ),
        (
            "IssueWarning",
            r#"{"type":"IssueWarning","zone":"F4"}  — warn the population of a zone (grid code, never a building name); later evacuations there are faster."#,
        ),
        (
            "CloseRoad",
            r#"{"type":"CloseRoad","road":"north_road"}  — close a road to civilian traffic (1 police patrol)."#,
        ),
        ("ReopenRoad", r#"{"type":"ReopenRoad","road":"north_road"}"#),
        (
            "OpenShelter",
            r#"{"type":"OpenShelter","building":"Valverde Sports Centre"}  — open a facility as shelter for evacuees."#,
        ),
        (
            "RequestReinforcement",
            r#"{"type":"RequestReinforcement","resource":"ground_crews","qty":2}  — request external units; they arrive after a delay."#,
        ),
        (
            "Ignite",
            r#"{"type":"Ignite","at":LOC,"radius_m":100}  — start a fire."#,
        ),
        (
            "Extinguish",
            r#"{"type":"Extinguish","area":"F2"}  or  {"type":"Extinguish","area":{"at":LOC,"radius_m":200}}  — put the fire out over a whole zone, a place (its radius, else 250 m) or a circle. Fuel is not removed, like ExtinguishArea."#,
        ),
        (
            "ExtinguishArea",
            r#"{"type":"ExtinguishArea","polygon":[LOC,LOC,LOC,...]}  — put the fire out inside a polygon (at least 3 vertices, in order). Fuel is not removed: unburnt ground inside can burn again if fire reaches it later."#,
        ),
        (
            "SetWeather",
            r#"{"type":"SetWeather","wind_dir_deg":45,"wind_speed_kmh":50,"moisture_pct":4}  — any subset; wind_dir = direction the wind blows FROM."#,
        ),
        (
            "SetBuildingStatus",
            r#"{"type":"SetBuildingStatus","building":"Pineta Fuel Station","damage":"destroyed","evacuated":true}  — damage: intact|damaged|destroyed. building may be a zone code."#,
        ),
        (
            "ReturnResource",
            r#"{"type":"ReturnResource","allocation":1,"qty":1} — EXCON confirms units returned from a recorded deployment. Does not undo its physical effects."#,
        ),
        (
            "SetDeploymentTravel",
            r#"{"type":"SetDeploymentTravel","role":"fire_commander","resource":"ground_crews","at":"Valverde Town Hall","speed_mps":10} — training staging base and straight-line travel speed in metres/second. Future deployments only."#,
        ),
        (
            "AdjustResource",
            r#"{"type":"AdjustResource","role":"fire_commander","resource":"ground_crews","delta":2}"#,
        ),
        (
            "DeployEngine",
            r#"{"type":"DeployEngine","at":LOC} — deploy a road engine (operational model only); with local autonomy, at is the approved sector centre and local moves are bounded by the scenario tactical policy."#,
        ),
        (
            "MonitorArea",
            r#"{"type":"MonitorArea","zones":["F3-G4"]}  — send the monitoring drone over grid zones (codes like "F3", or a rectangle "F3-G4"; at most 4 zones in all). It flies from the staging base, then keeps the map of those zones updated every few minutes until its battery ends, and returns to recharge."#,
        ),
        (
            "Reconnoitre",
            r#"{"type":"Reconnoitre","zone":"F4"}  — send one ground crew to reconnoitre a grid zone (code like "F4" or a named place). It travels from the staging base by road or on foot; on arrival it reports once what it sees there (fire, buildings, roads, lines, units) and the map of that zone is updated at that time. The crew stays deployed until EXCON confirms its return. One zone per order: use several orders for several zones."#,
        ),
        (
            "AdvanceTime",
            r#"{"type":"AdvanceTime","minutes":60}  — skip simulated time."#,
        ),
    ];
    let mut out = String::from("LOC is a zone code like \"F4\", a place/facility name like \"Pine Hill\", or {\"x\":metres_east,\"y\":metres_north}.\n");
    for (k, d) in docs {
        if allowed.iter().any(|a| a == k) {
            out.push_str("- ");
            out.push_str(d);
            out.push('\n');
        }
    }
    out
}

/// [`catalogue`] with the examples rewritten to use names that exist in `scn`
/// (a zone, a shelter, a school, the staging base, a role), so the model is
/// never shown places from another scenario.
pub fn catalogue_for(scn: &scenario::Scenario, allowed: &[String]) -> String {
    use scenario::BuildingKind::{School, SportsCentre};
    let named = |kind: Option<scenario::BuildingKind>| {
        scn.buildings
            .iter()
            .filter(|b| kind.is_none_or(|k| b.kind == k))
            .find_map(|b| b.name.clone())
    };
    let zone = scn.zones.names().into_iter().next();
    let place = scn.places.first().map(|p| p.name.clone());
    let staging = scn
        .staging_base
        .clone()
        .or_else(|| place.clone());
    let crew_role = scn
        .roles
        .iter()
        .find(|r| r.resources.contains_key("ground_crews"))
        .or(scn.roles.first())
        .map(|r| r.id.clone());
    let mut doc = catalogue(allowed);
    let mut swap = |from: &str, to: Option<String>| {
        if let Some(to) = to {
            doc = doc.replace(from, &to);
        }
    };
    swap("Valverde Sports Centre", named(Some(SportsCentre)).or_else(|| named(None)));
    swap("Pineta Primary School", named(Some(School)).or_else(|| named(None)));
    swap("Valverde Town Hall", staging);
    swap("Pine Hill", place);
    swap("fire_commander", crew_role);
    swap("F3:S", zone.clone().map(|z| format!("{z}:S")));
    swap("F3-G4", zone.clone().map(|z| format!("{z}")));
    swap("F4", zone.clone());
    swap("F3", zone);
    doc
}

/// Parameters an action of type `kind` needs before it can run, given the
/// (possibly partial) JSON object `params` (without `"type"`). Empty when the
/// action is complete; unknown kinds report `"type"`. A fire line needs either
/// `along` or both `from` and `to`; optional fields are never reported.
/// Used to show an AI suggestion as incomplete instead of guessing its values.
pub fn missing_params(kind: &str, params: &serde_json::Value) -> Vec<&'static str> {
    let present = |k: &str| {
        params.get(k).is_some_and(|v| match v {
            serde_json::Value::Null => false,
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Array(a) => !a.is_empty(),
            _ => true,
        })
    };
    let required: &[&'static str] = match kind {
        "CutFireLine" => {
            if present("along") {
                return Vec::new();
            }
            return ["from", "to"].into_iter().filter(|k| !present(k)).collect();
        }
        "WaterDrop" | "DeployEngine" | "Ignite" => &["at"],
        "MonitorArea" => &["zones"],
        "Reconnoitre" | "EvacuateZone" | "IssueWarning" => &["zone"],
        "EvacuateBuilding" | "OpenShelter" | "SetBuildingStatus" => &["building"],
        "CloseRoad" | "ReopenRoad" => &["road"],
        "RequestReinforcement" => &["resource", "qty"],
        "Extinguish" => &["area"],
        "ExtinguishArea" => &["polygon"],
        "SetWeather" => &[],
        "ReturnResource" => &["allocation", "qty"],
        "SetDeploymentTravel" => &["role", "resource", "at", "speed_mps"],
        "AdjustResource" => &["role", "resource", "delta"],
        "AdvanceTime" => &["minutes"],
        _ => return vec!["type"],
    };
    required.iter().copied().filter(|k| !present(k)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_params_names_what_an_action_still_needs() {
        use serde_json::json;
        assert_eq!(missing_params("CutFireLine", &json!({})), vec!["from", "to"]);
        assert_eq!(missing_params("CutFireLine", &json!({"from": "E3"})), vec!["to"]);
        assert!(missing_params("CutFireLine", &json!({"from": "E3", "to": "F3"})).is_empty());
        assert!(missing_params("CutFireLine", &json!({"along": "F4:N"})).is_empty());
        assert_eq!(missing_params("DeployEngine", &json!({"at": ""})), vec!["at"]);
        assert_eq!(missing_params("RequestReinforcement", &json!({"resource": "engines"})), vec!["qty"]);
        assert_eq!(missing_params("Teleport", &json!({})), vec!["type"]);
        // Every catalogue kind is known.
        for kind in ALL_KINDS {
            assert!(!missing_params(kind, &json!({})).contains(&"type"), "{kind}");
        }
    }

    #[test]
    fn json_round_trip_and_shapes() {
        let c: Command =
            serde_json::from_str(r#"{"type":"CutFireLine","from":"F3","to":{"x":10,"y":20}}"#)
                .unwrap();
        assert_eq!(
            c,
            Command::CutFireLine {
                from: Some(Loc::Named("F3".into())),
                to: Some(Loc::Point { x: 10.0, y: 20.0 }),
                along: None,
                width_m: None
            }
        );
        let e: Command = serde_json::from_str(r#"{"type":"Extinguish","area":"F2"}"#).unwrap();
        assert_eq!(
            e,
            Command::Extinguish {
                area: Area::Named("F2".into())
            }
        );
        let e: Command = serde_json::from_str(
            r#"{"type":"Extinguish","area":{"at":"Pine Hill","radius_m":200}}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Command::Extinguish {
                area: Area::Circle { .. }
            }
        ));
        let back: Command = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        let a: Command = serde_json::from_str(
            r#"{"type":"ExtinguishArea","polygon":[{"x":10,"y":20},"F3",{"x":30,"y":5}]}"#,
        )
        .unwrap();
        assert_eq!(
            a,
            Command::ExtinguishArea {
                polygon: vec![
                    Loc::Point { x: 10.0, y: 20.0 },
                    Loc::Named("F3".into()),
                    Loc::Point { x: 30.0, y: 5.0 }
                ]
            }
        );
        assert!(a.is_master_only());
        let back: Command = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn catalogue_examples_use_the_scenarios_own_names() {
        let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        scn.staging_base = Some("Depot X".into());
        scn.places[0].name = "Depot X".into();
        scn.roles[1].id = "crew_boss".into();
        let all: Vec<String> = ALL_KINDS.iter().map(|k| k.to_string()).collect();
        let doc = catalogue_for(&scn, &all);
        assert!(doc.contains("Depot X") && doc.contains("crew_boss"), "{doc}");
        assert!(!doc.contains("Valverde Town Hall") && !doc.contains("fire_commander"));
    }

    #[test]
    fn catalogue_is_filtered() {
        let doc = catalogue(&["WaterDrop".to_string()]);
        assert!(doc.contains("WaterDrop"));
        assert!(!doc.contains("Ignite"));
        assert_eq!(ALL_KINDS.len(), 21);
    }
}
