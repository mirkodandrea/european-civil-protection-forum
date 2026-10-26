use crate::api::{self, list, text};
use serde_json::{json, Value};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct HistoryProps {
    pub master: Value,
    #[prop_or_default]
    pub roles: Vec<Value>,
}

#[function_component(History)]
pub fn history(props: &HistoryProps) -> Html {
    let query = use_state(String::new);
    let page = use_state(|| 0usize);
    let error = use_state(String::new);
    let events = list(&props.master["log"]);
    let needle = query.trim().to_lowercase();
    let filtered = events
        .iter()
        .rev()
        .filter(|e| needle.is_empty() || e.to_string().to_lowercase().contains(&needle))
        .collect::<Vec<_>>();
    let last_page = filtered.len().saturating_sub(1) / 50;
    let current = (*page).min(last_page);
    let export = {
        let events = json!({"schema":"ttx-operational-history-v1","events":events});
        let error = error.clone();
        Callback::from(move |_| {
            if let Err(e) = api::download_as(&events, "ttx-cronologia.json") {
                error.set(e)
            }
        })
    };
    html! {<main class="page" id="system-history"><section class="card">
        <div class="row"><h2>{crate::i18n::ex("Cronologia")}</h2><span class="muted">{crate::exfmt!("{} eventi salvati · base per la valutazione","{} events saved · basis for the evaluation",events.len())}</span><div class="grow"/><button onclick={export}>{crate::i18n::ex("Esporta cronologia JSON")}</button></div>
        <div class="row"><label for="history-query">{crate::i18n::ex("Cerca")}</label><input id="history-query" type="search" placeholder={crate::i18n::ex("Turno, autore, evento o contenuto")} value={(*query).clone()} oninput={{let query=query.clone();let page=page.clone();Callback::from(move |e|{query.set(api::input(e));page.set(0);})}}/><span class="muted">{crate::exfmt!("{} risultati · pagina {} / {}","{} results · page {} / {}",filtered.len(),current+1,last_page+1)}</span><button disabled={current==0} onclick={{let page=page.clone();Callback::from(move |_|page.set(current.saturating_sub(1)))}}>{crate::i18n::ex("Precedenti")}</button><button disabled={current==last_page} onclick={{let page=page.clone();Callback::from(move |_|page.set(current+1))}}>{crate::i18n::ex("Successivi")}</button></div>
        if !error.is_empty(){<p class="error">{&*error}</p>}
        <div class="history-table"><table><thead><tr><th>{"#"}</th><th>{crate::i18n::ex("Turno / ora")}</th><th>{crate::i18n::ex("Autore")}</th><th>{crate::i18n::ex("Evento e dati salvati")}</th></tr></thead><tbody>{for filtered.iter().skip(current*50).take(50).map(|e|html!{<tr key={e["seq"].to_string()}><td>{e["seq"].to_string()}</td><td>{format!("{} · {}",e["turn"],text(&e["clock"]))}</td><td>{crate::render::role_label(&props.roles,text(&e["actor"]))}</td><td><details><summary>{crate::render::event_label(text(&e["kind"]))}{action_note(&props.master,e)}</summary>{crate::render::render_with_raw("",&e["data"])}</details></td></tr>})}</tbody></table></div>
        if filtered.is_empty(){<p class="muted">{crate::i18n::ex("Nessun evento corrispondente.")}</p>}
    </section></main>}
}

/// Readable title for action events: scripted actions by timeline id,
/// approved player actions by their executed command's outcome.
fn action_note(master: &Value, e: &Value) -> String {
    match text(&e["kind"]) {
        "timeline_action_decided" | "timeline_action_executed" => {
            let id = text(&e["data"]["action"]);
            let title = list(&master["timeline_actions"])
                .iter()
                .find(|a| text(&a["id"]) == id)
                .map(|a| text(&a["title"]).to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| id.to_string());
            format!(" · {title}")
        }
        "action_executed" => {
            let x = &e["data"]["execution"];
            let ok = if x["ok"] == true { "✓" } else { "✗" };
            format!(" · {ok} {}", text(&x["detail"]))
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_notes() {
        let m = json!({"timeline_actions":[{"id":"a1","title":"Ignition"}]});
        let e = json!({"kind":"timeline_action_executed","data":{"action":"a1"}});
        assert_eq!(action_note(&m, &e), " · Ignition");
        let e = json!({"kind":"action_executed","data":{"execution":{"ok":false,"detail":"no route"}}});
        assert_eq!(action_note(&m, &e), " · ✗ no route");
        assert_eq!(action_note(&m, &json!({"kind":"message"})), "");
    }
}
