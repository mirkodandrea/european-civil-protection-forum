//! Paired native reference for scripts/qa/standalone.cjs (seed 1, real commands).
use std::sync::Arc;
fn main() -> anyhow::Result<()> {
    let mut scn = scenario::Scenario::load("data/scenarios/valverde_factory.json")?;
    scn.enable_operational_model();
    let world = sim::World::with_options(
        Arc::new(scn),
        1,
        sim::FireOptions {
            realizations: 32,
            cutoff: 0.5,
        },
    )?;
    let mut ex = exercise::Exercise::new(world);
    ex.start()?;
    ex.set_language("mayor", "en")?;
    let initial = ex.world.view();
    // Same physical command as the manually reviewed player proposal in the browser.
    ex.direct_command(&sim::Command::IssueWarning { zone: "F4".into() })?;
    ex.resolve_turn(Some(1))?;
    let resolved = ex.world.view();
    ex.rollback(1)?;
    println!(
        "{}",
        serde_json::json!({"seed":1,"initial":initial,"resolved":resolved,"rollback":ex.world.view()})
    );
    Ok(())
}
