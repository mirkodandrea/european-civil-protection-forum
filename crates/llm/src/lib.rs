//! LLM gateway and the exercise's prompt workflows.
//!
//! [`Llm`] is the only thing in the system that talks to a model. It never
//! touches the simulation: every workflow returns text or *candidate* actions
//! (plain JSON), which the exercise validates and the Master approves.
//!
//! Backends:
//! - **OpenRouter** (`OPENROUTER_API_KEY`, optional `OPENROUTER_MODEL`,
//!   default [`DEFAULT_MODEL`]),
//! - **Mock**, a closure answering by task name — used by the tests,
//! - **Off**, every call fails fast so callers fall back to manual flow.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};

pub mod cassette;
pub mod tasks;

pub use tasks::{InjectDraft, ParsedReply, RoleMessage};

pub const DEFAULT_MODEL: &str = "deepseek/deepseek-v4.1-flash";
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// A mock answers `(task, system, user)` with the raw model text.
pub type MockFn = dyn Fn(&str, &str, &str) -> Result<String> + Send + Sync;

#[derive(Clone)]
pub enum Backend {
    OpenRouter { client: reqwest::Client, api_key: String, model: String, base_url: String },
    Mock(Arc<MockFn>),
    Off,
}

#[derive(Clone)]
pub struct Llm {
    backend: Backend,
    cassette: Option<Arc<cassette::Cassette>>,
}

impl Llm {
    /// OpenRouter if `OPENROUTER_API_KEY` is set, otherwise off.
    pub fn from_env() -> Llm {
        match std::env::var("OPENROUTER_API_KEY") {
            Ok(key) if !key.trim().is_empty() => {
                let model = std::env::var("OPENROUTER_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| DEFAULT_MODEL.into());
                let base_url = std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
                Llm::openrouter(key.trim().to_string(), model, base_url)
            }
            _ => Llm::off(),
        }
        .with_cassette_from_env()
    }

    /// Record/replay answers through the cassette named by `TTX_LLM_CASSETTE`.
    pub fn with_cassette_from_env(self) -> Llm {
        match cassette::Cassette::from_env() {
            Some(c) => self.with_cassette(c),
            None => self,
        }
    }

    pub fn with_cassette(mut self, c: cassette::Cassette) -> Llm {
        self.cassette = Some(Arc::new(c));
        self
    }

    pub fn openrouter(api_key: String, model: String, base_url: String) -> Llm {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(90)).build().expect("http client");
        Llm { backend: Backend::OpenRouter { client, api_key, model, base_url }, cassette: None }
    }

    pub fn mock(f: impl Fn(&str, &str, &str) -> Result<String> + Send + Sync + 'static) -> Llm {
        Llm { backend: Backend::Mock(Arc::new(f)), cassette: None }
    }

    pub fn off() -> Llm {
        Llm { backend: Backend::Off, cassette: None }
    }

    pub fn is_available(&self) -> bool {
        !matches!(self.backend, Backend::Off) || self.cassette.as_ref().is_some_and(|c| !c.is_empty())
    }

    pub fn model(&self) -> String {
        match &self.backend {
            Backend::OpenRouter { model, .. } => model.clone(),
            Backend::Mock(_) => "mock".into(),
            Backend::Off => "off".into(),
        }
    }

    /// One chat completion. `json` asks the model for a JSON object.
    pub async fn complete(&self, task: &str, system: &str, user: &str, json_mode: bool) -> Result<String> {
        let Some(tape) = self.cassette.clone() else {
            return self.complete_live(task, system, user, json_mode).await;
        };
        let key = cassette::key(&[&self.model(), task, system, user, if json_mode { "json" } else { "text" }]);
        if let Some(answer) = tape.get(&key) {
            tracing::info!(task, "llm replayed from cassette");
            return Ok(answer);
        }
        if tape.mode() == cassette::Mode::Replay {
            anyhow::bail!("LLM cassette miss for task {task} (replay mode)");
        }
        let answer = self.complete_live(task, system, user, json_mode).await?;
        tape.put(&key, task, &answer);
        Ok(answer)
    }

