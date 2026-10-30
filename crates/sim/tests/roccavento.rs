//! Roccavento island: borghi (districts) and their paced evacuation.
use scenario::Scenario;
use sim::{Actor, Command, World};
use std::{collections::BTreeSet, sync::Arc};

fn territory() -> Scenario {
    Scenario::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/scenarios/roccavento_island.json"
    ))
    .unwrap()
}

fn world(s: Scenario) -> World {
    World::with_options(
        Arc::new(s),
        1,
        fire::FireOptions {
            realizations: 2,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn island_has_a_sea_and_every_resident_belongs_to_one_borgo() {
    let s = territory();
    assert_eq!(s.grid.len(), 160_000);
    assert!(s.dem.iter().any(|&z| z < 0.0), "the sea must lie below 0 m");
    assert_eq!(s.buildings.iter().map(|b| b.occupants).sum::<u32>(), 1521);
    let names: Vec<&str> = s.districts.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Castelvento",
            "Pian dei Grilli",
            "Fornaci",
            "San Rocco",
            "Le Terrazze",
            "Le Ghiande"
        ]
    );
    let mut seen = BTreeSet::new();
    for d in &s.districts {
        for id in &d.buildings {
            assert!(seen.insert(*id), "building {id} in two borghi");
        }
    }
    for b in s.buildings.iter().filter(|b| b.occupants > 0) {
        assert!(
            seen.contains(&b.id),
            "resident building {} in no borgo",
            b.id
        );
    }
    // Every borgo but Le Ghiande has its gathering area, as in the source game.
    assert!(s.district("Le Ghiande").unwrap().gathering.is_none());
    assert_eq!(
        s.districts.iter().filter(|d| d.gathering.is_some()).count(),
        5
    );
    assert_eq!(s.district("borgo di san rocco").unwrap().id, "san_rocco");
}

#[test]
fn borgo_leaves_at_its_pace_and_reaches_its_gathering_area_unopened() {
    let s = territory();
    let pace = s.district_evacuation;
    let mut w = world(s);
    let before = w.snapshot();
    let notes = w
        .apply(
            &Command::EvacuateDistrict {
                district: "Le Terrazze".into(),
                destination: None,
            },
            &Actor::Master,
        )
        .unwrap();
    assert!(notes.iter().any(|n| n.contains("93 people")), "{notes:?}");
    let ready: Vec<i64> = w.d.traffic.vehicles.iter().map(|v| v.ready_s).collect();
    let first = *ready.iter().min().unwrap();
    let last = *ready.iter().max().unwrap();
    assert_eq!(first, i64::from(pace.prep_min) * 60);
    // 93 people at 6 per minute: the last home leaves about 15 minutes later.
    assert!(
        (last - first - 13 * 60).abs() <= 180,
        "spread {}",
        last - first
    );
    assert!(w.d.traffic.vehicles.iter().all(|v| v.gathering));
    w.advance(5400).unwrap();
    let gathering = w
        .scn
        .buildings
        .iter()
        .find(|b| b.name.as_deref() == Some("Area di attesa di Castelvento"))
        .unwrap()
        .id;
    assert_eq!(w.d.shelters.get(&gathering).copied(), Some(93));
    w.restore(&before).unwrap();
    assert!(w.d.traffic.vehicles.is_empty());
}

#[test]
fn borgo_names_given_as_zone_or_building_become_borgo_orders() {
    let w = world(territory());
    let (cmd, note) = w.reconcile_command(&Command::EvacuateZone {
        zone: "Castelvento".into(),
        destination: None,
    });
    assert!(
        matches!(cmd, Command::EvacuateDistrict { ref district, .. } if district == "Castelvento")
    );
    assert!(note.is_some());
    let (cmd, _) = w.reconcile_command(&Command::EvacuateBuilding {
        building: "borgo di Fornaci".into(),
        destination: None,
    });
    assert!(matches!(cmd, Command::EvacuateDistrict { ref district, .. } if district == "Fornaci"));
    let mut w = w;
    let err = w
        .apply(
            &Command::EvacuateDistrict {
                district: "Atlantide".into(),
                destination: None,
            },
            &Actor::Master,
        )
        .unwrap_err();
    assert!(err.contains("Le Ghiande"), "{err}");
}

#[test]
fn borgo_without_gathering_area_still_finds_safety() {
    let mut w = world(territory());
    w.apply(
        &Command::EvacuateDistrict {
            district: "Le Ghiande".into(),
            destination: None,
        },
        &Actor::Master,
    )
    .unwrap();
    w.advance(5400).unwrap();
    let arrived: u32 = w.d.shelters.values().sum();
    assert_eq!(
        arrived,
        105,
        "{:?}",
        w.d.traffic.vehicles.iter().find(|v| !v.completed)
    );
}
