//! "Banco di prova": the EXCON and player screens of one small workflow side
//! by side, driven by an in-memory fake server (`fake`), to try the
//! information flow between users by hand. Dev only (cargo feature `bench`):
//! standalone `?bench=1`, server `bench.html`.
//!
//! The columns are the real components (`Risposte`, `Proposal`, `SendDialog`)
//! and the real player screen (`PlayerView`); only the server is simulated.
//! Every request they make, through callbacks or `api::request`, reaches
//! [`intercept`] and the fake.
mod fake;
mod workflows;

use crate::{
    api::{list, text},
    player_view::{self, PlayerView},
    Action, ActionKind, Msg,
};
use fake::Fake;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use workflows::{Column, Status, World, BENCHES};
use yew::prelude::*;

thread_local! {
    static FAKE: RefCell<Option<Fake>> = const { RefCell::new(None) };
    /// Redraws the bench after a request a component sent by itself.
    static NOTIFY: RefCell<Option<Callback<()>>> = const { RefCell::new(None) };
}

/// The bench page was asked for (`?bench=1`, or `bench.html` on the server).
pub fn requested() -> bool {
    let loc = crate::api::window().location();
    loc.pathname().unwrap_or_default().ends_with("bench.html")
        || loc
            .search()
            .unwrap_or_default()
            .split(['&', '?'])
            .any(|p| p == "bench=1")
}

/// Answers `api::request` from the fake while the bench is open.
pub fn intercept(who: &str, path: &str, body: &Option<Value>) -> Option<Result<Value, String>> {
    let who = if who == "advanced" { fake::MASTER } else { who };
    let answer = FAKE.with(|f| {
        f.borrow_mut()
            .as_mut()
            .map(|f| f.handle(who, path, body.clone()))
    })?;
    if body.is_some() && answer.is_ok() {
        if let Some(cb) = NOTIFY.with(|n| n.borrow().clone()) {
            cb.emit(());
        }
    }
    Some(answer)
}

fn with_fake<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with(|x| f(x.borrow_mut().as_mut().expect("bench fake server")))
}

const LANG_KEY: &str = "ttx_excon_lang";
fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}
/// The EXCON console language of this browser, shared with the console.
fn stored_lang() -> Option<String> {
    storage()
        .and_then(|s| s.get_item(LANG_KEY).ok().flatten())
        .filter(|l| crate::api::EXCON_LANGUAGES.iter().any(|(id, _)| id == l))
}
fn save_lang(lang: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(LANG_KEY, lang);
    }
}

/// Italian or English bench text, following the EXCON console language.
fn tr(it: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::is_en() {
        en
    } else {
        it
    }
}

pub enum BMsg {
    Tab(usize),
    Reset,
    Lang(String),
    Player(String, Msg),
    Excon(Action),
    Changed,
    Suggest(String),
    OpenSend(Option<String>),
}

pub struct Bench {
    tab: usize,
    epoch: u64,
    lang: String,
    /// Composer fields per player role.
    fields: BTreeMap<String, BTreeMap<String, String>>,
    errors: BTreeMap<String, String>,
    send: Option<String>,
    timers: BTreeMap<String, gloo_timers::callback::Timeout>,
}

