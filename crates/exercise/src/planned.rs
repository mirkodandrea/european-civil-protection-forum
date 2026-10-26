//! EXCON master-view projections: the unified chronological MSEL plan and
//! the responses players gave to delivered injects.
use crate::timing::ScheduleStatus;
use crate::{Exercise, MsgKind, ProposalStatus, MASTER};
use serde_json::{json, Value};

impl Exercise {
    /// Planned time of an MSEL inject in simulated seconds from start: the live
    /// schedule if any, otherwise the start of its suggested turn.
    fn planned_due_s(&self, id: &str, turn: u32) -> i64 {
        self.scheduled_injects
            .iter()
            .filter(|s| {
                s.msel_id == id
                    && !matches!(s.status, ScheduleStatus::Cancelled | ScheduleStatus::Failed)
            })
            .map(|s| s.due_s)
            .min()
            .unwrap_or_else(|| (turn.max(1) as i64 - 1) * self.turn_minutes as i64 * 60)
    }

    /// All MSEL injects, chronological, with per-recipient send tracking.
    pub fn planned_injects(&self) -> Vec<Value> {
        let ratio = self.clock_control.ratio.max(0.01);
        let start = self.world.scn.start_clock_s;
        let all = self.role_ids();
        let mut rows: Vec<(i64, usize, Value)> = self
            .world
            .scn
            .msel
            .iter()
            .enumerate()
            .map(|(n, t)| {
                let due = self.planned_due_s(&t.id, t.turn);
                let planned: Vec<String> = if t.recipients.iter().any(|r| r == "all") {
                    all.clone()
                } else {
                    t.recipients.clone()
                };
                let sent_to = self.inject_sent_to.get(&t.id).cloned().unwrap_or_default();
                let not_yet: Vec<&String> =
                    planned.iter().filter(|r| !sent_to.contains(r)).collect();
                let schedule = self
                    .scheduled_injects
                    .iter()
                    .filter(|s| s.msel_id == t.id)
                    .map(|s| s.id)
                    .next_back();
                (
                    due,
                    n,
                    json!({
                        "id_inject": t.id, "title": t.title, "event": t.event, "sender": t.sender,
                        "turn": t.turn, "text": t.text, "objective": t.objective,
                        "real_time_offset": (due as f64 / ratio).round() as i64,
                        "scenario_time": scenario::format_clock(start + due),
                        "scenario_time_s": due,
                        "recipients_planned": planned,
                        "sent_to": sent_to,
                        "not_yet_sent_to": not_yet,
                        "sent": !sent_to.is_empty(),
                        "schedule_id": schedule,
                    }),
                )
            })
            .collect();
        rows.sort_by_key(|(d, n, _)| (*d, *n));
        rows.into_iter().map(|r| r.2).collect()
    }

    /// One row per (inject delivery, recipient): the inject, the player's
    /// replies in that thread and any proposal still awaiting EXCON review.
    pub fn inject_responses(&self) -> Vec<Value> {
        self.messages
            .iter()
            .filter(|m| m.kind == MsgKind::Inject && m.inject_id.is_some())
            .map(|m| {
                let delivery = m.inject_id.unwrap();
                let replies: Vec<Value> = self
                    .messages
                    .iter()
                    .filter(|r| {
                        r.kind == MsgKind::Reply && r.inject_id == Some(delivery) && r.from == m.to
                    })
                    .map(|r| {
                        json!({"id": r.id, "reply_to": r.reply_to, "clock": r.clock,
                        "t_s": r.t_s, "text": r.text_en, "original": r.original,
                        "proposal": r.proposal})
                    })
                    .collect();
                let pending: Vec<Value> = self
                    .proposals
                    .iter()
                    .filter(|p| {
                        p.inject_id == Some(delivery)
                            && p.source.label() == m.to
                            && matches!(
                                p.status,
                                ProposalStatus::Parsing
                                    | ProposalStatus::Pending
                                    | ProposalStatus::NeedsClarification
                            )
                    })
                    .map(|p| {
                        json!({"id": p.id, "status": p.status, "summary": p.summary,
                        "original": p.original, "reply_to": p.reply_to, "turn": p.turn,
                        "awaiting_clarification": p.status == ProposalStatus::NeedsClarification
                            && p.clarification_request.is_some(),
                        "clarification_request": p.clarification_request})
                    })
                    .collect();
                // The whole conversation hanging off this delivery, in order:
                // the inject, the player's replies, EXCON clarifications and
                // any chat that continued the thread with this recipient.
                let thread: Vec<Value> = self
                    .messages
                    .iter()
                    .filter(|t| {
                        t.inject_id == Some(delivery)
                            && (t.from == m.to || t.to == m.to)
                            // Peer chats between players stay private to them.
                            && (t.kind != MsgKind::Chat || t.from == MASTER || t.to == MASTER)
                            && matches!(
                                t.kind,
                                MsgKind::Inject | MsgKind::Reply | MsgKind::Clarification | MsgKind::Chat
                            )
                    })
                    .map(|t| {
                        let original = t.original.as_ref().filter(|o| **o != t.text_en);
                        json!({"id": t.id, "kind": t.kind, "from": t.from, "to": t.to,
                        "turn": t.turn, "clock": t.clock, "t_s": t.t_s, "title": t.title,
                        "text": t.text_en, "original": original, "reply_to": t.reply_to})
                    })
                    .collect();
                json!({
                    "thread": thread,
                    "inject_message_id": m.id, "delivery_id": delivery,
                    "id_inject": self.delivery_msel.get(&delivery),
                    "title": m.title, "role": m.to, "clock": m.clock, "t_s": m.t_s,
                    "inject_text": m.text_en, "replies": replies, "pending_proposals": pending,
                })
            })
            .collect()
    }
}
