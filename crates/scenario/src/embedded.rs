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
        name: "Valverde — Chestnut valley",
        description: "Synthetic 4 × 4 km valley with Valverde and Pineta.",
        json: include_str!("../../../data/scenarios/valverde_factory.json"),
    },
    Territory {
        id: "selvariva_basin",
        name: "Selvariva — Due Valli basin",
        description:
            "Synthetic 6 × 6 km basin with two settlements, a festival area and three road exits.",
        json: include_str!("../../../data/scenarios/selvariva_basin.json"),
    },
    Territory {
        id: "roccavento_island",
        name: "Isola di Roccavento",
        description: "Isola sintetica di 8 × 8 km con sei borghi, un valico e il porto.",
        json: include_str!("../../../data/scenarios/roccavento_island.json"),
    },
];
pub const PACKS: &[&str] = &[
    include_str!("../../../data/scenarios/valverde_night_wind.pack.json"),
    include_str!("../../../data/scenarios/selvariva_crisi_integrata.pack.json"),
    include_str!("../../../data/scenarios/roccavento_valico.pack.json"),
];

pub fn territory(id: &str) -> Result<&'static Territory> {
    TERRITORIES
        .iter()
        .find(|t| t.id == id)
        .with_context(|| format!("territory «{id}» is not included in the app"))
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

    /// Shipped scenarios are authored in English (international audience);
    /// Italian readers get the originals through the display map.
    #[test]
    fn shipped_texts_are_english_with_italian_display() {
        let italian = [" della ", " delle ", " degli ", " nella ", " sulla ", " è ", " il ", " gli ", " che ", "’"];
        for p in packs() {
            let scn = p.build(load(&p.base).unwrap()).unwrap();
            let it = scn.display_map("it");
            let mut texts = vec![p.name.clone(), p.description.clone()];
            texts.extend(scn.translatable_texts());
            for m in &scn.msel {
                texts.extend([m.text.clone(), m.objective.clone()]);
                texts.extend(m.actions.iter().flat_map(|a| [a.title.clone(), a.note.clone()]));
            }
            for t in texts.iter().filter(|t| !t.trim().is_empty()) {
                let padded = format!(" {t} ");
                assert!(!italian.iter().any(|w| padded.contains(w)), "{}: Italian text {t:?}", p.id);
            }
            let roles = scn.roles.iter().flat_map(|r| [&r.name, &r.description]);
            let resources = scn.resource_types.iter().flat_map(|r| [&r.name, &r.description]);
            for t in roles.chain(resources) {
                assert!(it.contains_key(t), "{}: no Italian display text for {t:?}", p.id);
            }
        }
    }
}
