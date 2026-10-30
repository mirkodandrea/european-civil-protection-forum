//! EXCON "Azioni dalle risposte": what the AI suggests from each player reply
//! thread, for the EXCON to execute, edit or ignore (see
//! `exercise::reply_review`). Players never act: only an executed suggestion
//! changes the simulation.
//!
//! One group per reply (role, inject, the reply in one line), then one compact
//! row per suggestion with its state at a glance (complete, incomplete with the
//! missing fields, question to review, awaiting the answer, executed, ignored)
//! and its details on demand (parameters, AI notes, source words); then the
//! questions: drafts are edited and sent with one click, never automatically.
use crate::{
    api::{list, text},
    timeline::role_name,
    Action,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use yew::prelude::*;

fn tr(it: &'static str, en: &'static str) -> &'static str {
    if crate::i18n_excon::is_en() {
        en
    } else {
        it
    }
}

fn lang() -> &'static str {
    if crate::i18n_excon::is_en() {
        "en"
    } else {
        "it"
    }
}

/// Label and tone of a suggestion's `display` state.
pub fn item_state(display: &str) -> (&'static str, &'static str) {
    match display {
        "complete" => (tr("Completa", "Complete"), "ok"),
        "infeasible" => (tr("Completa · non fattibile ora", "Complete · not feasible now"), "bad"),
        "incomplete" => (tr("Incompleta", "Incomplete"), "gap"),
        "question_draft" => (tr("Incompleta · domanda da rivedere", "Incomplete · question to review"), "gap"),
        "awaiting_answer" => (tr("Incompleta · attende risposta", "Incomplete · awaiting answer"), "wait"),
        "queued" => (tr("In coda · al prossimo avanzamento", "Queued · next advance"), "wait"),
        "executed" => (tr("Eseguita", "Executed"), "done"),
        "failed" => (tr("Non eseguita", "Not executed"), "bad"),
        "ignored" => (tr("Ignorata", "Ignored"), "off"),
        _ => (tr("—", "—"), "off"),
    }
}

/// Label and tone of a question's state.
pub fn question_state(state: &str) -> (&'static str, &'static str) {
    match state {
        "draft" => (tr("Domanda da rivedere", "Question to review"), "gap"),
        "sent" => (tr("Inviata · attende risposta", "Sent · awaiting answer"), "wait"),
        "answered" => (tr("Risposta ricevuta", "Answer received"), "done"),
        _ => (tr("Scartata", "Discarded"), "off"),
    }
}

fn change_label(change: &str) -> Option<&'static str> {
    match change {
        "completed" => Some(tr("completata dalla risposta", "completed by the answer")),
        "changed" => Some(tr("modificata dalla risposta", "changed by the answer")),
        "new" => Some(tr("nuova", "new")),
        _ => None,
    }
}

/// The parameters the editor shows for an action type (required first).
pub fn fields_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "CutFireLine" => &["from", "to", "along", "width_m"],
        "WaterDrop" | "DeployEngine" => &["at"],
        "MonitorArea" => &["zones"],
        "Reconnoitre" | "IssueWarning" => &["zone"],
        "EvacuateZone" => &["zone", "destination"],
        "EvacuateBuilding" => &["building", "destination"],
        "OpenShelter" => &["building"],
        "CloseRoad" | "ReopenRoad" => &["road"],
        "RequestReinforcement" => &["resource", "qty"],
        _ => &[],
    }
}

