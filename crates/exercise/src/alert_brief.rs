//! Context for the AI help around EXCON alerts (see [`crate::suggestions`]):
//!
//! * the **brief** crosses the alerts with what the roles that could act have
//!   said and done (responses, messages, conversations, deployments), so the
//!   EXCON reads e.g. "nobody mentioned the school";
//! * the **draft** gives the model one alert, to write an inject a realistic
//!   in-world sender would send now.
//!
//! Nothing here changes the exercise or calls the model: the server sends the
//! context outside the exercise lock and shows the answer to the EXCON only.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::suggestions::Suggestion;
use crate::{Exercise, MsgKind, Source, MASTER};

/// Longest text of one message or response in a context.
const MAX_TEXT: usize = 500;
/// Items of each kind kept per role, newest kept.
const MAX_ITEMS: usize = 30;

fn cut(s: &str) -> String {
    let mut out: String = s.chars().take(MAX_TEXT).collect();
    if s.chars().count() > MAX_TEXT {
        out.push('…');
    }
    out
}

fn newest<T>(mut v: Vec<T>) -> Vec<T> {
    let skip = v.len().saturating_sub(MAX_ITEMS);
    v.drain(..skip);
    v
}

impl Exercise {
    /// Roles named by `alerts` (`all` stands for every player role).
    fn alert_roles(&self, alerts: &[Suggestion]) -> BTreeSet<String> {
        let mut roles = BTreeSet::new();
        for a in alerts {
            for r in &a.recipients {
                if r == "all" {
                    roles.extend(
                        self.world
                            .scn
                            .roles
                            .iter()
                            .filter(|x| x.id != MASTER)
                            .map(|x| x.id.clone()),
                    );
                } else {
                    roles.insert(r.clone());
                }
            }
        }
        roles
    }

    /// What `role` said and did during the exercise, newest last (English texts).
    fn role_activity(&self, role: &str) -> Value {
        let clock = |t: i64| scenario::format_clock(self.world.scn.start_clock_s + t);
        let responses = self
            .proposals
            .iter()
            .filter(|p| matches!(&p.source, Source::Player(r) if r == role))
            .map(|p| {
                let text = if p.translation_en.trim().is_empty() {
                    &p.original
                } else {
                    &p.translation_en
                };
                json!({"clock": clock(p.t_s), "turn": p.turn, "status": p.status,
                    "text": cut(text), "actions": p.descriptions})
            })
            .collect::<Vec<_>>();
        let messages = self
            .messages
            .iter()
            .filter(|m| {
                (m.from == role || m.to == role)
                    && matches!(
                        m.kind,
                        MsgKind::Chat
                            | MsgKind::Inject
                            | MsgKind::Reply
                            | MsgKind::Clarification
                            | MsgKind::RoleMessage
                    )
            })
            .map(|m| {
                json!({"clock": m.clock, "kind": m.kind, "from": m.from, "to": m.to,
                    "title": m.title, "text": cut(&m.text_en)})
            })
            .collect::<Vec<_>>();
        let conversations = self
            .linked_conversations
            .iter()
            .filter(|c| c.has(role))
            .flat_map(|c| {
                c.messages.iter().map(move |m| {
                    json!({"conversation": c.id, "participants": c.participants,
                        "clock": m.clock, "from": m.from, "text": cut(&m.text_en)})
                })
            })
            .collect::<Vec<_>>();
        let deployments = self
            .world
            .d
            .tasks
            .iter()
            .filter(|t| t.role == role)
            .map(|t| {
                json!({"resource": t.resource, "qty": t.qty, "status": t.status,
                    "zone": self.world.scn.zone_of(t.target)})
            })
            .collect::<Vec<_>>();
        json!({
            "responses": newest(responses),
            "messages": newest(messages),
            "conversations": newest(conversations),
            "deployments": deployments,
        })
    }

    /// Context for the AI brief: the alerts, and for every role that could act
    /// on one of them what it said and did.
    pub fn alert_brief_context(&self, alerts: &[Suggestion]) -> Value {
        let roles = self
            .alert_roles(alerts)
            .into_iter()
            .map(|r| {
                let name = self
                    .world
                    .scn
                    .role(&r)
                    .map(|x| x.name.clone())
                    .unwrap_or_else(|| r.clone());
                json!({"id": r, "name": name, "activity": self.role_activity(&r)})
            })
            .collect::<Vec<_>>();
        json!({
            "clock": self.world.clock(),
            "turn": self.turn,
            "alerts": alerts.iter().map(|a| json!({
                "id": a.id, "rule": a.rule, "object": a.object.name,
                "alert": a.alert.en, "evidence": a.evidence.en, "roles_that_can_act": a.recipients,
            })).collect::<Vec<_>>(),
            "roles": roles,
        })
    }

    /// Context for an AI inject draft from `alert`: what is true now, how far
    /// the threat is (minutes, never a clock time to repeat) and who receives it.
    pub fn alert_draft_context(&self, alert: &Suggestion) -> Value {
        let now = self.world.time_s();
        let minutes = alert.reach_s.map(|t| ((t - now).max(0) + 59) / 60);
        let recipients = alert
            .recipients
            .iter()
            .map(|r| {
                self.world
                    .scn
                    .role(r)
                    .map(|x| x.name.clone())
                    .unwrap_or_else(|| r.clone())
            })
            .collect::<Vec<_>>();
        json!({
            "clock_now": self.world.clock(),
            "rule": alert.rule,
            "object": {"kind": alert.object.kind, "name": alert.object.name},
            "situation": alert.evidence.en,
            "already_happening": minutes == Some(0),
            "minutes_until_impact": minutes,
            "recipients": recipients,
        })
    }
}
