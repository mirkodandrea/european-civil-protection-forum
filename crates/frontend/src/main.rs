mod advanced;
mod api;
mod chat;
mod evaluator;
mod map;
#[cfg(feature = "standalone")]
mod standalone;
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
    Store(String),
    Send(String, String),
    Prepare(String),
    Submit(String),
    Download,
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
}
#[derive(Clone)]
pub struct Draft {
    pub text: String,
    pub mode: String,
    pub template: String,
    pub title: String,
    pub sender: String,
    pub scope: String,
    pub private: Value,
    pub dirty: bool,
    pub clock_revision: Value,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            text: String::new(),
            mode: "chat".into(),
            template: String::new(),
            title: String::new(),
            sender: "EXCON".into(),
            scope: "conversation".into(),
            private: Value::Null,
            dirty: false,
            clock_revision: Value::Null,
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
    busy: bool,
    reading: bool,
    resolving_request: bool,
    refresh_again: bool,
    drafts: BTreeMap<String, Draft>,
    dismissed: BTreeSet<u64>,
    seen: BTreeMap<String, u64>,
    fields: BTreeMap<String, String>,
    responses: BTreeMap<String, Value>,
    socket: Option<AbortHandle>,
    poll: Option<Interval>,
    filter: String,
}
pub enum Msg {
    Boot(Result<Bootstrap, String>),
    Refresh,
    Refreshed(Result<(Value, Value), String>),
    ResolutionDone(Result<Value, String>),
    Action(Action),
    Done(ActionKind, Result<Value, String>),
    Select(String),
    View(String),
    Field(String, String),
    Discard,
    Filter(String),
    Login(String),
    ClearError,
}
impl App {
    fn is_master(&self) -> bool {
        self.own == "master"
    }
    fn is_evaluator(&self) -> bool {
        self.page == "evaluator"
    }
    fn field(&self, key: &str, default: &str) -> String {
        self.fields
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    }
    fn draft(&self) -> Draft {
        self.drafts.get(&self.selected).cloned().unwrap_or_default()
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
            "EXCON".into()
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
            std::iter::once(
                json!({"id":"master","name":"EXCON","description":"Direzione esercitazione"}),
            )
            .chain(self.roles().into_iter().filter(|r| r["id"] != self.own))
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
        let from = text(&m["from"]);
        let to = text(&m["to"]);
        let peer = if m["kind"] == "inject" {
            "master"
        } else if from == self.own {
            to
        } else if from == "master" || !self.roles().iter().any(|r| r["id"] == from) {
            "master"
        } else {
            from
        };
        peer == id
    }
    fn mark_seen(&mut self) {
        if self.active == "chat" {
            let max = self
                .messages()
                .iter()
                .filter(|m| self.in_thread(m, &self.selected))
                .filter_map(|m| m["id"].as_u64())
                .max()
                .unwrap_or(0);
            self.seen.insert(self.selected.clone(), max);
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
    #[prop_or_default]
    pub token: Option<String>,
    #[prop_or_default]
    pub page: Option<String>,
}
impl Component for App {
    type Message = Msg;
    type Properties = AppProps;
    fn create(ctx: &Context<Self>) -> Self {
        let token = ctx.props().token.clone().unwrap_or_else(api::token);
        let key = token.clone();
        ctx.link().send_future(async move {
            let result = async {
                let access = api::request(&key, "/api/access", None).await?;
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
            busy: false,
            reading: false,
            resolving_request: false,
            refresh_again: false,
            drafts: BTreeMap::new(),
            dismissed: BTreeSet::new(),
            seen: BTreeMap::new(),
            fields: BTreeMap::new(),
            responses: BTreeMap::new(),
            socket: None,
            poll: None,
            filter: String::new(),
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
                } else if !self.is_master() {
                    self.page = "chat".into();
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
                self.request_refresh(ctx);
            }
            Msg::Boot(Err(e)) => self.error = e,
            Msg::Refresh => {
                self.request_refresh(ctx);
                return false;
            }
            Msg::Refreshed(result) => {
                self.reading = false;
                match result {
                    Ok((state, master)) => {
                        self.state = state;
                        self.master = master;
                        if !self.is_master() && !self.is_evaluator() {
                            let revision = self.state["clock_control"]["revision"].clone();
                            if !self.busy {
                                for draft in self.drafts.values_mut() {
                                    if !draft.private.is_null()
                                        && (draft.private["turn"] != self.state["turn"]
                                            || draft.clock_revision != revision)
                                    {
                                        draft.private = Value::Null;
                                    }
                                }
                            }
                            for private in list(&self.state["drafts"]) {
                                let id = private["id"].as_u64().unwrap_or(0);
                                let to = text(&private["to"]);
                                if self.dismissed.contains(&id)
                                    || private["status"] == "interpreting"
                                {
                                    continue;
                                }
                                let d = self.drafts.entry(to.into()).or_default();
                                if d.private.is_null() && private["turn"] == self.state["turn"] {
                                    d.private = private.clone();
                                    d.clock_revision = revision.clone();
                                    d.text = if private["status"] == "needs_clarification" {
                                        String::new()
                                    } else {
                                        text(&private["message"]).into()
                                    };
                                }
                            }
                        }
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
                if self.busy {
                    return false;
                }
                self.busy = true;
                self.error.clear();
                self.notice = "Elaborazione…".into();
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
            Msg::ResolutionDone(result) => {
                self.resolving_request = false;
                match result {
                    Ok(_) => self.notice = "Turno risolto".into(),
                    Err(e) => self.error = e,
                }
                self.request_refresh(ctx);
            }
            Msg::Done(kind, result) => {
                self.busy = false;
                match result {
                    Err(e) => {
                        self.error = e;
                        self.notice = "Operazione non riuscita".into()
                    }
                    Ok(value) => {
                        self.notice = "Operazione completata".into();
                        let failures = list(&value["results"])
                            .iter()
                            .filter(|r| r["ok"] == false)
                            .map(|r| text(&r["error"]))
                            .collect::<Vec<_>>();
                        if !failures.is_empty() {
                            self.error = failures.join(" · ");
                            self.notice = "Operazione con errori: consulta gli esiti".into();
                        }
                        match kind {
                            ActionKind::Store(key) => {
                                self.notice = match key.as_str() {
                                    "forecast" => "Bozza privata pronta · non pubblicata",
                                    "assessment" => {
                                        "Valutazione derivata pronta · richiede revisione umana"
                                    }
                                    _ => "Anteprima pronta · nessuna modifica applicata",
                                }
                                .into();
                                self.responses.insert(key, value);
                            }
                            ActionKind::Prepare(to) => {
                                let d = self.drafts.entry(to).or_default();
                                d.private = value["draft"].clone();
                                d.dirty = false;
                                d.clock_revision = self.state["clock_control"]["revision"].clone();
                                d.text = if d.private["status"] == "needs_clarification" {
                                    String::new()
                                } else {
                                    text(&d.private["message"]).into()
                                };
                                self.notice = "Bozza privata · nessun messaggio inviato".into();
                            }
                            ActionKind::Send(to, sent) => {
                                self.notice = if value["translation_failed"] == true {
                                    "Inviato · traduzione non disponibile"
                                } else {
                                    "Inviato"
                                }
                                .into();
                                let d = self.drafts.entry(to).or_default();
                                if d.text == sent {
                                    *d = Draft::default();
                                }
                            }
                            ActionKind::Submit(to) => {
                                self.notice = "Decisione inviata per revisione EXCON".into();
                                if let Some(d) = self.drafts.remove(&to) {
                                    if let Some(id) = d.private["id"].as_u64() {
                                        self.dismissed.insert(id);
                                    }
                                }
                            }
                            ActionKind::Download => {
                                if let Err(e) = api::download(&value) {
                                    self.error = e
                                }
                            }
                            ActionKind::Ordinary => {}
                        }
                    }
                }
                self.request_refresh(ctx);
            }
            Msg::Select(id) => {
                self.selected = id;
                self.mark_seen();
            }
            Msg::View(view) => {
                self.active = view;
                self.mark_seen();
            }
            Msg::Field(key, value) => {
                if key.starts_with("draft:") {
                    let d = self.drafts.entry(self.selected.clone()).or_default();
                    match key.as_str() {
                        "draft:text" => {
                            d.text = value;
                            if !d.private.is_null() && d.private["status"] != "needs_clarification"
                            {
                                d.dirty = d.text != text(&d.private["message"]);
                            }
                        }
                        "draft:mode" => d.mode = value,
                        "draft:title" => d.title = value,
                        "draft:sender" => d.sender = value,
                        "draft:scope" => d.scope = value,
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
            Msg::Discard => {
                if let Some(d) = self.drafts.remove(&self.selected) {
                    if let Some(id) = d.private["id"].as_u64() {
                        self.dismissed.insert(id);
                    }
                }
            }
            Msg::Filter(query) => {
                self.filter = query;
                self.request_refresh(ctx);
            }
            Msg::Login(key) => api::save_token(&key),
            Msg::ClearError => self.error.clear(),
        }
        true
    }
    fn view(&self, ctx: &Context<Self>) -> Html {
        if self.access.is_null() {
            let login = ctx.link().callback(|e: SubmitEvent| {
                e.prevent_default();
                let doc = window().document().unwrap();
                let input = doc.get_element_by_id("access-key").unwrap();
                use wasm_bindgen::JsCast;
                Msg::Login(input.unchecked_into::<web_sys::HtmlInputElement>().value())
            });
            return html! {<main class="page"><section class="card"><h1>{"PPRD East 4 · TTX"}</h1><p>{if self.error.is_empty(){"Connessione all’esercitazione…"}else{&self.error}}</p><form onsubmit={login}><label>{"Chiave di accesso ricevuta da EXCON"}<input id="access-key" type="password" required=true/></label><button type="submit">{"Apri esercitazione"}</button></form></section></main>};
        }
        let title = if self.is_evaluator() {
            "Valutatore · sola lettura".into()
        } else if self.is_master() {
            "Direzione esercitazione".into()
        } else {
            self.name(&self.own)
        };
        let phase = text(&self.state["phase"]);
        let nav = |id: &str, label: &str| {
            let id = id.to_string();
            let active = self.active == id;
            html! {<button class={if active{"active"}else{""}} data-view={id.clone()} onclick={ctx.link().callback(move |_|Msg::View(id.clone()))}>{label}</button>}
        };
        let href = |path: &str| format!("{path}?token={}", api::encode(&self.token));
        html! {<>
            <header class="topbar"><div class="brand"><div class="eyebrow">{"PPRD East 4 · TTX"}</div><h1>{title}</h1></div><div><span class="pill" id="turn">{format!("Turno {} · {}",self.state["turn"],phase)}</span><div class="clock" id="clock">{text(if self.is_master()||self.is_evaluator(){&self.state["world"]["stats"]["clock"]}else{&self.state["clock"]})}{" · tempo controllato da EXCON"}</div></div><div class="grow"/>
            if self.is_master()&&!self.is_evaluator(){<div id="turn-controls" class="row"><button id="start" class="primary" hidden={phase!="briefing"} disabled={self.busy} onclick={ctx.link().callback(|_|Msg::Action(Action::post("/api/master/start",json!({}))))}>{"Avvia esercitazione"}</button><label>{"Δt "}<input id="minutes" type="number" min="1" max="1440" value={self.field("minutes","30")} oninput={ctx.link().callback(|e|Msg::Field("minutes".into(),api::input(e)))}/>{" min"}</label><button id="resolve" class="primary" disabled={self.busy||self.resolving_request||phase!="orders"} onclick={{let minutes=self.field("minutes","30").parse::<u32>().unwrap_or(30);ctx.link().callback(move |_|Msg::Action(Action::post("/api/master/resolve",json!({"minutes":minutes}))))}}>{"Avanza turno →"}</button></div>}
            if !self.is_master()&&!self.is_evaluator(){<select key={self.field("language",text(&self.state["player"]["language"]))} id="language" aria-label="Lingua" value={self.field("language",text(&self.state["player"]["language"]))} onchange={{let own=self.own.clone();ctx.link().callback(move |e|Msg::Action(Action::post(format!("/api/player/{own}/language"),json!({"language":api::select(e)}))))}}>{for api::LANGUAGES.iter().map(|(id,name)|html!{<option value={*id} selected={*id==self.field("language",text(&self.state["player"]["language"]))}>{name}</option>})}</select>}
            </header>
            if !self.is_master()&&!self.is_evaluator()&&self.state["player"]["language_confirmed"]!=true{<div id="join" class="notice">{"Scegli la lingua dei messaggi. "}<button id="confirm-language" onclick={{let own=self.own.clone();let language=self.field("language",text(&self.state["player"]["language"]));ctx.link().callback(move |_|Msg::Action(Action::post(format!("/api/player/{own}/language"),json!({"language":language}))))}}>{"Conferma lingua"}</button></div>}
            <nav class="navigation" aria-label="Viste">
            if self.page=="chat" { {nav("chat","Conversazioni")} {nav("map",if self.is_master(){"Mappa dettagliata"}else{"Mappa tattica"})} }
            <div class="grow"/>
            if self.is_master()&&!cfg!(feature="standalone"){<a href={href("master.html")}>{"Conversazioni"}</a><a id="advanced" href={href("advanced.html")}>{"Strumenti avanzati ↗"}</a><a href={href("evaluator.html")}>{"Valutazione"}</a>}
            </nav>
            <div id="error" class="error" role="alert">{&self.error}</div>
            if (self.page=="advanced"||self.is_evaluator())&&!self.notice.is_empty(){<div class="notice" role="status">{&self.notice}</div>}
            if self.state.is_null(){<p class="page">{"Caricamento della situazione…"}</p>}
            else if self.is_evaluator(){<evaluator::Evaluator stat={self.stat.clone()} state={self.state.clone()} evidence={self.master.clone()} assessment={self.responses.get("assessment").cloned().unwrap_or_default()} busy={self.busy} action={self.action(ctx)} onfilter={ctx.link().callback(Msg::Filter)}/>}
            else if self.page=="advanced"&&self.is_master(){<advanced::Advanced stat={self.stat.clone()} state={self.state.clone()} master={self.master.clone()} links={self.links.clone()} catalogue={self.catalogue.clone()} responses={self.responses.clone()} busy={self.busy} action={self.action(ctx)}/>}
            else if self.page=="lobby"{<main class="page"><section class="card"><h2>{text(&self.stat["name"])}</h2><p>{text(&self.stat["description"])}</p><p><a href={href("master.html")}>{"Apri la console conversazionale"}</a></p>{widgets::participant_links(&self.links,&self.roles())}</section></main>}
            else{{self.view_chat(ctx)}}
        </>}
    }
    fn destroy(&mut self, _ctx: &Context<Self>) {
        if let Some(socket) = self.socket.take() {
            socket.abort();
        }
    }
}
fn main() {
    #[cfg(not(feature = "standalone"))]
    yew::Renderer::<App>::new().render();
    #[cfg(feature = "standalone")]
    yew::Renderer::<standalone::Standalone>::new().render();
}