/// "Dopo la risposta: 1 completata · 2 nuove" (only what happened).
fn diff_line(completed: usize, changed: usize, new: usize) -> String {
    let part = |n: usize, one: (&'static str, &'static str), many: (&'static str, &'static str)| match n {
        0 => None,
        1 => Some(format!("1 {}", tr(one.0, one.1))),
        n => Some(format!("{n} {}", tr(many.0, many.1))),
    };
    let parts: Vec<String> = [
        part(completed, ("completata", "completed"), ("completate", "completed")),
        part(changed, ("modificata", "changed"), ("modificate", "changed")),
        part(new, ("nuova", "new"), ("nuove", "new")),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!("{}{}", tr("Dopo la risposta: ", "After the answer: "), parts.join(" · "))
}

/// Name of a command field in the console language.
fn field_label(f: &str) -> &str {
    match f {
        "along" => tr("Lungo il lato (es. F4:N)", "Along the side (e.g. F4:N)"),
        _ => crate::i18n::field(lang(), f),
    }
}

/// Known parameters in one line (`Da E3 · A F3`), or "nessun parametro".
fn params_line(params: &Value) -> String {
    let parts: Vec<String> = params.as_object().into_iter().flatten().map(|(k, v)| format!("{} {}", field_label(k), param_text(v))).collect();
    if parts.is_empty() {
        tr("nessun parametro", "no parameters").into()
    } else {
        parts.join(" · ")
    }
}

/// A parameter as the editor shows it: points as `x,y`, lists joined.
pub fn param_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| text(x).to_string()).collect::<Vec<_>>().join(", "),
        Value::Object(o) if o.contains_key("x") => format!("{:.0},{:.0}", v["x"].as_f64().unwrap_or(0.0), v["y"].as_f64().unwrap_or(0.0)),
        other => other.to_string(),
    }
}

/// Back from the editor: `x,y` for a location becomes a point.
pub fn param_value(field: &str, s: &str) -> Value {
    let s = s.trim();
    if matches!(field, "at" | "from" | "to") {
        if let Some((x, y)) = s.split_once(',') {
            if let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) {
                return json!({"x": x, "y": y});
            }
        }
    }
    json!(s)
}

/// The suggestion in one line, with `?` for each missing field.
fn summary(item: &Value) -> String {
    let kind = text(&item["kind"]);
    let l = lang();
    let missing: Vec<&str> = list(&item["missing"]).iter().map(text).collect();
    if missing.is_empty() && item["invalid"].is_null() {
        let mut cmd = item["params"].clone();
        cmd["type"] = json!(kind);
        return crate::widgets::command_summary(l, &cmd);
    }
    let parts: Vec<String> = fields_for(kind)
        .iter()
        .filter(|f| item["params"].get(**f).is_some() || missing.contains(f))
        .map(|f| {
            let v = param_text(&item["params"][*f]);
            format!("{} {}", field_label(f), if v.is_empty() { "?".into() } else { v })
        })
        .collect();
    format!("{} · {}", crate::i18n::action(l, kind), parts.join(" · "))
}

#[derive(Properties, PartialEq)]
pub struct ReviewBoardProps {
    pub master: Value,
    pub stat: Value,
    pub action: Callback<Action>,
    pub busy: bool,
}

#[function_component(ReviewBoard)]
pub fn review_board(props: &ReviewBoardProps) -> Html {
    // Local edits: `p:<item>:<field>` parameters, `q:<question>` question texts,
    // `ask:<reply>` / `ask_to:<reply>` an EXCON-written question.
    let edits = use_state(BTreeMap::<String, String>::new);
    let open = use_state(|| Option::<u64>::None);
    let board = &props.master["reply_review"];
    let reviews = list(&board["reviews"]);
    let roles = list(&props.stat["roles"]).to_vec();
    let counts = &board["counts"];
    let n = |k: &str| counts[k].as_u64().unwrap_or(0);
    let (open_reviews, handled) = split_reviews(reviews);
    let group = |r: &Value| review_group(r, props, &roles, &edits, &open);
    html! {<section class="card review-board" id="review-board" aria-labelledby="review-board-title">
        <header class="rb-head"><h2 id="review-board-title">{tr("Azioni dalle risposte","Actions from the replies")}</h2>
            <span class="pill rb-tone-ok" hidden={n("complete")==0} data-review-count="complete">{crate::exfmt!("{} da eseguire","{} to execute",n("complete"))}</span>
            <span class="pill rb-tone-gap" hidden={n("drafts")==0} data-review-count="drafts">{if n("drafts")==1{tr("1 domanda da rivedere","1 question to review").to_string()}else{crate::exfmt!("{} domande da rivedere","{} questions to review",n("drafts"))}}</span>
            <span class="pill rb-tone-wait" hidden={n("awaiting_answer")==0} data-review-count="awaiting">{crate::exfmt!("{} in attesa di risposta","{} awaiting an answer",n("awaiting_answer"))}</span>
            <span class="pill" hidden={n("reading")==0}>{tr("L’AI sta leggendo…","The AI is reading…")}</span>
        </header>
        <p class="muted rb-intro">{tr("I player rispondono solo a parole. L’AI propone azioni del catalogo con la fonte; nulla accade finché non la metti in coda (si applica al prossimo avanzamento del tempo), e nessuna domanda parte senza Invia.","Players reply in words only. The AI proposes catalogue actions with their source; nothing happens until you queue it (it applies at the next time advance), and no question leaves without Send.")}</p>
        if reviews.is_empty(){<p class="muted">{tr("Nessuna risposta dei player da valutare.","No player replies to review.")}</p>}
        {for open_reviews.iter().map(|r|group(r))}
        if !handled.is_empty(){<details class="rb-handled" data-review-handled={handled.len().to_string()}><summary>{crate::exfmt!("Risposte gestite ({})","Handled replies ({})",handled.len())}</summary>
            {for handled.iter().map(|r|group(r))}</details>}
    </section>}
}

