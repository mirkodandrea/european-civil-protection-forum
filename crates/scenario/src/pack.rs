//! Scenario packs: the editable layer over a base scenario.
//!
//! A base scenario carries the territory (rasters, buildings, roads, sectors),
//! tens of MB. Everything an EXCON designs on top of it, such as roles,
//! resources, ignitions, weather and the MSEL, is small and lives in a
//! [`ScenarioPack`] that names its base. Several packs can share one base, and
//! a pack is a plain JSON file that can be copied, edited and versioned.
use crate::{
    District, DistrictEvacuation, InjectTemplate, TimelineAction, OperationalGrant, Operations, Place, ReinforcementOffer,
    ResourceType, Role, Scenario, TacticalPolicy, WeatherSpec,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Shortest ignition radius that does not fizzle (single-cell ignitions die out).

/// Overrides of a base scenario. A field that is `None` keeps the base value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioEdits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_clock_s: Option<i64>,
    /// Turn length in minutes (1..=1440); absent keeps the base (30 by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weather: Option<WeatherSpec>,
    /// Packs saved before ignitions became timeline actions carry this key;
    /// it is read and dropped so they still load.
    #[serde(default, rename = "initial_ignitions", skip_serializing)]
    pub legacy_ignitions: Option<serde::de::IgnoredAny>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub places: Option<Vec<Place>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_types: Option<Vec<ResourceType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<Role>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excon_roles: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reinforcements: Option<Vec<ReinforcementOffer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msel: Option<Vec<InjectTemplate>>,
    /// Legacy standalone actions: read for old packs, converted into inject
    /// actions when applied and never written back.
    #[serde(skip_serializing)]
    pub timeline: Option<Vec<TimelineAction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tactical_policy: Option<TacticalPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Operations>,
    /// Place deployments start from; empty means the centre of the map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staging_base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operational_grants: Option<Vec<OperationalGrant>>,
    /// Borghi of the territory, for `EvacuateDistrict`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub districts: Option<Vec<District>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub district_evacuation: Option<DistrictEvacuation>,
    /// Display texts in other languages (see [`Scenario::translations`]):
    /// the complete table, replacing the territory's, so a pack that
    /// translates its own texts also carries the territory's map labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translations: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

/// Simulation options a pack may fix. Command-line options take precedence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Monte Carlo ensemble size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realizations: Option<usize>,
    /// Fraction of runs that must reach a cell for it to count as burning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutoff: Option<f32>,
    /// Starts with the operational model on (road routes, progressive
    /// suppression, traffic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operational_model: Option<bool>,
}

/// A named scenario: a base territory plus the designer's edits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioPack {
    /// File-safe identifier (`a-z0-9_-`).
    pub id: String,
    pub name: String,
    pub description: String,
    /// Id of the base scenario (its file stem in the scenarios directory).
    pub base: String,
    pub edits: ScenarioEdits,
    pub run: RunOptions,
}

/// Facts about the running build that the scenario cannot know itself.
pub struct Vocabulary<'a> {
    /// Command kinds a role may be allowed to propose.
    pub role_actions: &'a [&'a str],
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl ScenarioPack {
    /// A pack with no edits over `base`.
    pub fn plain(id: &str, name: &str, base: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            base: base.into(),
            ..Self::default()
        }
    }

    /// The scenario the pack describes: `base` with the edits applied,
    /// checked for consistency.
    pub fn build(&self, mut base: Scenario) -> Result<Scenario> {
        base.apply_edits(&self.edits)?;
        if self.run.operational_model == Some(true) {
            base.enable_operational_model();
        }
        Ok(base)
    }
}

