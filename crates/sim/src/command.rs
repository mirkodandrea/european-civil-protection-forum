//! The command catalogue: every change to the world is one of these.
//!
//! The same JSON shape is produced by the LLM (from player replies and Master
//! prompts), by the Master GUI forms, and by scripts calling the HTTP API
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

/// An area: a whole zone (`"F4"`), a named place (250 m around it), or a circle.
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
    WaterDrop { at: Loc },
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
    IssueWarning { zone: String },
    CloseRoad { road: String },
    ReopenRoad { road: String },
    /// Open a facility as a shelter / assembly area for evacuees.
    OpenShelter { building: String },
    /// Request external units; they arrive after the offer's delay.
    RequestReinforcement { resource: String, qty: u32 },

    // ---- Master only ----
    Ignite {
        at: Loc,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_m: Option<f32>,
    },
    /// Put the fire out over an area (a whole zone, a place, or a circle).
    Extinguish { area: Area },
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
    ReturnResource { allocation: u64, qty: u32 },
    AdjustResource { role: String, resource: String, delta: i32 },
    /// Skip simulated time outside the turn loop.
    AdvanceTime { minutes: u32 },
}

pub const MASTER_ONLY: &[&str] = &["Ignite", "Extinguish", "SetWeather", "SetBuildingStatus", "AdjustResource", "ReturnResource", "AdvanceTime"];

pub const ALL_KINDS: &[&str] = &[
    "CutFireLine",
    "WaterDrop",
    "EvacuateZone",
    "EvacuateBuilding",
    "IssueWarning",
    "CloseRoad",
    "ReopenRoad",
    "OpenShelter",
    "RequestReinforcement",
    "Ignite",
    "Extinguish",
    "SetWeather",
    "SetBuildingStatus",
    "AdjustResource",
    "ReturnResource",
    "AdvanceTime",
];

impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Command::CutFireLine { .. } => "CutFireLine",
            Command::WaterDrop { .. } => "WaterDrop",
            Command::EvacuateZone { .. } => "EvacuateZone",
            Command::EvacuateBuilding { .. } => "EvacuateBuilding",
            Command::IssueWarning { .. } => "IssueWarning",
            Command::CloseRoad { .. } => "CloseRoad",
            Command::ReopenRoad { .. } => "ReopenRoad",
            Command::OpenShelter { .. } => "OpenShelter",
            Command::RequestReinforcement { .. } => "RequestReinforcement",
            Command::Ignite { .. } => "Ignite",
            Command::Extinguish { .. } => "Extinguish",
            Command::SetWeather { .. } => "SetWeather",
            Command::SetBuildingStatus { .. } => "SetBuildingStatus",
            Command::AdjustResource { .. } => "AdjustResource",
            Command::ReturnResource { .. } => "ReturnResource",
            Command::AdvanceTime { .. } => "AdvanceTime",
        }
    }

    pub fn is_master_only(&self) -> bool {
        MASTER_ONLY.contains(&self.kind())
    }

    /// One-line English description, for logs, the Master queue and debrief.
    pub fn describe(&self) -> String {
        fn loc(l: &Loc) -> String {
            match l {
                Loc::Point { x, y } => format!("({x:.0}, {y:.0})"),
                Loc::Named(s) => s.clone(),
            }
        }
        match self {
            Command::CutFireLine { from, to, along, .. } => match (along, from, to) {
                (Some(a), _, _) => format!("Cut fire line along {a}"),
                (_, Some(f), Some(t)) => format!("Cut fire line from {} to {}", loc(f), loc(t)),
                _ => "Cut fire line (incomplete)".into(),
            },
            Command::WaterDrop { at } => format!("Water drop on {}", loc(at)),
            Command::EvacuateZone { zone, destination } => {
                format!("Evacuate zone {zone}{}", destination.as_ref().map(|d| format!(" to {d}")).unwrap_or_default())
            }
            Command::EvacuateBuilding { building, destination } => {
                format!("Evacuate {building}{}", destination.as_ref().map(|d| format!(" to {d}")).unwrap_or_default())
            }
            Command::IssueWarning { zone } => format!("Issue warning to zone {zone}"),
            Command::CloseRoad { road } => format!("Close road {road}"),
            Command::ReopenRoad { road } => format!("Reopen road {road}"),
            Command::OpenShelter { building } => format!("Open shelter at {building}"),
            Command::RequestReinforcement { resource, qty } => format!("Request {qty} × {resource}"),
            Command::Ignite { at, .. } => format!("Ignite fire at {}", loc(at)),
            Command::Extinguish { area } => match area {
                Area::Named(s) => format!("Extinguish {s}"),
                Area::Circle { at, radius_m } => format!("Extinguish {radius_m:.0} m around {}", loc(at)),
            },
            Command::SetWeather { wind_dir_deg, wind_speed_kmh, moisture_pct } => format!(
                "Set weather{}{}{}",
                wind_dir_deg.map(|v| format!(" wind from {v:.0}°")).unwrap_or_default(),
                wind_speed_kmh.map(|v| format!(" {v:.0} km/h")).unwrap_or_default(),
                moisture_pct.map(|v| format!(" moisture {v:.0}%")).unwrap_or_default()
            ),
            Command::SetBuildingStatus { building, damage, evacuated } => format!(
                "Set {building}{}{}",
                damage.map(|d| format!(" damage={d:?}")).unwrap_or_default(),
                evacuated.map(|e| format!(" evacuated={e}")).unwrap_or_default()
            ),
            Command::ReturnResource { allocation, qty } => format!("Return {qty} units from allocation #{allocation}"),
            Command::AdjustResource { role, resource, delta } => format!("Adjust {role}.{resource} by {delta:+}"),
            Command::AdvanceTime { minutes } => format!("Advance time {minutes} min"),
        }
    }
}

