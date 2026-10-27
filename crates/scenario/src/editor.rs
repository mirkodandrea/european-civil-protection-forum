//! Headless authoring document. Draft edits never touch a running World.
use crate::*;
use anyhow::{ensure, Result};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Building(usize),
    Road(usize),
    Place(usize),
}

pub struct Document {
    pub scene: Arc<Scenario>,
    pub revision: u64,
    saved: Arc<Scenario>,
    undo: Vec<Arc<Scenario>>,
    redo: Vec<Arc<Scenario>>,
    /// While `Some`, edits belong to one brush stroke: one undo step, pushed on the first edit.
    stroke: Option<bool>,
}

/// What a new building looks like when placed.
#[derive(Clone, Debug)]
pub struct BuildingSpec {
    pub kind: BuildingKind,
    pub visual_kind: Option<String>,
    pub size: [f32; 2],
    pub rotation_deg: f32,
    pub occupants: u32,
}
impl Default for BuildingSpec {
    fn default() -> Self {
        Self {
            kind: BuildingKind::House,
            visual_kind: None,
            size: [20., 20.],
            rotation_deg: 0.,
            occupants: 0,
        }
    }
}

/// Presentation variants understood by the 3D viewer (`Building::visual_kind`).
pub const VISUAL_KINDS: &[&str] = &[
    "church", "chapel", "fire_station", "farm", "barn", "silo", "mill", "water_tower",
    "substation", "lighthouse", "plaza", "square", "pitch", "parking", "assembly", "lido",
    "campsite", "harbour", "cemetery",
];
/// Italian names for `VISUAL_KINDS`, same order.
pub const VISUAL_LABELS: &[&str] = &[
    "Chiesa", "Cappella", "Caserma vigili del fuoco", "Cascina", "Fienile", "Silo", "Mulino",
    "Torre idrica", "Cabina elettrica", "Faro", "Piazza", "Piazza (quadrata)", "Campo sportivo",
    "Parcheggio", "Area di raccolta", "Lido", "Campeggio", "Porto", "Cimitero",
];
/// Ready-made facilities for the building tool: label, kind, visual variant, footprint (m), occupants.
pub const FACILITY_PRESETS: &[(&str, BuildingKind, Option<&str>, [f32; 2], u32)] = &[
    ("Abitazione", BuildingKind::House, None, [14., 10.], 4),
    ("Scuola", BuildingKind::School, None, [40., 20.], 150),
    ("Ospedale", BuildingKind::Hospital, None, [50., 30.], 120),
    ("Distributore di carburante", BuildingKind::FuelStation, None, [25., 15.], 3),
    ("Centro sportivo", BuildingKind::SportsCentre, None, [45., 30.], 40),
    ("Municipio", BuildingKind::TownHall, None, [35., 20.], 30),
    ("Zona industriale", BuildingKind::Industrial, None, [50., 35.], 25),
    ("Chiesa", BuildingKind::House, Some("church"), [30., 14.], 0),
    ("Cappella", BuildingKind::House, Some("chapel"), [10., 7.], 0),
    ("Caserma vigili del fuoco", BuildingKind::Industrial, Some("fire_station"), [35., 20.], 12),
    ("Cascina", BuildingKind::House, Some("farm"), [30., 20.], 6),
    ("Fienile", BuildingKind::House, Some("barn"), [24., 12.], 0),
    ("Silo", BuildingKind::Industrial, Some("silo"), [10., 10.], 0),
    ("Mulino", BuildingKind::Industrial, Some("mill"), [16., 12.], 2),
    ("Torre idrica", BuildingKind::Industrial, Some("water_tower"), [8., 8.], 0),
    ("Cabina elettrica", BuildingKind::Industrial, Some("substation"), [12., 10.], 0),
    ("Faro", BuildingKind::House, Some("lighthouse"), [8., 8.], 0),
    ("Piazza", BuildingKind::House, Some("plaza"), [50., 50.], 0),
    ("Campo sportivo", BuildingKind::SportsCentre, Some("pitch"), [60., 35.], 0),
    ("Parcheggio", BuildingKind::House, Some("parking"), [40., 25.], 0),
    ("Area di raccolta", BuildingKind::House, Some("assembly"), [50., 40.], 0),
    ("Lido", BuildingKind::House, Some("lido"), [60., 25.], 0),
    ("Campeggio", BuildingKind::House, Some("campsite"), [70., 50.], 40),
    ("Porto", BuildingKind::Industrial, Some("harbour"), [60., 30.], 5),
    ("Cimitero", BuildingKind::House, Some("cemetery"), [40., 30.], 0),
];
pub fn visual_label(kind: &str) -> &str {
    VISUAL_KINDS
        .iter()
        .position(|k| *k == kind)
        .map_or(kind, |i| VISUAL_LABELS[i])
}
/// Distance from `p` to segment `a`–`b`, and the closest point on it.
pub fn nearest_on_segment(p: Pos, a: Pos, b: Pos) -> (f32, Pos) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0. {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0., 1.)
    } else {
        0.
    };
    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
    (distance(p, q), q)
}
impl Document {
    pub fn new(scene: Scenario) -> Result<Self> {
        validate(&scene)?;
        let scene = Arc::new(scene);
        Ok(Self {
            saved: scene.clone(),
            scene,
            revision: 1,
            undo: vec![],
            redo: vec![],
            stroke: None,
        })
    }
    pub fn dirty(&self) -> bool {
        !Arc::ptr_eq(&self.scene, &self.saved)
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.scene.clone();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn edit(&mut self, f: impl FnOnce(&mut Scenario) -> Result<()>) -> Result<()> {
        let mut next = (*self.scene).clone();
        f(&mut next)?;
        flood_below_sea_level(&mut next);
        validate(&next)?;
        let push = match &mut self.stroke {
            Some(done) => !std::mem::replace(done, true),
            None => true,
        };
        if push {
            self.undo.push(self.scene.clone());
            if self.undo.len() > 32 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.scene = Arc::new(next);
        self.revision += 1;
        Ok(())
    }
    /// Groups the edits until `end_stroke` into a single undo step.
    pub fn begin_stroke(&mut self) {
        self.stroke = Some(false);
    }
    pub fn end_stroke(&mut self) {
        self.stroke = None;
    }
    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.scene.clone());
            self.scene = s;
            self.revision += 1;
        }
    }
    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.scene.clone());
            self.scene = s;
            self.revision += 1;
        }
    }
    pub fn import(&mut self, json: &str) -> Result<()> {
        let mut s: Scenario = serde_json::from_str(json)?;
        s.migrate_legacy_timeline()?;
        self.edit(|next| {
            *next = s;
            Ok(())
        })
    }
    pub fn export(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&*self.scene)?)
    }
    pub fn clone_as(&mut self, id: &str) -> Result<()> {
        ensure!(
            id != self.scene.id,
            "La copia richiede un identificativo diverso dall’originale"
        );
        self.edit(|s| {
            s.id = id.into();
            s.name = format!("{} (copia)", s.name);
            Ok(())
        })
    }
    pub fn brush(
        &mut self,
        at: Pos,
        radius: f32,
        elevation: Option<f32>,
        fuel: Option<i32>,
    ) -> Result<()> {
        ensure!(radius.is_finite() && radius > 0., "Raggio non valido");
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            if let Some(h) = elevation {
                ensure!(h.is_finite(), "Quota non valida");
            }
            if let Some(code) = fuel {
                ensure!(
                    s.fuel_definitions.iter().any(|f| f.id == code)
                        || (s.fuel_definitions.is_empty() && (1..=7).contains(&code)),
                    "Combustibile sconosciuto"
                );
            }
            for i in 0..s.grid.len() {
                let p = s.cell_centre(Cell {
                    row: i / s.grid.cols,
                    col: i % s.grid.cols,
                });
                if distance(p, at) <= radius {
                    if let Some(h) = elevation {
                        s.dem[i] = h;
                    }
                    if let Some(c) = fuel {
                        s.fuel[i] = c;
                    }
                }
            }
            // Keep authored high-resolution geometry consistent with physics.
            if let (Some(t), Some(h)) = (&mut s.render_terrain, elevation) {
                let (rows, cols) = t.footprint(at, radius);
                for row in rows {
                    for col in cols.clone() {
                        let p = [
                            col as f32 * t.posting_m,
                            t.world_size_m[1] - row as f32 * t.posting_m,
                        ];
                        if distance(p, at) <= radius {
                            t.elev[row * t.cols + col] = h;
                        }
                    }
                }
            }
            Ok(())
        })
    }
    /// Push/pull: raises (`delta_m` > 0) or lowers the ground with a smooth falloff.
    pub fn sculpt(&mut self, at: Pos, radius: f32, delta_m: f32) -> Result<()> {
        ensure!(radius.is_finite() && radius > 0., "Raggio non valido");
        ensure!(delta_m.is_finite(), "Variazione non valida");
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            let weight = |p: Pos| {
                let t = 1. - distance(p, at) / radius;
                (t > 0.).then(|| t * t * (3. - 2. * t))
            };
            for i in 0..s.grid.len() {
                let p = s.cell_centre(Cell {
                    row: i / s.grid.cols,
                    col: i % s.grid.cols,
                });
                if let Some(w) = weight(p) {
                    s.dem[i] = (s.dem[i] + delta_m * w).clamp(-9000., 9000.);
                }
            }
            if let Some(t) = &mut s.render_terrain {
                let (rows, cols) = t.footprint(at, radius);
                for row in rows {
                    for col in cols.clone() {
                        let p = [
                            col as f32 * t.posting_m,
                            t.world_size_m[1] - row as f32 * t.posting_m,
                        ];
                        if let Some(w) = weight(p) {
                            let v = &mut t.elev[row * t.cols + col];
                            *v = (*v + delta_m * w).clamp(-9000., 9000.);
                        }
                    }
                }
            }
            Ok(())
        })
    }
    pub fn add_building(&mut self, at: Pos, spec: BuildingSpec) -> Result<Selection> {
        let mut selected = Selection::Building(0);
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            ensure!(!underwater(s, at), "Punto sott'acqua");
            ensure!(
                spec.size.iter().all(|v| v.is_finite() && *v >= 3.) && spec.rotation_deg.is_finite(),
                "Dimensioni o rotazione non valide"
            );
            let id = s
                .buildings
                .iter()
                .map(|b| b.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("ID esauriti"))?;
            let sector = s
                .sector_at(at)
                .ok_or_else(|| anyhow::anyhow!("Nessun settore in questo punto"))?
                .id
                .clone();
            s.buildings.push(Building {
                id,
                kind: spec.kind,
                visual_kind: spec.visual_kind,
                name: Some(format!("Edificio {id}")),
                pos: at,
                size: spec.size,
                occupants: spec.occupants,
                sector,
                shelter_capacity: 0,
                rotation_deg: spec.rotation_deg.rem_euclid(360.),
            });
            selected = Selection::Building(s.buildings.len() - 1);
            Ok(())
        })?;
        Ok(selected)
    }
    pub fn add(&mut self, kind: &str, at: Pos, end: Option<Pos>) -> Result<Selection> {
        if kind == "building" {
            return self.add_building(at, BuildingSpec::default());
        }
        let mut selected = Selection::Place(0);
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            selected = match kind {
                "road" => {
                    let end = end.ok_or_else(|| anyhow::anyhow!("Scegli il secondo punto"))?;
                    ensure!(
                        inside(s, end) && distance(at, end) >= 1.,
                        "Estremi strada non validi"
                    );
                    let mut n = s.roads.len() + 1;
                    while s.roads.iter().any(|r| r.id == format!("road-{n}")) {
                        n += 1;
                    }
                    s.roads.push(Road {
                        id: format!("road-{n}"),
                        name: format!("Strada {n}"),
                        points: vec![at, end],
                        class: "residential".into(),
                        drivable: true,
                        oneway: false,
                    });
                    Selection::Road(s.roads.len() - 1)
                }
                _ => {
                    let mut n = s.places.len() + 1;
                    while s.places.iter().any(|p| p.name == format!("Luogo {n}")) {
                        n += 1;
                    }
                    s.places.push(Place {
                        name: format!("Luogo {n}"),
                        aliases: vec![],
                        pos: at,
                        ..Default::default()
                    });
                    Selection::Place(s.places.len() - 1)
                }
            };
            Ok(())
        })?;
        Ok(selected)
    }
    /// Appends a vertex to an existing road, turning it into a polyline.
    pub fn extend_road(&mut self, index: usize, at: Pos) -> Result<()> {
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            let road = s
                .roads
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Strada inesistente"))?;
            let last = *road.points.last().expect("roads have points");
            ensure!(distance(last, at) >= 1., "Vertice coincidente con il precedente");
            road.points.push(at);
            Ok(())
        })
    }
    /// Inserts a vertex into segment `segment` (between points `segment` and `segment + 1`).
    /// Returns the index of the new vertex.
    pub fn insert_road_vertex(&mut self, index: usize, segment: usize, at: Pos) -> Result<usize> {
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            let road = s
                .roads
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Strada inesistente"))?;
            ensure!(segment + 1 < road.points.len(), "Segmento inesistente");
            road.points.insert(segment + 1, at);
            Ok(())
        })?;
        Ok(segment + 1)
    }
    pub fn move_road_vertex(&mut self, index: usize, vertex: usize, at: Pos) -> Result<()> {
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            let p = s
                .roads
                .get_mut(index)
                .and_then(|r| r.points.get_mut(vertex))
                .ok_or_else(|| anyhow::anyhow!("Vertice inesistente"))?;
            *p = at;
            Ok(())
        })
    }
    /// Removes a vertex; a road keeps at least two.
    pub fn remove_road_vertex(&mut self, index: usize, vertex: usize) -> Result<()> {
        self.edit(|s| {
            let road = s
                .roads
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Strada inesistente"))?;
            ensure!(road.points.len() > 2, "Una strada ha almeno due vertici");
            ensure!(vertex < road.points.len(), "Vertice inesistente");
            road.points.remove(vertex);
            Ok(())
        })
    }
    /// Moves a building or a place; a building follows the sector it lands in.
    pub fn move_object(&mut self, selection: Selection, at: Pos) -> Result<()> {
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            match selection {
                Selection::Building(i) => {
                    ensure!(!underwater(s, at), "Punto sott'acqua");
                    let sector = s
                        .sector_at(at)
                        .ok_or_else(|| anyhow::anyhow!("Nessun settore in questo punto"))?
                        .id
                        .clone();
                    let b = s
                        .buildings
                        .get_mut(i)
                        .ok_or_else(|| anyhow::anyhow!("Edificio inesistente"))?;
                    b.pos = at;
                    b.sector = sector;
                }
                Selection::Place(i) => {
                    s.places
                        .get_mut(i)
                        .ok_or_else(|| anyhow::anyhow!("Luogo inesistente"))?
                        .pos = at;
                }
                Selection::Road(_) => anyhow::bail!("Sposta i vertici della strada"),
            }
            Ok(())
        })
    }
    pub fn delete(&mut self, selection: Selection) -> Result<()> {
        self.edit(|s| {
            match selection {
                Selection::Building(i) if i < s.buildings.len() => {
                    let b = s.buildings.remove(i);
                    s.places.iter_mut().for_each(|p| p.buildings.retain(|id| *id != b.id));
                }
                Selection::Road(i) if i < s.roads.len() => {
                    let r = s.roads.remove(i);
                    s.places.iter_mut().for_each(|p| p.roads.retain(|id| *id != r.id));
                }
                Selection::Place(i) if i < s.places.len() => {
                    let p = s.places.remove(i);
                    if s.staging_base.as_ref() == Some(&p.name) {
                        s.staging_base = None;
                    }
                }
                _ => anyhow::bail!("Selezione non valida"),
            };
            Ok(())
        })
    }
}
/// Ground at or below this elevation (m) is water.
pub const SEA_LEVEL_M: f32 = 0.0;

