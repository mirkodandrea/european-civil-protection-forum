use anyhow::{Context, Result};
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(
        args.len() == 3,
        "usage: import_factory PACKAGE_DIR FUEL_TABLE OUTPUT_JSON"
    );
    let scn = scenario::factory::import(
        std::path::Path::new(&args[0]),
        std::path::Path::new(&args[1]),
    )?;
    scn.save(&args[2]).context("saving TTX scenario")?;
    println!("Imported {}: {} buildings, {} roads, {} occupants; author the msel (ignitions are Ignite actions of its injects) before exercise",scn.name,scn.buildings.len(),scn.roads.len(),scn.buildings.iter().map(|b|b.occupants as u64).sum::<u64>());
    Ok(())
}
