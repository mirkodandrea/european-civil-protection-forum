//! Regression: a building is damaged/destroyed only by confirmed fire that has
//! actually reached it (a confirmed burning cell in contact with the building),
//! never by distant fire, the ensemble halo or modelled ember/radiant loads.
//! Playtest bug: buildings three cells away from a still-small fire appeared
//! damaged on the Situation map, and the AI reported them destroyed.
use std::sync::Arc;

use fire::CellFire;
use sim::{Actor, Command, Damage, Loc, World};

fn check(operational: bool) {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    if operational {
        scn.enable_operational_model();
    }
    let mut w = World::new(Arc::new(scn), 1).unwrap();
    let (x, y) = (
        scenario::synthetic::IGNITION[0],
        scenario::synthetic::IGNITION[1],
    );
    let fire = Command::Ignite {
        at: Loc::Point { x, y },
        radius_m: Some(scenario::synthetic::IGNITION_RADIUS_M),
    };
    w.apply(&fire, &Actor::Master).unwrap();
    let contact = sim::contact_radius_m(w.scn.grid.cellsize);
    let cols = w.scn.grid.cols;
    let mut intact: Vec<bool> =
        w.d.buildings
            .iter()
            .map(|b| b.damage == Damage::Intact)
            .collect();
    let mut damaged = 0;
    for _ in 0..150 {
        w.advance(60).unwrap();
        let reached: Vec<[f32; 2]> = w
            .fire
            .state()
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != CellFire::Unburnt)
            .map(|(i, _)| {
                w.scn.cell_centre(scenario::Cell {
                    row: i / cols,
                    col: i % cols,
                })
            })
            .collect();
        for (i, b) in w.d.buildings.iter().enumerate() {
            if !intact[i] || b.damage == Damage::Intact {
                continue;
            }
            intact[i] = false;
            damaged += 1;
            let p = w.scn.buildings[i].pos;
            let nearest = reached
                .iter()
                .map(|q| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt())
                .fold(f32::INFINITY, f32::min);
            assert!(
                nearest <= contact,
                "building #{} became {:?} at t={} s with the nearest confirmed fire {nearest:.0} m away (contact {contact:.0} m, operational={operational})",
                b.id,
                b.damage,
                w.time_s()
            );
        }
    }
    assert!(
        damaged > 0,
        "unchecked fire should still damage buildings it reaches"
    );
    // The LLM digest reports the same confirmed damage as the map.
    let s = w.stats();
    let digest = w.digest(None);
    assert_eq!(digest["totals"]["buildings_damaged"], s.damaged);
    assert_eq!(digest["totals"]["buildings_destroyed"], s.destroyed);
}

#[test]
fn damage_requires_confirmed_fire_contact_legacy() {
    check(false);
}

#[test]
fn damage_requires_confirmed_fire_contact_operational() {
    check(true);
}