/// Documentation of the catalogue, restricted to `allowed` kinds, for the LLM
/// prompt and the API docs.
pub fn catalogue(allowed: &[String]) -> String {
    let docs: &[(&str, &str)] = &[
        ("CutFireLine", r#"{"type":"CutFireLine","from":LOC,"to":LOC}  or  {"type":"CutFireLine","along":"F3:S"}  — fuel break cut by ground crews (1 crew per 600 m). "along" = zone edge N/S/E/W. Optional "width_m" (default 40)."#),
        ("WaterDrop", r#"{"type":"WaterDrop","at":LOC}  — one air tanker drop, ~60 m radius. Wets fuel ahead of the front; does not put out burning cells."#),
        ("EvacuateZone", r#"{"type":"EvacuateZone","zone":"F4","destination":"Valverde Sports Centre"}  — evacuate all occupied buildings of a grid zone. destination optional (an open shelter). Schools/hospitals need buses (50 seats each)."#),
        ("EvacuateBuilding", r#"{"type":"EvacuateBuilding","building":"Pineta Primary School","destination":"Valverde Sports Centre"}  — evacuate one named facility. Schools/hospitals need buses."#),
        ("IssueWarning", r#"{"type":"IssueWarning","zone":"F4"}  — warn the population of a zone; later evacuations there are faster."#),
        ("CloseRoad", r#"{"type":"CloseRoad","road":"north_road"}  — close a road to civilian traffic (1 police patrol)."#),
        ("ReopenRoad", r#"{"type":"ReopenRoad","road":"north_road"}"#),
        ("OpenShelter", r#"{"type":"OpenShelter","building":"Valverde Sports Centre"}  — open a facility as shelter for evacuees."#),
        ("RequestReinforcement", r#"{"type":"RequestReinforcement","resource":"ground_crews","qty":2}  — request external units; they arrive after a delay."#),
        ("Ignite", r#"{"type":"Ignite","at":LOC,"radius_m":100}  — start a fire."#),
        ("Extinguish", r#"{"type":"Extinguish","area":"F2"}  or  {"type":"Extinguish","area":{"at":LOC,"radius_m":200}}  — put the fire out over a whole zone, a place or a circle."#),
        ("SetWeather", r#"{"type":"SetWeather","wind_dir_deg":45,"wind_speed_kmh":50,"moisture_pct":4}  — any subset; wind_dir = direction the wind blows FROM."#),
        ("SetBuildingStatus", r#"{"type":"SetBuildingStatus","building":"Pineta Fuel Station","damage":"destroyed","evacuated":true}  — damage: intact|damaged|destroyed. building may be a zone code."#),
        ("ReturnResource", r#"{"type":"ReturnResource","allocation":1,"qty":1} — EXCON confirms units returned from a recorded deployment. Does not undo its physical effects."#),
        ("AdjustResource", r#"{"type":"AdjustResource","role":"fire_commander","resource":"ground_crews","delta":2}"#),
        ("AdvanceTime", r#"{"type":"AdvanceTime","minutes":60}  — skip simulated time."#),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_shapes() {
        let c: Command = serde_json::from_str(r#"{"type":"CutFireLine","from":"F3","to":{"x":10,"y":20}}"#).unwrap();
        assert_eq!(
            c,
            Command::CutFireLine { from: Some(Loc::Named("F3".into())), to: Some(Loc::Point { x: 10.0, y: 20.0 }), along: None, width_m: None }
        );
        let e: Command = serde_json::from_str(r#"{"type":"Extinguish","area":"F2"}"#).unwrap();
        assert_eq!(e, Command::Extinguish { area: Area::Named("F2".into()) });
        let e: Command = serde_json::from_str(r#"{"type":"Extinguish","area":{"at":"Pine Hill","radius_m":200}}"#).unwrap();
        assert!(matches!(e, Command::Extinguish { area: Area::Circle { .. } }));
        let back: Command = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn catalogue_is_filtered() {
        let doc = catalogue(&["WaterDrop".to_string()]);
        assert!(doc.contains("WaterDrop"));
        assert!(!doc.contains("Ignite"));
        assert_eq!(ALL_KINDS.len(), 16);
    }
}
