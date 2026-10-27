//! Territories and exercise packs shipped in the standalone application.
//! One registry owns both the selectable maps and the loader used at startup.
use crate::{Scenario, ScenarioPack};
use anyhow::{ensure, Context, Result};

pub struct Territory {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub json: &'static str,
}

pub const DEFAULT_TERRITORY: &str = "valverde_factory";
pub const TERRITORIES: &[Territory] = &[
    Territory {
        id: "valverde_factory",
        name: "Valverde — valle dei Castagni",
        description: "Valle sintetica di 4 × 4 km con Valverde e Pineta.",
        json: include_str!("../../../data/scenarios/valverde_factory.json"),
    },
    Territory {
        id: "selvariva_basin",
        name: "Selvariva — conca delle Due Valli",
        description:
            "Conca sintetica di 6 × 6 km con due abitati, area festival e tre uscite stradali.",
        json: include_str!("../../../data/scenarios/selvariva_basin.json"),
    },
];
pub const PACKS: &[&str] = &[
    include_str!("../../../data/scenarios/valverde_night_wind.pack.json"),
    include_str!("../../../data/scenarios/selvariva_festival.pack.json"),
];

pub fn territory(id: &str) -> Result<&'static Territory> {
    TERRITORIES
        .iter()
        .find(|t| t.id == id)
        .with_context(|| format!("territorio «{id}» non incluso nell’app"))
}

pub fn load(id: &str) -> Result<Scenario> {
    let mut scn: Scenario = serde_json::from_str(territory(id)?.json)?;
    ensure!(
        scn.id == id,
        "embedded territory id does not match its registry key"
    );
    scn.migrate_legacy_timeline()?;
    scn.check()?;
    Ok(scn)
}

pub fn packs() -> Vec<ScenarioPack> {
    PACKS
        .iter()
        .map(|s| serde_json::from_str(s).expect("invalid bundled pack"))
        .collect()
}

/// Content identity for saved browser sessions (not a security hash).
pub fn fingerprint(id: &str) -> Result<String> {
    let hash = territory(id)?
        .json
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    Ok(format!("{id}:{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_pack_uses_its_own_territory() {
        let old = load(DEFAULT_TERRITORY).unwrap();
        let new = load("selvariva_basin").unwrap();
        assert_ne!(old.grid.len(), new.grid.len());
        assert_ne!(
            fingerprint(DEFAULT_TERRITORY).unwrap(),
            fingerprint(&new.id).unwrap()
        );
        for p in packs() {
            let b = load(&p.base).unwrap();
            let id = b.id.clone();
            assert_eq!(p.build(b).unwrap().id, id);
        }
        assert!(load("missing_map").is_err());
    }
}
