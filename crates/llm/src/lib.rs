//! LLM gateway and the exercise's prompt workflows.
//!
//! [`Llm`] is the only thing in the system that talks to a model. It never
//! touches the simulation: every workflow returns text or *candidate* actions
//! (plain JSON), which the exercise validates and the EXCON approves.
//!
//! Backends:
//! - **OpenRouter** (`OPENROUTER_API_KEY`, optional `OPENROUTER_MODEL`,
//!   default [`DEFAULT_MODEL`], optional `OPENROUTER_PROVIDERS`, a comma list
//!   in preference order; default [`DEFAULT_PROVIDERS`], `auto` = OpenRouter's choice),
//! - **Mock**, a closure answering by task name — used by the tests,
//! - **Off**, every call fails fast so callers fall back to manual flow.

use std::sync::Arc;
use web_time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};

pub mod cassette;
pub mod tasks;

pub use tasks::{InjectDraft, ParsedReply, RoleMessage};

/// One message of a conversation (`role` is "user" or "assistant").
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// Join consecutive messages of the same role (context notes pile up as
/// several "user" entries between two model answers).
pub fn merge_consecutive(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    for m in messages {
        match out.last_mut() {
            Some(last) if last.role == m.role => {
                last.content.push_str("\n\n");
                last.content.push_str(&m.content);
            }
            _ => out.push(m.clone()),
        }
    }
    out
}

pub const DEFAULT_TRANSLATION_MODEL: &str = "google/gemini-3.1-flash-lite";
pub const DEFAULT_MODEL: &str = "deepseek/deepseek-v4.1-flash";
/// Preferred hosts for [`DEFAULT_MODEL`], fastest first (OpenRouter P50 latency, Sep 2026:
/// Together 0.4 s, CoreWeave 0.6 s, Modal 1.0 s). Others remain as fallbacks.
pub const DEFAULT_PROVIDERS: &[&str] = &["Together", "CoreWeave", "Modal"];
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// A mock answers `(task, system, user)` with the raw model text.
pub type MockFn = dyn Fn(&str, &str, &str) -> Result<String> + Send + Sync;

#[derive(Clone)]
pub enum Backend {
    OpenRouter {
        client: reqwest::Client,
        api_key: String,
        model: String,
        base_url: String,
        providers: Vec<String>,
    },
    Mock(Arc<MockFn>),
    Off,
}

#[derive(Clone)]
pub struct Llm {
    backend: Backend,
    #[cfg(target_arch = "wasm32")]
    runtime: Option<Arc<std::sync::RwLock<Option<Llm>>>>,
    cassette: Option<Arc<cassette::Cassette>>,
    translation_model: Option<String>,
}

impl Llm {
    #[cfg(target_arch = "wasm32")]
    pub fn with_runtime_config(mut self) -> Self {
        self.runtime = Some(Arc::new(std::sync::RwLock::new(None)));
        self
    }
    #[cfg(target_arch = "wasm32")]
    pub fn configure_runtime(&self, configured: Llm) {
        if let Some(runtime) = &self.runtime {
            *runtime.write().unwrap() = Some(configured);
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn configured(&self) -> Option<Llm> {
        self.runtime
            .as_ref()
            .and_then(|r| r.read().unwrap().clone())
    }

    /// OpenRouter if `OPENROUTER_API_KEY` is set, otherwise off.
    pub fn from_env() -> Llm {
        match std::env::var("OPENROUTER_API_KEY") {
            Ok(key) if !key.trim().is_empty() => {
                let model = std::env::var("OPENROUTER_MODEL")
                    .ok()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_MODEL.into());
                let base_url = std::env::var("OPENROUTER_BASE_URL")
                    .unwrap_or_else(|_| DEFAULT_BASE_URL.into());
                let llm = Llm::openrouter(key.trim().to_string(), model, base_url);
                match std::env::var("OPENROUTER_PROVIDERS") {
                    Ok(list) => llm.with_providers(
                        list.split(',')
                            .map(str::trim)
                            .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("auto"))
                            .map(String::from)
                            .collect(),
                    ),
                    Err(_) => llm,
                }
            }
            _ => Llm::off(),
        }
        .with_translation_model_from_env()
        .with_cassette_from_env()
    }

