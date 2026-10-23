//! Presentation-only notifications derived exclusively from the authorized API view.
use crate::{
    api::{list, text, window},
    i18n, App, Msg,
};
use serde_json::Value;
use std::collections::BTreeSet;
use yew::prelude::*;

#[derive(Clone, Debug)]
pub struct Notice {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub clock: String,
    pub view: String,
    pub peer: Option<String>,
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
            message: None,
            read: false,
        }
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
    pub fn read_thread(&mut self, peer: &str, max: u64) {
        for n in &mut self.items {
            if n.peer.as_deref() == Some(peer) && n.message.is_some_and(|id| id <= max) {
                n.read = true;
            }
        }
    }
    pub fn read_view(&mut self, view: &str) {
        for n in &mut self.items {
            if n.view == view && n.peer.is_none() {
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
    fn ingest(&mut self, candidates: Vec<Notice>) -> Vec<String> {
        let mut timers = Vec::new();
        for n in candidates {
            if !self.known.insert(n.key.clone()) {
                continue;
            }
            if self.initialized && !n.read {
                self.toasts.push(n.key.clone());
                timers.push(n.key.clone());
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
        if self.toasts.len() > 3 {
            self.toasts.drain(..self.toasts.len() - 3);
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
            let peer = if from == self.own {
                text(&m["to"])
            } else {
                from
            };
            let view = if master_view {
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
        let proposals = if master_view {
            list(&master["proposals"])
        } else {
            list(&state["proposals"])
        };
        for p in proposals {
            let status = text(&p["status"]);
            let view = if master_view { "timeline" } else { "map" };
            let mut n = Notice::new(
                format!("proposal:{}:{status}", p["id"]),
                format!(
                    "{} · {}",
                    tr(lang, "Proposta", "Proposal"),
                    i18n::status(lang, status)
                ),
                text(&p["original"]).into(),
                "",
                view,
            );
            n.read = visible && self.active == view;
            candidates.push(n);
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
            );
            n.read = visible && self.active == "function";
            candidates.push(n);
        }
        if !self.state.is_null()
            && (self.state["turn"] != state["turn"] || self.state["phase"] != state["phase"])
        {
            self.notifications.sequence += 1;
            let view = if master_view { "timeline" } else { "function" };
            let mut n = Notice::new(
                format!("turn:{}", self.notifications.sequence),
                i18n::tf(
                    lang,
                    "turn_phase",
                    &[
                        &state["turn"].to_string(),
                        i18n::phase(lang, text(&state["phase"])),
                    ],
                ),
                tr(
                    lang,
                    "Stato dell’esercitazione aggiornato",
                    "Exercise status updated",
                )
                .into(),
                text(&state["clock"]),
                view,
            );
            // Notify turn transitions even while reading the destination view.
            n.read = false;
            candidates.push(n);
        }
        if !self.state.is_null()
            && self.state["resource_accounting"] != state["resource_accounting"]
        {
            self.notifications.sequence += 1;
            let view = if master_view { "advanced" } else { "function" };
            let mut n = Notice::new(
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
            );
            n.read = visible && self.active == view;
            candidates.push(n);
        }
        // EXCON receives only its authorized operational log, never private player chat.
        if master_view {
            for event in list(&master["log"]).iter().filter(|e| {
                ![
                    "message",
                    "chat_source",
                    "clock_physical_events",
                    "resource_event",
                    "turn_opened",
                ]
                .contains(&text(&e["kind"]))
            }) {
                let kind = text(&event["kind"]);
                let view = "history";
                let mut n = Notice::new(
                    format!("log:{}", event["seq"]),
                    tr(lang, "Evento dell’esercitazione", "Exercise event").into(),
                    kind.replace('_', " "),
                    text(&event["clock"]),
                    view,
                );
                n.read = visible && self.active == view;
                candidates.push(n);
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
            <button type="button" class="notification-bell" data-notification-bell="true" aria-label={format!("{} · {} {}",tr(l,"Notifiche","Notifications"),unread,tr(l,"non lette","unread"))} aria-controls={format!("notification-panel-{}",self.own)} aria-expanded={self.notifications.open.to_string()} onclick={ctx.link().callback(|_|Msg::ToggleNotifications)}><span aria-hidden="true">{"🔔"}</span>if unread>0{<span class="unread" data-notification-count="true">{unread}</span>}</button>
            if self.notifications.open {<button type="button" class="notification-backdrop" tabindex="-1" aria-label={tr(l,"Chiudi notifiche","Close notifications")} onclick={ctx.link().callback(|_|Msg::ToggleNotifications)}/><section class="notification-panel" id={format!("notification-panel-{}",self.own)} data-notification-panel="true" role="region" aria-label={tr(l,"Notifiche","Notifications")} onkeydown={ctx.link().batch_callback(|e:KeyboardEvent|if e.key()=="Escape"{Some(Msg::ToggleNotifications)}else{None})}>
                <header><strong>{tr(l,"Notifiche","Notifications")}</strong><button type="button" data-mark-notifications-read="true" disabled={unread==0} onclick={ctx.link().callback(|_|Msg::ReadNotifications)}>{tr(l,"Segna tutte lette","Mark all read")}</button></header>
                if self.notifications.items.is_empty(){<p class="muted">{tr(l,"Nessuna notifica","No notifications")}</p>}
                <ol>{for self.notifications.items.iter().rev().map(|n|{let key=n.key.clone();html!{<li key={key.clone()}><button type="button" data-open-notification={key.clone()} class={classes!("notification-item",(!n.read).then_some("unread-event"))} onclick={ctx.link().callback(move |_|Msg::OpenNotification(key.clone()))}><strong>{&n.title}</strong><small>{&n.clock}</small><span>{&n.detail}</span></button></li>}})}</ol>
            </section>}
        </div>}
    }
    pub fn view_notification_toasts(&self, ctx: &Context<Self>) -> Html {
        let lang = self.lang();
        html! {<aside class="notification-toasts" aria-live="polite" aria-atomic="false">{for self.notifications.toasts.iter().filter_map(|key| self.notifications.items.iter().find(|n| &n.key == key)).map(|n|{
            let key=n.key.clone();let dismiss=key.clone();html!{<div class="notification-toast" data-notification-toast={key.clone()} key={key.clone()}><button type="button" data-open-notification={key.clone()} onclick={ctx.link().callback(move |_|Msg::OpenNotification(key.clone()))}><strong>{&n.title}</strong><span>{&n.detail}</span></button><button type="button" class="dismiss-toast" aria-label={tr(&lang,"Chiudi notifica","Dismiss notification")} onclick={ctx.link().callback(move |_|Msg::DismissToast(dismiss.clone()))}>{"×"}</button></div>}
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
}