/// Fuel code of cells that are water: the scenario's non-burnable fuel
/// (legacy table: non-vegetated).
fn water_fuel(s: &Scenario) -> Option<i32> {
    if s.fuel_definitions.is_empty() {
        return Some(fuel::NON_VEGETATED);
    }
    let named = |f: &&FuelDefinition| {
        let n = f.name.to_lowercase();
        n.contains("water") || n.contains("acqua")
    };
    let inert = || s.fuel_definitions.iter().filter(|f| !f.burn);
    inert().find(named).or_else(|| inert().next()).map(|f| f.id)
}
/// Cells below sea level are lakes or sea: they cannot carry vegetation or fire.
pub fn flood_below_sea_level(s: &mut Scenario) {
    let Some(water) = water_fuel(s) else { return };
    for (fuel, dem) in s.fuel.iter_mut().zip(&s.dem) {
        if *dem < SEA_LEVEL_M {
            *fuel = water;
        }
    }
}
pub fn underwater(s: &Scenario, p: Pos) -> bool {
    s.cell_of(p).is_some_and(|c| s.elevation_at(c) < SEA_LEVEL_M)
}
pub fn distance(a: Pos, b: Pos) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
pub fn inside(s: &Scenario, p: Pos) -> bool {
    p.iter().all(|v| v.is_finite())
        && p[0] >= 0.
        && p[1] >= 0.
        && p[0] < s.grid.width_m()
        && p[1] < s.grid.height_m()
}
pub fn validate(s: &Scenario) -> Result<()> {
    ensure!(
        pack::valid_id(&s.id),
        "Identificativo: usa a-z, 0-9, _ e - (max 64)"
    );
    ensure!(!s.name.trim().is_empty(), "Nome obbligatorio");
    ensure!(
        s.grid.rows >= 2
            && s.grid.cols >= 2
            && s.grid
                .rows
                .checked_mul(s.grid.cols)
                .is_some_and(|n| n <= 1_000_000),
        "Griglia: da 2×2 a un milione di celle"
    );
    ensure!(
        (1..=1440).contains(&s.turn_minutes),
        "Durata turno: 1–1440 minuti"
    );
    ensure!(
        (0..=30 * 86400).contains(&s.start_clock_s),
        "Ora iniziale fuori intervallo"
    );
    ensure!(
        (0. ..=360.).contains(&s.weather.wind_dir_deg)
            && (0. ..=200.).contains(&s.weather.wind_speed_kmh)
            && (1. ..=60.).contains(&s.weather.moisture_pct),
        "Meteo fuori intervallo"
    );
    s.check()?;
    for b in &s.buildings {
        ensure!(
            inside(s, b.pos) && b.size.iter().all(|v| v.is_finite() && *v > 0.),
            "Edificio {} non valido",
            b.id
        );
        ensure!(b.rotation_deg.is_finite(), "Rotazione edificio {} non valida", b.id);
    }
    for r in &s.roads {
        ensure!(
            r.points.iter().all(|p| p.iter().all(|v| v.is_finite())
                && p[0] >= 0.
                && p[1] >= 0.
                && p[0] <= s.grid.width_m()
                && p[1] <= s.grid.height_m()),
            "Strada fuori mappa: {}",
            r.id
        );
    }
    for p in &s.places {
        ensure!(
            !p.name.trim().is_empty() && inside(s, p.pos),
            "Luogo non valido"
        );
        ensure!(
            p.radius_m.map_or(true, |r| r.is_finite() && r > 0.),
            "Luogo {}: area non valida",
            p.name
        );
        ensure!(
            p.buildings.iter().all(|id| s.buildings.iter().any(|b| b.id == *id))
                && p.roads.iter().all(|id| s.roads.iter().any(|r| r.id == *id)),
            "Luogo {}: edificio o strada inesistente",
            p.name
        );
    }
    ensure!(
        s.dem.iter().all(|v| v.abs() <= 10000.),
        "Quota fuori intervallo ±10000 m"
    );
    Ok(())
}
/// "grassland - medium susceptibility" in the EU12 table.
const EU12_GRASSLAND_MEDIUM: i32 = 2;

