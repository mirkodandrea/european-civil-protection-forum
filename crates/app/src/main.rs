//! `ttx` — the whole exercise in one process.
//!
//! ```text
//! ttx [--scenario data/scenarios/synthetic_valley.json] [--seed 1]
//!     [--port 8080] [--web web] [--headless]
//!     [--realizations 32] [--cutoff 0.5]
//! ```
//!
//! Loads `.env` (OPENROUTER_API_KEY, optional OPENROUTER_MODEL), builds the
//! world and the exercise, serves the web UIs on the LAN, and — unless
//! `--headless` or built without the `viz` feature — opens the Bevy
//! big-screen window on the main thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

struct Args {
    scenario: PathBuf,
    seed: u64,
    port: u16,
    web: PathBuf,
    headless: bool,
    realizations: usize,
    cutoff: f32,
}

fn args() -> Result<Args> {
    let mut a = Args {
        scenario: PathBuf::from("data/scenarios/synthetic_valley.json"),
        seed: 1,
        port: 8080,
        web: PathBuf::from("web"),
        headless: false,
        realizations: 32,
        cutoff: 0.5,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().with_context(|| format!("{k} needs a value"));
        match k.as_str() {
            "--scenario" => a.scenario = PathBuf::from(val()?),
            "--seed" => a.seed = val()?.parse()?,
            "--port" => a.port = val()?.parse()?,
            "--web" => a.web = PathBuf::from(val()?),
            "--headless" => a.headless = true,
            "--realizations" => a.realizations = val()?.parse()?,
            "--cutoff" => a.cutoff = val()?.parse()?,
            "-h" | "--help" => {
                println!("ttx [--scenario FILE] [--seed N] [--port 8080] [--web DIR] [--headless] [--realizations 32] [--cutoff 0.5]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,naga=warn,bevy_render=warn".into()))
        .init();
    let a = args()?;

    let scn = if a.scenario.exists() {
        scenario::Scenario::load(&a.scenario)?
    } else {
        tracing::info!("{} not found: generating the synthetic scenario", a.scenario.display());
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        s.save(&a.scenario)?;
        s
    };
    let world = sim::World::with_options(Arc::new(scn), a.seed, sim::FireOptions { realizations: a.realizations, cutoff: a.cutoff })?;
    tracing::info!("fire: {}-run Monte Carlo ensemble, cutoff {}", a.realizations, a.cutoff);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let log = PathBuf::from("runs").join(format!("run-{stamp}.jsonl"));
    tracing::info!("event log: {}", log.display());
    let ex = Arc::new(Mutex::new(exercise::Exercise::new(world).with_log_file(log)));

    let llm = llm::Llm::from_env();
    if llm.is_available() {
        tracing::info!("LLM: OpenRouter, model {}", llm.model());
    } else {
        tracing::warn!("LLM off (no OPENROUTER_API_KEY): manual mode");
    }
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], a.port).into();

    let headless = a.headless || !cfg!(feature = "viz");
    if headless {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(server::serve(ex, llm, a.web, addr));
    }

    let model = if llm.is_available() { llm.model() } else { "off".to_string() };
    let server_ex = ex.clone();
    let web = a.web.clone();
    std::thread::Builder::new().name("ttx-server".into()).spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        if let Err(e) = rt.block_on(server::serve(server_ex, llm, web, addr)) {
            tracing::error!("server stopped: {e}");
        }
    })?;
    #[cfg(feature = "viz")]
    viz::run(ex, a.port, model);
    Ok(())
}
