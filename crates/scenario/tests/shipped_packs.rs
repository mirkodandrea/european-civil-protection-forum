//! Every pack shipped in `data/scenarios` must load over its base and be valid.
use scenario::{pack::Vocabulary, Scenario, ScenarioPack};
use std::path::Path;

const ROLE_ACTIONS: &[&str] = &[
    "CutFireLine",
    "WaterDrop",
    "DeployEngine",
    "MonitorArea",
    "Reconnoitre",
    "EvacuateZone",
    "EvacuateBuilding",
    "IssueWarning",
    "CloseRoad",
    "ReopenRoad",
    "OpenShelter",
    "RequestReinforcement",
];

#[test]
fn shipped_packs_build_over_their_base() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenarios");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let Some(id) = name.strip_suffix(".pack.json") else { continue };
        let pack: ScenarioPack = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(pack.id, id, "{name}: id must match the file name");
        let base = Scenario::load(dir.join(format!("{}.json", pack.base)))
            .unwrap_or_else(|e| panic!("{name}: base {}: {e}", pack.base));
        let problems = base.edit_problems(&pack.edits, &Vocabulary { role_actions: ROLE_ACTIONS });
        assert!(problems.is_empty(), "{name}: {problems:?}");
        pack.build(base).unwrap_or_else(|e| panic!("{name}: {e}"));
        seen += 1;
    }
    assert!(seen > 0, "no pack found in {}", dir.display());
}
