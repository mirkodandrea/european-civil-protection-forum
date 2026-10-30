#[cfg(feature = "standalone")]
mod editor3d;
mod action_form;
mod action_pick;
#[cfg(feature = "bench")]
mod bench;
mod advanced;
mod controllo;
mod excon_queue;
mod conv_viewer;
mod api;
mod chat;
mod clarify;
mod evaluator;
mod function;
mod history;
mod inject_panel;
mod i18n;
mod i18n_excon;
mod i18n_function;
mod i18n_help;
mod i18n_place;
mod i18n_lang;
mod i18n_lobby;
mod i18n_more;
mod label_layout;
mod i18n_review;
mod i18n_reply;
mod i18n_risposte;
mod pdf_view;
mod proposal_drafts;
mod place_pick;
mod player_view;
mod map;
mod map_legend;
mod map_layers;
#[cfg(not(feature = "standalone"))]
mod maptest;
mod notifications;
mod render;
mod risposte;
mod review_board;
mod situation;
mod staged_commands;
mod scenario_editor;
mod webmcp;
mod server_settings;
mod settings;
#[cfg(feature = "standalone")]
mod simbench;
#[cfg(feature = "standalone")]
mod standalone;
#[cfg(feature = "standalone")]
mod store;
mod tactical;
mod timeline;
mod timeline_plan;
mod widgets;