impl Bench {
    fn reset(&mut self) {
        // Proposal ids restart: notes typed for the old ones must not reappear.
        for id in 0..256 {
            crate::widgets::save_note(id, "");
        }
        let mut fake = Fake::new(&self.lang);
        (BENCHES[self.tab].seed)(&mut fake);
        FAKE.with(|f| *f.borrow_mut() = Some(fake));
        self.fields.clear();
        self.errors.clear();
        self.send = None;
        self.timers.clear();
        self.epoch += 1;
    }
    fn fields_of(&mut self, role: &str) -> &mut BTreeMap<String, String> {
        self.fields.entry(role.to_string()).or_default()
    }
    /// One request of `who`, with the follow-up an `App` would do on success.
    fn request(&mut self, who: &str, action: Action) {
        let result = with_fake(|f| f.handle(who, &action.path, action.body.clone()));
        let kind = match action.kind {
            ActionKind::Inject { then, .. } => *then,
            k => k,
        };
        match result {
            Err(e) => {
                self.errors.insert(who.to_string(), e);
            }
            Ok(value) => {
                self.errors.remove(who);
                let fields = self.fields_of(who);
                match kind {
                    ActionKind::WorkSubmit => player_view::response_sent(fields),
                    ActionKind::ConvStart => {
                        player_view::conversation_started(fields, value["conversation"].as_u64())
                    }
                    ActionKind::ConvSend => player_view::conversation_sent(fields),
                    ActionKind::ClearField(key) => {
                        fields.remove(&key);
                    }
                    _ => {}
                }
            }
        }
    }
    fn player_msg(&mut self, ctx: &Context<Self>, role: String, msg: Msg) {
        match msg {
            Msg::Action(a) => self.request(&role, a),
            Msg::Field(key, value) => {
                if key == "work:narrative" && self.fields_of(&role).get(&key) != Some(&value) {
                    // As in `App`: suggestions follow a pause in typing.
                    let link = ctx.link().clone();
                    let r = role.clone();
                    self.timers.insert(
                        role.clone(),
                        gloo_timers::callback::Timeout::new(700, move || {
                            link.send_message(BMsg::Suggest(r))
                        }),
                    );
                }
                self.fields_of(&role).insert(key, value);
            }
            Msg::OpenWork(parent) => {
                let parent = parent.unwrap_or_else(|| {
                    with_fake(|f| {
                        f.messages
                            .iter()
                            .filter(|m| m.kind == "inject" && m.to == role)
                            .map(|m| m.id)
                            .max()
                    })
                    .map(|id| id.to_string())
                    .unwrap_or_default()
                });
                player_view::open_work(self.fields_of(&role), parent);
                self.errors.remove(&role);
            }
            Msg::OpenWorkConv(parent, conversation) => {
                player_view::open_work(self.fields_of(&role), parent);
                self.fields_of(&role)
                    .insert("work:conversation".into(), conversation);
            }
            Msg::OpenInjectDetail(key) => {
                player_view::open_inject_detail(self.fields_of(&role), key)
            }
            Msg::StageActions(commands) => player_view::stage(self.fields_of(&role), commands),
            Msg::Unstage(index) => player_view::unstage(self.fields_of(&role), index),
            _ => {}
        }
    }
    /// The automatic suggestions of `App`'s `Msg::Suggest`, from the fake.
    fn suggest(&mut self, role: &str) {
        self.timers.remove(role);
        let fields = self.fields_of(role).clone();
        let narrative = fields.get("work:narrative").cloned().unwrap_or_default();
        if fields.get("work:open").map(String::as_str) != Some("true")
            || narrative.trim().chars().count() < 20
            || fields.get("work:sugg_for") == Some(&narrative)
        {
            return;
        }
        let body = json!({"narrative": narrative, "reply_to": fields.get("work:reply_to").and_then(|v| v.parse::<u64>().ok())});
        let value = with_fake(|f| {
            f.handle(
                role,
                &format!("/api/player/{role}/suggest-actions"),
                Some(body),
            )
        })
        .unwrap_or_else(|e| json!({"status": "ai_unavailable", "actions": [], "failure": e}));
        let fields = self.fields_of(role);
        fields.insert("work:sugg".into(), value.to_string());
        fields.insert("work:sugg_for".into(), narrative);
    }

    fn view_excon(&self, ctx: &Context<Self>, stat: &Value, master: &Value, state: &Value) -> Html {
        let action = ctx.link().callback(BMsg::Excon);
        let roles = list(&stat["roles"]).to_vec();
        let unsent: Vec<&Value> = list(&master["planned_injects"])
            .iter()
            .filter(|p| !list(&p["not_yet_sent_to"]).is_empty())
            .collect();
        let open = self.send.as_ref().and_then(|id| {
            list(&master["planned_injects"])
                .iter()
                .find(|p| text(&p["id_inject"]) == id)
                .cloned()
        });
        html! {<>
            if !unsent.is_empty() {<section class="card bench-planned"><h2>{tr("Inject pianificati","Planned injects")}</h2>
                <ul>{for unsent.iter().map(|p|{let id=text(&p["id_inject"]).to_string();html!{<li><span><b>{text(&p["title"])}</b><small class="muted">{format!(" · {}",text(&p["text"]))}</small></span>
                    <button class="primary" data-bench-send={id.clone()} onclick={ctx.link().callback(move |_|BMsg::OpenSend(Some(id.clone())))}>{tr("Invia…","Send…")}</button></li>}})}</ul></section>}
            <crate::risposte::Risposte token={fake::MASTER} master={master.clone()} state={state.clone()} stat={stat.clone()} responses={BTreeMap::new()} action={action.clone()} busy=false/>
            if let Some(inject)=open {<crate::timeline::SendDialog inject={inject} roles={roles} busy=false action={action} onclose={ctx.link().callback(|_|BMsg::OpenSend(None))}/>}
        </>}
    }

    fn view_player(&self, ctx: &Context<Self>, role: &str, stat: &Value) -> Html {
        let r = role.to_string();
        PlayerView {
            state: with_fake(|f| f.player(role)),
            stat: stat.clone(),
            own: role.to_string(),
            fields: self.fields.get(role).cloned().unwrap_or_default(),
            busy: false,
            error: self.errors.get(role).cloned().unwrap_or_default(),
            active: "map".into(),
            roles: list(&stat["roles"]).to_vec(),
            maps: false,
            link: ctx.link().callback(move |m| BMsg::Player(r.clone(), m)),
        }
        .view_tactical()
    }
}

