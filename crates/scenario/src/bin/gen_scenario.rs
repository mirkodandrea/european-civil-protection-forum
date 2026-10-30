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
    let mut scn = scenario::synthetic::with_districts(scenario::synthetic::generate(seed));
    if operational {
        scn.enable_operational_model();
    }
    // Static translations live in the file: keep those whose source text is still used.
    if let Ok(previous) = std::fs::read_to_string(&out) {
        if let Ok(previous) = serde_json::from_str::<scenario::Scenario>(&previous) {
            let texts: std::collections::BTreeSet<String> =
                scn.map_texts().into_iter().chain(scn.translatable_texts()).collect();
            for (lang, table) in previous.translations {
                let kept: std::collections::BTreeMap<_, _> =
                    table.into_iter().filter(|(k, _)| texts.contains(k)).collect();
                if !kept.is_empty() {
                    scn.translations.insert(lang, kept);
                }
            }
        }
    }
    let missing = scn
        .map_texts()
        .iter()
        .filter(|t| !scn.translations.get("it").is_some_and(|m| m.contains_key(*t)))
        .count();
    if missing > 0 {
        eprintln!("{missing} map labels without translations: python3 scripts/factory/translations.py synthetic_valley");
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
