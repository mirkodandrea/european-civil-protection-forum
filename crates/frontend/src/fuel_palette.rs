//! Base-map colours of the 2D situation map, driven by the scenario's fuel
//! definitions (`/api/static` `fuel_names`, keyed by raster byte) rather than
//! by fuel ids: EU12 ids (1–12, grouped by family) and the legacy 7-class ids
//! overlap, so only the class name says what a code is.
//!
//! One coherent palette by vegetation family, light enough for fire, works and
//! labels drawn on top: grassland light yellow-greens, broadleaves mid greens,
//! shrubs olive/brown-greens, conifers dark blue-greens. Within a family a
//! higher susceptibility is more saturated and darker. Sea and lakes come from
//! the static `water` bitset (ground below sea level) and are drawn as calm
//! water with a darker coastline, distinct from the bright hatched blue of
//! water/retardant drops.
use base64::Engine;
use serde_json::Value;

/// Vegetation family of a fuel class, as the base map groups them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Grassland,
    Broadleaves,
    Shrubs,
    Conifers,
    AgroForestry,
    LowRiskForest,
    NonVegetated,
    Water,
    Unknown,
}

/// Neutral for a code the scenario does not name.
pub const UNKNOWN: [u8; 3] = [200, 200, 190];
/// Open water (sea, lakes).
pub const SEA: [u8; 3] = [168, 201, 222];
/// Water when the base layer is off: a faint tint on the plain background.
pub const PLAIN_WATER: [u8; 3] = [224, 235, 242];
/// The first water cell off the shore: a thin, darker coastline.
pub const COAST: [u8; 3] = [104, 146, 176];

/// Family and susceptibility level (0 low, 1 medium/unspecified, 2 high) of a
/// class name such as `"conifers - high susceptibility"` or `"shrubs"`.
pub fn classify(name: &str) -> (Family, usize) {
    let n = name.to_ascii_lowercase();
    let family = if n.contains("water") || n.contains("sea") || n.contains("lake") || n.contains("acqua") {
        Family::Water
    } else if n.contains("non-fire-prone") || n.contains("non fire prone") {
        Family::LowRiskForest
    } else if n.contains("not vegetated") || n.contains("non-vegetated") || n.contains("non vegetated") {
        Family::NonVegetated
    } else if n.contains("conifer") {
        Family::Conifers
    } else if n.contains("broadlea") {
        Family::Broadleaves
    } else if n.contains("shrub") {
        Family::Shrubs
    } else if n.contains("agro") {
        Family::AgroForestry
    } else if n.contains("grass") {
        Family::Grassland
    } else {
        Family::Unknown
    };
    let level = if n.contains("low") {
        0
    } else if n.contains("high") {
        2
    } else {
        1
    };
    (family, level)
}

/// Swatch colour of a family at a susceptibility level.
pub fn rgb(family: Family, level: usize) -> [u8; 3] {
    let l = level.min(2);
    match family {
        Family::Grassland => [[226, 229, 176], [210, 218, 140], [193, 204, 104]][l],
        Family::Broadleaves => [[160, 196, 128], [128, 176, 98], [100, 154, 74]][l],
        Family::Shrubs => [[184, 182, 132], [164, 158, 104], [143, 134, 80]][l],
        Family::Conifers => [[118, 162, 142], [86, 138, 120], [60, 114, 98]][l],
        Family::AgroForestry => [204, 196, 146],
        Family::LowRiskForest => [150, 180, 160],
        Family::NonVegetated => [218, 214, 205],
        Family::Water => SEA,
        Family::Unknown => UNKNOWN,
    }
}

/// Colour of every raster byte for one territory: a lookup table built once
/// per redraw, so the per-cell loop is a plain index.
pub struct Palette {
    lut: [[u8; 3]; 256],
}

impl Palette {
    /// From the static payload's `fuel_names` (`{"<byte>": "<class name>"}`).
    pub fn new(stat: &Value) -> Self {
        let mut lut = [UNKNOWN; 256];
        if let Some(names) = stat["fuel_names"].as_object() {
            for (code, name) in names {
                if let (Ok(code), Some(name)) = (code.parse::<u8>(), name.as_str()) {
                    let (family, level) = classify(name);
                    lut[code as usize] = rgb(family, level);
                }
            }
        }
        Self { lut }
    }
    pub fn fuel(&self, code: u8) -> [u8; 3] {
        self.lut[code as usize]
    }
}

/// Decode the static `water` bitset (bit `i % 8` of byte `i / 8`) into one
/// flag per cell; empty when the territory has no water.
pub fn water_mask(stat: &Value, cells: usize) -> Vec<bool> {
    let Some(b64) = stat["water"].as_str() else {
        return Vec::new();
    };
    let bits = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_default();
    if bits.len() * 8 < cells {
        return Vec::new();
    }
    (0..cells).map(|i| bits[i / 8] >> (i % 8) & 1 == 1).collect()
}

/// Whether water cell `i` touches land (4-neighbourhood): the coastline.
pub fn is_coast(water: &[bool], cols: usize, rows: usize, i: usize) -> bool {
    let (r, c) = (i / cols, i % cols);
    (r > 0 && !water[i - cols])
        || (r + 1 < rows && !water[i + cols])
        || (c > 0 && !water[i - 1])
        || (c + 1 < cols && !water[i + 1])
}