    async fn complete_live(&self, task: &str, system: &str, user: &str, json_mode: bool) -> Result<String> {
        let started = Instant::now();
        let out = match &self.backend {
            Backend::Off => anyhow::bail!("LLM is not configured (set OPENROUTER_API_KEY)"),
            Backend::Mock(f) => f(task, system, user),
            Backend::OpenRouter { client, api_key, model, base_url } => {
                let mut body = json!({
                    "model": model,
                    "temperature": 0.2,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user},
                    ],
                });
                if json_mode {
                    body["response_format"] = json!({"type": "json_object"});
                }
                let resp = client
                    .post(format!("{base_url}/chat/completions"))
                    .bearer_auth(api_key)
                    .header("X-Title", "PPRD East TTX")
                    .json(&body)
                    .send()
                    .await
                    .context("calling OpenRouter")?;
                let status = resp.status();
                let v: Value = resp.json().await.context("decoding OpenRouter response")?;
                if !status.is_success() {
                    anyhow::bail!("OpenRouter {status}: {}", v["error"]["message"].as_str().unwrap_or(&v.to_string()));
                }
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("OpenRouter response has no content: {v}"))
            }
        };
        match &out {
            Ok(t) => tracing::info!(task, ms = started.elapsed().as_millis() as u64, chars = t.len(), "llm ok"),
            Err(e) => tracing::warn!(task, ms = started.elapsed().as_millis() as u64, error = %e, "llm failed"),
        }
        out
    }

    /// A completion parsed as a JSON object, retried once on unparseable output.
    pub async fn complete_json(&self, task: &str, system: &str, user: &str) -> Result<Value> {
        let text = self.complete(task, system, user, true).await?;
        match extract_json(&text) {
            Ok(v) => Ok(v),
            Err(_) => {
                let retry = format!("{user}\n\nYour previous answer was not valid JSON. Answer with ONE JSON object only.");
                let text = self.complete(task, system, &retry, true).await?;
                extract_json(&text)
            }
        }
    }
}

/// Pull the JSON object out of a model answer: tolerates code fences and
/// prose around it.
pub fn extract_json(text: &str) -> Result<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Ok(v);
    }
    let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) else {
        anyhow::bail!("no JSON object in model output: {t:.200}");
    };
    serde_json::from_str(&t[a..=b]).with_context(|| format!("invalid JSON in model output: {t:.200}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fences_and_prose() {
        assert_eq!(extract_json(r#"{"a":1}"#).unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\": 2}\n```").unwrap()["a"], 2);
        assert_eq!(extract_json("Sure! {\"a\": {\"b\": 3}} hope it helps").unwrap()["a"]["b"], 3);
        assert!(extract_json("no json here").is_err());
    }

    #[tokio::test]
    async fn off_fails_fast_and_mock_answers() {
        assert!(Llm::off().complete("t", "s", "u", false).await.is_err());
        let m = Llm::mock(|task, _, _| Ok(format!("hello from {task}")));
        assert_eq!(m.complete("x", "", "", false).await.unwrap(), "hello from x");
    }

    #[tokio::test]
    async fn cassette_records_then_replays() {
        let path = std::env::temp_dir().join(format!("ttx-cassette-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let mk = move || {
            let c = c.clone();
            Llm::mock(move |_, _, u| {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(format!("answer to {u}"))
            })
        };
        let a = mk().with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Auto).unwrap());
        assert_eq!(a.complete("t", "s", "hello", false).await.unwrap(), "answer to hello");
        assert_eq!(a.complete("t", "s", "hello", false).await.unwrap(), "answer to hello");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "second call replayed");
        // A new process (fresh Llm) replays from the file; replay mode refuses misses.
        let b = Llm::off().with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Replay).unwrap());
        assert!(b.is_available());
        let b_answer = b.complete("t", "s", "hello", false).await;
        // The key includes the model name, which differs ("off" vs "mock"): a miss.
        assert!(b_answer.is_err());
        let m = mk().with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Replay).unwrap());
        assert_eq!(m.complete("t", "s", "hello", false).await.unwrap(), "answer to hello");
        assert!(m.complete("t", "s", "other", false).await.is_err(), "replay mode: misses are errors");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cassette_key_is_stable_and_separates_parts() {
        assert_eq!(cassette::key(&["a", "b"]), cassette::key(&["a", "b"]));
        assert_ne!(cassette::key(&["ab", ""]), cassette::key(&["a", "b"]));
    }

    #[tokio::test]
    async fn json_retry_on_bad_output() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let m = Llm::mock(move |_, _, _| {
            let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if n == 0 { "oops".into() } else { r#"{"ok":true}"#.into() })
        });
        assert_eq!(m.complete_json("t", "", "").await.unwrap()["ok"], true);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