/// The EU12 fuel table, as the Scenario Factory imports it into the shipped
/// territories (`data/fuels_eu12.json` is the `fuel_definitions` of Selvariva).
fn eu12_fuels() -> Vec<FuelDefinition> {
    serde_json::from_str(include_str!("../data/fuels_eu12.json")).expect("the EU12 fuel table parses")
}

/// Empty, flat territory with no inherited scenario content.
pub fn blank(id: &str, cells: usize, cellsize: f32) -> Result<Scenario> {
    ensure!(
        (8..=256).contains(&cells) && cellsize.is_finite() && (5. ..=100.).contains(&cellsize),
        "Dimensioni non valide"
    );
    let mut s = synthetic::generate(7);
    s.id = id.into();
    s.name = "Nuovo scenario".into();
    s.description = String::new();
    s.grid = GridSpec {
        rows: cells,
        cols: cells,
        cellsize,
    };
    s.dem = vec![0.; s.grid.len()];
    s.fuel = vec![fuel::GRASSLAND; s.grid.len()];
    s.render_terrain = None;
    s.buildings.clear();
    s.roads.clear();
    s.places.clear();
    s.msel.clear();
    s.timeline.clear();
    s.roles.clear();
    s.resource_types.clear();
    s.excon_roles.clear();
    s.reinforcements.clear();
    s.operational_grants.clear();
    s.communication_sites.clear();
    // The twelve EU12 classes (grass, broadleaves, shrubs, conifers at three
    // susceptibilities) plus the non-vegetated ones, not the legacy seven.
    s.fuel_definitions = eu12_fuels();
    s.fuel = vec![EU12_GRASSLAND_MEDIUM; s.grid.len()];
    s.staging_base = None;
    let size = s.grid.width_m();
    s.sectors = vec![Sector {
        id: "territory".into(),
        name: "Territorio".into(),
        bounds: [0., 0., size, size],
    }];
    s.zones = ZoneGrid {
        cols: 8,
        rows: 8,
        size_m: size / 8.,
    };
    validate(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_scenario_has_the_twelve_eu12_classes() {
        let s = blank("eu12", 8, 20.).unwrap();
        let burnable = s.fuel_definitions.iter().filter(|f| f.burn).count();
        assert_eq!(burnable, 12);
        assert!(s.is_burnable(Cell { row: 0, col: 0 }));
        assert!(s.fuel.iter().all(|f| s.fuel_definitions.iter().any(|d| d.id == *f)));
    }

    #[test]
    fn author_roundtrip_history_and_atomic_errors() {
        let mut d = Document::new(blank("test", 32, 20.).unwrap()).unwrap();
        let original = d.export().unwrap();
        let b = d.add("building", [100., 100.], None).unwrap();
        d.brush([100., 100.], 40., Some(123.), Some(5)).unwrap();
        let painted = d.export().unwrap();
        assert!(d.brush([f32::NAN, 0.], 40., None, Some(5)).is_err());
        assert_eq!(d.export().unwrap(), painted);
        d.undo();
        d.undo();
        assert_eq!(d.export().unwrap(), original);
        assert!(!d.dirty());
        d.redo();
        d.redo();
        assert_eq!(d.export().unwrap(), painted);
        d.delete(b).unwrap();
        d.undo();
        let mut reopened = Document::new(blank("other", 8, 20.).unwrap()).unwrap();
        reopened.import(&painted).unwrap();
        assert_eq!(reopened.export().unwrap(), painted);
        d.clone_as("copy").unwrap();
        assert_eq!(d.scene.id, "copy");
        assert_eq!(reopened.scene.id, "test");
    }
    #[test]
    fn invalid_import_preserves_document_and_redo() {
        let mut d = Document::new(blank("test", 8, 20.).unwrap()).unwrap();
        d.add("road", [10., 10.], Some([100., 100.])).unwrap();
        d.undo();
        assert!(d.import("{}").is_err());
        assert!(d.can_redo());
        d.redo();
        assert_eq!(d.scene.roads.len(), 1);
        assert!(d.clone_as("../escape").is_err());
    }
}
