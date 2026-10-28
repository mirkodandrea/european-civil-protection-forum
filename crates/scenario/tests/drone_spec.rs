//! Drone parameters: defaults, validation and the shipped territories.
use scenario::{DroneSpec, Scenario};
use std::path::Path;

#[test]
fn defaults_and_validation() {
    let d = DroneSpec::default();
    assert_eq!(
        (
            d.max_zones,
            d.endurance_s,
            d.turnaround_s,
            d.update_interval_s
        ),
        (4, 2400, 900, 300)
    );
    assert_eq!(d.wind_limit_kmh, None);
    d.check().unwrap();
    let from_json: DroneSpec = serde_json::from_str(r#"{"max_zones":2}"#).unwrap();
    assert_eq!((from_json.max_zones, from_json.endurance_s), (2, 2400));
    for bad in [
        DroneSpec {
            max_zones: 0,
            ..Default::default()
        },
        DroneSpec {
            update_interval_s: 0,
            ..Default::default()
        },
        DroneSpec {
            update_interval_s: 5000,
            ..Default::default()
        },
        DroneSpec {
            wind_limit_kmh: Some(-1.0),
            ..Default::default()
        },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
}

#[test]
fn every_territory_gives_the_fire_commander_one_drone() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scenarios");
    for name in ["synthetic_valley", "valverde_factory", "selvariva_basin"] {
        let scn = Scenario::load(dir.join(format!("{name}.json"))).unwrap();
        let fc = scn.role("fire_commander").unwrap();
        assert_eq!(fc.resources.get("drones"), Some(&1), "{name}");
        assert!(
            fc.allowed_actions.iter().any(|a| a == "MonitorArea"),
            "{name}"
        );
        let spec = scn
            .drone_spec("drones")
            .unwrap_or_else(|| panic!("{name}: no drone resource"));
        assert_eq!(spec.max_zones, 4);
        assert!(scn.drone_spec("engines").is_none());
    }
}
