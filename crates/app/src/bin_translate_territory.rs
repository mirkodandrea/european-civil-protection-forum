//! Static translations of a territory's map labels, drafted by the LLM.
//!
//! `translate-territory FILE.json [LANG ...]` prints `{lang: {source: text}}`
//! with the labels of [`scenario::Scenario::map_texts`] the file does not
//! translate yet (every player language when none is given). It writes
//! nothing: `scripts/factory/translations.py` merges the answer into the
//! territory and keeps its manifest honest. Review the result before shipping.
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

const LANGUAGES: [&str; 12] = ["en", "it", "fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"];

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next().filter(|a| !a.starts_with('-')) else {
        bail!("usage: translate-territory FILE.json [LANG ...]");
    };
    let mut langs: Vec<String> = args.collect();
    if langs.is_empty() {
        langs = LANGUAGES.iter().map(|l| l.to_string()).collect();
    }
    if let Some(bad) = langs.iter().find(|l| !LANGUAGES.contains(&l.as_str())) {
        bail!("unsupported language {bad}");
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let scn: scenario::Scenario = serde_json::from_str(&text).with_context(|| format!("parsing {path}"))?;
    let llm = llm::Llm::from_env();
    if !llm.is_available() {
        bail!("no LLM configured (OPENROUTER_API_KEY in .env)");
    }
    let texts = scn.map_texts();
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for lang in langs {
        let done = scn.translations.get(&lang);
        let missing: Vec<String> = texts
            .iter()
            .filter(|t| !done.is_some_and(|d| d.contains_key(*t)))
            .cloned()
            .collect();
        if missing.is_empty() {
            continue;
        }
        let got = llm.translate_names(&missing, &lang).await?;
        eprintln!("{lang}: {}/{} labels", got.len(), missing.len());
        out.insert(lang, got);
    }
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}
