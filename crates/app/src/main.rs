//! `ttx` — the whole exercise in one process.
//!
//! ```text
//! ttx [--scenario data/scenarios/valverde_factory.json] [--seed 1]
//!     [--port 8080] [--web web/dist] [--headless]
//!     [--realizations 32] [--cutoff 0.5] [--operational-model]
//!     [--state-dir runs/state | --no-state] [--fresh]
//! ```
//!
//! Loads `.env` (OPENROUTER_API_KEY, optional OPENROUTER_MODEL), builds the
//! world and the exercise, serves the web UIs on the LAN, and — unless
//! `--headless` or built without the `viz` feature — opens the Bevy
//! big-screen window on the main thread.
//!
//! The session (journal of accepted requests, AI answers) and the inject
//! library are kept in `--state-dir`; the session is replayed on startup, so a
//! restart continues the exercise. `--fresh` discards the saved session.

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
    operational_model: bool,
    state_dir: Option<PathBuf>,
    fresh: bool,
}

fn args() -> Result<Args> {
    let mut a = Args {
        scenario: PathBuf::from("data/scenarios/valverde_factory.json"),
        seed: 1,
        port: 8080,
        web: PathBuf::from("web/dist"),
        headless: false,
        realizations: 32,
        cutoff: 0.5,
        operational_model: false,
        state_dir: Some(PathBuf::from("runs/state")),
        fresh: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().with_context(|| format!("{k} needs a value"));
        match k.as_str() {
            "--scenario" => a.scenario = PathBuf::from(val()?),
            "--seed" => a.seed = val()?.parse()?,
            "--port" => a.port = val()?.parse()?,
            "--web" => a.web = PathBuf::from(val()?),
            "--operational-model" => a.operational_model = true,
            "--headless" => a.headless = true,
            "--state-dir" => a.state_dir = Some(PathBuf::from(val()?)),
            "--no-state" => a.state_dir = None,
            "--fresh" => a.fresh = true,
            "--realizations" => a.realizations = val()?.parse()?,
            "--cutoff" => a.cutoff = val()?.parse()?,
            "-h" | "--help" => {
                println!("ttx [--scenario FILE] [--seed N] [--port 8080] [--web DIR] [--headless] [--realizations 32] [--cutoff 0.5] [--operational-model] [--state-dir DIR | --no-state] [--fresh]");
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
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu=warn,naga=warn,bevy_render=warn".into()),
        )
        .init();
    let a = args()?;

    anyhow::ensure!(
        a.scenario.exists() || a.scenario != PathBuf::from("data/scenarios/valverde_factory.json"),
        "default landscape is missing; regenerate with python3 scripts/scenario_factory.py landscape"
    );
    let mut scn = if a.scenario.exists() {
        scenario::Scenario::load(&a.scenario)?
    } else {
        tracing::info!(
            "{} not found: generating the synthetic scenario",
            a.scenario.display()
        );
        let s = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        s.save(&a.scenario)?;
        s
    };
    if a.operational_model {
        scn.enable_operational_model();
    }
    tracing::info!(
        "fire: {}-run Monte Carlo ensemble, cutoff {}",
        a.realizations,
        a.cutoff
    );
    let default_msel = scn.msel.clone();
    let fingerprint = format!(
        "{}|{}|{}|{}|{}|{}",
        a.scenario.display(),
        std::fs::metadata(&a.scenario).map(|m| m.len()).unwrap_or(0),
        a.seed,
        a.realizations,
        a.cutoff,
        a.operational_model
    );
    // Builds a fresh exercise, optionally with replacement injects. The same
    // function serves the first start and "start over" from the settings.
    let factory: server::persist::Factory = {
        let (seed, realizations, cutoff) = (a.seed, a.realizations, a.cutoff);
        Arc::new(move |msel| {
            let mut scn = scn.clone();
            if let Some(msel) = msel {
                scn.msel = msel;
            }
            let world = sim::World::with_options(
                Arc::new(scn),
                seed,
                sim::FireOptions {
                    realizations,
                    cutoff,
                },
            )?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let log = PathBuf::from("runs").join(format!("run-{stamp}.jsonl"));
            tracing::info!("event log: {}", log.display());
            Ok(exercise::Exercise::new(world).with_log_file(log))
        })
    };
    let snapshot = std::env::var("TTX_SNAPSHOT").is_ok();
    let storage = a
        .state_dir
        .clone()
        .filter(|_| !snapshot)
        .map(|dir| {
            if a.fresh {
                let _ = std::fs::remove_file(dir.join("session.json"));
            }
            server::persist::Persist::new(dir, fingerprint, default_msel, factory.clone())
        })
        .map(|persist| {
            let session = persist.load_session();
            (persist, session)
        });
    let msel = storage.as_ref().and_then(|(persist, session)| {
        session
            .as_ref()
            .map(|s| s.msel.clone())
            .or_else(|| persist.load_injects())
    });
    if let Some((persist, session)) = &storage {
        tracing::info!(
            "storage: session {}, inject library {}",
            if session.is_some() { "found" } else { "none" },
            if persist.load_injects().is_some() { "found" } else { "none" },
        );
    }
    let ex = Arc::new(Mutex::new(factory(msel)?));

    // Reproducible visual fixtures go through the same EXCON authority and
    // exercise turn resolution as the console. No renderer-only invented state.
    // TTX_SNAPSHOT_SPEC points to {"commands":[...],"minutes":30}.
    if std::env::var("TTX_SNAPSHOT").is_ok() {
        if let Ok(path) = std::env::var("TTX_SNAPSHOT_SPEC") {
            let spec: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            let commands: Vec<sim::Command> = serde_json::from_value(
                spec.get("commands")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            )?;
            let mut exercise = ex.lock().expect("exercise lock");
            exercise.start()?;
            for command in commands {
                exercise.direct_command(&command)?;
            }
            for order in spec["role_orders"].as_array().into_iter().flatten() {
                let role = order["role"]
                    .as_str()
                    .context("role_orders requires role")?;
                exercise.set_language(role, "en")?;
                let message = exercise.send_inject(
                    None,
                    "Visual fixture",
                    "EXCON",
                    &[role.into()],
                    "Reproducible visual inspection order",
                    &std::collections::BTreeMap::new(),
                )?[0];
                let proposal = exercise.player_reply(
                    role,
                    "Execute fixture order",
                    message,
                    exercise::DecisionEvidence::judgment("Controlled visual regression fixture"),
                )?;
                exercise.attach_parse(
                    proposal,
                    exercise::ParsedInput {
                        resource_limit_draft: None,
                        communication_drafts: vec![],
                        language: "en".into(),
                        translation_en: "Execute fixture order".into(),
                        actions: order["commands"]
                            .as_array()
                            .context("role_orders requires commands array")?
                            .clone(),
                        clarification: None,
                        summary: "Visual fixture deployment".into(),
                        inject_drafts: vec![],
                    },
                )?;
                exercise.decide(
                    proposal,
                    exercise::Decision {
                        approve: true,
                        ..Default::default()
                    },
                )?;
                exercise.execute_approved(proposal)?;
            }
            if let Some(minutes) = spec["minutes"].as_u64() {
                exercise.resolve_turn(Some(u32::try_from(minutes)?))?;
            }
            if let Some(turn) = spec["rollback_to"].as_u64() {
                exercise.rollback(u32::try_from(turn)?)?;
            }
        }
    }

    #[cfg(feature = "viz")]
    if std::env::var("TTX_SNAPSHOT").is_ok() {
        viz::run(
            Box::new(server::view::ExerciseSource(ex)),
            viz::Info {
                llm_model: "offline visual fixture".into(),
                port: Some(a.port),
                remote: None,
            },
        );
        return Ok(());
    }

    anyhow::ensure!(
        a.web.join("index.html").is_file(),
        "Rust frontend missing at {}: run ./scripts/build_frontend.sh (or supply --web DIR)",
        a.web.display()
    );

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
        return rt.block_on(server::serve_with(ex, llm, a.web, addr, storage));
    }

    let model = if llm.is_available() {
        llm.model()
    } else {
        "off".to_string()
    };
    let server_ex = ex.clone();
    let web = a.web.clone();
    std::thread::Builder::new()
        .name("ttx-server".into())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            if let Err(e) = rt.block_on(server::serve_with(server_ex, llm, web, addr, storage)) {
                tracing::error!("server stopped: {e}");
            }
        })?;
    #[cfg(feature = "viz")]
    viz::run(
        Box::new(server::view::ExerciseSource(ex)),
        viz::Info {
            llm_model: model,
            port: Some(a.port),
            remote: None,
        },
    );
    Ok(())
}
