//! Fire behaviour on the shipped synthetic scenario, and every lever the
//! exercise has on it.
use fire::{CellFire, FireSim};
use scenario::{synthetic, Pos, Scenario};

fn scenario() -> Scenario {
    synthetic::generate(synthetic::DEFAULT_SEED)
}

fn started(scn: &Scenario, seed: u64) -> FireSim {
    let mut f = FireSim::new(scn, seed).unwrap();
    let ign = &scn.initial_ignitions[0];
    f.ignite(ign.at, ign.radius_m).unwrap();
    f
}

fn nearest_fire_m(scn: &Scenario, f: &FireSim, p: Pos) -> f32 {
    let cols = scn.grid.cols;
    f.state()
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != CellFire::Unburnt)
        .map(|(i, _)| {
            let q = scn.cell_centre(scenario::Cell { row: i / cols, col: i % cols });
            ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt()
        })
        .fold(f32::MAX, f32::min)
}

#[test]
fn fire_reaches_pineta_within_ninety_minutes() {
    let scn = scenario();
    let school = scn.resolve_place("Pineta Primary School").unwrap();
    // Individual seeds vary (one realization is one sample): pin the mean.
    let mut total = 0.0;
    for seed in [1, 2, 3] {
        let mut f = started(&scn, seed);
        f.advance(90 * 60).unwrap();
        let d = nearest_fire_m(&scn, &f, school);
        assert!(d < 450.0, "seed {seed}: fire still {d:.0} m from the school after 90 min");
        assert!(f.burned_area_ha() > 10.0);
        total += d;
    }
    assert!(total / 3.0 < 320.0, "mean distance {:.0} m", total / 3.0);
}

#[test]
fn fire_grows_and_burns_out() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    f.advance(15 * 60).unwrap();
    let a1 = f.burned_area_ha();
    f.advance(60 * 60).unwrap();
    assert!(f.burned_area_ha() > a1);
    assert!(f.state().iter().any(|s| *s == CellFire::Burnt), "nothing burnt out after 75 min");
}

#[test]
fn nothing_burns_before_ignition() {
    let scn = scenario();
    let mut f = FireSim::new(&scn, 1).unwrap();
    f.advance(3600).unwrap();
    assert_eq!(f.burned_area_ha(), 0.0);
}

/// A fire line across the head of the fire must protect what is behind it
/// (the Pineta side), and must not act anywhere else (the NaN-sparse
/// `vegetation_changes` trap: the fire keeps burning on its own side).
#[test]
fn fire_line_holds_the_head() {
    let scn = scenario();
    let (a, b) = ([2650.0f32, 2880.0f32], [3300.0f32, 2230.0f32]);
    // Signed side of the line; the school is on the negative side.
    let side = |p: Pos| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let school = scn.resolve_place("Pineta Primary School").unwrap();
    assert!(side(school) < 0.0);
    let behind = |f: &FireSim| {
        let cols = scn.grid.cols;
        f.state()
            .iter()
            .enumerate()
            .filter(|(i, s)| **s != CellFire::Unburnt && !f.cleared()[*i])
            .filter(|(i, _)| side(scn.cell_centre(scenario::Cell { row: i / cols, col: i % cols })) < -30.0 * 920.0)
            .count()
    };
    let (mut free, mut lined) = (0, 0);
    for seed in [1, 2, 3] {
        let mut f = started(&scn, seed);
        f.advance(20 * 60).unwrap();
        let mut g = started(&scn, seed);
        g.advance(20 * 60).unwrap();
        let already = behind(&g);
        let cut = g.fire_line(a, b, 60.0);
        // A checkpoint before physics must retain the pending fuel edit.
        let checkpoint=g.snapshot();
        g.restore(&checkpoint,seed).unwrap();
        assert!(cut > 50, "only {cut} cells cut");
        f.advance(90 * 60).unwrap();
        g.advance(90 * 60).unwrap();
        free += behind(&f);
        lined += behind(&g) - already;
        assert!(g.burned_area_ha() > 10.0, "the fire must keep burning on its own side");
    }
    assert!(free > 100, "without the line the fire should cross to the Pineta side ({free} cells)");
    assert!(lined * 5 < free, "the line should hold: {lined} cells crossed vs {free} without it (3 seeds)");
}

#[test]
fn water_drop_wets_fuel_and_decays() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    let n = f.water_drop([2900.0, 2500.0], 60.0, 1.5);
    assert!(n > 20);
    let wet = f.added_moisture().iter().cloned().fold(0.0, f32::max);
    assert!((wet - 45.0).abs() < 1e-3, "1.5 L/m2 should add 45 points, got {wet}");
    f.advance(3600).unwrap();
    let later = f.added_moisture().iter().cloned().fold(0.0, f32::max);
    assert!(later < wet && later > 0.0);
}

#[test]
fn extinguish_stops_the_fire() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    f.advance(20 * 60).unwrap();
    f.extinguish(scn.initial_ignitions[0].at, 900.0);
    assert_eq!(f.burning_cells(), 0);
    let area = f.burned_area_ha();
    f.advance(60 * 60).unwrap();
    assert_eq!(f.burning_cells(), 0);
    assert!((f.burned_area_ha() - area).abs() < 1e-9, "fire regrew after being put out");
}

#[test]
fn ignition_on_non_burnable_ground_is_refused() {
    let scn = scenario();
    let mut f = FireSim::new(&scn, 1).unwrap();
    let town = scn.resolve_place("Valverde").unwrap();
    assert!(f.ignite(town, 50.0).is_err());
}

