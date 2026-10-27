use fire::{
    forecast::{ForecastJob, ForecastSpec},
    FireOptions, FireSim,
};
use std::sync::Arc;

fn scene() -> Arc<scenario::Scenario> {
    let mut s = scenario::synthetic::generate(7);
    s.grid = scenario::GridSpec {
        rows: 20,
        cols: 20,
        cellsize: 20.0,
    };
    s.dem = vec![0.0; 400];
    s.fuel = vec![scenario::fuel::GRASSLAND; 400];
    Arc::new(s)
}
fn fire(s: &scenario::Scenario) -> FireSim {
    let mut f = FireSim::with_options(
        s,
        11,
        FireOptions {
            realizations: 2,
            cutoff: 0.5,
        },
    )
    .unwrap();
    f.ignite([200.0, 200.0], 40.0).unwrap();
    f.advance(60).unwrap();
    f
}
#[test]
fn forecast_is_repeatable_non_mutating_and_keeps_completed_interventions() {
    let s = scene();
    let mut a = fire(&s);
    let mut control = fire(&s);
    a.fire_line([50.0, 50.0], [50.0, 350.0], 20.0);
    control.fire_line([50.0, 50.0], [50.0, 350.0], 20.0);
    let spec = ForecastSpec {
        members: 2,
        horizons_s: vec![60, 120],
        ..Default::default()
    };
    let before = (
        a.time_s(),
        a.probability().to_vec(),
        a.added_moisture().to_vec(),
    );
    let p = ForecastJob::capture(&a, s.clone(), spec.clone())
        .unwrap()
        .run()
        .unwrap();
    let q = ForecastJob::capture(&a, s.clone(), spec)
        .unwrap()
        .run()
        .unwrap();
    for (a, b) in p.horizons.iter().zip(&q.horizons) {
        assert_eq!(a.probability, b.probability);
        assert_eq!(a.after_s, b.after_s);
    }
    assert_eq!(
        before,
        (
            a.time_s(),
            a.probability().to_vec(),
            a.added_moisture().to_vec()
        )
    );
    p.check(s.grid).unwrap();
    for (j, cleared) in a.cleared().iter().enumerate() {
        if *cleared {
            assert_eq!(p.horizons[1].probability[j], 0.0);
        }
        assert!(p.horizons[1].probability[j] >= p.horizons[0].probability[j]);
    }
    a.advance(180).unwrap();
    control.advance(180).unwrap();
    assert_eq!(
        a.probability(),
        control.probability(),
        "forecast must not consume live RNG"
    );
    assert_eq!(a.arrival_times(), control.arrival_times());
}
#[test]
fn invalid_or_excessive_forecast_requests_are_rejected() {
    for spec in [
        ForecastSpec {
            members: 0,
            ..Default::default()
        },
        ForecastSpec {
            horizons_s: vec![120, 60],
            ..Default::default()
        },
        ForecastSpec {
            direction_spread_deg: f64::NAN,
            ..Default::default()
        },
        ForecastSpec {
            horizons_s: vec![21601],
            ..Default::default()
        },
    ] {
        let s = scene();
        assert!(ForecastJob::capture(&fire(&s), s, spec).is_err());
    }
}
