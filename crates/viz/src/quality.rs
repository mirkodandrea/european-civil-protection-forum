//! Graphics level: what the renderer may spend on a machine weaker than the one
//! the scene was tuned on (levels as in `propagator_abm`'s `quality.rs`).
//! Presentation only: it never alters the simulation or the operational overlays.
//!
//! The level is picked from the HUD (or `Q` cycles it), applied live by
//! `quality_live` and remembered for the next launch. `TTX_QUALITY=low|projector|high`
//! wins over the remembered level; `TTX_GFX` then overrides single knobs, for
//! tuning on the projector machine: `TTX_GFX=-bloom,msaa=1,cascades=2,shadow=1500,veg=0.3`.
//!
//! Most knobs apply at once. The terrain mesh stride and the share of plants
//! planted are baked into the scene, so changing them rebuilds it (`Rebuild`).
use bevy::prelude::*;

/// The levels the selector offers, lowest first.
pub const LEVELS: [&str; 3] = ["low", "projector", "high"];

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub name: &'static str,
    pub shadows: bool,
    pub bloom: f32,
    /// Share of the authored plant density that is planted (rebuilds the scene).
    pub vegetation: f32,
    /// Plant chunks further than this (m) are hidden; the overview widens it.
    pub vegetation_distance: f32,
    pub fire_lights: usize,
    /// Shadow cascades, their reach (m) and the shadow map side in texels.
    pub cascades: usize,
    pub shadow_distance: f32,
    pub shadow_map: usize,
    /// 4 (MSAA 4x) or 1. SMAA stands in for MSAA on the low level.
    pub msaa: u32,
    pub smaa: bool,
    /// Terrain samples per render-mesh vertex (rebuilds the scene).
    pub terrain_stride: usize,
}

/// Set when a change needs the scene rebuilt (terrain stride, plant density).
#[derive(Resource, Default)]
pub struct Rebuild(pub bool);

impl Quality {
    pub const LOW: Quality = Quality {
        name: "low",
        shadows: false,
        bloom: 0.0,
        vegetation: 0.35,
        vegetation_distance: 2500.0,
        fire_lights: 0,
        cascades: 2,
        shadow_distance: 1800.0,
        shadow_map: 1024,
        msaa: 1,
        smaa: true,
        terrain_stride: 4,
    };
    pub const PROJECTOR: Quality = Quality {
        name: "projector",
        shadows: true,
        bloom: 0.06,
        vegetation: 0.65,
        vegetation_distance: 4500.0,
        fire_lights: 2,
        cascades: 4,
        shadow_distance: 6000.0,
        shadow_map: 2048,
        msaa: 4,
        smaa: false,
        terrain_stride: 2,
    };
    pub const HIGH: Quality = Quality {
        name: "high",
        shadows: true,
        bloom: 0.18,
        vegetation: 1.0,
        vegetation_distance: 9000.0,
        fire_lights: 8,
        shadow_map: 4096,
        terrain_stride: 1,
        ..Quality::PROJECTOR
    };
    /// The browser: no shadow pass, no multisampling, a coarse mesh. The plants
    /// are the same as `LOW`'s: at 0.15 the forests read as scattered trees.
    pub const WEB: Quality = Quality {
        smaa: false,
        ..Quality::LOW
    };

    pub fn preset(name: &str) -> Quality {
        match name {
            "low" if cfg!(target_arch = "wasm32") => Quality::WEB,
            "low" => Quality::LOW,
            "high" => Quality::HIGH,
            _ => Quality::PROJECTOR,
        }
    }

    pub fn from_env() -> Self {
        let name = std::env::var("TTX_QUALITY").ok().or_else(saved);
        let mut q = match name.as_deref() {
            _ if cfg!(target_arch = "wasm32") => Quality::WEB,
            Some(n) if LEVELS.contains(&n) => Quality::preset(n),
            _ => Quality::PROJECTOR,
        };
        if let Ok(spec) = std::env::var("TTX_GFX") {
            q.apply(&spec);
        }
        q
    }

