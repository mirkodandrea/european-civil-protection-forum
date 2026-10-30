//! What a proposal would send besides its actions, shown as the EXCON will
//! see it go out: messages to roles, scenario injects and resource limits.
//!
//! Each editor reads the draft's JSON text (the form the decision sends) and
//! writes it back after an edit; the raw JSON stays under «Dettagli tecnici».
use crate::{
    api::{self, list, text},
    i18n::ex,
    timeline::role_name,
};
use serde_json::{json, Value};
use yew::prelude::*;

fn parse(src: &str) -> Value {
    serde_json::from_str(src).unwrap_or(Value::Null)
}

/// `value` with `f` applied, as the pretty JSON the decision sends.
fn edited(src: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v = parse(src);
    f(&mut v);
    api::pretty(&v)
}

/// A text field that rewrites one key of draft `i`.
fn on_text(src: &str, onchange: &Callback<String>, i: usize, key: &'static str) -> Callback<InputEvent> {
    let (src, onchange) = (src.to_string(), onchange.clone());
    Callback::from(move |e| {
        let v = api::input(e);
        onchange.emit(edited(&src, |d| d[i][key] = json!(v)));
    })
}

fn remove(src: &str, onchange: &Callback<String>, i: usize) -> Callback<MouseEvent> {
    let (src, onchange) = (src.to_string(), onchange.clone());
    Callback::from(move |_| {
        onchange.emit(edited(&src, |d| {
            if let Some(a) = d.as_array_mut() {
                if i < a.len() {
                    a.remove(i);
                }
            }
        }))
    })
}

/// Messages to roles: recipient, title and text, each editable.
pub fn communications(src: &str, roles: &[Value], disabled: bool, onchange: &Callback<String>) -> Html {
    let drafts = parse(src);
    if list(&drafts).is_empty() {
        return html! {};
    }
    html! {<div class="draft-group" data-communication-drafts="">
        <h4 class="action-review-title">{ex("Messaggi che partiranno")}</h4>
        {for list(&drafts).iter().enumerate().map(|(i, d)| html!{<article class="draft-card" data-communication-draft={i.to_string()}>
            <header><b>{format!("{} → {}", crate::render::role_label(roles, text(&d["from"])), role_name(roles, text(&d["to"])))}</b>
                <span class="pill">{if d["kind"] == "clarification" {ex("Chiarimento")} else {ex("Messaggio")}}</span><div class="grow"/>
                <button type="button" disabled={disabled} onclick={remove(src, onchange, i)}>{ex("Togli")}</button></header>
            <input aria-label={ex("Titolo")} placeholder={ex("Titolo")} value={text(&d["title"]).to_string()} disabled={disabled} oninput={on_text(src, onchange, i, "title")}/>
            <textarea aria-label={ex("Testo")} rows="3" value={text(&d["text_en"]).to_string()} disabled={disabled} oninput={on_text(src, onchange, i, "text_en")}/>
        </article>})}
    </div>}
}

/// Scenario injects: recipients (all or chosen roles), sender, title, text.
pub fn injects(src: &str, roles: &[Value], disabled: bool, onchange: &Callback<String>) -> Html {
    let drafts = parse(src);
    if list(&drafts).is_empty() {
        return html! {};
    }
    html! {<div class="draft-group" data-inject-drafts="">
        <h4 class="action-review-title">{ex("Inject che partiranno")}</h4>
        {for list(&drafts).iter().enumerate().map(|(i, d)| {
            let to: Vec<String> = list(&d["recipients"]).iter().map(|r| text(r).to_string()).collect();
            let all = to.iter().any(|r| r == "all");
            let toggle = |role: String| {
                let (src, onchange) = (src.to_string(), onchange.clone());
                Callback::from(move |_: Event| onchange.emit(edited(&src, |d| {
                    let mut to: Vec<String> = list(&d[i]["recipients"]).iter().map(|r| text(r).to_string()).filter(|r| r != "all").collect();
                    if role == "all" {
                        to = vec!["all".into()];
                    } else if let Some(k) = to.iter().position(|r| *r == role) {
                        to.remove(k);
                    } else {
                        to.push(role.clone());
                    }
                    d[i]["recipients"] = json!(to);
                })))
            };
            html!{<article class="draft-card" data-inject-draft={i.to_string()}>
                <header><b>{if text(&d["sender"]).is_empty() {ex("Inject").to_string()} else {text(&d["sender"]).to_string()}}</b><div class="grow"/>
                    <button type="button" disabled={disabled} onclick={remove(src, onchange, i)}>{ex("Togli")}</button></header>
                <div class="draft-recipients" role="group" aria-label={ex("Destinatari")}>
                    <label class="inline-check"><input type="checkbox" checked={all} disabled={disabled} onchange={toggle("all".into())}/>{" "}{ex("Tutti i player")}</label>
                    {for roles.iter().map(|r| {let id = text(&r["id"]).to_string(); html!{<label class="inline-check" key={id.clone()}><input type="checkbox" checked={!all && to.contains(&id)} disabled={disabled} onchange={toggle(id.clone())}/>{" "}{role_name(roles, &id)}</label>}})}
                </div>
                <input aria-label={ex("Mittente simulato")} placeholder={ex("Mittente simulato")} value={text(&d["sender"]).to_string()} disabled={disabled} oninput={on_text(src, onchange, i, "sender")}/>
                <input aria-label={ex("Titolo")} placeholder={ex("Titolo")} value={text(&d["title"]).to_string()} disabled={disabled} oninput={on_text(src, onchange, i, "title")}/>
                <textarea aria-label={ex("Testo")} rows="3" value={text(&d["text"]).to_string()} disabled={disabled} oninput={on_text(src, onchange, i, "text")}/>
            </article>}
        })}
    </div>}
}

