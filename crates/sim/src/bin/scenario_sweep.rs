//! Reproducible headless fire atlas: Python authors jobs, Rust runs the core.
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
struct Job {
    name: String,
    seed: u64,
    at: scenario::Pos,
    radius_m: f32,
    wind_from_deg: f64,
    wind_kmh: f64,
    moisture_pct: f64,
    minutes: i64,
    realizations: usize,
    cutoff: f32,
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(
        args.len() == 3,
        "usage: scenario_sweep SCENARIO_JSON JOBS_JSON OUTPUT_DIR"
    );
    let base = scenario::Scenario::load(&args[0])?;
    let jobs: Vec<Job> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let out = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(out)?;
    let mut results = vec![];
    for (index, job) in jobs.iter().enumerate() {
        anyhow::ensure!(
            job.minutes > 0
                && job.minutes <= 20160
                && job.radius_m.is_finite()
                && job.radius_m >= 40.0,
            "invalid sweep job"
        );
        let mut scn = base.clone();
        scn.weather = scenario::WeatherSpec {
            wind_dir_deg: job.wind_from_deg,
            wind_speed_kmh: job.wind_kmh,
            moisture_pct: job.moisture_pct,
        };
        let mut fire = fire::FireSim::with_options(
            &scn,
            job.seed,
            fire::FireOptions {
                realizations: job.realizations,
                cutoff: job.cutoff,
            },
        )?;
        fire.ignite(job.at, job.radius_m)?;
        let mut areas = vec![];
        for minute in 1..=job.minutes {
            fire.advance(60)?;
            if minute % 30 == 0 || minute == job.minutes {
                areas.push(serde_json::json!({"minute":minute,"ha":fire.burned_area_ha()}));
            }
        }
        let raster = format!("arrival-{index:04}.json");
        std::fs::write(out.join(&raster), serde_json::to_vec(fire.arrival_times())?)?;
        results.push(serde_json::json!({"job":job,"area":areas,"arrival":raster,"burned_ha":fire.burned_area_ha(),"established":fire.burned_area_ha()>=5.0}));
    }
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"scenario":base.id,"grid":base.grid,"operations":base.operations,"results":results}),
        )?,
    )?;
    println!("{} reproducible runs in {}", jobs.len(), out.display());
    Ok(())
}
