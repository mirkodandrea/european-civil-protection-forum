//! The landscape as the renderer draws it: rendering only, never read by the
//! fire, the routes or any other model.
//!
//! A territory imported from `propagator_abm` carries the source's finer
//! rendering data in [`Landscape`] (`scripts/factory/landscape_pack.py`): the
//! 5 m heightfield, the 5 m land cover, the island description (port, sea
//! stacks, landmarks) and the traced building footprints. [`Land`] decodes it
//! once into the shapes the source renderer reads (`Terrain`, `Cover`, roads,
//! footprints) so that renderer's modules can be ported unchanged; the model
//! keeps its own `dem`, `fuel` and building rectangles.

mod cover;
pub mod grade;
mod terrain;

use std::sync::{Arc, OnceLock};

use anyhow::{ensure, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};

pub use cover::{Cover, CoverClass};
pub use terrain::Terrain;

/// A position in the map frame, metres (+x east, +y north, origin at the
/// south-west corner): the source renderer's struct form of [`crate::Pos`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
}

impl From<crate::Pos> for Pos {
    fn from(p: crate::Pos) -> Pos {
        Pos { x: p[0], y: p[1] }
    }
}

impl From<Pos> for crate::Pos {
    fn from(p: Pos) -> crate::Pos {
        [p.x, p.y]
    }
}

pub use crate::Cell;

/// A raster travelling packed in the scenario JSON (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackedRaster {
    pub rows: usize,
    pub cols: usize,
    pub cell_m: f32,
    pub world_size_m: [f32; 2],
    /// `i16-cm-row-delta-zlib` (heights) or `u8-zlib` (classes).
    pub encoding: String,
    /// Base64 of the zlib stream.
    pub data: String,
}

impl PackedRaster {
    fn bytes(&self) -> Result<Vec<u8>> {
        use std::io::Read;
        let z = base64::engine::general_purpose::STANDARD.decode(&self.data).context("landscape raster: base64")?;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&z[..]).read_to_end(&mut out).context("landscape raster: zlib")?;
        Ok(out)
    }

    /// Heights in metres, row 0 north.
    pub fn heights(&self) -> Result<Vec<f32>> {
        ensure!(self.encoding == "i16-cm-row-delta-zlib", "landscape terrain: encoding {}", self.encoding);
        let b = self.bytes()?;
        ensure!(b.len() == self.rows * self.cols * 2, "landscape terrain: {} bytes for {}x{}", b.len(), self.rows, self.cols);
        let mut out = Vec::with_capacity(self.rows * self.cols);
        for row in b.chunks_exact(self.cols * 2) {
            let mut acc = 0i64;
            for d in row.chunks_exact(2) {
                acc += i16::from_le_bytes([d[0], d[1]]) as i64;
                out.push(acc as f32 / 100.0);
            }
        }
        Ok(out)
    }

    pub fn classes(&self) -> Result<Vec<u8>> {
        ensure!(self.encoding == "u8-zlib", "landscape cover: encoding {}", self.encoding);
        let b = self.bytes()?;
        ensure!(b.len() == self.rows * self.cols, "landscape cover: {} bytes for {}x{}", b.len(), self.rows, self.cols);
        Ok(b)
    }
}

/// A building's traced outline, matched to `Scenario::buildings` by `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub id: u32,
    /// The source's id, for provenance.
    #[serde(default)]
    pub source_id: i64,
    /// The source's building tag (`house`, `church`, `terrace`…).
    pub kind: String,
    pub ring: Vec<crate::Pos>,
    /// Storeys, where the source's population bake knows them (0: unknown).
    #[serde(default)]
    pub levels: u8,
}

/// Visual-only rendering detail of an imported territory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Landscape {
    pub terrain: PackedRaster,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<PackedRaster>,
    /// The source's `island.json` (sea level, port, stacks, landmarks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub island: Option<serde_json::Value>,
    #[serde(default)]
    pub buildings: Vec<Footprint>,
    #[serde(default)]
    pub provenance: String,
    /// Decoded once per scenario value; dropped by [`Landscape::forget`].
    #[serde(skip)]
    decoded: OnceLock<Option<Arc<Land>>>,
}