impl Scenario {
    /// Replace the edited parts. The result is checked as a whole.
    pub fn apply_edits(&mut self, e: &ScenarioEdits) -> Result<()> {
        if let Some(v) = &e.name {
            self.name = v.clone();
        }
        if let Some(v) = &e.description {
            self.description = v.clone();
        }
        if let Some(v) = e.start_clock_s {
            self.start_clock_s = v;
        }
        if let Some(v) = e.turn_minutes {
            self.turn_minutes = v;
        }
        if let Some(v) = e.weather {
            self.weather = v;
        }
        if let Some(v) = &e.places {
            self.places = v.clone();
        }
        if let Some(v) = &e.resource_types {
            self.resource_types = v.clone();
        }
        if let Some(v) = &e.roles {
            self.roles = v.clone();
        }
        if let Some(v) = &e.excon_roles {
            self.excon_roles = Vec::new();
            for r in v.iter().map(|r| r.trim()).filter(|r| !r.is_empty()) {
                if !self.excon_roles.iter().any(|x| x == r) {
                    self.excon_roles.push(r.to_string());
                }
            }
        }
        if let Some(v) = &e.default_persona {
            self.default_persona = v.trim().chars().take(80).collect();
        }
        if let Some(v) = &e.reinforcements {
            self.reinforcements = v.clone();
        }
        if let Some(v) = &e.msel {
            self.msel = v.clone();
        }
        if let Some(v) = &e.timeline {
            self.timeline = v.clone();
        }
        self.migrate_legacy_timeline()?;
        if let Some(v) = &e.tactical_policy {
            self.tactical_policy = v.clone();
        }
        if let Some(v) = &e.operations {
            self.operations = v.clone();
        }
        if let Some(v) = &e.staging_base {
            self.staging_base = Some(v.clone()).filter(|v| !v.trim().is_empty());
        }
        if let Some(v) = &e.operational_grants {
            self.operational_grants = v.clone();
        }
        if let Some(v) = &e.districts {
            self.districts = v.clone();
        }
        if let Some(v) = e.district_evacuation {
            self.district_evacuation = v;
        }
        if let Some(v) = &e.translations {
            self.translations = v.clone();
        }
        self.check()
    }

    /// Every editable part with its current value.
    pub fn edits(&self) -> ScenarioEdits {
        ScenarioEdits {
            name: Some(self.name.clone()),
            description: Some(self.description.clone()),
            start_clock_s: Some(self.start_clock_s),
            turn_minutes: Some(self.turn_minutes),
            weather: Some(self.weather),
            places: Some(self.places.clone()),
            resource_types: Some(self.resource_types.clone()),
            roles: Some(self.roles.clone()),
            excon_roles: Some(self.excon_roles.clone()),
            default_persona: Some(self.default_persona.clone()),
            reinforcements: Some(self.reinforcements.clone()),
            msel: Some(self.msel.clone()),
            timeline: None,
            tactical_policy: Some(self.tactical_policy.clone()),
            operations: Some(self.operations.clone()),
            staging_base: Some(self.staging_base.clone().unwrap_or_default()),
            operational_grants: Some(self.operational_grants.clone()),
            districts: Some(self.districts.clone()),
            district_evacuation: Some(self.district_evacuation),
            translations: Some(self.translations.clone()),
            ..Default::default()
        }
    }

    /// Everything wrong with `e` applied to this scenario, in plain words
    /// (empty when it is fine). Unlike [`Scenario::check`] it reports all
    /// problems at once and does not stop at the first.
    pub fn edit_problems(&self, e: &ScenarioEdits, vocab: &Vocabulary) -> Vec<String> {
        let mut out = Vec::new();
        let roles = e.roles.as_deref().unwrap_or(&self.roles);
        let resources = e.resource_types.as_deref().unwrap_or(&self.resource_types);
        let places = e.places.as_deref().unwrap_or(&self.places);
        let weather = e.weather.unwrap_or(self.weather);

        let resource_ids: BTreeSet<&str> = resources.iter().map(|r| r.id.as_str()).collect();
        let role_ids: BTreeSet<&str> = roles.iter().map(|r| r.id.as_str()).collect();

        if e.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            out.push("The scenario name is required".into());
        }
        if !(0.0..360.0).contains(&weather.wind_dir_deg)
            || !(0.0..=200.0).contains(&weather.wind_speed_kmh)
            || !(1.0..=60.0).contains(&weather.moisture_pct)
        {
            out.push("Weather: direction 0–359°, wind 0–200 km/h, moisture 1–60 %".into());
        }
        if e.start_clock_s.is_some_and(|t| !(0..=30 * 86_400).contains(&t)) {
            out.push("Start time out of range".into());
        }
        if e.turn_minutes.is_some_and(|m| !(1..=1440).contains(&m)) {
            out.push("Turn length: 1–1440 minutes".into());
        }

