//! Observer dashboard ("Osservatori"): read-only analysis of the exercise,
//! per turn and cumulated, overall or for one player, with the exercise's
//! pace. Data from `GET /api/observer` (`exercise::observer`). A tab of the
//! standalone app, `observer.html` on the server (one observer, no keys or
//! permissions: it reads as the EXCON).
use crate::api::{self, list, text};
use futures::future::AbortHandle;
use gloo_timers::callback::Interval;
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ObserverProps {
    pub token: String,
    /// `"it"` or `"en"`.
    #[prop_or_else(|| "it".into())]
    pub lang: String,
    /// Hidden standalone tabs neither poll nor tick.
    #[prop_or(true)]
    pub visible: bool,
}

/// Basis kinds as the player's reply composer offers them, in their fixed
/// colour order; "plan" also counts procedures (one choice for players).
const BASIS: [(&str, &str, &str); 4] = [
    ("plan", "Piano o procedura", "Plan or procedure"),
    ("law", "Legge o norma", "Law or regulation"),
    ("judgment", "Giudizio operativo", "Operational judgement"),
    ("none", "Senza fondamento", "No basis"),
];

/// Count of basis `k` in `exercise::observer::BasisCounts` (plan + procedure for "plan").
fn bn(b: &Value, k: &str) -> u64 {
    if k == "plan" {
        n(&b["plan"]) + n(&b["procedure"])
    } else {
        n(&b[k])
    }
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}
fn now_ms() -> f64 {
    js_sys::Date::now()
}
/// `h:mm:ss`, or `mm:ss` under an hour.
fn duration(ms: f64) -> String {
    let s = (ms.max(0.0) / 1000.0) as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
/// Short wall duration: "45 s", "12 min", "1 h 05".
fn short(ms: f64) -> String {
    let s = (ms.max(0.0) / 1000.0) as u64;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h {:02}", s / 3600, s / 60 % 60)
    }
}
fn clock(start: i64, s: i64) -> String {
    let t = (start + s).rem_euclid(86_400);
    format!("{:02}:{:02}", t / 3600, t / 60 % 60)
}
fn wall_clock(ms: f64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}
fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// Activity of the selection (`""` = everyone) inside a block (`turn_only`/`cumulative`).
fn activity<'a>(block: &'a Value, role: &str) -> &'a Value {
    if role.is_empty() {
        &block["overall"]
    } else {
        &block["roles"][role]
    }
}
fn total(a: &Value) -> u64 {
    n(&a["responses"]) + n(&a["interactions"]) + n(&a["answers"])
}