impl PartialEq for Landscape {
    fn eq(&self, o: &Self) -> bool {
        self.terrain == o.terrain && self.cover == o.cover && self.island == o.island && self.buildings == o.buildings
    }
}

impl Landscape {
    /// Forget the decoded form (the scenario's roads or buildings changed).
    pub fn forget(&mut self) {
        self.decoded = OnceLock::new();
    }

    pub fn validate(&self, s: &crate::Scenario) -> Result<()> {
        let t = &self.terrain;
        ensure!(
            t.rows >= 2 && t.cols >= 2 && t.rows.checked_mul(t.cols).is_some_and(|n| n <= 16_777_216)
                && t.cell_m.is_finite() && t.cell_m > 0.0
                && (t.world_size_m[0] - s.grid.width_m()).abs() < 0.1
                && (t.world_size_m[1] - s.grid.height_m()).abs() < 0.1
                && ((t.cols - 1) as f32 * t.cell_m - t.world_size_m[0]).abs() < 0.1
                && ((t.rows - 1) as f32 * t.cell_m - t.world_size_m[1]).abs() < 0.1,
            "landscape terrain must cover exactly the scenario extent"
        );
        if let Some(c) = &self.cover {
            ensure!(
                c.rows >= 1 && c.cols >= 1 && c.cell_m.is_finite() && c.cell_m > 0.0
                    && (c.cols as f32 * c.cell_m - s.grid.width_m()).abs() < c.cell_m
                    && (c.rows as f32 * c.cell_m - s.grid.height_m()).abs() < c.cell_m,
                "landscape cover must cover the scenario extent"
            );
        }
        ensure!(
            self.buildings.iter().all(|b| b.ring.len() >= 3 && b.ring.iter().all(|p| p.iter().all(|v| v.is_finite()))),
            "landscape footprints must be finite rings"
        );
        Ok(())
    }
}

/// A road as the renderer reads it.
#[derive(Debug, Clone)]
pub struct Road {
    pub class: String,
    pub drivable: bool,
    /// A track: graded and drawn, narrower, not drivable by the public.
    pub track: bool,
    pub line: Vec<[f32; 2]>,
}

/// A building as the renderer reads it: its traced ring when the landscape
/// has one, else the scenario's rectangle.
#[derive(Debug, Clone)]
pub struct Building {
    /// `Scenario::buildings` id.
    pub id: u32,
    pub kind: Option<String>,
    pub name: Option<String>,
    pub centroid: [f32; 2],
    pub ring: Vec<[f32; 2]>,
    pub levels: u8,
}

