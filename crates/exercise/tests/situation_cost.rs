//! Cost of a turn preview on the shipped territories (default 32-run ensemble).
//! Run with `cargo test --release -p exercise --test situation_cost -- --ignored --nocapture`.
use std::sync::Arc;
use std::time::Instant;

use exercise::Exercise;

fn data(name: &str) -> String {
    std::fs::read_to_string(format!("{}/../../data/scenarios/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn exercise_of(pack_file: &str) -> Exercise {
    let pack: scenario::ScenarioPack = serde_json::from_str(&data(pack_file)).unwrap();
    let mut base: scenario::Scenario = serde_json::from_str(&data(&format!("{}.json", pack.base))).unwrap();
    base.migrate_legacy_timeline().unwrap();
    let mut scn = pack.build(base).unwrap();
    scn.enable_operational_model();
    let world = sim::World::with_options(
        Arc::new(scn),
        pack.run.seed.unwrap_or(1),
        sim::FireOptions { realizations: pack.run.realizations.unwrap_or(32), cutoff: pack.run.cutoff.unwrap_or(0.5) },
    )
    .unwrap();
    let mut ex = Exercise::new(world);
    ex.start().unwrap();
    ex
}

#[test]
#[ignore]
fn measure_preview_cost() {
    for name in ["valverde_night_wind", "selvariva_crisi_integrata"] {
        let mut ex = exercise_of(&format!("{name}.pack.json"));
        for step in 0..3 {
            let t = Instant::now();
            let job = ex.preview_job().unwrap();
            let capture = t.elapsed();
            let p = job.run().unwrap();
            let suggest = Instant::now();
            let n = ex.suggestions(Some(&p)).len();
            eprintln!(
                "{name} turn {} ({} min): capture {:?}, run {} ms, suggestions {:?} ({n}), end threatened {}, cut {}",
                ex.turn,
                ex.turn_minutes,
                capture,
                p.wall_ms,
                suggest.elapsed(),
                p.end.totals.threatened,
                p.end.totals.roads_cut
            );
            // Compare with the real resolution of the same turn.
            let real = Instant::now();
            ex.resolve_turn(None).unwrap();
            eprintln!("    real resolution: {:?}", real.elapsed());
            let _ = step;
        }
    }
}
