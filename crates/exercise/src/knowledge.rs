//! Information acquired by a role, independent of the authoritative simulation.
use crate::{Exercise, MsgKind, MASTER};
use anyhow::{bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FireCell {
    pub row: usize,
    pub col: usize,
    pub state: u8,
}

/// Dynamic state of one named building, as disclosed to a role. Occupant counts stay hidden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingObs {
    pub index: usize,
    pub damage: u8,
    pub threatened: bool,
    pub evacuating: bool,
    pub evacuated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadObs {
    pub id: String,
    pub closed: bool,
    pub cut_by_fire: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineObs {
    pub from: [f32; 2],
    pub to: [f32; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DropObs {
    pub at: [f32; 2],
    pub radius_m: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitObs {
    pub id: u64,
    pub status: String,
    pub resource: String,
    pub position_m: [f32; 2],
    #[serde(default)]
    pub target: Option<[f32; 2]>,
}

/// Layers EXCON may disclose for a set of zones.
pub const ZONE_LAYERS: [&str; 5] = ["fire", "buildings", "roads", "lines", "units"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapObservation {
    /// Zones this observation covers; a role knows nothing outside the union of its zones.
    #[serde(default)]
    pub zones: Vec<String>,
    #[serde(default)]
    pub buildings: Vec<BuildingObs>,
    #[serde(default)]
    pub roads: Vec<RoadObs>,
    #[serde(default)]
    pub lines: Vec<LineObs>,
    #[serde(default)]
    pub drops: Vec<DropObs>,
    #[serde(default)]
    pub units: Vec<UnitObs>,
    #[serde(default)]
    pub locations: Vec<[f32; 2]>,
    #[serde(default)]
    pub cells: Vec<FireCell>,
    #[serde(default)]
    pub forecast_products: Vec<fire::forecast::Forecast>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    Delivered,
    Acquired,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeEvent {
    pub role: String,
    pub message: u64,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
    pub source: KnowledgeSource,
    pub observation: MapObservation,
    pub timeline: usize,
}

impl Exercise {
    pub fn validate_observation(&self, o: &MapObservation) -> Result<()> {
        if o.forecast_products.len() > 1 {
            bail!("at most one forecast product per message");
        }
        for product in &o.forecast_products {
            product.check(self.world.scn.grid)?;
            if product.issued_s > self.world.time_s() {
                bail!("forecast cannot be issued in the future");
            }
        }
        let g = &self.world.scn.grid;
        if o.locations.len() > 1000 || o.cells.len() > g.rows * g.cols {
            bail!("map observation is too large");
        }
        for [x, y] in &o.locations {
            if !x.is_finite()
                || !y.is_finite()
                || *x < 0.0
                || *y < 0.0
                || *x >= g.cols as f32 * g.cellsize
                || *y >= g.rows as f32 * g.cellsize
            {
                bail!("reported location is outside the map");
            }
        }
        if o.cells
            .iter()
            .any(|c| c.row >= g.rows || c.col >= g.cols || c.state > 5)
        {
            bail!("invalid reported fire cell");
        }
        if o.zones.len() > 676
            || o.zones.iter().any(|z| self.world.scn.zone_bounds(z).is_none())
        {
            bail!("unknown zone in map observation");
        }
        let scn = &self.world.scn;
        if o.buildings.len() > scn.buildings.len()
            || o.buildings.iter().any(|b| b.index >= scn.buildings.len() || b.damage > 2)
            || o.roads.len() > scn.roads.len()
            || o.roads.iter().any(|r| !scn.roads.iter().any(|x| x.id == r.id))
            || o.lines.len() > 1000
            || o.drops.len() > 1000
            || o.units.len() > 1000
        {
            bail!("invalid reported dynamic state");
        }
        Ok(())
    }

    /// Attach only to an actual recipient's delivered message, once. No inference from text.
    pub fn attach_map_observation(
        &mut self,
        mid: u64,
        observation: MapObservation,
        source: KnowledgeSource,
    ) -> Result<()> {
        self.validate_observation(&observation)?;
        let m = self
            .messages
            .iter()
            .find(|m| m.id == mid)
            .ok_or_else(|| anyhow::anyhow!("unknown delivered message"))?;
        if !self.players.contains_key(&m.to)
            || !matches!(
                m.kind,
                MsgKind::Inject | MsgKind::RoleMessage | MsgKind::Situation
            )
        {
            bail!("map information must accompany a delivered operational message");
        }
        if self.knowledge_events.iter().any(|e| e.message == mid) {
            bail!("message already has a map observation");
        }
        let event = KnowledgeEvent {
            role: m.to.clone(),
            message: mid,
            turn: m.turn,
            t_s: m.t_s,
            clock: m.clock.clone(),
            source,
            observation,
            timeline: self.log.iter().filter(|e| e.kind == "rollback").count(),
        };
        self.record("map_information_acquired", MASTER, json!(&event));
        self.knowledge_events.push(event);
        Ok(())
    }

    pub fn acquire_map(
        &mut self,
        role: &str,
        source: &str,
        observation: MapObservation,
        translated: Option<&str>,
    ) -> Result<u64> {
        if self.phase == crate::Phase::Debrief {
            bail!("the exercise has ended");
        }
        if !self.players.contains_key(role)
            || source.trim().is_empty()
            || source.chars().count() > 2000
        {
            bail!("known role and information source required");
        }
        self.validate_observation(&observation)?;
        let forecast = observation.forecast_products.first();
        let title = if forecast.is_some() {
            "Reviewed forecast"
        } else {
            "Map information"
        };
        let text = if let Some(product) = forecast {
            let horizons = product
                .horizons
                .iter()
                .map(|h| format!("+{} min", h.after_s / 60))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Reviewed projection received from {source}. Issued at t={} s; horizons {horizons}. {}",product.issued_s,product.assumptions)
        } else {
            Self::map_information_text(source)
        };
        let mid = self.add_message(
            MASTER,
            role,
            MsgKind::Situation,
            Some(title),
            translated.unwrap_or(&text),
            &text,
            None,
        );
        // The information source is who the player hears it from.
        let persona = self.persona_for(Some(source), None);
        self.set_message_persona(mid, persona);
        self.attach_map_observation(mid, observation, KnowledgeSource::Acquired)?;
        Ok(mid)
    }

    pub fn map_information_text(source: &str) -> String {
        format!("Map information received from {source}. Consult the attached situation map; reported data are observations at delivery time.")
    }

    /// EXCON explicitly chooses to disclose this snapshot; never called by player reads.
    pub fn current_perimeter(&self) -> MapObservation {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(self.world.fire_raster_b64())
            .expect("simulator emits base64");
        let cols = self.world.scn.grid.cols;
        let zones = &self.world.scn.zones;
        MapObservation {
            zones: (0..zones.rows)
                .flat_map(|r| (0..zones.cols).map(move |c| format!("{}{}", (b'A' + c as u8) as char, r + 1)))
                .collect(),
            cells: bytes
                .into_iter()
                .enumerate()
                .map(|(i, state)| FireCell {
                    row: i / cols,
                    col: i % cols,
                    state,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// EXCON explicitly discloses the chosen layers inside the chosen zones: every cell of those
    /// zones (so clear cells read "no fire"), plus buildings/roads/lines/units located there.
    pub fn zone_observation(&self, zones: &[String], layers: &[String]) -> Result<MapObservation> {
        let scn = &self.world.scn;
        if zones.is_empty() {
            bail!("select at least one zone");
        }
        if let Some(l) = layers.iter().find(|l| !ZONE_LAYERS.contains(&l.as_str())) {
            bail!("unknown layer {l}");
        }
        let has = |l: &str| layers.iter().any(|x| x == l);
        let mut bounds = Vec::new();
        for z in zones {
            bounds.push(
                scn.zone_bounds(&z.to_uppercase())
                    .ok_or_else(|| anyhow::anyhow!("unknown zone {z}"))?,
            );
        }
        let inside = |p: [f32; 2]| {
            bounds
                .iter()
                .any(|b| p[0] >= b[0] && p[0] < b[2] && p[1] >= b[1] && p[1] < b[3])
        };
        let view = self.world.view();
        let pos = |v: &Value| -> Option<[f32; 2]> {
            Some([v[0].as_f64()? as f32, v[1].as_f64()? as f32])
        };
        let mut o = MapObservation {
            zones: zones.iter().map(|z| z.to_uppercase()).collect(),
            ..Default::default()
        };
        if has("fire") {
            let raster = base64::engine::general_purpose::STANDARD
                .decode(self.world.fire_raster_b64())
                .expect("simulator emits base64");
            let g = &scn.grid;
            for (i, state) in raster.into_iter().enumerate() {
                let (row, col) = (i / g.cols, i % g.cols);
                let centre = [
                    (col as f32 + 0.5) * g.cellsize,
                    g.height_m() - (row as f32 + 0.5) * g.cellsize,
                ];
                if inside(centre) {
                    o.cells.push(FireCell { row, col, state });
                }
            }
        }
        if has("buildings") {
            for (index, b) in scn.buildings.iter().enumerate() {
                let state = &view["buildings"][index];
                if b.name.as_deref().is_some_and(|n| !n.is_empty()) && inside(b.pos) {
                    o.buildings.push(BuildingObs {
                        index,
                        damage: state["damage"].as_u64().unwrap_or(0) as u8,
                        threatened: state["threatened"] == true,
                        evacuating: state["evacuating"] == true,
                        evacuated: state["inside"] == 0 && state["evacuated"].as_u64().unwrap_or(0) > 0,
                    });
                }
            }
        }
        if has("roads") {
            for r in scn.roads.iter().filter(|r| r.drivable) {
                let state = &view["roads"][&r.id];
                if r.points.iter().any(|p| inside(*p)) {
                    o.roads.push(RoadObs {
                        id: r.id.clone(),
                        closed: state["closed"] == true,
                        cut_by_fire: state["cut_by_fire"] == true,
                    });
                }
            }
        }
        if has("lines") {
            for l in view["lines"].as_array().into_iter().flatten() {
                if let (Some(from), Some(to)) = (pos(&l["from"]), pos(&l["to"])) {
                    if inside(from) || inside(to) {
                        o.lines.push(LineObs { from, to });
                    }
                }
            }
            for d in view["drops"].as_array().into_iter().flatten() {
                if let Some(at) = pos(&d["at"]) {
                    if inside(at) {
                        o.drops.push(DropObs {
                            at,
                            radius_m: d["radius_m"].as_f64().unwrap_or(0.) as f32,
                        });
                    }
                }
            }
        }
        if has("units") {
            for d in view["deployments"].as_array().into_iter().flatten() {
                let task = &d["task"];
                let Some(position_m) = pos(&d["position_m"]) else {
                    continue;
                };
                if d["active_qty"] == 0 || task["status"] == "cancelled" || !inside(position_m) {
                    continue;
                }
                o.units.push(UnitObs {
                    id: task["id"].as_u64().unwrap_or(0),
                    status: task["status"].as_str().unwrap_or("").into(),
                    resource: task["resource"].as_str().unwrap_or("").into(),
                    position_m,
                    target: pos(&task["target"]),
                });
            }
        }
        self.validate_observation(&o)?;
        Ok(o)
    }

    /// Zones explicitly disclosed to a role. Point observations do not reveal a whole zone.
    pub fn known_zones(&self, role: &str) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for event in self.knowledge_events.iter().filter(|e| e.role == role) {
            out.extend(event.observation.zones.iter().cloned());
        }
        out.into_iter().collect()
    }

    /// EXCON-only: what each role currently knows, for the map workspace.
    pub fn role_knowledge(&self) -> Value {
        Value::Object(
            self.players
                .keys()
                .map(|role| {
                    let last = self.knowledge_events.iter().rev().find(|e| &e.role == role);
                    (
                        role.clone(),
                        json!({"zones":self.known_zones(role),"last_clock":last.map(|e| &e.clock),"events":self.knowledge_events.iter().filter(|e| &e.role == role).count()}),
                    )
                })
                .collect(),
        )
    }

    pub fn player_map(&self, role: &str) -> Result<Value> {
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        let mut cells = std::collections::BTreeMap::new();
        let mut buildings = std::collections::BTreeMap::new();
        let mut roads = serde_json::Map::new();
        let (mut lines, mut drops, mut units) = (Vec::new(), Vec::new(), Vec::new());
        let mut locations = Vec::new();
        let timeline = self.log.iter().filter(|e| e.kind == "rollback").count();
        let forecast_products = self
            .knowledge_events
            .iter()
            .rev()
            .find(|e| {
                e.role == role
                    && e.timeline == timeline
                    && !e.observation.forecast_products.is_empty()
            })
            .map(|e| e.observation.forecast_products.clone())
            .unwrap_or_default();
        for event in self.knowledge_events.iter().filter(|e| e.role == role) {
            for cell in &event.observation.cells {
                cells.insert((cell.row, cell.col), cell);
            }
            for b in &event.observation.buildings {
                buildings.insert(b.index, json!({"damage":b.damage,"threatened":b.threatened,"evacuating":b.evacuating,"fully_evacuated":b.evacuated}));
            }
            for r in &event.observation.roads {
                roads.insert(r.id.clone(), json!({"closed":r.closed,"cut_by_fire":r.cut_by_fire}));
            }
            // Lines, drops and units are snapshots: the latest disclosure that has any replaces the rest.
            if !event.observation.lines.is_empty() || !event.observation.drops.is_empty() {
                lines = event.observation.lines.iter().map(|l| json!({"from":l.from,"to":l.to})).collect();
                drops = event.observation.drops.iter().map(|d| json!({"at":d.at,"radius_m":d.radius_m})).collect();
            }
            if !event.observation.units.is_empty() {
                units = event.observation.units.iter().map(|u| json!({"active_qty":1,"position_m":u.position_m,"task":{"id":u.id,"status":u.status,"resource":u.resource,"target":u.target,"detail":""}})).collect();
            }
            for location in &event.observation.locations {
                if !locations.contains(location) {
                    locations.push(*location);
                }
            }
        }
        Ok(
            json!({"cells":cells.values().collect::<Vec<_>>(), "reported_locations":locations,
            "forecast_products":forecast_products,
            "known_zones":self.known_zones(role), "observed_units":units,
            "roads":roads, "lines":lines, "drops":drops, "warned":[], "burning_zones":[],
            "buildings":(0..self.world.scn.buildings.len()).map(|i| buildings.get(&i).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
            "last_observation_s":self.knowledge_events.iter().rev().find(|e| e.role == role).map(|e| e.t_s),
            "last_observation_clock":self.knowledge_events.iter().rev().find(|e| e.role == role).map(|e| &e.clock)}),
        )
    }

    /// Explicit allowlist of pre-exercise geography. No MSEL, initial conditions,
    /// hidden population counts, other-role inventories or reinforcement schedules.
    pub fn player_static(&self, role: &str) -> Result<Value> {
        let own = self
            .world
            .scn
            .role(role)
            .ok_or_else(|| anyhow::anyhow!("unknown role {role}"))?;
        let full = self.world.static_view();
        Ok(
            json!({"id":full["id"], "name":full["name"], "grid":full["grid"], "zones":full["zones"], "fuel":full["fuel"],
            "buildings":self.world.scn.buildings.iter().map(|b| json!({"id":b.id,"name":b.name,"pos":b.pos,"size":b.size})).collect::<Vec<_>>(),
            "roads":self.world.scn.roads.iter().map(|r| json!({"id":r.id,"name":r.name,"points":r.points})).collect::<Vec<_>>(),
            "places":full["places"], "roles":[own],
            "contacts":self.world.scn.roles.iter().map(|r|json!({"id":r.id,"name":r.name})).collect::<Vec<_>>(), "resource_types":full["resource_types"]}),
        )
    }
}
