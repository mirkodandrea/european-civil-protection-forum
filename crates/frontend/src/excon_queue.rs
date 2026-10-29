//! EXCON actions that wait for play: the "when" choice of the Controllo tools
//! and the queue (what, when, cancel, change the time) shown in Controllo and
//! next to the turn panel of the Timeline. The model is `exercise::queue`.
use crate::{
    api::{self, list, text},
    i18n::ex,
    Action,
};
use serde_json::{json, Value};
use yew::prelude::*;

/// When a Controllo action takes effect.
#[derive(Clone, PartialEq, Debug)]
pub enum When {
    /// When the EXCON next advances time (play).
    Play,
    /// At a clock time of Day 1, `HH:MM`.
    At(String),
    /// At once (exercise control escape hatch).
    Now,
}

/// Body of `/api/master/command` for `cmd` taking effect `when`.
pub fn command_body(cmd: Value, when: &When) -> Value {
    match when {
        When::Play => json!({"commands": [cmd], "when": "play"}),
        When::At(clock) => json!({"commands": [cmd], "at_clock": clock}),
        When::Now => json!({"commands": [cmd], "when": "now"}),
    }
}

/// `HH:MM` (or `HH:MM:SS`) that the server accepts.
pub fn valid_clock(c: &str) -> bool {
    let p: Vec<_> = c.split(':').collect();
    (2..=3).contains(&p.len())
        && p.iter().all(|x| x.len() == 2 && x.chars().all(|c| c.is_ascii_digit()))
        && p[0] < "24"
        && p[1] < "60"
        && p.get(2).is_none_or(|s| *s < "60")
}

#[derive(Properties, PartialEq)]
pub struct WhenPickerProps {
    pub value: When,
    pub onchange: Callback<When>,
    /// Current clock, as the default for a planned time.
    pub now_clock: AttrValue,
    /// A turn is open for orders: "play" means "when time next moves".
    pub started: bool,
}

/// "When": at the next play (default), at a time, or at once.
#[function_component(WhenPicker)]
pub fn when_picker(p: &WhenPickerProps) -> Html {
    let pick = |w: When| {
        let cb = p.onchange.clone();
        Callback::from(move |_| cb.emit(w.clone()))
    };
    let hhmm = p.now_clock.split(' ').next_back().unwrap_or("").chars().take(5).collect::<String>();
    let at_value = match &p.value {
        When::At(c) => c.clone(),
        _ => hhmm,
    };
    let on_time = {
        let cb = p.onchange.clone();
        Callback::from(move |e: InputEvent| cb.emit(When::At(api::input(e))))
    };
    let bad = matches!(&p.value, When::At(c) if !valid_clock(c));
    html! {<fieldset class="when-pick" data-when="">
        <legend>{ex("Quando")}</legend>
        <label class="mt-check"><input type="radio" name="when" checked={p.value == When::Play} onchange={pick(When::Play)}/>
            {if p.started {ex("Al prossimo avanzamento (play)")} else {ex("Subito (preparazione)")}}</label>
        <label class="mt-check"><input type="radio" name="when" checked={matches!(p.value, When::At(_))} onchange={pick(When::At(at_value.clone()))}/>
            {ex("All’ora simulata")}
            <input type="time" aria-label={ex("Ora simulata")} value={at_value} disabled={!matches!(p.value, When::At(_))} oninput={on_time}/></label>
        <label class="mt-check"><input type="radio" name="when" checked={p.value == When::Now} onchange={pick(When::Now)}/>{ex("Subito, a turno aperto (eccezione)")}</label>
        if bad {<p class="error">{ex("Ora non valida (HH:MM).")}</p>}
        <p class="muted">{match &p.value {
            When::Play if p.started => ex("Resta in coda e si applica quando avanzi il tempo: i player non vedono cambiare la situazione mentre rispondono."),
            When::Play => ex("L’esercitazione non è iniziata: si applica subito."),
            When::At(_) => ex("Resta in coda e si applica a quell’ora simulata, indipendentemente dagli inject."),
            When::Now => ex("Cambia il mondo adesso, sotto gli occhi dei player: usalo solo per correggere l’esercitazione."),
        }}</p>
    </fieldset>}
}