/// Base colour of cell `i`: water (with coastline) over the fuel class.
pub fn base(palette: &Palette, fuel: &[u8], water: &[bool], cols: usize, rows: usize, i: usize) -> [u8; 3] {
    if water.get(i).copied().unwrap_or(false) {
        if is_coast(water, cols, rows, i) {
            COAST
        } else {
            SEA
        }
    } else {
        palette.fuel(fuel.get(i).copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eu12() -> Value {
        // As `sim::digest::static_view` keys them: the raster byte (-1 → 255).
        let mut names = serde_json::Map::new();
        names.insert("255".into(), json!("not vegetated"));
        names.insert("0".into(), json!("not vegetated"));
        for (i, fam) in ["grassland", "broadleaves", "shrubs", "conifers"].iter().enumerate() {
            for (j, lvl) in ["low", "medium", "high"].iter().enumerate() {
                names.insert((1 + 3 * i + j).to_string(), json!(format!("{fam} - {lvl} susceptibility")));
            }
        }
        json!({ "fuel_names": names })
    }
    fn legacy() -> Value {
        json!({"fuel_names": {"1":"broadleaves","2":"shrubs","3":"non-vegetated","4":"grassland",
            "5":"conifers","6":"agro-forestry","7":"non-fire-prone forest"}})
    }
    fn lum(c: [u8; 3]) -> u32 {
        c.iter().map(|&v| v as u32).sum()
    }

    #[test]
    fn eu12_codes_follow_family_and_susceptibility() {
        let p = Palette::new(&eu12());
        assert_eq!(p.fuel(1), rgb(Family::Grassland, 0));
        assert_eq!(p.fuel(6), rgb(Family::Broadleaves, 2));
        assert_eq!(p.fuel(8), rgb(Family::Shrubs, 1));
        assert_eq!(p.fuel(12), rgb(Family::Conifers, 2));
        assert_eq!(p.fuel(0), rgb(Family::NonVegetated, 1));
        assert_eq!(p.fuel(255), rgb(Family::NonVegetated, 1), "id -1 travels as byte 255");
        for base in [1u8, 4, 7, 10] {
            // Higher susceptibility reads darker within a family.
            assert!(lum(p.fuel(base)) > lum(p.fuel(base + 1)) && lum(p.fuel(base + 1)) > lum(p.fuel(base + 2)));
        }
        // Every EU12 class has its own colour.
        let mut all: Vec<[u8; 3]> = (1..=12).map(|c| p.fuel(c)).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 12);
        // Grass is the lightest family, conifers the darkest.
        assert!(lum(p.fuel(2)) > lum(p.fuel(5)) && lum(p.fuel(5)) > lum(p.fuel(11)));
    }

    #[test]
    fn legacy_codes_keep_their_own_meaning() {
        let p = Palette::new(&legacy());
        assert_eq!(p.fuel(1), rgb(Family::Broadleaves, 1));
        assert_eq!(p.fuel(3), rgb(Family::NonVegetated, 1));
        assert_eq!(p.fuel(4), rgb(Family::Grassland, 1));
        assert_eq!(p.fuel(5), rgb(Family::Conifers, 1));
        assert_eq!(p.fuel(6), rgb(Family::AgroForestry, 1));
        assert_eq!(p.fuel(7), rgb(Family::LowRiskForest, 1));
        // Same byte, different classification, different colour.
        assert_ne!(Palette::new(&eu12()).fuel(1), p.fuel(1));
    }

    #[test]
    fn unknown_codes_and_names_are_neutral() {
        let p = Palette::new(&legacy());
        assert_eq!(p.fuel(9), UNKNOWN);
        assert_eq!(Palette::new(&json!({})).fuel(1), UNKNOWN);
        assert_eq!(classify("lava field").0, Family::Unknown);
        assert_eq!(classify("open water").0, Family::Water);
    }

    #[test]
    fn sea_with_coastline_overrides_fuel() {
        use base64::Engine;
        // 4×3 raster: the left column is sea.
        let (cols, rows) = (4, 3);
        let wet = [0usize, 4, 8];
        let mut bits = vec![0u8; 2];
        for i in wet {
            bits[i / 8] |= 1 << (i % 8);
        }
        let stat = json!({"water": base64::engine::general_purpose::STANDARD.encode(&bits)});
        let water = water_mask(&stat, cols * rows);
        assert_eq!(water.iter().filter(|w| **w).count(), 3);
        let p = Palette::new(&eu12());
        let fuel = vec![0u8; cols * rows];
        assert_eq!(base(&p, &fuel, &water, cols, rows, 4), COAST, "touches land");
        assert_eq!(base(&p, &fuel, &water, cols, rows, 5), rgb(Family::NonVegetated, 1));
        // A fully enclosed water cell is open sea.
        let open = vec![true; 9];
        assert_eq!(base(&p, &[0; 9], &open, 3, 3, 4), SEA);
        assert!(water_mask(&json!({"water": null}), 12).is_empty());
        // Sea stays apart from the retardant overlay blue (60,150,255).
        assert!(SEA[2] < 240 && SEA[0] > 120);
    }
}