        for (n, r) in resources.iter().enumerate() {
            let at = format!("Resource {}", n + 1);
            if !valid_id(&r.id) {
                out.push(format!("{at}: invalid identifier «{}» (a-z, 0-9, _ -)", r.id));
            }
            if resources[..n].iter().any(|o| o.id == r.id) {
                out.push(format!("{at}: duplicate identifier «{}»", r.id));
            }
            if r.name.trim().is_empty() {
                out.push(format!("{at}: the name is required"));
            }
            if r.speed_mps.is_some_and(|s| !(s.is_finite() && s > 0.0 && s <= 400.0)) {
                out.push(format!("{at}: speed out of range (0–400 m/s)"));
            }
        }

        for (n, r) in roles.iter().enumerate() {
            let at = format!("Role {}", n + 1);
            if !valid_id(&r.id) || r.id == "all" || r.id == "master" {
                out.push(format!("{at}: invalid identifier «{}» (a-z, 0-9, _ -)", r.id));
            }
            if roles[..n].iter().any(|o| o.id == r.id) {
                out.push(format!("{at}: duplicate identifier «{}»", r.id));
            }
            if r.name.trim().is_empty() {
                out.push(format!("{at}: the name is required"));
            }
            if !["national", "regional", "local"].contains(&r.level.as_str()) {
                out.push(format!("{at}: level must be national, regional or local"));
            }
            if r.language.len() != 2 || !r.language.chars().all(|c| c.is_ascii_lowercase()) {
                out.push(format!("{at}: the language is a two-letter code (e.g. en)"));
            }
            for a in &r.allowed_actions {
                if !vocab.role_actions.contains(&a.as_str()) {
                    out.push(format!("{at}: action «{a}» is unknown or reserved to EXCON"));
                }
            }
            for k in r.resources.keys() {
                if !resource_ids.contains(k.as_str()) {
                    out.push(format!("{at}: resource «{k}» is not defined"));
                }
            }
        }

        for (n, r) in e.reinforcements.as_deref().unwrap_or(&self.reinforcements).iter().enumerate() {
            let at = format!("Reinforcement {}", n + 1);
            if !resource_ids.contains(r.resource.as_str()) {
                out.push(format!("{at}: resource «{}» is not defined", r.resource));
            }
            if !role_ids.contains(r.assigned_to.as_str()) {
                out.push(format!("{at}: role «{}» is not defined", r.assigned_to));
            }
            if r.max_qty == 0 {
                out.push(format!("{at}: the quantity must be at least 1"));
            }
        }

        for (n, p) in places.iter().enumerate() {
            let at = format!("Place {}", n + 1);
            if p.name.trim().is_empty() {
                out.push(format!("{at}: the name is required"));
            }
            if places[..n].iter().any(|o| o.name == p.name) {
                out.push(format!("{at}: duplicate name «{}»", p.name));
            }
            if self.cell_of(p.pos).is_none() {
                out.push(format!("{at}: position outside the map"));
            }
        }

