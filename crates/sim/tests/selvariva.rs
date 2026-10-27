//! The second shipped map must support the real routing/evacuation model.
use scenario::Scenario;
use sim::{network::Network, Actor, Command, RoadState, World};
use std::{collections::BTreeMap, sync::Arc};

fn territory() -> Scenario {
    Scenario::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/scenarios/selvariva_basin.json"
    ))
    .unwrap()
}

#[test]
fn distinct_map_has_connected_facilities_exits_and_clear_footprints() {
    let s = territory();
    assert_eq!(s.grid.len(), 90_000);
    assert_eq!(s.buildings.iter().map(|b| b.occupants).sum::<u32>(), 1102);
    let graph = Network::build(&s);
    let states = s
        .roads
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                RoadState {
                    closed: false,
                    cut_by_fire: false,
                    fire_closed_links: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let base = s.resolve_place(s.staging_base.as_deref().unwrap()).unwrap();
    for b in &s.buildings {
        assert!(s.cell_of(b.pos).is_some());
        // EU12 bare ground; legacy ids differ.
        assert_eq!(s.fuel_at(s.cell_of(b.pos).unwrap()), 0);
        if b.name.is_some() {
            assert!(
                graph.route(base, b.pos, &states, true, false).is_some(),
                "unreachable {:?}",
                b.name
            );
        }
        for other in s.buildings.iter().filter(|c| c.id > b.id) {
            assert!(
                (b.pos[0] - other.pos[0]).abs() >= (b.size[0] + other.size[0]) / 2.
                    || (b.pos[1] - other.pos[1]).abs() >= (b.size[1] + other.size[1]) / 2.,
                "overlap {} / {}",
                b.id,
                other.id
            );
        }
    }
    for exit in [[0., 2400.], [6000., 2700.], [4100., 0.]] {
        assert!(graph.route(base, exit, &states, true, false).is_some());
    }
}

#[test]
fn second_map_evacuations_arrive_and_restore() {
    let mut w = World::with_options(
        Arc::new(territory()),
        1,
        fire::FireOptions {
            realizations: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let before = w.snapshot();
    let mayor = Actor::Role("mayor".into());
    w.apply(
        &Command::OpenShelter {
            building: "Centro di accoglienza di Pianalto".into(),
        },
        &mayor,
    )
    .unwrap();
    for name in ["Rifugio della scolaresca", "Area festival delle Fonti"] {
        w.apply(
            &Command::EvacuateBuilding {
                building: name.into(),
                destination: Some("Centro di accoglienza di Pianalto".into()),
            },
            &mayor,
        )
        .unwrap();
    }
    w.advance(3600).unwrap();
    assert_eq!(w.stats().sheltered, 400);
    w.restore(&before).unwrap();
    assert_eq!(w.stats().evacuated_people, 0);
    assert_eq!(w.stats().sheltered, 0);
}
