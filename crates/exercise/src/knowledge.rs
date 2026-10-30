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

/// A population warning in force in an observed zone (zone and time only, never
/// the people still inside).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WarningObs {
    pub zone: String,
    /// Simulated second the warning was issued.
    pub t_s: i64,
    pub clock: String,
}

pub use scenario::ZONE_LAYERS;

/// An unverified report of where the fire is: a circle with a spread
/// direction, deliberately inexact (see [`Exercise::fire_estimate`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FireEstimate {
    /// Reported centre, metres (x east, y north).
    pub at: [f32; 2],
    /// Radius of the area where the fire is said to be: reported extent plus the position error.
    pub radius_m: f32,
    pub area_ha: f32,
    /// Bearing the fire spreads toward, degrees clockwise from north.
    #[serde(default)]
    pub spread_toward_deg: Option<f32>,
    /// Largest position error the report may carry, metres.
    #[serde(default)]
    pub error_m: f32,
}

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
    /// Unverified fire estimates; the latest one a role received is shown.
    #[serde(default)]
    pub estimates: Vec<FireEstimate>,
    /// Population warnings in force in `zones` when observed (part of the
    /// `buildings` layer). `None`: warnings were not observed, so what the role
    /// knew of these zones stands; `Some` replaces it for every zone in `zones`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<WarningObs>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    Delivered,
    Acquired,
    /// Periodic observation by a drone on station; tied to no message.
    Monitoring,
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
    /// Who the information comes from, as the player sees it.
    pub source_name: Option<String>,
    /// Layers of a zone reveal (empty for other observations): a refresh repeats them.
    pub layers: Vec<String>,
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
        if o.estimates.len() > 4
            || o.estimates.iter().any(|e| {
                !e.at.iter().all(|v| v.is_finite())
                    || e.at[0] < 0.0
                    || e.at[1] < 0.0
                    || e.at[0] > g.cols as f32 * g.cellsize
                    || e.at[1] > g.rows as f32 * g.cellsize
                    || !(0.0..=100_000.0).contains(&e.radius_m)
                    || !(0.0..=10_000_000.0).contains(&e.area_ha)
                    || !(0.0..=100_000.0).contains(&e.error_m)
                    || e.spread_toward_deg.is_some_and(|d| !(0.0..360.0).contains(&d))
            })
        {
            bail!("invalid fire estimate");
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
        if let Some(ws) = &o.warnings {
            if ws.iter().any(|w| {
                !o.zones.contains(&w.zone)
                    || w.t_s < 0
                    || w.t_s > self.world.time_s()
                    || w.clock.chars().count() > 40
            }) {
                bail!("invalid reported warning");
            }
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
        self.attach_map_observation_named(mid, observation, source, None, Vec::new())
    }

    /// As [`Self::attach_map_observation`], recording who the information comes
    /// from (default: the message sender) and the layers of a zone reveal.
    pub fn attach_map_observation_named(
        &mut self,
        mid: u64,
        observation: MapObservation,
        source: KnowledgeSource,
        source_name: Option<String>,
        layers: Vec<String>,
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
        let source_name = source_name.or_else(|| (m.from != MASTER).then(|| m.from.clone()));
        let event = KnowledgeEvent {
            role: m.to.clone(),
            message: mid,
            turn: m.turn,
            t_s: m.t_s,
            clock: m.clock.clone(),
            source,
            observation,
            timeline: self.log.iter().filter(|e| e.kind == "rollback").count(),
            source_name,
            layers,
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
        self.acquire_map_layers(role, source, observation, translated, Vec::new())
    }

    /// As [`Self::acquire_map`] for a zone reveal: `layers` are remembered so a
    /// refresh can repeat it.
    pub fn acquire_map_layers(
        &mut self,
        role: &str,
        source: &str,
        observation: MapObservation,
        translated: Option<&str>,
        layers: Vec<String>,
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
        self.attach_map_observation_named(
            mid,
            observation,
            KnowledgeSource::Acquired,
            Some(source.to_string()),
            layers,
        )?;
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
        self.zone_observation_from(&self.world.view(), zones, layers)
    }

    /// As [`Self::zone_observation`] for the world as it was in `view` (a [`sim::World::view`]).
    pub fn zone_observation_from(&self, view: &Value, zones: &[String], layers: &[String]) -> Result<MapObservation> {
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
        let pos = |v: &Value| -> Option<[f32; 2]> {
            Some([v[0].as_f64()? as f32, v[1].as_f64()? as f32])
        };
        let mut o = MapObservation {
            zones: zones.iter().map(|z| z.to_uppercase()).collect(),
            ..Default::default()
        };
        if has("fire") {
            let raster = base64::engine::general_purpose::STANDARD
                .decode(view["fire"].as_str().unwrap_or_default())
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
            // Warnings in force in these zones (zone and time; the population stays hidden).
            let warned = view["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|w| {
                    let zone = w["zone"].as_str()?.to_string();
                    o.zones.contains(&zone).then(|| WarningObs {
                        zone,
                        t_s: w["t_s"].as_i64().unwrap_or(0),
                        clock: w["clock"].as_str().unwrap_or_default().into(),
                    })
                })
                .collect();
            o.warnings = Some(warned);
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

    /// Zones an exact reveal covers, resolved now.
    pub fn reveal_zones(&self, zones: &scenario::RevealZones) -> Result<Vec<String>> {
        use scenario::{RevealZones, ZoneRule};
        let burning = || -> Result<Vec<String>> {
            let b = self.world.burning_zones();
            if b.is_empty() {
                bail!("no zone is burning");
            }
            Ok(b)
        };
        Ok(match zones {
            RevealZones::List(z) => z.iter().map(|z| z.to_uppercase()).collect(),
            RevealZones::Rule(ZoneRule::Burning) => burning()?,
            RevealZones::Rule(ZoneRule::BurningAndAdjacent) => self.world.scn.zones.with_neighbours(&burning()?),
        })
    }

    /// The actual fire (cells burning or burnt, which is what the fire raster
    /// shows): centroid in metres, area in hectares and the bearing it spreads
    /// toward. The bearing is the direction from the centroid of the burnt
    /// cells (where the fire has been) to that of the burning cells (the
    /// front), a proxy for the fastest-advancing front; when the two are less
    /// than one cell apart (fire just started, or all still burning) it is the
    /// wind blowing toward, i.e. the wind direction (given FROM) plus 180°.
    pub fn actual_fire(&self) -> Option<([f32; 2], f64, f32)> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(self.world.fire_raster_b64()).ok()?;
        let g = &self.world.scn.grid;
        let (mut all, mut burning, mut burnt) = (([0f64; 2], 0usize), ([0f64; 2], 0usize), ([0f64; 2], 0usize));
        for (i, state) in bytes.into_iter().enumerate() {
            let slot = match state {
                sim::digest::cell_code::BURNING => &mut burning,
                sim::digest::cell_code::BURNT => &mut burnt,
                _ => continue,
            };
            let (row, col) = (i / g.cols, i % g.cols);
            let p = [(col as f64 + 0.5) * g.cellsize as f64, g.height_m() as f64 - (row as f64 + 0.5) * g.cellsize as f64];
            for s in [&mut all, slot] {
                s.0[0] += p[0];
                s.0[1] += p[1];
                s.1 += 1;
            }
        }
        if all.1 == 0 {
            return None;
        }
        let mean = |s: &([f64; 2], usize)| [s.0[0] / s.1 as f64, s.0[1] / s.1 as f64];
        let centroid = mean(&all);
        let area_ha = all.1 as f64 * (g.cellsize as f64).powi(2) / 10_000.0;
        let bearing = |dx: f64, dy: f64| dx.atan2(dy).to_degrees().rem_euclid(360.0) as f32;
        let front = (burning.1 > 0 && burnt.1 > 0).then(|| {
            let (a, b) = (mean(&burnt), mean(&burning));
            (b[0] - a[0], b[1] - a[1])
        });
        let spread = match front.filter(|(dx, dy)| dx.hypot(*dy) > g.cellsize as f64) {
            Some((dx, dy)) => bearing(dx, dy),
            None => (self.world.fire.weather().wind_dir_deg + 180.0).rem_euclid(360.0) as f32,
        };
        Some(([centroid[0] as f32, centroid[1] as f32], area_ha, spread))
    }

    /// A deliberately wrong report of the actual fire. The error is a pure
    /// function of the scenario seed and `key` (inject, action and role), so a
    /// replay or a second recipient with the same key reads the same thing:
    /// position within `position_error_m` of the true centroid, area within
    /// ±`area_error_frac` and rounded to two significant digits, direction
    /// within ±10° and rounded to 5°.
    pub fn fire_estimate(&self, key: &str, position_error_m: f32, area_error_frac: f32) -> Result<FireEstimate> {
        let Some((centroid, area_ha, spread)) = self.actual_fire() else {
            bail!("there is no fire to report");
        };
        // FNV-1a over seed and key, then splitmix64 for each draw.
        let mut state = 0xcbf29ce484222325u64;
        for b in self.world.seed.to_le_bytes().iter().chain(key.as_bytes()) {
            state = (state ^ *b as u64).wrapping_mul(0x100000001b3);
        }
        let mut draw = move || {
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        };
        let g = &self.world.scn.grid;
        let dist = position_error_m as f64 * draw().sqrt();
        let angle = std::f64::consts::TAU * draw();
        let at = [
            (centroid[0] + (dist * angle.cos()) as f32).clamp(0.0, g.cols as f32 * g.cellsize - 0.01),
            (centroid[1] + (dist * angle.sin()) as f32).clamp(0.0, g.rows as f32 * g.cellsize - 0.01),
        ];
        let raw = (area_ha * (1.0 + area_error_frac as f64 * (2.0 * draw() - 1.0))).max(0.1);
        let step = 10f64.powf(raw.log10().floor() - 1.0);
        let area = ((raw / step).round() * step).max(0.1);
        let spread = ((spread as f64 + (2.0 * draw() - 1.0) * 10.0) / 5.0).round() * 5.0;
        Ok(FireEstimate {
            at,
            radius_m: ((area * 10_000.0 / std::f64::consts::PI).sqrt() + position_error_m as f64) as f32,
            area_ha: area as f32,
            spread_toward_deg: Some(spread.rem_euclid(360.0) as f32),
            error_m: position_error_m,
        })
    }

    /// Roles whose latest zone reveal a refresh would repeat, with its zones and layers.
    pub fn refresh_plan(&self) -> Vec<Value> {
        let timeline = self.log.iter().filter(|e| e.kind == "rollback").count();
        self.players
            .keys()
            .filter_map(|role| {
                let e = self.knowledge_events.iter().rev().find(|e| {
                    &e.role == role
                        && e.timeline == timeline
                        && !matches!(e.source, KnowledgeSource::Monitoring)
                        && !e.layers.is_empty()
                        && !e.observation.zones.is_empty()
                })?;
                Some(json!({"role":role,"zones":e.observation.zones,"layers":e.layers,"clock":e.clock,"source":e.source_name}))
            })
            .collect()
    }

    /// Re-reveal to each role that received a zone reveal the same zones and
    /// layers as a fresh snapshot of the world now. `source` is who the players
    /// hear it from (default: the scenario's operations-room persona in the
    /// role's language); `translations` maps role → message text in its
    /// language. Returns `(role, message)` per update.
    pub fn refresh_role_maps(
        &mut self,
        source: Option<&str>,
        translations: &std::collections::BTreeMap<String, String>,
    ) -> Result<Vec<(String, u64)>> {
        if self.phase == crate::Phase::Debrief {
            bail!("the exercise has ended");
        }
        let mut out = Vec::new();
        for item in self.refresh_plan() {
            let role = item["role"].as_str().unwrap_or_default().to_string();
            let strings = |k: &str| -> Vec<String> {
                item[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
            };
            let (zones, layers) = (strings("zones"), strings("layers"));
            let observation = self.zone_observation(&zones, &layers)?;
            let source = source.map(String::from).unwrap_or_else(|| self.persona_name(None, &self.language_of(&role)));
            let mid = self.acquire_map_layers(&role, &source, observation, translations.get(&role).map(String::as_str), layers)?;
            out.push((role, mid));
        }
        if out.is_empty() {
            bail!("no role has received a zone reveal yet");
        }
        Ok(out)
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
                        json!({"zones":self.known_zones(role),"last_clock":last.map(|e| &e.clock),"events":self.knowledge_events.iter().filter(|e| &e.role == role).count(),
                            "refresh":self.refresh_plan().into_iter().find(|p| p["role"] == role.as_str())}),
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
        let mut zone_updates = serde_json::Map::new();
        // Warned zones the role has been told about: the latest observation of each zone wins.
        let mut warnings = std::collections::BTreeMap::new();
        for event in self.knowledge_events.iter().filter(|e| e.role == role) {
            if let Some(ws) = &event.observation.warnings {
                for z in &event.observation.zones {
                    warnings.remove(z);
                }
                for w in ws {
                    warnings.insert(w.zone.clone(), w);
                }
            }
            for z in &event.observation.zones {
                zone_updates.insert(z.clone(), json!(event.t_s));
            }
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
        let mut warnings_sorted = warnings.values().collect::<Vec<_>>();
        warnings_sorted.sort_by_key(|w| (w.t_s, w.zone.clone()));
        Ok(
            json!({"cells":cells.values().collect::<Vec<_>>(), "reported_locations":locations,
            "forecast_products":forecast_products,
            "known_zones":self.known_zones(role), "observed_units":units,
            "roads":roads, "lines":lines, "drops":drops, "burning_zones":[],
            // Warned zones as last observed by this role (fog of war), zone and time only.
            "warned":warnings.keys().collect::<Vec<_>>(),
            "warnings":warnings_sorted,
            "buildings":(0..self.world.scn.buildings.len()).map(|i| buildings.get(&i).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
            // The latest estimate, until a later disclosure of the fire itself supersedes it.
            "estimates":self.knowledge_events.iter().enumerate().rev().find(|(_, e)| e.role == role && !e.observation.estimates.is_empty())
                .filter(|(i, _)| !self.knowledge_events[i + 1..].iter().any(|e| e.role == role && !e.observation.cells.is_empty()))
                .map(|(_, e)| e.observation.estimates.iter().map(|x| json!({"at":x.at,"radius_m":x.radius_m,"area_ha":x.area_ha,"spread_toward_deg":x.spread_toward_deg,"error_m":x.error_m,"source":e.source_name,"clock":e.clock,"t_s":e.t_s})).collect::<Vec<_>>())
                .unwrap_or_default(),
            "drones":self.world.drone_status(role),
            "live_zones":self.world.drone_status(role).iter().filter(|d| d["status"] == "on_station")
                .flat_map(|d| d["zones"].as_array().cloned().unwrap_or_default()).collect::<Vec<_>>(),
            "zone_updates":zone_updates,
            "last_observation_source":self.knowledge_events.iter().rev().find(|e| e.role == role).map(|e| &e.source_name),
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
            json!({"id":full["id"], "name":full["name"], "grid":full["grid"], "zones":full["zones"], "fuel":full["fuel"], "fuel_names":full["fuel_names"],
            "buildings":self.world.scn.buildings.iter().map(|b| json!({"id":b.id,"kind":b.kind,"name":b.name,"pos":b.pos,"size":b.size})).collect::<Vec<_>>(),
            "roads":self.world.scn.roads.iter().map(|r| json!({"id":r.id,"name":r.name,"points":r.points})).collect::<Vec<_>>(),
            "places":full["places"], "roles":[own],
            "contacts":self.world.scn.roles.iter().map(|r|json!({"id":r.id,"name":r.name})).collect::<Vec<_>>(), "resource_types":full["resource_types"]}),
        )
    }
}
