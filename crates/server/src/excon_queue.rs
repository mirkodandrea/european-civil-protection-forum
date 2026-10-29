//! EXCON actions on the world: applied or queued for play
//! ([`exercise::queue`]), and the queue's edit/cancel routes.
use super::*;
use exercise::queue::{ExconOutcome, ExconWhen};

/// Seconds from the scenario start of an `HH:MM[:SS]` clock on Day 1.
pub(crate) fn clock_s(clock: &str, start_clock_s: i64) -> Result<i64, ApiError> {
    let parts = clock
        .split(':')
        .map(str::parse::<i64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| bad("at_clock must be HH:MM or HH:MM:SS on Day 1"))?;
    if !(2..=3).contains(&parts.len())
        || !(0..=23).contains(&parts[0])
        || !(0..=59).contains(&parts[1])
        || parts.get(2).is_some_and(|s| !(0..=59).contains(s))
    {
        return Err(bad("at_clock must be HH:MM or HH:MM:SS on Day 1"));
    }
    Ok(parts[0] * 3600 + parts[1] * 60 + parts.get(2).copied().unwrap_or(0) - start_clock_s)
}

/// When, from a request body: `at_clock` (`"13:00"`), `at_s`, or `when`
/// (`"play"`, the default, or `"now"`).
pub(crate) fn when_of(body: &Value, start_clock_s: i64) -> Result<Option<ExconWhen>, ApiError> {
    if let Some(c) = body["at_clock"].as_str() {
        return Ok(Some(ExconWhen::At(clock_s(c, start_clock_s)?)));
    }
    if let Some(t) = body["at_s"].as_i64() {
        return Ok(Some(ExconWhen::At(t)));
    }
    match body["when"].as_str() {
        None => Ok(None),
        Some("play") => Ok(Some(ExconWhen::Play)),
        Some("now") => Ok(Some(ExconWhen::Now)),
        Some(w) => Err(bad(format!("unknown when {w:?}: use play, now, at_s or at_clock"))),
    }
}

/// One result row of `/api/master/command` and `/api/master/preview/apply`.
pub(crate) fn result_row(cmd: &Command, r: anyhow::Result<ExconOutcome>) -> Value {
    match r {
        Ok(ExconOutcome::Applied { notes, .. }) => {
            json!({"command": cmd.describe(), "ok": true, "notes": notes})
        }
        Ok(ExconOutcome::Queued { id, run_at_s, run_at_clock, .. }) => json!({
            "command": cmd.describe(), "ok": true, "queued": id,
            "run_at_s": run_at_s, "run_at_clock": run_at_clock,
            "notes": [format!("queued for {run_at_clock}")]}),
        Err(e) => json!({"command": cmd.describe(), "ok": false, "error": e.to_string()}),
    }
}

#[derive(Deserialize)]
pub(crate) struct CancelBody {
    #[serde(default)]
    reason: String,
}
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/excon-actions/<id>/cancel", data = "<b>")
)]
pub(crate) async fn cancel(_principal: Principal, s: AppState, id: u64, b: Json<CancelBody>) -> ApiResult {
    let Json(b) = b;
    s.lock().cancel_excon_action(id, &b.reason)?;
    Ok(Json(json!({"ok": true})))
}

/// Body: optional `command`, and the new instant (`at_clock`, `at_s` or
/// `when: "play"`).
#[cfg_attr(
    not(target_arch = "wasm32"),
    rocket::post("/api/master/excon-actions/<id>/edit", data = "<b>")
)]
pub(crate) async fn edit(_principal: Principal, s: AppState, id: u64, b: Json<Value>) -> ApiResult {
    let Json(b) = b;
    let command = match b.get("command") {
        Some(v) if !v.is_null() => Some(
            serde_json::from_value::<Command>(v.clone()).map_err(|e| bad(format!("invalid command: {e}")))?,
        ),
        _ => None,
    };
    let mut ex = s.lock();
    let when = when_of(&b, ex.world.scn.start_clock_s)?;
    ex.edit_excon_action(id, command, when)?;
    Ok(Json(json!({"ok": true})))
}