#[function_component(Observers)]
pub fn observers(props: &ObserverProps) -> Html {
    let data = use_state(|| Value::Null);
    let error = use_state(String::new);
    // Server wall clock minus the local one, so the timers agree with the server.
    let skew = use_state(|| 0.0f64);
    let tick = use_force_update();
    let role = use_state(String::new);
    let picked_turn = use_state(|| None::<u64>);
    let cumulative = use_state(|| false);
    {
        let (data, error, skew, tick) = (data.clone(), error.clone(), skew.clone(), tick.clone());
        use_effect_with((props.token.clone(), props.visible), move |(token, visible)| {
            let mut guards: Option<(AbortHandle, Interval, Interval)> = None;
            if *visible {
                let last = Rc::new(Cell::new(0.0f64));
                let fetch = {
                    let token = token.clone();
                    Callback::from(move |force: bool| {
                        // The live feed fires on every change (several per second while
                        // a turn runs): at most one read every 1.5 s.
                        if !force && now_ms() - last.get() < 1500.0 {
                            return;
                        }
                        last.set(now_ms());
                        let (token, data, error, skew) = (token.clone(), data.clone(), error.clone(), skew.clone());
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::request(&token, "/api/observer", None).await {
                                Ok(v) => {
                                    if let Some(w) = v["wall_ms"].as_f64() {
                                        skew.set(w - now_ms());
                                    }
                                    error.set(String::new());
                                    data.set(v);
                                }
                                Err(e) => error.set(e),
                            }
                        });
                    })
                };
                fetch.emit(true);
                let live = api::live(token.clone(), fetch.reform(|_| false));
                let poll = {
                    let fetch = fetch.clone();
                    Interval::new(5000, move || fetch.emit(true))
                };
                let clock = Interval::new(1000, move || tick.force_update());
                guards = Some((live, poll, clock));
            }
            move || {
                if let Some((live, _, _)) = guards {
                    live.abort();
                }
            }
        });
    }
    let en = props.lang == "en";
    let tr = move |it: &'static str, e: &'static str| if en { e } else { it };
    let d = &*data;
    if d.is_null() {
        return html! {<main class="page obs"><section class="card"><h2>{tr("Osservatori","Observers")}</h2><p class="muted">{if error.is_empty(){tr("Caricamento del cruscotto…","Loading the dashboard…").to_string()}else{(*error).clone()}}</p></section></main>};
    }
    let now = now_ms() + *skew;
    let roles = list(&d["roles"]);
    let role_name = |id: &str| {
        roles
            .iter()
            .find(|r| r["id"] == id)
            .map(|r| text(&r["name"]).to_string())
            .unwrap_or_else(|| id.to_string())
    };
    let turns = list(&d["turns"]);
    let started = d["phase"] != "briefing";
    // Default: the last closed turn (the dashboard updates at each turn's end).
    let last_closed = turns.iter().rev().find(|t| t["closed"] == true).map(|t| n(&t["turn"]));
    let latest = turns.last().map(|t| n(&t["turn"])).unwrap_or(1);
    let sel_turn = picked_turn
        .filter(|t| turns.iter().any(|x| n(&x["turn"]) == *t))
        .or(last_closed)
        .unwrap_or(latest);
    let turn_of = |t: u64| turns.iter().find(|x| n(&x["turn"]) == t);
    let sel = turn_of(sel_turn).cloned().unwrap_or(Value::Null);
    let prev = turn_of(sel_turn.saturating_sub(1)).cloned().unwrap_or(Value::Null);
    let block_key = if *cumulative { "cumulative" } else { "turn_only" };
    let r = (*role).clone();
    let a = activity(&sel[block_key], &r).clone();

    // ---------------------------------------------------------------- hero
    let timer = {
        let started_ms = d["started_wall_ms"].as_f64();
        let ended_ms = d["ended_wall_ms"].as_f64();
        let elapsed = started_ms.map(|s| ended_ms.unwrap_or(now) - s);
        let open_turn = turns.last().filter(|t| t["closed"] != true);
        let turn_elapsed = open_turn.and_then(|t| t["opened_wall_ms"].as_f64()).map(|o| now - o);
        html! {<section class="obs-hero-tile obs-timer" aria-label={tr("Tempo dall’avvio","Time since start")}>
            <div class="obs-eyebrow">{tr("Tempo dall’avvio","Time since start")}</div>
            <div class={classes!("obs-bigclock",ended_ms.is_none().then_some("live"))} id="observer-elapsed">{elapsed.map(duration).unwrap_or_else(||"--:--".into())}</div>
            <div class="obs-sub">{match (started_ms, ended_ms) {
                (None, _) => tr("L’esercitazione non è ancora iniziata","The exercise has not started yet").to_string(),
                (Some(s), None) => format!("{} {}", tr("Avviata alle","Started at"), wall_clock(s)),
                (Some(s), Some(e)) => format!("{} {} → {}", tr("Conclusa ·","Ended ·"), wall_clock(s), wall_clock(e)),
            }}</div>
            if let Some(t) = turn_elapsed {<div class="obs-chip-line"><span class="obs-dot live"/>{format!("{} {} · {}", tr("Turno","Turn"), d["turn"], short(t))}</div>}
            <div class="obs-turn-walls">{for turns.iter().filter(|t| t["closed"]==true).map(|t| {
                let len = t["closed_wall_ms"].as_f64().zip(t["opened_wall_ms"].as_f64()).map(|(c,o)| short(c-o)).unwrap_or_else(||"–".into());
                html!{<span title={tr("Durata reale del turno","Wall duration of the turn")}><b>{format!("T{}",t["turn"])}</b>{len}</span>}
            })}</div>
        </section>}
    };

    let scenario = {
        let s = &d["scenario"];
        let start_clock = s["start_clock_s"].as_i64().unwrap_or(0);
        let now_s = s["now_s"].as_i64().unwrap_or(0);
        let turn_s = s["turn_minutes"].as_i64().unwrap_or(30).max(1) * 60;
        let planned = n(&s["planned_turns"]).max(1);
        // Played turns keep their real windows; later ones are planned.
        let mut segs: Vec<(u64, i64, i64, &str)> = Vec::new();
        for t in turns {
            let k = n(&t["turn"]);
            let a = t["start_s"].as_i64().unwrap_or(0);
            let (b, state) = match t["end_s"].as_i64() {
                Some(e) if t["closed"] == true => (e, "done"),
                _ => (s["turn_end_s"].as_i64().unwrap_or(a + turn_s).max(now_s), "open"),
            };
            segs.push((k, a, b.max(a + 1), if started { state } else { "planned" }));
        }
        let mut end = segs.last().map(|x| x.2).unwrap_or(0);
        for k in (segs.len() as u64 + 1)..=planned {
            segs.push((k, end, end + turn_s, "planned"));
            end += turn_s;
        }
        let span = end.max(s["planned_end_s"].as_i64().unwrap_or(0)).max(1) as f64;
        let x = |t: i64| (t as f64 * 100.0 / span).clamp(0.0, 100.0);
        let in_turn = now_s - s["turn_start_s"].as_i64().unwrap_or(0);
        let turn_len = s["turn_end_s"].as_i64().unwrap_or(0) - s["turn_start_s"].as_i64().unwrap_or(0);
        let markers = list(&d["injects"]).iter().filter(|i| i["msel"] == true && i["planned_s"].is_i64()).map(|i| {
            let at = i["planned_s"].as_i64().unwrap_or(0);
            let (cls, state) = if i["sent_turn"].is_u64() {
                ("sent", tr("inviato","sent"))
            } else if i["skipped"] == true {
                ("skipped", tr("saltato","skipped"))
            } else if started && at <= now_s {
                ("late", tr("in ritardo","overdue"))
            } else {
                ("future", tr("da inviare","to send"))
            };
            html!{<span class={classes!("obs-mark",cls)} style={format!("left:{:.2}%",x(at))} data-tip={format!("{} · {} · {}",text(&i["title"]),clock(start_clock,at),state)}/>}
        });
        html! {<section class="obs-hero-tile obs-scenario" aria-label={tr("Tempo dello scenario","Scenario time")}>
            <div class="obs-eyebrow">{tr("Tempo dello scenario","Scenario time")}</div>
            <div class="obs-scenario-head">
                <div class="obs-bigclock" id="observer-scenario-clock">{clock(start_clock, now_s)}</div>
                <div class="obs-sub">{if started {format!("{} {} {} {} · {} / {} min {}", tr("Turno","Turn"), d["turn"], tr("di","of"), planned, in_turn.max(0)/60, turn_len.max(0)/60, tr("nel turno","into the turn"))} else {tr("In briefing","Briefing").to_string()}}</div>
                <div class="grow"/>
                <div class="obs-sub right">{format!("{} → {}", clock(start_clock,0), clock(start_clock,end))}</div>
            </div>
            <div class="obs-track" role="img" aria-label={format!("{} {} {} {}", tr("Turno","Turn"), d["turn"], tr("di","of"), planned)}>
                {for segs.iter().map(|(k,a,b,state)| {
                    let fill = if *state=="open" { ((now_s-a) as f64*100.0/((b-a) as f64)).clamp(0.0,100.0) } else if *state=="done" {100.0} else {0.0};
                    html!{<div class={classes!("obs-seg",*state)} style={format!("left:{:.2}%;width:calc({:.2}% - 3px)",x(*a),x(*b)-x(*a))} data-tip={format!("{} {} · {} → {}",tr("Turno","Turn"),k,clock(start_clock,*a),clock(start_clock,*b))}>
                        <div class="obs-seg-fill" style={format!("width:{fill:.1}%")}/>
                        <span class="obs-seg-label">{format!("T{k}")}</span>
                    </div>}
                })}
                if started {<div class="obs-now" style={format!("left:{:.2}%",x(now_s))}><span>{clock(start_clock,now_s)}</span></div>}
            </div>
            <div class="obs-marks">{for markers}</div>
            <div class="obs-legend small">
                <span><i class="obs-mark sent"/>{tr("Inject inviato","Inject sent")}</span>
                <span><i class="obs-mark late"/>{tr("Previsto, non inviato","Due, not sent")}</span>
                <span><i class="obs-mark future"/>{tr("Da inviare","To send")}</span>
                <span><i class="obs-mark skipped"/>{tr("Saltato","Skipped")}</span>
            </div>
        </section>}
    };

    let pace = {
        let (sent, all, due, skipped) = (n(&d["injects_sent"]), n(&d["injects_total"]), n(&d["injects_due"]), n(&d["injects_skipped"]));
        let gap = sent as i64 - due as i64;
        let (cls, icon, label) = if !started {
            ("neutral", "•", tr("In attesa dell’avvio","Waiting for the start").to_string())
        } else if gap >= 0 {
            ("good", "✓", if gap == 0 { tr("In linea con l’MSEL","On schedule").to_string() } else { format!("{} {gap}", tr("In anticipo di","Ahead by")) })
        } else {
            ("warn", "!", format!("{} {}", tr("Indietro di","Behind by"), -gap))
        };
        html! {<section class="obs-hero-tile obs-pace" aria-label={tr("Ritmo degli inject","Inject pace")}>
            <div class="obs-eyebrow">{tr("Ritmo degli inject","Inject pace")}</div>
            <div class="obs-bigclock" id="observer-injects"><span>{sent}</span><small>{format!(" / {all}")}</small></div>
            <div class="obs-sub">{format!("{} · {:.0}% {}", tr("inviati sul totale MSEL","sent of the MSEL"), pct(sent,all), tr("dell’esercitazione","of the exercise"))}</div>
            <div class="obs-meter" role="meter" aria-valuemin="0" aria-valuemax={all.to_string()} aria-valuenow={sent.to_string()}>
                <div class="obs-meter-fill" style={format!("width:{:.1}%",pct(sent,all))}/>
                if started && all>0 {<div class="obs-meter-due" style={format!("left:{:.1}%",pct(due,all))} data-tip={format!("{}: {due}",tr("Previsti a quest’ora","Due by now"))}/>}
            </div>
            <div class={classes!("obs-status",cls)}><b aria-hidden="true">{icon}</b>{label}</div>
            <div class="obs-sub">{format!("{}: {due}", tr("Previsti a quest’ora","Due by now"))}if skipped>0{{format!(" · {}: {skipped}", tr("saltati","skipped"))}}</div>
        </section>}
    };

    // ---------------------------------------------------------------- navigation
    let role_tabs = {
        let mut all = vec![(String::new(), tr("Complessivo","Overall").to_string())];
        all.extend(roles.iter().map(|r| (text(&r["id"]).to_string(), text(&r["name"]).to_string())));
        html! {<div class="obs-seg-control" role="tablist" aria-label={tr("Giocatore","Player")}>{for all.into_iter().map(|(id,name)| {
            let active = *role == id;
            let role = role.clone();
            let cum = turns.last().map(|t| total(activity(&t["cumulative"], &id))).unwrap_or(0);
            html!{<button role="tab" aria-selected={active.to_string()} data-observer-role={id.clone()} class={classes!(active.then_some("active"))} onclick={Callback::from(move |_| role.set(id.clone()))}>{name}<span class="obs-count">{cum}</span></button>}
        })}</div>}
    };
    let turn_tabs = html! {<div class="obs-turns" role="tablist" aria-label={tr("Turno","Turn")}>{for turns.iter().map(|t| {
        let k = n(&t["turn"]);
        let picked = picked_turn.clone();
        let open = t["closed"] != true && started;
        html!{<button role="tab" aria-selected={(k==sel_turn).to_string()} data-observer-turn={k.to_string()} class={classes!((k==sel_turn).then_some("active"),open.then_some("open"))} onclick={Callback::from(move |_| picked.set(Some(k)))}>{format!("T{k}")}if open{<small>{tr(" in corso"," live")}</small>}</button>}
    })}</div>};
    let mode = {
        let set = |v: bool| { let c = cumulative.clone(); Callback::from(move |_| c.set(v)) };
        html! {<div class="obs-mode" role="group" aria-label={tr("Valori","Values")}>
            <button class={classes!((!*cumulative).then_some("active"))} data-observer-mode="turn" onclick={set(false)}>{tr("Nel turno","This turn")}</button>
            <button class={classes!((*cumulative).then_some("active"))} data-observer-mode="cumulative" onclick={set(true)}>{tr("Cumulato","Cumulative")}</button>
        </div>}
    };
    let provisional = sel["closed"] != true && started;

    // ---------------------------------------------------------------- KPI tiles
    let kpi = |key: &str, label: &str, note: Option<String>| {
        let v = n(&a[key]);
        let delta = if *cumulative {
            Some((n(&activity(&sel["turn_only"], &r)[key]) as i64, tr("in questo turno","this turn")))
        } else if !prev.is_null() {
            Some((v as i64 - n(&activity(&prev["turn_only"], &r)[key]) as i64, tr("rispetto al turno prima","vs previous turn")))
        } else {
            None
        };
        html! {<div class="obs-kpi" data-kpi={key.to_string()}>
            <div class="obs-kpi-label">{label}</div>
            <div class="obs-kpi-value">{v}</div>
            if let Some((d, what)) = delta {<div class={classes!("obs-delta",(d>0).then_some("up"),(d<0).then_some("down"))}>{format!("{}{d} {what}", if d>0 {"+"} else {""})}</div>}
            if let Some(note)=note {<div class="obs-kpi-note">{note}</div>}
        </div>}
    };
    let injects_kpi = if r.is_empty() {
        let b = &sel[block_key];
        let sent = n(&b["injects_sent"]);
        let custom = n(&b["custom_injects"]);
        html! {<div class="obs-kpi" data-kpi="injects"><div class="obs-kpi-label">{tr("Inject inviati","Injects sent")}</div><div class="obs-kpi-value">{sent}</div>
            <div class="obs-kpi-note">{if custom>0 {format!("+ {custom} {}", tr("personalizzati","custom"))} else {format!("MSEL · {} {}", d["injects_total"], tr("in totale","in total"))}}</div></div>}
    } else {
        kpi("received", tr("Inject ricevuti","Injects received"), None)
    };
    let kpis = html! {<div class="obs-kpis">
        {injects_kpi}
        {kpi("responses", tr("Risposte agli inject","Responses to injects"), None)}
        {kpi("interactions", tr("Interazioni tra player","Player interactions"), Some(format!("{} {}", n(&a["conversations"]), tr("conversazioni avviate","conversations started"))))}
        {kpi("answers", tr("Risposte a domande EXCON","Answers to EXCON questions"), None)}
        {kpi("suggested", tr("Azioni suggerite dall’AI","AI-suggested actions"), Some(format!("{} {} · {:.0}%", n(&a["executed"]), tr("eseguite dall’EXCON","executed by EXCON"), pct(n(&a["executed"]), n(&a["suggested"])))))}
    </div>};

    // ---------------------------------------------------------------- basis split
    let basis_split = {
        let b = &a["basis"];
        let all: u64 = BASIS.iter().map(|(k, _, _)| bn(b,k)).sum();
        let label = |it: &'static str, e: &'static str| if en { e } else { it };
        html! {<section class="card obs-card" id="observer-basis">
            <div class="obs-card-head"><h3>{tr("Su cosa si basano le risposte","What the responses rest on")}</h3><span class="muted">{format!("{all} {}", tr("risposte","responses"))}</span></div>
            if all==0 {<p class="obs-empty">{tr("Nessuna risposta nel periodo selezionato.","No responses in the selected period.")}</p>} else {
            <div class="obs-split" role="img" aria-label={BASIS.iter().map(|(k,i,e)| format!("{} {}", label(i,e), bn(b,k))).collect::<Vec<_>>().join(", ")}>
                {for BASIS.iter().filter(|(k,_,_)| bn(b,k)>0).map(|(k,i,e)| {
                    let v = bn(b,k);
                    let p = pct(v, all);
                    html!{<div class={classes!("obs-split-seg",format!("basis-{k}"))} style={format!("flex-grow:{v}")} data-tip={format!("{} · {v} ({p:.0}%)",label(i,e))}>if p>=12.0{<span>{format!("{p:.0}%")}</span>}</div>}
                })}
            </div>
            <ul class="obs-basis-legend">{for BASIS.iter().map(|(k,i,e)| {
                let v = bn(b,k);
                html!{<li class={classes!((v==0).then_some("zero"))}><i class={format!("basis-{k}")}/><span>{label(i,e)}</span><b>{v}</b><small>{format!("{:.0}%",pct(v,all))}</small></li>}
            })}</ul>}
        </section>}
    };

    // ---------------------------------------------------------------- per-turn columns
    let column_chart = |id: &str, title: &str, stacked: bool| {
        let values: Vec<(u64, Vec<(&str, u64)>, bool)> = turns.iter().map(|t| {
            let a = activity(&t[block_key], &r);
            let parts = if stacked {
                BASIS.iter().map(|(k,_,_)| (*k, bn(&a["basis"],k))).collect()
            } else {
                vec![("interactions", n(&a["interactions"]))]
            };
            (n(&t["turn"]), parts, t["closed"] != true && started)
        }).collect();
        let max = values.iter().map(|(_, p, _)| p.iter().map(|x| x.1).sum::<u64>()).max().unwrap_or(0).max(1);
        let label = |k: &str| BASIS.iter().find(|(x,_,_)| *x==k).map(|(_,i,e)| if en {*e} else {*i}).unwrap_or(tr("Interazioni","Interactions"));
        html! {<section class="card obs-card" id={id.to_string()}>
            <div class="obs-card-head"><h3>{title}</h3><span class="muted">{if *cumulative {tr("cumulato","cumulative")} else {tr("per turno","per turn")}}</span></div>
            if stacked {<ul class="obs-basis-legend inline">{for BASIS.iter().map(|(k,i,e)| html!{<li><i class={format!("basis-{k}")}/><span>{if en {*e} else {*i}}</span></li>})}</ul>}
            <div class="obs-columns" style={format!("--cols:{}",values.len().max(1))}>
                {for values.iter().map(|(k, parts, open)| {
                    let sum: u64 = parts.iter().map(|x| x.1).sum();
                    let picked = picked_turn.clone();
                    let k = *k;
                    let tip = std::iter::once(format!("T{k}: {sum}")).chain(parts.iter().filter(|x| x.1>0 && stacked).map(|(p,v)| format!("{} {v}",label(p)))).collect::<Vec<_>>().join(" · ");
                    html!{<button class={classes!("obs-col",(k==sel_turn).then_some("active"),open.then_some("open"))} data-tip={tip} onclick={Callback::from(move |_| picked.set(Some(k)))}>
                        <span class="obs-col-plot"><span class="obs-col-stack" style={format!("height:{:.1}%",sum as f64*100.0/max as f64)}>
                            <span class="obs-col-value">{sum}</span>
                            {for parts.iter().rev().filter(|x| x.1>0).map(|(p,v)| html!{<span class={classes!("obs-col-seg",if stacked {format!("basis-{p}")} else {"series-interactions".to_string()})} style={format!("flex-grow:{v}")}/>})}
                        </span></span>
                        <span class="obs-col-label">{format!("T{k}")}if *open{<small>{tr("in corso","live")}</small>}</span>
                    </button>}
                })}
            </div>
        </section>}
    };

    // ---------------------------------------------------------------- per-turn table
    let rows: [(&str, &str, &str); 10] = [
        ("received", "Inject ricevuti", "Injects received"),
        ("responses", "Risposte agli inject", "Responses to injects"),
        ("basis.plan", "· su piano o procedura", "· on a plan or procedure"),
        ("basis.law", "· su legge o norma", "· on a law or regulation"),
        ("basis.judgment", "· giudizio operativo", "· operational judgement"),
        ("interactions", "Interazioni tra player", "Player interactions"),
        ("conversations", "Conversazioni avviate", "Conversations started"),
        ("answers", "Risposte a domande EXCON", "Answers to EXCON questions"),
        ("suggested", "Azioni suggerite dall’AI", "AI-suggested actions"),
        ("executed", "· eseguite dall’EXCON", "· executed by EXCON"),
    ];
    let get = |a: &Value, key: &str| match key.split_once('.') {
        Some((x, y)) => bn(&a[x], y),
        None => n(&a[key]),
    };
    let table = html! {<section class="card obs-card" id="observer-table">
        <div class="obs-card-head"><h3>{tr("Numeri per turno","Numbers per turn")}</h3><span class="muted">{tr("valore del turno · Σ cumulato","turn value · Σ cumulative")}</span></div>
        <div class="obs-table-wrap"><table class="obs-table">
            <thead><tr><th/>{for turns.iter().map(|t| html!{<th class={classes!((n(&t["turn"])==sel_turn).then_some("active"),(t["closed"]!=true&&started).then_some("open"))}>{format!("T{}",t["turn"])}if t["closed"]!=true&&started{<small>{tr("in corso","live")}</small>}</th>})}<th class="total">{tr("Totale","Total")}</th></tr></thead>
            <tbody>{for rows.iter().map(|(key,i,e)| {
                let sub = i.starts_with('·');
                html!{<tr class={classes!(sub.then_some("sub"))}><th scope="row">{if en {*e} else {*i}}</th>
                    {for turns.iter().map(|t| {
                        let v = get(activity(&t["turn_only"],&r),key);
                        let c = get(activity(&t["cumulative"],&r),key);
                        html!{<td class={classes!((n(&t["turn"])==sel_turn).then_some("active"),(v==0).then_some("zero"))}><b>{v}</b><small>{format!("Σ {c}")}</small></td>}
                    })}
                    <td class="total"><b>{turns.last().map(|t| get(activity(&t["cumulative"],&r),key)).unwrap_or(0)}</b></td></tr>}
            })}</tbody>
        </table></div>
    </section>};

    // ---------------------------------------------------------------- actors heatmap
    let actors = if r.is_empty() {
        let max = turns.iter().flat_map(|t| roles.iter().map(move |ro| total(activity(&t["turn_only"], text(&ro["id"]))))).max().unwrap_or(0).max(1);
        let last = turns.last().cloned().unwrap_or(Value::Null);
        let max_cum = roles.iter().map(|ro| total(activity(&last["cumulative"], text(&ro["id"])))).max().unwrap_or(0).max(1);
        html! {<section class="card obs-card" id="observer-actors">
            <div class="obs-card-head"><h3>{tr("Attività di ciascun attore","Activity of each actor")}</h3><span class="muted">{tr("contributi nel turno: risposte + interazioni + risposte a domande","contributions per turn: responses + interactions + answers")}</span></div>
            <div class="obs-heat" style={format!("--cols:{}",turns.len().max(1))}>
                <div class="obs-heat-row head"><span/>{for turns.iter().map(|t| html!{<span class={classes!("obs-heat-h",(n(&t["turn"])==sel_turn).then_some("active"))}>{format!("T{}",t["turn"])}</span>})}<span class="obs-heat-h">{tr("Totale","Total")}</span><span class="obs-heat-h">{tr("Azioni AI","AI actions")}</span></div>
                {for roles.iter().map(|ro| {
                    let id = text(&ro["id"]).to_string();
                    let cum = activity(&last["cumulative"], &id);
                    let all = total(cum);
                    let pick = { let role = role.clone(); let id = id.clone(); Callback::from(move |_| role.set(id.clone())) };
                    html!{<div class="obs-heat-row">
                        <button class="obs-heat-name" onclick={pick} title={tr("Apri il dettaglio del giocatore","Open the player detail")}>{text(&ro["name"])}</button>
                        {for turns.iter().map(|t| {
                            let a = activity(&t["turn_only"], &id);
                            let v = total(a);
                            let step = if v == 0 { 0 } else { 1 + (v * 5).div_ceil(max).min(5) };
                            html!{<span class={classes!("obs-heat-cell",format!("h{step}"),(n(&t["turn"])==sel_turn).then_some("active"),(t["closed"]!=true&&started).then_some("open"))} data-tip={format!("{} · T{} · {} {} · {} {} · {} {} · {} {}",text(&ro["name"]),t["turn"],n(&a["responses"]),tr("risposte","responses"),n(&a["interactions"]),tr("interazioni","interactions"),n(&a["answers"]),tr("risposte a domande","answers"),n(&a["suggested"]),tr("azioni AI","AI actions"))}>{v}</span>}
                        })}
                        <span class="obs-heat-total"><span class="obs-bar"><span style={format!("width:{:.1}%",pct(all,max_cum))}/></span><b>{all}</b></span>
                        <span class="obs-heat-ai" data-tip={format!("{} {} · {} {}",n(&cum["suggested"]),tr("suggerite","suggested"),n(&cum["executed"]),tr("eseguite","executed"))}><b>{n(&cum["executed"])}</b><small>{format!("/ {}",n(&cum["suggested"]))}</small></span>
                    </div>}
                })}
            </div>
        </section>}
    } else {
        Html::default()
    };

    // ---------------------------------------------------------------- injects
    let injects = {
        let start_clock = d["scenario"]["start_clock_s"].as_i64().unwrap_or(0);
        let rows: Vec<&Value> = list(&d["injects"]).iter().filter(|i| {
            r.is_empty() || list(&i["recipients"]).iter().any(|x| x == r.as_str()) || !i["roles"][r.as_str()].is_null()
        }).collect();
        let max = rows.iter().map(|i| { let a = activity(i, &r); n(&a["responses"]).max(n(&a["interactions"])) }).max().unwrap_or(0).max(1);
        html! {<section class="card obs-card" id="observer-injects">
            <div class="obs-card-head"><h3>{tr("Risposte e interazioni per inject","Responses and interactions per inject")}</h3><span class="muted">{format!("{} {}", rows.len(), tr("inject","injects"))}</span></div>
            if rows.is_empty() {<p class="obs-empty">{tr("Nessun inject per questa selezione.","No injects for this selection.")}</p>} else {
            <div class="obs-table-wrap"><table class="obs-table obs-injects">
                <thead><tr><th>{"Inject"}</th><th>{tr("Previsto","Planned")}</th><th>{tr("Stato","Status")}</th><th>{tr("Risposte","Responses")}</th><th>{tr("Fondamento","Basis")}</th><th>{tr("Interazioni","Interactions")}</th><th>{tr("1ª risposta","1st response")}</th><th>{tr("Azioni AI","AI actions")}</th></tr></thead>
                <tbody>{for rows.iter().map(|i| {
                    let a = activity(i, &r);
                    let b = &a["basis"];
                    let all: u64 = BASIS.iter().map(|(k,_,_)| bn(b,k)).sum();
                    let status = if let Some(t) = i["sent_turn"].as_u64() {
                        html!{<span class="obs-pill sent">{format!("{} T{t}", tr("Inviato","Sent"))}</span>}
                    } else if i["skipped"] == true {
                        html!{<span class="obs-pill skipped">{tr("Saltato","Skipped")}</span>}
                    } else if started && i["planned_s"].as_i64().is_some_and(|s| s <= d["scenario"]["now_s"].as_i64().unwrap_or(0)) {
                        html!{<span class="obs-pill late">{tr("Previsto, non inviato","Due, not sent")}</span>}
                    } else {
                        html!{<span class="obs-pill future">{tr("Da inviare","To send")}</span>}
                    };
                    let recipients = list(&i["recipients"]).iter().map(|x| role_name(text(x))).collect::<Vec<_>>().join(", ");
                    html!{<tr class={classes!((i["sent_turn"].as_u64()==Some(sel_turn)).then_some("active"),i["sent_turn"].is_null().then_some("unsent"))}>
                        <th scope="row"><div class="obs-inject-title">{text(&i["title"])}</div><small>{if i["msel"]==true {text(&i["id"]).to_string()} else {tr("personalizzato","custom").to_string()}}{format!(" · {}",text(&i["sender"]))}if !recipients.is_empty(){{format!(" → {recipients}")}}</small></th>
                        <td>{match i["planned_s"].as_i64() { Some(s) => format!("T{} · {}", i["planned_turn"], clock(start_clock,s)), None => "–".into() }}</td>
                        <td>{status}</td>
                        <td class="num"><span class="obs-bar"><span style={format!("width:{:.1}%",pct(n(&a["responses"]),max))}/></span><b>{n(&a["responses"])}</b></td>
                        <td>if all>0 {<div class="obs-split mini" data-tip={BASIS.iter().filter(|(k,_,_)| bn(b,k)>0).map(|(k,it,e)| format!("{} {}", if en {*e} else {*it}, bn(b,k))).collect::<Vec<_>>().join(" · ")}>{for BASIS.iter().filter(|(k,_,_)| bn(b,k)>0).map(|(k,_,_)| html!{<div class={classes!("obs-split-seg",format!("basis-{k}"))} style={format!("flex-grow:{}",bn(b,k))}/>})}</div>} else {<span class="muted">{"–"}</span>}</td>
                        <td class="num"><span class="obs-bar alt"><span style={format!("width:{:.1}%",pct(n(&a["interactions"]),max))}/></span><b>{n(&a["interactions"])}</b></td>
                        <td>{if r.is_empty() { i["first_response_ms"].as_f64().map(short).unwrap_or_else(||"–".into()) } else { "–".into() }}</td>
                        <td class="num">{format!("{} / {}", n(&a["executed"]), n(&a["suggested"]))}</td>
                    </tr>}
                })}</tbody>
            </table></div>}
        </section>}
    };

    let subject = if r.is_empty() { tr("Tutti i giocatori","All players").to_string() } else { role_name(&r) };
    html! {<main class="page obs" id="observer">
        <header class="obs-head">
            <div><div class="eyebrow">{tr("Osservatori · cruscotto di analisi","Observers · analysis dashboard")}</div>
            <h2>{subject}</h2>
            <p class="muted">{tr("I numeri di ogni turno si consolidano alla sua chiusura; il turno in corso è provvisorio.","Each turn's numbers are final once it closes; the live turn is provisional.")}</p></div>
            if !error.is_empty() {<p class="error" role="alert">{(*error).clone()}</p>}
        </header>
        <div class="obs-hero">{timer}{scenario}{pace}</div>
        <div class="obs-nav">{role_tabs}</div>
        <div class="obs-toolbar">{turn_tabs}<div class="grow"/>{mode}</div>
        if provisional {<p class="obs-provisional" role="status"><span class="obs-dot live"/>{format!("T{sel_turn} {}", tr("in corso: dati provvisori, si consolidano a fine turno.","live: provisional numbers, final at the end of the turn."))}</p>}
        {kpis}
        <div class="obs-grid">
            {column_chart("observer-responses-chart", tr("Risposte agli inject, per fondamento","Responses to injects, by basis"), true)}
            {column_chart("observer-interactions-chart", tr("Interazioni tra player","Player interactions"), false)}
            {basis_split}
        </div>
        {actors}
        {table}
        {injects}
    </main>}
}
