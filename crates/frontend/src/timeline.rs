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

#[function_component(ExconTimeline)]
pub fn excon_timeline(props: &ExconTimelineProps) -> Html {
    let ratio = use_state(|| "60".to_string());
    let grouped = use_state(|| true);
    let collapsed = use_state(BTreeSet::<String>::new);
    let opened = use_state(|| Option::<String>::None);
    let running = props.state["clock_control"]["running"] == true;
    let roles = list(&props.stat["roles"])
        .iter()
        .filter(|r| text(&r["id"]) != "master")
        .cloned()
        .collect::<Vec<_>>();
    let injects = list(&props.master["planned_injects"]).to_vec();
    let groups = group_by_event(&injects, *grouped);
    let sent_count = injects.iter().filter(|i| i["sent"] == true).count();
    let open_inject = opened
        .as_ref()
        .and_then(|id| injects.iter().find(|i| text(&i["id_inject"]) == id).cloned());
    let row = |i: &Value| {
        let id = text(&i["id_inject"]).to_string();
        let sent = i["sent"] == true;
        let complete = sent && list(&i["not_yet_sent_to"]).is_empty();
        let onclick = {
            let opened = opened.clone();
            let id = id.clone();
            Callback::from(move |_| opened.set(Some(id.clone())))
        };
        html! {<li class={classes!("planned-inject",sent.then_some("is-sent"),complete.then_some("is-complete"))} data-inject={id.clone()} key={id.clone()}>
            <button class="planned-open" {onclick} disabled={props.busy} aria-label={crate::exfmt!("Apri l’invio dell’inject {}","Open sending for inject {}",text(&i["title"]))}>
                <span class="planned-times"><b>{format_offset(i["real_time_offset"].as_i64().unwrap_or(0))}</b><small>{text(&i["scenario_time"])}</small>
                    <small>{crate::exfmt!("turno {}","turn {}",i["turn"])}</small></span>
                <span class="planned-body"><b class="planned-title">{text(&i["title"])}</b>
                    if sent{<span class="pill rcpt sent">{if complete{crate::i18n::ex("Inviato")}else{crate::i18n::ex("Inviato in parte")}}</span>}
                    <span class="planned-text">{text(&i["text"])}</span>
                    if !text(&i["objective"]).is_empty(){<small class="planned-objective">{crate::exfmt!("Obiettivo: {}","Objective: {}",text(&i["objective"]))}</small>}
                    {pills(i,&roles)}</span>
            </button></li>}
    };
    html! {<section class="card" id="excon-timeline"><div class="row"><h2>{crate::i18n::ex("Timeline EXCON")}</h2><div class="grow"/>
        <label class="inline-check"><input type="checkbox" id="group-by-event" checked={*grouped} onchange={{let g=grouped.clone();Callback::from(move |e:Event|{use yew::TargetCast;g.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().checked())})}}/> {crate::i18n::ex("raggruppa per evento")}</label>
        <button class="primary" data-inject-all="" disabled={props.busy} onclick={{let cb=props.oninject.clone();Callback::from(move |_|cb.emit("all".into()))}}>{crate::i18n::ex("Inject a tutti")}</button></div>
        <div class="row timeline-clock"><span>{format!("{} · clock {}",text(&props.state["world"]["stats"]["clock"]),if running{crate::i18n::ex("in esecuzione")}else{crate::i18n::ex("fermo")})}</span><label>{crate::i18n::ex("Velocità clock × ")}<input aria-label={crate::i18n::ex("Velocità clock")} type="number" min="0.1" step="1" value={(*ratio).clone()} oninput={{let ratio=ratio.clone();Callback::from(move |e|ratio.set(crate::api::input(e)))}}/></label><button id="timeline-clock-toggle" disabled={props.busy||props.state["phase"]=="resolving"||props.state["phase"]=="debrief"||ratio.parse::<f64>().map_or(true,|r|!r.is_finite()||r<=0.)} onclick={{let cb=props.action.clone();let ratio=ratio.parse::<f64>().unwrap_or(60.);Callback::from(move |_|cb.emit(Action::post("/api/master/clock",json!({"running":!running,"ratio":ratio,"reason":crate::i18n::ex("Controllo clock dalla timeline EXCON")}))))}}>{if running{crate::i18n::ex("Ferma clock")}else{crate::i18n::ex("Avvia clock")}}</button></div>
        if injects.is_empty(){<p class="muted">{crate::i18n::ex("Nessun inject pianificato")}</p>}
        {for groups.iter().map(|(event,items)|{
            let is_closed=!event.is_empty()&&collapsed.contains(event);
            let list_html=html!{<ol class="planned-list">{for items.iter().map(|i|row(i))}</ol>};
            if event.is_empty(){list_html}else{
                let toggle={let collapsed=collapsed.clone();let event=event.clone();Callback::from(move |_|{let mut n=(*collapsed).clone();if !n.remove(&event){n.insert(event.clone());}collapsed.set(n);})};
                html!{<section class="event-group" data-event={event.clone()}><button class="event-head" aria-expanded={(!is_closed).to_string()} onclick={toggle}><span>{if is_closed{"▸"}else{"▾"}}</span><b>{event.clone()}</b><small>{items.len()}</small></button>if !is_closed{{list_html}}</section>}
            }
        })}
        <p class="muted">{crate::exfmt!("{} inject pianificati · {} già inviati · turno {} · {}","{} planned injects · {} already sent · turn {} · {}",injects.len(),sent_count,props.state["turn"],text(&props.state["phase"]))}</p>
        if let Some(inject)=open_inject{<SendDialog key={text(&inject["id_inject"]).to_string()} inject={inject.clone()} roles={roles.clone()} busy={props.busy} action={props.action.clone()} onclose={{let o=opened.clone();Callback::from(move |_|o.set(None))}}/>}
    </section>}
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
    fn grouping() {
        let v = vec![json!({"event":"E1"}), json!({"event":""}), json!({"event":"E1"}), json!({"event":""})];
        let g = group_by_event(&v, true);
        assert_eq!(g.iter().map(|(e, l)| (e.as_str(), l.len())).collect::<Vec<_>>(), [("E1", 2), ("", 2)]);
        assert_eq!(group_by_event(&v, false).len(), 1);
    }
}
