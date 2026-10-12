//! Write the synthetic scenario package to disk.
//!
//! ```bash
//! cargo run -p scenario --bin gen-scenario -- [--seed 7] [--out data/scenarios/synthetic_valley.json]
//! ```
use anyhow::Result;

fn main() -> Result<()> {
    let mut operational = false;
    let mut seed = scenario::synthetic::DEFAULT_SEED;
    let mut out = String::from("data/scenarios/synthetic_valley.json");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--operational-model" => operational = true,
            "--seed" => seed = args.next().expect("--seed needs a value").parse()?,
            "--out" => out = args.next().expect("--out needs a value"),
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let mut scn = scenario::synthetic::generate(seed);
    if operational {
        scn.enable_operational_model();
    }
    scn.check()?;
    scn.save(&out)?;
    println!(
        "wrote {out}: {} buildings, {} roads, {} roles, {} injects",
        scn.buildings.len(),
        scn.roads.len(),
        scn.roles.len(),
        scn.msel.len()
    );
    Ok(())
}
