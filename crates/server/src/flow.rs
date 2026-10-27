//! LLM orchestration of the turn loop.
//!
//! Pattern for every workflow: take what the prompt needs **under the lock**,
//! release it, await the model, then take the lock again to store the result.
//! The exercise lock is never held across an `.await`, so the renderer and
//! other requests are never blocked by a slow model.
//!
//! Every workflow has a no-LLM fallback: English text, or a proposal left for
//! the EXCON to fill in by hand.

#[cfg(target_arch = "wasm32")]
use crate::browser_runtime as tokio;

use std::collections::BTreeMap;

use anyhow::Result;
use exercise::{MsgKind, ParsedInput, MASTER};
use futures::future::join_all;

use crate::AppState;

/// A role's conversation, in the LLM's message format.
fn history(ex: &exercise::Exercise, key: &str) -> Vec<llm::ChatMessage> {
    ex.conversation(key)
        .into_iter()
        .map(|t| llm::ChatMessage {
            role: t.role,
            content: t.content,
        })
        .collect()
}

/// What goes back into the conversation as the AI's turn: its interpretation, compactly.
fn assistant_turn(p: &llm::ParsedReply) -> String {
    serde_json::json!({"actions": p.actions, "messages": p.messages, "injects": p.injects, "clarification": p.clarification, "summary": p.summary}).to_string()
}

/// Translate `text_en` for each recipient; English on failure.
pub async fn translations(
    state: &AppState,
    recipients: &[String],
    text_en: &str,
) -> BTreeMap<String, String> {
    let langs: Vec<(String, String)> = {
        let ex = state.lock();
        recipients
            .iter()
            .map(|r| (r.clone(), ex.language_of(r)))
            .collect()
    };
    let futs = langs.iter().map(|(r, lang)| {
        let llm = state.llm.clone();
        let (r, lang, text) = (r.clone(), lang.clone(), text_en.to_string());
        async move {
            let t = if llm.is_available() {
                llm.translate(&text, &lang)
                    .await
                    .unwrap_or_else(|_| text.clone())
            } else {
                text.clone()
            };
            (r, t)
        }
    });
    join_all(futs).await.into_iter().collect()
}

/// Send an inject to recipients (role ids or "all"), translated per player.
pub async fn send_inject(
    state: &AppState,
    inject_id: Option<&str>,
    title: &str,
    sender: &str,
    recipients: &[String],
    text_en: &str,
    actions: &exercise::actions::SendActions,
) -> Result<Vec<u64>> {
    let to = state.lock().recipients(recipients)?;
    let tr = translations(state, &to, text_en).await;
    let ids = state
        .lock()
        .send_inject_with(inject_id, title, sender, &to, text_en, &tr, actions)?;
    Ok(ids)
}

/// Parse a player reply into a proposal (runs the LLM; call it spawned).
pub async fn process_reply(state: AppState, pid: u64) {
    let prep = {
        let ex = state.lock();
        let Some(p) = ex.proposal(pid) else { return };
        let role_id = p.source.label().to_string();
        let Some(role) = ex.world.scn.role(&role_id).cloned() else {
            return;
        };
        let others: Vec<(String, String)> = ex
            .world
            .scn
            .roles
            .iter()
            .filter(|r| r.id != role_id)
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect();
        (
            role,
            ex.language_of(&role_id),
            sim::catalogue_for(
                &ex.world.scn,
                &ex.world
                    .scn
                    .role(&role_id)
                    .map(|r| r.allowed_actions.clone())
                    .unwrap_or_default(),
            ),
            others,
            ex.world.gazetteer(),
            serde_json::json!({"chat_recipient":ex.messages.iter().find(|m|m.proposal==Some(pid) && m.kind==MsgKind::Chat).map(|m|&m.to),"clock":ex.world.clock(),"map":ex.player_map(&role_id).unwrap_or_default(),"resources":ex.world.d.pools.get(&role_id),"deployments":ex.world.d.tasks.iter().filter(|t|t.role==role_id).collect::<Vec<_>>()}),
            history(&ex, &role_id),
            p.original.clone(),
        )
    };
    let (role, lang, catalogue, others, gazetteer, digest, history, reply) = prep;
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
            history: &history,
            reply: &reply,
        })
        .await;
    {
        // The reply joins the conversation whatever happened to the parse.
        let mut ex = state.lock();
        if ex.proposal(pid).map(|p| p.status) != Some(exercise::ProposalStatus::Parsing) {
            return;
        }
        let clock = ex.world.clock();
        ex.converse(
            &role.id,
            "user",
            format!("[{clock}] PLAYER REPLY:\n{reply}"),
        );
        if let Ok(parsed) = &result {
            ex.converse(&role.id, "assistant", assistant_turn(parsed));
        }
    }
    match result {
        Ok(parsed) => {
            let is_chat = state
                .lock()
                .messages
                .iter()
                .any(|m| m.proposal == Some(pid) && m.kind == MsgKind::Chat);
            let communication_drafts = parsed
                .messages
                .iter()
                .filter(|_| !is_chat)
                .filter(|m| m.to != role.id && state.lock().players.contains_key(&m.to))
                .map(|m| exercise::CommunicationDraft {
                    to: m.to.clone(),
                    from: role.id.clone(),
                    kind: MsgKind::RoleMessage,
                    title: format!("Message from {}", role.name),
                    text_en: m.text.clone(),
                })
                .collect();
            let input = ParsedInput {
                resource_limit_draft: None,
                communication_drafts,
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
        }
        Err(e) => {
            let _ = state.lock().parse_failed(pid, &e.to_string());
        }
    }
}

