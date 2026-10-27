use crate::{
    api::{list, text},
    Action,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ExconTimelineProps {
    pub master: Value,
    pub state: Value,
    pub stat: Value,
    pub responses: BTreeMap<String, Value>,
    pub action: Callback<Action>,
    pub busy: bool,
    pub oninject: Callback<String>,
    /// Advance to the given simulated second with this resolve body.
    pub onadvance: Callback<(Value, i64)>,
}

/// `+HH:MM:SS` from a real-time offset in seconds.
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
        <p class="muted">{format!("{} · {}",format_offset(inject["real_time_offset"].as_i64().unwrap_or(0)),text(&inject["scenario_time"]))}</p>
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

#[derive(Properties, PartialEq)]
pub struct DueActionsProps {
    /// Resolve body of the advance being started; the choices are added to it.
    pub body: Value,
    pub due: Vec<Value>,
    pub busy: bool,
    pub action: Callback<Action>,
    pub onclose: Callback<()>,
}

/// Asked when a turn starts: what to do with each scripted action planned
/// inside the turn's window (default: apply it at its planned instant).
pub fn actions_body(base: &Value, choices: &BTreeMap<String, String>) -> Value {
    let mut body = base.clone();
    body["actions"] = json!(choices);
    body
}

#[function_component(DueActionsDialog)]
pub fn due_actions_dialog(props: &DueActionsProps) -> Html {
    let choices = {
        let due = props.due.clone();
        use_state(move || {
            due.iter()
                .map(|a| (text(&a["id"]).to_string(), "run".to_string()))
                .collect::<BTreeMap<_, _>>()
        })
    };
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
    html! {<div class="modal-backdrop"><section class="inject-modal send-dialog" role="dialog" aria-modal="true" aria-labelledby="due-dialog-title">
        <div class="row"><h2 id="due-dialog-title">{crate::i18n::ex("Azioni pianificate in questo turno")}</h2><div class="grow"/><button onclick={{let c=props.onclose.clone();Callback::from(move |_|c.emit(()))}}>{crate::i18n::ex("Annulla")}</button></div>
        <p class="muted">{crate::i18n::ex("Decidi cosa fare di ciascuna azione prima di far avanzare il tempo.")}</p>
        <ul class="planned-list">{for props.due.iter().map(|a|{
            let id=text(&a["id"]).to_string();
            let overdue=a["overdue"]==true;
            html!{<li class="planned-inject" data-action={id.clone()} key={id.clone()}>
                <span class="planned-times"><b>{text(&a["clock"])}</b>if overdue{<small>{crate::i18n::ex("istante già passato")}</small>}</span>
                <span class="planned-body"><b class="planned-title">{text(&a["title"])}</b>
                    <span class="planned-text">{text(&a["summary"])}</span>
                    if !text(&a["note"]).is_empty(){<small class="planned-objective">{text(&a["note"])}</small>}
                    <span class="row">{if overdue{option(&id,"run","Esegui subito")}else{option(&id,"run","Esegui all’istante previsto")}}{option(&id,"skip","Salta")}{option(&id,"defer","Rinvia")}</span></span></li>}
        })}</ul>
        <div class="row send-actions"><div class="grow"/><button class="primary" data-due-confirm="" disabled={props.busy} onclick={confirm}>{crate::i18n::ex("Avanza")}</button></div>
    </section></div>}
}


#[derive(Properties, PartialEq)]
struct TurnPanelProps {
    agenda: Value,
    state: Value,
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

/// Scroll the timeline row of an agenda item into view and flash it.
fn reveal_row(kind: &str, id: &str) {
    let attr = if kind == "inject" { "data-inject" } else { "data-action" };
    let Some(row) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(&format!("[{attr}=\"{id}\"]")).ok().flatten())
    else {
        return;
    };
    row.scroll_into_view();
    let _ = row.remove_attribute("data-flash");
    let _ = row.set_attribute("data-flash", "");
    gloo_timers::callback::Timeout::new(1600, move || {
        let _ = row.remove_attribute("data-flash");
    })
    .forget();
}

/// The single place where EXCON moves time: the open turn as a window on the
/// scenario timeline (where "now" is, the events inside it) and every advance,
/// from the next event to closing the turn. Markers only point to list rows.
#[function_component(TurnPanel)]
fn turn_panel(props: &TurnPanelProps) -> Html {
    let step = use_state(|| "5".to_string());
    let length = use_state(String::new);
    let a = &props.agenda;
    let w = &props.state["turn_window"];
    let start = a["turn_start_s"].as_i64().unwrap_or(0);
    let end = a["planned_end_s"].as_i64().unwrap_or(start + 1800);
    let resolving = !props.state["resolving"].is_null();
    let orders = text(&props.state["phase"]) == "orders";
    // The polled state is the freshest while a turn runs.
    let now = w["now_s"].as_i64().or(a["now_s"].as_i64()).unwrap_or(start);
    let target = props.state["resolving"]["to_s"].as_i64();
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
    let set_length = {
        let (action, length) = (props.action.clone(), length.clone());
        Callback::from(move |_| {
            if let Ok(m) = length.parse::<u32>() {
                action.emit(Action::post("/api/master/settings", json!({"turn_minutes": m})));
            }
        })
    };
    html! {<section class={classes!("card","turn-panel",resolving.then_some("running"))} id="turn-bar" data-now={now.to_string()}>
        <div class="tp-head">
            <h2>{crate::exfmt!("Turno {}","Turn {}",a["turn"])}</h2>
            <span class="muted">{format!("{} → {}",text(&a["start_clock"]),text(&a["end_clock"]))}</span>
            <div class="grow"/>
            <label class="tp-length">{crate::i18n::ex("Durata")}<input aria-label={crate::i18n::ex("Durata turno (min)")} id="turn-length" type="number" min="1" max="1440" placeholder={((end-start)/60).to_string()} value={(*length).clone()} oninput={{let l=length.clone();Callback::from(move |e|l.set(crate::api::input(e)))}}/>{"min"}</label>
            if length.parse::<u32>().is_ok(){<button id="turn-length-set" class="tp-small" disabled={ctl_off} onclick={set_length}>{crate::i18n::ex("Imposta")}</button>}
        </div>
        <div class="tb-track" role="img" aria-label={crate::exfmt!("Avanzamento del turno: ora {}","Turn progress: now {}",text(&a["now_clock"]))}>
            <div class="tb-fill" style={format!("width:{:.2}%",track_pct(now,start,end))}/>
            if let Some(t)=target.filter(|_|resolving){<div class="tb-run" style={format!("left:{:.2}%;width:{:.2}%",track_pct(now,start,end),(track_pct(t,start,end)-track_pct(now,start,end)).max(0.0))}/>}
            {for list(&a["substeps"]).iter().map(|s|html!{<i class="tb-tick" style={format!("left:{:.2}%",track_pct(s["to_s"].as_i64().unwrap_or(0),start,end))}/>})}
            {for items.iter().map(|i|{
                let at=i["at_s"].as_i64().unwrap_or(0);
                let kind=text(&i["kind"]).to_string();
                let state=if i["due"]==true{"due"}else if i["pending"]==true{"pending"}else{"done"};
                let title=format!("{} · {}",text(&i["clock"]),text(&i["title"]));
                let onclick={let id=text(&i["id"]).to_string();let kind=kind.clone();Callback::from(move |_|reveal_row(&kind,&id))};
                html!{<button class={classes!("tb-marker",kind.clone(),state,(at<start).then_some("overdue"))} key={format!("{}:{}",kind,text(&i["id"]))} style={format!("left:{:.2}%",track_pct(at,start,end))} title={title.clone()} aria-label={title} {onclick}/>}
            })}
            <div class="tb-now" style={format!("left:{:.2}%",track_pct(now,start,end))}><span>{text(&a["now_clock"])}</span></div>
        </div>
        <div class="tp-controls">
            <span class="tp-label">{crate::i18n::ex("Avanza il tempo")}</span>
            <button id="advance-next" disabled={ctl_off||to_next.is_none()} onclick={go(to_next)}>{match next{Some(_)=>crate::exfmt!("▶ al prossimo evento ({})","▶ to the next event ({})",text(&a["next_event_clock"])),None=>crate::i18n::ex("nessun altro evento nel turno").to_string()}}</button>
            <span class="tp-step"><button id="advance-step-go" disabled={ctl_off} onclick={go(to_step)}>{"▶ +"}</button><input aria-label={crate::i18n::ex("Minuti di avanzamento")} id="advance-step" type="number" min="1" max="1440" value={(*step).clone()} oninput={{let s=step.clone();Callback::from(move |e|s.set(crate::api::input(e)))}}/>{"min"}</span>
            <div class="grow"/>
            <button class="primary" id="resolve" disabled={ctl_off} onclick={{let (to,body)=close_target(now,end);let onadvance=props.onadvance.clone();Callback::from(move |_|onadvance.emit((body.clone(),to)))}}>{crate::exfmt!("Chiudi turno {} ▶▶","Close turn {} ▶▶",a["turn"])}</button>
        </div>
        <p class="tp-legend muted"><span class="lg inject"/>{crate::i18n::ex("inject")}<span class="lg action"/>{crate::i18n::ex("azione")}{" · "}{crate::i18n::ex("i primi due comandi lasciano il turno aperto; «Chiudi turno» arriva alla fine prevista e apre il successivo")}</p>
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

#[function_component(ExconTimeline)]
pub fn excon_timeline(props: &ExconTimelineProps) -> Html {
    let grouped = use_state(|| true);
    let collapsed = use_state(BTreeSet::<String>::new);
    let opened = use_state(|| Option::<String>::None);
    let roles = list(&props.stat["roles"])
        .iter()
        .filter(|r| text(&r["id"]) != "master")
        .cloned()
        .collect::<Vec<_>>();
    let injects = list(&props.master["planned_injects"]).to_vec();
    let actions = list(&props.master["timeline_actions"]).to_vec();
    let groups = group_by_event(&merge_items(&injects, &actions), *grouped);
    let due_ids: BTreeSet<String> = list(&props.master["agenda"]["items"])
        .iter()
        .filter(|i| i["due"] == true)
        .map(|i| text(&i["id"]).to_string())
        .collect();
    let sent_count = injects.iter().filter(|i| i["sent"] == true).count();
    let open_inject = opened
        .as_ref()
        .and_then(|id| injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let row = |i: &Value| {
        if i["item"] == "action" {
            let id = text(&i["id"]).to_string();
            let fired = matches!(text(&i["status"]), "done" | "failed");
            let due = due_ids.contains(&id);
            return html! {<li class={classes!("planned-inject","planned-action",fired.then_some("fired"),due.then_some("is-due"))} data-action={id.clone()} key={format!("a:{id}")}>
                <span class="planned-times"><b>{text(&i["clock"])}</b></span>
                <span class="planned-body"><span class="pill kind action" title={crate::i18n::ex("Azione pianificata")}>{crate::i18n::ex("Azione")}</span> <b class="planned-title">{text(&i["title"])}</b>
                    <span class="pill rcpt">{action_status_label(i)}</span>
                    if due{<span class="pill due-now">{crate::i18n::ex("raggiunta: decidi all’avanzamento")}</span>}
                    <span class="planned-text">{text(&i["summary"])}</span>
                    if let Some(r)=i["run"]["result"].as_str(){<small class="planned-objective">{r}</small>}</span></li>};
        }
        let id = text(&i["id_inject"]).to_string();
        let sent = i["sent"] == true;
        let complete = sent && list(&i["not_yet_sent_to"]).is_empty();
        let onclick = {
            let opened = opened.clone();
            let id = id.clone();
            Callback::from(move |_| opened.set(Some(id.clone())))
        };
        html! {<li class={classes!("planned-inject",sent.then_some("is-sent"),complete.then_some("is-complete"),sent.then_some("fired"),due_ids.contains(&id).then_some("is-due"))} data-inject={id.clone()} key={format!("i:{id}")}>
            <button class="planned-open" {onclick} disabled={props.busy} aria-label={crate::exfmt!("Apri l’invio dell’inject {}","Open sending for inject {}",text(&i["title"]))}>
                <span class="planned-times"><b>{format_offset(i["real_time_offset"].as_i64().unwrap_or(0))}</b><small>{text(&i["scenario_time"])}</small>
                    <small>{crate::exfmt!("turno {}","turn {}",i["turn"])}</small></span>
                <span class="planned-body"><span class="pill kind inject" title={crate::i18n::ex("Inject pianificato")}>{crate::i18n::ex("Inject")}</span> <b class="planned-title">{text(&i["title"])}</b>
                    if due_ids.contains(&id){<span class="pill due-now">{crate::i18n::ex("raggiunto: da inviare")}</span>}
                    if sent{<span class="pill rcpt sent">{if complete{crate::i18n::ex("Inviato")}else{crate::i18n::ex("Inviato in parte")}}</span>}
                    <span class="planned-text">{text(&i["text"])}</span>
                    if !text(&i["objective"]).is_empty(){<small class="planned-objective">{crate::exfmt!("Obiettivo: {}","Objective: {}",text(&i["objective"]))}</small>}
                    {pills(i,&roles)}</span>
            </button></li>}
    };
    let panel = (!props.master["agenda"].is_null() && matches!(text(&props.state["phase"]), "orders" | "resolving"))
        .then(|| html! {<TurnPanel agenda={props.master["agenda"].clone()} state={props.state.clone()} busy={props.busy} action={props.action.clone()} onadvance={props.onadvance.clone()}/>});
    html! {<div class="timeline-layout">{panel.unwrap_or_default()}<section class="card" id="excon-timeline"><div class="row tl-head"><h2>{crate::i18n::ex("Timeline EXCON")}</h2>
        <small class="muted">{crate::exfmt!("{} inject · {} inviati","{} injects · {} sent",injects.len(),sent_count)}</small><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="group-by-event" checked={*grouped} onchange={{let g=grouped.clone();Callback::from(move |e:Event|{use yew::TargetCast;g.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("raggruppa per evento")}</label>
        <button class="primary" data-inject-all="" disabled={props.busy} onclick={{let cb=props.oninject.clone();Callback::from(move |_|cb.emit("all".into()))}}>{crate::i18n::ex("Inject personalizzato")}</button></div>
        if injects.is_empty()&&actions.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject pianificato")}</p>}
        {for groups.iter().map(|(event,items)|{
            let is_closed=!event.is_empty()&&collapsed.contains(event);
            let list_html=html!{<ol class="planned-list">{for items.iter().map(|i|row(i))}</ol>};
            if event.is_empty(){list_html}else{
                let toggle={let collapsed=collapsed.clone();let event=event.clone();Callback::from(move |_|{let mut n=(*collapsed).clone();if !n.remove(&event){n.insert(event.clone());}collapsed.set(n);})};
                html!{<section class="event-group" data-event={event.clone()}><button class="event-head" aria-expanded={(!is_closed).to_string()} onclick={toggle}><span>{if is_closed{"▸"}else{"▾"}}</span><b>{event.clone()}</b><small>{items.len()}</small></button>if !is_closed{{list_html}}</section>}
            }
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
        assert_eq!(format_offset(3661), "+01:01:01");
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
}
