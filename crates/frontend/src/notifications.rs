//! Presentation-only notifications derived exclusively from the authorized API view.
use crate::{
    api::{list, text, window},
    i18n, App, Msg,
};
use serde_json::Value;
use std::collections::BTreeSet;
use yew::prelude::*;

/// `Action` needs the EXCON (or player) to do something: its toast stays until handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Info,
    Action,
}
#[derive(Clone, Debug)]
pub struct Notice {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub clock: String,
    pub view: String,
    pub peer: Option<String>,
    pub priority: Priority,
    /// Similar notices sharing a group replace each other while still unread.
    group: Option<String>,
    toast: bool,
    message: Option<u64>,
    read: bool,
}
impl Notice {
    fn new(key: String, title: String, detail: String, clock: &str, view: &str) -> Self {
        Self {
            key,
            title,
            detail: detail.chars().take(180).collect(),
            clock: clock.into(),
            view: view.into(),
            peer: None,
            priority: Priority::Info,
            group: None,
            toast: true,
            message: None,
            read: false,
        }
    }
    fn action(mut self) -> Self {
        self.priority = Priority::Action;
        self
    }
    fn quiet(mut self) -> Self {
        self.toast = false;
        self
    }
    fn group(mut self, group: String) -> Self {
        self.group = Some(group);
        self
    }
    fn from_spec(key: String, spec: Spec, clock: &str) -> Self {
        let mut n = Self::new(key, spec.title, spec.detail, clock, spec.view);
        n.priority = spec.priority;
        n.toast = spec.toast;
        n.group = spec.group;
        n
    }
}
/// What an EXCON log event says to the console, before it becomes a `Notice`.
#[derive(Debug)]
struct Spec {
    title: String,
    detail: String,
    view: &'static str,
    priority: Priority,
    toast: bool,
    group: Option<String>,
}
impl Spec {
    fn new(title: String, detail: String, view: &'static str) -> Self {
        Self {
            title,
            detail,
            view,
            priority: Priority::Info,
            toast: true,
            group: None,
        }
    }
    fn quiet(mut self) -> Self {
        self.toast = false;
        self
    }
    fn action(mut self) -> Self {
        self.priority = Priority::Action;
        self
    }
}
#[derive(Default)]
pub struct NotificationFeed {
    known: BTreeSet<String>,
    items: Vec<Notice>,
    toasts: Vec<String>,
    pub open: bool,
    initialized: bool,
    sequence: u64,
}
impl NotificationFeed {
    pub fn unread(&self) -> usize {
        self.items.iter().filter(|n| !n.read).count()
    }
    pub fn unread_actions(&self) -> usize {
        self.items
            .iter()
            .filter(|n| !n.read && n.priority == Priority::Action)
            .count()
    }
    /// The thing a pending notice asked for has been handled: clear it and its toast.
    pub fn resolve_group(&mut self, group: &str) {
        let keys: Vec<String> = self
            .items
            .iter_mut()
            .filter(|n| n.group.as_deref() == Some(group))
            .map(|n| {
                n.read = true;
                n.key.clone()
            })
            .collect();
        self.toasts.retain(|k| !keys.contains(k));
    }
    pub fn read_thread(&mut self, peer: &str, max: u64) {
        for n in &mut self.items {
            if n.peer.as_deref() == Some(peer) && n.message.is_some_and(|id| id <= max) {
                n.read = true;
            }
        }
    }
    pub fn read_view(&mut self, view: &str) {
        for n in &mut self.items {
            if n.view == view && n.peer.is_none() && n.priority == Priority::Info {
                n.read = true;
            }
        }
    }
    pub fn mark_all_read(&mut self) {
        for n in &mut self.items {
            n.read = true;
        }
        self.toasts.clear();
    }
    pub fn dismiss_toast(&mut self, key: &str) {
        self.toasts.retain(|k| k != key);
    }
    pub fn open_item(&mut self, key: &str) -> Option<Notice> {
        self.open = false;
        self.dismiss_toast(key);
        let item = self.items.iter_mut().find(|n| n.key == key)?;
        item.read = true;
        Some(item.clone())
    }
    /// Returns the keys of the toasts that must expire by themselves.
    fn ingest(&mut self, candidates: Vec<Notice>) -> Vec<String> {
        let mut timers = Vec::new();
        for n in candidates {
            if !self.known.insert(n.key.clone()) {
                continue;
            }
            let mut show = self.initialized && !n.read && n.toast;
            if let Some(group) = &n.group {
                if let Some(pos) = self
                    .items
                    .iter()
                    .position(|o| !o.read && o.group.as_ref() == Some(group))
                {
                    let old = self.items.remove(pos);
                    if let Some(t) = self.toasts.iter().position(|k| *k == old.key) {
                        self.toasts.remove(t);
                        show = !n.read && n.toast;
                    }
                }
            }
            if show {
                self.toasts.push(n.key.clone());
                if n.priority == Priority::Info {
                    timers.push(n.key.clone());
                }
            }
            self.items.push(n);
        }
        self.initialized = true;
        // Keep the bell useful during long exercises without unbounded DOM growth.
        let mut excess = self.items.len().saturating_sub(200);
        self.items.retain(|n| {
            if excess > 0 && n.read {
                excess -= 1;
                false
            } else {
                true
            }
        });
        // At most three toasts; informative ones give way before the ones that need action.
        while self.toasts.len() > 3 {
            let info = self.toasts.iter().position(|k| {
                self.items
                    .iter()
                    .any(|n| &n.key == k && n.priority == Priority::Info)
            });
            self.toasts.remove(info.unwrap_or(0));
        }
        timers
    }
}
fn tr<'a>(lang: &str, it: &'a str, en: &'a str) -> &'a str {
    if lang == "it" {
        it
    } else {
        en
    }
}
fn mission_changed(previous: &[Value], task: &Value) -> bool {
    !previous
        .iter()
        .any(|d| d["task"]["id"] == task["id"] && d["task"]["status"] == task["status"])
}
/// "Day 1 13:05" -> "13:05".
fn short_clock(clock: &str) -> &str {
    clock
        .split_whitespace()
        .rfind(|w| w.contains(':'))
        .unwrap_or(clock)
}
/// Up to three names, then "+n".
fn names(ids: &Value, name: &dyn Fn(&str) -> String) -> String {
    let all: Vec<String> = list(ids).iter().map(|r| name(text(r))).collect();
    let more = all.len().saturating_sub(3);
    let mut out = all[..all.len() - more].join(", ");
    if more > 0 {
        out.push_str(&format!(" +{more}"));
    }
    out
}
/// The proposal a role is waiting on the EXCON to review; other statuses need nothing.
fn pending_proposal_spec(p: &Value, name: &dyn Fn(&str) -> String) -> Option<Spec> {
    if p["status"] != "pending" {
        return None;
    }
    let title = if p["source"]["kind"] == "master" {
        i18n::ex("Suggerimento AI da rivedere").to_string()
    } else {
        crate::exfmt!(
            "Proposta in attesa da {}",
            "Proposal awaiting review from {}",
            name(text(&p["source"]["role"]))
        )
    };
    Some(
        Spec::new(title, text(&p["original"]).into(), "risposte")
            .action()
            .group(format!("pending:proposal:{}", p["id"])),
    )
}
impl Spec {
    fn group(mut self, group: String) -> Self {
        self.group = Some(group);
        self
    }
}
/// Maps an EXCON log event to a notice; `None` for bookkeeping that needs no attention
/// (the EXCON's own commands, parsing steps, state echoes).
fn log_spec(
    e: &Value,
    name: &dyn Fn(&str) -> String,
    action_title: &dyn Fn(&str) -> String,
) -> Option<Spec> {
    let d = &e["data"];
    let clock = text(&e["clock"]);
    let spec = |title: String, detail: String, view: &'static str| Spec::new(title, detail, view);
    Some(match text(&e["kind"]) {
        "message"
        | "chat_source"
        | "reply_source"
        | "clock_physical_events"
        | "resource_event"
        | "turn_opened"
        | "exercise_created"
        | "proposal_declared"
        | "proposal_parsed"
        | "proposal_parse_failed"
        | "proposal_status"
        | "proposal_decided"
        | "proposal_revalidated"
        | "proposal_superseded"
        | "clarification_requested"
        | "clarification_closed"
        | "clarification_answered"
        | "master_command"
        | "master_prompt"
        | "settings"
        | "language_set"
        | "timeline_action_decided"
        | "timeline_action_toggled"
        | "inject_skipped"
        | "inject_restored"
        | "inject_scheduled"
        | "inject_schedule_cancelled"
        | "communications_reviewed"
        | "resource_limit_draft_reviewed"
        | "reference_materials_updated"
        | "training_design_configured"
        | "training_path_approved"
        | "clock_configured" => return None,
        "turn_resolving" => spec(
            crate::exfmt!(
                "Turno {} · risoluzione avviata",
                "Turn {} · resolution started",
                d["turn"]
            ),
            crate::exfmt!("{} min simulati", "{} simulated min", d["minutes"]),
            "timeline",
        )
        .quiet(),
        "turn_substep" => {
            let advanced =
                (d["to_s"].as_i64().unwrap_or(0) - d["from_s"].as_i64().unwrap_or(0)) / 60;
            spec(
                crate::exfmt!(
                    "Turno {} · avanzamento a {}",
                    "Turn {} · advanced to {}",
                    d["turn"],
                    short_clock(clock)
                ),
                crate::exfmt!("+{} min", "+{} min", advanced),
                "timeline",
            )
            .quiet()
            .group(format!("progress:{}", d["turn"]))
        }
        "turn_resolved" => spec(
            crate::exfmt!("Turno {} risolto", "Turn {} resolved", d["turn"]),
            crate::exfmt!("{} min simulati", "{} simulated min", d["minutes"]),
            "timeline",
        ),
        "inject_sent" => spec(
            crate::exfmt!(
                "Inject inviato a {}",
                "Inject sent to {}",
                names(&d["recipients"], name)
            ),
            text(&d["title"]).into(),
            "history",
        )
        .quiet(),
        "scheduled_inject_triggered" => spec(
            i18n::ex("Inject programmato attivato").to_string(),
            clock.into(),
            "timeline",
        ),
        "timeline_action_executed" => {
            let id = text(&d["action"]);
            let title = action_title(id);
            let result = text(&d["result"]).to_string();
            match text(&d["status"]) {
                "done" => spec(
                    crate::exfmt!(
                        "Azione pianificata eseguita: {}",
                        "Scheduled action executed: {}",
                        title
                    ),
                    result,
                    "timeline",
                ),
                "failed" => spec(
                    crate::exfmt!(
                        "Azione pianificata non riuscita: {}",
                        "Scheduled action failed: {}",
                        title
                    ),
                    result,
                    "timeline",
                )
                .action(),
                _ => spec(
                    crate::exfmt!(
                        "Azione pianificata non eseguita: {}",
                        "Scheduled action not executed: {}",
                        title
                    ),
                    result,
                    "timeline",
                )
                .quiet(),
            }
        }
        "approved_inject_delivered" | "approved_communication_delivered" => spec(
            crate::render::event_label(text(&e["kind"])),
            clock.into(),
            "history",
        )
        .quiet(),
        "approved_inject_failed" | "approved_communication_failed" | "clock_failed" => spec(
            crate::render::event_label(text(&e["kind"])),
            text(if d["error"].is_null() {
                &d["reason"]
            } else {
                &d["error"]
            })
            .into(),
            "history",
        )
        .action(),
        "forecast_published" => spec(
            i18n::ex("Previsione pubblicata").to_string(),
            names(&d["roles"], name),
            "history",
        ),
        "exercise_ended" => spec(
            i18n::ex("Esercitazione terminata").to_string(),
            clock.into(),
            "history",
        ),
        "rollback" => spec(
            crate::exfmt!(
                "Ripristino al turno {}",
                "Rolled back to turn {}",
                d["to_turn"]
            ),
            clock.into(),
            "timeline",
        )
        .quiet(),
        "player_draft_submitted" => {
            if !d["proposal"].is_null() {
                return None;
            }
            spec(
                crate::exfmt!(
                    "Bozza di {} da rivedere",
                    "Draft from {} to review",
                    name(text(&e["actor"]))
                ),
                clock.into(),
                "risposte",
            )
            .action()
        }
        kind => spec(crate::render::event_label(kind), String::new(), "history").quiet(),
    })
}
impl App {
    pub fn panel_visible(&self) -> bool {
        if window().document().is_some_and(|d| d.hidden()) {
            return false;
        }
        window()
            .document()
            .and_then(|d| d.get_element_by_id(&format!("app-header-{}", self.own)))
            .is_some_and(|e| e.get_bounding_client_rect().width() > 0.0)
    }
    pub fn chat_unread(&self) -> usize {
        self.messages()
            .iter()
            .filter(|m| {
                let peer = text(&m["from"]);
                m["kind"] == "chat"
                    && m["to"] == self.own
                    && peer != self.own
                    && self.contacts().iter().any(|c| c["id"] == peer)
                    && m["id"].as_u64().unwrap_or(0) > self.seen.get(peer).copied().unwrap_or(0)
            })
            .count()
    }
    pub fn update_notifications(&mut self, ctx: &Context<Self>, state: &Value, master: &Value) {
        // Strings from `i18n::ex` (event labels) are baked into the notice:
        // build them under this panel's own language, not whichever panel rendered last.
        let own_lang = self.lang();
        i18n::with_ui_lang(&own_lang, || self.build_notifications(ctx, state, master));
    }
    fn build_notifications(&mut self, ctx: &Context<Self>, state: &Value, master: &Value) {
        let lang = self.lang();
        let lang = lang.as_str();
        let visible = self.panel_visible();
        let master_view = self.is_master() || self.is_evaluator();
        let mut candidates = Vec::new();
        let messages = if master_view {
            list(&master["messages"])
        } else {
            list(&state["inbox"])
        };
        for m in messages {
            let from = text(&m["from"]);
            let is_chat = m["kind"] == "chat";
            // EXCON hears about injects and proposals from the log and proposals, chat only when addressed to it.
            if self.is_master() && (!is_chat || m["to"] != self.own) {
                continue;
            }
            let peer = if from == self.own {
                text(&m["to"])
            } else {
                from
            };
            let view = if self.is_master() {
                "risposte"
            } else if master_view {
                "timeline"
            } else if is_chat {
                "chat"
            } else if m["kind"] == "situation" {
                "function"
            } else {
                "map"
            };
            let title = if is_chat {
                format!("{} · {}", tr(lang, "Messaggio", "Message"), self.name(peer))
            } else if !text(&m["title"]).is_empty() {
                text(&m["title"]).to_string()
            } else {
                tr(lang, "Aggiornamento ricevuto", "Update received").into()
            };
            let mut n = Notice::new(
                format!("message:{}", m["id"]),
                title,
                text(&m["text"]).into(),
                text(&m["clock"]),
                view,
            );
            if is_chat {
                n.peer = Some(peer.into());
                n.message = m["id"].as_u64();
            }
            n.read = from == self.own
                || (visible && self.active == view && (!is_chat || self.selected == peer));
            if is_chat && n.message.unwrap_or(0) <= self.seen.get(peer).copied().unwrap_or(0) {
                n.read = true;
            }
            candidates.push(n);
        }
        if master_view {
            // The EXCON is notified only of proposals waiting for review, until they are handled.
            let name = |id: &str| self.name(id);
            let mut handled = Vec::new();
            for p in list(&master["proposals"]) {
                let pending = p["status"] == "pending";
                match pending_proposal_spec(p, &name) {
                    Some(spec) => {
                        let rounds = list(&p["history"])
                            .iter()
                            .filter(|h| h["status"] == "pending")
                            .count();
                        candidates.push(Notice::from_spec(
                            format!("proposal:{}:pending:{rounds}", p["id"]),
                            spec,
                            "",
                        ));
                    }
                    None if !pending => handled.push(format!("pending:proposal:{}", p["id"])),
                    None => {}
                }
            }
            for group in handled {
                self.notifications.resolve_group(&group);
            }
        } else {
            for p in list(&state["proposals"]) {
                let status = text(&p["status"]);
                if ["superseded", "parsing", "pending", "informational"].contains(&status) {
                    continue;
                }
                let rounds = list(&p["history"])
                    .iter()
                    .filter(|h| h["status"] == status)
                    .count();
                let mut n = Notice::new(
                    format!("proposal:{}:{status}:{rounds}", p["id"]),
                    format!(
                        "{} · {}",
                        tr(lang, "Proposta", "Proposal"),
                        i18n::status(lang, if p["narrative"]==true&&status=="approved" {"narrative_approved"}else{status})
                    ),
                    text(&p["original"]).into(),
                    "",
                    "map",
                );
                if status == "needs_clarification" {
                    n = n.action();
                } else {
                    n.read = visible && self.active == "map";
                }
                candidates.push(n);
            }
        }
        for summary in list(&state["turn_summaries"])
            .iter()
            .filter(|s| s["status"] == "ready" || s["status"] == "unavailable")
        {
            let mut n = Notice::new(
                format!("summary:{}:{}", summary["id"], summary["status"]),
                format!(
                    "{} {} · {}",
                    tr(lang, "Riepilogo turno", "Turn recap"),
                    summary["turn"],
                    i18n::summary_status(lang, text(&summary["status"]))
                ),
                text(&summary["text"]).into(),
                text(&summary["clock"]),
                "function",
            );
            n.read = visible && self.active == "function";
            candidates.push(n);
        }
        for deployment in list(&state["deployments"]) {
            let task = &deployment["task"];
            if !mission_changed(list(&self.state["deployments"]), task) {
                continue;
            }
            self.notifications.sequence += 1;
            let mut n = Notice::new(
                format!(
                    "mission:{}:{}:{}",
                    task["id"], task["status"], self.notifications.sequence
                ),
                format!(
                    "{} · {}",
                    tr(lang, "Missione", "Mission"),
                    i18n::mission_status(lang, text(&task["status"]))
                ),
                text(&task["detail"]).into(),
                "",
                "function",
            )
            .group(format!("mission:{}", task["id"]));
            n.toast = !master_view;
            n.read = visible && self.active == "function";
            candidates.push(n);
        }
        // EXCON learns of turn changes from the log; players get one notice per phase change.
        if !master_view
            && !self.state.is_null()
            && (self.state["turn"] != state["turn"] || self.state["phase"] != state["phase"])
        {
            self.notifications.sequence += 1;
            let phase = text(&state["phase"]);
            let detail = match phase {
                "orders" => tr(
                    lang,
                    "Puoi rispondere e proporre azioni",
                    "You can respond and propose actions",
                ),
                "resolving" => tr(
                    lang,
                    "Gli ordini approvati vengono simulati",
                    "Approved orders are being simulated",
                ),
                _ => "",
            };
            let n = Notice::new(
                format!("turn:{}", self.notifications.sequence),
                i18n::tf(
                    lang,
                    "turn_phase",
                    &[&state["turn"].to_string(), i18n::phase(lang, phase)],
                ),
                detail.into(),
                text(&state["clock"]),
                "function",
            );
            candidates.push(n);
        }
        // Availability changes continuously while a turn resolves: bell only, one entry.
        if !self.state.is_null()
            && self.state["resource_accounting"] != state["resource_accounting"]
        {
            self.notifications.sequence += 1;
            let view = if master_view { "advanced" } else { "function" };
            let n = Notice::new(
                format!("resources:{}", self.notifications.sequence),
                tr(lang, "Risorse aggiornate", "Resources updated").into(),
                tr(
                    lang,
                    "Disponibilità e impieghi aggiornati",
                    "Availability and deployments updated",
                )
                .into(),
                text(&state["clock"]),
                view,
            )
            .quiet()
            .group("resources".into());
            candidates.push(n);
        }
        // EXCON receives only its authorized operational log, never private player chat.
        if master_view {
            let name = |id: &str| self.name(id);
            let action_title = |id: &str| {
                list(&master["inject_actions"])
                    .iter()
                    .find(|a| a["id"] == id)
                    .map(|a| text(&a["title"]).to_string())
                    .unwrap_or(id.into())
            };
            for event in list(&master["log"]) {
                if let Some(spec) = log_spec(event, &name, &action_title) {
                    let mut n = Notice::from_spec(
                        format!("log:{}", event["seq"]),
                        spec,
                        text(&event["clock"]),
                    );
                    n.read = n.priority == Priority::Info && visible && self.active == n.view;
                    candidates.push(n);
                }
            }
        }
        for key in self.notifications.ingest(candidates) {
            let link = ctx.link().clone();
            gloo_timers::callback::Timeout::new(6500, move || {
                link.send_message(Msg::DismissToast(key))
            })
            .forget();
        }
    }
    pub fn push_failure_notification(&mut self, ctx: &Context<Self>) {
        self.notifications.sequence += 1;
        let l = self.lang();
        let n = Notice::new(
            format!("failure:{}", self.notifications.sequence),
            tr(&l, "Operazione non riuscita", "Operation failed").into(),
            self.error.clone(),
            "",
            &self.active,
        );
        for key in self.notifications.ingest(vec![n]) {
            let link = ctx.link().clone();
            gloo_timers::callback::Timeout::new(6500, move || {
                link.send_message(Msg::DismissToast(key))
            })
            .forget();
        }
    }
    pub fn view_notification_bell(&self, ctx: &Context<Self>) -> Html {
        let lang = self.lang();
        let l = lang.as_str();
        let unread = self.notifications.unread();
        html! {<div class="notification-hub">
            <button type="button" class="notification-bell" data-notification-bell="true" aria-label={format!("{} · {} {}",tr(l,"Notifiche","Notifications"),unread,tr(l,"non lette","unread"))} aria-controls={format!("notification-panel-{}",self.own)} aria-expanded={self.notifications.open.to_string()} onclick={ctx.link().callback(|_|Msg::ToggleNotifications)}><span aria-hidden="true">{"🔔"}</span>if unread>0{<span class={classes!("unread",(self.notifications.unread_actions()>0).then_some("urgent"))} data-notification-count="true">{unread}</span>}</button>
            if self.notifications.open {<button type="button" class="notification-backdrop" tabindex="-1" aria-label={tr(l,"Chiudi notifiche","Close notifications")} onclick={ctx.link().callback(|_|Msg::ToggleNotifications)}/><section class="notification-panel" id={format!("notification-panel-{}",self.own)} data-notification-panel="true" role="region" aria-label={tr(l,"Notifiche","Notifications")} onkeydown={ctx.link().batch_callback(|e:KeyboardEvent|if e.key()=="Escape"{Some(Msg::ToggleNotifications)}else{None})}>
                <header><strong>{tr(l,"Notifiche","Notifications")}</strong><button type="button" data-mark-notifications-read="true" disabled={unread==0} onclick={ctx.link().callback(|_|Msg::ReadNotifications)}>{tr(l,"Segna tutte lette","Mark all read")}</button></header>
                if self.notifications.items.is_empty(){<p class="muted">{tr(l,"Nessuna notifica","No notifications")}</p>}
                <ol>{for self.notifications.items.iter().rev().map(|n|{let key=n.key.clone();html!{<li key={key.clone()}><button type="button" data-open-notification={key.clone()} class={classes!("notification-item",(!n.read).then_some("unread-event"),(n.priority==Priority::Action).then_some("needs-action"))} onclick={ctx.link().callback(move |_|Msg::OpenNotification(key.clone()))}><strong>{&n.title}</strong><small>{if n.priority==Priority::Action&&!n.read{format!("{} · {}",tr(l,"Richiede azione","Needs action"),n.clock)}else{n.clock.clone()}}</small><span>{&n.detail}</span></button></li>}})}</ol>
            </section>}
        </div>}
    }
    pub fn view_notification_toasts(&self, ctx: &Context<Self>) -> Html {
        let lang = self.lang();
        html! {<aside class="notification-toasts" aria-live="polite" aria-atomic="false">{for self.notifications.toasts.iter().filter_map(|key| self.notifications.items.iter().find(|n| &n.key == key)).map(|n|{
            let key=n.key.clone();let dismiss=key.clone();html!{<div class={classes!("notification-toast",(n.priority==Priority::Action).then_some("needs-action"))} data-notification-toast={key.clone()} key={key.clone()}><button type="button" data-open-notification={key.clone()} onclick={ctx.link().callback(move |_|Msg::OpenNotification(key.clone()))}><strong>{&n.title}</strong><span>{&n.detail}</span></button><button type="button" class="dismiss-toast" aria-label={tr(&lang,"Chiudi notifica","Dismiss notification")} onclick={ctx.link().callback(move |_|Msg::DismissToast(dismiss.clone()))}>{"×"}</button></div>}
        })}</aside>}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> Notice {
        Notice::new(id.into(), "Message".into(), "Body".into(), "13:00", "chat")
    }
    #[test]
    fn mission_progress_is_quiet_but_repeated_status_transitions_notify() {
        let previous =
            vec![serde_json::json!({"task":{"id":7,"status":"working","detail":"Cut 10m"}})];
        assert!(!mission_changed(
            &previous,
            &serde_json::json!({"id":7,"status":"working","detail":"Cut 20m"})
        ));
        assert!(mission_changed(
            &previous,
            &serde_json::json!({"id":7,"status":"refilling"})
        ));
        let refill = vec![serde_json::json!({"task":{"id":7,"status":"refilling"}})];
        assert!(mission_changed(
            &refill,
            &serde_json::json!({"id":7,"status":"working"})
        ));
    }
    #[test]
    fn repeated_polls_do_not_duplicate_or_resurrect_notifications() {
        let mut feed = NotificationFeed::default();
        assert!(feed.ingest(vec![item("1")]).is_empty());
        assert_eq!(feed.unread(), 1);
        assert_eq!(feed.ingest(vec![item("1"), item("2")]), vec!["2"]);
        feed.mark_all_read();
        assert!(feed.ingest(vec![item("1"), item("2")]).is_empty());
        assert_eq!(feed.unread(), 0);
    }
    #[test]
    fn reading_one_thread_preserves_other_threads_and_toast_expiry_preserves_unread() {
        let mut feed = NotificationFeed::default();
        feed.ingest(vec![]);
        let mut a = item("1");
        a.peer = Some("mayor".into());
        a.message = Some(1);
        let mut b = item("2");
        b.peer = Some("fire".into());
        b.message = Some(2);
        feed.ingest(vec![a, b]);
        feed.dismiss_toast("2");
        assert_eq!(feed.unread(), 2);
        feed.read_thread("mayor", 1);
        assert_eq!(feed.unread(), 1);
    }
    fn nm(id: &str) -> String {
        match id {
            "fire" => "Vigili del Fuoco".into(),
            "mayor" => "Sindaco".into(),
            other => other.into(),
        }
    }
    fn at(id: &str) -> String {
        format!("Azione {id}")
    }
    fn spec(lang: &str, e: Value) -> Option<Spec> {
        i18n::with_ui_lang(lang, || log_spec(&e, &nm, &at))
    }
    #[test]
    fn log_events_get_specific_titles_in_both_languages() {
        let step = serde_json::json!({"kind":"turn_substep","clock":"Day 1 13:05","data":{"turn":1,"from_s":0,"to_s":300}});
        let s = spec("it", step.clone()).unwrap();
        assert_eq!(s.title, "Turno 1 · avanzamento a 13:05");
        assert_eq!(s.detail, "+5 min");
        assert!(!s.toast);
        assert_eq!(
            spec("en", step).unwrap().title,
            "Turn 1 · advanced to 13:05"
        );
        let inject = serde_json::json!({"kind":"inject_sent","data":{"title":"Incendio","recipients":["fire","mayor"]}});
        assert_eq!(
            spec("it", inject.clone()).unwrap().title,
            "Inject inviato a Vigili del Fuoco, Sindaco"
        );
        assert_eq!(
            spec("en", inject).unwrap().title,
            "Inject sent to Vigili del Fuoco, Sindaco"
        );
        let done = serde_json::json!({"kind":"timeline_action_executed","data":{"action":"a1","status":"done","result":"ok"}});
        let s = spec("it", done).unwrap();
        assert_eq!(s.title, "Azione pianificata eseguita: Azione a1");
        assert!(s.toast && s.priority == Priority::Info);
        let failed = serde_json::json!({"kind":"timeline_action_executed","data":{"action":"a1","status":"failed","result":"x"}});
        assert_eq!(spec("it", failed).unwrap().priority, Priority::Action);
    }
    #[test]
    fn bookkeeping_events_are_silent_and_unknown_ones_quiet() {
        for kind in [
            "proposal_status",
            "master_command",
            "settings",
            "turn_opened",
            "proposal_parsed",
        ] {
            assert!(
                spec("it", serde_json::json!({"kind":kind,"data":{}})).is_none(),
                "{kind}"
            );
        }
        let s = spec(
            "it",
            serde_json::json!({"kind":"brand_new_kind","clock":"13:00","data":{}}),
        )
        .unwrap();
        assert!(!s.toast);
        let draft = serde_json::json!({"kind":"player_draft_submitted","actor":"mayor","data":{"proposal":7}});
        assert!(spec("it", draft).is_none());
    }
    #[test]
    fn only_pending_proposals_ask_the_excon_for_action() {
        let p = serde_json::json!({"id":3,"status":"pending","original":"Evacuare","source":{"kind":"player","role":"mayor"}});
        let s = i18n::with_ui_lang("it", || pending_proposal_spec(&p, &nm)).unwrap();
        assert_eq!(s.title, "Proposta in attesa da Sindaco");
        assert_eq!(s.priority, Priority::Action);
        let approved = serde_json::json!({"id":3,"status":"approved","source":{"kind":"player","role":"mayor"}});
        assert!(pending_proposal_spec(&approved, &nm).is_none());
    }
    #[test]
    fn grouped_notices_replace_each_other_while_unread_and_quiet_ones_never_toast() {
        let mut feed = NotificationFeed::default();
        feed.ingest(vec![]);
        let step = |k: &str| item(k).quiet().group("progress:1".into());
        assert!(feed
            .ingest(vec![step("a"), step("b"), step("c")])
            .is_empty());
        assert_eq!(feed.items.len(), 1);
        assert!(feed.toasts.is_empty());
        feed.mark_all_read();
        feed.ingest(vec![step("d")]);
        assert_eq!(feed.items.len(), 2);
    }
    #[test]
    fn action_toasts_persist_info_toasts_expire_and_resolution_clears_them() {
        let mut feed = NotificationFeed::default();
        feed.ingest(vec![]);
        let pending = item("p").action().group("pending:proposal:1".into());
        assert!(feed.ingest(vec![pending, item("i")]).iter().eq(["i"]));
        assert_eq!(feed.toasts, vec!["p", "i"]);
        assert_eq!(feed.unread_actions(), 1);
        feed.read_view("chat");
        assert_eq!(feed.unread_actions(), 1);
        feed.resolve_group("pending:proposal:1");
        assert_eq!(feed.unread_actions(), 0);
        assert_eq!(feed.toasts, vec!["i"]);
    }
    #[test]
    fn toast_cap_drops_info_before_action() {
        let mut feed = NotificationFeed::default();
        feed.ingest(vec![]);
        feed.ingest(vec![item("a").action(), item("b"), item("c"), item("d")]);
        assert_eq!(feed.toasts, vec!["a", "c", "d"]);
    }
}