        let districts = e.districts.as_deref().unwrap_or(&self.districts);
        let mut claimed: BTreeMap<u32, &str> = BTreeMap::new();
        for (n, d) in districts.iter().enumerate() {
            let at = format!("Borgo {}", n + 1);
            if d.id.trim().is_empty() || d.name.trim().is_empty() {
                out.push(format!("{at}: identificativo e nome sono obbligatori"));
            }
            if districts[..n].iter().any(|o| o.id == d.id || o.name == d.name) {
                out.push(format!("{at}: identificativo o nome «{}» duplicato", d.name));
            }
            if d.buildings.is_empty() {
                out.push(format!("{at}: indica almeno una casa"));
            }
            for id in &d.buildings {
                if !self.buildings.iter().any(|b| b.id == *id) {
                    out.push(format!("{at}: edificio {id} sconosciuto"));
                } else if let Some(other) = claimed.insert(*id, d.id.as_str()) {
                    out.push(format!("{at}: l’edificio {id} è già nel borgo «{other}»"));
                }
            }
            if let Some(g) = d.gathering.as_deref() {
                if !self.buildings.iter().any(|b| b.name.as_deref() == Some(g) && b.shelter_capacity > 0) {
                    out.push(format!("{at}: l’area di raccolta «{g}» non è un edificio con capienza"));
                }
            }
        }
        if let Some(de) = e.district_evacuation {
            if !(de.per_min.is_finite() && de.per_min > 0.0) || de.prep_min > 240 {
                out.push("Ritmo di evacuazione dei borghi: persone al minuto > 0 e preparazione al massimo 240 min".into());
            }
        }

        let msel = e.msel.as_deref().unwrap_or(&self.msel);
        for (n, i) in msel.iter().enumerate() {
            let at = format!("Inject {}", n + 1);
            if i.id.trim().is_empty() || i.title.trim().is_empty() || i.text.trim().is_empty() {
                out.push(format!("{at}: identifier, title and text are required"));
            }
            if msel[..n].iter().any(|o| o.id == i.id) {
                out.push(format!("{at}: duplicate identifier «{}»", i.id));
            }
            if i.turn == 0 {
                out.push(format!("{at}: turns start at 1"));
            }
            if i.recipients.is_empty() {
                out.push(format!("{at}: give at least one recipient (or «all»)"));
            }
            for rcp in &i.recipients {
                if rcp != "all" && !role_ids.contains(rcp.as_str()) {
                    out.push(format!("{at}: unknown recipient «{rcp}»"));
                }
            }
        }

        let mut action_ids: BTreeSet<&str> = BTreeSet::new();
        for (n, i) in msel.iter().enumerate() {
            for a in &i.actions {
                let at = format!("Inject {}, action «{}»", n + 1, a.id);
                if a.id.trim().is_empty() || a.title.trim().is_empty() {
                    out.push(format!("{at}: identifier and title are required"));
                }
                if !action_ids.insert(a.id.as_str()) {
                    out.push(format!("{at}: duplicate identifier"));
                }
                if a.reveal.is_some() {
                    if !a.command.is_null() {
                        out.push(format!("{at}: command and reveal together: choose one"));
                    }
                } else if !a.command.is_object() {
                    out.push(format!("{at}: the command must be a JSON object"));
                }
                if a.reveal.is_some() && a.offset_s < 0 {
                    out.push(format!("{at}: a reveal cannot come before its inject"));
                }
            }
        }

        if let Some(base) = e.staging_base.as_deref().filter(|b| !b.trim().is_empty()) {
            // The place may be one this edit adds.
            let mut probe = Scenario::probe(self);
            probe.places = places.to_vec();
            if probe.resolve_place(base).is_none() {
                out.push(format!("Staging base: unknown place «{base}»"));
            }
        }
        out
    }

    /// A scenario with only what place lookup needs (no rasters), so that a
    /// validation does not copy the territory.
    fn probe(&self) -> Scenario {
        Scenario {
            fuel: vec![],
            dem: vec![],
            render_terrain: None,
            roads: self.roads.clone(),
            buildings: self.buildings.iter().filter(|b| b.name.is_some()).cloned().collect(),
            ..self.shallow()
        }
    }
}

