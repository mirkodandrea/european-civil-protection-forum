//! EXCON actions that wait for play.
//!
//! While a turn is open for orders the players are reading injects and
//! answering: an EXCON world change taken then (an ignition, a closed road,
//! new weather…) must not change the situation under their hands. Every
//! EXCON action path goes through [`Exercise::apply_or_queue_excon`], which
//! queues it and applies it when time next moves: at the start of the next
//! advance (before the approved proposals) or at a chosen simulated instant,
//! independently of any inject (e.g. the ignition at 13:00 that a report
//! inject announces at 13:20).
//!
//! A queued action is an [`ActionRun`] with [`Queued`] metadata, so it shares
//! the inject actions' machinery: it runs at its instant during the stepped
//! resolution (step-size invariant), in the Situation preview fork exactly as
//! in the real resolution, and a rollback drops or re-arms it with the rest.
//!
//! Applied at once: everything before the exercise starts (setup), commands
//! that are time control or bookkeeping rather than a change of the situation
//! ([`immediate_by_nature`]), and an explicit [`ExconWhen::Now`].
use crate::actions::{ActionRun, ActionStatus};
use crate::{Exercise, Phase, MASTER};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::{Actor, Command, Verdict};

/// When an EXCON action should take effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExconWhen {
    /// When time next moves (the start of the next advance). Before the
    /// exercise starts, at once.
    #[default]
    Play,
    /// At a simulated instant (seconds from the scenario start), now or later.
    At(i64),
    /// At once: the explicit escape hatch for exercise control.
    Now,
}

/// Queue metadata of an [`ActionRun`].
#[derive(Debug, Clone, Serialize)]
pub struct Queued {
    pub id: u64,
    /// Where the action came from (Controllo tool, narrative outcome, …).
    pub origin: Value,
    /// The EXCON asked for "next play" rather than a fixed instant.
    pub on_play: bool,
    pub cancelled_s: Option<i64>,
    pub cancel_reason: Option<String>,
}

/// What [`Exercise::apply_or_queue_excon`] did.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExconOutcome {
    Applied { notes: Vec<String>, log_seq: u64 },
    Queued { id: u64, run_at_s: i64, run_at_clock: String, log_seq: u64 },
}

impl ExconOutcome {
    /// `seq` of the log entry that records the application or the queueing.
    pub fn log_seq(&self) -> u64 {
        match self {
            ExconOutcome::Applied { log_seq, .. } | ExconOutcome::Queued { log_seq, .. } => *log_seq,
        }
    }

    /// Readable notes for the API: the sim notes, or where it was queued.
    pub fn notes(&self) -> Vec<String> {
        match self {
            ExconOutcome::Applied { notes, .. } => notes.clone(),
            ExconOutcome::Queued { run_at_clock, .. } => vec![format!("queued for {run_at_clock}")],
        }
    }
}

/// Commands that take effect at once even while orders are open: time
/// control, and resource bookkeeping or configuration that decides what can
/// be approved during the pause rather than changing the situation players
/// are reading.
pub fn immediate_by_nature(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::AdvanceTime { .. }
            | Command::ReturnResource { .. }
            | Command::AdjustResource { .. }
            | Command::SetDeploymentTravel { .. }
    )
}

fn key(id: u64) -> String {
    format!("excon:{id}")
}

impl Exercise {
    /// The single entry point of EXCON actions on the world. Queues `cmd`
    /// while the exercise runs (applied when time next moves, or at the
    /// instant of [`ExconWhen::At`]); applies it now before the start, for
    /// [`immediate_by_nature`] commands and for [`ExconWhen::Now`]. `origin`
    /// (an object) is kept with the action and merged into its log entries.
    /// The command is checked against the present world when queued and
    /// again when it runs.
    pub fn apply_or_queue_excon(
        &mut self,
        cmd: &Command,
        when: ExconWhen,
        origin: Value,
    ) -> Result<ExconOutcome> {
        if self.phase == Phase::Debrief {
            bail!("the exercise has ended");
        }
        let immediate = match when {
            ExconWhen::Now => true,
            ExconWhen::Play => self.phase == Phase::Briefing || immediate_by_nature(cmd),
            ExconWhen::At(_) if immediate_by_nature(cmd) => {
                bail!("{} takes effect at once and cannot be scheduled", cmd.describe())
            }
            ExconWhen::At(_) => false,
        };
        if immediate {
            let (notes, log_seq) = self.direct_command_logged(cmd, origin)?;
            return Ok(ExconOutcome::Applied { notes, log_seq });
        }
        let now = self.world.time_s();
        let run_at_s = match when {
            ExconWhen::At(t) if t < now => bail!("the chosen time has already passed"),
            ExconWhen::At(t) if t > now + 7 * 86400 => bail!("the chosen time is more than seven simulated days ahead"),
            ExconWhen::At(t) => t,
            _ => now,
        };
        self.check_excon(cmd)?;
        let id = self.enqueue_excon(cmd, run_at_s, matches!(when, ExconWhen::Play), origin)?;
        let log_seq = self.log.last().map(|e| e.seq).unwrap_or(0);
        // A running within-turn clock or resolution picks it up at its next step.
        Ok(ExconOutcome::Queued {
            id,
            run_at_s,
            run_at_clock: scenario::format_clock(self.world.scn.start_clock_s + run_at_s),
            log_seq,
        })
    }