use api::{list, text, window};
use futures::future::AbortHandle;
use gloo_timers::callback::Interval;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub enum ActionKind {
    Ordinary,
    WorkSubmit,
    /// A new linked conversation: open it once the server created it.
    ConvStart,
    /// A message in a linked conversation: clear the composer.
    ConvSend,
    Store(String),
    Send(String, String),
    Download,
    /// Remove this form field once the request succeeded.
    ClearField(String),
    /// An inject send in its own slot: it neither blocks nor is blocked by
    /// other actions, and only one send per slot is in flight at a time.
    Inject { slot: String, then: Box<ActionKind> },
}
#[derive(Clone, PartialEq)]
pub struct Action {
    pub path: String,
    pub body: Option<Value>,
    pub kind: ActionKind,
}
impl Action {
    pub fn post(path: impl Into<String>, body: Value) -> Self {
        Self {
            path: path.into(),
            body: Some(body),
            kind: ActionKind::Ordinary,
        }
    }
    pub fn store(path: impl Into<String>, body: Option<Value>, key: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            body,
            kind: ActionKind::Store(key.into()),
        }
    }
    /// Runs this inject send in `slot`, beside other actions.
    pub fn in_slot(mut self, slot: impl Into<String>) -> Self {
        let then = std::mem::replace(&mut self.kind, ActionKind::Ordinary);
        self.kind = ActionKind::Inject {
            slot: slot.into(),
            then: Box::new(then),
        };
        self
    }
}
#[derive(Clone)]
pub struct Draft {
    pub reply_to: Option<u64>,
    pub inject_ref: Option<u64>,
    pub text: String,
    pub mode: String,
    pub template: String,
    pub title: String,
    pub sender: String,
    pub scope: String,
    /// Comma-joined role ids when `scope` is "custom".
    pub recipients: String,
    /// Actions of a custom inject (run when it is sent).
    pub actions: Vec<scenario::InjectAction>,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            reply_to: None,
            inject_ref: None,
            text: String::new(),
            mode: "inject".into(),
            template: String::new(),
            title: String::new(),
            sender: "EXCON".into(),
            scope: "conversation".into(),
            recipients: String::new(),
            actions: Vec::new(),
        }
    }
}
pub struct Bootstrap {
    access: Value,
    stat: Value,
    links: Value,
    catalogue: Value,
}
pub struct App {
    token: String,
    access: Value,
    stat: Value,
    state: Value,
    master: Value,
    links: Value,
    catalogue: Value,
    own: String,
    selected: String,
    active: String,
    page: String,
    error: String,
    notice: String,
    done: u64,
    busy: bool,
    /// Inject sends in flight, by slot (see `ActionKind::Inject`).
    pending: BTreeSet<String>,
    reading: bool,
    resolving_request: bool,
    /// Pre-advance review: (resolve body, target second) of an advance that
    /// crosses unsent injects or pre-actions.
    due_modal: Option<(Value, i64)>,
    /// Report of the last advance run from this console, for the turn panel.
    last_report: Value,
    /// EXCON console language; stored in this browser only.
    excon_lang: String,
    /// The language gate was answered in this browser session.
    lang_chosen: bool,
    refresh_again: bool,
    drafts: BTreeMap<String, Draft>,
    /// Debounce of the automatic action suggestions on the narrative.
    seen: BTreeMap<String, u64>,
    fields: BTreeMap<String, String>,
    responses: BTreeMap<String, Value>,
    socket: Option<AbortHandle>,
    poll: Option<Interval>,
    presence_poll: Option<Interval>,
    peer_typing: bool,
    filter: String,
    inject_open: bool,
    notifications: notifications::NotificationFeed,
}
/// Focuses the first element matching `selector`, retrying every ~16 ms
/// (up to `attempts_left` times) until Yew has rendered the modal.
fn focus_when_rendered(selector: &'static str, attempts_left: u32) {
    use wasm_bindgen::JsCast;
    let focused = window()
        .document()
        .and_then(|d| d.query_selector(selector).ok().flatten())
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|el| el.focus().is_ok())
        .unwrap_or(false);
    if !focused && attempts_left > 0 {
        gloo_timers::callback::Timeout::new(16, move || {
            focus_when_rendered(selector, attempts_left - 1)
        })
        .forget();
    }
}
pub enum Msg {
    Boot(Result<Bootstrap, String>),
    Refresh,
    Refreshed(Result<(Value, Value), String>),
    ResolutionDone(Result<Value, String>),
    /// Advance to simulated second `.1` with resolve body `.0` (intermediate
    /// advance or turn close), reviewing what it crosses first.
    Advance(Value, i64),
    CloseDue,
    Action(Action),
    Done(ActionKind, Result<Value, String>),
    Select(String),
    ReplyChat(u64),
    CancelChatReply,
    ChatInject(u64),
    ToggleNotifications,
    ReadNotifications,
    OpenNotification(String),
    DismissToast(String),
    ExconLang(String),
    /// Language chosen in the start-of-session gate.
    PickLang(String),
    /// Static view reloaded in the player's language.
    StaticLoaded(Result<Value, String>),
    OpenInject(String),
    /// Open the drill-down of a response row (empty: back to the list).
    OpenInjectDetail(String),
    OpenWork(Option<String>),
    /// Answer the inject delivery `.0` from the linked conversation `.1`.
    OpenWorkConv(String, String),
    CloseInject,
    View(String),
    Field(String, String),
    Filter(String),
    ClearError,
    PresenceCheck,
    PresenceUpdated(Result<Value, String>),
    PresenceSent,
}
impl App {
    fn is_master(&self) -> bool {
        self.own == "master"
    }
    fn is_evaluator(&self) -> bool {
        self.page == "evaluator"
    }
    /// UI language: the player's selected language, the EXCON's browser choice; the evaluator stays Italian.
    fn lang(&self) -> String {
        match text(&self.state["player"]["language"]) {
            _ if self.is_evaluator() => "it".into(),
            _ if self.is_master() => self.excon_lang.clone(),
            l if !l.is_empty() => l.to_string(),
            _ => "it".into(),
        }
    }
    fn draft(&self) -> Draft {
        self.drafts.get(&self.selected).cloned().unwrap_or_default()
    }
    /// Planned injects still unsent whose instant an advance to `to_s` reaches or crosses.
    fn has_unsent_before(&self, to_s: i64) -> bool {
        let a = &self.master["agenda"];
        let (now, start) = (a["now_s"].as_i64().unwrap_or(0), a["turn_start_s"].as_i64().unwrap_or(0));
        !timeline::unsent_injects(list(&self.master["planned_injects"]), now, to_s, start).is_empty()
    }
    /// Pre-actions an advance to `to_s` runs by itself on the way.
    fn has_pre_actions_before(&self, to_s: i64) -> bool {
        let now = self.master["agenda"]["now_s"].as_i64().unwrap_or(0);
        !timeline::crossed_pre_actions(list(&self.master["planned_injects"]), now, to_s).is_empty()
    }
    /// The player's working screen, drawn from this console's state.
    fn player_view(&self, ctx: &Context<Self>) -> player_view::PlayerView {
        player_view::PlayerView {
            state: self.state.clone(),
            stat: self.stat.clone(),
            own: self.own.clone(),
            fields: self.fields.clone(),
            busy: self.busy,
            error: self.error.clone(),
            active: self.active.clone(),
            roles: self.roles(),
            maps: true,
            link: ctx.link().callback(|m| m),
        }
    }
    /// Injects of the current turn that wait for the player (nav badge).
    fn inject_todo(&self, ctx: &Context<Self>) -> usize {
        self.player_view(ctx).inject_todo()
    }
    fn roles(&self) -> Vec<Value> {
        let mut roles = list(&self.stat["roles"]).to_vec();
        for contact in list(&self.stat["contacts"]) {
            if !roles.iter().any(|r| r["id"] == contact["id"]) {
                roles.push(contact.clone())
            }
        }
        roles
    }
    fn name(&self, id: &str) -> String {
        if id == "master" {
            // Players never read "EXCON": exercise control speaks as a persona.
            if self.is_master() || self.is_evaluator() || text(&self.state["default_persona"]).is_empty() {
                "EXCON".into()
            } else {
                text(&self.state["default_persona"]).into()
            }
        } else {
            self.roles()
                .iter()
                .find(|r| r["id"] == id)
                .map(|r| text(&r["name"]).to_string())
                .unwrap_or(id.into())
        }
    }
    fn action(&self, ctx: &Context<Self>) -> Callback<Action> {
        ctx.link().callback(Msg::Action)
    }
    fn contacts(&self) -> Vec<Value> {
        if self.is_master() {
            self.roles()
        } else {
            self.roles()
                .into_iter()
                .filter(|r| r["id"] != self.own)
                .collect()
        }
    }
    fn messages(&self) -> &[Value] {
        if self.is_master() {
            list(&self.master["messages"])
        } else {
            list(&self.state["inbox"])
        }
    }
    fn in_thread(&self, m: &Value, id: &str) -> bool {
        if self.is_master() {
            return m["from"] == id || m["to"] == id;
        }
        m["kind"] == "chat"
            && ((m["from"] == self.own && m["to"] == id)
                || (m["to"] == self.own && m["from"] == id))
    }

