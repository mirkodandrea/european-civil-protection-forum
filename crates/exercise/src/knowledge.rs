//! Information acquired by a role, independent of the authoritative simulation.
use anyhow::{bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{Exercise, MsgKind, MASTER};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FireCell { pub row: usize, pub col: usize, pub state: u8 }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapObservation {
    #[serde(default)] pub locations: Vec<[f32; 2]>,
    #[serde(default)] pub cells: Vec<FireCell>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource { Delivered, Acquired }

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeEvent {
    pub role: String,
    pub message: u64,
    pub turn: u32,
    pub t_s: i64,
    pub clock: String,
    pub source: KnowledgeSource,
    pub observation: MapObservation,
}

impl Exercise {
    pub fn validate_observation(&self, o: &MapObservation) -> Result<()> {
        let g = &self.world.scn.grid;
        if o.locations.len() > 1000 || o.cells.len() > g.rows * g.cols { bail!("map observation is too large"); }
        for [x, y] in &o.locations {
            if !x.is_finite() || !y.is_finite() || *x < 0.0 || *y < 0.0 || *x >= g.cols as f32 * g.cellsize || *y >= g.rows as f32 * g.cellsize { bail!("reported location is outside the map"); }
        }
        if o.cells.iter().any(|c| c.row >= g.rows || c.col >= g.cols || c.state > 5) { bail!("invalid reported fire cell"); }
        Ok(())
    }

    /// Attach only to an actual recipient's delivered message, once. No inference from text.
    pub fn attach_map_observation(&mut self, mid: u64, observation: MapObservation, source: KnowledgeSource) -> Result<()> {
        self.validate_observation(&observation)?;
        let m = self.messages.iter().find(|m| m.id == mid).ok_or_else(|| anyhow::anyhow!("unknown delivered message"))?;
        if !self.players.contains_key(&m.to) || !matches!(m.kind, MsgKind::Inject | MsgKind::RoleMessage | MsgKind::Situation) { bail!("map information must accompany a delivered operational message"); }
        if self.knowledge_events.iter().any(|e| e.message == mid) { bail!("message already has a map observation"); }
        let event = KnowledgeEvent { role:m.to.clone(), message:mid, turn:m.turn, t_s:m.t_s, clock:m.clock.clone(), source, observation };
        self.record("map_information_acquired", MASTER, json!(&event));
        self.knowledge_events.push(event);
        Ok(())
    }

    pub fn acquire_map(&mut self, role: &str, source: &str, observation: MapObservation, translated: Option<&str>) -> Result<u64> {
        if self.phase == crate::Phase::Debrief { bail!("the exercise has ended"); }
        if !self.players.contains_key(role) || source.trim().is_empty() || source.chars().count() > 2000 { bail!("known role and information source required"); }
        self.validate_observation(&observation)?;
        let text = Self::map_information_text(source);
        let mid = self.add_message(MASTER, role, MsgKind::Situation, Some("Map information"), translated.unwrap_or(&text), &text, None);
        self.attach_map_observation(mid, observation, KnowledgeSource::Acquired)?;
        Ok(mid)
    }

    pub fn map_information_text(source: &str) -> String {
        format!("Map information received from {source}. Consult the attached situation map; reported data are observations at delivery time.")
    }

    /// EXCON explicitly chooses to disclose this snapshot; never called by player reads.
    pub fn current_perimeter(&self) -> MapObservation {
        let bytes = base64::engine::general_purpose::STANDARD.decode(self.world.fire_raster_b64()).expect("simulator emits base64");
        let cols = self.world.scn.grid.cols;
        MapObservation { locations:Vec::new(), cells:bytes.into_iter().enumerate().map(|(i, state)| FireCell {row:i / cols, col:i % cols, state}).collect() }
    }

    pub fn player_map(&self, role: &str) -> Result<Value> {
        if !self.players.contains_key(role) { bail!("unknown role {role}"); }
        let mut cells = std::collections::BTreeMap::new();
        let mut locations = Vec::new();
        for event in self.knowledge_events.iter().filter(|e| e.role == role) {
            for cell in &event.observation.cells { cells.insert((cell.row, cell.col), cell); }
            for location in &event.observation.locations { if !locations.contains(location) { locations.push(*location); } }
        }
        Ok(json!({"cells":cells.values().collect::<Vec<_>>(), "reported_locations":locations,
            "roads":{}, "buildings":[], "lines":[], "drops":[], "warned":[], "burning_zones":[],
            "last_observation_s":self.knowledge_events.iter().rev().find(|e| e.role == role).map(|e| e.t_s),
            "last_observation_clock":self.knowledge_events.iter().rev().find(|e| e.role == role).map(|e| &e.clock)}))
    }

    /// Explicit allowlist of pre-exercise geography. No MSEL, initial conditions,
    /// hidden population counts, other-role inventories or reinforcement schedules.
    pub fn player_static(&self, role: &str) -> Result<Value> {
        let own = self.world.scn.role(role).ok_or_else(|| anyhow::anyhow!("unknown role {role}"))?;
        let full = self.world.static_view();
        Ok(json!({"id":full["id"], "name":full["name"], "grid":full["grid"], "zones":full["zones"], "fuel":full["fuel"],
            "buildings":self.world.scn.buildings.iter().map(|b| json!({"id":b.id,"name":b.name,"pos":b.pos,"size":b.size})).collect::<Vec<_>>(),
            "roads":self.world.scn.roads.iter().map(|r| json!({"id":r.id,"name":r.name,"points":r.points})).collect::<Vec<_>>(),
            "places":full["places"], "roles":[own], "resource_types":full["resource_types"]}))
    }
}
