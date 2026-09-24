//! Live OpenRouter smoke test. Skipped unless `TTX_LIVE_LLM=1` and
//! `OPENROUTER_API_KEY` are set (reads `.env` from the workspace root).
//!
//! ```bash
//! TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture
//! ```
use llm::Llm;

fn live() -> Option<Llm> {
    if std::env::var("TTX_LIVE_LLM").ok().as_deref() != Some("1") {
        eprintln!("skipped: set TTX_LIVE_LLM=1");
        return None;
    }
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env");
    if let Ok(text) = std::fs::read_to_string(root) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if std::env::var(k.trim()).is_err() {
                    std::env::set_var(k.trim(), v.trim());
                }
            }
        }
    }
    let llm = Llm::from_env();
    assert!(llm.is_available(), "OPENROUTER_API_KEY missing");
    Some(llm)
}

#[tokio::test]
async fn translate_and_parse_live() {
    let Some(llm) = live() else { return };
    eprintln!("model: {}", llm.model());
    let it = llm.translate("Evacuate zone F4 to the Valverde Sports Centre.", "it").await.unwrap();
    eprintln!("translation: {it}");
    assert!(it.contains("F4"));

    let g = serde_json::json!({"places": ["Pineta", "Valverde Sports Centre"], "map": "zones A1..H8"});
    let p = llm
        .parse_reply(llm::tasks::ReplyInput {
            role_id: "mayor",
            role_name: "Mayor of Valverde",
            role_description: "local authority",
            language: "it",
            catalogue: &"- {\"type\":\"EvacuateZone\",\"zone\":\"F4\",\"destination\":\"Valverde Sports Centre\"}\n- {\"type\":\"IssueWarning\",\"zone\":\"F4\"}",
            other_roles: &[("fire_commander".into(), "Fire Commander".into())],
            gazetteer: &g,
            digest: &serde_json::json!({"clock": "Day 1 13:30"}),
            history: &[],
            reply: "Evacuate zone F4 toward the Valverde sports centre and warn F3 too. Tell the fire commander the school is empty.",
        })
        .await
        .unwrap();
    eprintln!("{p:#?}");
    assert!(p.actions.iter().any(|a| a["type"] == "EvacuateZone"));
    assert!(!p.messages.is_empty());
}