impl Component for Bench {
    type Message = BMsg;
    type Properties = ();
    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        NOTIFY.with(|n| {
            *n.borrow_mut() = Some(Callback::from(move |()| link.send_message(BMsg::Changed)))
        });
        let wanted = crate::api::window().location().search().unwrap_or_default();
        let tab = wanted
            .trim_start_matches('?')
            .split('&')
            .find_map(|p| p.strip_prefix("b="))
            .and_then(|id| BENCHES.iter().position(|b| b.id == id))
            .unwrap_or(0);
        let lang = stored_lang().unwrap_or_else(|| "it".into());
        let mut bench = Bench {
            tab,
            epoch: 0,
            lang,
            fields: BTreeMap::new(),
            errors: BTreeMap::new(),
            send: None,
            timers: BTreeMap::new(),
        };
        bench.reset();
        bench
    }
    fn update(&mut self, ctx: &Context<Self>, msg: BMsg) -> bool {
        match msg {
            BMsg::Tab(tab) => {
                self.tab = tab;
                self.reset();
            }
            BMsg::Reset => self.reset(),
            BMsg::Lang(lang) => {
                self.lang = lang.clone();
                with_fake(|f| f.lang = lang.clone());
                save_lang(&lang);
            }
            BMsg::Player(role, msg) => self.player_msg(ctx, role, msg),
            BMsg::Excon(action) => self.request(fake::MASTER, action),
            BMsg::Changed => {}
            BMsg::Suggest(role) => self.suggest(&role),
            BMsg::OpenSend(id) => self.send = id,
        }
        true
    }
    fn view(&self, ctx: &Context<Self>) -> Html {
        crate::i18n_excon::set_ui_lang(&self.lang);
        let bench = &BENCHES[self.tab];
        let (stat, master, state) = with_fake(|f| (f.stat(), f.master(), f.excon_state()));
        let statuses = FAKE.with(|f| {
            let f = f.borrow();
            let world = World {
                fake: f.as_ref().expect("bench fake server"),
                fields: &self.fields,
            };
            workflows::statuses(bench, &world)
        });
        let cols = bench.columns.len();
        html! {<div class="bench" data-bench={bench.id}>
            <header class="bench-head"><div><div class="eyebrow">{tr("Sviluppo · server simulato, nessuna AI","Development · simulated server, no AI")}</div><h1>{tr("Banco di prova","Test bench")}</h1></div><div class="grow"/>
                <div class="row" role="group" aria-label={tr("Lingua","Language")}>{for crate::api::EXCON_LANGUAGES.iter().map(|(id,name)|{let id=id.to_string();html!{<button data-bench-lang={id.clone()} class={classes!((self.lang==id).then_some("primary"))} onclick={ctx.link().callback(move |_|BMsg::Lang(id.clone()))}>{*name}</button>}})}</div>
                <button data-bench-reset="" onclick={ctx.link().callback(|_|BMsg::Reset)}>{tr("Ricomincia","Reset")}</button></header>
            <nav class="bench-tabs" aria-label={tr("Flussi","Workflows")}>{for BENCHES.iter().enumerate().map(|(i,b)|html!{<button data-bench-tab={b.id} class={classes!((i==self.tab).then_some("active"))} onclick={ctx.link().callback(move |_|BMsg::Tab(i))}>{tr(b.title.0,b.title.1)}</button>})}</nav>
            <section class="bench-flow"><p class="muted">{tr(bench.about.0,bench.about.1)}</p>
                <ol class="bench-steps">{for bench.steps.iter().zip(&statuses).enumerate().map(|(i,(s,st))|html!{
                    <li data-bench-step={(i+1).to_string()} data-status={st.as_str()} class={classes!("bench-step",st.as_str())} aria-current={(*st==Status::Active).then_some("step")}>
                        <span class="bench-step-n">{if *st==Status::Done{"✓".to_string()}else{(i+1).to_string()}}</span>
                        <span><small>{match s.side{Column::Excon=>"EXCON".to_string(),Column::Player(r)=>fake::role_name(r).to_string()}}</small>{tr(s.text.0,s.text.1)}</span></li>})}</ol></section>
            <div class="bench-cols" key={format!("{}-{}",self.tab,self.epoch)} style={format!("--bench-cols:{cols}")}>{for bench.columns.iter().map(|c|match c{
                Column::Excon=>html!{<section class="bench-col" data-bench-col="excon"><h2 class="bench-col-title">{"EXCON"}</h2>
                    if let Some(e)=self.errors.get(fake::MASTER){<p class="error" role="alert">{e}</p>}
                    {self.view_excon(ctx,&stat,&master,&state)}</section>},
                Column::Player(r)=>html!{<section class="bench-col player-ui" data-bench-col={*r}><h2 class="bench-col-title">{fake::role_name(r)}</h2>{self.view_player(ctx,r,&stat)}</section>},
            })}</div>
        </div>}
    }
    fn destroy(&mut self, _ctx: &Context<Self>) {
        NOTIFY.with(|n| *n.borrow_mut() = None);
        FAKE.with(|f| *f.borrow_mut() = None);
    }
}