    fn mark_seen(&mut self) {
        if self.panel_visible() {
            self.notifications.read_view(&self.active);
        }
        if self.active == "chat" && self.panel_visible() {
            let max = self
                .messages()
                .iter()
                .filter(|m| self.in_thread(m, &self.selected))
                .filter_map(|m| m["id"].as_u64())
                .max()
                .unwrap_or(0);
            self.seen.insert(self.selected.clone(), max);
            self.notifications.read_thread(&self.selected, max);
        }
    }
    fn request_refresh(&mut self, ctx: &Context<Self>) {
        if self.own.is_empty() {
            return;
        }
        if self.reading {
            self.refresh_again = true;
            return;
        }
        self.reading = true;
        let token = self.token.clone();
        let own = self.own.clone();
        let evaluator = self.is_evaluator();
        let query = self.filter.clone();
        ctx.link().send_future(async move {
            let result = if evaluator {
                let path = format!("/api/evaluator?{query}");
                let (state, evidence) = futures::join!(
                    api::request(&token, "/api/state", None),
                    api::request(&token, &path, None)
                );
                state.and_then(|s| evidence.map(|m| (s, m)))
            } else if own == "master" {
                let (state, master) = futures::join!(
                    api::request(&token, "/api/state", None),
                    api::request(&token, "/api/master", None)
                );
                state.and_then(|s| master.map(|m| (s, m)))
            } else {
                api::request(&token, &format!("/api/player/{}", api::encode(&own)), None)
                    .await
                    .map(|s| (s, Value::Null))
            };
            Msg::Refreshed(result)
        });
    }
}
#[derive(Properties, PartialEq, Default)]
pub struct AppProps {
    /// Refresh unread state when the standalone role switcher reveals this panel.
    #[prop_or(true)]
    pub visible: bool,
    #[prop_or_default]
    pub token: Option<String>,
    #[prop_or_default]
    pub role: Option<String>,
    #[prop_or_default]
    pub page: Option<String>,
}
impl Component for App {
    type Message = Msg;
    type Properties = AppProps;
    fn create(ctx: &Context<Self>) -> Self {
        #[cfg(feature = "standalone")]
        let token = ctx.props().role.clone().unwrap_or_else(|| "master".into());
        #[cfg(not(feature = "standalone"))]
        let token = ctx.props().token.clone().unwrap_or_else(api::token);
        let key = token.clone();
        // Players confirm their language on the exercise (`language_confirmed`,
        // asked again for every new exercise, standalone included). The
        // standalone EXCON picks its own in the launcher; on the server the
        // EXCON choice is per browser session and asked again after a restart.
        let lang_chosen = cfg!(feature = "standalone") || session_flag(&format!("ttx_lang_ok_{token}"));
        ctx.link().send_future(async move {
            let result = async {
                let access = api::request(&key, "/api/access", None).await?;
                if access["kind"] == "master" {
                    // The server translates player messages to the EXCON language.
                    let lang = web_sys::window()
                        .and_then(|w| w.local_storage().ok().flatten())
                        .and_then(|s| s.get_item("ttx_excon_lang").ok().flatten())
                        .filter(|l| api::EXCON_LANGUAGES.iter().any(|(id, _)| id == l))
                        .unwrap_or_else(|| "it".into());
                    let _ = api::request(&key, "/api/player/master/language", Some(json!({"language": lang}))).await;
                }
                let stat = api::request(&key, "/api/static", None).await?;
                let links = if access["kind"] == "master" {
                    api::request(&key, "/api/master/access", None).await?
                } else {
                    Value::Null
                };
                let catalogue = if access["kind"] == "master" {
                    api::request(&key, "/api/catalogue", None).await?
                } else {
                    Value::Null
                };
                Ok(Bootstrap {
                    access,
                    stat,
                    links,
                    catalogue,
                })
            }
            .await;
            Msg::Boot(result)
        });
        let path = window().location().pathname().unwrap_or_default();
        Self {
            token,
            access: Value::Null,
            stat: Value::Null,
            state: Value::Null,
            master: Value::Null,
            links: Value::Null,
            catalogue: Value::Null,
            own: String::new(),
            selected: String::new(),
            active: "chat".into(),
            page: ctx.props().page.clone().unwrap_or_else(|| {
                if path.ends_with("advanced.html") {
                    "advanced"
                } else if path.ends_with("evaluator.html") {
                    "evaluator"
                } else if path == "/" || path.ends_with("index.html") {
                    "lobby"
                } else {
                    "chat"
                }
                .into()
            }),
            error: String::new(),
            notice: String::new(),
            done: 0,
            busy: false,
            pending: BTreeSet::new(),
            reading: false,
            resolving_request: false,
            due_modal: None,
            last_report: Value::Null,
            excon_lang: web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|s| s.get_item("ttx_excon_lang").ok().flatten())
                .filter(|l| api::EXCON_LANGUAGES.iter().any(|(id, _)| id == l))
                .unwrap_or_else(|| "it".into()),
            lang_chosen,
            refresh_again: false,
            drafts: BTreeMap::new(),
            seen: BTreeMap::new(),
            fields: BTreeMap::new(),
            responses: BTreeMap::new(),
            socket: None,
            poll: None,
            presence_poll: None,
            peer_typing: false,
            filter: String::new(),
            inject_open: false,
            notifications: notifications::NotificationFeed::default(),
        }
    }
    fn update(&mut self, ctx: &Context<Self>, msg: Msg) -> bool {
        match msg {
            Msg::Boot(Ok(b)) => {
                self.access = b.access;
                self.stat = b.stat;
                self.links = b.links;
                self.catalogue = b.catalogue;
                self.own = if self.access["kind"] == "master" {
                    "master".into()
                } else if self.access["kind"] == "evaluator" {
                    "evaluator".into()
                } else {
                    text(&self.access["role"]).into()
                };
                if self.own == "evaluator" {
                    self.page = "evaluator".into()
                } else if self.is_master() {
                    self.active = "timeline".into();
                } else if !self.is_master() {
                    self.page = "chat".into();
                    self.active = "function".into();
                    if !cfg!(feature = "standalone") {
                        let _ = window()
                            .document()
                            .unwrap()
                            .body()
                            .unwrap()
                            .set_attribute("class", "player-ui");
                    }
                }
                self.selected = self
                    .contacts()
                    .first()
                    .map(|r| text(&r["id"]).into())
                    .unwrap_or_default();
                self.socket = Some(api::live(
                    self.token.clone(),
                    ctx.link().callback(|()| Msg::Refresh),
                ));
                let link = ctx.link().clone();
                self.poll = Some(Interval::new(10000, move || {
                    link.send_message(Msg::Refresh)
                }));
                let link = ctx.link().clone();
                self.presence_poll = Some(Interval::new(1800, move || {
                    link.send_message(Msg::PresenceCheck)
                }));
                self.request_refresh(ctx);
            }
            Msg::Boot(Err(e)) => self.error = e,
            Msg::Refresh => {
                self.request_refresh(ctx);
                return false;
            }
            Msg::PresenceCheck => {
                if self.is_master() || self.is_evaluator() || self.selected == "master" {
                    return false;
                }
                let token = self.token.clone();
                let path = format!(
                    "/api/player/{}/presence?peer={}",
                    self.own,
                    api::encode(&self.selected)
                );
                ctx.link().send_future(async move {
                    Msg::PresenceUpdated(api::request(&token, &path, None).await)
                });
                return false;
            }
            Msg::PresenceUpdated(result) => {
                self.peer_typing = result.is_ok_and(|v| v["typing"] == true);
                return true;
            }
            Msg::PresenceSent => return false,
            Msg::Refreshed(result) => {
                self.reading = false;
                match result {
                    Ok((state, master)) => {
                        self.update_notifications(ctx, &state, &master);
                        // A restarted exercise (back to the briefing) asks the EXCON
                        // console language again: someone else may run it now.
                        // Standalone picks it in the launcher.
                        let restarted = !self.state.is_null() && self.state["phase"] != "briefing" && state["phase"] == "briefing";
                        if restarted && self.is_master() && !cfg!(feature = "standalone") {
                            if let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
                                let _ = s.remove_item(&format!("ttx_lang_ok_{}", self.token));
                            }
                            self.lang_chosen = false;
                        }
                        self.state = state;
                        // From now on the server's `language_confirmed` decides the gate
                        // (a deleted session asks again).
                        if !self.is_master() {
                            self.lang_chosen = false;
                        }
                        self.master = master;
                        self.mark_seen();
                    }
                    Err(e) => self.error = e,
                }
                if self.refresh_again {
                    self.refresh_again = false;
                    self.request_refresh(ctx);
                }
            }
            Msg::Action(action) => {
                // A live turn must not block EXCON injects or other independent controls.
                if action.path == "/api/master/resolve" {
                    if self.resolving_request || self.busy {
                        return false;
                    }
                    self.resolving_request = true;
                    let key = self.token.clone();
                    ctx.link().send_future(async move {
                        Msg::ResolutionDone(api::request(&key, &action.path, action.body).await)
                    });
                    return true;
                }
                if let ActionKind::Inject { slot, .. } = &action.kind {
                    if !self.pending.insert(slot.clone()) {
                        return false;
                    }
                } else {
                    if self.busy {
                        return false;
                    }
                    self.busy = true;
                    self.notice = match &action.kind {
                        ActionKind::Send(_, _) => {
                            crate::i18n::ex("Traduzione e invio in corso…")
                        }
                        _ => crate::i18n::ex("Elaborazione…"),
                    }
                    .into();
                }
                self.error.clear();
                let key = self.token.clone();
                ctx.link().send_future(async move {
                    let body = action.body.clone();
                    let mut r = api::request(&key, &action.path, action.body).await;
                    if matches!(&action.kind, ActionKind::Store(_)) {
                        if let Ok(value) = &mut r {
                            if value.is_object() {
                                value["_request"] = body.unwrap_or(Value::Null);
                            }
                        }
                    }
                    Msg::Done(action.kind, r)
                });
            }
            Msg::Advance(body, to_s) => {
                if self.resolving_request || self.busy {
                    return false;
                }
                self.error.clear();
                // Nothing unsent reached or crossed and no pre-action on the way: go.
                if !self.has_unsent_before(to_s) && !self.has_pre_actions_before(to_s) {
                    ctx.link().send_message(Msg::Action(Action::post("/api/master/resolve", body)));
                    return false;
                }
                self.due_modal = Some((body, to_s));
            }
            Msg::CloseDue => self.due_modal = None,
            Msg::ResolutionDone(result) => {
                self.resolving_request = false;
                match result {
                    Ok(report) => {
                        self.notice = crate::i18n::ex("Turno risolto").into();
                        self.last_report = report;
                    }
                    Err(e) => self.error = e,
                }
                self.request_refresh(ctx);
            }
            Msg::Done(kind, result) => {
                self.done += 1;
                let (kind, slot) = match kind {
                    ActionKind::Inject { slot, then } => (*then, Some(slot)),
                    other => (other, None),
                };
                match slot {
                    Some(slot) => {
                        self.pending.remove(&slot);
                    }
                    None => self.busy = false,
                }
                match result {
                    Err(e) => {
                        self.error = e;
                        self.push_failure_notification(ctx);
                        self.notice = crate::i18n::ex("Operazione non riuscita").into()
                    }
                    Ok(value) => {
                        self.notice = crate::i18n::ex("Operazione completata").into();
                        // Say what the simulator did (e.g. "population of F4 warned at 14:20 …").
                        let notes = list(&value["results"])
                            .iter()
                            .filter(|r| r["ok"] == true && r["queued"].is_null())
                            .flat_map(|r| list(&r["notes"]).iter().map(|n| crate::render::localize_note(text(n))).collect::<Vec<_>>())
                            .filter(|n| !n.is_empty())
                            .collect::<Vec<_>>();
                        if !notes.is_empty() {
                            self.notice = format!("{} · {}", self.notice, notes.join(" · "));
                        }
                        let failures = list(&value["results"])
                            .iter()
                            .filter(|r| r["ok"] == false)
                            .map(|r| text(&r["error"]))
                            .collect::<Vec<_>>();
                        if !failures.is_empty() {
                            self.error = failures.join(" · ");
                            self.notice =
                                crate::i18n::ex("Operazione con errori: consulta gli esiti").into();
                        }
                        if self.is_master() && matches!(&kind, ActionKind::Send(_, _)) {
                            self.inject_open = false;
                        }
                        match kind {
                            ActionKind::Store(key) => {
                                self.notice = match key.as_str() {
                                    "forecast" => {
                                        crate::i18n::ex("Bozza privata pronta · non pubblicata")
                                    }
                                    "assessment" => crate::i18n::ex(
                                        "Valutazione derivata pronta · richiede revisione umana",
                                    ),
                                    _ => crate::i18n::ex(
                                        "Anteprima pronta · nessuna modifica applicata",
                                    ),
                                }
                                .into();
                                self.responses.insert(key, value);
                            }
                            ActionKind::Send(to, sent) => {
                                self.notice = if value["translation_failed"] == true {
                                    crate::i18n::ex("Inviato · traduzione non disponibile")
                                } else {
                                    "Inviato"
                                }
                                .into();
                                let d = self.drafts.entry(to).or_default();
                                if d.text == sent {
                                    *d = Draft::default();
                                }
                            }
                            ActionKind::Download => {
                                if let Err(e) = api::download(&value) {
                                    self.error = e
                                }
                            }
                            ActionKind::WorkSubmit => {
                                player_view::response_sent(&mut self.fields);
                                self.notice = if value["translation_failed"]==true {crate::i18n::ex("Inviato · traduzione non disponibile")}else{crate::i18n::ex("Risposta inviata")}.into();
                            }
                            ActionKind::ConvStart => {
                                player_view::conversation_started(&mut self.fields, value["conversation"].as_u64());
                                self.notice = crate::i18n::ex("Inviato").into();
                            }
                            ActionKind::ConvSend => {
                                player_view::conversation_sent(&mut self.fields);
                                self.notice = crate::i18n::ex("Inviato").into();
                            }
                            ActionKind::ClearField(key) => {
                                self.fields.remove(&key);
                            }
                            ActionKind::Ordinary | ActionKind::Inject { .. } => {}
                        }
                    }
                }
                self.request_refresh(ctx);
            }
            Msg::OpenWorkConv(parent, conversation) => {
                player_view::open_work_conv(&mut self.fields, &self.state, &self.own, parent, conversation);
                self.error.clear();
                focus_when_rendered("#response-narrative", 10);
            }
            Msg::OpenWork(parent) => {
                let parent = parent
                    .or_else(|| self.latest_inject().map(|m| m["id"].to_string()))
                    .unwrap_or_default();
                player_view::open_work(&mut self.fields, parent);
                self.error.clear();
                focus_when_rendered("#response-narrative", 10);
            }
            Msg::OpenInjectDetail(key) => player_view::open_inject_detail(&mut self.fields, key),
            Msg::OpenInject(id) => {
                self.selected = id;
                self.inject_open = true;
                self.error.clear();
                focus_when_rendered("#inject-title", 10);
            }
            Msg::PickLang(l) => {
                let key = format!("ttx_lang_ok_{}", self.token);
                if let Some(s) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
                    let _ = s.set_item(&key, "1");
                }
                self.lang_chosen = true;
                if self.is_master() {
                    ctx.link().send_message(Msg::ExconLang(l));
                } else {
                    // Static names (roles, resources) are localised by the server: reload them
                    // once the language is set.
                    let (token, own) = (self.token.clone(), self.own.clone());
                    ctx.link().send_future(async move {
                        let _ = api::request(&token, &format!("/api/player/{own}/language"), Some(json!({"language": l}))).await;
                        Msg::StaticLoaded(api::request(&token, "/api/static", None).await)
                    });
                }
            }
            Msg::StaticLoaded(Ok(stat)) => {
                self.stat = stat;
                ctx.link().send_message(Msg::Refresh);
            }
            Msg::StaticLoaded(Err(_)) => ctx.link().send_message(Msg::Refresh),
            Msg::ExconLang(l) => {
                if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
                    let _ = s.set_item("ttx_excon_lang", &l);
                }
                self.excon_lang = l;
                // Child views are cached; reloading re-renders every string.
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
            }
            Msg::CloseInject => {
                self.inject_open = false;
            }
            Msg::ChatInject(id) => {
                if self
                    .messages()
                    .iter()
                    .any(|m| m["id"] == id && m["kind"] == "inject" && m["to"] == self.own)
                {
                    let draft = self.drafts.entry(self.selected.clone()).or_default();
                    draft.inject_ref = Some(id);
                    draft.reply_to = None;
                }
            }
            Msg::ReplyChat(id) => {
                if self.messages().iter().any(|m| {
                    m["id"] == id && self.in_thread(m, &self.selected) && m["kind"] == "chat"
                }) {
                    self.drafts
                        .entry(self.selected.clone())
                        .or_default()
                        .reply_to = Some(id);
                    focus_when_rendered("#chat-view:not([hidden]) #text", 10);
                }
            }
            Msg::CancelChatReply => {
                self.drafts
                    .entry(self.selected.clone())
                    .or_default()
                    .reply_to = None
            }
            Msg::ToggleNotifications => self.notifications.open = !self.notifications.open,
            Msg::ReadNotifications => {
                self.notifications.mark_all_read();
                // The explicit action also clears conversation unread badges.
                for contact in self.contacts() {
                    let id = text(&contact["id"]).to_string();
                    let max = self
                        .messages()
                        .iter()
                        .filter(|m| self.in_thread(m, &id))
                        .filter_map(|m| m["id"].as_u64())
                        .max()
                        .unwrap_or(0);
                    self.seen.insert(id, max);
                }
            }
            Msg::OpenNotification(key) => {
                if let Some(item) = self.notifications.open_item(&key) {
                    self.active = item.view;
                    if let Some(peer) = item.peer {
                        self.selected = peer;
                    }
                    self.mark_seen();
                }
            }
            Msg::DismissToast(key) => self.notifications.dismiss_toast(&key),
            Msg::Select(id) => {
                self.selected = id;
                self.peer_typing = false;
                self.mark_seen();
            }
            Msg::View(view) => {
                self.notifications.open = false;
                self.active = view;
                self.mark_seen();
            }
            Msg::Field(key, value) => {
                if key.starts_with("draft:") {
                    let selected = self.selected.clone();
                    let own = self.own.clone();
                    let player_chat = !self.is_master() && selected != "master";
                    let d = self.drafts.entry(selected.clone()).or_default();
                    match key.as_str() {
                        "draft:text" => {
                            d.text = value;
                            if player_chat {
                                let token = self.token.clone();
                                let path = format!("/api/player/{own}/presence");
                                let body =
                                    json!({"to":selected,"typing":!d.text.trim().is_empty()});
                                ctx.link().send_future(async move {
                                    let _ = api::request(&token, &path, Some(body)).await;
                                    Msg::PresenceSent
                                });
                            }
                        }
                        "draft:mode" => d.mode = value,
                        "draft:title" => d.title = value,
                        "draft:sender" => {
                            d.sender = if value.trim().is_empty() { "EXCON".into() } else { value }
                        }
                        "draft:scope" => d.scope = value,
                        "draft:recipients" => d.recipients = value,
                        "draft:actions" => d.actions = serde_json::from_str(&value).unwrap_or_default(),
                        "draft:template" => {
                            d.template = value.clone();
                            if let Some(t) =
                                list(&self.stat["msel"]).iter().find(|t| t["id"] == value)
                            {
                                d.text = text(&t["text"]).into();
                                d.title = text(&t["title"]).into();
                                d.sender = text(&t["sender"]).into();
                            } else {
                                d.text.clear();
                                d.title.clear();
                                d.sender = "EXCON".into()
                            }
                            d.scope = "conversation".into();
                        }
                        _ => {}
                    }
                } else {
                    self.fields.insert(key, value);
                }
            }
            Msg::Filter(query) => {
                self.filter = query;
                self.request_refresh(ctx);
            }
            Msg::ClearError => self.error.clear(),
        }
        // Hidden role panels do not re-render: their labels would otherwise be
        // drawn under another panel's language (see `set_ui_lang_if_visible`).
        ctx.props().visible
    }
    fn view(&self, ctx: &Context<Self>) -> Html {
        i18n::set_ui_lang_if_visible(ctx.props().visible, &self.lang());
        if self.access.is_null() {
            #[cfg(feature = "standalone")]
            return html! {<main class="page"><section class="card"><h1>{"PPRD East 4 · TTX"}</h1><p>{if self.error.is_empty(){crate::i18n::ex("Connessione all’esercitazione…")}else{&self.error}}</p></section></main>};
            #[cfg(not(feature = "standalone"))]
            {
                return html! {<main class="page"><section class="card"><h1>{"PPRD East 4 · TTX"}</h1><p>{if self.error.is_empty(){crate::i18n::ex("Connessione all’esercitazione…")}else{&self.error}}</p></section></main>};
            }
        }
        let title = if self.is_evaluator() {
            crate::i18n::ex("Valutatore · sola lettura").into()
        } else if self.is_master() {
            crate::i18n::ex("Direzione esercitazione").into()
        } else {
            self.name(&self.own)
        };
        let phase = text(&self.state["phase"]);
        // Before the exercise starts the EXCON screens stay open, but their actions are disabled.
        let gated = self.is_master() && !self.is_evaluator() && phase == "briefing";
        let nav = |id: &str, label: &str| {
            let id = id.to_string();
            let active = self.active == id;
            let unread = if id == "chat" && !self.is_master() {
                self.chat_unread()
            } else if id == "map" && !self.is_master() && !self.is_evaluator() {
                self.inject_todo(ctx)
            } else if id == "risposte" && self.is_master() {
                // Legacy approvals, plus what the reply review waits on the EXCON for.
                let c = &self.master["reply_review"]["counts"];
                risposte::pending_count(&self.master) + (c["complete"].as_u64().unwrap_or(0) + c["drafts"].as_u64().unwrap_or(0)) as usize
            } else {
                0
            };
            html! {<button class={if active{"active"}else{""}} data-view={id.clone()} onclick={ctx.link().callback(move |_|Msg::View(id.clone()))}>{label}if unread>0{<span class="unread" data-chat-unread="true">{unread}</span>}</button>}
        };
        let l = self.lang();
        let l = l.as_str();
        let href = |path: &str| path.to_string();
        // Participants still choosing a language hold the start; the EXCON may override.
        let waiting: Vec<String> = self.state["players"].as_object().map(|players| players.values().filter(|p| p["language_confirmed"] == false).map(|p| self.name(text(&p["role"]))).collect()).unwrap_or_default();
        let time_controls = html! {<div id="turn-controls" class="row"><button id="start" class="primary" hidden={phase!="briefing"} disabled={self.busy||!waiting.is_empty()} onclick={ctx.link().callback(|_|Msg::Action(Action::post("/api/master/start",json!({}))))}>{i18n::t(l,"start_exercise")}</button>
            if phase=="briefing"&&!waiting.is_empty(){<span id="language-waiting" class="muted" role="status">{crate::exfmt!("In attesa della lingua: {}","Waiting for the language of: {}",waiting.join(", "))}</span><button id="start-anyway" disabled={self.busy} onclick={ctx.link().callback(|_|Msg::Action(Action::post("/api/master/start",json!({"force":true}))))}>{crate::i18n::ex("Avvia comunque")}</button>}
        </div>};
        let running = phase == "resolving" || self.state["clock_control"]["running"] == true;
        // On the Timeline the turn panel owns status and time: the header keeps one indicator.
        let compact = self.is_master() && !self.is_evaluator() && self.page == "chat" && self.active == "timeline" && phase != "briefing";
        let sim_badge = if compact || self.state.is_null() || phase == "briefing" || phase == "debrief" {
            Html::default()
        } else if running {
            html! {<span class="pill sim-state running" id="sim-state" role="status">{i18n::t(l,"sim_running")}</span>}
        } else {
            html! {<span class="pill sim-state paused" id="sim-state" role="status">{i18n::t(l,if phase=="orders"{"sim_paused_orders"}else{"sim_paused"})}</span>}
        };
        let lang_gate = if self.is_master() {
            !self.lang_chosen
        } else {
            !self.lang_chosen && !self.state.is_null() && self.state["player"]["language_confirmed"] == false
        };
        if lang_gate && !self.own.is_empty() && !self.is_evaluator() && self.page != "lobby" && self.page != "advanced" {
            let options = if self.is_master() { api::EXCON_LANGUAGES } else { api::LANGUAGES };
            // The scenario's language for this role is the proposed one.
            let proposed = if self.is_master() { self.excon_lang.clone() } else { text(&self.state["player"]["language"]).to_string() };
            let buttons = html! {<div class="row" id="language-gate">{for options.iter().map(|(id, name)| { let id = id.to_string(); html!{<button class={classes!((id==proposed).then_some("primary"))} data-lang={id.clone()} onclick={ctx.link().callback(move |_| Msg::PickLang(id.clone()))}>{*name}</button>} })}</div>};
            // Standalone: one tab per role, so the question stays inside the role's
            // panel (a full-page modal would hide the role tabs).
            if cfg!(feature = "standalone") {
                return html! {<main class="page"><section class="card" id="await-start" aria-labelledby="lang-gate-title">
                    <h2 id="lang-gate-title">{format!("{} · Language / Lingua", self.name(&self.own))}</h2>{buttons}
                </section></main>};
            }
            return html! {<div class="modal-backdrop"><section class="inject-modal" role="dialog" aria-modal="true" aria-labelledby="lang-gate-title">
                <h2 id="lang-gate-title">{"Language / Lingua"}</h2>{buttons}
            </section></div>};
        }
        html! {<>
            <header class="topbar" id={format!("app-header-{}",self.own)}><div class="brand"><div class="eyebrow">{"PPRD East 4 · TTX"}</div><h1>{title}</h1></div><div><span class="pill" id="turn">{i18n::tf(l,"turn_phase",&[&self.state["turn"].to_string(),i18n::phase(l,phase)])}</span>{sim_badge}if !compact{<div class="clock" id="clock">{text(if self.is_master()||self.is_evaluator(){&self.state["world"]["stats"]["clock"]}else{&self.state["clock"]})}{i18n::t(l,"clock_note")}</div>}</div><div class="grow"/>
            if self.is_master()&&!self.is_evaluator()&&self.page!="history" {{time_controls.clone()}}
            {self.view_notification_bell(ctx)}
            </header>

            <nav class="navigation" aria-label={i18n::t(l,"views")}>
            if self.page=="chat" {if self.is_master(){ {nav("timeline",i18n::t(l,"nav_timeline"))} {nav("situazione",crate::i18n::ex("Situazione"))} {nav("risposte",crate::i18n::ex("Risposte"))} {nav("map",crate::i18n::ex("Controllo"))} }else{{nav("function",i18n::t(l,"nav_function"))} {nav("map",i18n::t(l,"nav_injects"))}}}
            <div class="grow"/>
            if self.page=="chat"&&self.is_master(){<div class="mgmt-tabs" role="group" aria-label="Management"><span class="mgmt-label">{"Management"}</span>{nav("history",i18n::t(l,"nav_history"))}{nav("scenario",crate::i18n::ex("Impostazioni"))}</div>}
            if self.is_master()&&!cfg!(feature="standalone"){<a href={href("master.html")}>{i18n::t(l,"nav_timeline")}</a><a id="advanced" href={href("advanced.html")}>{i18n::t(l,"tools_link")}</a>}
            </nav>
            {self.view_notification_toasts(ctx)}
            <div id="error" class="error" role="alert">{&self.error}</div>
            if (self.page=="advanced"||self.active=="advanced"||(self.is_master()&&self.active=="map")||self.is_evaluator())&&!self.notice.is_empty(){<div class="notice" role="status">{&self.notice}</div>}
            if self.state.is_null(){<p class="page">{i18n::t(l,"loading")}</p>}
            else if self.is_evaluator(){<evaluator::Evaluator stat={self.stat.clone()} state={self.state.clone()} evidence={self.master.clone()} assessment={self.responses.get("assessment").cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)} onfilter={ctx.link().callback(Msg::Filter)}/>}
            else if self.is_master()&&(self.page=="history"||self.active=="history"){<history::History master={self.master.clone()} roles={self.roles()}/> }
            else if self.page=="advanced"&&self.is_master(){<controllo::Controllo stat={self.stat.clone()} state={self.state.clone()} master={self.master.clone()} links={self.links.clone()} catalogue={self.catalogue.clone()} responses={self.responses.clone()} busy={self.busy} error={self.error.clone()} done={self.done} action={self.action(ctx)}/>}
            else if self.page=="lobby"{<main class="page"><section class="card"><h2>{text(&self.stat["name"])}</h2><p>{text(&self.stat["description"])}</p><p><a href={href("master.html")}>{crate::i18n::ex("Apri la console conversazionale")}</a></p>{widgets::participant_links(&self.links,&self.roles())}</section></main>}
            else if self.is_master()&&self.active=="timeline"{<main class="page timeline-page"><timeline::ExconTimeline master={self.master.clone()} state={self.state.clone()} stat={self.stat.clone()} responses={self.responses.clone()} last_report={self.last_report.clone()} action={self.action(ctx)} busy={self.busy||gated} pending={self.pending.clone()} oninject={ctx.link().callback(Msg::OpenInject)} onview={ctx.link().callback(Msg::View)} onadvance={ctx.link().callback(|(b,t)|Msg::Advance(b,t))}/></main>}
            else if self.is_master()&&self.active=="scenario"{<scenario_editor::ScenarioEditor revision={self.state["generation"].as_u64().unwrap_or(0)}/>}
            else if self.is_master()&&self.active=="situazione"{<situation::Situation stat={self.stat.clone()} state={self.state.clone()} master={self.master.clone()}/>}
            else if self.is_master()&&self.active=="risposte"{<main class="page"><risposte::Risposte token={self.token.clone()} master={self.master.clone()} state={self.state.clone()} stat={self.stat.clone()} responses={self.responses.clone()} action={self.action(ctx)} busy={self.busy||gated}/></main>}
            else if !self.is_master()&&phase=="briefing"{<main class="page"><section class="card" id="await-start"><h2>{i18n::t(l,"await_start")}</h2><p>{i18n::t(l,"await_start_note")}</p><p class="muted">{api::LANGUAGES.iter().find(|(id,_)|*id==l).map(|(_,name)|*name).unwrap_or(l)}</p></section></main>}
            else{{self.view_chat(ctx)}}
            if let Some((body,to_s))=&self.due_modal{<timeline::AdvanceReviewDialog body={body.clone()} to_s={*to_s} injects={list(&self.master["planned_injects"]).to_vec()} agenda={self.master["agenda"].clone()} roles={list(&self.stat["roles"]).iter().filter(|r|text(&r["id"])!="master").cloned().collect::<Vec<_>>()} busy={self.busy} pending={self.pending.clone()} action={self.action(ctx)} onclose={ctx.link().callback(|_|Msg::CloseDue)}/>}
            if self.inject_open {<div class="modal-backdrop"><section class="inject-modal" role="dialog" aria-modal="true" aria-labelledby="inject-modal-title" onkeydown={{let link=ctx.link().clone();let busy=self.busy;Callback::from(move |e:KeyboardEvent|{if e.key()=="Escape"&&!busy{e.prevent_default();link.send_message(Msg::CloseInject);}})}}><div class="row"><h2 id="inject-modal-title">{if self.selected=="all"{crate::i18n::ex("Inject personalizzato").into()}else{crate::exfmt!("Inject · {}","Inject · {}",self.name(&self.selected))}}</h2><div class="grow"/><button aria-label={crate::i18n::ex("Chiudi modale inject")} disabled={self.busy} onclick={ctx.link().callback(|_|Msg::CloseInject)}>{crate::i18n::ex("Chiudi")}</button></div>if !self.error.is_empty(){<p class="error" role="alert">{&self.error}</p>}{self.view_inject_form(ctx)}</section></div>}

        </>}
    }
    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        if ctx.props().visible && !old.visible {
            ctx.link().send_message(Msg::Refresh);
        }
        // A panel that just became hidden must not re-render under the new active panel's language.
        ctx.props().visible
    }
    fn destroy(&mut self, _ctx: &Context<Self>) {
        if let Some(socket) = self.socket.take() {
            socket.abort();
        }
    }
}
fn main() {
    #[cfg(feature = "bench")]
    if bench::requested() {
        yew::Renderer::<bench::Bench>::new().render();
        return;
    }
    #[cfg(feature = "standalone")]
    if simbench::requested() {
        yew::Renderer::<simbench::SimBench>::new().render();
        return;
    }
    #[cfg(feature = "standalone")]
    if api::window().location().search().unwrap_or_default()
        .split(['&', '?']).any(|p| p == "editor=1")
    {
        editor3d::start();
        return;
    }
    #[cfg(not(feature = "standalone"))]
    if api::window()
        .location()
        .pathname()
        .unwrap_or_default()
        .ends_with("maptest.html")
    {
        yew::Renderer::<maptest::MapTest>::new().render();
    } else {
        // `Renderer::new` would use `AppProps::default()`, where `visible` is
        // false (the `prop_or(true)` only applies inside `html!`): the page
        // would never redraw after loading.
        yew::Renderer::<App>::with_props(AppProps {
            visible: true,
            ..Default::default()
        })
        .render();
    }
    #[cfg(feature = "standalone")]
    yew::Renderer::<standalone::Standalone>::new().render();
}

/// True when this browser session already stored `key`.
fn session_flag(key: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.session_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
        .is_some()
}
