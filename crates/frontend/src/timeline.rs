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

/// Send slot of an inject: its sends run beside other actions, one at a time.
pub fn send_slot(id: &str) -> String {
    format!("msel:{id}")
}

/// Body for `POST /api/master/msel/<id>/send` with the actions to run.
pub fn send_body_with(selected: &BTreeSet<String>, actions: &BTreeSet<String>) -> Value {
    json!({"recipients": selected.iter().collect::<Vec<_>>(), "actions": actions.iter().collect::<Vec<_>>()})
}

/// Actions the send dialog offers: those of an inject not yet sent whose fate
/// is still open (not yet run), plus pre-actions switched off beforehand.
pub fn sendable_actions(inject: &Value) -> Vec<Value> {
    if !list(&inject["sent_to"]).is_empty() {
        return Vec::new();
    }
    list(&inject["actions"])
        .iter()
        .filter(|a| a["status"].is_null() || (a["pre"] == true && a["enabled"] == false))
        .cloned()
        .collect()
}

/// Actions ticked by default in the send dialog: all the offered ones that are
/// not switched off.
pub fn default_actions(inject: &Value) -> BTreeSet<String> {
    sendable_actions(inject)
        .iter()
        .filter(|a| a["enabled"] != false)
        .map(|a| text(&a["id"]).to_string())
        .collect()
}

/// An action's offset as a chip: `−5 min`, `+2 min`, `all’invio`.
/// Readable summary of an inject action; reveal actions are described in the EXCON language.
fn action_summary(a: &Value) -> String {
    use crate::i18n_excon::ex;
    let r = &a["reveal"];
    if !r.is_object() {
        return text(&a["summary"]).to_string();
    }
    if r["kind"] == "approximate" {
        return format!(
            "{} · ±{:.0} m · {} ±{:.0}%",
            ex("Stima sulla mappa (non verificata)"),
            r["position_error_m"].as_f64().unwrap_or(0.),
            ex("area"),
            r["area_error_frac"].as_f64().unwrap_or(0.) * 100.
        );
    }
    let zones = match &r["zones"] {
        Value::String(s) if s == "burning" => ex("zone in fiamme").to_string(),
        Value::String(s) if s == "burning_and_adjacent" => ex("zone in fiamme e adiacenti").to_string(),
        z => list(z).iter().map(|x| text(x).to_string()).collect::<Vec<_>>().join(", "),
    };
    let layers = list(&r["layers"])
        .iter()
        .map(|l| match text(l) {
            "fire" => ex("Incendio"),
            "buildings" => ex("Edifici"),
            "roads" => ex("Strade"),
            "lines" => ex("Linee e lanci"),
            "units" => ex("Squadre"),
            other => other,
        }
        .to_lowercase())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}: {zones} · {layers}", ex("Mappa rivelata"))
}

pub fn offset_chip(offset_s: i64) -> String {
    let m = (offset_s.abs() as f64 / 60.0 * 10.0).round() / 10.0;
    let m = if m.fract() == 0.0 { format!("{}", m as i64) } else { format!("{m:.1}") };
    match offset_s.signum() {
        0 => crate::i18n::ex("all’invio").to_string(),
        1 => format!("+{m} min"),
        _ => format!("−{m} min"),
    }
}

/// The actions of an inject split into those before it (pre-actions, most
/// anticipated first) and those from its send on, each in time order.
pub fn split_actions(inject: &Value) -> (Vec<Value>, Vec<Value>) {
    let mut all = list(&inject["actions"]).to_vec();
    all.sort_by_key(|a| a["offset_s"].as_i64().unwrap_or(0));
    all.into_iter().partition(|a| a["pre"] == true)
}