/// Pending rows first, chronological; then the rest, most recent first.
pub fn split_queue(rows: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let (mut pending, mut done): (Vec<Value>, Vec<Value>) = rows.iter().cloned().partition(|r| r["status"] == "approved");
    pending.sort_by_key(|r| r["run_at_s"].as_i64().unwrap_or(0));
    done.sort_by_key(|r| std::cmp::Reverse(r["executed_s"].as_i64().or(r["cancelled_s"].as_i64()).unwrap_or(0)));
    (pending, done)
}

/// When a pending row applies, in words.
pub fn when_text(r: &Value, now_s: i64) -> String {
    let at = r["run_at_s"].as_i64().unwrap_or(0);
    if r["on_play"] == true || at <= now_s {
        ex("al prossimo avanzamento").to_string()
    } else {
        crate::exfmt!("alle {}", "at {}", text(&r["run_at_clock"]).split(' ').next_back().unwrap_or(""))
    }
}

#[derive(Properties, PartialEq)]
pub struct ExconQueueProps {
    pub master: Value,
    pub state: Value,
    pub busy: bool,
    pub action: Callback<Action>,
    /// Pending rows only, without the history (Timeline side column).
    #[prop_or_default]
    pub compact: bool,
}

/// The EXCON actions waiting for play or for their time, with cancel and
/// change-time; in full also those already applied or cancelled.
#[function_component(ExconQueue)]
pub fn excon_queue(p: &ExconQueueProps) -> Html {
    let editing = use_state(|| None::<(u64, String)>);
    let rows = list(&p.master["excon_queue"]);
    let (pending, done) = split_queue(rows);
    if p.compact && pending.is_empty() {
        return Html::default();
    }
    let now = p.state["world"]["stats"]["time_s"].as_i64().or(p.master["agenda"]["now_s"].as_i64()).unwrap_or(0);
    let row = |r: &Value| {
        let id = r["id"].as_u64().unwrap_or(0);
        let cancel = {
            let cb = p.action.clone();
            Callback::from(move |_| cb.emit(Action::post(format!("/api/master/excon-actions/{id}/cancel"), json!({"reason": ex("Annullata dall’EXCON")}))))
        };
        let is_editing = editing.as_ref().filter(|(e, _)| *e == id).map(|(_, c)| c.clone());
        let edit_ui = match is_editing {
            Some(clock) => {
                let ok = valid_clock(&clock);
                let save = {
                    let (cb, editing, clock) = (p.action.clone(), editing.clone(), clock.clone());
                    Callback::from(move |_| {
                        cb.emit(Action::post(format!("/api/master/excon-actions/{id}/edit"), json!({"at_clock": clock})));
                        editing.set(None);
                    })
                };
                let to_play = {
                    let (cb, editing) = (p.action.clone(), editing.clone());
                    Callback::from(move |_| {
                        cb.emit(Action::post(format!("/api/master/excon-actions/{id}/edit"), json!({"when": "play"})));
                        editing.set(None);
                    })
                };
                let oninput = {
                    let editing = editing.clone();
                    Callback::from(move |e: InputEvent| editing.set(Some((id, api::input(e)))))
                };
                html! {<span class="eq-edit"><input type="time" aria-label={ex("Nuova ora simulata")} value={clock} {oninput}/>
                    <button class="tp-small" disabled={p.busy || !ok} onclick={save}>{ex("Salva")}</button>
                    <button class="tp-small" disabled={p.busy} onclick={to_play}>{ex("Al play")}</button></span>}
            }
            None if r["editable"] == true => {
                let (editing, start) = (editing.clone(), text(&r["run_at_clock"]).split(' ').next_back().unwrap_or("").chars().take(5).collect::<String>());
                html! {<button class="tp-small" disabled={p.busy} onclick={Callback::from(move |_| editing.set(Some((id, start.clone()))))}>{ex("Cambia ora")}</button>}
            }
            None => Html::default(),
        };
        html! {<li class="eq-row" key={id} data-queued={id.to_string()}>
            <span class="pill eq-when">{when_text(r, now)}</span>
            <b class="eq-title">{text(&r["title"])}</b>
            <span class="grow"/>
            {edit_ui}
            <button class="tp-small warn" disabled={p.busy} onclick={cancel} aria-label={crate::exfmt!("Annulla {}", "Cancel {}", text(&r["title"]))}>{ex("Annulla")}</button>
        </li>}
    };
    let past = |r: &Value| {
        let (label, class) = match text(&r["status"]) {
            "done" => (ex("applicata"), "ok"),
            "failed" => (ex("non applicata"), "bad"),
            _ => (ex("annullata"), "off"),
        };
        let when = r["executed_s"].as_i64().or(r["cancelled_s"].as_i64()).map(|t| crate::timeline::hhmm(p.master["agenda"]["clock_origin_s"].as_i64().unwrap_or(0), t)).unwrap_or_default();
        html! {<li class={classes!("eq-row","past",class)} key={r["id"].to_string()} title={text(&r["result"]).to_string()}>
            <span class="pill">{format!("{when} · {label}")}</span><span>{text(&r["title"])}</span></li>}
    };
    html! {<section class={classes!("card","excon-queue",p.compact.then_some("compact"))} id="excon-queue" aria-label={ex("Azioni EXCON in coda")}>
        <h3>{crate::exfmt!("Azioni EXCON in coda ({})", "Queued EXCON actions ({})", pending.len())}</h3>
        if pending.is_empty() {<p class="muted">{ex("Nessuna azione in coda. Le azioni prese a turno aperto aspettano il prossimo avanzamento.")}</p>}
        else {<ul class="eq-list">{for pending.iter().map(row)}</ul>}
        if !p.compact && !done.is_empty() {<details class="eq-past"><summary>{crate::exfmt!("Applicate o annullate ({})", "Applied or cancelled ({})", done.len())}</summary>
            <ul class="eq-list">{for done.iter().take(30).map(past)}</ul></details>}
    </section>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bodies_and_clocks() {
        let cmd = json!({"type":"Ignite","at":"F4"});
        assert_eq!(command_body(cmd.clone(), &When::Play), json!({"commands":[cmd.clone()],"when":"play"}));
        assert_eq!(command_body(cmd.clone(), &When::At("13:00".into()))["at_clock"], "13:00");
        assert_eq!(command_body(cmd, &When::Now)["when"], "now");
        assert!(valid_clock("13:00") && valid_clock("09:05:30"));
        assert!(!valid_clock("25:00") && !valid_clock("13") && !valid_clock("1:00") && !valid_clock(""));
    }
    #[test]
    fn pending_rows_come_first_in_time_order() {
        let rows = vec![
            json!({"id":1,"status":"done","executed_s":60}),
            json!({"id":2,"status":"approved","run_at_s":600}),
            json!({"id":3,"status":"approved","run_at_s":0,"on_play":true}),
            json!({"id":4,"status":"cancelled","cancelled_s":120}),
        ];
        let (p, d) = split_queue(&rows);
        assert_eq!(p.iter().map(|r| r["id"].as_u64().unwrap()).collect::<Vec<_>>(), vec![3, 2]);
        assert_eq!(d.iter().map(|r| r["id"].as_u64().unwrap()).collect::<Vec<_>>(), vec![4, 1]);
        assert_eq!(when_text(&p[0], 0), ex("al prossimo avanzamento"));
        assert!(when_text(&json!({"run_at_s":600,"run_at_clock":"Day 1 13:10"}), 0).ends_with("13:10"));
    }
}