fn phase_label(key: &str) -> String {
    match key {
        "early_warning" => ex("Allerta precoce").into(),
        "early_action" => ex("Azione precoce").into(),
        "escalation" => ex("Escalation").into(),
        "full_response" => ex("Risposta completa").into(),
        _ => key.into(),
    }
}

/// Resource limits: a cap per resource, overall and per exercise phase.
pub fn limits(src: &str, stat: &Value, disabled: bool, onchange: &Callback<String>) -> Html {
    let draft = parse(src);
    if !draft.is_object() {
        return html! {};
    }
    let name = |id: &str| list(&stat["resource_types"]).iter().find(|r| text(&r["id"]) == id).map(|r| text(&r["name"]).to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string());
    let field = |path: Vec<String>, value: &Value| {
        let (src, onchange) = (src.to_string(), onchange.clone());
        let label = path.last().cloned().unwrap_or_default();
        html! {<label class="draft-limit"><span>{name(&label)}</span><input type="number" min="0" value={value.to_string()} disabled={disabled}
            oninput={Callback::from(move |e| {
                let Ok(n) = api::input(e).trim().parse::<u32>() else { return };
                onchange.emit(edited(&src, |d| {
                    let mut at = d;
                    for k in &path { at = &mut at[k.as_str()]; }
                    *at = json!(n);
                }));
            })}/></label>}
    };
    let rows = |m: &Value, prefix: Vec<String>| html! {<div class="draft-limits">{for m.as_object().into_iter().flatten().map(|(k, v)| {
        let mut path = prefix.clone();
        path.push(k.clone());
        field(path, v)
    })}</div>};
    html! {<div class="draft-group" data-limit-draft="">
        <h4 class="action-review-title">{ex("Limiti di risorse proposti")}</h4>
        if draft["total"].as_object().is_some_and(|m| !m.is_empty()) {<p class="muted">{ex("Per tutta l’esercitazione")}</p>{rows(&draft["total"], vec!["total".into()])}}
        {for draft["phases"].as_object().into_iter().flatten().map(|(phase, m)| html!{<>
            <p class="muted">{crate::exfmt!("Fase: {}", "Phase: {}", phase_label(phase))}</p>{rows(m, vec!["phases".into(), phase.clone()])}
        </>})}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_the_json_the_decision_sends() {
        let src = api::pretty(&json!([{"to":"mayor","text_en":"a"},{"to":"police","text_en":"b"}]));
        let v: Value = serde_json::from_str(&edited(&src, |d| d[1]["text_en"] = json!("c"))).unwrap();
        assert_eq!(v, json!([{"to":"mayor","text_en":"a"},{"to":"police","text_en":"c"}]));
        let limits = api::pretty(&json!({"total":{"engines":4},"phases":{"escalation":{"engines":2}}}));
        let v: Value = serde_json::from_str(&edited(&limits, |d| d["phases"]["escalation"]["engines"] = json!(3))).unwrap();
        assert_eq!(v["phases"]["escalation"]["engines"], 3);
        assert_eq!(v["total"]["engines"], 4);
    }
}