/// Reviews still to handle first; closed (handled) ones apart, newest first.
pub fn split_reviews(reviews: &[Value]) -> (Vec<&Value>, Vec<&Value>) {
    reviews.iter().partition(|r| r["status"] != "closed")
}

fn review_group(r: &Value, props: &ReviewBoardProps, roles: &[Value], edits: &UseStateHandle<BTreeMap<String, String>>, open: &UseStateHandle<Option<u64>>) -> Html {
    let set = {
        let edits = edits.clone();
        move |key: String| {
            let edits = edits.clone();
            Callback::from(move |e: InputEvent| {
                let mut next = (*edits).clone();
                next.insert(key.clone(), crate::api::input(e));
                edits.set(next);
            })
        }
    };
    let post = |path: String, body: Value| {
        let a = props.action.clone();
        Callback::from(move |_: MouseEvent| a.emit(Action::post(path.clone(), body.clone())))
    };
    let roles = roles.to_vec();
    let (edits, open) = (edits.clone(), open.clone());
    {
            let pid=r["proposal"].as_u64().unwrap_or(0);
            let status=text(&r["status"]);
            let diff=&r["diff"];
            let (dc,dch,dn)=(list(&diff["completed"]).len(),list(&diff["changed"]).len(),list(&diff["new"]).len());
            let closed=status=="closed";
            html!{<article class={classes!("rb-group",closed.then_some("is-closed"))} key={pid} data-review={pid.to_string()}>
                <div class="rb-source">
                    <span class="pill rcpt sent">{role_name(&roles,text(&r["role"]))}</span>
                    <span class="rb-inject"><b>{text(&r["inject_title"])}</b><small>{format!(" · {}",text(&r["reply_clock"]))}</small></span>
                    <q class="rb-reply" title={text(&r["reply"]).to_string()}>{text(&r["reply"])}</q>
                    <div class="grow"/>
                    if status=="reading"{<span class="pill rb-tone-wait" data-review-status="reading">{tr("lettura AI…","AI reading…")}</span>}
                    if status=="failed"{<span class="pill rb-tone-bad" data-review-status="failed" title={text(&r["error"]).to_string()}>{tr("AI non disponibile: usa Controllo","AI unavailable: use Controllo")}</span>}
                    if closed{<span class="pill rb-tone-off">{tr("revisione chiusa","review closed")}</span>}
                    if r["revision"].as_u64().unwrap_or(1)>1&&(dc+dch+dn)>0{<span class="pill rb-diff" data-review-diff="">{diff_line(dc,dch,dn)}</span>}
                    <button class="rb-mini" disabled={props.busy||status=="reading"} onclick={post(format!("/api/master/review/reread/{pid}"),json!({}))} title={tr("L’AI rilegge l’intero thread","The AI reads the whole thread again")}>{tr("Rileggi","Re-read")}</button>
                    if !closed{<button class="rb-mini" disabled={props.busy} data-review-close={pid.to_string()} onclick={post(format!("/api/master/review/close/{pid}"),json!({}))}>{tr("Chiudi revisione","Close review")}</button>}
                </div>
                if !text(&r["summary"]).is_empty(){<p class="rb-summary muted">{text(&r["summary"])}</p>}
                if list(&r["items"]).is_empty()&&status=="ready"{<p class="muted rb-none">{tr("Nessuna azione del catalogo in questa risposta.","No catalogue action in this reply.")}</p>}
                <ol class="rb-items">{for list(&r["items"]).iter().map(|i|{
                    let id=i["id"].as_u64().unwrap_or(0);
                    let display=text(&i["display"]);
                    let (label,tone)=item_state(display);
                    let missing:Vec<&str>=list(&i["missing"]).iter().map(text).collect();
                    let rejected=i["verdict"]["verdict"]=="rejected";
                    let decided=matches!(display,"queued"|"executed"|"failed");
                    let is_open=*open==Some(id);
                    let toggle={let open=open.clone();Callback::from(move |_:MouseEvent|open.set(if is_open{None}else{Some(id)}))};
                    let fields=fields_for(text(&i["kind"]));
                    let typed=|f:&str|edits.get(&format!("p:{id}:{f}")).cloned();
                    let params={let mut p=i["params"].as_object().cloned().unwrap_or_default();for f in fields{if let Some(v)=typed(f){if v.trim().is_empty(){p.remove(*f);}else{p.insert((*f).into(),param_value(f,&v));}}}Value::Object(p)};
                    let edited=fields.iter().any(|f|typed(f).is_some());
                    let can_run=!props.busy&&!decided&&display!="ignored"&&((missing.is_empty()&&!rejected)||edited);
                    let run_body=if edited{json!({"params":params})}else{json!({})};
                    html!{<li key={id} class={classes!("rb-item",format!("rb-{tone}"))} data-review-item={id.to_string()} data-review-state={display.to_string()}>
                        <div class="rb-row">
                            <span class={classes!("pill","rb-state",format!("rb-tone-{tone}"))}>{label}</span>
                            <span class="rb-what"><b>{summary(i)}</b>
                                <small>{crate::exfmt!("esegue: {}","executed by: {}",role_name(&roles,text(&i["role"])))}{if i["source_role"]!=i["role"]{crate::exfmt!(" · detto da {}"," · said by {}",role_name(&roles,text(&i["source_role"])))}else{String::new()}}</small></span>
                            if !missing.is_empty(){<span class="rb-missing" data-review-missing={missing.join(",")}>{crate::exfmt!("Mancano: {}","Missing: {}",missing.iter().map(|m|field_label(m).to_string()).collect::<Vec<_>>().join(", "))}</span>}
                            if let Some(c)=change_label(text(&i["change"])){<span class="pill rb-diff">{c}</span>}
                            if rejected&&!decided{<span class="rb-warn" title={text(&i["verdict"]["reason"]).to_string()}>{format!("⚠ {}",text(&i["verdict"]["reason"]))}</span>}
                            <div class="grow"/>
                            if !decided&&display!="ignored"{<button class="primary rb-run" data-review-execute={id.to_string()} disabled={!can_run} title={tr("L’azione va in coda e si applica al prossimo avanzamento del tempo (la annulli dalla coda delle azioni EXCON)","The action is queued and applies at the next time advance (cancel it from the EXCON action queue)")} onclick={post(format!("/api/master/review/execute/{id}"),run_body.clone())}>{tr("Metti in coda","Queue")}</button>}
                            <button class="rb-mini" aria-expanded={is_open.to_string()} data-review-details={id.to_string()} onclick={toggle}>{if is_open{tr("Chiudi","Close")}else if decided{tr("Dettagli","Details")}else{tr("Modifica","Edit")}}</button>
                            if display=="ignored"{<button class="rb-mini" disabled={props.busy} onclick={post(format!("/api/master/review/restore/{id}"),json!({}))}>{tr("Ripristina","Restore")}</button>}
                            else if !decided{<button class="rb-mini" disabled={props.busy} data-review-ignore={id.to_string()} onclick={post(format!("/api/master/review/ignore/{id}"),json!({}))}>{tr("Ignora","Ignore")}</button>}
                        </div>
                        <small class="rb-quote">{format!("«{}»",text(&i["quote"]))}</small>
                        if is_open{<div class="rb-details">
                            if !decided{<div class="rb-fields">{for fields.iter().map(|f|{
                                let key=format!("p:{id}:{f}");
                                let value=edits.get(&key).cloned().unwrap_or_else(||param_text(&i["params"][*f]));
                                let gap=missing.contains(f)&&value.trim().is_empty();
                                html!{<label class={classes!("rb-field",gap.then_some("is-missing"))}><span>{field_label(f)}{if gap{" *"}else{""}}</span>
                                    <input data-review-field={format!("{id}:{f}")} value={value} placeholder={if gap{tr("da chiedere o da inserire","to ask or to fill in")}else{""}} oninput={set(key)}/></label>}
                            })}
                            <button class="rb-mini" disabled={props.busy||!edited} onclick={post(format!("/api/master/review/edit/{id}"),json!({"params":params}))}>{tr("Salva e verifica","Save and check")}</button></div>}
                            if !list(&i["notes"]).is_empty(){<ul class="rb-notes">{for list(&i["notes"]).iter().map(|n|html!{<li>{text(n)}</li>})}</ul>}
                            if i["previous"].is_object(){<small class="muted">{crate::exfmt!("Prima: {}","Before: {}",params_line(&i["previous"]))}</small>}
                            if let Some(ex)=i["execution"].as_object(){<small class={classes!(if ex.get("ok")==Some(&json!(true)){"muted"}else{"danger"})}>{format!("{} · {}",text(&i["execution"]["command"]["type"]),text(&i["execution"]["detail"]))}</small>}
                        </div>}
                    </li>}
                })}</ol>
                <div class="rb-questions">{for list(&r["questions"]).iter().filter(|q|q["state"]!="discarded").map(|q|{
                    let qid=q["id"].as_u64().unwrap_or(0);
                    let state=text(&q["state"]);
                    let (label,tone)=question_state(state);
                    let key=format!("q:{qid}");
                    let draft=edits.get(&key).cloned().unwrap_or_else(||text(&q["text"]).to_string());
                    let about:Vec<String>=list(&q["about"]).iter().filter_map(|a|list(&r["items"]).iter().find(|i|i["id"]==*a)).map(summary).collect();
                    html!{<div key={qid} class={classes!("rb-question",format!("rb-{tone}"))} data-review-question={qid.to_string()} data-question-state={state.to_string()}>
                        <div class="rb-row">
                            <span class={classes!("pill","rb-state",format!("rb-tone-{tone}"))}>{label}</span>
                            <span class="rb-what"><b>{crate::exfmt!("{} → {}","{} → {}",text(&q["persona_name"]),role_name(&roles,text(&q["to"])))}</b>
                                if !about.is_empty(){<small>{crate::exfmt!("su: {}","about: {}",about.join("; "))}</small>}</span>
                            if q["cross_role"]==true{<span class="pill rb-tone-gap" title={text(&q["reason"]).to_string()}>{tr("ad altro ruolo","to another role")}</span>}
                            if q["template"]==true{<span class="pill">{tr("domanda standard","standard question")}</span>}
                        </div>
                        if state=="draft"{<>
                            <textarea class="rb-qtext" data-review-qtext={qid.to_string()} value={draft.clone()} oninput={set(key.clone())}/>
                            <div class="rb-row"><small class="muted">{crate::exfmt!("Parte così com’è, nella lingua del player ({}).","Sent as written, in the player's language ({}).",text(&q["language"]))}</small>
                                if !text(&q["reason"]).is_empty()&&q["cross_role"]!=true{<small class="muted">{text(&q["reason"])}</small>}
                                <div class="grow"/>
                                <button class="rb-mini" disabled={props.busy} data-review-discard={qid.to_string()} onclick={post(format!("/api/master/review/discard/{qid}"),json!({}))}>{tr("Scarta","Discard")}</button>
                                <button class="primary" disabled={props.busy||draft.trim().is_empty()} data-review-send={qid.to_string()} onclick={post(format!("/api/master/review/send/{qid}"),json!({"text":draft}))}>{tr("Invia","Send")}</button></div>
                        </>}else{<p class="rb-qsent">{text(&q["text"])}<small class="muted">{format!(" · {}",text(&q["sent_clock"]))}</small></p>}
                        if q["answer_text"].is_object(){<p class="rb-answer" data-review-answer={qid.to_string()}><b>{tr("Risposta: ","Answer: ")}</b>{text(&q["answer_text"]["original"]).to_string()}<small class="muted">{format!(" · {}",text(&q["answer_text"]["clock"]))}</small></p>}
                        if state=="sent"{<div class="rb-row"><div class="grow"/><button class="rb-mini" disabled={props.busy} onclick={post(format!("/api/master/review/discard/{qid}"),json!({}))}>{tr("Ritira la domanda","Withdraw the question")}</button></div>}
                    </div>}
                })}
                if !closed{{ask_form(pid,&r["role"],&roles,&edits,&props.action,props.busy)}}
                </div>
            </article>}
    }
}