    /// Queue an already checked command at `run_at_s` and return its id.
    /// Callers check feasibility for the actor that will run it (the EXCON,
    /// or the role of a reviewed reply).
    pub(crate) fn enqueue_excon(&mut self, cmd: &Command, run_at_s: i64, on_play: bool, origin: Value) -> Result<u64> {
        let now = self.world.time_s();
        let id = self.id();
        let run = ActionRun {
            delivery: 0,
            action: key(id),
            inject: None,
            title: cmd.describe(),
            command: serde_json::to_value(cmd)?,
            reveal: None,
            sender: MASTER.into(),
            roles: Vec::new(),
            offset_s: 0,
            status: ActionStatus::Approved,
            decided_s: now,
            run_at_s: Some(run_at_s),
            executed_s: None,
            result: None,
            queued: Some(Queued {
                id,
                origin: origin.clone(),
                on_play,
                cancelled_s: None,
                cancel_reason: None,
            }),
        };
        self.action_runs.insert(key(id), run);
        self.record(
            "excon_action_queued",
            MASTER,
            json!({"id": id, "command": cmd, "run_at_s": run_at_s, "on_play": on_play, "origin": origin}),
        );
        Ok(id)
    }

    fn check_excon(&self, cmd: &Command) -> Result<()> {
        let verdict = self
            .world
            .validate_batch(&self.prior(0), std::slice::from_ref(cmd), &Actor::Master)
            .remove(0);
        if let Verdict::Rejected { reason } = verdict {
            bail!("{reason}");
        }
        Ok(())
    }

    fn queued_mut(&mut self, id: u64) -> Result<&mut ActionRun> {
        let r = self
            .action_runs
            .get_mut(&key(id))
            .ok_or_else(|| anyhow::anyhow!("unknown queued EXCON action {id}"))?;
        if r.status != ActionStatus::Approved {
            bail!("the action has already run or been cancelled");
        }
        Ok(r)
    }

    /// Withdraw a queued action before it runs. A rollback to a time before
    /// the cancellation puts it back.
    pub fn cancel_excon_action(&mut self, id: u64, reason: &str) -> Result<()> {
        let now = self.world.time_s();
        let r = self.queued_mut(id)?;
        r.status = ActionStatus::Cancelled;
        if let Some(q) = r.queued.as_mut() {
            q.cancelled_s = Some(now);
            q.cancel_reason = Some(reason.trim().to_string());
        }
        self.record("excon_action_cancelled", MASTER, json!({"id": id, "reason": reason}));
        self.sync_review_queue();
        Ok(())
    }

    /// Change the command or the instant of an action queued in the open
    /// turn. Actions queued in earlier turns are cancelled and queued again
    /// instead, so a rollback never needs to undo an edit.
    pub fn edit_excon_action(&mut self, id: u64, command: Option<Command>, when: Option<ExconWhen>) -> Result<()> {
        let now = self.world.time_s();
        let turn_start = self.turn_start_s;
        if let Some(c) = &command {
            if immediate_by_nature(c) {
                bail!("{} takes effect at once and cannot be queued", c.describe());
            }
            self.check_excon(c)?;
        }
        let run_at = match when {
            None => None,
            Some(ExconWhen::Play) => Some((now, true)),
            Some(ExconWhen::At(t)) if t < now => bail!("the chosen time has already passed"),
            Some(ExconWhen::At(t)) => Some((t, false)),
            Some(ExconWhen::Now) => bail!("use apply now on a new action instead"),
        };
        let r = self.queued_mut(id)?;
        if r.decided_s < turn_start {
            bail!("queued in an earlier turn: cancel it and queue it again");
        }
        if let Some(c) = &command {
            r.title = c.describe();
            r.command = serde_json::to_value(c)?;
        }
        if let Some((t, on_play)) = run_at {
            r.run_at_s = Some(t);
            if let Some(q) = r.queued.as_mut() {
                q.on_play = on_play;
            }
        }
        let data = json!({"id": id, "command": r.command, "run_at_s": r.run_at_s});
        self.record("excon_action_edited", MASTER, data);
        Ok(())
    }

    /// Queued EXCON actions for the console, chronological: pending ones and
    /// those that ran or were cancelled, with their state.
    pub fn excon_queue(&self) -> Vec<Value> {
        let start = self.world.scn.start_clock_s;
        let mut rows: Vec<&ActionRun> = self.action_runs.values().filter(|r| r.queued.is_some()).collect();
        rows.sort_by_key(|r| (r.run_at_s, r.queued.as_ref().map(|q| q.id)));
        rows.into_iter()
            .map(|r| {
                let q = r.queued.as_ref().unwrap();
                json!({
                    "id": q.id, "title": r.title, "command": r.command, "origin": q.origin,
                    "status": r.status, "on_play": q.on_play,
                    "run_at_s": r.run_at_s, "run_at_clock": r.run_at_s.map(|t| scenario::format_clock(start + t)),
                    "decided_s": r.decided_s, "editable": r.status == ActionStatus::Approved && r.decided_s >= self.turn_start_s,
                    "executed_s": r.executed_s, "result": r.result,
                    "cancelled_s": q.cancelled_s, "cancel_reason": q.cancel_reason,
                })
            })
            .collect()
    }
}
