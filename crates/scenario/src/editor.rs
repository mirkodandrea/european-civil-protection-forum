//! Headless authoring document. Draft edits never touch a running World.
use crate::*;
use anyhow::{ensure, Result};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Building(usize),
    Road(usize),
    Place(usize),
    Ignition(usize),
}

pub struct Document {
    pub scene: Arc<Scenario>,
    pub revision: u64,
    saved: Arc<Scenario>,
    undo: Vec<Arc<Scenario>>,
    redo: Vec<Arc<Scenario>>,
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
        validate(&next)?;
        self.undo.push(self.scene.clone());
        if self.undo.len() > 32 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.scene = Arc::new(next);
        self.revision += 1;
        Ok(())
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
                for row in 0..t.rows {
                    for col in 0..t.cols {
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
    pub fn add(&mut self, kind: &str, at: Pos, end: Option<Pos>) -> Result<Selection> {
        let mut selected = Selection::Place(0);
        self.edit(|s| {
            ensure!(inside(s, at), "Punto fuori mappa");
            selected = match kind {
                "building" => {
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
                        kind: BuildingKind::House,
                        visual_kind: None,
                        name: Some(format!("Edificio {id}")),
                        pos: at,
                        size: [20., 20.],
                        occupants: 0,
                        sector,
                        shelter_capacity: 0,
                    });
                    Selection::Building(s.buildings.len() - 1)
                }
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
                "ignition" => {
                    s.initial_ignitions.push(IgnitionSpec { at, radius_m: 40. });
                    Selection::Ignition(s.initial_ignitions.len() - 1)
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
                    });
                    Selection::Place(s.places.len() - 1)
                }
            };
            Ok(())
        })?;
        Ok(selected)
    }
    pub fn delete(&mut self, selection: Selection) -> Result<()> {
        self.edit(|s| {
            match selection {
                Selection::Building(i) if i < s.buildings.len() => {
                    s.buildings.remove(i);
                }
                Selection::Road(i) if i < s.roads.len() => {
                    s.roads.remove(i);
                }
                Selection::Place(i) if i < s.places.len() => {
                    let p = s.places.remove(i);
                    if s.staging_base.as_ref() == Some(&p.name) {
                        s.staging_base = None;
                    }
                }
                Selection::Ignition(i) if i < s.initial_ignitions.len() => {
                    s.initial_ignitions.remove(i);
                }
                _ => anyhow::bail!("Selezione non valida"),
            };
            Ok(())
        })
    }
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
    }
    for i in &s.initial_ignitions {
        ensure!(
            inside(s, i.at) && i.radius_m.is_finite() && i.radius_m >= 40.,
            "Innesco: raggio minimo 40 m e centro sulla mappa"
        );
    }
    ensure!(
        s.dem.iter().all(|v| v.abs() <= 10000.),
        "Quota fuori intervallo ±10000 m"
    );
    Ok(())
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
    s.initial_ignitions.clear();
    s.msel.clear();
    s.timeline.clear();
    s.roles.clear();
    s.resource_types.clear();
    s.excon_roles.clear();
    s.reinforcements.clear();
    s.operational_grants.clear();
    s.communication_sites.clear();
    s.fuel_definitions.clear();
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