    /// `-bloom`, `+shadows`, `msaa=1`, `cascades=2`, `shadow=1500`, `veg=0.3`,
    /// `stride=2`, `lights=4`, separated by commas.
    pub fn apply(&mut self, spec: &str) {
        for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (on, key) = match item.strip_prefix('-') {
                Some(k) => (false, k),
                None => (true, item.strip_prefix('+').unwrap_or(item)),
            };
            let (key, value) = key.split_once('=').unwrap_or((key, ""));
            let num = value.parse::<f32>().ok();
            match (key, num) {
                ("bloom", None) => self.bloom = if on { Quality::PROJECTOR.bloom } else { 0.0 },
                ("bloom", Some(v)) => self.bloom = v.max(0.0),
                ("shadows", _) => self.shadows = on,
                ("smaa", _) => self.smaa = on,
                ("msaa", Some(n)) => self.msaa = if n >= 4.0 { 4 } else { 1 },
                ("msaa", None) => self.msaa = if on { 4 } else { 1 },
                ("cascades", Some(n)) => self.cascades = (n as usize).clamp(1, 4),
                ("shadow", Some(m)) => self.shadow_distance = m.max(100.0),
                ("shadow_map", Some(n)) => self.shadow_map = (n as usize).clamp(256, 4096),
                ("veg", Some(v)) => self.vegetation = v.max(0.0),
                ("stride", Some(n)) => self.terrain_stride = (n as usize).clamp(1, 8),
                ("lights", Some(n)) => self.fire_lights = (n as usize).min(8),
                _ => warn!("TTX_GFX: «{item}» not recognised"),
            }
        }
    }

    /// The level after this one, wrapping round (what `Q` does).
    pub fn next_level(&self) -> &'static str {
        let i = LEVELS.iter().position(|l| *l == self.name).unwrap_or(1);
        LEVELS[(i + 1) % LEVELS.len()]
    }

    /// Whether `other` needs the scene rebuilt to show: those knobs are baked in.
    pub fn needs_rebuild(&self, other: &Quality) -> bool {
        self.terrain_stride != other.terrain_stride || self.vegetation != other.vegetation
    }

    /// MSAA is a global resource in Bevy 0.14; WebGL2 has no multisampled HDR targets.
    pub fn msaa_mode(&self) -> Msaa {
        if cfg!(target_arch = "wasm32") || self.msaa < 4 {
            Msaa::Off
        } else {
            Msaa::Sample4
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saved_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::Path::new(&home).join(".ttx").join("graphics.txt")
}

#[cfg(not(target_arch = "wasm32"))]
fn saved() -> Option<String> {
    // A snapshot or a bench is a fixture: never at the mercy of a saved choice.
    if std::env::var_os("TTX_SNAPSHOT").is_some() {
        return None;
    }
    std::fs::read_to_string(saved_path()).ok().map(|s| s.trim().to_string())
}

/// Remember the level for the next launch. A failure is reported, never fatal.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(level: &str) {
    let p = saved_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&p, level) {
        warn!("graphics level not saved: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
fn saved() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn save(_level: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_climb_in_cost_and_wrap() {
        let [lo, mid, hi] = LEVELS.map(Quality::preset);
        assert!(lo.vegetation < mid.vegetation && mid.vegetation < hi.vegetation);
        assert!(lo.terrain_stride > mid.terrain_stride && mid.terrain_stride > hi.terrain_stride);
        assert!(!lo.shadows && mid.shadows && hi.shadows);
        assert_eq!(hi.next_level(), "low");
        assert_eq!(lo.next_level(), "projector");
        assert!(lo.needs_rebuild(&mid));
        assert!(!mid.needs_rebuild(&Quality::PROJECTOR));
    }

    #[test]
    fn overrides_parse() {
        let mut q = Quality::HIGH;
        q.apply("-bloom,msaa=1,cascades=9,shadow=1500,veg=0.3,stride=2,lights=3");
        assert_eq!((q.bloom, q.msaa, q.cascades, q.terrain_stride, q.fire_lights), (0.0, 1, 4, 2, 3));
        assert_eq!((q.shadow_distance, q.vegetation), (1500.0, 0.3));
    }
}
