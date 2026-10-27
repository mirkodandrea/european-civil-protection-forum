use crate::{
    api::{list, text},
    Action,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use yew::prelude::*;

/// `+HH:MM:SS` from an offset in seconds.
pub fn format_offset(s: i64) -> String {
    let sign = if s < 0 { '-' } else { '+' };
    let s = s.abs();
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn strings(v: &Value) -> Vec<String> {
    list(v).iter().map(|r| text(r).to_string()).collect()
}

/// Roles ticked by default in the send dialog: planned and not yet served.
pub fn default_recipients(inject: &Value) -> BTreeSet<String> {
    let sent = strings(&inject["sent_to"]);
    strings(&inject["recipients_planned"])
        .into_iter()
        .filter(|r| !sent.contains(r))
        .collect()
}

/// Body for `POST /api/master/msel/<id>/send`.
pub fn send_body(selected: &BTreeSet<String>) -> Value {
    json!({"recipients": selected.iter().collect::<Vec<_>>()})
}

/// Groups of injects sharing an `event` (first-appearance order, chronological
/// inside). Injects without event (or all, when `group` is false) form
/// consecutive unnamed groups.
pub fn group_by_event(injects: &[Value], group: bool) -> Vec<(String, Vec<Value>)> {
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    for i in injects {
        let event = if group { text(&i["event"]).to_string() } else { String::new() };
        if event.is_empty() {
            match out.last_mut() {
                Some((e, v)) if e.is_empty() => v.push(i.clone()),
                _ => out.push((String::new(), vec![i.clone()])),
            }
        } else if let Some((_, v)) = out.iter_mut().find(|(e, _)| *e == event) {
            v.push(i.clone());
        } else {
            out.push((event, vec![i.clone()]));
        }
    }
    out
}

/// Injects and scripted actions as one chronological list. Each item is
/// tagged with `"item": "inject" | "action"` and sorted by scenario time
/// (stable, so injects come before actions planned at the same instant).
pub fn merge_items(injects: &[Value], actions: &[Value]) -> Vec<Value> {
    let tag = |v: &Value, kind: &str, t: i64| {
        let mut v = v.clone();
        v["item"] = json!(kind);
        (t, v)
    };
    let mut rows: Vec<(i64, Value)> = injects
        .iter()
        .map(|i| tag(i, "inject", i["scenario_time_s"].as_i64().unwrap_or(0)))
        .chain(actions.iter().map(|a| tag(a, "action", a["at_s"].as_i64().unwrap_or(0))))
        .collect();
    rows.sort_by_key(|(t, _)| *t);
    rows.into_iter().map(|(_, v)| v).collect()
}

pub fn role_name(roles: &[Value], id: &str) -> String {
    roles
        .iter()
        .find(|r| text(&r["id"]) == id)
        .map(|r| text(&r["name"]).to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string())
}

fn pills(inject: &Value, roles: &[Value]) -> Html {
    let sent = strings(&inject["sent_to"]);
    let pending = strings(&inject["not_yet_sent_to"]);
    html! {<span class="recipient-pills">
        {for sent.iter().map(|r|html!{<span class="pill rcpt sent" title={crate::i18n::ex("Inviato")}>{format!("✓ {}",role_name(roles,r))}</span>})}
        {for pending.iter().map(|r|html!{<span class="pill rcpt todo" title={crate::i18n::ex("Non ancora inviato")}>{role_name(roles,r)}</span>})}
    </span>}
}

#[derive(Properties, PartialEq)]
struct SendDialogProps {
    inject: Value,
    roles: Vec<Value>,
    busy: bool,
    action: Callback<Action>,
    onclose: Callback<()>,
}

#[function_component(SendDialog)]
fn send_dialog(props: &SendDialogProps) -> Html {
    let inject = &props.inject;
    let id = text(&inject["id_inject"]).to_string();
    let served = strings(&inject["sent_to"]);
    let selected = use_state(|| default_recipients(inject));
    let all_roles = props.roles.iter().map(|r| text(&r["id"]).to_string()).collect::<Vec<_>>();
    let planned = strings(&inject["recipients_planned"]);
    let send = {
        let selected = selected.clone();
        let action = props.action.clone();
        let onclose = props.onclose.clone();
        let id = id.clone();
        Callback::from(move |_| {
            action.emit(Action::post(format!("/api/master/msel/{id}/send"), send_body(&selected)));
            onclose.emit(());
        })
    };
    html! {<div class="modal-backdrop"><section class="inject-modal send-dialog" role="dialog" aria-modal="true" aria-labelledby="send-dialog-title">
        <div class="row"><h2 id="send-dialog-title">{crate::exfmt!("Invia inject · {}","Send inject · {}",text(&inject["title"]))}</h2><div class="grow"/><button onclick={{let c=props.onclose.clone();Callback::from(move |_|c.emit(()))}}>{crate::i18n::ex("Chiudi")}</button></div>
        <p class="muted">{text(&inject["scenario_time"])}</p>
        <p class="inject-text">{text(&inject["text"])}</p>
        <h3>{crate::i18n::ex("Destinatari")}</h3>
        <div class="send-roles">{for all_roles.iter().map(|r|{
            let done=served.contains(r);
            let checked=done||selected.contains(r);
            let onchange={let selected=selected.clone();let r=r.clone();Callback::from(move |e:Event|{
                use yew::TargetCast;
                let on=e.target_unchecked_into::<web_sys::HtmlInputElement>().checked();
                let mut next=(*selected).clone();
                if on{next.insert(r.clone());}else{next.remove(&r);}
                selected.set(next);
            })};
            html!{<label class={classes!("send-role",done.then_some("served"))}><input type="checkbox" data-send-role={r.clone()} checked={checked} disabled={done||props.busy} {onchange}/> <span>{role_name(&props.roles,r)}</span>
                if done{<small>{crate::i18n::ex("già inviato")}</small>}else if !planned.contains(r){<small>{crate::i18n::ex("non previsto dallo scenario")}</small>}</label>}
        })}</div>
        <div class="row send-actions"><div class="grow"/><button class="primary" data-send="" disabled={props.busy||selected.is_empty()} onclick={send}>{crate::exfmt!("Invia a {} destinatari","Send to {} recipients",selected.len())}</button></div>
    </section></div>}
}

// ---------------------------------------------------------------------------
// Pure helpers of the turn panel and the list (unit-tested below).

/// `Day N HH:MM` of scenario second `t` (`origin` = the scenario's start clock).
pub fn clock_at(origin: i64, t: i64) -> String {
    scenario::format_clock(origin + t)
}

/// `HH:MM` of scenario second `t`.
pub fn hhmm(origin: i64, t: i64) -> String {
    let rem = (origin + t).rem_euclid(86_400);
    format!("{:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

/// Time shown in list rows: `13:05` on the first day, `Day 2 13:05` afterwards.
pub fn compact_clock(origin: i64, t: i64) -> String {
    if (origin + t).div_euclid(86_400) == 0 {
        hhmm(origin, t)
    } else {
        clock_at(origin, t)
    }
}

/// A tick of the time axis under the track; `label` is left out near the ends,
/// where the start and end times are written instead.
#[derive(Debug, PartialEq)]
pub struct Tick {
    pub t: i64,
    pub major: bool,
    pub label: Option<String>,
}

/// Ticks inside `(start, end)` on round clock times: every 10 minutes for
/// windows up to 2 h, every 30 minutes up to 8 h, hourly beyond. Hours are major.
pub fn time_ticks(origin: i64, start: i64, end: i64) -> Vec<Tick> {
    let span = (end - start).max(1);
    let step = if span <= 2 * 3600 {
        600
    } else if span <= 8 * 3600 {
        1800
    } else {
        3600
    };
    let mut t = (origin + start).div_euclid(step) * step + step - origin;
    let mut out = Vec::new();
    while t < end && out.len() < 64 {
        let near_edge = (t - start) * 100 < span * 7 || (end - t) * 100 < span * 7;
        out.push(Tick {
            t,
            major: (origin + t).rem_euclid(3600) == 0,
            label: (!near_edge).then(|| hhmm(origin, t)),
        });
        t += step;
    }
    out
}

/// Agenda items sharing a spot of the track.
#[derive(Debug, PartialEq)]
pub struct Cluster {
    pub at: i64,
    pub items: Vec<Value>,
}

/// Splits chronological agenda items into track clusters (items closer than
/// `gap_pct` percent of the window share one badge) and the overdue ones,
/// planned before the window starts.
pub fn cluster_items(items: &[Value], start: i64, end: i64, gap_pct: f64) -> (Vec<Cluster>, Vec<Value>) {
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut overdue = Vec::new();
    for i in items {
        let at = i["at_s"].as_i64().unwrap_or(0);
        if at < start {
            overdue.push(i.clone());
            continue;
        }
        match clusters.last_mut() {
            Some(c) if track_pct(at, start, end) - track_pct(c.at, start, end) <= gap_pct => c.items.push(i.clone()),
            _ => clusters.push(Cluster { at, items: vec![i.clone()] }),
        }
    }
    (clusters, overdue)
}

/// `(injects, actions)` that are due now: reached (or overdue) and undecided.
pub fn attention_counts(items: &[Value]) -> (usize, usize) {
    let due = items.iter().filter(|i| i["due"] == true);
    (due.clone().filter(|i| i["kind"] == "inject").count(), due.filter(|i| i["kind"] == "action").count())
}

/// Why an unsent inject is raised before an advance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reach {
    /// Planned before the open turn started.
    Overdue,
    /// Its instant has been reached.
    Reached,
    /// The advance will go past its instant.
    Crossed,
}

/// Injects not (fully) sent whose instant is before `to_s`: what an advance to
/// `to_s` would leave behind. Chronological, with the reason.
pub fn unsent_injects(injects: &[Value], now: i64, to_s: i64, window_start: i64) -> Vec<(Value, Reach)> {
    let mut out: Vec<(i64, Value, Reach)> = injects
        .iter()
        .filter(|i| !list(&i["not_yet_sent_to"]).is_empty())
        .filter_map(|i| {
            let at = i["scenario_time_s"].as_i64().unwrap_or(0);
            let reach = if at < window_start {
                Reach::Overdue
            } else if at <= now {
                Reach::Reached
            } else if at < to_s {
                Reach::Crossed
            } else {
                return None;
            };
            Some((at, i.clone(), reach))
        })
        .collect();
    out.sort_by_key(|(t, ..)| *t);
    out.into_iter().map(|(_, i, r)| (i, r)).collect()
}

/// Which advance a resolve body asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdvanceKind {
    /// Up to the next pending event; the turn stays open.
    Event,
    /// By a number of minutes; the turn stays open.
    Step,
    /// To the end of the turn, which opens the next one.
    Close,
}

pub fn advance_kind(body: &Value) -> AdvanceKind {
    if body["until"] == "next_event" {
        AdvanceKind::Event
    } else if body["substep"] == true {
        AdvanceKind::Step
    } else {
        AdvanceKind::Close
    }
}

/// `(title, confirm button)` stating where time goes and what happens to the turn.
pub fn advance_wording(kind: AdvanceKind, turn: u64, to_clock: &str, to_hhmm: &str) -> (String, String) {
    match kind {
        AdvanceKind::Close => (
            crate::exfmt!("Chiudi turno {} alle {} · si apre il turno {}", "Close turn {} at {} · turn {} opens", turn, to_hhmm, turn + 1),
            crate::exfmt!("Chiudi turno {}", "Close turn {}", turn),
        ),
        _ => (
            crate::exfmt!("Avanza a {} · il turno resta aperto", "Advance to {} · the turn stays open", to_clock),
            crate::exfmt!("Avanza a {}", "Advance to {}", to_hhmm),
        ),
    }
}

/// Planned and served recipients of an inject in a few words.
pub fn recipients_summary(inject: &Value) -> String {
    let planned = list(&inject["recipients_planned"]).len();
    let sent = list(&inject["sent_to"]).len();
    match (planned, sent) {
        (0, _) => String::new(),
        (p, s) if s >= p => crate::exfmt!("inviato a {}", "sent to {}", p),
        (_, 0) => crate::exfmt!("da inviare a {}", "to send to {}", planned),
        (p, s) => crate::exfmt!("inviato a {} su {}", "sent to {} of {}", s, p),
    }
}

/// Position of an item in the list: scenario second of an inject or action.
pub fn item_time(item: &Value) -> i64 {
    if item["item"] == "action" {
        item["at_s"].as_i64().unwrap_or(0)
    } else {
        item["scenario_time_s"].as_i64().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BandState {
    Past,
    Current,
    Future,
}

/// The list items of one turn.
#[derive(Debug)]
pub struct Band {
    pub turn: u32,
    pub start_s: i64,
    pub end_s: i64,
    pub state: BandState,
    pub items: Vec<Value>,
}

/// Splits chronological list items into turns of the open turn's length. The
/// open turn is `cur_turn` over `[cur_start, cur_end)`; earlier and later turns
/// are extrapolated with that length (past turns are approximate when the
/// length was changed). The open turn is always present, even without items.
pub fn turn_bands(items: &[Value], cur_turn: u32, cur_start: i64, cur_end: i64) -> Vec<Band> {
    let cur_turn = cur_turn.max(1);
    let len = (cur_end - cur_start).max(60);
    let turn_of = |t: i64| -> u32 {
        if t < cur_start {
            (cur_turn as i64 - (cur_start - t + len - 1) / len).max(1) as u32
        } else if t >= cur_end {
            (cur_turn as i64 + 1 + (t - cur_end) / len) as u32
        } else {
            cur_turn
        }
    };
    let window = |turn: u32| -> (i64, i64) {
        let start = cur_start + (turn as i64 - cur_turn as i64) * len;
        (if turn == 1 { start.max(0) } else { start }, start + len)
    };
    let mut bands: Vec<Band> = Vec::new();
    for i in items {
        let turn = turn_of(item_time(i));
        match bands.iter_mut().find(|b| b.turn == turn) {
            Some(b) => b.items.push(i.clone()),
            None => {
                let (start_s, end_s) = window(turn);
                bands.push(Band { turn, start_s, end_s, state: BandState::Past, items: vec![i.clone()] });
            }
        }
    }
    if !bands.iter().any(|b| b.turn == cur_turn) {
        bands.push(Band { turn: cur_turn, start_s: cur_start, end_s: cur_end, state: BandState::Current, items: Vec::new() });
    }
    for b in &mut bands {
        b.state = match b.turn.cmp(&cur_turn) {
            std::cmp::Ordering::Less => BandState::Past,
            std::cmp::Ordering::Equal => BandState::Current,
            std::cmp::Ordering::Greater => BandState::Future,
        };
    }
    bands.sort_by_key(|b| b.turn);
    bands
}

/// Scripted actions executed during `[from_s, to_s)`: `(title, ok, result)`.
pub fn executed_actions(actions: &[Value], from_s: i64, to_s: i64) -> Vec<(String, bool, String)> {
    actions
        .iter()
        .filter(|a| a["run"]["executed_s"].as_i64().is_some_and(|t| t >= from_s && t < to_s))
        .map(|a| (text(&a["title"]).to_string(), a["run"]["status"] == "done", text(&a["run"]["result"]).to_string()))
        .collect()
}

/// Playback speed in words: `max` for 0, otherwise `60×`.
pub fn speed_label(v: f64) -> String {
    if v <= 0.0 {
        crate::i18n::ex("max").to_string()
    } else {
        format!("{}×", v)
    }
}

/// Choices of the speed selector, plus the current value when it is not one.
pub fn speed_options(current: f64) -> Vec<f64> {
    let mut v = vec![0.0, 10.0, 60.0, 120.0];
    if !v.iter().any(|x| (x - current).abs() < 1e-6) {
        v.push(current);
        v.sort_by(|a, b| a.total_cmp(b));
    }
    v
}

/// Whole simulated minutes still to run.
pub fn remaining_min(now: i64, to_s: i64) -> i64 {
    ((to_s - now).max(0) + 59) / 60
}

/// Scroll the timeline rows of agenda items into view and flash them all.
fn reveal_rows(targets: &[(String, String)]) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let mut first = true;
    for (kind, id) in targets {
        let attr = if kind == "inject" { "data-inject" } else { "data-action" };
        let Some(row) = doc.query_selector(&format!("[{attr}=\"{id}\"]")).ok().flatten() else {
            continue;
        };
        if first {
            row.scroll_into_view();
            first = false;
        }
        let _ = row.remove_attribute("data-flash");
        let _ = row.set_attribute("data-flash", "");
        gloo_timers::callback::Timeout::new(1600, move || {
            let _ = row.remove_attribute("data-flash");
        })
        .forget();
    }
}

/// Scroll the list to the first item waiting in the open turn, else its band header.
fn scroll_to_current_turn() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let target = doc
        .query_selector("[data-band-current] .planned-inject.is-due")
        .ok()
        .flatten()
        .or_else(|| doc.query_selector("[data-band-current]").ok().flatten());
    if let Some(t) = target {
        t.scroll_into_view();
    }
}

// ---------------------------------------------------------------------------
// Pre-advance review

#[derive(Properties, PartialEq)]
pub struct AdvanceReviewProps {
    /// Resolve body of the advance being started; the choices are added to it.
    pub body: Value,
    /// Simulated second the advance goes to.
    pub to_s: i64,
    /// Scripted actions the advance crosses: each needs a decision.
    pub due: Vec<Value>,
    /// Planned injects as of now, so sending one updates the list.
    pub injects: Vec<Value>,
    pub agenda: Value,
    pub roles: Vec<Value>,
    pub busy: bool,
    pub action: Callback<Action>,
    pub onclose: Callback<()>,
}

/// Asked at every advance that has something to handle: what to do with each
/// scripted action it crosses (default: apply it at its planned instant) and
/// with the injects whose instant was reached or will be crossed but are still
/// unsent. Advancing with unsent injects is possible, but explicit.
pub fn actions_body(base: &Value, choices: &BTreeMap<String, String>) -> Value {
    let mut body = base.clone();
    body["actions"] = json!(choices);
    body
}

#[function_component(AdvanceReviewDialog)]
pub fn advance_review_dialog(props: &AdvanceReviewProps) -> Html {
    let choices = {
        let due = props.due.clone();
        use_state(move || {
            due.iter()
                .map(|a| (text(&a["id"]).to_string(), "run".to_string()))
                .collect::<BTreeMap<_, _>>()
        })
    };
    let sending = use_state(|| Option::<String>::None);
    let a = &props.agenda;
    let origin = a["clock_origin_s"].as_i64().unwrap_or(0);
    let now = a["now_s"].as_i64().unwrap_or(0);
    let start = a["turn_start_s"].as_i64().unwrap_or(0);
    let turn = a["turn"].as_u64().unwrap_or(1);
    let unsent = unsent_injects(&props.injects, now, props.to_s, start);
    let kind = advance_kind(&props.body);
    let (title, confirm_label) = advance_wording(kind, turn, &clock_at(origin, props.to_s), &hhmm(origin, props.to_s));
    let confirm = {
        let (choices, action, onclose, base) =
            (choices.clone(), props.action.clone(), props.onclose.clone(), props.body.clone());
        Callback::from(move |_| {
            action.emit(Action::post("/api/master/resolve", actions_body(&base, &choices)));
            onclose.emit(());
        })
    };
    let option = |id: &str, value: &'static str, label: &'static str| {
        let selected = choices.get(id).is_some_and(|c| c == value);
        let onchange = {
            let (choices, id) = (choices.clone(), id.to_string());
            Callback::from(move |_| {
                let mut next = (*choices).clone();
                next.insert(id.clone(), value.to_string());
                choices.set(next);
            })
        };
        html! {<label class="inline-check"><input type="radio" name={format!("due-{id}")} data-due-choice={value} checked={selected} {onchange}/> {crate::i18n::ex(label)}</label>}
    };
    let open_inject = sending
        .as_ref()
        .and_then(|id| props.injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let nothing = props.due.is_empty() && unsent.is_empty();
    let confirm_class = classes!("primary", (!unsent.is_empty()).then_some("warn"));
    html! {<div class="modal-backdrop"><section class="inject-modal send-dialog advance-review" role="dialog" aria-modal="true" aria-labelledby="due-dialog-title">
        <div class="row"><h2 id="due-dialog-title">{title}</h2><div class="grow"/><button onclick={{let c=props.onclose.clone();Callback::from(move |_|c.emit(()))}}>{crate::i18n::ex("Annulla")}</button></div>
        <p class="muted">{crate::exfmt!("Ora {} → {}.","Now {} → {}.",clock_at(origin,now),clock_at(origin,props.to_s))}</p>
        if nothing{<p>{crate::i18n::ex("Non c’è più nulla da gestire.")}</p>}
        if !unsent.is_empty(){<h3>{crate::exfmt!("Inject non ancora inviati ({})","Injects not yet sent ({})",unsent.len())}</h3>
            <p class="muted">{crate::i18n::ex("Gli istanti previsti sono raggiunti o attraversati da questo avanzamento: inviali ora, oppure avanza comunque.")}</p>
            <ul class="planned-list">{for unsent.iter().map(|(inj,reach)|{
                let id=text(&inj["id_inject"]).to_string();
                let why=match reach{Reach::Overdue=>crate::i18n::ex("in ritardo"),Reach::Reached=>crate::i18n::ex("raggiunto"),Reach::Crossed=>crate::i18n::ex("attraversato dall’avanzamento")};
                let at=inj["scenario_time_s"].as_i64().unwrap_or(0);
                let send_now={let (action,id)=(props.action.clone(),id.clone());let body=send_body(&default_recipients(inj));
                    Callback::from(move |_|action.emit(Action::post(format!("/api/master/msel/{id}/send"),body.clone())))};
                let choose={let (sending,id)=(sending.clone(),id.clone());Callback::from(move |_|sending.set(Some(id.clone())))};
                html!{<li class="planned-inject review-row" data-review-inject={id.clone()} key={id.clone()}>
                    <span class="planned-times"><b>{compact_clock(origin,at)}</b></span>
                    <span class="planned-body"><span class="row"><b class="planned-title">{text(&inj["title"])}</b><span class={classes!("pill","due-now",(*reach==Reach::Overdue).then_some("late"))}>{why}</span></span>
                        <small class="muted">{recipients_summary(inj)}</small></span>
                    <span class="review-send"><button class="primary" data-review-send="" disabled={props.busy} onclick={send_now}>{crate::i18n::ex("Invia ora")}</button>
                        <button disabled={props.busy} onclick={choose}>{crate::i18n::ex("Destinatari…")}</button></span></li>}
            })}</ul>}
        if !props.due.is_empty(){<h3>{crate::exfmt!("Azioni pianificate ({})","Scheduled actions ({})",props.due.len())}</h3>
            <p class="muted">{crate::i18n::ex("Decidi cosa fare di ciascuna azione che l’avanzamento attraversa.")}</p>
            <ul class="planned-list">{for props.due.iter().map(|act|{
                let id=text(&act["id"]).to_string();
                let overdue=act["overdue"]==true;
                let after=text(&act["after"]).to_string();
                let waits=!after.is_empty()&&unsent.iter().any(|(i,_)|text(&i["id_inject"])==after);
                html!{<li class="planned-inject" data-action={id.clone()} key={id.clone()}>
                    <span class="planned-times"><b>{compact_clock(origin,act["at_s"].as_i64().unwrap_or(0))}</b>if overdue{<small>{crate::i18n::ex("istante già passato")}</small>}</span>
                    <span class="planned-body"><b class="planned-title">{text(&act["title"])}</b>
                        <span class="planned-text">{text(&act["summary"])}</span>
                        if waits{<span class="pill due-now late">{crate::exfmt!("segue l’inject {}, non ancora inviato","follows inject {}, not yet sent",after)}</span>}
                        if !text(&act["note"]).is_empty(){<small class="planned-objective">{text(&act["note"])}</small>}
                        <span class="row">{if overdue{option(&id,"run","Esegui subito")}else{option(&id,"run","Esegui all’istante previsto")}}{option(&id,"skip","Salta")}{option(&id,"defer","Rinvia")}</span></span></li>}
            })}</ul>}
        <div class="row send-actions"><div class="grow"/>
            <button class={confirm_class} data-due-confirm="" disabled={props.busy} onclick={confirm}>{if unsent.is_empty(){confirm_label}else{crate::exfmt!("Avanza comunque · {}","Advance anyway · {}",confirm_label)}}</button></div>
        if let Some(inject)=open_inject{<SendDialog key={text(&inject["id_inject"]).to_string()} inject={inject.clone()} roles={props.roles.clone()} busy={props.busy} action={props.action.clone()} onclose={{let s=sending.clone();Callback::from(move |_|s.set(None))}}/>}
    </section></div>}
}

// ---------------------------------------------------------------------------
// Turn panel

#[derive(Properties, PartialEq)]
struct TurnPanelProps {
    agenda: Value,
    state: Value,
    /// Scripted actions with their runs, for the last-advance summary.
    actions: Vec<Value>,
    /// Report returned by the last advance, when this console ran it.
    report: Value,
    busy: bool,
    action: Callback<Action>,
    onadvance: Callback<(Value, i64)>,
}

/// Position of `t` on a track from `start` to `end`, in percent.
pub fn track_pct(t: i64, start: i64, end: i64) -> f64 {
    ((t - start) as f64 / (end - start).max(1) as f64 * 100.0).clamp(0.0, 100.0)
}

/// Intermediate advances of the open turn: `[to the next event, by step_min]`
/// as `(target second, resolve body)`; the turn stays open.
pub fn advance_targets(now: i64, next: Option<i64>, step_min: u32) -> [Option<(i64, Value)>; 2] {
    [
        next.map(|t| (t, json!({"substep": true, "until": "next_event"}))),
        Some((now + step_min.max(1) as i64 * 60, json!({"substep": true, "minutes": step_min.max(1)}))),
    ]
}

/// Closing the open turn: run to its planned end, or one minute once that has
/// been passed by intermediate advances.
pub fn close_target(now: i64, end: i64) -> (i64, Value) {
    if end > now {
        (end, json!({"until": "turn_end"}))
    } else {
        (now + 60, json!({"minutes": 1}))
    }
}

/// What the last advance did: executed scripted actions with their result,
/// applied player actions and simulation events.
fn last_advance_summary(state: &Value, actions: &[Value], report: &Value, origin: i64) -> Html {
    let w = &state["last_turn"];
    let (Some(from), Some(to)) = (w["from_s"].as_i64(), w["to_s"].as_i64()) else {
        return Html::default();
    };
    let ran = executed_actions(actions, from, to);
    let mine = report["from_s"].as_i64() == Some(from) && report["to_s"].as_i64() == Some(to);
    let applied = if mine { list(&report["applied"]) } else { &[] };
    let events = if mine { list(&report["events"]) } else { &[] };
    let mut parts: Vec<String> = ran.iter().map(|(t, _, r)| format!("{t}: {r}")).collect();
    parts.extend(applied.iter().map(|a| format!("{} ({})", text(&a["action"]), text(&a["by"]))));
    if !events.is_empty() {
        parts.push(crate::exfmt!("{} eventi di simulazione", "{} simulation events", events.len()));
    }
    let bad = ran.iter().any(|(_, ok, _)| !ok) || applied.iter().any(|a| a["ok"] == false);
    let line = if parts.is_empty() { crate::i18n::ex("nessuna azione eseguita").to_string() } else { parts.join(" · ") };
    html! {<span class={classes!("tp-last",bad.then_some("bad"))} data-last-advance="" title={line.clone()}>
        <b>{crate::exfmt!("Ultimo avanzamento {} → {}:","Last advance {} → {}:",hhmm(origin,from),hhmm(origin,to))}</b>{" "}{line}</span>}
}

/// The single owner of status and time on the Timeline page: the open turn as
/// a window on the scenario timeline (where "now" is, the events inside it),
/// what waits to be handled, the playback speed, and every advance, from the
/// next event to closing the turn. Markers only point to list rows.
#[function_component(TurnPanel)]
fn turn_panel(props: &TurnPanelProps) -> Html {
    let step = use_state(|| "5".to_string());
    let length = use_state(String::new);
    let a = &props.agenda;
    let w = &props.state["turn_window"];
    let origin = a["clock_origin_s"].as_i64().unwrap_or(0);
    let start = a["turn_start_s"].as_i64().unwrap_or(0);
    let end = a["planned_end_s"].as_i64().unwrap_or(start + 1800);
    let resolving = !props.state["resolving"].is_null();
    let orders = text(&props.state["phase"]) == "orders";
    // The polled state is the freshest while a turn runs.
    let now = w["now_s"].as_i64().or(a["now_s"].as_i64()).unwrap_or(start);
    let target = props.state["resolving"]["to_s"].as_i64();
    let speed = props.state["speed"].as_f64().unwrap_or(0.0);
    let items = list(&a["items"]);
    let next = a["next_event_s"].as_i64();
    let step_min = step.parse::<u32>().unwrap_or(5);
    let [to_next, to_step] = advance_targets(now, next, step_min);
    let go = |t: Option<(i64, Value)>| {
        let onadvance = props.onadvance.clone();
        Callback::from(move |_| {
            if let Some((to, body)) = t.clone() {
                onadvance.emit((body, to));
            }
        })
    };
    let ctl_off = props.busy || !orders || resolving;
    let new_end = length.parse::<u32>().ok().map(|m| start + m.clamp(1, 1440) as i64 * 60);
    let set_length = {
        let (action, length) = (props.action.clone(), length.clone());
        Callback::from(move |_| {
            if let Ok(m) = length.parse::<u32>() {
                action.emit(Action::post("/api/master/settings", json!({"turn_minutes": m})));
            }
        })
    };
    let (clusters, overdue) = cluster_items(items, start, end, 1.2);
    let (due_inj, due_act) = attention_counts(items);
    let due_targets: Vec<(String, String)> = items
        .iter()
        .filter(|i| i["due"] == true)
        .map(|i| (text(&i["kind"]).to_string(), text(&i["id"]).to_string()))
        .collect();
    let overdue_targets: Vec<(String, String)> = overdue.iter().map(|i| (text(&i["kind"]).to_string(), text(&i["id"]).to_string())).collect();
    let pct = |t: i64| track_pct(t, start, end);
    let (close_to, close_body) = close_target(now, end);
    let speed_select = {
        let action = props.action.clone();
        html! {<label class="tp-speed">{crate::i18n::ex("Velocità")}
            <select id="tp-speed" aria-label={crate::i18n::ex("Velocità di risoluzione")} title={crate::i18n::ex("Minuti simulati al secondo: vale anche per l’avanzamento in corso")} disabled={props.busy}
                onchange={Callback::from(move |e| {
                    let v: f64 = crate::api::select(e).parse().unwrap_or(0.0);
                    action.emit(Action::post("/api/master/settings", json!({"speed": v})));
                })}>
                {for speed_options(speed).into_iter().map(|v|html!{<option value={v.to_string()} selected={(v-speed).abs()<1e-6}>{speed_label(v)}</option>})}
            </select></label>}
    };
    let status = if resolving {
        html! {<span class="tp-status running" role="status" id="tp-status">
            <b>{"▶ "}{crate::i18n::ex("In corso")}</b>
            <span class="tp-run">{format!("{} → {}",hhmm(origin,now),hhmm(origin,target.unwrap_or(now)))}</span>
            <span class="pill">{speed_label(speed)}</span>
            <span>{crate::exfmt!("rimangono {} min simulati","{} simulated min left",remaining_min(now,target.unwrap_or(now)))}</span></span>}
    } else {
        html! {<span class="tp-status" role="status" id="tp-status" title={crate::i18n::ex("ordini aperti: il tempo avanza solo quando lo decidi tu")}>
            <b>{"⏸ "}{crate::exfmt!("Fermo · {}","Paused · {}",clock_at(origin,now))}</b></span>}
    };
    let stop = resolving.then(|| html! {<button id="advance-stop" class="warn tp-small" disabled={props.busy} title={crate::i18n::ex("Termina l’avanzamento al secondo simulato corrente; il turno resta aperto")}
        onclick={{let action=props.action.clone();Callback::from(move |_|action.emit(Action::post("/api/master/resolve/stop",json!({}))))}}>{"⏸ "}{crate::i18n::ex("Ferma qui")}</button>});
    html! {<section class={classes!("card","turn-panel",resolving.then_some("running"))} id="turn-bar" data-now={now.to_string()}>
        <div class="tp-head">
            <h2>{crate::exfmt!("Turno {}","Turn {}",a["turn"])}</h2>
            <span class="muted">{format!("{} → {}",hhmm(origin,start),hhmm(origin,end))}</span>
            {status}
            <div class="grow"/>{speed_select}{stop}
            <label class="tp-length" title={crate::i18n::ex("Durata del turno dall’inizio: cambiarla sposta la fine prevista")}>{crate::i18n::ex("Durata")}<input aria-label={crate::i18n::ex("Durata turno (min)")} id="turn-length" type="number" min="1" max="1440" placeholder={((end-start)/60).to_string()} value={(*length).clone()} oninput={{let l=length.clone();Callback::from(move |e|l.set(crate::api::input(e)))}}/>{"min"}</label>
            if let Some(t)=new_end{<button id="turn-length-set" class="tp-small" disabled={ctl_off} title={crate::exfmt!("Sposta la fine del turno a {} ({})","Moves the end of the turn to {} ({})",hhmm(origin,t),clock_at(origin,t))} onclick={set_length}>{crate::exfmt!("Sposta la fine a {}","Move the end to {}",hhmm(origin,t))}</button>}
        </div>
        <div class="tb-wrap">
            if !overdue.is_empty(){<button class="tb-overdue" title={crate::exfmt!("{} elementi pianificati prima di questo turno e ancora da gestire","{} items planned before this turn and still to handle",overdue.len())} onclick={{let t=overdue_targets.clone();Callback::from(move |_|reveal_rows(&t))}}><b>{"⚠ "}{overdue.len()}</b><small>{crate::i18n::ex("in ritardo")}</small></button>}
            <div class="tb-col">
            <div class="tb-track" role="img" aria-label={crate::exfmt!("Avanzamento del turno: ora {}","Turn progress: now {}",text(&a["now_clock"]))}>
                <div class="tb-fill" style={format!("width:{:.2}%",pct(now))}/>
                if let Some(t)=target.filter(|_|resolving){<div class="tb-run" style={format!("left:{:.2}%;width:{:.2}%",pct(now),(pct(t)-pct(now)).max(0.0))}/>}
                {for list(&a["substeps"]).iter().map(|s|html!{<i class="tb-tick" style={format!("left:{:.2}%",pct(s["to_s"].as_i64().unwrap_or(0)))}/>})}
                {for clusters.iter().map(|c|{
                    let state=if c.items.iter().any(|i|i["due"]==true){"due"}else if c.items.iter().any(|i|i["pending"]==true){"pending"}else{"done"};
                    let targets:Vec<(String,String)>=c.items.iter().map(|i|(text(&i["kind"]).to_string(),text(&i["id"]).to_string())).collect();
                    let title=c.items.iter().map(|i|format!("{} · {}",text(&i["clock"]),text(&i["title"]))).collect::<Vec<_>>().join("\n");
                    let key=targets.iter().map(|(k,i)|format!("{k}:{i}")).collect::<Vec<_>>().join(",");
                    let onclick=Callback::from(move |_|reveal_rows(&targets));
                    if let [one]=c.items.as_slice(){
                        let kind=text(&one["kind"]).to_string();
                        html!{<button class={classes!("tb-marker",kind,state)} key={key} style={format!("left:{:.2}%",pct(c.at))} title={title.clone()} aria-label={title} {onclick}/>}
                    }else{
                        let kinds=if c.items.iter().all(|i|i["kind"]=="inject"){"inject"}else if c.items.iter().all(|i|i["kind"]=="action"){"action"}else{"mixed"};
                        html!{<button class={classes!("tb-cluster",kinds,state)} key={key} style={format!("left:{:.2}%",pct(c.at))} title={title.clone()} aria-label={title} {onclick}>{c.items.len()}</button>}
                    }
                })}
                <div class="tb-now" style={format!("left:{:.2}%",pct(now))}><span>{text(&a["now_clock"])}</span></div>
            </div>
            <div class="tb-axis" aria-hidden="true">
                <span class="tb-axis-end start">{hhmm(origin,start)}</span>
                {for time_ticks(origin,start,end).into_iter().map(|t|html!{<><i class={classes!("tb-axis-tick",t.major.then_some("major"))} style={format!("left:{:.2}%",pct(t.t))}/>if let Some(l)=t.label{<span class="tb-axis-label" style={format!("left:{:.2}%",pct(t.t))}>{l}</span>}</>})}
                <span class="tb-axis-end">{hhmm(origin,end)}{" · "}{crate::i18n::ex("fine")}</span>
            </div></div>
        </div>
        <div class="tp-controls">
            if due_inj+due_act>0{<button class="tp-attention" id="tp-attention" title={crate::i18n::ex("Mostra gli elementi nella lista")} onclick={{let t=due_targets.clone();Callback::from(move |_|reveal_rows(&t))}}><b>{crate::i18n::ex("Da gestire ora")}</b>{format!(": {} inject · {} {}",due_inj,due_act,if due_act==1{crate::i18n::ex("azione")}else{crate::i18n::ex("azioni")})}</button>}
            else{<span class="tp-attention none muted" id="tp-attention">{crate::i18n::ex("Nulla da gestire ora")}</span>}
            {if resolving{Html::default()}else{last_advance_summary(&props.state,&props.actions,&props.report,origin)}}
            <div class="grow"/>
            <button id="advance-next" disabled={ctl_off||to_next.is_none()} title={next.map(|t|crate::exfmt!("Porta il tempo a {} e lascia il turno aperto","Moves time to {} and keeps the turn open",clock_at(origin,t)))} onclick={go(to_next)}>{match next{Some(_)=>crate::exfmt!("▶ al prossimo evento ({})","▶ to the next event ({})",text(&a["next_event_clock"])),None=>crate::i18n::ex("▶ nessun evento successivo in programma").to_string()}}</button>
            <span class="tp-step">{crate::i18n::ex("Avanza di")}<input aria-label={crate::i18n::ex("Minuti di avanzamento")} id="advance-step" type="number" min="1" max="1440" value={(*step).clone()} oninput={{let s=step.clone();Callback::from(move |e|s.set(crate::api::input(e)))}}/>{"min"}
                <button id="advance-step-go" disabled={ctl_off} title={crate::exfmt!("Porta il tempo a {} e lascia il turno aperto","Moves time to {} and keeps the turn open",clock_at(origin,now+step_min.max(1) as i64*60))} onclick={go(to_step)}>{"▶"}</button></span>
            <button class="primary" id="resolve" disabled={ctl_off} title={crate::exfmt!("Porta il tempo alle {} e apre il turno {}","Moves time to {} and opens turn {}",hhmm(origin,close_to),a["turn"].as_u64().unwrap_or(1)+1)} onclick={{let onadvance=props.onadvance.clone();Callback::from(move |_|onadvance.emit((close_body.clone(),close_to)))}}>{crate::exfmt!("Chiudi turno {} ▶▶","Close turn {} ▶▶",a["turn"])}</button>
        </div>
    </section>}
}

fn action_status_label(a: &Value) -> &'static str {
    match text(&a["status"]) {
        "approved" => crate::i18n::ex("approvata"),
        "done" => crate::i18n::ex("eseguita"),
        "failed" => crate::i18n::ex("fallita"),
        "skipped" => crate::i18n::ex("saltata"),
        _ if a["run"].is_null() => crate::i18n::ex("in attesa"),
        _ => crate::i18n::ex("rinviata"),
    }
}

#[derive(Properties, PartialEq)]
pub struct ExconTimelineProps {
    pub master: Value,
    pub state: Value,
    pub stat: Value,
    pub responses: BTreeMap<String, Value>,
    /// Report returned by the last advance this console ran (`Null` if none).
    pub last_report: Value,
    pub action: Callback<Action>,
    pub busy: bool,
    pub oninject: Callback<String>,
    /// Advance to the given simulated second with this resolve body.
    pub onadvance: Callback<(Value, i64)>,
}

#[function_component(ExconTimeline)]
pub fn excon_timeline(props: &ExconTimelineProps) -> Html {
    let grouped = use_state(|| true);
    let collapsed = use_state(BTreeSet::<String>::new);
    let opened = use_state(|| Option::<String>::None);
    let expanded = use_state(BTreeSet::<String>::new);
    // Bands the user toggled, against their default (past turns without pending items stay closed).
    let toggled = use_state(BTreeSet::<u32>::new);
    let agenda = &props.master["agenda"];
    let turn = agenda["turn"].as_u64().unwrap_or(0) as u32;
    // Back to the open turn whenever it changes (and on first load).
    use_effect_with(turn, |_| {
        gloo_timers::callback::Timeout::new(120, scroll_to_current_turn).forget();
        || ()
    });
    let roles = list(&props.stat["roles"])
        .iter()
        .filter(|r| text(&r["id"]) != "master")
        .cloned()
        .collect::<Vec<_>>();
    let injects = list(&props.master["planned_injects"]).to_vec();
    let actions = list(&props.master["timeline_actions"]).to_vec();
    let origin = agenda["clock_origin_s"].as_i64().unwrap_or(0);
    let started = !matches!(text(&props.state["phase"]), "briefing" | "");
    let win_start = agenda["turn_start_s"].as_i64().unwrap_or(0);
    let win_end = agenda["planned_end_s"].as_i64().unwrap_or(win_start + 1800);
    let bands = turn_bands(&merge_items(&injects, &actions), turn, win_start, win_end);
    let due_ids: BTreeSet<String> = list(&agenda["items"])
        .iter()
        .filter(|i| i["due"] == true && started)
        .map(|i| format!("{}:{}", text(&i["kind"]), text(&i["id"])))
        .collect();
    let sent_count = injects.iter().filter(|i| i["sent"] == true).count();
    let open_inject = opened
        .as_ref()
        .and_then(|id| injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let toggle_expanded = |key: String| {
        let expanded = expanded.clone();
        Callback::from(move |_| {
            let mut n = (*expanded).clone();
            if !n.remove(&key) {
                n.insert(key.clone());
            }
            expanded.set(n);
        })
    };
    // "in ritardo" (planned before the open turn) vs "raggiunto" (instant reached in it).
    let late_pill = |due: bool, at: i64, reached: &'static str| {
        if !due {
            Html::default()
        } else if at < win_start {
            html! {<span class="pill due-now late">{crate::i18n::ex("in ritardo")}</span>}
        } else {
            html! {<span class="pill due-now">{reached}</span>}
        }
    };
    let row = |i: &Value| {
        let at = item_time(i);
        let time = html! {<span class="pi-time" title={clock_at(origin,at)}>{compact_clock(origin,at)}</span>};
        if i["item"] == "action" {
            let id = text(&i["id"]).to_string();
            let fired = matches!(text(&i["status"]), "done" | "failed");
            let due = due_ids.contains(&format!("action:{id}"));
            let key = format!("a:{id}");
            let open = expanded.contains(&key);
            return html! {<li class={classes!("planned-inject","planned-action","pi",fired.then_some("fired"),due.then_some("is-due"),(due&&at<win_start).then_some("is-late"))} data-action={id.clone()} key={key.clone()}>
                <div class="pi-line">{time}
                    <span class="pill kind action" title={crate::i18n::ex("Azione pianificata")}>{crate::i18n::ex("Azione")}</span>
                    <button class="pi-toggle" aria-expanded={open.to_string()} onclick={toggle_expanded(key)}><span class="pi-chev">{if open{"▾"}else{"▸"}}</span><b class="planned-title">{text(&i["title"])}</b></button>
                    <span class="pill rcpt">{action_status_label(i)}</span>
                    {late_pill(due,at,crate::i18n::ex("raggiunta: decidi all’avanzamento"))}
                    if let Some(r)=i["run"]["result"].as_str(){<small class="pi-result muted" title={r.to_string()}>{r}</small>}</div>
                if open{<div class="pi-detail"><span class="planned-text">{text(&i["summary"])}</span>
                    if !text(&i["note"]).is_empty(){<small class="planned-objective">{text(&i["note"])}</small>}</div>}</li>};
        }
        let id = text(&i["id_inject"]).to_string();
        let sent = i["sent"] == true;
        let complete = sent && list(&i["not_yet_sent_to"]).is_empty();
        let due = due_ids.contains(&format!("inject:{id}"));
        let key = format!("i:{id}");
        let open = expanded.contains(&key);
        let onclick = {
            let opened = opened.clone();
            let id = id.clone();
            Callback::from(move |_| opened.set(Some(id.clone())))
        };
        html! {<li class={classes!("planned-inject","pi",sent.then_some("is-sent"),complete.then_some("is-complete"),sent.then_some("fired"),due.then_some("is-due"),(due&&at<win_start).then_some("is-late"))} data-inject={id.clone()} key={key.clone()}>
            <div class="pi-line">{time}
                <span class="pill kind inject" title={crate::i18n::ex("Inject pianificato")}>{crate::i18n::ex("Inject")}</span>
                <button class="pi-toggle" aria-expanded={open.to_string()} onclick={toggle_expanded(key)}><span class="pi-chev">{if open{"▾"}else{"▸"}}</span><b class="planned-title">{text(&i["title"])}</b></button>
                {late_pill(due,at,crate::i18n::ex("raggiunto: da inviare"))}
                if sent{<span class="pill rcpt sent">{if complete{crate::i18n::ex("Inviato")}else{crate::i18n::ex("Inviato in parte")}}</span>}
                <small class="pi-recips muted">{recipients_summary(i)}</small>
                <button class="planned-open" {onclick} disabled={props.busy} aria-label={crate::exfmt!("Apri l’invio dell’inject {}","Open sending for inject {}",text(&i["title"]))}>{if complete{crate::i18n::ex("Altri destinatari…")}else{crate::i18n::ex("Invia…")}}</button></div>
            if open{<div class="pi-detail"><span class="planned-text">{text(&i["text"])}</span>
                if !text(&i["objective"]).is_empty(){<small class="planned-objective">{crate::exfmt!("Obiettivo: {}","Objective: {}",text(&i["objective"]))}</small>}
                {pills(i,&roles)}</div>}</li>}
    };
    let panel = (!agenda.is_null() && matches!(text(&props.state["phase"]), "orders" | "resolving")).then(|| {
        html! {<TurnPanel agenda={agenda.clone()} state={props.state.clone()} actions={actions.clone()} report={props.last_report.clone()} busy={props.busy} action={props.action.clone()} onadvance={props.onadvance.clone()}/>}
    });
    html! {<div class="timeline-layout">{panel.unwrap_or_default()}<section class="card" id="excon-timeline"><div class="row tl-head"><h2>{crate::i18n::ex("Timeline EXCON")}</h2>
        <small class="muted">{crate::exfmt!("{} inject · {} inviati","{} injects · {} sent",injects.len(),sent_count)}</small><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="group-by-event" checked={*grouped} onchange={{let g=grouped.clone();Callback::from(move |e:Event|{use yew::TargetCast;g.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("raggruppa per evento")}</label>
        <button class="primary" data-inject-all="" disabled={props.busy} onclick={{let cb=props.oninject.clone();Callback::from(move |_|cb.emit("all".into()))}}>{crate::i18n::ex("Inject personalizzato")}</button></div>
        if injects.is_empty()&&actions.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject pianificato")}</p>}
        {for bands.iter().map(|band|{
            let waiting=band.items.iter().filter(|i|{let k=if i["item"]=="action"{format!("action:{}",text(&i["id"]))}else{format!("inject:{}",text(&i["id_inject"]))};due_ids.contains(&k)}).count();
            let default_open=match band.state{BandState::Past=>waiting>0,_=>true};
            let is_open=band.state==BandState::Current||(default_open!=toggled.contains(&band.turn));
            let state_label=match band.state{BandState::Past=>crate::i18n::ex("concluso"),BandState::Current if started=>crate::i18n::ex("in corso"),BandState::Current=>crate::i18n::ex("da iniziare"),BandState::Future=>crate::i18n::ex("previsto")};
            let toggle={let toggled=toggled.clone();let t=band.turn;Callback::from(move |_|{let mut n=(*toggled).clone();if !n.remove(&t){n.insert(t);}toggled.set(n);})};
            let class=classes!("turn-band",match band.state{BandState::Past=>"past",BandState::Current=>"current",BandState::Future=>"future"});
            let groups=group_by_event(&band.items,*grouped);
            html!{<section {class} data-band={band.turn.to_string()} data-band-current={(band.state==BandState::Current).then_some("")} key={band.turn}>
                <button class="band-head" aria-expanded={is_open.to_string()} disabled={band.state==BandState::Current} onclick={toggle}>
                    <span>{if is_open{"▾"}else{"▸"}}</span><b>{crate::exfmt!("Turno {}","Turn {}",band.turn)}</b>
                    <span class="muted">{format!("{}–{}",hhmm(origin,band.start_s),hhmm(origin,band.end_s))}</span>
                    <span class="pill band-state">{state_label}</span>
                    if waiting>0{<span class="pill due-now">{crate::exfmt!("{} da gestire","{} to handle",waiting)}</span>}
                    <small class="muted band-count">{band.items.len()}</small></button>
                if is_open{
                    if band.items.is_empty(){<p class="muted band-empty">{crate::i18n::ex("Nessun elemento pianificato in questo turno")}</p>}
                    {for groups.iter().map(|(event,items)|{
                        let is_closed=!event.is_empty()&&collapsed.contains(&format!("{}:{event}",band.turn));
                        let list_html=html!{<ol class="planned-list">{for items.iter().map(|i|row(i))}</ol>};
                        if event.is_empty(){list_html}else{
                            let toggle={let collapsed=collapsed.clone();let k=format!("{}:{event}",band.turn);Callback::from(move |_|{let mut n=(*collapsed).clone();if !n.remove(&k){n.insert(k.clone());}collapsed.set(n);})};
                            html!{<section class="event-group" data-event={event.clone()}><button class="event-head" aria-expanded={(!is_closed).to_string()} onclick={toggle}><span>{if is_closed{"▸"}else{"▾"}}</span><b>{event.clone()}</b><small>{items.len()}</small></button>if !is_closed{{list_html}}</section>}
                        }
                    })}
                }</section>}
        })}
        if let Some(inject)=open_inject{<SendDialog key={text(&inject["id_inject"]).to_string()} inject={inject.clone()} roles={roles.clone()} busy={props.busy} action={props.action.clone()} onclose={{let o=opened.clone();Callback::from(move |_|o.set(None))}}/>}
    </section></div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offset_format() {
        assert_eq!(format_offset(750), "+00:12:30");
        assert_eq!(format_offset(-5), "-00:00:05");
    }
    #[test]
    fn defaults_skip_served() {
        let i = json!({"recipients_planned":["a","b","c"],"sent_to":["b"]});
        assert_eq!(default_recipients(&i).into_iter().collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!(send_body(&default_recipients(&i)), json!({"recipients":["a","c"]}));
    }
    #[test]
    fn merged_chronologically() {
        let i = vec![json!({"id_inject":"i1","scenario_time_s":600}), json!({"id_inject":"i2","scenario_time_s":0})];
        let a = vec![json!({"id":"a1","at_s":300}), json!({"id":"a2","at_s":600})];
        let m = merge_items(&i, &a);
        let kinds = m.iter().map(|v| v["item"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(kinds, ["inject", "action", "inject", "action"]);
        assert_eq!(m[1]["id"], "a1");
    }
    #[test]
    fn track_positions_clamp_to_the_window() {
        assert_eq!(track_pct(450, 0, 1800), 25.0);
        assert_eq!(track_pct(-60, 0, 1800), 0.0);
        assert_eq!(track_pct(5000, 0, 1800), 100.0);
    }
    #[test]
    fn advance_targets_follow_the_agenda() {
        let [next, step] = advance_targets(300, Some(600), 5);
        assert_eq!(next.unwrap().0, 600);
        assert_eq!(step.unwrap().0, 600);
        assert!(advance_targets(1800, None, 5)[0].is_none());
        assert_eq!(close_target(300, 1800).1["until"], "turn_end");
        assert_eq!(close_target(1800, 1800).0, 1860);
    }
    #[test]
    fn grouping() {
        let v = vec![json!({"event":"E1"}), json!({"event":""}), json!({"event":"E1"}), json!({"event":""})];
        let g = group_by_event(&v, true);
        assert_eq!(g.iter().map(|(e, l)| (e.as_str(), l.len())).collect::<Vec<_>>(), [("E1", 2), ("", 2)]);
        assert_eq!(group_by_event(&v, false).len(), 1);
    }
    const NOON: i64 = 12 * 3600;
    #[test]
    fn clocks() {
        assert_eq!(hhmm(NOON, 3900), "13:05");
        assert_eq!(clock_at(NOON, 3900), "Day 1 13:05");
        assert_eq!(compact_clock(NOON, 3900), "13:05");
        assert_eq!(compact_clock(NOON, 13 * 3600), "Day 2 01:00");
    }
    #[test]
    fn ticks_sit_on_round_clock_times() {
        // 13:30 → 14:00: only 13:40 and 13:50 are inside; the hour is the end.
        let t = time_ticks(13 * 3600 + 1800, 0, 1800);
        assert_eq!(t.iter().map(|t| t.t).collect::<Vec<_>>(), [600, 1200]);
        assert_eq!(t[0].label.as_deref(), Some("13:40"));
        // 13:29 → 13:59: the 13:30 tick is too close to the start label.
        let t = time_ticks(13 * 3600 + 29 * 60, 0, 1800);
        assert_eq!(t.iter().map(|t| (t.t, t.major)).collect::<Vec<_>>(), [(60, false), (660, false), (1260, false)]);
        assert_eq!(t[0].label, None);
        assert_eq!(t[1].label.as_deref(), Some("13:40"));
        let t = time_ticks(13 * 3600, 0, 3 * 3600);
        assert_eq!(t.len(), 5);
        assert!(t.iter().filter(|t| t.major).count() == 2);
        assert!(time_ticks(0, 0, 1).is_empty());
    }
    #[test]
    fn simultaneous_items_share_one_badge() {
        let item = |id: &str, at: i64| json!({"id": id, "kind": "inject", "at_s": at});
        let items = vec![item("o1", -600), item("o2", -300), item("a", 0), item("b", 0), item("c", 0), item("d", 0), item("e", 900)];
        let (clusters, overdue) = cluster_items(&items, 0, 1800, 1.0);
        assert_eq!(overdue.len(), 2, "earlier turns go to their own zone");
        assert_eq!(clusters.iter().map(|c| (c.at, c.items.len())).collect::<Vec<_>>(), [(0, 4), (900, 1)]);
        // Close but not equal instants merge when they would overlap on screen.
        let (clusters, _) = cluster_items(&[item("a", 0), item("b", 10), item("c", 400)], 0, 1800, 1.0);
        assert_eq!(clusters.iter().map(|c| c.items.len()).collect::<Vec<_>>(), [2, 1]);
    }
    #[test]
    fn attention_counts_what_is_due() {
        let items = vec![
            json!({"kind": "inject", "due": true}),
            json!({"kind": "inject", "due": false}),
            json!({"kind": "action", "due": true}),
            json!({"kind": "action", "due": true}),
        ];
        assert_eq!(attention_counts(&items), (1, 2));
    }
    #[test]
    fn unsent_injects_are_reached_or_crossed() {
        let i = |id: &str, at: i64, left: &[&str]| json!({"id_inject": id, "scenario_time_s": at, "not_yet_sent_to": left});
        let injects = vec![i("old", 60, &["a"]), i("now", 300, &["a"]), i("soon", 900, &["a"]), i("late", 1800, &["a"]), i("done", 0, &[])];
        let got = unsent_injects(&injects, 300, 1800, 120);
        let ids: Vec<(&str, Reach)> = got.iter().map(|(v, r)| (v["id_inject"].as_str().unwrap(), *r)).collect();
        assert_eq!(ids, [("old", Reach::Overdue), ("now", Reach::Reached), ("soon", Reach::Crossed)]);
        // Stopping at the next event only raises what is already reached.
        assert_eq!(unsent_injects(&injects, 300, 900, 120).len(), 2);
    }
    #[test]
    fn advance_wording_states_target_and_effect() {
        let body = advance_targets(0, Some(300), 5)[0].clone().unwrap().1;
        assert_eq!(advance_kind(&body), AdvanceKind::Event);
        assert_eq!(advance_kind(&advance_targets(0, None, 5)[1].clone().unwrap().1), AdvanceKind::Step);
        assert_eq!(advance_kind(&close_target(0, 1800).1), AdvanceKind::Close);
        let (title, button) = advance_wording(AdvanceKind::Event, 1, "Day 1 13:05", "13:05");
        assert_eq!(title, "Avanza a Day 1 13:05 · il turno resta aperto");
        assert_eq!(button, "Avanza a 13:05");
        let (title, button) = advance_wording(AdvanceKind::Close, 1, "Day 1 13:30", "13:30");
        assert_eq!(title, "Chiudi turno 1 alle 13:30 · si apre il turno 2");
        assert_eq!(button, "Chiudi turno 1");
    }
    #[test]
    fn recipients_in_a_few_words() {
        let s = |p: &[&str], d: &[&str]| recipients_summary(&json!({"recipients_planned": p, "sent_to": d}));
        assert_eq!(s(&["a", "b"], &[]), "da inviare a 2");
        assert_eq!(s(&["a", "b"], &["a"]), "inviato a 1 su 2");
        assert_eq!(s(&["a", "b"], &["a", "b"]), "inviato a 2");
        assert_eq!(s(&[], &[]), "");
    }
    #[test]
    fn bands_follow_the_turn_length() {
        let inj = |id: &str, at: i64| json!({"item": "inject", "id_inject": id, "scenario_time_s": at});
        // Turn 2 is open over 1800..3600.
        let items = vec![inj("a", 0), inj("b", 1700), inj("c", 1800), inj("d", 3599), inj("e", 3600), inj("f", 9000)];
        let bands = turn_bands(&items, 2, 1800, 3600);
        let shape: Vec<(u32, BandState, usize)> = bands.iter().map(|b| (b.turn, b.state, b.items.len())).collect();
        assert_eq!(shape, [(1, BandState::Past, 2), (2, BandState::Current, 2), (3, BandState::Future, 1), (6, BandState::Future, 1)]);
        assert_eq!((bands[0].start_s, bands[0].end_s), (0, 1800));
        assert_eq!((bands[3].start_s, bands[3].end_s), (9000, 10800));
        // The open turn is listed even when empty.
        let bands = turn_bands(&[inj("a", 0)], 2, 1800, 3600);
        assert_eq!(bands.iter().map(|b| (b.turn, b.items.len())).collect::<Vec<_>>(), [(1, 1), (2, 0)]);
    }
    #[test]
    fn executed_actions_of_an_advance() {
        let a = |t: &str, at: Option<i64>, st: &str| json!({"title": t, "run": {"executed_s": at, "status": st, "result": "108 celle"}});
        let list = vec![a("Innesco", Some(0), "done"), a("Vento", Some(300), "failed"), a("Altro", None, "approved")];
        assert_eq!(executed_actions(&list, 0, 300), [("Innesco".to_string(), true, "108 celle".to_string())]);
        assert_eq!(executed_actions(&list, 300, 900).len(), 1);
    }
    #[test]
    fn speed_choices_keep_the_current_value() {
        assert_eq!(speed_options(60.0), [0.0, 10.0, 60.0, 120.0]);
        assert_eq!(speed_options(30.0), [0.0, 10.0, 30.0, 60.0, 120.0]);
        assert_eq!(speed_label(60.0), "60×");
        assert_eq!(remaining_min(0, 61), 2);
        assert_eq!(remaining_min(100, 50), 0);
    }
}
