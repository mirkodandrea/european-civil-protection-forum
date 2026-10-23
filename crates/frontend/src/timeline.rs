use crate::{
    api::{self, list, text},
    widgets::Proposal,
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
    pub controls: Html,
    pub oninject: Callback<String>,
}

#[derive(Clone)]
struct Entry {
    turn: u32,
    time_s: i64,
    clock: String,
    actor: String,
    message: Option<Value>,
    proposal: Option<Value>,
}

fn entries(master: &Value, roles: &[Value]) -> Vec<Entry> {
    let mut out = Vec::new();
    for message in list(&master["messages"]) {
        let from = text(&message["from"]);
        let kind = text(&message["kind"]);
        // EXCON injects and player-to-EXCON messages are the operational conversation.
        if kind == "chat" && from != "master" && message["to"] != "master" {
            continue;
        }
        if !message["proposal"].is_null()
            && list(&master["proposals"])
                .iter()
                .any(|p| p["id"] == message["proposal"])
            && from != "master"
        {
            continue;
        }
        let actor = if kind == "inject" {
            text(&message["to"])
        } else if from == "master" {
            text(&message["to"])
        } else {
            from
        };
        if actor != "master" && !roles.iter().any(|r| text(&r["id"]) == actor) {
            continue;
        }
        out.push(Entry {
            turn: message["turn"].as_u64().unwrap_or(0) as u32,
            time_s: message["t_s"].as_i64().unwrap_or(0),
            clock: text(&message["clock"]).into(),
            actor: actor.into(),
            message: Some(message.clone()),
            proposal: None,
        });
    }
    for proposal in list(&master["proposals"]) {
        let source = &proposal["source"];
        let actor = if source["kind"] == "player" {
            text(&source["role"])
        } else {
            "master"
        };
        out.push(Entry {
            turn: proposal["turn"].as_u64().unwrap_or(0) as u32,
            time_s: proposal["t_s"].as_i64().unwrap_or(0),
            clock: text(&proposal["clock"]).into(),
            actor: actor.into(),
            message: None,
            proposal: Some(proposal.clone()),
        });
    }
    out.sort_by(|a, b| a.turn.cmp(&b.turn).then(a.time_s.cmp(&b.time_s)));
    out
}

#[function_component(ExconTimeline)]
pub fn excon_timeline(props: &ExconTimelineProps) -> Html {
    let ratio = use_state(|| "60".to_string());
    let running = props.state["clock_control"]["running"] == true;
    let roles = list(&props.stat["roles"]);
    let current = props.state["turn"].as_u64().unwrap_or(1) as u32;
    let events = entries(&props.master, roles);
    let turns = events
        .iter()
        .map(|e| e.turn)
        .chain(std::iter::once(current))
        .collect::<BTreeSet<_>>();
    let selected = use_state(|| current.to_string());
    {
        let selected = selected.clone();
        use_effect_with(current, move |turn| selected.set(turn.to_string()));
    }
    let turn = selected.parse::<u32>().unwrap_or(current);
    let columns = roles
        .iter()
        .map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string()))
        .collect::<Vec<_>>();
    html! {<section class="card" id="excon-timeline"><div class="row"><h2>{crate::i18n::ex("Timeline EXCON")}</h2><div class="grow"/>{props.controls.clone()}<button class="primary" data-inject-all="" disabled={props.busy} onclick={{let cb=props.oninject.clone();Callback::from(move |_|cb.emit("all".into()))}}>{crate::i18n::ex("Inject a tutti")}</button><label>{crate::i18n::ex("Turno ")}<select aria-label={crate::i18n::ex("Turno timeline")} value={turn.to_string()} onchange={{let selected=selected.clone();Callback::from(move |e|selected.set(api::select(e)))}}>{for turns.iter().rev().map(|t|html!{<option value={t.to_string()} selected={*t==turn}>{t}</option>})}</select></label></div>
        <div class="row timeline-clock"><span>{format!("{} · clock {}",text(&props.state["world"]["stats"]["clock"]),if running{crate::i18n::ex("in esecuzione")}else{crate::i18n::ex("fermo")})}</span><label>{crate::i18n::ex("Velocità clock × ")}<input aria-label={crate::i18n::ex("Velocità clock")} type="number" min="0.1" step="1" value={(*ratio).clone()} oninput={{let ratio=ratio.clone();Callback::from(move |e|ratio.set(api::input(e)))}}/></label><button id="timeline-clock-toggle" disabled={props.busy||props.state["phase"]=="resolving"||props.state["phase"]=="debrief"||ratio.parse::<f64>().map_or(true,|r|!r.is_finite()||r<=0.)} onclick={{let cb=props.action.clone();let ratio=ratio.parse::<f64>().unwrap_or(60.);Callback::from(move |_|cb.emit(Action::post("/api/master/clock",json!({"running":!running,"ratio":ratio,"reason":crate::i18n::ex("Controllo clock dalla timeline EXCON")}))))}}>{if running{crate::i18n::ex("Ferma clock")}else{crate::i18n::ex("Avvia clock")}}</button></div>
        <div class="row timeline-columns" style="align-items:flex-start; overflow-x:auto">{for columns.iter().map(|(id,name)|{
            let items=events.iter().filter(|e|e.turn==turn&&e.actor==*id).collect::<Vec<_>>();
            html!{<section class="card" style="min-width:240px; flex:1"><h3>{name}</h3>if id!="master"{<button data-inject-role={id.clone()} disabled={props.busy} onclick={{let cb=props.oninject.clone();let id=id.clone();Callback::from(move |_|cb.emit(id.clone()))}}>{crate::i18n::ex("Nuovo inject")}</button>}{if items.is_empty(){html!{<p class="muted">{crate::i18n::ex("Nessun evento in questo turno")}</p>}}else{html!{<ol>{for items.iter().map(|entry|{
                let clock=if entry.clock.is_empty(){format!("t={}s",entry.time_s)}else{entry.clock.clone()};
                if let Some(proposal)=&entry.proposal {
                    let key=format!("proposal:{}",proposal["id"]);
                    html!{<li><small>{clock}</small><Proposal key={proposal["id"].to_string()} proposal={proposal.clone()} generation={props.state["generation"].clone()} preview={props.responses.get(&key).cloned().unwrap_or_default()} busy={props.busy} action={props.action.clone()}/></li>}
                } else if let Some(message)=&entry.message {
                    let inject=message["kind"]=="inject";
                    let title=if inject && !text(&message["title"]).is_empty() {text(&message["title"])} else if inject {"Inject"} else {match text(&message["kind"]){"reply"=>crate::i18n::ex("Risposta player"),"feedback"=>crate::i18n::ex("Feedback EXCON"),"clarification"=>"Chiarimento",_=>"Messaggio"}};
                    let detail=if text(&message["text_en"]).is_empty(){text(&message["text"])}else{text(&message["text_en"])};
                    let recipient=text(&message["to"]); let recipients=roles.iter().find(|r|r["id"]==recipient).map(|r|text(&r["name"])).unwrap_or(recipient);
                    html!{<li><small>{format!("{clock} · {}",title)}</small>{if recipients.is_empty(){html!{}}else{html!{<p>{crate::exfmt!("A: {recipients}","To: {recipients}")}</p>}}}<p>{detail}</p></li>}
                } else { html!{} }
            })}</ol>}}}</section>}
        })}</div>
        <p class="muted">{crate::exfmt!("{} inject, messaggi e proposte · stato corrente: turno {} · {}","{} injects, messages and proposals · current state: turn {} · {}",events.iter().filter(|e|e.turn==turn).count(),current,text(&props.state["phase"]))}</p>
    </section>}
}