#[test]
fn snapshot_restore_rolls_back() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    f.advance(15 * 60).unwrap();
    let snap = f.snapshot();
    let area = f.burned_area_ha();
    f.fire_line([2500.0, 2800.0], [3350.0, 1950.0], 60.0);
    f.advance(60 * 60).unwrap();
    assert!(f.burned_area_ha() > area);
    f.restore(&snap, 99).unwrap();
    assert_eq!(f.time_s(), 15 * 60);
    assert!((f.burned_area_ha() - area).abs() < 1e-9);
    assert!(f.cleared().iter().all(|c| !c), "fire line survived the rollback");
    f.advance(30 * 60).unwrap();
    assert!(f.burned_area_ha() > area, "restored fire does not continue");
}

#[test]
fn weather_change_is_applied() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    let mut w = f.weather();
    w.moisture_pct = 40.0; // soaking: fire should nearly stall
    f.set_weather(w).unwrap();
    f.advance(60 * 60).unwrap();
    let wet = f.burned_area_ha();
    let mut g = started(&scn, 1);
    g.advance(60 * 60).unwrap();
    assert!(wet < g.burned_area_ha(), "wet {wet} vs dry {}", g.burned_area_ha());
}

#[test]
fn extinguish_a_whole_zone() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    f.advance(15 * 60).unwrap();
    let zone = scn.zone_of(scn.initial_ignitions[0].at).unwrap();
    let b = scn.zone_bounds(&zone).unwrap();
    let before = f.burning_cells();
    let out = f.extinguish_rect(b);
    assert!(out > 0 && f.burning_cells() < before);
    // Nothing inside the zone burns afterwards.
    f.advance(30 * 60).unwrap();
    for c in f.cells_in_rect(b) {
        assert_ne!(f.cell_state(c), CellFire::Burning);
    }
}

fn started_with(scn: &Scenario, seed: u64, options: fire::FireOptions) -> FireSim {
    let mut f = FireSim::with_options(scn, seed, options).unwrap();
    let ign = &scn.initial_ignitions[0];
    f.ignite(ign.at, ign.radius_m).unwrap();
    f
}

/// The "actual" fire is the cutoff quantile of the ensemble: every reached
/// cell was reached by at least that fraction of the realizations, and a lower
/// cutoff gives a larger, pessimistic fire.
#[test]
fn ensemble_cutoff_defines_the_fire() {
    let scn = scenario();
    let median = fire::FireOptions { realizations: 32, cutoff: 0.5 };
    let pessimistic = fire::FireOptions { realizations: 32, cutoff: 0.25 };
    let mut m = started_with(&scn, 1, median);
    let mut p = started_with(&scn, 1, pessimistic);
    m.advance(60 * 60).unwrap();
    p.advance(60 * 60).unwrap();
    assert!(p.burned_area_ha() > m.burned_area_ha(), "0.25 cutoff {} ha should exceed median {} ha", p.burned_area_ha(), m.burned_area_ha());
    for (i, s) in m.state().iter().enumerate() {
        let pr = m.probability()[i];
        assert!((0.0..=1.0).contains(&pr));
        if *s != CellFire::Unburnt {
            assert!(pr >= 0.5 - 1e-6, "cell {i} is part of the median fire with probability {pr}");
        }
    }
    // Some cells are reached by a minority of runs: the uncertainty halo.
    assert!(m.probability().iter().zip(m.state()).any(|(pr, s)| *pr > 0.0 && *s == CellFire::Unburnt));
}

#[test]
fn ensemble_fire_only_grows_and_is_reproducible() {
    let scn = scenario();
    let mut a = FireSim::new(&scn, 7).unwrap();
    let mut b = FireSim::new(&scn, 7).unwrap();
    let ign = &scn.initial_ignitions[0];
    a.ignite(ign.at, ign.radius_m).unwrap();
    b.ignite(ign.at, ign.radius_m).unwrap();
    let mut last = 0.0;
    for _ in 0..12 {
        a.advance(300).unwrap();
        assert!(a.burned_area_ha() >= last);
        last = a.burned_area_ha();
    }
    b.advance(3600).unwrap();
    assert_eq!(a.arrival_times(), b.arrival_times(), "same seed, same fire, whatever the step size");
}

#[test]
fn single_realization_still_works() {
    let scn = scenario();
    let mut f = started_with(&scn, 1, fire::FireOptions { realizations: 1, cutoff: 0.5 });
    f.advance(3600).unwrap();
    assert!(f.burned_area_ha() > 5.0);
    assert!(f.probability().iter().all(|p| *p == 0.0 || *p == 1.0));
}

#[test]
fn intensity_and_flame_length_are_physical() {
    let scn = scenario();
    let mut f = started(&scn, 1);
    f.advance(45 * 60).unwrap();
    let mut burned: Vec<f32> = f.state().iter().zip(f.intensity()).filter(|(s, _)| **s != CellFire::Unburnt).map(|(_, i)| *i).collect();
    assert!(!burned.is_empty());
    burned.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = burned[burned.len() / 2];
    let max = *burned.last().unwrap();
    println!("fireline intensity kW/m: median {median:.0}, max {max:.0}; flame length median {:.1} m, max {:.1} m", fire::flame_length_m(median), fire::flame_length_m(max));
    assert!(median > 50.0 && max < 200_000.0, "median {median}, max {max}");
    assert_eq!(fire::flame_length_m(0.0), 0.0);
    assert!((fire::flame_length_m(10_000.0) - 5.4).abs() < 0.2);
}
