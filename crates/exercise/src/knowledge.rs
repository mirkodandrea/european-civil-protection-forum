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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapObservation {
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
            format!("EXCON-reviewed projection received from {source}. Issued at t={} s; horizons {horizons}. {}",product.issued_s,product.assumptions)
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
        MapObservation {
            forecast_products: Vec::new(),
            locations: Vec::new(),
            cells: bytes
                .into_iter()
                .enumerate()
                .map(|(i, state)| FireCell {
                    row: i / cols,
                    col: i % cols,
                    state,
                })
                .collect(),
        }
    }

    pub fn player_map(&self, role: &str) -> Result<Value> {
        if !self.players.contains_key(role) {
            bail!("unknown role {role}");
        }
        let mut cells = std::collections::BTreeMap::new();
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
            for location in &event.observation.locations {
                if !locations.contains(location) {
                    locations.push(*location);
                }
            }
        }
        Ok(
            json!({"cells":cells.values().collect::<Vec<_>>(), "reported_locations":locations,
            "forecast_products":forecast_products,
            "roads":{}, "buildings":[], "lines":[], "drops":[], "warned":[], "burning_zones":[],
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
            "places":full["places"], "roles":[own], "resource_types":full["resource_types"]}),
        )
    }
}