/// Interpret a EXCON prompt (runs the LLM; call it spawned).
pub async fn process_master_prompt(state: AppState, pid: u64) {
    let (catalogue, roles, gazetteer, digest, history, prompt) = {
        let ex = state.lock();
        let all: Vec<String> = sim::ALL_KINDS.iter().map(|s| s.to_string()).collect();
        (
            sim::catalogue_for(&ex.world.scn, &all),
            ex.world
                .scn
                .roles
                .iter()
                .map(|r| (r.id.clone(), r.name.clone()))
                .collect::<Vec<_>>(),
            ex.world.gazetteer(),
            serde_json::json!({"training_design":ex.design_view(),"recorded_decisions":ex.proposals.iter().rev().take(100).collect::<Vec<_>>(),"deployments":ex.world.deployment_view(),"travel_settings":ex.world.d.travel_settings,"world":ex.world.digest(None),"exercise_phase":ex.exercise_phase,"resource_limits":ex.resource_limits,"resource_accounting":ex.resource_accounting(None)}),
            history(&ex, exercise::MASTER),
            ex.proposal(pid)
                .map(|p| p.original.clone())
                .unwrap_or_default(),
        )
    };
    let r = state
        .llm
        .master_prompt(llm::tasks::MasterInput {
            catalogue: &catalogue,
            roles: &roles,
            gazetteer: &gazetteer,
            digest: &digest,
            history: &history,
            prompt: &prompt,
        })
        .await;
    let mut ex = state.lock();
    let clock = ex.world.clock();
    ex.converse(
        exercise::MASTER,
        "user",
        format!("[{clock}] EXCON:\n{prompt}"),
    );
    if let Ok(p) = &r {
        ex.converse(exercise::MASTER, "assistant", assistant_turn(p));
    }
    match r {
        Ok(p) => {
            let drafts = p
                .injects
                .iter()
                .filter_map(|d| serde_json::to_value(d).ok())
                .collect();
            let input = ParsedInput {
                resource_limit_draft: p.resource_limits,
                communication_drafts: Vec::new(),
                language: "en".into(),
                translation_en: if p.translation_en.is_empty() {
                    prompt.clone()
                } else {
                    p.translation_en
                },
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

/// Feedback to a player after a EXCON decision (translated, or English).
pub async fn send_feedback(state: AppState, role: String, pid: u64, facts_en: String) {
    let lang = state.lock().language_of(&role);
    let text = if state.llm.is_available() {
        state
            .llm
            .translate(&facts_en, &lang)
            .await
            .unwrap_or_else(|_| facts_en.clone())
    } else {
        facts_en.clone()
    };
    state.lock().add_message(
        MASTER,
        &role,
        MsgKind::Feedback,
        Some("Orders reviewed"),
        &text,
        &facts_en,
        Some(pid),
    );
}

/// Translate and deliver the EXCON's clarification question to the player.
pub async fn send_clarification(state: AppState, role: String, pid: u64, text_en: String) {
    let lang = state.lock().language_of(&role);
    let text = if state.llm.is_available() {
        state
            .llm
            .translate(&text_en, &lang)
            .await
            .unwrap_or_else(|_| text_en.clone())
    } else {
        text_en.clone()
    };
    let _ = state.lock().deliver_clarification(pid, &text, &text_en);
}

/// Claim and deliver communications once, only after the reviewed proposal executes.
pub async fn send_drafts(state: &AppState, pid: u64) {
    let (drafts, injects) = {
        let mut ex = state.lock();
        let Some(p) = ex.proposals.iter_mut().find(|p| p.id == pid) else {
            return;
        };
        if p.status != exercise::ProposalStatus::Applied
            || p.decision.as_ref().map(|d| d.approve) != Some(true)
            || p.communications_claimed
        {
            return;
        }
        p.communications_claimed = true;
        (p.communication_drafts.clone(), p.inject_drafts.clone())
    };
    for (index, d) in drafts.into_iter().enumerate() {
        let tr = translations(state, std::slice::from_ref(&d.to), &d.text_en).await;
        let text = tr.get(&d.to).cloned().unwrap_or_else(|| d.text_en.clone());
        let mut ex = state.lock();
        if ex.phase == exercise::Phase::Debrief
            || ex.proposal(pid).map(|p| p.status) != Some(exercise::ProposalStatus::Applied)
        {
            return;
        }
        if let Err(e) = ex.deliver_approved_communication(pid, index, &text) {
            ex.record(
                "approved_communication_failed",
                MASTER,
                serde_json::json!({"proposal": pid, "draft": index, "error": e.to_string()}),
            );
        }
    }
    for d in injects {
        let Ok(d) = serde_json::from_value::<llm::InjectDraft>(d) else {
            continue;
        };
        let recipients = if d.recipients.is_empty() {
            vec!["all".to_string()]
        } else {
            d.recipients.clone()
        };
        let Ok(to) = state.lock().recipients(&recipients) else {
            continue;
        };
        let tr = translations(state, &to, &d.text).await;
        let mut ex = state.lock();
        if ex.proposal(pid).map(|p| p.status) != Some(exercise::ProposalStatus::Applied) {
            return;
        }
        match ex.send_inject(
            None,
            &d.title,
            if d.sender.is_empty() {
                "EXCON"
            } else {
                &d.sender
            },
            &to,
            &d.text,
            &tr,
        ) {
            Ok(ids) => ex.record(
                "approved_inject_delivered",
                MASTER,
                serde_json::json!({"proposal": pid, "messages": ids}),
            ),
            Err(e) => ex.record(
                "approved_inject_failed",
                MASTER,
                serde_json::json!({"proposal": pid, "error": e.to_string()}),
            ),
        }
    }
}

/// End-of-turn situation update for every player.
pub async fn situation_updates(state: AppState, _events_en: Vec<String>) {
    let per_role = {
        let ex = state.lock();
        ex.players.values().map(|p| (p.role.clone(), p.language.clone(), format!("{} — Turn resolved. Your resource ledger and reviewed orders are updated. Map reports retain their observation time; await new information from EXCON.", ex.world.clock()))).collect::<Vec<_>>()
    };
    for (role, lang, facts) in per_role {
        let text = if state.llm.is_available() {
            state
                .llm
                .translate(&facts, &lang)
                .await
                .unwrap_or_else(|_| facts.clone())
        } else {
            facts.clone()
        };
        state.lock().add_message(
            MASTER,
            &role,
            MsgKind::Situation,
            Some("Turn status"),
            &text,
            &facts,
            None,
        );
    }
    let summaries = state.lock().claim_turn_summaries();
    futures::future::join_all(summaries.into_iter().map(|(id, lang, evidence)| {
        let state = state.clone();
        async move {
            let result = if state.llm.is_available() {
                state.llm.player_turn_summary(&lang, &evidence).await.ok()
            } else {
                None
            };
            state.lock().complete_turn_summary(id, result);
        }
    }))
    .await;
}

/// Simulate the resolving turn one simulated second at a time. The lock is
/// taken per step, so the renderer redraws continuously and the WebSocket feed
/// (throttled to 4 Hz) tells browsers to re-fetch.
///
/// `speed` is simulated seconds per wall-clock second: the loop keeps the
/// simulation clock on that schedule (re-anchoring if the EXCON changes
/// it mid-turn). 0 runs as fast as the model computes (~0.4 s for a
/// 30-minute turn with the default 32-run ensemble).
pub async fn run_resolution(state: AppState) -> Result<exercise::TurnReport> {
    use web_time::{Duration, Instant};
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
        #[cfg(target_arch = "wasm32")]
        let frame_started = Instant::now();
        while !done && state.lock().world.time_s() < target {
            done = state.lock().step_resolution(exercise::STEP_S)?;
            #[cfg(target_arch = "wasm32")]
            if frame_started.elapsed().as_millis() >= 12 {
                break;
            }
        }
        if done {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let report = state.lock().finish_resolution()?;
    Ok(report)
}

/// Claim scheduled deliveries under the lock, then translate independently. Actual
/// simulated/wall delivery times are retained rather than pretending translation is instant.
pub fn dispatch_due(state: &AppState) {
    let due = state.lock().claim_due_injects();
    for (id, template) in due {
        let state = state.clone();
        tokio::spawn(async move {
            let recipients = { state.lock().recipients(&template.recipients) };
            let to = match recipients {
                Ok(to) => to,
                Err(e) => {
                    state.lock().complete_schedule(id, Err(e));
                    return;
                }
            };
            let tr = translations(&state, &to, &template.text).await;
            let mut ex = state.lock();
            if ex
                .scheduled_injects
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.status)
                != Some(exercise::ScheduleStatus::Dispatching)
            {
                return;
            }
            let result = ex.send_inject_with(
                Some(&template.id),
                &template.title,
                &template.sender,
                &to,
                &template.text,
                &tr,
                &exercise::actions::SendActions::all(),
            );
            ex.complete_schedule(id, result);
        });
    }
}

/// Set while a saved session is replayed: the wall-clock pacing below stands
/// down, because [`advance_to`] reproduces the recorded simulated time instead.
pub static REPLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Advance an open turn's clock to `target` (simulated seconds) in the same
/// steps as [`run_clock`], delivering due scheduled injects on the way. The
/// clock must be running; otherwise nothing moves.
pub async fn advance_to(state: &AppState, target: i64) {
    loop {
        dispatch_due(state);
        let step = {
            let mut ex = state.lock();
            let now = ex.world.time_s();
            if ex.phase != exercise::Phase::Orders || !ex.clock_control.running || now >= target {
                None
            } else {
                let next_due = ex
                    .scheduled_injects
                    .iter()
                    .filter(|i| i.status == exercise::ScheduleStatus::Pending && i.due_s > now)
                    .map(|i| i.due_s)
                    .min()
                    .unwrap_or(target);
                let dt = (target - now).min(next_due - now).min(60);
                (dt > 0).then(|| ex.advance_clock(dt))
            }
        };
        if !matches!(step, Some(Ok(_))) {
            break;
        }
        tokio::task::yield_now().await;
    }
    settle(state).await;
}

/// Wait until spawned follow-ups (deliveries, situation updates) stop changing
/// the exercise.
pub async fn settle(state: &AppState) {
    let mut stable = 0;
    let mut last = state.lock().generation;
    for _ in 0..2000 {
        tokio::task::yield_now().await;
        let (generation, busy) = {
            let ex = state.lock();
            (
                ex.generation,
                ex.phase == exercise::Phase::Resolving
                    || ex
                        .scheduled_injects
                        .iter()
                        .any(|i| i.status == exercise::ScheduleStatus::Dispatching),
            )
        };
        if generation == last && !busy {
            stable += 1;
            if stable >= 8 {
                break;
            }
        } else {
            stable = 0;
            last = generation;
        }
    }
}

/// Wall-time pacing while a decision turn stays open. Revision changes (pause,
/// ratio edits, explicit time jumps and rollback) always reanchor at the actual world.
pub async fn run_clock(state: AppState) {
    use web_time::Instant;
    let mut anchor: Option<(u64, Instant, i64)> = None;
    loop {
        dispatch_due(&state);
        let control = {
            let ex = state.lock();
            (ex.clock_control.clone(), ex.phase, ex.world.time_s())
        };
        if !control.0.running
            || control.1 != exercise::Phase::Orders
            || REPLAYING.load(std::sync::atomic::Ordering::Relaxed)
        {
            anchor = None;
        } else {
            if anchor.is_none_or(|(revision, _, _)| revision != control.0.revision) {
                anchor = Some((control.0.revision, Instant::now(), control.2));
            }
            let (revision, start, sim_start) = anchor.unwrap();
            let target = sim_start + (start.elapsed().as_secs_f64() * control.0.ratio) as i64;
            for _ in 0..10 {
                let result = {
                    let mut ex = state.lock();
                    if !ex.clock_control.running
                        || ex.clock_control.revision != revision
                        || ex.phase != exercise::Phase::Orders
                    {
                        break;
                    }
                    let now = ex.world.time_s();
                    let next_due = ex
                        .scheduled_injects
                        .iter()
                        .filter(|i| i.status == exercise::ScheduleStatus::Pending && i.due_s > now)
                        .map(|i| i.due_s)
                        .min()
                        .unwrap_or(target);
                    let dt = (target - now).min(next_due - now).min(60);
                    if dt <= 0 {
                        break;
                    }
                    ex.advance_clock(dt)
                };
                if let Err(e) = result {
                    let mut ex = state.lock();
                    let ratio = ex.clock_control.ratio;
                    let _ = ex.configure_clock(false, ratio, "Clock paused after simulation error");
                    ex.record(
                        "clock_failed",
                        MASTER,
                        serde_json::json!({"error":e.to_string()}),
                    );
                    break;
                }
                dispatch_due(&state);
                tokio::task::yield_now().await;
            }
            let mut ex = state.lock();
            if ex.clock_control.revision == revision {
                ex.clock_control.lag_s = (target - ex.world.time_s()).max(0);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
