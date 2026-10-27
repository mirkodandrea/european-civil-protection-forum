//! The open turn as a window on the scenario timeline: which injects fall
//! inside it, which are due now, and where "advance to the next inject" goes.
use crate::{Exercise, MASTER};
use anyhow::{bail, Result};
use serde_json::{json, Value};

/// Where an "advance to the next inject" goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceTarget {
    /// Simulated instant to advance to.
    pub to_s: i64,
    /// The advance reaches the turn's planned end, so it closes the turn.
    pub closes_turn: bool,
    /// The inject it stops at (none when it runs to the turn end).
    pub inject: Option<String>,
}

impl Exercise {
    /// Simulated instant at which the open turn is planned to end.
    pub fn planned_end_s(&self) -> i64 {
        self.turn_start_s + self.turn_minutes as i64 * 60
    }

    /// Planned time of the next pending inject: not yet sent (to everyone),
    /// not skipped, and later than now. Overdue pending injects are due, not
    /// next.
    pub fn next_pending_inject(&self) -> Option<(i64, String)> {
        let now = self.world.time_s();
        self.planned_injects()
            .into_iter()
            .filter(|i| {
                !i["not_yet_sent_to"].as_array().is_none_or(|a| a.is_empty())
                    && i["skipped"] != true
            })
            .filter_map(|i| Some((i["scenario_time_s"].as_i64()?, i["id_inject"].as_str()?.to_string())))
            .filter(|(t, _)| *t > now)
            .min()
    }

    /// Where "advance to the next inject" goes: the next pending inject's
    /// planned time, never beyond the open turn's planned end; with no inject
    /// before it, the turn end (which closes the turn). A turn end already
    /// passed gives one more minute.
    pub fn advance_target(&self) -> AdvanceTarget {
        let now = self.world.time_s();
        let end = self.planned_end_s().max(now + 60);
        match self.next_pending_inject() {
            Some((t, id)) if t < end => AdvanceTarget { to_s: t, closes_turn: false, inject: Some(id) },
            _ => AdvanceTarget { to_s: end, closes_turn: true, inject: None },
        }
    }

    /// EXCON decides not to send an MSEL inject. It no longer blocks advancing
    /// and cancels its pending pre-actions. A rollback to a time before this
    /// decision forgets it.
    pub fn skip_inject(&mut self, id: &str) -> Result<()> {
        if !self.world.scn.msel.iter().any(|i| i.id == id) {
            bail!("unknown MSEL inject {id}");
        }
        if self.inject_sent_to.get(id).is_some_and(|v| !v.is_empty()) {
            bail!("inject {id} has already been sent");
        }
        let now = self.world.time_s();
        self.skipped_injects.insert(id.to_string(), now);
        self.record("inject_skipped", MASTER, json!({"inject": id, "t_s": now}));
        Ok(())
    }

    /// Put a skipped inject back among the pending ones.
    pub fn restore_inject(&mut self, id: &str) -> Result<()> {
        if self.skipped_injects.remove(id).is_none() {
            bail!("inject {id} is not skipped");
        }
        self.record("inject_restored", MASTER, json!({"inject": id}));
        Ok(())
    }

    /// Injects of the open turn's window, chronological, each with its
    /// actions. Pending ones planned earlier than the window (overdue) are
    /// included; finished ones only from the window's start. `due` marks the
    /// pending ones whose instant has been reached: EXCON decides to send or
    /// skip those. `next_event_s` is the next pending inject.
    pub fn turn_agenda(&self) -> Value {
        let now = self.world.time_s();
        let (from, to) = (self.turn_start_s, self.planned_end_s());
        let start_clock = self.world.scn.start_clock_s;
        let clock = |t: i64| scenario::format_clock(start_clock + t);
        let mut items: Vec<(i64, Value)> = Vec::new();
        for i in self.planned_injects() {
            let at = i["scenario_time_s"].as_i64().unwrap_or(0);
            let pending = !i["not_yet_sent_to"].as_array().is_none_or(|a| a.is_empty());
            let skipped = i["skipped"] == true;
            // A skipped inject stays listed (to be restored) from its window.
            let active = pending && !skipped;
            if at >= to || (!active && at < from) {
                continue;
            }
            let status = match (i["sent"] == true, pending, skipped) {
                (_, _, true) => "skipped",
                (true, true, _) => "partial",
                (true, false, _) => "sent",
                _ => "pending",
            };
            items.push((
                at,
                json!({"kind": "inject", "id": i["id_inject"], "title": i["title"],
                    "at_s": at, "clock": clock(at), "status": status, "pending": active,
                    "skipped": skipped, "due": active && at <= now,
                    "recipients": i["not_yet_sent_to"], "actions": i["actions"]}),
            ));
        }
        items.sort_by_key(|(t, _)| *t);
        let next = self.next_pending_inject().map(|(t, _)| t);
        let due_now = items.iter().filter(|(_, v)| v["due"] == true).count();
        let adv = self.advance_target();
        json!({
            "turn": self.turn, "phase": self.phase, "turn_minutes": self.turn_minutes,
            "clock_origin_s": start_clock,
            "turn_start_s": from, "planned_end_s": to, "now_s": now,
            "start_clock": clock(from), "end_clock": clock(to), "now_clock": clock(now),
            "items": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
            "due_now": due_now,
            "next_event_s": next, "next_event_clock": next.map(clock),
            "advance": {"to_s": adv.to_s, "clock": clock(adv.to_s), "closes_turn": adv.closes_turn, "inject": adv.inject},
            "substeps": self.substeps,
        })
    }
}
