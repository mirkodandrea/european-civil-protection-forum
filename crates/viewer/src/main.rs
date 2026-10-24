//! `ttx-viewer` — the 3D big screen as a client of a running `ttx` server.
//!
//! ```text
//! ttx-viewer --server http://host:8080 --token EXCON_KEY
//! ```
//!
//! The server (usually `ttx --headless`) keeps the exercise; this process only
//! draws what `/api/view/*` reports. The view shows the ground truth, so the
//! EXCON access key is required (the one printed at server start; also read
//! from `TTX_TOKEN`). It can be started, closed and restarted at will.

use std::io::Read;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use view::{Have, ViewSource, ViewState};

/// Polls the server from a worker thread so a slow network never stalls a frame.
struct Remote {
    base: String,
    token: String,
    agent: ureq::Agent,
    updates: Option<mpsc::Receiver<ViewState>>,
}

impl Remote {
    fn new(base: String, token: String) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(30))
                .build(),
            base,
            token,
            updates: None,
        }
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        let response = self
            .agent
            .get(&format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.token))
            .call();
        match response {
            Ok(r) => {
                let mut body = Vec::new();
                r.into_reader()
                    .take(256 * 1024 * 1024)
                    .read_to_end(&mut body)?;
                Ok(body)
            }
            Err(ureq::Error::Status(401 | 403, _)) => {
                bail!("the server refused the access key (the view needs the EXCON key)")
            }
            Err(e) => bail!("{path}: {e}"),
        }
    }
}

impl ViewSource for Remote {
    fn scenario(&mut self) -> Result<Arc<scenario::Scenario>, String> {
        let load = || -> Result<scenario::Scenario> {
            let body = self.get("/api/view/scene")?;
            serde_json::from_slice(&body).context("scenario")
        };
        let scn = load().map_err(|e| format!("{e:#}"))?;
        // Start polling once the world is known.
        let (tx, rx) = mpsc::sync_channel(1);
        self.updates = Some(rx);
        let (agent, base, token) = (self.agent.clone(), self.base.clone(), self.token.clone());
        std::thread::spawn(move || {
            let worker = Remote {
                agent,
                base,
                token,
                updates: None,
            };
            let mut have = Have::default();
            let mut failing = false;
            loop {
                let path = format!(
                    "/api/view/state?generation={}&cue={}",
                    have.generation, have.cue
                );
                match worker
                    .get(&path)
                    .and_then(|b| Ok(serde_json::from_slice::<Option<ViewState>>(&b)?))
                {
                    Ok(Some(state)) => {
                        failing = false;
                        // Blocks here until the renderer has taken the previous state.
                        have = Have {
                            generation: state.generation,
                            cue: state.cues.iter().map(|c| c.seq).max().unwrap_or(have.cue),
                        };
                        if tx.send(state).is_err() {
                            return;
                        }
                    }
                    Ok(None) => failing = false,
                    Err(e) => {
                        if !failing {
                            eprintln!("view: {e:#} (retrying)");
                        }
                        failing = true;
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Ok(Arc::new(scn))
    }

    fn poll(&mut self, _have: Have) -> Option<ViewState> {
        // The worker keeps track of what it last delivered.
        self.updates.as_ref()?.try_recv().ok()
    }
}

fn main() -> Result<()> {
    let mut server = "http://127.0.0.1:8080".to_string();
    let mut token = std::env::var("TTX_TOKEN").unwrap_or_default();
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().with_context(|| format!("{k} needs a value"));
        match k.as_str() {
            "--server" => server = val()?,
            "--token" => token = val()?,
            "-h" | "--help" => {
                println!("ttx-viewer [--server http://127.0.0.1:8080] --token EXCON_KEY   (or TTX_TOKEN)");
                return Ok(());
            }
            other => bail!("unknown argument {other}"),
        }
    }
    let base = server.trim_end_matches('/').to_string();
    println!("loading the scenario from {base} …");
    viz::run(
        Box::new(Remote::new(base.clone(), token)),
        viz::Info {
            llm_model: String::new(),
            port: None,
            remote: Some(base),
        },
    );
    Ok(())
}
