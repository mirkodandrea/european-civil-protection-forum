//! Import Scenario Factory / propagator_abm packages without losing EU12 fuel ids.
use crate::*;
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
fn json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
    )?)
}
fn pos(v: &Value) -> Result<Pos> {
    Ok(serde_json::from_value(v.clone())?)
}
pub fn import(dir: &Path, fuel_table: &Path) -> Result<Scenario> {
    let meta = json(&dir.join("scenario.json"))?;
    let vectors = json(&dir.join("osm.json"))?;
    let g = &vectors["fire_grid"];
    let rows = g["rows"].as_u64().context("rows")? as usize;
    let cols = g["cols"].as_u64().context("cols")? as usize;
    let cellsize = g["cellsize"].as_f64().context("cellsize")? as f32;
    ensure!(
        rows > 0 && cols > 0 && cellsize.is_finite() && cellsize > 0.0,
        "invalid Factory grid"
    );
    let dem_bytes = std::fs::read(dir.join("dem.f64"))?;
    let fuel_bytes = std::fs::read(dir.join("fuel.i32"))?;
    let n = rows.checked_mul(cols).context("grid overflow")?;
    ensure!(
        dem_bytes.len() == n * 8 && fuel_bytes.len() == n * 4,
        "Factory raster length mismatch"
    );
    let dem = dem_bytes
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()) as f32)
        .collect();
    let fuel = fuel_bytes
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut scn = synthetic::generate(synthetic::DEFAULT_SEED);
    scn.id = meta["id"].as_str().context("scenario id")?.into();
    scn.name = meta["name"].as_str().unwrap_or(&scn.id).into();
    scn.description=format!("Imported Factory package: {}. Synthetic TTX roles and resources; author MSEL and initial fire for this territory.",scn.name);
    scn.grid = GridSpec {
        rows,
        cols,
        cellsize,
    };
    scn.dem = dem;
    // Source package convention: independent north-first, node-sampled visual DEM.
    if dir.join("render_terrain.json").exists() {
        let t = json(&dir.join("render_terrain.json"))?;
        let bytes = std::fs::read(dir.join("render_terrain.f32"))?;
        ensure!(bytes.len() % 4 == 0, "invalid visual terrain byte length");
        scn.render_terrain = Some(RenderTerrain {
            rows: t["rows"]
                .as_u64()
                .context("visual terrain rows")?
                .try_into()?,
            cols: t["cols"]
                .as_u64()
                .context("visual terrain cols")?
                .try_into()?,
            posting_m: t["posting_m"].as_f64().context("visual terrain posting")? as f32,
            world_size_m: serde_json::from_value(t["world_size_m"].clone())?,
            elev: bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
        });
    }
    scn.fuel = fuel;
    scn.operations = Operations::enhanced();
    scn.msel.clear();
    scn.places.clear();
    let w = scn.grid.width_m();
    let h = scn.grid.height_m();
    scn.zones = ZoneGrid {
        cols: 8,
        rows: 8,
        size_m: w.max(h) / 8.0,
    };
    scn.sectors = vec![Sector {
        id: "territory".into(),
        name: scn.name.clone(),
        bounds: [0.0, 0.0, w, h],
    }];
    scn.fuel_definitions = serde_json::from_value(json(fuel_table)?)?;
    ensure!(!scn.fuel_definitions.is_empty(), "empty fuel table");
    let population = json(&dir.join("population.json"))?;
    let mut occupants: BTreeMap<i64, u32> = BTreeMap::new();
    if let Some(homes) = population["households"].as_array() {
        for home in homes {
            let id = home["building"].as_i64().context("household building")?;
            let size = home["size"].as_u64().context("household size")?;
            *occupants.entry(id).or_default() += u32::try_from(size)?;
        }
    }
    scn.buildings.clear();
    for (i, b) in vectors["buildings"]
        .as_array()
        .context("buildings")?
        .iter()
        .enumerate()
    {
        let id = b["id"].as_i64().context("building id")?;
        let point = pos(&b["centroid"])?;
        let source = crate::canonical_visual(b["kind"].as_str().unwrap_or(""));
        let kind = match source {
            "school" | "hospital" => BuildingKind::Sensitive,
            "fuel_station" | "town_hall" | "industrial" | "assembly" | "sports_centre" => {
                BuildingKind::Facility
            }
            _ => BuildingKind::House,
        };
        let assembly = matches!(source, "assembly" | "sports_centre");
        let ring = b["ring"]
            .as_array()
            .map(|p| p.iter().map(pos).collect::<Result<Vec<_>>>())
            .transpose()?
            .unwrap_or_default();
        let size = if ring.is_empty() {
            [12.0, 12.0]
        } else {
            [0, 1].map(|axis| {
                ring.iter()
                    .map(|p| p[axis])
                    .fold(f32::NEG_INFINITY, f32::max)
                    - ring.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min)
            })
        };
        let name = b["name"].as_str().map(str::to_owned);
        if let Some(name) = &name {
            scn.places.push(Place {
                name: name.clone(),
                aliases: vec![],
                pos: point,
                ..Default::default()
            });
        }
        scn.buildings.push(Building {
            id: u32::try_from(i + 1)?,
            visual_kind: b["kind"].as_str().map(str::to_owned),
            kind,
            name,
            pos: point,
            size,
            occupants: occupants.remove(&id).unwrap_or(0),
            sector: "territory".into(),
            rotation_deg: 0.0,
            shelter_capacity: if assembly {
                500
            } else {
                0
            },
        });
    }
    ensure!(
        occupants.is_empty(),
        "households reference missing buildings"
    );
    scn.roads = vectors["roads"]
        .as_array()
        .context("roads")?
        .iter()
        .map(|r| {
            Ok(Road {
                oneway: r["oneway"].as_bool().unwrap_or(false),
                id: r["id"].to_string(),
                name: r["name"].as_str().unwrap_or("Unnamed road").into(),
                class: r["class"].as_str().unwrap_or("residential").into(),
                drivable: r["drivable"].as_bool().unwrap_or(false),
                points: serde_json::from_value(r["line"].clone())?,
            })
        })
        .collect::<Result<_>>()?;
    scn.check()?;
    Ok(scn)
}
