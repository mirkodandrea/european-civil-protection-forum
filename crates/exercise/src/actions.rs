//! Scripted timeline actions: world changes planned in the scenario that are
//! independent of any inject. EXCON is asked about the ones falling inside a
//! turn's window when the turn is resolved; approved ones are applied, as EXCON
//! commands, at the planned instant during resolution.
use crate::{Exercise, MASTER};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::Command;
use std::collections::BTreeMap;

/// What EXCON decides for a due action when resolving a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionChoice {
    /// Apply it at its planned instant.
    Run,
    /// Never apply it.
    Skip,
    /// Leave it for a later turn.
    Defer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    /// Approved, waiting for its instant inside the running turn.
    Approved,
    Done,
    Failed,
    Skipped,
    Deferred,
}

/// The decision taken on one action and what came of it. Dropped by a rollback
/// to a time before the decision, which makes the action pending again.
#[derive(Debug, Clone, Serialize)]
pub struct ActionRun {
    pub status: ActionStatus,
    /// Simulated time when EXCON decided.
    pub decided_s: i64,
    /// Instant at which an approved action is applied.
    pub run_at_s: Option<i64>,
    pub executed_s: Option<i64>,
    pub result: Option<String>,
}

impl Exercise {
    fn action_pending(&self, id: &str) -> bool {
        self.action_runs
            .get(id)
            .is_none_or(|r| r.status == ActionStatus::Deferred)
    }

    /// Every timeline row with its current state, chronological.
    pub fn timeline_actions(&self) -> Vec<Value> {
        let start = self.world.scn.start_clock_s;
        let mut rows: Vec<&scenario::TimelineAction> = self.world.scn.timeline.iter().collect();
        rows.sort_by_key(|a| a.at_s);
        rows.into_iter()
            .map(|a| {
                let run = self.action_runs.get(&a.id);
                json!({
                    "id": a.id, "title": a.title, "note": a.note, "after": a.after,
                    "at_s": a.at_s, "clock": scenario::format_clock(start + a.at_s),
                    "command": a.command,
                    "summary": serde_json::from_value::<Command>(a.command.clone())
                        .map(|c| c.describe()).unwrap_or_else(|e| format!("invalid command: {e}")),
                    "status": run.map(|r| r.status).unwrap_or(ActionStatus::Deferred),
                    "pending": self.action_pending(&a.id),
                    "run": run,
                })
            })
            .collect()
    }

    /// Pending actions planned before the end of a turn of `minutes` that
    /// starts now (overdue ones included), chronological.
    pub fn due_actions(&self, minutes: Option<u32>) -> Vec<Value> {
        let minutes = minutes.unwrap_or(self.turn_minutes).clamp(1, 24 * 60);
        let to_s = self.world.time_s() + minutes as i64 * 60;
        self.timeline_actions()
            .into_iter()
            .filter(|a| a["pending"] == true && a["at_s"].as_i64().is_some_and(|t| t < to_s))
            .collect()
    }

    /// Take EXCON's decisions for the due actions of the turn about to be
    /// resolved. Fails, changing nothing, while any due action is undecided.
    pub(crate) fn decide_actions(
        &mut self,
        minutes: u32,
        choices: &BTreeMap<String, ActionChoice>,
    ) -> Result<()> {
        let due = self.due_actions(Some(minutes));
        let ids: Vec<String> = due
            .iter()
            .filter_map(|a| a["id"].as_str().map(String::from))
            .collect();
        if let Some(unknown) = choices
            .keys()
            .find(|k| !self.world.scn.timeline.iter().any(|a| &a.id == *k))
        {
            bail!("unknown timeline action {unknown}");
        }
        let undecided: Vec<&String> = ids.iter().filter(|i| !choices.contains_key(*i)).collect();
        if !undecided.is_empty() {
            bail!(
                "scheduled actions need a decision before the turn starts: {}",
                undecided
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let now = self.world.time_s();
        for id in ids {
            let choice = choices[&id];
            let at_s = self
                .world
                .scn
                .timeline
                .iter()
                .find(|a| a.id == id)
                .map_or(now, |a| a.at_s);
            let (status, run_at_s) = match choice {
                ActionChoice::Run => (ActionStatus::Approved, Some(at_s.max(now))),
                ActionChoice::Skip => (ActionStatus::Skipped, None),
                ActionChoice::Defer => (ActionStatus::Deferred, None),
            };
            self.action_runs.insert(
                id.clone(),
                ActionRun {
                    status,
                    decided_s: now,
                    run_at_s,
                    executed_s: None,
                    result: None,
                },
            );
            self.record(
                "timeline_action_decided",
                MASTER,
                json!({"action": id, "choice": choice, "run_at_s": run_at_s}),
            );
        }
        Ok(())
    }

    /// Apply approved actions whose instant has been reached.
    pub(crate) fn run_approved_actions(&mut self) {
        let now = self.world.time_s();
        let mut due: Vec<(i64, String)> = self
            .action_runs
            .iter()
            .filter(|(_, r)| {
                r.status == ActionStatus::Approved && r.run_at_s.is_some_and(|t| t <= now)
            })
            .map(|(id, r)| (r.run_at_s.unwrap_or(now), id.clone()))
            .collect();
        due.sort();
        for (_, id) in due {
            let Some(action) = self.world.scn.timeline.iter().find(|a| a.id == id).cloned() else {
                continue;
            };
            let outcome = serde_json::from_value::<Command>(action.command.clone())
                .map_err(|e| anyhow::anyhow!("invalid command: {e}"))
                .and_then(|cmd| self.direct_command(&cmd));
            let (status, result) = match outcome {
                Ok(notes) => (ActionStatus::Done, notes.join("; ")),
                Err(e) => (ActionStatus::Failed, e.to_string()),
            };
            if let Some(r) = self.action_runs.get_mut(&id) {
                r.status = status;
                r.executed_s = Some(now);
                r.result = Some(result.clone());
            }
            self.record(
                "timeline_action_executed",
                MASTER,
                json!({"action": id, "status": status, "result": result, "t_s": now}),
            );
        }
    }

    /// Seconds until the next approved action, so a step can stop exactly there.
    pub(crate) fn next_action_in(&self) -> Option<i64> {
        let now = self.world.time_s();
        self.action_runs
            .values()
            .filter(|r| r.status == ActionStatus::Approved)
            .filter_map(|r| r.run_at_s)
            .filter(|t| *t > now)
            .map(|t| t - now)
            .min()
    }

    /// A rollback forgets the decisions taken from the restored time onwards.
    pub(crate) fn forget_actions_since_restore(&mut self) {
        let t = self.world.time_s();
        self.action_runs.retain(|_, r| r.decided_s < t);
    }
}
