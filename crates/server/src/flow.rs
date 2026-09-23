//! LLM orchestration of the turn loop.
//!
//! Pattern for every workflow: take what the prompt needs **under the lock**,
//! release it, await the model, then take the lock again to store the result.
//! The exercise lock is never held across an `.await`, so the renderer and
//! other requests are never blocked by a slow model.
//!
//! Every workflow has a no-LLM fallback: English text, or a proposal left for
//! the Master to fill in by hand.

use std::collections::BTreeMap;

use anyhow::Result;
use exercise::{MsgKind, ParsedInput, MASTER};
use futures::future::join_all;
use serde_json::Value;

use crate::AppState;

/// Translate `text_en` for each recipient; English on failure.
pub async fn translations(state: &AppState, recipients: &[String], text_en: &str) -> BTreeMap<String, String> {
    let langs: Vec<(String, String)> = {
        let ex = state.lock();
        recipients.iter().map(|r| (r.clone(), ex.language_of(r))).collect()
    };
    let futs = langs.iter().map(|(r, lang)| {
        let llm = state.llm.clone();
        let (r, lang, text) = (r.clone(), lang.clone(), text_en.to_string());
        async move {
            let t = if llm.is_available() { llm.translate(&text, &lang).await.unwrap_or_else(|_| text.clone()) } else { text.clone() };
            (r, t)
        }
    });
    join_all(futs).await.into_iter().collect()
}

/// Send an inject to recipients (role ids or "all"), translated per player.
pub async fn send_inject(state: &AppState, inject_id: Option<&str>, title: &str, sender: &str, recipients: &[String], text_en: &str) -> Result<Vec<u64>> {
    let to = state.lock().recipients(recipients)?;
    let tr = translations(state, &to, text_en).await;
    let ids = state.lock().send_inject(inject_id, title, sender, &to, text_en, &tr)?;
    Ok(ids)
}

/// Parse a player reply into a proposal (runs the LLM; call it spawned).
pub async fn process_reply(state: AppState, pid: u64) {
    let prep = {
        let ex = state.lock();
        let Some(p) = ex.proposal(pid) else { return };
        let role_id = p.source.label().to_string();
        let Some(role) = ex.world.scn.role(&role_id).cloned() else { return };
        let others: Vec<(String, String)> = ex.world.scn.roles.iter().filter(|r| r.id != role_id).map(|r| (r.id.clone(), r.name.clone())).collect();
        (
            role,
            ex.language_of(&role_id),
            sim::catalogue(&ex.world.scn.role(&role_id).map(|r| r.allowed_actions.clone()).unwrap_or_default()),
            others,
            ex.world.gazetteer(),
            ex.world.digest(Some(&role_id)),
            ex.recent_received_en(&role_id, 4),
            p.original.clone(),
        )
    };
    let (role, lang, catalogue, others, gazetteer, digest, recent, reply) = prep;
    let result = state
        .llm
        .parse_reply(llm::tasks::ReplyInput {
            role_id: &role.id,
            role_name: &role.name,
            role_description: &role.description,
            language: &lang,
            catalogue: &catalogue,
            other_roles: &others,
            gazetteer: &gazetteer,
            digest: &digest,
            recent_injects: &recent,
            reply: &reply,
        })
        .await;
    match result {
        Ok(parsed) => {
            let messages = parsed.messages.clone();
            let input = ParsedInput {
                language: parsed.detected_language,
                translation_en: parsed.translation_en,
                actions: parsed.actions,
                clarification: parsed.clarification,
                summary: parsed.summary,
                inject_drafts: Vec::new(),
            };
            if let Err(e) = state.lock().attach_parse(pid, input) {
                tracing::warn!("attach_parse {pid}: {e}");
            }
            // Messages to other roles: delivered at once, translated.
            for m in messages {
                let valid = state.lock().players.contains_key(&m.to);
                if !valid || m.to == role.id {
                    continue;
                }
                let tr = translations(&state, std::slice::from_ref(&m.to), &m.text).await;
                let text = tr.get(&m.to).cloned().unwrap_or_else(|| m.text.clone());
                let title = format!("Message from {}", role.name);
                state.lock().add_message(&role.id, &m.to, MsgKind::RoleMessage, Some(&title), &text, &m.text, Some(pid));
            }
        }
        Err(e) => {
            let _ = state.lock().parse_failed(pid, &e.to_string());
        }
    }
}

/// Interpret a Master prompt (runs the LLM; call it spawned).
pub async fn process_master_prompt(state: AppState, pid: u64) {
    let (catalogue, roles, gazetteer, digest, prompt) = {
        let ex = state.lock();
        let all: Vec<String> = sim::ALL_KINDS.iter().map(|s| s.to_string()).collect();
        (
            sim::catalogue(&all),
            ex.world.scn.roles.iter().map(|r| (r.id.clone(), r.name.clone())).collect::<Vec<_>>(),
            ex.world.gazetteer(),
            ex.world.digest(None),
            ex.proposal(pid).map(|p| p.original.clone()).unwrap_or_default(),
        )
    };
    let r = state.llm.master_prompt(llm::tasks::MasterInput { catalogue: &catalogue, roles: &roles, gazetteer: &gazetteer, digest: &digest, prompt: &prompt }).await;
    let mut ex = state.lock();
    match r {
        Ok(p) => {
            let drafts = p.injects.iter().filter_map(|d| serde_json::to_value(d).ok()).collect();
            let input = ParsedInput {
                language: "en".into(),
                translation_en: if p.translation_en.is_empty() { prompt.clone() } else { p.translation_en },
                actions: p.actions,
                clarification: p.clarification,
                summary: p.summary,
                inject_drafts: drafts,
            };
            if let Err(e) = ex.attach_parse(pid, input) {
                tracing::warn!("attach_parse {pid}: {e}");
            }
        }
        Err(e) => {
            let _ = ex.parse_failed(pid, &e.to_string());
        }
    }
}