    /// Translation is the only separately selected model; interpretation keeps the existing model.
    fn with_translation_model_from_env(mut self) -> Self {
        if matches!(self.backend, Backend::OpenRouter { .. }) {
            let model = std::env::var("OPENROUTER_TRANSLATION_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_TRANSLATION_MODEL.into());
            self.translation_model = Some(model.trim().into());
        }
        self
    }

    pub fn with_translation_model(mut self, model: &str) -> Self {
        self.translation_model = Some(model.into());
        self
    }

    pub fn model_for(&self, task: &str) -> String {
        #[cfg(target_arch = "wasm32")]
        if let Some(configured) = self.configured() {
            return configured.model_for(task);
        }
        if task == "translate" {
            self.translation_model
                .clone()
                .unwrap_or_else(|| self.model())
        } else {
            self.model()
        }
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

    pub fn has_cassette(&self) -> bool {
        self.cassette.is_some()
    }

    /// Several `Llm`s (e.g. before and after a reconfiguration) can share one tape.
    pub fn with_shared_cassette(mut self, c: Arc<cassette::Cassette>) -> Llm {
        self.cassette = Some(c);
        self
    }

    pub fn openrouter(api_key: String, model: String, base_url: String) -> Llm {
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(Duration::from_secs(90));
        let client = builder.build().expect("http client");
        let providers = DEFAULT_PROVIDERS.iter().map(|p| p.to_string()).collect();
        Llm {
            backend: Backend::OpenRouter {
                client,
                api_key,
                model,
                base_url,
                providers,
            },
            #[cfg(target_arch = "wasm32")]
            runtime: None,
            cassette: None,
            translation_model: None,
        }
    }

    /// Preferred OpenRouter hosts in order (empty = let OpenRouter choose). No effect on other backends.
    pub fn with_providers(mut self, list: Vec<String>) -> Llm {
        if let Backend::OpenRouter { providers, .. } = &mut self.backend {
            *providers = list;
        }
        self
    }

    pub fn mock(f: impl Fn(&str, &str, &str) -> Result<String> + Send + Sync + 'static) -> Llm {
        Llm {
            backend: Backend::Mock(Arc::new(f)),
            #[cfg(target_arch = "wasm32")]
            runtime: None,
            cassette: None,
            translation_model: None,
        }
    }

    pub fn off() -> Llm {
        Llm {
            backend: Backend::Off,
            #[cfg(target_arch = "wasm32")]
            runtime: None,
            cassette: None,
            translation_model: None,
        }
    }

    pub fn is_available(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if let Some(configured) = self.configured() {
            return configured.is_available();
        }
        !matches!(self.backend, Backend::Off)
            || self.cassette.as_ref().is_some_and(|c| !c.is_empty())
    }

    pub fn model(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        if let Some(configured) = self.configured() {
            return configured.model();
        }
        match &self.backend {
            Backend::OpenRouter { model, .. } => model.clone(),
            Backend::Mock(_) => "mock".into(),
            Backend::Off => "off".into(),
        }
    }

    /// One chat completion with a single user message. `json` asks the model for a JSON object.
    pub async fn complete(
        &self,
        task: &str,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String> {
        self.complete_chat(task, system, &[ChatMessage::user(user)], json_mode)
            .await
    }

    /// A completion over a whole conversation (`messages` after the system prompt).
    pub async fn complete_chat(
        &self,
        task: &str,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<String> {
        #[cfg(target_arch = "wasm32")]
        if let Some(configured) = self.configured() {
            return Box::pin(configured.complete_chat(task, system, messages, json_mode)).await;
        }
        let mut routed = self.clone();
        if let Backend::OpenRouter {
            model, providers, ..
        } = &mut routed.backend
        {
            if let Some(selected) = self
                .translation_model
                .as_ref()
                .filter(|_| task == "translate")
            {
                if selected != model {
                    *model = selected.clone();
                    providers.clear();
                }
            }
        }
        routed
            .complete_routed(task, system, messages, json_mode)
            .await
    }

    async fn complete_routed(
        &self,
        task: &str,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<String> {
        let messages = merge_consecutive(messages);
        let Some(tape) = self.cassette.clone() else {
            return self.complete_live(task, system, &messages, json_mode).await;
        };
        // Single-message requests keep the key format of earlier cassettes.
        let flat = if messages.len() == 1 && messages[0].role == "user" {
            messages[0].content.clone()
        } else {
            serde_json::to_string(&messages)?
        };
        let model = if tape.is_model_agnostic() {
            String::new()
        } else {
            self.model()
        };
        let key = cassette::key(&[
            &model,
            task,
            system,
            &flat,
            if json_mode { "json" } else { "text" },
        ]);
        if let Some(answer) = tape.get(&key) {
            tracing::info!(task, "llm replayed from cassette");
            return Ok(answer);
        }
        if tape.mode() == cassette::Mode::Replay {
            anyhow::bail!("LLM cassette miss for task {task} (replay mode)");
        }
        let answer = self
            .complete_live(task, system, &messages, json_mode)
            .await?;
        tape.put(&key, task, &answer);
        Ok(answer)
    }

    async fn complete_live(
        &self,
        task: &str,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<String> {
        let started = Instant::now();
        let out = match &self.backend {
            Backend::Off => anyhow::bail!("LLM is not configured (set OPENROUTER_API_KEY)"),
            // The mock sees the conversation flattened, oldest first.
            Backend::Mock(f) => match messages {
                [only] if only.role == "user" => f(task, system, &only.content),
                _ => f(
                    task,
                    system,
                    &messages
                        .iter()
                        .map(|m| format!("[{}] {}", m.role, m.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            },
            Backend::OpenRouter {
                client,
                api_key,
                model,
                base_url,
                providers,
            } => {
                let mut all = vec![json!({"role": "system", "content": system})];
                all.extend(
                    messages
                        .iter()
                        .map(|m| json!({"role": m.role, "content": m.content})),
                );
                let mut body = json!({
                    "model": model,
                    "temperature": 0.2,
                    "messages": all,
                });
                // Preferred hosts first, but any other may serve if they fail; every
                // host must honour `response_format`; no prompt retention.
                body["provider"] = if providers.is_empty() {
                    json!({"require_parameters": true, "data_collection": "deny"})
                } else {
                    json!({"order": providers, "allow_fallbacks": true, "require_parameters": true, "data_collection": "deny"})
                };
                if json_mode {
                    body["response_format"] = json!({"type": "json_object"});
                }
                let resp = client
                    .post(format!("{base_url}/chat/completions"))
                    .timeout(Duration::from_secs(90))
                    .bearer_auth(api_key)
                    .header("X-Title", "PPRD East TTX")
                    .json(&body)
                    .send()
                    .await
                    .context("calling OpenRouter")?;
                let status = resp.status();
                let v: Value = resp.json().await.context("decoding OpenRouter response")?;
                if !status.is_success() {
                    anyhow::bail!(
                        "OpenRouter {status}: {}",
                        v["error"]["message"].as_str().unwrap_or(&v.to_string())
                    );
                }
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("OpenRouter response has no content: {v}"))
            }
        };
        match &out {
            Ok(t) => {
                tracing::info!(task, model = %self.model(), ms = started.elapsed().as_millis() as u64, chars = t.len(), "llm ok")
            }
            Err(e) => {
                tracing::warn!(task, model = %self.model(), ms = started.elapsed().as_millis() as u64, error = %e, "llm failed")
            }
        }
        out
    }

    /// A completion parsed as a JSON object, retried once on unparseable output.
    pub async fn complete_json(&self, task: &str, system: &str, user: &str) -> Result<Value> {
        self.chat_json(task, system, &[ChatMessage::user(user)])
            .await
    }

    /// A conversation completion parsed as a JSON object, retried once on unparseable output.
    pub async fn chat_json(
        &self,
        task: &str,
        system: &str,
        messages: &[ChatMessage],
    ) -> Result<Value> {
        let text = self.complete_chat(task, system, messages, true).await?;
        match extract_json(&text) {
            Ok(v) => Ok(v),
            Err(_) => {
                let mut retry = messages.to_vec();
                retry.push(ChatMessage::assistant(text));
                retry.push(ChatMessage::user(
                    "Your previous answer was not valid JSON. Answer with ONE JSON object only.",
                ));
                let text = self.complete_chat(task, system, &retry, true).await?;
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
    serde_json::from_str(&t[a..=b])
        .with_context(|| format!("invalid JSON in model output: {t:.200}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fences_and_prose() {
        assert_eq!(extract_json(r#"{"a":1}"#).unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\": 2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("Sure! {\"a\": {\"b\": 3}} hope it helps").unwrap()["a"]["b"],
            3
        );
        assert!(extract_json("no json here").is_err());
    }

    #[tokio::test]
    async fn off_fails_fast_and_mock_answers() {
        assert!(Llm::off().complete("t", "s", "u", false).await.is_err());
        let m = Llm::mock(|task, _, _| Ok(format!("hello from {task}")));
        assert_eq!(
            m.complete("x", "", "", false).await.unwrap(),
            "hello from x"
        );
    }

    #[tokio::test]
    async fn memory_tape_replays_across_models() {
        let tape = Arc::new(cassette::Cassette::memory([]));
        let online = Llm::mock(|_, _, u| Ok(format!("answer to {u}"))).with_shared_cassette(tape.clone());
        assert_eq!(online.complete("t", "s", "hi", false).await.unwrap(), "answer to hi");
        let saved = tape.entries();
        assert_eq!(saved.len(), 1);
        // Restored from saved entries, an offline Llm answers the same request.
        let offline = Llm::off().with_cassette(cassette::Cassette::memory(saved));
        assert!(offline.is_available());
        assert_eq!(offline.complete("t", "s", "hi", false).await.unwrap(), "answer to hi");
        assert!(offline.complete("t", "s", "other", false).await.is_err());
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
        let a = mk()
            .with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Auto).unwrap());
        assert_eq!(
            a.complete("t", "s", "hello", false).await.unwrap(),
            "answer to hello"
        );
        assert_eq!(
            a.complete("t", "s", "hello", false).await.unwrap(),
            "answer to hello"
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "second call replayed"
        );
        // A new process (fresh Llm) replays from the file; replay mode refuses misses.
        let b = Llm::off()
            .with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Replay).unwrap());
        assert!(b.is_available());
        let b_answer = b.complete("t", "s", "hello", false).await;
        // The key includes the model name, which differs ("off" vs "mock"): a miss.
        assert!(b_answer.is_err());
        let m = mk()
            .with_cassette(cassette::Cassette::open(path.clone(), cassette::Mode::Replay).unwrap());
        assert_eq!(
            m.complete("t", "s", "hello", false).await.unwrap(),
            "answer to hello"
        );
        assert!(
            m.complete("t", "s", "other", false).await.is_err(),
            "replay mode: misses are errors"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cassette_key_is_stable_and_separates_parts() {
        assert_eq!(cassette::key(&["a", "b"]), cassette::key(&["a", "b"]));
        assert_ne!(cassette::key(&["ab", ""]), cassette::key(&["a", "b"]));
    }

    #[test]
    fn consecutive_messages_are_merged() {
        let m = merge_consecutive(&[
            ChatMessage::user("a"),
            ChatMessage::user("b"),
            ChatMessage::assistant("c"),
            ChatMessage::user("d"),
        ]);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].content, "a\n\nb");
    }

    #[tokio::test]
    async fn json_retry_on_bad_output() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let m = Llm::mock(move |_, _, _| {
            let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if n == 0 {
                "oops".into()
            } else {
                r#"{"ok":true}"#.into()
            })
        });
        assert_eq!(m.complete_json("t", "", "").await.unwrap()["ok"], true);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}

#[cfg(test)]
mod translation_routing_tests {
    use super::*;
    #[test]
    fn translation_selection_preserves_the_action_and_excon_model() {
        let llm = Llm::openrouter(
            "unused".into(),
            "general-model".into(),
            "http://127.0.0.1:1".into(),
        )
        .with_translation_model("small-translator");
        assert_eq!(llm.model_for("translate"), "small-translator");
        assert_eq!(llm.model_for("prepare_decision"), "general-model");
        assert_eq!(llm.model_for("parse_reply"), "general-model");
        assert_eq!(llm.model_for("master_prompt"), "general-model");
    }
}
