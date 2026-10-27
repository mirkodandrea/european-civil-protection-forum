//! The open turn as a window on the scenario timeline: what falls inside it,
//! what is due now, and where the next event is.
use crate::Exercise;
use serde_json::{json, Value};

impl Exercise {
    /// Simulated instant at which the open turn is planned to end.
    pub fn planned_end_s(&self) -> i64 {
        self.turn_start_s + self.turn_minutes as i64 * 60
    }

    /// Injects and scripted actions of the open turn's window, chronological.
    /// Pending ones planned earlier than the window (overdue) are included;
    /// finished ones only from the window's start. `due` marks the pending
    /// ones whose instant has been reached: EXCON is asked about those.
    pub fn turn_agenda(&self) -> Value {
        let now = self.world.time_s();
        let (from, to) = (self.turn_start_s, self.planned_end_s());
        let start_clock = self.world.scn.start_clock_s;
        let clock = |t: i64| scenario::format_clock(start_clock + t);
        let mut items: Vec<(i64, Value)> = Vec::new();
        for i in self.planned_injects() {
            let at = i["scenario_time_s"].as_i64().unwrap_or(0);
            let pending = !i["not_yet_sent_to"].as_array().is_none_or(|a| a.is_empty());
            if at >= to || (!pending && at < from) {
                continue;
            }
            let status = match (i["sent"] == true, pending) {
                (true, true) => "partial",
                (true, false) => "sent",
                _ => "pending",
            };
            items.push((
                at,
                json!({"kind": "inject", "id": i["id_inject"], "title": i["title"],
                    "at_s": at, "clock": clock(at), "status": status, "pending": pending,
                    "due": pending && at <= now, "recipients": i["not_yet_sent_to"]}),
            ));
        }
        for a in self.timeline_actions() {
            let at = a["at_s"].as_i64().unwrap_or(0);
            let pending = a["pending"] == true;
            if at >= to || (!pending && at < from) {
                continue;
            }
            items.push((
                at,
                json!({"kind": "action", "id": a["id"], "title": a["title"], "summary": a["summary"],
                    "at_s": at, "clock": clock(at), "status": a["status"], "pending": pending,
                    "run": !a["run"].is_null(), "due": pending && at <= now}),
            ));
        }
        items.sort_by_key(|(t, _)| *t);
        let next = items
            .iter()
            .filter(|(t, v)| v["pending"] == true && *t > now)
            .map(|(t, _)| *t)
            .min();
        let due_now = items.iter().filter(|(_, v)| v["due"] == true).count();
        json!({
            "turn": self.turn, "phase": self.phase,
            "turn_start_s": from, "planned_end_s": to, "now_s": now,
            "start_clock": clock(from), "end_clock": clock(to), "now_clock": clock(now),
            "items": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
            "due_now": due_now,
            "next_event_s": next, "next_event_clock": next.map(clock),
            "substeps": self.substeps,
        })
    }
}