/// "Scrivi una domanda": an EXCON-written draft, reviewed like the AI's.
fn ask_form(pid: u64, author: &Value, roles: &[Value], edits: &UseStateHandle<BTreeMap<String, String>>, action: &Callback<Action>, busy: bool) -> Html {
    let (tk, kk) = (format!("ask:{pid}"), format!("ask_to:{pid}"));
    let txt = edits.get(&tk).cloned().unwrap_or_default();
    let to = edits.get(&kk).cloned().unwrap_or_else(|| text(author).to_string());
    let on_text = {
        let (edits, tk) = (edits.clone(), tk.clone());
        Callback::from(move |e: InputEvent| {
            let mut next = (*edits).clone();
            next.insert(tk.clone(), crate::api::input(e));
            edits.set(next);
        })
    };
    let on_to = {
        let (edits, kk) = (edits.clone(), kk.clone());
        Callback::from(move |e: Event| {
            let mut next = (*edits).clone();
            next.insert(kk.clone(), crate::api::select(e));
            edits.set(next);
        })
    };
    let add = {
        let (action, edits, txt, to, tk) = (action.clone(), edits.clone(), txt.clone(), to.clone(), tk.clone());
        Callback::from(move |_: MouseEvent| {
            action.emit(Action::post(format!("/api/master/review/ask/{pid}"), json!({"to": to, "text": txt})));
            let mut next = (*edits).clone();
            next.remove(&tk);
            edits.set(next);
        })
    };
    html! {<details class="rb-ask"><summary>{tr("+ Scrivi una domanda","+ Write a question")}</summary>
        <div class="rb-row"><label>{tr("A ","To ")}<select onchange={on_to}>{for roles.iter().map(|r|{let id=text(&r["id"]).to_string();html!{<option value={id.clone()} selected={id==to}>{text(&r["name"]).to_string()}</option>}})}</select></label></div>
        <textarea value={txt.clone()} oninput={on_text} placeholder={tr("Nella lingua del player. Diventa una bozza da rivedere.","In the player's language. It becomes a draft to review.")}/>
        <div class="rb-row"><div class="grow"/><button disabled={busy||txt.trim().is_empty()} onclick={add}>{tr("Aggiungi bozza","Add draft")}</button></div>
    </details>}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn states_are_legible_and_distinct() {
        let labels: Vec<_> = ["complete", "infeasible", "incomplete", "question_draft", "awaiting_answer", "queued", "executed", "failed", "ignored"].iter().map(|d| item_state(d).0).collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());
        assert_eq!(question_state("draft").0, "Domanda da rivedere");
    }
    #[test]
    fn incomplete_suggestions_show_question_marks() {
        let line = json!({"kind":"CutFireLine","params":{},"missing":["from","to"],"invalid":null});
        let s = summary(&line);
        assert_eq!(s.matches('?').count(), 2, "{s}");
        assert_eq!(param_value("at", "2800, 2250"), json!({"x": 2800.0, "y": 2250.0}));
        assert_eq!(param_value("at", "F4:N"), json!("F4:N"));
        assert_eq!(param_text(&json!({"x": 2800.0, "y": 2250.0})), "2800,2250");
    }
    #[test]
    fn handled_replies_leave_the_open_list() {
        let reviews = vec![json!({"proposal":10,"status":"ready"}), json!({"proposal":5,"status":"closed"}), json!({"proposal":3,"status":"reading"})];
        let (open, handled) = split_reviews(&reviews);
        assert_eq!(open.iter().map(|r| r["proposal"].as_u64().unwrap()).collect::<Vec<_>>(), vec![10, 3]);
        assert_eq!(handled.len(), 1);
        assert_eq!(handled[0]["proposal"], 5);
    }
}