/// Feedback to a player after a Master decision (translated, or English).
pub async fn send_feedback(state: AppState, role: String, pid: u64, facts_en: String) {
    let (name, lang) = {
        let ex = state.lock();
        (ex.players.get(&role).map(|p| p.name.clone()).unwrap_or_default(), ex.language_of(&role))
    };
    let text = if state.llm.is_available() {
        state.llm.feedback(&name, &lang, &facts_en).await.unwrap_or_else(|_| facts_en.clone())
    } else {
        facts_en.clone()
    };
    state.lock().add_message(MASTER, &role, MsgKind::Feedback, Some("Orders reviewed"), &text, &facts_en, Some(pid));
}

/// Send the inject drafts of an approved Master proposal.
pub async fn send_drafts(state: &AppState, drafts: &[Value]) {
    for d in drafts {
        let Ok(d) = serde_json::from_value::<llm::InjectDraft>(d.clone()) else { continue };
        let recipients = if d.recipients.is_empty() { vec!["all".to_string()] } else { d.recipients.clone() };
        let sender = if d.sender.is_empty() { "EXCON".to_string() } else { d.sender.clone() };
        if let Err(e) = send_inject(state, None, &d.title, &sender, &recipients, &d.text).await {
            tracing::warn!("sending drafted inject: {e}");
        }
    }
}

/// End-of-turn situation update for every player.
pub async fn situation_updates(state: AppState, events_en: Vec<String>) {
    let per_role: Vec<(String, String, String, Value)> = {
        let ex = state.lock();
        ex.players.values().map(|p| (p.role.clone(), p.name.clone(), p.language.clone(), ex.world.digest(Some(&p.role)))).collect()
    };
    let futs = per_role.into_iter().map(|(role, name, lang, digest)| {
        let llm = state.llm.clone();
        let events = events_en.clone();
        async move {
            let fallback = fallback_update(&digest, &events);
            let text = if llm.is_available() { llm.situation_update(&name, &lang, &digest, &events).await.unwrap_or_else(|_| fallback.clone()) } else { fallback.clone() };
            (role, text, fallback)
        }
    });
    for (role, text, en) in join_all(futs).await {
        let title = format!("Situation update – {}", state.lock().world.clock());
        state.lock().add_message(MASTER, &role, MsgKind::Situation, Some(&title), &text, &en, None);
    }
}

fn fallback_update(digest: &Value, events: &[String]) -> String {
    let mut s = format!(
        "{} — burned area {} ha, fire in zones {}.",
        digest["clock"].as_str().unwrap_or(""),
        digest["fire"]["burned_ha"],
        digest["fire"]["burning_zones"].as_array().map(|a| a.iter().filter_map(|z| z.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    );
    for e in events.iter().take(12) {
        s.push_str("\n• ");
        s.push_str(e);
    }
    s
}

/// Simulate the resolving turn one simulated second at a time. The lock is
/// taken per step, so the renderer redraws continuously and the WebSocket feed
/// (throttled to 4 Hz) tells browsers to re-fetch.
///
/// `speed` is simulated seconds per wall-clock second: the loop keeps the
/// simulation clock on that schedule (re-anchoring if the Director changes
/// it mid-turn). 0 runs as fast as the model computes (~0.4 s for a
/// 30-minute turn with the default 32-run ensemble).
pub async fn run_resolution(state: AppState) -> Result<exercise::TurnReport> {
    use std::time::{Duration, Instant};
    let mut anchor: Option<(Instant, i64, f32)> = None;
    loop {
        let (sim_now, speed) = {
            let ex = state.lock();
            (ex.world.time_s(), ex.speed)
        };
        if speed <= 0.0 {
            anchor = None;
            let done = state.lock().step_resolution(exercise::STEP_S)?;
            if done {
                break;
            }
            tokio::task::yield_now().await;
            continue;
        }
        let (t0, s0, _) = *anchor.get_or_insert((Instant::now(), sim_now, speed));
        if anchor.map(|a| a.2) != Some(speed) {
            anchor = Some((Instant::now(), sim_now, speed));
            continue;
        }
        let target = s0 + (t0.elapsed().as_secs_f64() * speed as f64) as i64;
        let mut done = false;
        while !done && state.lock().world.time_s() < target {
            done = state.lock().step_resolution(exercise::STEP_S)?;
        }
        if done {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let report = state.lock().finish_resolution()?;
    Ok(report)
}
