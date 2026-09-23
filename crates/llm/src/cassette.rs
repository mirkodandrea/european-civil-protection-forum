//! Record / replay of model answers ("cassette"), keyed by the exact request.
//!
//! Used by the demo recording to make re-recording fast and reproducible: the
//! first run records every `(model, task, system, user, json)` request and its
//! answer to a JSONL file; later runs answer from the file instantly. Because
//! the simulator is seeded and the replayed answers are identical, every later
//! prompt is identical too, so the whole session replays exactly.
//!
//! Enabled with `TTX_LLM_CASSETTE=<path>`; `TTX_LLM_CASSETTE_MODE` is `auto`
//! (default: replay hits, record misses), `replay` (misses are errors) or
//! `record` (always call the model, overwrite entries).

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Replay,
    Record,
}

pub struct Cassette {
    path: PathBuf,
    mode: Mode,
    entries: Mutex<HashMap<String, String>>,
}

/// Stable 64-bit FNV-1a, hex. (std's hasher is not stable across releases.)
pub fn key(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.as_bytes().iter().chain(std::iter::once(&0xffu8)) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

impl Cassette {
    pub fn open(path: PathBuf, mode: Mode) -> Result<Cassette> {
        let mut entries = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    if let (Some(k), Some(a)) = (v["key"].as_str(), v["answer"].as_str()) {
                        entries.insert(k.to_string(), a.to_string());
                    }
                }
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Cassette { path, mode, entries: Mutex::new(entries) })
    }

    pub fn from_env() -> Option<Cassette> {
        let path = std::env::var("TTX_LLM_CASSETTE").ok().filter(|p| !p.trim().is_empty())?;
        let mode = match std::env::var("TTX_LLM_CASSETTE_MODE").unwrap_or_default().as_str() {
            "replay" => Mode::Replay,
            "record" => Mode::Record,
            _ => Mode::Auto,
        };
        match Cassette::open(PathBuf::from(&path), mode) {
            Ok(c) => {
                tracing::info!("LLM cassette {path} ({mode:?}, {} entries)", c.len());
                Some(c)
            }
            Err(e) => {
                tracing::warn!("LLM cassette {path}: {e}");
                None
            }
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &str) -> Option<String> {
        if self.mode == Mode::Record {
            return None;
        }
        self.entries.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: &str, task: &str, answer: &str) {
        if let Ok(mut e) = self.entries.lock() {
            e.insert(key.to_string(), answer.to_string());
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{}", json!({"key": key, "task": task, "answer": answer}));
        }
    }
}