impl Building {
    /// Footprint area, square metres (shoelace).
    pub fn area(&self) -> f32 {
        let n = self.ring.len();
        if n < 3 {
            return 0.0;
        }
        let acc: f32 = (0..n)
            .map(|i| {
                let (a, b) = (self.ring[i], self.ring[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum();
        (acc / 2.0).abs()
    }
}

/// The source renderer's view of the model grid.
#[derive(Debug, Clone, Copy)]
pub struct World {
    pub width_m: f32,
    pub height_m: f32,
    pub cellsize: f32,
    pub fire_rows: usize,
    pub fire_cols: usize,
}

impl World {
    /// Centre of a fire cell, map frame (row 0 north).
    pub fn centre_of(&self, c: Cell) -> Pos {
        Pos { x: (c.col as f32 + 0.5) * self.cellsize, y: self.height_m - (c.row as f32 + 0.5) * self.cellsize }
    }

    /// The cell under `p`, clamped to the grid.
    pub fn cell_of(&self, p: Pos) -> Cell {
        let col = (p.x / self.cellsize).floor().clamp(0.0, (self.fire_cols - 1) as f32);
        let row = ((self.height_m - p.y) / self.cellsize).floor().clamp(0.0, (self.fire_rows - 1) as f32);
        Cell { row: row as usize, col: col as usize }
    }

    pub fn contains(&self, p: Pos) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x < self.width_m && p.y < self.height_m
    }
}

pub struct Vectors {
    pub roads: Vec<Road>,
    pub buildings: Vec<Building>,
}

/// Everything the landscape renderer reads, decoded.
pub struct Land {
    pub id: String,
    pub world: World,
    pub terrain: Terrain,
    pub cover: Option<Cover>,
    pub vectors: Vectors,
    pub island: Option<serde_json::Value>,
    /// The model's fuel codes and its elevation per cell (row-major).
    pub fuel: Vec<i32>,
    pub dem: Vec<f64>,
    burnable: Vec<bool>,
}

impl std::fmt::Debug for Land {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Land({}, {}x{} @ {} m)", self.id, self.terrain.cols, self.terrain.rows, self.terrain.posting)
    }
}

impl Land {
    pub fn fuel_at(&self, c: Cell) -> i32 {
        self.fuel[c.row * self.world.fire_cols + c.col]
    }

    pub fn is_burnable(&self, c: Cell) -> bool {
        self.burnable[c.row * self.world.fire_cols + c.col]
    }

    fn build(s: &crate::Scenario, l: &Landscape) -> Result<Land> {
        l.validate(s)?;
        let world = World {
            width_m: s.grid.width_m(),
            height_m: s.grid.height_m(),
            cellsize: s.grid.cellsize,
            fire_rows: s.grid.rows,
            fire_cols: s.grid.cols,
        };
        let t = &l.terrain;
        let mut terrain = Terrain::new(t.rows, t.cols, t.cell_m, t.world_size_m, t.heights()?);
        let cover = match &l.cover {
            Some(c) => Some(Cover::new(c.rows, c.cols, c.cell_m, c.world_size_m[1], c.classes()?)),
            None => None,
        };
        let roads: Vec<Road> = s
            .roads
            .iter()
            .map(|r| Road {
                class: r.class.clone(),
                drivable: r.drivable,
                track: !r.drivable && matches!(r.class.as_str(), "track" | "path" | "service"),
                line: r.points.clone(),
            })
            .collect();
        if grade::enabled() {
            terrain.grade_roads(&roads);
        }
        let traced: std::collections::HashMap<u32, &Footprint> = l.buildings.iter().map(|f| (f.id, f)).collect();
        let buildings = s
            .buildings
            .iter()
            .map(|b| {
                // A footprint whose building the EXCON has since moved is
                // stale: the rectangle is drawn instead.
                let ring = traced.get(&b.id).filter(|f| {
                    let c = centroid(&f.ring);
                    (c[0] - b.pos[0]).hypot(c[1] - b.pos[1]) < 0.75 * b.size[0].max(b.size[1]).max(8.0)
                });
                match ring {
                    Some(f) => Building {
                        id: b.id,
                        kind: Some(f.kind.clone()),
                        name: b.name.clone(),
                        centroid: centroid(&f.ring),
                        ring: f.ring.clone(),
                        levels: f.levels,
                    },
                    None => Building {
                        id: b.id,
                        kind: b.visual_kind.clone(),
                        name: b.name.clone(),
                        centroid: b.pos,
                        ring: rectangle(b),
                        levels: 0,
                    },
                }
            })
            .collect();
        let burnable = (0..s.grid.len()).map(|i| s.is_burnable(Cell { row: i / s.grid.cols, col: i % s.grid.cols })).collect();
        Ok(Land {
            id: s.id.clone(),
            world,
            terrain,
            cover,
            vectors: Vectors { roads, buildings },
            island: l.island.clone(),
            fuel: s.fuel.clone(),
            dem: s.dem.iter().map(|v| *v as f64).collect(),
            burnable,
        })
    }
}

fn centroid(ring: &[crate::Pos]) -> crate::Pos {
    let n = ring.len().max(1) as f32;
    let (x, y) = ring.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    [x / n, y / n]
}

fn rectangle(b: &crate::Building) -> Vec<crate::Pos> {
    let (s, c) = b.rotation_deg.to_radians().sin_cos();
    let (hx, hy) = (b.size[0] * 0.5, b.size[1] * 0.5);
    [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
        .into_iter()
        .map(|(x, y)| [b.pos[0] + x * c - y * s, b.pos[1] + x * s + y * c])
        .collect()
}

impl crate::Scenario {
    /// The decoded landscape, when the scenario carries one that decodes.
    /// Rendering only.
    pub fn land(&self) -> Option<Arc<Land>> {
        self.land_slot().clone()
    }

    /// [`Self::land`] borrowed, for the hot paths (a height per vertex).
    pub fn land_ref(&self) -> Option<&Land> {
        self.land_slot().as_deref()
    }

    fn land_slot(&self) -> &Option<Arc<Land>> {
        static NONE: Option<Arc<Land>> = None;
        let Some(l) = self.landscape.as_ref() else { return &NONE };
        l.decoded.get_or_init(|| match Land::build(self, l) {
            Ok(land) => Some(Arc::new(land)),
            Err(e) => {
                eprintln!("landscape of {}: {e:#}", self.id);
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed_heights(rows: usize, cols: usize, f: impl Fn(usize, usize) -> f32) -> PackedRaster {
        use std::io::Write;
        let mut raw = Vec::new();
        for r in 0..rows {
            let mut prev = 0i64;
            for c in 0..cols {
                let cm = (f(r, c) * 100.0).round() as i64;
                raw.extend_from_slice(&((cm - prev) as i16).to_le_bytes());
                prev = cm;
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        PackedRaster {
            rows,
            cols,
            cell_m: 4000.0 / (cols - 1) as f32,
            world_size_m: [4000.0, 4000.0],
            encoding: "i16-cm-row-delta-zlib".into(),
            data: base64::engine::general_purpose::STANDARD.encode(z.finish().unwrap()),
        }
    }

    #[test]
    fn packed_heights_round_trip_north_first_and_stay_visual_only() {
        let mut s = crate::synthetic::generate(crate::synthetic::DEFAULT_SEED);
        let (dem, fuel) = (s.dem.clone(), s.fuel.clone());
        s.landscape = Some(Landscape {
            terrain: packed_heights(41, 41, |r, c| 100.0 + r as f32 * 2.5 - c as f32 * 0.37),
            cover: None,
            island: None,
            buildings: Vec::new(),
            provenance: String::new(),
            decoded: OnceLock::new(),
        });
        let land = s.land().expect("decodes");
        // row 0 is the north edge
        assert!((land.terrain.raw_height_at(Pos { x: 0.0, y: 4000.0 }) - 100.0).abs() < 0.01);
        assert!((land.terrain.raw_height_at(Pos { x: 4000.0, y: 0.0 }) - (100.0 + 40.0 * 2.5 - 40.0 * 0.37)).abs() < 0.01);
        assert_eq!(s.dem, dem);
        assert_eq!(s.fuel, fuel);
        assert!(Arc::ptr_eq(&land, &s.land().unwrap()), "decoded once");
        // round trip through JSON keeps the packed data, not the decoded form
        let back: crate::Scenario = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.landscape, s.landscape);
        assert!(back.land().is_some());
    }

    #[test]
    fn a_landscape_of_another_extent_is_refused() {
        let mut s = crate::synthetic::generate(crate::synthetic::DEFAULT_SEED);
        let mut t = packed_heights(11, 11, |_, _| 5.0);
        t.world_size_m = [3000.0, 3000.0];
        s.landscape = Some(Landscape { terrain: t, cover: None, island: None, buildings: Vec::new(), provenance: String::new(), decoded: OnceLock::new() });
        assert!(s.land().is_none());
        assert!(s.check().is_err());
    }
}