impl Scenario {
    /// A copy without the heavy rasters and with empty edit-only lists.
    fn shallow(&self) -> Scenario {
        Scenario {
            tactical_policy: self.tactical_policy.clone(),
            operations: self.operations.clone(),
            fuel_definitions: vec![],
            id: self.id.clone(),
            name: self.name.clone(),
            description: String::new(),
            grid: self.grid,
            fuel: vec![],
            dem: vec![],
            render_terrain: None,
            start_clock_s: self.start_clock_s,
            turn_minutes: self.turn_minutes,
            weather: self.weather,
            buildings: vec![],
            roads: vec![],
            sectors: self.sectors.clone(),
            districts: self.districts.clone(),
            district_evacuation: self.district_evacuation,
            zones: self.zones,
            places: vec![],
            resource_types: vec![],
            roles: vec![],
            excon_roles: vec![],
            default_persona: String::new(),
            translations: Default::default(),
            reinforcements: vec![],
            msel: vec![],
            timeline: vec![],
            staging_base: None,
            operational_grants: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Scenario {
        let mut s = crate::synthetic::generate(crate::synthetic::DEFAULT_SEED);
        s.enable_operational_model();
        s
    }
        fn vocab() -> Vocabulary<'static> {
        Vocabulary { role_actions: ALL }
    }

    #[test]
    fn packs_carry_map_label_translations_and_english_is_a_target() {
        let mut b = base();
        let place = b.places[0].name.clone();
        assert!(b.map_texts().contains(&place));
        b.translations.entry("it".into()).or_default().insert(place.clone(), "Luogo".into());
        let mut e = ScenarioEdits::default();
        let mut s = b.clone();
        s.apply_edits(&e).unwrap();
        assert_eq!(s.display_map("it")[&place], "Luogo", "absent edits keep the territory's");
        e.translations = Some([("en".to_string(), [(place.clone(), "Place".to_string())].into())].into());
        s.apply_edits(&e).unwrap();
        assert_eq!(s.display_map("en")[&place], "Place");
        assert!(!s.display_map("it").contains_key(&place));
        assert_eq!(s.edits().translations, e.translations);
    }

    #[test]
    fn districts_are_editable_and_checked() {
        let s = base();
        let ids: Vec<u32> = s.buildings.iter().take(3).map(|b| b.id).collect();
        let d = |id: &str, buildings: Vec<u32>| District { id: id.into(), name: id.into(), buildings, gathering: None };
        let ok = ScenarioEdits { districts: Some(vec![d("a", ids[..2].to_vec())]), ..Default::default() };
        assert!(s.edit_problems(&ok, &vocab()).is_empty());
        let mut built = s.clone();
        built.apply_edits(&ok).unwrap();
        assert_eq!(built.edits().districts, ok.districts);
        for (bad, why) in [
            (vec![d("a", vec![])], "no houses"),
            (vec![d("a", vec![u32::MAX])], "unknown building"),
            (vec![d("a", ids[..2].to_vec()), d("b", ids[1..].to_vec())], "house in two boroughs"),
            (vec![d("a", ids[..1].to_vec()), d("a", ids[1..].to_vec())], "duplicate id"),
        ] {
            let e = ScenarioEdits { districts: Some(bad), ..Default::default() };
            assert!(!s.edit_problems(&e, &vocab()).is_empty(), "{why}");
        }
        let slow = ScenarioEdits { district_evacuation: Some(DistrictEvacuation { per_min: 0.0, prep_min: 5 }), ..Default::default() };
        assert!(!s.edit_problems(&slow, &vocab()).is_empty());
    }

    #[test]
    fn full_snapshot_round_trips_and_is_valid() {
        let s = base();
        let e = s.edits();
        let p = s.edit_problems(&e, &Vocabulary { role_actions: ALL });
        assert!(p.is_empty(), "{p:?}");
        let mut again = s.clone();
        again.apply_edits(&e).unwrap();
        assert_eq!(again.roles.len(), s.roles.len());
        assert_eq!(again.msel, s.msel);
        let json = serde_json::to_string(&e).unwrap();
        let back: ScenarioEdits = serde_json::from_str(&json).unwrap();
        assert_eq!(back.msel, e.msel);
    }
    pub(super) const ALL: &[&str] = &[
        "CutFireLine",
        "WaterDrop",
        "DeployEngine",
        "MonitorArea",
        "Reconnoitre",
        "EvacuateZone",
        "EvacuateBuilding",
        "EvacuateDistrict",
        "IssueWarning",
        "CloseRoad",
        "ReopenRoad",
        "OpenShelter",
        "RequestReinforcement",
    ];

    #[test]
    fn turn_minutes_round_trips_validates_and_defaults() {
        let s = base();
        assert_eq!(s.turn_minutes, 30);
        assert_eq!(s.edits().turn_minutes, Some(30));
        let ok = ScenarioEdits { turn_minutes: Some(15), ..Default::default() };
        assert!(s.edit_problems(&ok, &vocab()).is_empty());
        let mut built = s.clone();
        built.apply_edits(&ok).unwrap();
        assert_eq!(built.turn_minutes, 15);
        for bad in [0, 1441] {
            let e = ScenarioEdits { turn_minutes: Some(bad), ..Default::default() };
            let p = s.edit_problems(&e, &vocab());
            assert!(p.iter().any(|m| m.contains("Turn length")), "{p:?}");
        }
        // Packs written before the field existed leave it absent.
        let old: ScenarioEdits = serde_json::from_str("{}").unwrap();
        assert_eq!(old.turn_minutes, None);
        let json = serde_json::to_string(&built.edits()).unwrap();
        let back: ScenarioEdits = serde_json::from_str(&json).unwrap();
        assert_eq!(back.turn_minutes, Some(15));
    }

    #[test]
    fn partial_edits_keep_the_rest() {
        let s = base();
        let pack = ScenarioPack {
            edits: ScenarioEdits {
                weather: Some(WeatherSpec { wind_dir_deg: 90.0, wind_speed_kmh: 10.0, moisture_pct: 8.0 }),
                ..Default::default()
            },
            ..ScenarioPack::plain("calm", "Calm", "x")
        };
        let built = pack.build(s.clone()).unwrap();
        assert_eq!(built.weather.wind_speed_kmh, 10.0);
        assert_eq!(built.roles.len(), s.roles.len());
        assert_eq!(built.msel, s.msel);
    }

    #[test]
    fn problems_are_all_reported() {
        let s = base();
        let mut e = ScenarioEdits::default();
        let mut roles = s.roles.clone();
        roles[0].id = "Bad Id".into();
        roles[1].allowed_actions.push("Ignite".into());
        roles[1].resources.insert("ghosts".into(), 3);
        e.roles = Some(roles);
        let problems = s.edit_problems(&e, &vocab());
        let all = problems.join("\n");
        assert!(all.contains("Bad Id"), "{all}");
        assert!(all.contains("Ignite"), "{all}");
        assert!(all.contains("ghosts"), "{all}");
    }

    #[test]
    fn packs_saved_with_initial_ignitions_still_load() {
        let e: ScenarioEdits =
            serde_json::from_str(r#"{"initial_ignitions":[{"at":[1,2],"radius_m":60}],"turn_minutes":15}"#).unwrap();
        assert_eq!(e.turn_minutes, Some(15));
        assert!(!serde_json::to_string(&e).unwrap().contains("ignitions"));
    }

    #[test]
    fn staging_base_may_be_a_place_added_by_the_same_edit() {
        let s = base();
        let e = ScenarioEdits {
            places: Some(vec![Place { name: "New Depot".into(), aliases: vec![], pos: [1000.0, 1000.0], ..Default::default() }]),
            staging_base: Some("New Depot".into()),
            ..Default::default()
        };
        let p = s.edit_problems(&e, &vocab());
        assert!(p.is_empty(), "{p:?}");
        let bad = ScenarioEdits { staging_base: Some("Nowhere".into()), ..Default::default() };
        assert_eq!(s.edit_problems(&bad, &vocab()).len(), 1);
    }

    #[test]
    fn dangling_inject_recipient_fails_the_build() {
        let s = base();
        let mut msel = s.msel.clone();
        msel[0].recipients = vec!["nobody".into()];
        let pack = ScenarioPack {
            edits: ScenarioEdits { msel: Some(msel), ..Default::default() },
            ..ScenarioPack::plain("x", "X", "b")
        };
        assert!(pack.build(s).is_err());
    }
}
