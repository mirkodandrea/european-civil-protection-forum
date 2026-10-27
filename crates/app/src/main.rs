//! `ttx` — the whole exercise in one process.
//!
//! ```text
//! ttx [--scenario data/scenarios/valverde_factory.json] [--scenarios-dir DIR]
//!     [--seed 1] [--port 8080] [--web web/dist] [--headless]
//!     [--realizations 32] [--cutoff 0.5] [--operational-model]
//!     [--state-dir runs/state | --no-state] [--fresh]
//! ```
//!
//! Loads `.env` (OPENROUTER_API_KEY, optional OPENROUTER_MODEL), builds the
//! world and the exercise, serves the web UIs on the LAN, and — unless
//! `--headless` or built without the `viz` feature — opens the Bevy
//! big-screen window on the main thread.
//!
//! The session (journal of accepted requests, AI answers, the scenario it
//! started from) and the chosen scenario are kept in `--state-dir`; the
//! session is replayed on startup, so a restart continues the exercise.
//! `--fresh` discards the saved session.
//!
//! Scenarios live in `--scenarios-dir` (default: the folder of `--scenario`):
//! `<id>.json` is a territory, `<id>.pack.json` a scenario designed on top of
//! one (roles, resources, ignitions, MSEL…), editable from the EXCON console.
//! `--scenario` names the territory or pack to start from. `--seed`,
//! `--realizations` and `--cutoff` override what a pack says.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

struct Args {
    scenario: PathBuf,
    scenarios_dir: Option<PathBuf>,
    seed: Option<u64>,
    port: u16,
    web: PathBuf,
    headless: bool,
    realizations: Option<usize>,
    cutoff: Option<f32>,
    operational_model: bool,
    state_dir: Option<PathBuf>,
    fresh: bool,
}

fn args() -> Result<Args> {
    let mut a = Args {
        scenario: PathBuf::from("data/scenarios/valverde_factory.json"),
        scenarios_dir: None,
        seed: None,
        port: 8080,
        web: PathBuf::from("web/dist"),
        headless: false,
        realizations: None,
        cutoff: None,
        operational_model: false,
        state_dir: Some(PathBuf::from("runs/state")),
        fresh: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().with_context(|| format!("{k} needs a value"));
        match k.as_str() {
            "--scenario" => a.scenario = PathBuf::from(val()?),
            "--scenarios-dir" => a.scenarios_dir = Some(PathBuf::from(val()?)),
            "--seed" => a.seed = Some(val()?.parse()?),
            "--port" => a.port = val()?.parse()?,
            "--web" => a.web = PathBuf::from(val()?),
            "--operational-model" => a.operational_model = true,
            "--headless" => a.headless = true,
            "--state-dir" => a.state_dir = Some(PathBuf::from(val()?)),
            "--no-state" => a.state_dir = None,
            "--fresh" => a.fresh = true,
            "--realizations" => a.realizations = Some(val()?.parse()?),
            "--cutoff" => a.cutoff = Some(val()?.parse()?),
            "-h" | "--help" => {
                println!("ttx [--scenario FILE] [--scenarios-dir DIR] [--seed N] [--port 8080] [--web DIR] [--headless] [--realizations 32] [--cutoff 0.5] [--operational-model] [--state-dir DIR | --no-state] [--fresh]");
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

    // `--scenario` names a territory (`<id>.json`) or a pack (`<id>.pack.json`).
    let dir = a
        .scenarios_dir
        .clone()
        .or_else(|| a.scenario.parent().map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()))
        .unwrap_or_else(|| PathBuf::from("data/scenarios"));
    let file_name = a
        .scenario
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("valverde_factory.json")
        .to_string();
    let default_id = file_name
        .trim_end_matches(".json")
        .trim_end_matches(".pack")
        .to_string();
    let is_pack = file_name.ends_with(".pack.json");
    let default_landscape = PathBuf::from("data/scenarios/valverde_factory.json");
    let base_file = dir.join(format!("{default_id}.json"));
    if !is_pack && !base_file.exists() {
        anyhow::ensure!(
            a.scenario != default_landscape,
            "default landscape is missing; regenerate with python3 scripts/scenario_factory.py landscape"
        );
        tracing::info!(
            "{} not found: generating the synthetic scenario",
            base_file.display()
        );
        scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED).save(&base_file)?;
    }
    let state_dir = a.state_dir.clone().filter(|_| std::env::var("TTX_SNAPSHOT").is_err());
    let library = Arc::new(server::library::FileLibrary::new(dir.clone(), state_dir.clone()));
    let base = server::library::file_base_loader(dir.clone());

    // Seed, ensemble size and cutoff: command line, else the pack, else default.
    let run_of = {
        let a = (a.seed, a.realizations, a.cutoff, a.operational_model);
        move |pack: &scenario::ScenarioPack| {
            (
                a.0.or(pack.run.seed).unwrap_or(1),
                a.1.or(pack.run.realizations).unwrap_or(32),
                a.2.or(pack.run.cutoff).unwrap_or(0.5),
                a.3 || pack.run.operational_model == Some(true),
            )
        }
    };
    let factory: server::library::Factory = {
        let (base, run_of) = (base.clone(), run_of.clone());
        Arc::new(move |pack| {
            let (seed, realizations, cutoff, operational) = run_of(pack);
            let mut scn = pack.build((*base(&pack.base)?).clone())?;
            if operational {
                scn.enable_operational_model();
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
    let fingerprint: Arc<dyn Fn(&scenario::ScenarioPack) -> String + Send + Sync> = {
        let dir = dir.clone();
        Arc::new(move |pack| {
            let (seed, realizations, cutoff, operational) = run_of(pack);
            let size = std::fs::metadata(dir.join(format!("{}.json", pack.base)))
                .map(|m| m.len())
                .unwrap_or(0);
            format!("{}|{size}|{seed}|{realizations}|{cutoff}|{operational}", pack.base)
        })
    };
    let scenarios = Arc::new(server::library::Scenarios {
        library,
        base,
        build: factory,
        fingerprint,
        default_id: default_id.clone(),
        running: Mutex::new(None),
    });
    tracing::info!(
        "scenarios in {}: starting from {}",
        dir.display(),
        default_id
    );
    let storage = state_dir
        .clone()
        .map(|dir| {
            if a.fresh {
                let _ = std::fs::remove_file(dir.join("session.json"));
            }
            server::persist::Persist::new(dir, scenarios.clone())
        })
        .map(|persist| {
            let session = persist.load_session();
            (persist, session)
        });
    // Start from the saved session's scenario, else the chosen one, else the
    // one named on the command line.
    let pack = storage
        .as_ref()
        .and_then(|(_, session)| session.as_ref().map(|s| s.pack.clone()))
        .or_else(|| scenarios.next_pack())
        .with_context(|| format!("scenario {default_id} not found in {}", dir.display()))?;
    if let Some((_, session)) = &storage {
        tracing::info!(
            "storage: session {}, scenario {}",
            if session.is_some() { "found" } else { "none" },
            pack.id,
        );
    }
    let ex = Arc::new(Mutex::new((scenarios.build)(&pack)?));
    scenarios.set_running(&pack);


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
        return rt.block_on(server::serve_with(ex, llm, a.web, addr, storage, Some(scenarios)));
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
            if let Err(e) = rt.block_on(server::serve_with(server_ex, llm, web, addr, storage, Some(scenarios))) {
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