/// Pre-actions an advance up to `to_s` runs by itself: not yet run, from
/// injects neither sent nor skipped, with their instant before `to_s`.
/// `(inject, action)`, chronological.
pub fn crossed_pre_actions(injects: &[Value], _now: i64, to_s: i64) -> Vec<(Value, Value)> {
    let mut out: Vec<(i64, Value, Value)> = injects
        .iter()
        .filter(|i| list(&i["sent_to"]).is_empty() && i["skipped"] != true)
        .flat_map(|i| {
            list(&i["actions"])
                .iter()
                .filter(|a| a["pre"] == true && a["status"].is_null() || (a["pre"] == true && a["enabled"] == false && a["status"] == "not_run"))
                .filter_map(|a| {
                    let t = a["instant_s"].as_i64()?;
                    (t <= to_s).then(|| (t, i.clone(), a.clone()))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort_by_key(|(t, ..)| *t);
    out.into_iter().map(|(_, i, a)| (i, a)).collect()
}

/// Status of an action in a few words.
pub fn action_status_text(inject: &Value, a: &Value) -> String {
    match text(&a["status"]) {
        "approved" => match text(&a["run_at_clock"]) {
            "" => crate::i18n::ex("approvata").to_string(),
            c => crate::exfmt!("approvata alle {}", "approved at {}", c.rsplit(' ').next().unwrap_or(c)),
        },
        "done" => crate::i18n::ex("eseguita").to_string(),
        "failed" => crate::i18n::ex("fallita").to_string(),
        "not_run" => crate::i18n::ex("non eseguita").to_string(),
        _ if inject["skipped"] == true && a["pre"] == true => crate::i18n::ex("annullata").to_string(),
        _ => crate::i18n::ex("pianificata").to_string(),
    }
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

/// Injects in chronological order (stable).
pub fn sorted_injects(injects: &[Value]) -> Vec<Value> {
    let mut rows = injects.to_vec();
    rows.sort_by_key(|i| i["scenario_time_s"].as_i64().unwrap_or(0));
    rows
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
pub struct SendDialogProps {
    pub inject: Value,
    pub roles: Vec<Value>,
    pub busy: bool,
    pub action: Callback<Action>,
    pub onclose: Callback<()>,
}

#[function_component(SendDialog)]
pub fn send_dialog(props: &SendDialogProps) -> Html {
    let inject = &props.inject;
    let id = text(&inject["id_inject"]).to_string();
    let served = strings(&inject["sent_to"]);
    let selected = use_state(|| default_recipients(inject));
    let chosen_actions = use_state(|| default_actions(inject));
    let offered = sendable_actions(inject);
    let all_roles = props.roles.iter().map(|r| text(&r["id"]).to_string()).collect::<Vec<_>>();
    let planned = strings(&inject["recipients_planned"]);
    let send = {
        let (selected, chosen_actions) = (selected.clone(), chosen_actions.clone());
        let action = props.action.clone();
        let onclose = props.onclose.clone();
        let id = id.clone();
        Callback::from(move |_| {
            action.emit(Action::post(format!("/api/master/msel/{id}/send"), send_body_with(&selected, &chosen_actions)).in_slot(send_slot(&id)));
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
        if !offered.is_empty(){<h3>{crate::i18n::ex("Azioni")}</h3>
            <p class="muted">{crate::i18n::ex("Cambiamenti del mondo che accompagnano l’inject: l’EXCON li esegue con l’invio (quelli con ritardo, durante il prossimo avanzamento). Togli la spunta per non eseguirli.")}</p>
            <ul class="send-action-list">{for offered.iter().map(|a|{
                let aid=text(&a["id"]).to_string();
                let off=a["enabled"]==false;
                let checked=chosen_actions.contains(&aid);
                let onchange={let chosen_actions=chosen_actions.clone();let aid=aid.clone();Callback::from(move |e:Event|{
                    use yew::TargetCast;
                    let on=e.target_unchecked_into::<web_sys::HtmlInputElement>().checked();
                    let mut next=(*chosen_actions).clone();
                    if on{next.insert(aid.clone());}else{next.remove(&aid);}
                    chosen_actions.set(next);
                })};
                html!{<li key={aid.clone()}><label class="send-role"><input type="checkbox" data-send-action={aid.clone()} checked={checked} disabled={off||props.busy} {onchange}/>
                    <span class="pi-off">{offset_chip(a["offset_s"].as_i64().unwrap_or(0))}</span> <b>{text(&a["title"])}</b> <span class="muted">{action_summary(a)}</span>
                    if off{<small>{crate::i18n::ex("disattivata nella timeline")}</small>}</label></li>}
            })}</ul>}
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

/// Injects that are due now: reached (or overdue), neither sent nor skipped.
pub fn attention_count(items: &[Value]) -> usize {
    items.iter().filter(|i| i["due"] == true).count()
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
        .filter(|i| !list(&i["not_yet_sent_to"]).is_empty() && i["skipped"] != true)
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
    /// Up to the next pending inject; the turn stays open.
    Inject,
    /// By a number of minutes; the turn stays open.
    Step,
    /// To the end of the turn, which opens the next one.
    Close,
}

/// `closes_turn` is the agenda's statement that the advance to the next inject
/// reaches the turn's planned end.
pub fn advance_kind(body: &Value, closes_turn: bool) -> AdvanceKind {
    if matches!(body["until"].as_str(), Some("next_inject" | "next_event")) {
        if closes_turn {
            AdvanceKind::Close
        } else {
            AdvanceKind::Inject
        }
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

/// Position of an inject in the list: its scenario second.
pub fn item_time(item: &Value) -> i64 {
    item["scenario_time_s"].as_i64().unwrap_or(0)
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

/// Inject actions executed during `[from_s, to_s)`: `(title, ok, result)`,
/// from the flat list of action runs.
pub fn executed_actions(runs: &[Value], from_s: i64, to_s: i64) -> Vec<(String, bool, String)> {
    runs.iter()
        .filter(|a| a["executed_s"].as_i64().is_some_and(|t| t >= from_s && t < to_s))
        .map(|a| (text(&a["title"]).to_string(), a["status"] == "done", text(&a["result"]).to_string()))
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

/// Scroll the list so that the open turn's band header is at the top, under
/// the sticky list header (`scroll-margin-top`); its waiting rows follow it.
fn scroll_to_current_turn() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Some(t) = doc.query_selector("[data-band-current]>.band-head").ok().flatten() {
        t.scroll_into_view();
    }
}

// ---------------------------------------------------------------------------
// Pre-advance review

#[derive(Properties, PartialEq)]
pub struct AdvanceReviewProps {
    /// Resolve body of the advance being started.
    pub body: Value,
    /// Simulated second the advance goes to.
    pub to_s: i64,
    /// Planned injects as of now, so sending one or switching an action
    /// updates the list.
    pub injects: Vec<Value>,
    pub agenda: Value,
    pub roles: Vec<Value>,
    pub busy: bool,
    /// Inject sends in flight (slots, see `send_slot`).
    pub pending: BTreeSet<String>,
    pub action: Callback<Action>,
    pub onclose: Callback<()>,
}

/// Asked at every advance that has something to handle: the injects whose
/// instant was reached or will be crossed but are still unsent (send them,
/// skip them, or advance anyway), and the pre-actions the advance runs by
/// itself, each with its switch.
#[function_component(AdvanceReviewDialog)]
pub fn advance_review_dialog(props: &AdvanceReviewProps) -> Html {
    let sending = use_state(|| Option::<String>::None);
    let a = &props.agenda;
    let origin = a["clock_origin_s"].as_i64().unwrap_or(0);
    let now = a["now_s"].as_i64().unwrap_or(0);
    let start = a["turn_start_s"].as_i64().unwrap_or(0);
    let turn = a["turn"].as_u64().unwrap_or(1);
    let unsent = unsent_injects(&props.injects, now, props.to_s, start);
    let pre = crossed_pre_actions(&props.injects, now, props.to_s);
    let kind = advance_kind(&props.body, a["advance"]["closes_turn"] == true);
    let (title, confirm_label) = advance_wording(kind, turn, &clock_at(origin, props.to_s), &hhmm(origin, props.to_s));
    let confirm = {
        let (action, onclose, body) = (props.action.clone(), props.onclose.clone(), props.body.clone());
        Callback::from(move |_| {
            action.emit(Action::post("/api/master/resolve", body.clone()));
            onclose.emit(());
        })
    };
    let open_inject = sending
        .as_ref()
        .and_then(|id| props.injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let nothing = pre.is_empty() && unsent.is_empty();
    let confirm_class = classes!("primary", (!unsent.is_empty()).then_some("warn"));
    html! {<div class="modal-backdrop"><section class="inject-modal send-dialog advance-review" role="dialog" aria-modal="true" aria-labelledby="due-dialog-title">
        <div class="row"><h2 id="due-dialog-title">{title}</h2><div class="grow"/><button onclick={{let c=props.onclose.clone();Callback::from(move |_|c.emit(()))}}>{crate::i18n::ex("Annulla")}</button></div>
        <p class="muted">{crate::exfmt!("Ora {} → {}.","Now {} → {}.",clock_at(origin,now),clock_at(origin,props.to_s))}</p>
        if nothing{<p>{crate::i18n::ex("Non c’è più nulla da gestire.")}</p>}
        if !unsent.is_empty(){<h3>{crate::exfmt!("Inject non ancora inviati ({})","Injects not yet sent ({})",unsent.len())}</h3>
            <p class="muted">{crate::i18n::ex("Gli istanti previsti sono raggiunti o attraversati da questo avanzamento: inviali ora, scegli di non inviarli, oppure avanza comunque.")}</p>
            <ul class="planned-list">{for unsent.iter().map(|(inj,reach)|{
                let id=text(&inj["id_inject"]).to_string();
                let why=match reach{Reach::Overdue=>crate::i18n::ex("in ritardo"),Reach::Reached=>crate::i18n::ex("raggiunto"),Reach::Crossed=>crate::i18n::ex("attraversato dall’avanzamento")};
                let at=inj["scenario_time_s"].as_i64().unwrap_or(0);
                let in_flight=props.pending.contains(&send_slot(&id));
                let send_now={let (action,id)=(props.action.clone(),id.clone());let body=send_body(&default_recipients(inj));
                    Callback::from(move |_|action.emit(Action::post(format!("/api/master/msel/{id}/send"),body.clone()).in_slot(send_slot(&id))))};
                let skip={let (action,id)=(props.action.clone(),id.clone());Callback::from(move |_|action.emit(Action::post(format!("/api/master/msel/{id}/skip"),json!({}))))};
                let choose={let (sending,id)=(sending.clone(),id.clone());Callback::from(move |_|sending.set(Some(id.clone())))};
                html!{<li class="planned-inject review-row" data-review-inject={id.clone()} key={id.clone()}>
                    <span class="planned-times"><b>{compact_clock(origin,at)}</b></span>
                    <span class="planned-body"><span class="row"><b class="planned-title">{text(&inj["title"])}</b><span class={classes!("pill","due-now",(*reach==Reach::Overdue).then_some("late"))}>{why}</span></span>
                        <small class="muted">{recipients_summary(inj)}</small></span>
                    <span class="review-send"><button class="primary" data-review-send="" disabled={props.busy||in_flight} aria-busy={in_flight.to_string()} onclick={send_now}>{if in_flight{html!{<><span class="loader" aria-hidden="true"></span>{crate::i18n::ex("Invio in corso…")}</>}}else{html!{crate::i18n::ex("Invia ora")}}}</button>
                        <button disabled={props.busy} onclick={choose}>{crate::i18n::ex("Destinatari…")}</button>
                        <button data-review-skip="" disabled={props.busy} onclick={skip}>{crate::i18n::ex("Non inviare")}</button></span></li>}
            })}</ul>}
        if !pre.is_empty(){<h3>{crate::exfmt!("Azioni che partono da sole ({})","Actions that run by themselves ({})",pre.len())}</h3>
            <p class="muted">{crate::i18n::ex("Azioni che precedono il loro inject: l’avanzamento le esegue all’istante previsto. Disattivane una per non eseguirla.")}</p>
            <ul class="planned-list">{for pre.iter().map(|(inj,act)|{
                let (iid,aid)=(text(&inj["id_inject"]).to_string(),text(&act["id"]).to_string());
                let enabled=act["enabled"]!=false;
                let toggle={let (action,iid,aid)=(props.action.clone(),iid.clone(),aid.clone());Callback::from(move |_|action.emit(Action::post(format!("/api/master/msel/{iid}/actions/{aid}/enabled"),json!({"enabled":!enabled}))))};
                html!{<li class="planned-inject review-row" data-review-pre={format!("{iid}:{aid}")} key={format!("{iid}:{aid}")}>
                    <span class="planned-times"><b>{compact_clock(origin,act["instant_s"].as_i64().unwrap_or(0))}</b><small>{offset_chip(act["offset_s"].as_i64().unwrap_or(0))}</small></span>
                    <span class="planned-body"><b class="planned-title">{text(&act["title"])}</b>
                        <span class="planned-text">{action_summary(act)}</span>
                        <small class="muted">{crate::exfmt!("prima dell’inject «{}»","before inject «{}»",text(&inj["title"]))}</small></span>
                    <span class="review-send"><label class="inline-check"><input type="checkbox" data-pre-toggle={aid.clone()} checked={enabled} disabled={props.busy} onchange={toggle}/> {crate::i18n::ex("esegui")}</label></span></li>}
            })}</ul>}
        <div class="row send-actions"><div class="grow"/>
            <button class={confirm_class} data-due-confirm="" disabled={props.busy||!props.pending.is_empty()} title={(!props.pending.is_empty()).then(|| crate::i18n::ex("Attendi la fine degli invii in corso"))} onclick={confirm}>{if unsent.is_empty(){confirm_label}else{crate::exfmt!("Avanza comunque · {}","Advance anyway · {}",confirm_label)}}</button></div>
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
    roles: Vec<Value>,
    /// Report returned by the last advance, when this console ran it.
    report: Value,
    busy: bool,
    action: Callback<Action>,
    onadvance: Callback<(Value, i64)>,
    #[prop_or_default]
    onview: Callback<String>,
    /// `reply_review.counts`: open questions and suggestions waiting for the EXCON.
    #[prop_or_default]
    review: Value,
}

/// "Domande aperte · azioni da decidere" for the turn panel (open questions
/// stay visible, above all before closing the turn); opens Risposte.
fn review_chip(counts: &Value, onview: &Callback<String>) -> Html {
    let n = |k: &str| counts[k].as_u64().unwrap_or(0);
    let (questions, actions) = (n("drafts") + n("awaiting_answer"), n("complete") + n("infeasible") + n("incomplete"));
    if questions + actions == 0 {
        return Html::default();
    }
    let v = onview.clone();
    html! {<button class="tp-review" id="tp-review" title={crate::exfmt!("Apri Risposte: azioni suggerite dalle risposte e domande ai player","Open Responses: actions suggested by the replies and questions to players")} onclick={Callback::from(move |_|v.emit("risposte".into()))}>
        if questions>0{<span><b>{questions}</b>{if questions==1{crate::exfmt!(" domanda aperta"," open question")}else{crate::exfmt!(" domande aperte"," open questions")}}</span>}
        if questions>0&&actions>0{{" · "}}
        if actions>0{<span><b>{actions}</b>{if actions==1{crate::exfmt!(" azione da decidere"," action to decide")}else{crate::exfmt!(" azioni da decidere"," actions to decide")}}</span>}
    </button>}
}

/// Position of `t` on a track from `start` to `end`, in percent.
pub fn track_pct(t: i64, start: i64, end: i64) -> f64 {
    ((t - start) as f64 / (end - start).max(1) as f64 * 100.0).clamp(0.0, 100.0)
}

/// The primary advance: where "advance to the next inject" goes according to
/// the agenda: `(target second, resolve body, closes the turn)`.
pub fn next_inject_advance(agenda: &Value) -> Option<(i64, Value, bool)> {
    let adv = &agenda["advance"];
    Some((adv["to_s"].as_i64()?, json!({"until": "next_inject"}), adv["closes_turn"] == true))
}

/// Label of the primary advance button.
pub fn advance_label(agenda: &Value) -> String {
    let adv = &agenda["advance"];
    if adv["closes_turn"] == true {
        crate::exfmt!("▶▶ Avanza e chiudi turno {} ({})", "▶▶ Advance and close turn {} ({})", agenda["turn"], text(&adv["clock"]).rsplit(' ').next().unwrap_or(""))
    } else {
        crate::exfmt!("▶▶ Avanza al prossimo inject ({})", "▶▶ Advance to the next inject ({})", text(&adv["clock"]).rsplit(' ').next().unwrap_or(""))
    }
}

/// Intermediate advance by `step_min` minutes; the turn stays open.
pub fn step_target(now: i64, step_min: u32) -> (i64, Value) {
    (now + step_min.max(1) as i64 * 60, json!({"substep": true, "minutes": step_min.max(1)}))
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

/// Roles that received a zone reveal and the zones a refresh would repeat for
/// each: `(role name, zones, layers)`.
pub fn refresh_plan(state: &Value, roles: &[Value]) -> Vec<(String, Vec<String>, Vec<String>)> {
    let strings = |v: &Value| list(v).iter().map(|x| text(x).to_string()).collect::<Vec<_>>();
    state["role_knowledge"].as_object().into_iter().flatten().filter(|(_, k)| k["refresh"].is_object()).map(|(r, k)| (role_name(roles, r), strings(&k["refresh"]["zones"]), strings(&k["refresh"]["layers"]))).collect()
}

/// «Update role maps»: manual, one click. Each role that received a zone
/// reveal gets the same zones and layers again as a fresh snapshot. Compact:
/// the roles and zones it will touch are listed before the click.
fn refresh_maps(state: &Value, roles: &[Value], disabled: bool, action: &Callback<Action>) -> Html {
    let plan = refresh_plan(state, roles);
    if plan.is_empty() {
        return Html::default();
    }
    let zones = |z: &[String]| if z.len() > 6 { format!("{}…", z[..6].join(" ")) } else { z.join(" ") };
    let list_text = plan.iter().map(|(r, z, _)| format!("{r} ({})", zones(z))).collect::<Vec<_>>().join(" · ");
    let title = plan.iter().map(|(r, z, l)| format!("{r}: {} [{}]", z.join(" "), l.join(", "))).collect::<Vec<_>>().join("\n");
    let action = action.clone();
    html! {<span class="tp-refresh"><button id="refresh-role-maps" disabled={disabled} title={title} onclick={Callback::from(move |_| action.emit(Action::post("/api/master/knowledge/refresh", json!({}))))}>{crate::exfmt!("Aggiorna mappe dei ruoli ({})","Update role maps ({})",plan.len())}</button>{" "}<small class="muted" data-refresh-roles="">{list_text}</small></span>}
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
    let step_min = step.parse::<u32>().unwrap_or(5);
    let primary = next_inject_advance(a);
    let to_next = primary.clone().map(|(t, b, _)| (t, b));
    let to_step = Some(step_target(now, step_min));
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
    let due_inj = attention_count(items);
    let due_targets: Vec<(String, String)> = items
        .iter()
        .filter(|i| i["due"] == true)
        .map(|i| (text(&i["kind"]).to_string(), text(&i["id"]).to_string()))
        .collect();
    let overdue_targets: Vec<(String, String)> = overdue.iter().map(|i| (text(&i["kind"]).to_string(), text(&i["id"]).to_string())).collect();
    let pct = |t: i64| track_pct(t, start, end);
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
            if due_inj>0{<button class="tp-attention" id="tp-attention" title={crate::i18n::ex("Mostra gli inject nella lista: invia o non inviare")} onclick={{let t=due_targets.clone();Callback::from(move |_|reveal_rows(&t))}}><b>{crate::i18n::ex("Da gestire ora")}</b>{format!(": {} inject",due_inj)}</button>}
            else{<span class="tp-attention none muted" id="tp-attention">{crate::i18n::ex("Nulla da gestire ora")}</span>}
            <crate::situation::SuggestionBadge generation={props.state["generation"].as_u64().unwrap_or(0)} onopen={{let v=props.onview.clone();Callback::from(move |_|v.emit("situazione".into()))}}/>
            {review_chip(&props.review,&props.onview)}
            {if resolving{Html::default()}else{last_advance_summary(&props.state,&props.actions,&props.report,origin)}}
            {if resolving{Html::default()}else{refresh_maps(&props.state,&props.roles,props.busy||!orders,&props.action)}}
            <div class="grow"/>
            <button class="primary" id="advance-next" disabled={ctl_off||to_next.is_none()} title={primary.as_ref().map(|(t,_,closes)|if *closes{crate::exfmt!("Porta il tempo alle {} e chiude il turno {}: si apre il turno {}","Moves time to {} and closes turn {}: turn {} opens",hhmm(origin,*t),a["turn"],a["turn"].as_u64().unwrap_or(1)+1)}else{crate::exfmt!("Porta il tempo a {} e lascia il turno aperto: l’EXCON decide poi se inviare l’inject","Moves time to {} and keeps the turn open: you then decide whether to send the inject",clock_at(origin,*t))})} onclick={go(to_next)}>{advance_label(a)}</button>
            <span class="tp-step">{crate::i18n::ex("Avanza di")}<input aria-label={crate::i18n::ex("Minuti di avanzamento")} id="advance-step" type="number" min="1" max="1440" value={(*step).clone()} oninput={{let s=step.clone();Callback::from(move |e|s.set(crate::api::input(e)))}}/>{"min"}
                <button id="advance-step-go" disabled={ctl_off} title={crate::exfmt!("Porta il tempo a {} e lascia il turno aperto","Moves time to {} and keeps the turn open",clock_at(origin,now+step_min.max(1) as i64*60))} onclick={go(to_step)}>{"▶"}</button></span>
        </div>
    </section>}
}

/// One action under its inject: offset, title, command, status and, for a
/// pre-action still to come, the switch that decides whether it runs.
fn action_sub_row(inject: &Value, a: &Value, can_toggle: bool, action: &Callback<Action>) -> Html {
    let id = text(&a["id"]).to_string();
    let iid = text(&inject["id_inject"]).to_string();
    let status = text(&a["status"]).to_string();
    let enabled = a["enabled"] != false;
    let pre = a["pre"] == true;
    let open = !(inject["sent"] == true || inject["skipped"] == true) && (status.is_empty() || (status == "not_run" && !enabled));
    let toggle = (pre && open && can_toggle).then(|| {
        let onchange = {
            let (action, iid, id) = (action.clone(), iid.clone(), id.clone());
            Callback::from(move |_| action.emit(Action::post(format!("/api/master/msel/{iid}/actions/{id}/enabled"), json!({"enabled": !enabled}))))
        };
        html! {<label class="inline-check pi-run-toggle" title={crate::i18n::ex("Azione che precede l’inject: parte da sola all’istante previsto durante l’avanzamento. Togli la spunta per non eseguirla.")}>
            <input type="checkbox" data-action-enabled={id.clone()} checked={enabled} {onchange}/> {crate::i18n::ex("esegui")}</label>}
    });
    html! {<li class={classes!("pi-action",pre.then_some("pre"),matches!(status.as_str(),"done"|"failed").then_some("fired"),(status=="failed").then_some("bad"),(status=="not_run").then_some("off"))} data-action={id.clone()} data-action-of={iid.clone()} key={format!("{iid}:{id}")}>
        <span class="pi-off" title={if pre{crate::i18n::ex("Prima dell’ora prevista dell’inject")}else{crate::i18n::ex("Dopo l’invio dell’inject")}}>{offset_chip(a["offset_s"].as_i64().unwrap_or(0))}</span>
        <span class="pill kind action" title={crate::i18n::ex("Azione dell’inject: cambia il mondo")}>{crate::i18n::ex("Azione")}</span>
        <b class="planned-title">{text(&a["title"])}</b>
        <span class="pi-preview muted">{action_summary(a)}</span>
        <span class="pill rcpt">{action_status_text(inject,a)}</span>
        {toggle.unwrap_or_default()}
        if let Some(r)=a["result"].as_str().filter(|r|!r.is_empty()){<small class="pi-result muted" title={r.to_string()}>{r}</small>}</li>}
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
    /// Inject sends in flight (slots, see `send_slot`).
    pub pending: BTreeSet<String>,
    pub oninject: Callback<String>,
    /// Switch the console to another view (by id).
    #[prop_or_default]
    pub onview: Callback<String>,
    /// Advance to the given simulated second with this resolve body.
    pub onadvance: Callback<(Value, i64)>,
}

#[function_component(ExconTimeline)]
pub fn excon_timeline(props: &ExconTimelineProps) -> Html {
    let grouped = use_state(|| true);
    let collapsed = use_state(BTreeSet::<String>::new);
    let opened = use_state(|| Option::<String>::None);
    // Row shown in the detail column (`i:<id>`); by default the first waiting inject.
    let selected = use_state(|| Option::<String>::None);
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
    let runs = list(&props.master["inject_actions"]).to_vec();
    let origin = agenda["clock_origin_s"].as_i64().unwrap_or(0);
    let started = !matches!(text(&props.state["phase"]), "briefing" | "");
    let win_start = agenda["turn_start_s"].as_i64().unwrap_or(0);
    let win_end = agenda["planned_end_s"].as_i64().unwrap_or(win_start + 1800);
    let all_items = sorted_injects(&injects);
    let bands = turn_bands(&all_items, turn, win_start, win_end);
    let due_ids: BTreeSet<String> = list(&agenda["items"])
        .iter()
        .filter(|i| i["due"] == true && started)
        .map(|i| text(&i["id"]).to_string())
        .collect();
    let sent_count = injects.iter().filter(|i| i["sent"] == true).count();
    let open_inject = opened
        .as_ref()
        .and_then(|id| injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let item_key = |i: &Value| format!("i:{}", text(&i["id_inject"]));
    let current_key = (*selected).clone().or_else(|| {
        all_items.iter().find(|i| due_ids.contains(text(&i["id_inject"]))).map(item_key)
    });
    let toggle_expanded = |key: String| {
        let selected = selected.clone();
        let current = current_key.clone();
        Callback::from(move |_| selected.set(if current.as_deref() == Some(key.as_str()) { Some(String::new()) } else { Some(key.clone()) }))
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
    let skip_cb = |id: &str, restore: bool| {
        let (action, id) = (props.action.clone(), id.to_string());
        Callback::from(move |_| action.emit(Action::post(format!("/api/master/msel/{id}/{}", if restore { "restore" } else { "skip" }), json!({}))))
    };
    let row = |i: &Value| {
        let at = item_time(i);
        let time = html! {<span class="pi-time" title={clock_at(origin,at)}>{compact_clock(origin,at)}</span>};
        let id = text(&i["id_inject"]).to_string();
        let sent = i["sent"] == true;
        let skipped = i["skipped"] == true;
        let complete = sent && list(&i["not_yet_sent_to"]).is_empty();
        let due = due_ids.contains(&id);
        let key = format!("i:{id}");
        let open = current_key.as_deref() == Some(key.as_str());
        let onclick = {
            let opened = opened.clone();
            let id = id.clone();
            Callback::from(move |_| opened.set(Some(id.clone())))
        };
        let (pre, post) = split_actions(i);
        let can_toggle = started && !props.busy;
        let subs = |l: &[Value]| if l.is_empty() { Html::default() } else {
            html! {<ul class="pi-actions">{for l.iter().map(|a|action_sub_row(i,a,can_toggle,&props.action))}</ul>}
        };
        let send_label = if due { crate::i18n::ex("Invia") } else if complete { crate::i18n::ex("Altri destinatari…") } else { crate::i18n::ex("Invia…") };
        let sending = props.pending.contains(&send_slot(&id));
        html! {<li class={classes!("planned-inject","pi",open.then_some("is-selected"),sent.then_some("is-sent"),complete.then_some("is-complete"),sent.then_some("fired"),due.then_some("is-due"),(due&&at<win_start).then_some("is-late"),skipped.then_some("is-skipped"))} data-inject={id.clone()} key={key.clone()}>
            {subs(&pre)}
            <div class="pi-line">{time}
                <span class="pill kind inject" title={crate::i18n::ex("Inject pianificato")}>{crate::i18n::ex("Inject")}</span>
                <button class="pi-toggle" aria-expanded={open.to_string()} onclick={toggle_expanded(key)}><span class="pi-chev">{if open{"▾"}else{"▸"}}</span><b class="planned-title">{text(&i["title"])}</b><span class="pi-preview muted" aria-hidden="true">{text(&i["text"])}</span></button>
                {late_pill(due,at,crate::i18n::ex("raggiunto: da inviare"))}
                if sent{<span class="pill rcpt sent">{if complete{crate::i18n::ex("Inviato")}else{crate::i18n::ex("Inviato in parte")}}</span>}
                if skipped{<span class="pill rcpt skipped">{crate::i18n::ex("Non inviato")}</span>}
                <small class="pi-recips muted">{recipients_summary(i)}</small>
                if skipped{<button class="planned-open" data-restore="" onclick={skip_cb(&id,true)} disabled={props.busy||!started}>{crate::i18n::ex("Ripristina")}</button>}
                else{<>
                    <button class={classes!("planned-open",due.then_some("primary"))} data-send-open="" {onclick} disabled={props.busy||sending} aria-busy={sending.to_string()} aria-label={crate::exfmt!("Apri l’invio dell’inject {}","Open sending for inject {}",text(&i["title"]))}>{if sending{html!{<><span class="loader" aria-hidden="true"></span>{crate::i18n::ex("Invio in corso…")}</>}}else{html!{send_label}}}</button>
                    if due{<button class="planned-open" data-skip="" onclick={skip_cb(&id,false)} disabled={props.busy} title={crate::i18n::ex("Non invia questo inject: l’avanzamento prosegue oltre e le sue azioni anticipate vengono annullate. Si può ripristinare.")}>{crate::i18n::ex("Non inviare")}</button>}
                </>}</div>
            if open{<div class="pi-detail"><span class="planned-text">{text(&i["text"])}</span>
                if !text(&i["objective"]).is_empty(){<small class="planned-objective">{crate::exfmt!("Obiettivo: {}","Objective: {}",text(&i["objective"]))}</small>}
                {pills(i,&roles)}</div>}
            {subs(&post)}</li>}
    };
    let panel = (!agenda.is_null() && matches!(text(&props.state["phase"]), "orders" | "resolving")).then(|| {
        html! {<TurnPanel agenda={agenda.clone()} state={props.state.clone()} actions={runs.clone()} roles={roles.clone()} report={props.last_report.clone()} busy={props.busy} action={props.action.clone()} onadvance={props.onadvance.clone()} onview={props.onview.clone()} review={props.master["reply_review"]["counts"].clone()}/>}
    });
    // The selected inject in full, in the side column on wide screens (inline below it otherwise).
    let detail = current_key.as_ref().and_then(|k| all_items.iter().find(|i| item_key(i) == *k)).map(|i| {
        let at = item_time(i);
        let close = {
            let selected = selected.clone();
            Callback::from(move |_| selected.set(Some(String::new())))
        };
        let id = text(&i["id_inject"]).to_string();
        let complete = i["sent"] == true && list(&i["not_yet_sent_to"]).is_empty();
        let send = {
            let (opened, id) = (opened.clone(), id.clone());
            Callback::from(move |_| opened.set(Some(id.clone())))
        };
        let (pre, post) = split_actions(i);
        let acts = |l: &[Value]| html! {{for l.iter().map(|a|html!{<li><span class="pi-off">{offset_chip(a["offset_s"].as_i64().unwrap_or(0))}</span>{" "}<b>{text(&a["title"])}</b>{" · "}<span class="muted">{action_summary(a)}{" · "}{action_status_text(i,a)}</span></li>})}};
        let body = html! {<>
            <div class="tl-detail-head"><span class="pill kind inject">{crate::i18n::ex("Inject")}</span><b class="tl-detail-title">{text(&i["title"])}</b><div class="grow"/>
                <button class="tp-small" aria-label={crate::i18n::ex("Chiudi")} onclick={close}>{"✕"}</button></div>
            <p class="muted tl-detail-when">{clock_at(origin, at)}
                if !text(&i["sender"]).is_empty(){{" · "}{crate::exfmt!("da {}","from {}",text(&i["sender"]))}}
                if !text(&i["event"]).is_empty(){{" · "}{text(&i["event"])}}</p>
            <p class="planned-text">{text(&i["text"])}</p>
            if !text(&i["objective"]).is_empty(){<p class="planned-objective">{crate::exfmt!("Obiettivo: {}","Objective: {}",text(&i["objective"]))}</p>}
            {pills(i, &roles)}
            if !pre.is_empty()||!post.is_empty(){<><h4>{crate::i18n::ex("Azioni")}</h4><ul class="tl-detail-actions">{acts(&pre)}{acts(&post)}</ul></>}
            <div class="row"><div class="grow"/><button class={classes!((!complete).then_some("primary"))} disabled={props.busy} onclick={send}>{if complete{crate::i18n::ex("Altri destinatari…")}else{crate::i18n::ex("Invia…")}}</button></div>
        </>};
        html! {<section class="card tl-detail" aria-label={crate::i18n::ex("Dettaglio dell’elemento selezionato")}>{body}</section>}
    });
    let side_hint = html! {<section class="card tl-detail empty"><p class="muted">{crate::i18n::ex("Seleziona un inject nella lista per vederne qui il testo completo, le azioni, i destinatari e l’invio.")}</p></section>};
    html! {<div class="timeline-layout"><aside class="tl-side">{panel.unwrap_or_default()}{detail.unwrap_or(side_hint)}</aside><section class="card" id="excon-timeline"><div class="row tl-head"><h2>{crate::i18n::ex("Timeline EXCON")}</h2>
        <small class="muted">{crate::exfmt!("{} inject · {} inviati","{} injects · {} sent",injects.len(),sent_count)}</small><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="group-by-event" checked={*grouped} onchange={{let g=grouped.clone();Callback::from(move |e:Event|{use yew::TargetCast;g.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("raggruppa per evento")}</label>
        <button class="primary" data-inject-all="" disabled={props.busy} onclick={{let cb=props.oninject.clone();Callback::from(move |_|cb.emit("all".into()))}}>{crate::i18n::ex("Inject personalizzato")}</button></div>
        if injects.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject pianificato")}</p>}
        {for bands.iter().map(|band|{
            let waiting=band.items.iter().filter(|i|due_ids.contains(text(&i["id_inject"]))).count();
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
    fn injects_sort_chronologically() {
        let i = vec![json!({"id_inject":"i1","scenario_time_s":600}), json!({"id_inject":"i2","scenario_time_s":0})];
        let m = sorted_injects(&i);
        assert_eq!(m[0]["id_inject"], "i2");
    }
    #[test]
    fn track_positions_clamp_to_the_window() {
        assert_eq!(track_pct(450, 0, 1800), 25.0);
        assert_eq!(track_pct(-60, 0, 1800), 0.0);
        assert_eq!(track_pct(5000, 0, 1800), 100.0);
    }
    #[test]
    fn primary_advance_follows_the_agenda() {
        let agenda = json!({"turn": 1, "advance": {"to_s": 600, "clock": "Day 1 13:10", "closes_turn": false, "inject": "I-01"}});
        let (to, body, closes) = next_inject_advance(&agenda).unwrap();
        assert_eq!((to, closes), (600, false));
        assert_eq!(body["until"], "next_inject");
        assert_eq!(advance_label(&agenda), "▶▶ Avanza al prossimo inject (13:10)");
        let agenda = json!({"turn": 2, "advance": {"to_s": 1800, "clock": "Day 1 13:30", "closes_turn": true, "inject": null}});
        assert_eq!(advance_label(&agenda), "▶▶ Avanza e chiudi turno 2 (13:30)");
        assert!(next_inject_advance(&json!({})).is_none());
        let (to, body) = step_target(300, 5);
        assert_eq!((to, &body["minutes"], &body["substep"]), (600, &json!(5), &json!(true)));
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
        let items = vec![json!({"kind": "inject", "due": true}), json!({"kind": "inject", "due": false}), json!({"kind": "inject", "due": true})];
        assert_eq!(attention_count(&items), 2);
    }
    #[test]
    fn unsent_injects_are_reached_or_crossed() {
        let i = |id: &str, at: i64, left: &[&str]| json!({"id_inject": id, "scenario_time_s": at, "not_yet_sent_to": left});
        let injects = vec![i("old", 60, &["a"]), i("now", 300, &["a"]), i("soon", 900, &["a"]), i("late", 1800, &["a"]), i("done", 0, &[])];
        let got = unsent_injects(&injects, 300, 1800, 120);
        let ids: Vec<(&str, Reach)> = got.iter().map(|(v, r)| (v["id_inject"].as_str().unwrap(), *r)).collect();
        assert_eq!(ids, [("old", Reach::Overdue), ("now", Reach::Reached), ("soon", Reach::Crossed)]);
        // Stopping at the next inject only raises what is already reached.
        assert_eq!(unsent_injects(&injects, 300, 900, 120).len(), 2);
        // A skipped inject no longer needs attention.
        let skipped = vec![json!({"id_inject": "s", "scenario_time_s": 300, "not_yet_sent_to": ["a"], "skipped": true})];
        assert!(unsent_injects(&skipped, 300, 1800, 120).is_empty());
    }
    #[test]
    fn advance_wording_states_target_and_effect() {
        let body = json!({"until": "next_inject"});
        assert_eq!(advance_kind(&body, false), AdvanceKind::Inject);
        assert_eq!(advance_kind(&body, true), AdvanceKind::Close, "the target is the turn end");
        assert_eq!(advance_kind(&step_target(0, 5).1, false), AdvanceKind::Step);
        assert_eq!(advance_kind(&json!({"until": "turn_end"}), false), AdvanceKind::Close);
        let (title, button) = advance_wording(AdvanceKind::Inject, 1, "Day 1 13:05", "13:05");
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
        let a = |t: &str, at: Option<i64>, st: &str| json!({"title": t, "executed_s": at, "status": st, "result": "108 celle"});
        let list = vec![a("Innesco", Some(0), "done"), a("Vento", Some(300), "failed"), a("Altro", None, "approved")];
        assert_eq!(executed_actions(&list, 0, 300), [("Innesco".to_string(), true, "108 celle".to_string())]);
        assert_eq!(executed_actions(&list, 300, 900).len(), 1);
    }
    fn act(id: &str, off: i64, status: Value) -> Value {
        json!({"id": id, "title": id, "offset_s": off, "pre": off < 0, "enabled": true, "status": status,
            "instant_s": if off < 0 { json!(600 + off) } else { Value::Null }, "summary": "x"})
    }
    #[test]
    fn actions_split_around_their_inject() {
        let i = json!({"actions": [act("late", 120, Value::Null), act("pre2", -600, Value::Null), act("now", 0, json!("done")), act("pre1", -300, Value::Null)]});
        let (pre, post) = split_actions(&i);
        assert_eq!(pre.iter().map(|a| text(&a["id"])).collect::<Vec<_>>(), ["pre2", "pre1"], "most anticipated first");
        assert_eq!(post.iter().map(|a| text(&a["id"])).collect::<Vec<_>>(), ["now", "late"]);
        assert_eq!(offset_chip(-300), "−5 min");
        assert_eq!(offset_chip(120), "+2 min");
        assert_eq!(offset_chip(90), "+1.5 min");
        assert_eq!(offset_chip(0), "all’invio");
    }
    #[test]
    fn advance_review_lists_the_pre_actions_it_crosses() {
        let mut off = act("off", -120, json!("not_run"));
        off["enabled"] = json!(false);
        let inj = |id: &str, acts: Vec<Value>, extra: Value| {
            let mut v = json!({"id_inject": id, "sent_to": [], "skipped": false, "actions": acts});
            for (k, x) in extra.as_object().unwrap() { v[k] = x.clone(); }
            v
        };
        let injects = vec![
            inj("a", vec![act("p1", -300, Value::Null), act("post", 60, Value::Null), act("ran", -400, json!("done"))], json!({})),
            inj("b", vec![off.clone()], json!({})),
            inj("sent", vec![act("p3", -300, Value::Null)], json!({"sent_to": ["x"]})),
            inj("skip", vec![act("p4", -300, Value::Null)], json!({"skipped": true})),
        ];
        // Instants: p1 at 300, off at 480; the advance to 400 crosses only p1.
        let got: Vec<String> = crossed_pre_actions(&injects, 0, 400).iter().map(|(_, a)| text(&a["id"]).to_string()).collect();
        assert_eq!(got, ["p1"]);
        // Further on it also shows the one EXCON switched off, so it can be switched back on.
        let got: Vec<String> = crossed_pre_actions(&injects, 0, 600).iter().map(|(_, a)| text(&a["id"]).to_string()).collect();
        assert_eq!(got, ["p1", "off"]);
    }
    #[test]
    fn send_dialog_offers_the_undecided_actions() {
        let mut off = act("off", -120, json!("not_run"));
        off["enabled"] = json!(false);
        let i = json!({"sent_to": [], "actions": [act("a", 0, Value::Null), act("done", -60, json!("done")), off]});
        let ids: Vec<&str> = sendable_actions(&i).iter().map(|a| text(&a["id"])).collect::<Vec<_>>().into_iter().map(|s| if s == "a" { "a" } else { "off" }).collect();
        assert_eq!(ids, ["a", "off"]);
        assert_eq!(default_actions(&i).into_iter().collect::<Vec<_>>(), ["a"], "the switched-off one is not ticked");
        assert_eq!(send_body_with(&BTreeSet::from(["m".to_string()]), &default_actions(&i)), json!({"recipients": ["m"], "actions": ["a"]}));
        // Already sent: the actions were decided with the first send.
        let sent = json!({"sent_to": ["m"], "actions": [act("a", 0, Value::Null)]});
        assert!(sendable_actions(&sent).is_empty());
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
