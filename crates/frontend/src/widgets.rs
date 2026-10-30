use crate::{
    api::{self, list, pretty, text},
    controllo::CommonProps,
    Action,
};
use serde_json::{json, Value};
use yew::prelude::*;

pub fn details(title: impl Into<AttrValue>, value: &Value) -> Html {
    crate::render::details(title, value)
}
pub fn participant_links(links: &Value, roles: &[Value]) -> Html {
    if cfg!(feature = "standalone") {
        return html! {<p>{crate::i18n::ex("Tutti i ruoli sono disponibili nelle schede di questa app. Ogni apertura del file avvia un’esercitazione indipendente.")}</p>};
    }
    html! {<div id="links">{for list(&links["players"]).iter().chain(list(&links["evaluators"])).map(|p|{
        let id=text(if p["role"].is_null(){&p["id"]}else{&p["role"]});
        let name=roles.iter().find(|r|r["id"]==id).map(|r|text(&r["name"])).unwrap_or(id);
        html!{<p><a href={text(&p["url"]).to_string()} target="_blank" rel="noopener noreferrer">{name}</a></p>}
    })}</div>}
}
pub fn stats(v: &Value) -> Html {
    html! {<p>{crate::exfmt!("Bruciato {:.1} ha · edifici danneggiati {} · distrutti {} · minacciati {} · evacuati {} · a rischio {} · nei rifugi {}","Burnt {:.1} ha · buildings damaged {} · destroyed {} · threatened {} · evacuated {} · at risk {} · in shelters {}",v["burned_ha"].as_f64().unwrap_or(0.),v["damaged"],v["destroyed"],v["threatened"],v["evacuated_people"],v["people_at_risk"],v["sheltered"])}</p>}
}
pub fn command_name(s: &str) -> &str {
    match s {
        "IssueWarning" => crate::i18n::ex("Avvisare la popolazione"),
        "EvacuateZone" => crate::i18n::ex("Evacuare una zona"),
        "EvacuateBuilding" => crate::i18n::ex("Evacuare un edificio"),
        "EvacuateDistrict" => crate::i18n::ex("Evacuare un borgo"),
        "OpenShelter" => crate::i18n::ex("Aprire un rifugio"),
        "CloseRoad" => crate::i18n::ex("Chiudere una strada"),
        "ReopenRoad" => crate::i18n::ex("Riaprire una strada"),
        "CutFireLine" => crate::i18n::ex("Creare una linea tagliafuoco"),
        "WaterDrop" => crate::i18n::ex("Effettuare un lancio d’acqua"),
        "DeployEngine" => crate::i18n::ex("Inviare un’autobotte"),
        "MonitorArea" => crate::i18n::ex("Inviare il drone di monitoraggio"),
        "Reconnoitre" => crate::i18n::ex("Inviare una squadra in ricognizione"),
        "RequestReinforcement" => crate::i18n::ex("Richiedere rinforzi"),
        "Ignite" => crate::i18n::ex("Innescare un incendio"),
        "Extinguish" => crate::i18n::ex("Spegnere un’area"),
        "ExtinguishArea" => crate::i18n::ex("Spegnere un’area disegnata"),
        "ReturnResource" => crate::i18n::ex("Restituire risorse"),
        "SetWeather" => crate::i18n::ex("Modificare il meteo"),
        "SetBuildingStatus" => crate::i18n::ex("Aggiornare lo stato di un edificio"),
        "SetDeploymentTravel" => crate::i18n::ex("Configurare trasferimento risorse"),
        "AdjustResource" => crate::i18n::ex("Modificare le risorse"),
        "AdvanceTime" => crate::i18n::ex("Avanzare il tempo"),
        _ => s,
    }
}
/// Player-language one-line summary of a serialized command, built from the
/// localized command and field names; no English server text is used.
pub fn command_summary(lang: &str, cmd: &Value) -> String {
    command_summary_in(lang, cmd, &Value::Null)
}

/// [`command_summary`] with places in words from the scenario (`stat`):
/// points as zone and nearby place, buildings `#id` by name.
pub fn command_summary_in(lang: &str, cmd: &Value, stat: &Value) -> String {
    let name = if lang == "it" { command_name(text(&cmd["type"])) } else { crate::i18n::action(lang, text(&cmd["type"])) };
    let params = command_params_in(lang, cmd, stat);
    if params.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {params}")
    }
}

/// The parameters part of [`command_summary_in`], without the command name.
pub fn command_params_in(lang: &str, cmd: &Value, stat: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(obj) = cmd.as_object() {
        for (key, value) in obj {
            if key == "type" || value.is_null() {
                continue;
            }
            parts.push(format!("{}: {}", crate::i18n::field(lang, key), value_label(lang, key, value, stat)));
        }
    }
    parts.join(" · ")
}

fn value_label(lang: &str, key: &str, value: &Value, stat: &Value) -> String {
    match value {
        Value::String(s) if key == "building" || key == "destination" => crate::place_pick::building_label(stat, s),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => crate::i18n::t(lang, if *b { "yes" } else { "no" }).into(),
        Value::Object(o) if o.contains_key("x") && o.contains_key("y") => {
            let (x, y) = (o["x"].as_f64().unwrap_or(0.), o["y"].as_f64().unwrap_or(0.));
            if stat.is_null() { format!("({x:.0}, {y:.0})") } else { crate::place_pick::point_label(stat, x, y) }
        }
        // A circular area: `{at, radius_m}`.
        Value::Object(o) if o.contains_key("at") => match o.get("radius_m").and_then(Value::as_f64) {
            Some(r) => format!("{} · r {r:.0} m", value_label(lang, "at", &o["at"], stat)),
            None => value_label(lang, "at", &o["at"], stat),
        },
        Value::Array(a) if a.iter().all(Value::is_string) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}
#[cfg(test)]
mod recheck_tests {
    use super::*;
    fn all_ok(v: &Option<Value>, n: usize) -> bool {
        kept_ok(v, n, &Excluded::new())
    }
    #[test]
    fn partial_approval_ignores_left_out_actions() {
        let v = Some(json!([{"verdict":"ok"},{"verdict":"rejected","reason":"no buses"}]));
        assert!(!kept_ok(&v, 2, &Excluded::new()));
        let mut out = Excluded::new();
        out.insert(1, "  not now ".into());
        assert!(kept_ok(&v, 2, &out));
        assert_eq!(rejections(&out), json!([{"index":1,"note":"not now"}]));
        out.insert(0, " ".into());
        assert_eq!(rejections(&out), json!([{"index":0},{"index":1,"note":"not now"}]));
        let cmds = vec![json!({"type":"A"}), json!({"type":"B"}), json!({"type":"C"})];
        let mut skip = Excluded::new();
        skip.insert(1, String::new());
        assert_eq!(kept(&cmds, &skip), vec![json!({"type":"A"}), json!({"type":"C"})]);
    }
    #[test]
    fn verdicts_follow_the_edited_commands() {
        let stored = json!([{"type":"WaterDrop","at":"far"}]);
        let edited = json!([{"type":"WaterDrop","at":"near"}]);
        let bad = json!([{"verdict":"rejected","reason":"too far"}]);
        let good = json!([{"verdict":"ok","notes":[]}]);
        let idle = Recheck::Idle;
        // unedited: stored (failing) verdicts apply and block approval
        let v = current_verdicts(&stored, &stored, &bad, &idle);
        assert!(!all_ok(&v, 1));
        // edited, nothing answered yet: stale verdicts must not be used
        assert!(current_verdicts(&edited, &stored, &bad, &idle).is_none());
        assert!(!all_ok(&None, 1));
        // server answered for the edit: approvable
        let done = Recheck::Done { commands: edited.clone(), proposal: json!({"verdicts": good}) };
        assert!(all_ok(&current_verdicts(&edited, &stored, &bad, &done), 1));
        // further typing makes that answer stale again
        let again = json!([{"type":"WaterDrop","at":"nearer"}]);
        assert!(current_verdicts(&again, &stored, &bad, &done).is_none());
    }
    #[test]
    fn note_survives_remount_until_cleared() {
        save_note(7, "motivo");
        assert_eq!(saved_note(7), "motivo");
        save_note(7, "");
        assert_eq!(saved_note(7), "");
    }
}
#[cfg(test)]
mod summary_tests {
    use super::*;
    #[test]
    fn player_actions_edit_with_the_player_form() {
        let warn = json!({"type":"IssueWarning","zone":"F4"});
        assert_eq!(editor_for(&warn, true), Some("player"));
        assert_eq!(editor_for(&warn, false), Some("warn"));
        assert_eq!(editor_for(&json!({"type":"DeployEngine","at":"Pineta"}), false), Some("player"));
        assert_eq!(editor_for(&json!({"type":"Ignite","at":{"x":1.0,"y":2.0}}), true), Some("ignite"));
        assert_eq!(editor_for(&json!({"type":"AdjustResource"}), false), None);
    }
    #[test]
    fn summary_names_places_with_the_scenario() {
        let stat = json!({"zones":{"cols":8,"rows":8,"size_m":750.0},"buildings":[{"id":7,"name":"Scuola","pos":[0.0,0.0]}],
            "places":[{"name":"Selvariva","pos":[3500.0,2280.0],"radius_m":500.0}]});
        let line = json!({"type":"CutFireLine","from":{"x":3550.0,"y":2300.0},"to":{"x":5800.0,"y":300.0}});
        assert_eq!(command_summary_in("it", &line, &stat), "Creare una linea tagliafuoco · Da: E5 (Selvariva) · A: H8");
        let shelter = json!({"type":"OpenShelter","building":"#7"});
        assert!(command_summary_in("it", &shelter, &stat).ends_with(": Scuola"));
        let drone = json!({"type":"MonitorArea","zones":["F3","F4"]});
        assert!(command_summary("it", &drone).ends_with(": F3, F4"));
    }
    #[test]
    fn summary_is_italian() {
        let s = command_summary("it", &json!({"type":"EvacuateBuilding","building":"Scuola Pineta","destination":null}));
        assert_eq!(s, "Evacuare un edificio · Edificio o struttura: Scuola Pineta");
    }
    #[test]
    fn summary_follows_language() {
        let c = json!({"type":"EvacuateBuilding","building":"Scuola Pineta","destination":null});
        assert_eq!(command_summary("de", &c), "Evakuierung eines Gebäudes · Gebäude oder Einrichtung: Scuola Pineta");
    }
}
pub fn field_name(s: &str) -> &str {
    match s {
        "zone" => crate::i18n::ex("Zona"),
        "destination" => crate::i18n::ex("Destinazione"),
        "building" => crate::i18n::ex("Edificio o struttura"),
        "road" => crate::i18n::ex("Strada"),
        "from" => crate::i18n::ex("Da"),
        "to" => crate::i18n::ex("A"),
        "width_m" => crate::i18n::ex("Larghezza (metri)"),
        "at" => crate::i18n::ex("Luogo"),
        "qty" => crate::i18n::ex("Quantità"),
        "resource" => crate::i18n::ex("Risorsa"),
        "radius_m" => crate::i18n::ex("Raggio (metri)"),
        "area" => crate::i18n::ex("Area"),
        "role" => crate::i18n::ex("Ruolo"),
        "minutes" => crate::i18n::ex("Minuti"),
        _ => s,
    }
}

thread_local! {
    /// EXCON note per proposal id, so closing/reopening the dialog (which
    /// remounts `Proposal`) keeps what was typed until the decision is sent.
    static NOTES: std::cell::RefCell<std::collections::BTreeMap<u64, String>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}
pub fn saved_note(id: u64) -> String {
    NOTES.with(|n| n.borrow().get(&id).cloned().unwrap_or_default())
}
pub fn save_note(id: u64, note: &str) {
    NOTES.with(|n| {
        if note.is_empty() {
            n.borrow_mut().remove(&id);
        } else {
            n.borrow_mut().insert(id, note.to_string());
        }
    });
}
/// Identity used for EXCON requests: the EXCON role in standalone, the stored access key otherwise.
fn excon_token() -> String {
    #[cfg(feature = "standalone")]
    return "master".into();
    #[cfg(not(feature = "standalone"))]
    api::token()
}
/// Outcome of re-checking the edited commands against the server.
#[derive(Clone, PartialEq, Debug)]
pub enum Recheck {
    /// Nothing sent yet for the current edit.
    Idle,
    Done { commands: Value, proposal: Value },
    Failed { commands: Value, error: String },
}
/// Verdicts that belong to exactly the commands currently shown, if any.
pub fn current_verdicts(local: &Value, stored_commands: &Value, stored: &Value, re: &Recheck) -> Option<Value> {
    match re {
        Recheck::Done { commands, proposal } if commands == local => Some(proposal["verdicts"].clone()),
        _ if local == stored_commands => Some(stored.clone()),
        _ => None,
    }
}
/// Actions the EXCON leaves out of a partial approval: index -> reason for the player.
pub type Excluded = std::collections::BTreeMap<usize, String>;
/// True when every command the EXCON keeps has an `ok` verdict (stale or missing verdicts block).
pub fn kept_ok(verdicts: &Option<Value>, commands: usize, excluded: &Excluded) -> bool {
    match verdicts {
        Some(v) => {
            list(v).len() >= commands
                && list(v).iter().enumerate().all(|(i, v)| excluded.contains_key(&i) || v["verdict"] == "ok")
        }
        None => false,
    }
}
/// The `rejected_actions` of a decision body (reasons trimmed, empty ones omitted).
pub fn rejections(excluded: &Excluded) -> Value {
    Value::Array(
        excluded
            .iter()
            .map(|(i, note)| {
                let note = note.trim();
                if note.is_empty() { json!({"index": i}) } else { json!({"index": i, "note": note}) }
            })
            .collect(),
    )
}
/// The commands an approval keeps.
pub fn kept(commands: &[Value], excluded: &Excluded) -> Vec<Value> {
    commands.iter().enumerate().filter(|(i, _)| !excluded.contains_key(i)).map(|(_, c)| c.clone()).collect()
}

#[derive(Properties, PartialEq)]
pub struct ProposalProps {
    pub proposal: Value,
    pub generation: Value,
    pub preview: Value,
    pub action: Callback<Action>,
    pub busy: bool,
    #[prop_or_default]
    pub roles: Vec<Value>,
    /// Console state: personas to speak as and the Controllo tool forms for outcome actions.
    #[prop_or_default]
    pub common: Option<CommonProps>,
}

/// Choice of the persona EXCON speaks as. `""` = the default (the inject's
/// sender, then the scenario's default persona); players never see "EXCON".
#[derive(Properties, PartialEq)]
pub struct PersonaProps {
    pub stat: Value,
    pub value: String,
    pub onchange: Callback<String>,
    #[prop_or_default]
    pub disabled: bool,
}
#[function_component(PersonaPicker)]
pub fn persona_picker(p: &PersonaProps) -> Html {
    let personas = &p.stat["personas"];
    let roles = list(&personas["roles"]).to_vec();
    let senders = list(&personas["senders"]).iter().map(|s| text(s).to_string()).collect::<Vec<_>>();
    let known = p.value.is_empty() || roles.iter().any(|r| text(&r["name"]) == p.value) || senders.contains(&p.value);
    let free = use_state(|| !known);
    let default_name = if crate::i18n_excon::is_en() { text(&personas["default_en"]) } else { text(&personas["default"]) };
    let onselect = {
        let (free, cb) = (free.clone(), p.onchange.clone());
        Callback::from(move |e: Event| {
            let v = api::select(e);
            free.set(v == "__free__");
            cb.emit(if v == "__free__" { String::new() } else { v });
        })
    };
    html! {<label class="persona-picker"><span>{crate::i18n::ex("Parla come")}</span>
        <select data-persona="" aria-label={crate::i18n::ex("Parla come")} disabled={p.disabled} onchange={onselect}>
            <option value="" selected={!*free&&p.value.is_empty()}>{crate::exfmt!("Predefinito (mittente dell’inject, altrimenti {})","Default (inject sender, otherwise {})",default_name)}</option>
            <optgroup label={crate::i18n::ex("Ruoli")}>{for roles.iter().map(|r|html!{<option value={text(&r["name"]).to_string()} selected={!*free&&text(&r["name"])==p.value}>{text(&r["name"])}</option>})}</optgroup>
            if !senders.is_empty(){<optgroup label={crate::i18n::ex("Mittenti simulati")}>{for senders.iter().map(|s|html!{<option value={s.clone()} selected={!*free&&*s==p.value}>{s.clone()}</option>})}</optgroup>}
            <option value="__free__" selected={*free}>{crate::i18n::ex("Testo libero…")}</option>
        </select>
        if *free {<input data-persona-free="" maxlength="80" aria-label={crate::i18n::ex("Nome del personaggio")} placeholder={crate::i18n::ex("Nome del personaggio")} value={p.value.clone()} disabled={p.disabled} oninput={{let cb=p.onchange.clone();Callback::from(move|e|cb.emit(api::input(e)))}}/>}
    </label>}
}
#[function_component(Proposal)]
pub fn proposal(props: &ProposalProps) -> Html {
    let p = &props.proposal;
    // A pure narrative initiative has no actions; a response may carry both.
    let has_narrative = p["narrative"] == true;
    let outcome_text = use_state(String::new);
    let persona = use_state(String::new);
    let staged = use_state(Vec::<Value>::new);
    let commands = use_state(|| list(&p["commands"]).to_vec());
    // A pure initiative until the EXCON adds actions to review (e.g. AI suggestions).
    let narrative = has_narrative && p["carries_actions"] != true && commands.is_empty();
    // AI action suggestions the EXCON asked for (full state, all conversations).
    let suggestions = use_state(|| Value::Null);
    let suggesting = use_state(|| false);
    // Last commands known from the server; local edits that differ from it are never overwritten.
    let synced = use_mut_ref(|| p["commands"].clone());
    {
        let commands = commands.clone();
        let synced = synced.clone();
        use_effect_with(p["commands"].clone(), move |next| {
            let mut known = synced.borrow_mut();
            if json!(*commands) == *known {
                commands.set(list(next).to_vec());
            }
            *known = next.clone();
        });
    }
    let recheck = use_state(|| Recheck::Idle);
    let editable = ["pending", "needs_clarification"].contains(&text(&p["status"]));
    // Partial approval: the actions the EXCON leaves out, with a reason each.
    let excluded = use_state(Excluded::new);
    // Actions whose parameters are open for editing.
    let editing = use_state(std::collections::BTreeSet::<usize>::new);
    // Action being edited with its Controllo tool (map or weather form) or the player's form.
    let tool_edit = use_state(|| Option::<usize>::None);
    {
        // Indices only hold for the list they were chosen in.
        let (excluded, editing, tool_edit) = (excluded.clone(), editing.clone(), tool_edit.clone());
        use_effect_with(commands.len(), move |_| {
            excluded.set(Excluded::new());
            editing.set(Default::default());
            tool_edit.set(None);
        });
    }
    {
        let recheck = recheck.clone();
        let proposal_id = p["id"].as_u64().unwrap_or(0);
        use_effect_with((json!(*commands), editable, p["commands"].clone()), move |(local, editable, stored)| {
            let local = local.clone();
            let stored = stored.clone();
            let timer = (*editable && local != stored && !local.as_array().is_some_and(Vec::is_empty)).then(|| {
                gloo_timers::callback::Timeout::new(500, move || {
                    wasm_bindgen_futures::spawn_local(async move {
                        let body = json!({"commands": local});
                        let result = api::request(&excon_token(), &format!("/api/master/proposals/{proposal_id}/revalidate"), Some(body)).await;
                        recheck.set(match result {
                            Ok(proposal) => Recheck::Done { commands: local, proposal },
                            Err(error) => Recheck::Failed { commands: local, error },
                        });
                    });
                })
            });
            move || drop(timer)
        });
    }
    let note = use_state(|| saved_note(p["id"].as_u64().unwrap_or(0)));
    let error = use_state(String::new);
    let comms = use_state(|| pretty(&p["communication_drafts"]));
    let injects = use_state(|| pretty(&p["inject_drafts"]));
    let limits = use_state(|| pretty(&p["resource_limit_draft"]));
    let id = p["id"].as_u64().unwrap_or(0);
    let local_commands = json!(*commands);
    let verdicts = current_verdicts(&local_commands, &p["commands"], &p["verdicts"], &recheck);
    let recheck_error = match &*recheck {
        Recheck::Failed { commands: c, error } if *c == local_commands => Some(error.clone()),
        _ => None,
    };
    let needs_preview = p["source"]["kind"] == "master" && !commands.is_empty();
    let needs_reason = p["source"]["kind"] == "player" && note.trim().is_empty();
    let kept_commands = kept(&commands, &excluded);
    let valid_preview = props.preview["generation"] == props.generation
        && props.preview["_request"]["commands"] == json!(kept_commands)
        && !props.preview.is_null();
    let clarify = {
        let action = props.action.clone();
        let note = note.clone();
        let persona = persona.clone();
        Callback::from(move |_| {
            save_note(id, "");
            action.emit(Action::post(
                format!("/api/master/proposals/{id}/clarify"),
                json!({"note": note.trim(), "persona": (!persona.trim().is_empty()).then(|| persona.trim().to_string())}),
            ));
            // The question is sent: an empty note keeps a second click from resending it.
            note.set(String::new());
        })
    };
    let decide = |approve: bool| {
        let action = props.action.clone();
        let commands = commands.clone();
        let note = note.clone();
        let error = error.clone();
        let p = p.clone();
        let comms = comms.clone();
        let injects = injects.clone();
        let limits = limits.clone();
        let generation = props.generation.clone();
        let persona = persona.clone();
        let excluded = excluded.clone();
        Callback::from(move |_| {
            let mut body = json!({"approve":approve,"commands":*commands,"note":*note});
            if !excluded.is_empty() {
                body["rejected_actions"] = rejections(&excluded);
            }
            if !persona.trim().is_empty() {
                body["persona"] = json!(persona.trim());
            }
            if approve {
                if needs_preview {
                    body["preview_generation"] = generation.clone();
                }
                for (key, src) in [
                    ("communication_drafts", &*comms),
                    ("inject_drafts", &*injects),
                    ("resource_limits", &*limits),
                ] {
                    let original = if key == "resource_limits" {
                        &p["resource_limit_draft"]
                    } else {
                        &p[key]
                    };
                    if original.is_null() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(src) {
                        Ok(v) => body[key] = v,
                        Err(e) => {
                            error.set(format!("{key}: {e}"));
                            return;
                        }
                    }
                }
            }
            save_note(id, "");
            action.emit(Action::post(
                format!("/api/master/proposals/{id}/decide"),
                body,
            ));
            note.set(String::new());
            excluded.set(Excluded::new());
        })
    };
    let outcome_stage = has_narrative && (p["status"]=="approved"||(!narrative&&(p["status"]=="applied"||p["status"]=="failed")));
    let can_suggest = has_narrative && props.common.is_some() && p["source"]["kind"]=="player" && (editable||outcome_stage);
    let suggest = {
        let suggestions = suggestions.clone();
        let suggesting = suggesting.clone();
        Callback::from(move |_| {
            let suggestions = suggestions.clone();
            let suggesting = suggesting.clone();
            suggesting.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::request(&excon_token(), &format!("/api/master/proposals/{id}/suggest-actions"), Some(json!({}))).await;
                suggestions.set(result.unwrap_or_else(|e| json!({"status":"ai_unavailable","actions":[],"failure":e})));
                suggesting.set(false);
            });
        })
    };
    // Suggestions go to the reviewed actions while the proposal is open, else to the outcome interventions.
    let add_suggested = {
        let commands = commands.clone();
        let staged = staged.clone();
        Callback::from(move |picked: Vec<Value>| {
            let target = if editable { &commands } else { &staged };
            let mut next = (**target).clone();
            next.extend(picked.into_iter().filter(|c| !target.contains(c)));
            target.set(next);
        })
    };
    let open_suggestions = list(&suggestions["actions"]).iter().filter(|a| !commands.contains(&a["command"]) && !staged.contains(&a["command"])).cloned().collect::<Vec<_>>();
    let carries_other=!list(&p["inject_drafts"]).is_empty()||!list(&p["communication_drafts"]).is_empty()||!p["resource_limit_draft"].is_null();
    // Nothing to execute: approving acknowledges the reply (the note is the answer).
    let no_actions=editable&&!narrative&&commands.is_empty()&&!carries_other;
    let all_excluded=editable&&!commands.is_empty()&&excluded.len()==commands.len()&&!carries_other&&!narrative;
    let blocked=editable&&(!kept_ok(&verdicts,commands.len(),&excluded)||all_excluded);
    let rejected_actions=list(&p["decision"]["rejected_actions"]).to_vec();
    let stat=props.common.as_ref().map(|c|c.stat.clone()).unwrap_or_default();
    let lang=if crate::i18n_excon::is_en(){"en"}else{"it"};
    let request=text(if text(&p["translation_en"]).is_empty(){&p["original"]}else{&p["translation_en"]}).to_string();
    // One line on what the decision buttons will do, most important first.
    let status_line:Option<(&str,String)>=if blocked{
        Some((if p["status"]=="needs_clarification"{"muted proposal-blocked"}else{"danger proposal-blocked"},(if all_excluded{crate::i18n::ex("Tutte le azioni sono escluse: per non approvare nulla usa Rifiuta.")}else if p["status"]=="needs_clarification"{crate::i18n::ex("In attesa del chiarimento del player: le azioni attuali non sarebbero approvabili (vedi verifica sopra).")}else{crate::i18n::ex("Non approvabile: escludi o correggi le azioni non valide (vedi verifica sopra) oppure rifiuta la proposta.")}).to_string()))
    }else if editable&&!excluded.is_empty(){
        Some(("muted proposal-partial-hint",crate::exfmt!("Approvazione parziale: {} di {} azioni approvate; il player riceve il motivo delle altre.","Partial approval: {} of {} actions approved; the player receives the reason for the others.",commands.len()-excluded.len(),commands.len())))
    }else if no_actions{
        Some(("muted proposal-ack-hint",crate::i18n::ex("Nessuna azione da eseguire: puoi prendere atto (la nota è la risposta al player), chiedere un chiarimento o rifiutare.").to_string()))
    }else{None};
    let from_player=p["source"]["kind"]=="player";
    let set_comms={let comms=comms.clone();Callback::from(move|v:String|comms.set(v))};
    let set_injects={let injects=injects.clone();Callback::from(move|v:String|injects.set(v))};
    let set_limits={let limits=limits.clone();Callback::from(move|v:String|limits.set(v))};
    let tool_modal=match (*tool_edit,&props.common){
        (Some(i),Some(common)) if i<commands.len()&&editable=>{
            let original=commands[i].clone();
            let onclose={let tool_edit=tool_edit.clone();Callback::from(move |_|tool_edit.set(None))};
            let common=CommonProps{onclose,..common.clone()};
            let replace={let (commands,tool_edit)=(commands.clone(),tool_edit.clone());Callback::from(move |edited:Value|{
                let mut next=(*commands).clone();
                if let Some(c)=next.get_mut(i){*c=crate::controllo::keep_extra_fields(c,&edited);}
                commands.set(next);tool_edit.set(None);
            })};
            let replace_all={let (commands,tool_edit)=(commands.clone(),tool_edit.clone());Callback::from(move |edited:Vec<Value>|{
                let mut next=(*commands).clone();
                if i<next.len()&&!edited.is_empty(){next.splice(i..=i,edited);}
                commands.set(next);tool_edit.set(None);
            })};
            match editor_for(&original,from_player){
                Some("player")=>html!{<crate::action_pick::ActionPickModal key={format!("{id}:{i}:player")} command={original.clone()} role={text(&p["source"]["role"]).to_string()} common={common.clone()} oncommands={replace_all}/>},
                Some("weather")=>{let w=&common.state["world"]["weather"];let f=|k:&str|original[k].as_f64().or(w[k].as_f64());
                    html!{<crate::controllo::WeatherModal title={crate::controllo::tool_label("weather")} common={common.clone()} dir={f("wind_dir_deg").unwrap_or(0.)} speed={f("wind_speed_kmh").unwrap_or(10.)} moisture={f("moisture_pct").unwrap_or(10.)} oncommand={Some(replace)}/>}}
                Some(tool)=>html!{<crate::controllo::MapToolModal key={format!("{id}:{i}:{tool}")} tool={tool} common={common.clone()} initial={Some(original.clone())} oncommand={Some(replace)}/>},
                None=>Html::default(),
            }
        }
        _=>Html::default(),
    };
    html! {<section class={classes!("proposal-inline",(p["status"]=="superseded").then_some("superseded"))} data-proposal={id.to_string()}>
        <div class="proposal-head"><b>{if narrative{crate::i18n::ex("Iniziativa narrativa")}else if has_narrative{crate::i18n::ex("Risposta: narrazione e azioni")}else{crate::i18n::ex("Azioni proposte")}}</b><span class={classes!("pill","pending",format!("review-{}",text(&p["review"])))} data-review={text(&p["review"]).to_string()}>{if p["status"]=="superseded"{crate::i18n::ex("sostituita")}else if p["status"]=="needs_clarification"&&p["clarification_request"].is_string(){crate::i18n::ex("in attesa di chiarimento")}else if narrative&&p["status"]=="approved"{crate::i18n::ex(if list(&p["narrative_outcomes"]).is_empty(){"Approvata · esito EXCON in attesa"}else{"Esito narrativo registrato"})}else{api::proposal_status(p)}}</span></div>
        if p["status"]=="superseded"{<p class="muted proposal-superseded" data-superseded="">{if p["superseded_by"].is_number(){crate::exfmt!("Sostituita dalla proposta #{}","Superseded by proposal #{}",p["superseded_by"])}else{crate::i18n::ex("sostituita").to_string()}}</p>}
        if p["status"]=="needs_clarification"&&p["clarification_request"].is_string(){<p class="proposal-clarification" data-clarification-request="">{format!("{}: {}",crate::i18n::ex("Domanda inviata al player"),text(&p["clarification_request"]))}</p>}
        <blockquote class="proposal-request">{request.clone()}</blockquote>
        if text(&p["summary"])!=request.as_str()&&!text(&p["summary"]).is_empty(){<p class="proposal-summary"><span class="muted">{crate::i18n::ex("Interpretazione")}{": "}</span>{text(&p["summary"])}</p>}
        if !text(&p["translation_en"]).is_empty()&&text(&p["original"])!=text(&p["translation_en"]){<details class="proposal-original-box"><summary>{crate::i18n::ex("Testo originale del player")}</summary><p class="muted proposal-original" data-original="" lang={text(&p["language"]).to_string()}>{text(&p["original"])}</p></details>}
        if !list(&p["thread"]).is_empty() {<details class="proposal-thread"><summary>{format!("{} · {}",crate::i18n::ex("Thread collegato"),crate::i18n_excon::count_messages(list(&p["thread"]).len()))}</summary><ol>{for list(&p["thread"]).iter().map(|m|html!{<li data-thread-message={m["id"].to_string()} key={m["id"].to_string()}><small>{format!("{} · {} → {} · {} {}",text(&m["clock"]),crate::render::role_label(&props.roles,text(&m["from"])),crate::render::role_label(&props.roles,text(&m["to"])),crate::i18n::ex("Turno"),m["turn"])}</small>if !text(&m["title"]).is_empty(){<b>{text(&m["title"])}</b>}<p>{text(&m["text"])}</p>if m["reply_to"].is_number(){<small>{crate::exfmt!("In risposta al messaggio #{}", "In reply to message #{}",m["reply_to"])}</small>}</li>})}</ol></details>}
        if p["clarification"].is_string(){<p class="proposal-clarification">{text(&p["clarification"])}</p>}
        if has_narrative {
            <p class="muted" data-narrative-proposal="">{if narrative{crate::i18n::ex("Iniziativa narrativa: approvazione ed esito sono decisi da EXCON. Non modifica la simulazione.")}else{crate::i18n::ex("Risposta con narrazione e azioni: le azioni seguono approvazione ed esecuzione; l’esito narrativo non modifica la simulazione.")}}</p>
            {for list(&p["narrative_outcomes"]).iter().map(|o|html!{<div class="narrative-outcome"><p><b>{crate::i18n::ex("Esito narrativo EXCON")}</b>{" · "}<span class="muted">{if o["persona"].is_string(){text(&o["persona"]).to_string()}else{crate::i18n::ex("persona predefinita").to_string()}}</span>{": "}{text(&o["text"])}</p>
                if !list(&o["interventions"]).is_empty(){<ul class="outcome-interventions">{for list(&o["interventions"]).iter().map(|x|html!{<li data-outcome-intervention={x["log_seq"].to_string()}>{crate::exfmt!("Intervento EXCON #{}: {}","EXCON intervention #{}: {}",x["log_seq"],text(&x["description"]))}</li>})}</ul>}</div>})}
            if p["status"]=="approved"||(!narrative&&(p["status"]=="applied"||p["status"]=="failed")) {
                <label for={format!("narrative-outcome-{id}")}>{crate::i18n::ex("Esito narrativo EXCON (facoltativo se aggiungi azioni)")}</label>
                <textarea id={format!("narrative-outcome-{id}")} data-narrative-outcome="" maxlength="8000" value={(*outcome_text).clone()} oninput={{let v=outcome_text.clone();Callback::from(move|e|v.set(api::input(e)))}}/>
                if let Some(common)=&props.common {
                    <PersonaPicker stat={common.stat.clone()} value={(*persona).clone()} onchange={{let persona=persona.clone();Callback::from(move|v|persona.set(v))}} disabled={props.busy}/>
                    <crate::staged_commands::StagedCommands commands={(*staged).clone()} onchange={{let staged=staged.clone();Callback::from(move|v|staged.set(v))}} common={common.clone()} disabled={props.busy}/>
                }
                <button data-record-narrative-outcome="" disabled={props.busy||(outcome_text.trim().is_empty()&&staged.is_empty())} onclick={{let cb=props.action.clone();let v=outcome_text.clone();let persona=persona.clone();let staged=staged.clone();Callback::from(move |_|{
                    let mut body=json!({"text":v.trim(),"language":if crate::i18n_excon::is_en(){"en"}else{"it"},"commands":*staged});
                    if !persona.trim().is_empty(){body["persona"]=json!(persona.trim());}
                    cb.emit(Action::post(format!("/api/master/proposals/{id}/narrative-outcome"),body));
                    // Sent once: the interventions must not be applied twice.
                    v.set(String::new());staged.set(Vec::new());
                })}}>{crate::i18n::ex("Registra esito narrativo")}</button>
            }
        }
        if can_suggest {<div class="excon-suggestions" data-excon-suggestions={text(&suggestions["status"]).to_string()}>
            <button type="button" data-suggest-actions={id.to_string()} disabled={props.busy||*suggesting} title={crate::i18n::ex("L’AI legge lo stato completo e tutte le conversazioni collegate e propone azioni del ruolo da rivedere.")} onclick={suggest}>{if *suggesting{crate::i18n::ex("Suggerimento in corso…")}else{crate::i18n::ex("Suggerisci azioni")}}</button>
            if suggestions["status"]=="ai_unavailable"{<p class="muted">{crate::i18n::ex("Suggerimenti AI non disponibili.")}if !text(&suggestions["failure"]).is_empty(){<small>{format!(" {}",text(&suggestions["failure"]))}</small>}</p>}
            else if !suggestions.is_null()&&open_suggestions.is_empty(){<p class="muted">{crate::i18n::ex("Nessuna nuova azione suggerita.")}</p>}
            if !open_suggestions.is_empty(){
                if !text(&suggestions["summary"]).is_empty(){<p class="muted">{text(&suggestions["summary"])}</p>}
                <p class="muted">{if editable{crate::i18n::ex("Bozze AI: aggiungile alle azioni da approvare, poi rivedile e approva.")}else{crate::i18n::ex("Bozze AI: aggiungile agli interventi dell’esito narrativo, poi registralo.")}}</p>
                <ol class="suggested-actions">{for open_suggestions.iter().enumerate().map(|(i,a)|{let cmd=a["command"].clone();let add=add_suggested.clone();html!{<li key={i} data-excon-suggested-action={i.to_string()}>
                    <span>{command_summary(if crate::i18n_excon::is_en(){"en"}else{"it"},&cmd)}</span>
                    if !text(&a["reason"]).is_empty(){<small class="muted">{format!(" · {}",text(&a["reason"]))}</small>}
                    if a["verdict"]["verdict"]=="rejected"{<small class="danger">{format!(" · {}",text(&a["verdict"]["reason"]))}</small>}
                    <button type="button" data-add-excon-suggestion={i.to_string()} disabled={props.busy} onclick={Callback::from(move |_|add.emit(vec![cmd.clone()]))}>{crate::i18n::ex("Aggiungi")}</button>
                </li>}})}</ol>
                if open_suggestions.len()>1{<button type="button" data-add-excon-suggestions="" disabled={props.busy} onclick={{let add=add_suggested.clone();let all=open_suggestions.iter().map(|a|a["command"].clone()).collect::<Vec<_>>();Callback::from(move |_|add.emit(all.clone()))}}>{crate::i18n::ex("Aggiungi tutte")}</button>}
            }
        </div>}
        <div class="action-review" data-action-review="">
        if !commands.is_empty(){<h4 class="action-review-title">{if editable&&!narrative{crate::i18n::ex("Cosa succede se approvi")}else{crate::i18n::ex("Azioni")}}</h4>}
        <ol class="command-list">{for commands.iter().enumerate().map(|(i,cmd)|{
            let left_out=excluded.get(&i).cloned();
            let toggle={let excluded=excluded.clone();Callback::from(move |e:Event|{use yew::TargetCast;let on=e.target_unchecked_into::<web_sys::HtmlInputElement>().checked();let mut next=(*excluded).clone();if on{next.remove(&i);}else{next.insert(i,String::new());}excluded.set(next);})};
            let reason={let excluded=excluded.clone();Callback::from(move |e|{let mut next=(*excluded).clone();next.insert(i,api::input(e));excluded.set(next);})};
            let open=editing.contains(&i);
            // The Controllo tool when one builds this command, else the parameter fields.
            let with_tool=props.common.is_some()&&editor_for(cmd,from_player).is_some();
            let edit={let (editing,tool_edit)=(editing.clone(),tool_edit.clone());Callback::from(move |_|{if with_tool{tool_edit.set(Some(i));return;}let mut next=(*editing).clone();if !next.remove(&i){next.insert(i);}editing.set(next);})};
            let params=command_params_in(lang,cmd,&stat);
            html!{<li class={classes!("command-card",left_out.is_some().then_some("excluded"))} data-command-card={i.to_string()}>
            <div class="action-row">
                if editable&&!narrative{<input type="checkbox" data-approve-action={i.to_string()} aria-label={crate::i18n::ex("Approva questa azione")} title={crate::i18n::ex("Approva questa azione")} checked={left_out.is_none()} disabled={props.busy} onchange={toggle}/>}
                <span class="action-text"><b>{command_name(text(&cmd["type"]))}</b>if !params.is_empty(){<span class="action-params">{params}</span>}
                {match &verdicts {
                    None=>html!{<small class="verdict-checking" data-verdict-checking="">{if recheck_error.is_some(){crate::i18n::ex("Verifica non riuscita")}else{crate::i18n::ex("Verifica in corso…")}}</small>},
                    Some(v) if v[i]["verdict"]=="ok"=>html!{<small class="verdict-ok">{format!("✓ {}",if list(&v[i]["notes"]).is_empty(){crate::i18n::ex("azione valida").into()}else{list(&v[i]["notes"]).iter().map(|n|crate::render::localize_note(text(n))).collect::<Vec<_>>().join(" · ")})}</small>},
                    Some(v)=>html!{<small class="danger verdict-bad">{format!("⚠ {}",text(&v[i]["reason"]))}</small>},
                }}</span>
                if editable{<button type="button" class="action-edit" data-edit-action={i.to_string()} data-edit-tool={with_tool.then(||editor_for(cmd,from_player).unwrap_or_default())} aria-expanded={(!with_tool).then(||open.to_string())} title={with_tool.then_some(crate::i18n::ex("Modifica sulla mappa"))} disabled={props.busy} onclick={edit}>{if open{crate::i18n::ex("Chiudi")}else{crate::i18n::ex("Modifica")}}</button>}
            </div>
            if let Some(r)=left_out.clone(){<input class="reject-reason" data-reject-reason={i.to_string()} maxlength="500" aria-label={crate::i18n::ex("Motivo per il player (facoltativo)")} placeholder={crate::i18n::ex("Motivo per il player (facoltativo)")} value={r} disabled={props.busy} oninput={reason}/>}
            if open{<div class="action-fields">{for cmd.as_object().into_iter().flatten().filter(|(k,_)|k.as_str()!="type").map(|(key,value)|{
                let key=key.clone();let label_key=key.clone();let original=value.clone();let commands=commands.clone();let error=error.clone();
                let display=if value.is_string(){text(value).into()}else{value.to_string()};
                let onchange=Callback::from(move |e:Event|{use yew::TargetCast;let value=e.target_unchecked_into::<web_sys::HtmlInputElement>().value();
                    let parsed=if original.is_string(){Ok(json!(value))}else{serde_json::from_str::<Value>(&value).or_else(|err|if original.is_object()&&!value.trim().starts_with('{'){Ok(json!(value))}else{Err(err)})};
                    match parsed {Ok(value)=>{let mut next=(*commands).clone();next[i][&key]=value;commands.set(next);error.set(String::new());},Err(e)=>error.set(crate::exfmt!("Parametro non valido: {e}","Invalid parameter: {e}"))}
                });
                html!{<label class="command-field"><span>{field_name(&label_key)}</span><input aria-label={field_name(&label_key).to_string()} data-command={i.to_string()} data-field={label_key.clone()} value={display} {onchange} disabled={!editable||props.busy}/></label>}
            })}</div>}
            </li>}
        })}</ol>
        if needs_preview {<button disabled={props.busy||!editable||kept_commands.is_empty()} onclick={{let cb=props.action.clone();let commands=kept_commands.clone();Callback::from(move |_|cb.emit(Action::store("/api/master/preview",Some(json!({"commands":commands})),format!("proposal:{id}"))))}}>{crate::i18n::ex("Esamina anteprima fisica")}</button>
            if !props.preview.is_null(){{preview(&props.preview)}}
        }
        </div>
        if !p["invalid"].is_null()&&!list(&p["invalid"]).is_empty(){<p class="danger proposal-invalid">{crate::exfmt!("{} azioni non interpretabili: vedi i dettagli tecnici.","{} actions could not be read: see the technical details.",list(&p["invalid"]).len())}</p>}
        if !rejected_actions.is_empty(){<div class="rejected-actions" data-rejected-actions=""><b>{crate::i18n::ex("Azioni non approvate")}</b><ul>{for rejected_actions.iter().map(|r|html!{<li data-rejected-action={r["index"].to_string()}><span>{command_summary_in(lang,&r["command"],&stat)}</span>if !text(&r["note"]).is_empty(){<small class="muted">{format!(" · {}: {}",crate::i18n::ex("Motivo"),text(&r["note"]))}</small>}</li>})}</ul></div>}
        if editable&&!list(&p["communication_drafts"]).is_empty(){{crate::proposal_drafts::communications(&comms,&props.roles,props.busy,&set_comms)}}
        if editable&&!list(&p["inject_drafts"]).is_empty(){{crate::proposal_drafts::injects(&injects,&props.roles,props.busy,&set_injects)}}
        if editable&&!p["resource_limit_draft"].is_null(){{crate::proposal_drafts::limits(&limits,&stat,props.busy,&set_limits)}}
        if !narrative||editable {<footer class="proposal-decision">
        if (!narrative||editable)&&props.common.is_some() {<PersonaPicker stat={props.common.as_ref().map(|c|c.stat.clone()).unwrap_or_default()} value={(*persona).clone()} onchange={{let persona=persona.clone();Callback::from(move|v|persona.set(v))}} disabled={props.busy||!editable}/>}
        <label class="proposal-note"><span>{crate::i18n::ex("Nota per il player (richiesta per rifiutare o chiedere un chiarimento)")}</span><input data-note="" aria-label={crate::i18n::ex("Nota EXCON")} value={(*note).clone()} oninput={{let note=note.clone();Callback::from(move |e|{let v=api::input(e);save_note(id,&v);note.set(v)})}}/></label>
        if !error.is_empty(){<p class="error">{&*error}</p>}
        if let Some(e)=&recheck_error{<p class="danger proposal-recheck-error" role="status">{format!("{}: {}",crate::i18n::ex("Verifica non riuscita"),e)}</p>}
        if let Some((class,msg))=status_line.clone(){<p {class} role="status">{msg}</p>}
        <div class="row proposal-actions"><button data-approve="" class="primary" disabled={props.busy||!editable||blocked||!error.is_empty()||(needs_preview&&!kept_commands.is_empty()&&!valid_preview)} onclick={decide(true)}>{if narrative{crate::i18n::ex("Approva iniziativa narrativa").to_string()}else if no_actions{crate::i18n::ex("Prendi atto").to_string()}else if !excluded.is_empty(){crate::exfmt!("Approva {} di {} azioni","Approve {} of {} actions",commands.len()-excluded.len(),commands.len())}else if commands.len()>1{crate::exfmt!("Approva {} azioni","Approve {} actions",commands.len())}else{crate::i18n::ex("Approva azione").to_string()}}</button><button data-reject="" disabled={props.busy||!editable||needs_reason} title={needs_reason.then_some(crate::i18n::ex("Per rifiutare o chiedere un chiarimento scrivi una nota per il player."))} onclick={decide(false)}>{crate::i18n::ex("Rifiuta")}</button>
        if p["source"]["kind"]=="player"{<button data-clarify="" disabled={props.busy||!editable||note.trim().is_empty()} title={note.trim().is_empty().then_some(crate::i18n::ex("Spiega cosa chiarire al player"))} onclick={clarify}>{crate::i18n::ex("Chiedi chiarimento")}</button>}
        if p["status"]=="approved"&&!narrative{<button disabled={props.busy} data-execute={id.to_string()} onclick={{let cb=props.action.clone();Callback::from(move |_|cb.emit(Action::post(format!("/api/master/proposals/{id}/execute"),json!({}))))}}>{crate::i18n::ex("Esegui ora ordine approvato")}</button>}
        </div></footer>}
        <details class="proposal-tech" data-proposal-tech=""><summary>{crate::i18n::ex("Dettagli tecnici")}</summary>
            if editable&&!narrative{<details class="manual-commands"><summary>{crate::i18n::ex("Modifica o aggiungi comandi JSON (interpretazione manuale EXCON)")}</summary><textarea data-manual-commands="" aria-label={crate::i18n::ex("Comandi manuali EXCON")} value={pretty(&json!(*commands))} oninput={{let commands=commands.clone();let error=error.clone();Callback::from(move |e|match serde_json::from_str::<Vec<Value>>(&api::input(e)){Ok(next)=>{commands.set(next);error.set(String::new());},Err(e)=>error.set(crate::exfmt!("Comandi non validi: {e}","Invalid commands: {e}"))})}}/></details>}
            if !p["invalid"].is_null()&&!list(&p["invalid"]).is_empty(){<div class="danger">{crate::render::render_value("",&p["invalid"])}</div>}
            if editable&&!list(&p["communication_drafts"]).is_empty(){<label>{crate::i18n::ex("Comunicazioni da revisionare")}<textarea value={(*comms).clone()} oninput={{let comms=comms.clone();Callback::from(move|e|comms.set(api::input(e)))}}/></label>}
            if editable&&!list(&p["inject_drafts"]).is_empty(){<label>{crate::i18n::ex("Inject proposti")}<textarea value={(*injects).clone()} oninput={{let injects=injects.clone();Callback::from(move|e|injects.set(api::input(e)))}}/></label>}
            if editable&&!p["resource_limit_draft"].is_null(){<label>{crate::i18n::ex("Limiti di risorse proposti")}<textarea value={(*limits).clone()} oninput={{let limits=limits.clone();Callback::from(move|e|limits.set(api::input(e)))}}/></label>}
            {details(crate::i18n::ex("Cronologia ed esiti effettivi"),&json!({"history":p["history"],"executions":p["executions"]}))}
        </details>
        {tool_modal}
    </section>}
}
/// How an action is edited: a player's action with the player's own form
/// (role menu, zones, places, edges), else the Controllo tool that builds it
/// (map or weather); `None` keeps the parameter fields.
pub fn editor_for(cmd: &Value, from_player: bool) -> Option<&'static str> {
    let player = crate::action_pick::PLAYER_ACTIONS.contains(&text(&cmd["type"]));
    if from_player && player {
        return Some("player");
    }
    crate::controllo::tool_for(cmd).or_else(|| player.then_some("player"))
}
pub fn preview(value: &Value) -> Html {
    html! {<div class="preview"><h3>{crate::i18n::ex("Effetto immediato previsto")}</h3>
    <p>{crate::exfmt!("Celle in fiamme {} → {} · bruciato {:.2} → {:.2} ha","Burning cells {} → {} · burnt {:.2} → {:.2} ha",value["before"]["burning_cells"],value["after"]["burning_cells"],value["before"]["burned_ha"].as_f64().unwrap_or(0.),value["after"]["burned_ha"].as_f64().unwrap_or(0.))}</p>
    {for list(&value["results"]).iter().map(|r|{let g=&r["geometry"];html!{<div class="card"><b>{command_name(text(&r["command"]["type"]))}</b>
        if g["centre_m"].is_array(){<p>{crate::exfmt!("Centro: {}, {} m est/nord","Centre: {}, {} m east/north",g["centre_m"][0],g["centre_m"][1])}</p>}
        if let Some(radius)=g["radius_m"].as_f64(){<p>{crate::exfmt!("Raggio: {radius:.0} m","Radius: {radius:.0} m")}</p>}
        if g["bounds_m"].is_array(){<p>{crate::exfmt!("Limiti (metri): {}","Bounds (metres): {}",list(&g["bounds_m"]).iter().map(Value::to_string).collect::<Vec<_>>().join(", "))}</p>}
        if let Some(length)=g["length_m"].as_f64(){<p>{crate::exfmt!("Lunghezza: {length:.0} m · larghezza: {} m","Length: {length:.0} m · width: {} m",g["width_m"])}</p>}
        if let Some(area)=g["nominal_area_ha"].as_f64(){<p>{crate::exfmt!("Area nominale: {area:.2} ha","Nominal area: {area:.2} ha")}</p>}
        <p class={if r["result"]["Err"].is_null(){""}else{"danger"}}>{if let Some(err)=r["result"]["Err"].as_str(){err.to_string()}else{list(&r["result"]["Ok"]).iter().map(text).collect::<Vec<_>>().join(" · ")}}</p><p>{text(&g["effect"])}</p>
    </div>}})}<p class="muted">{text(&value["warning"])}</p>
    {details(crate::i18n::ex("Dati completi anteprima"),value)}</div>}
}

#[derive(Properties, PartialEq)]
pub struct ApiFormProps {
    pub title: AttrValue,
    pub endpoint: AttrValue,
    pub initial: Value,
    pub action: Callback<Action>,
    #[prop_or_default]
    pub busy: bool,
    #[prop_or_default]
    pub store: Option<String>,
    #[prop_or(crate::i18n::ex("Invia per revisione / applica").into())]
    pub button: AttrValue,
}
#[function_component(ApiForm)]
pub fn api_form(props: &ApiFormProps) -> Html {
    let values = use_state(|| form_values(&props.initial));
    {
        let values = values.clone();
        use_effect_with(props.initial.clone(), move |initial| {
            values.set(form_values(initial))
        });
    }
    let error = use_state(String::new);
    let onsubmit = {
        let values = values.clone();
        let initial = props.initial.clone();
        let endpoint = props.endpoint.to_string();
        let cb = props.action.clone();
        let store = props.store.clone();
        let error = error.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let mut body = json!({});
            for (key, value) in values.iter() {
                let original = &initial[key];
                let parsed = if original.is_string() {
                    json!(value)
                } else {
                    match serde_json::from_str(value) {
                        Ok(v) => v,
                        Err(e) => {
                            error.set(format!("{key}: {e}"));
                            return;
                        }
                    }
                };
                body[key] = parsed;
            }
            error.set(String::new());
            cb.emit(if let Some(key) = &store {
                Action::store(&endpoint, Some(body), key)
            } else {
                Action::post(&endpoint, body)
            });
        })
    };
    html! {<section class="card"><h2>{props.title.clone()}</h2><form {onsubmit}>{for props.initial.as_object().into_iter().flatten().map(|(key,initial)|{
        let key=key.clone();let val=values.get(&key).cloned().unwrap_or_default();let onchange={let key=key.clone();let values=values.clone();Callback::from(move|e|{let mut next=(*values).clone();next.insert(key.clone(),api::input(e));values.set(next);})};
        html!{<label>{field_name(&key)} if initial.is_boolean(){<input type="checkbox" checked={val=="true"} onchange={{let key=key.clone();let values=values.clone();Callback::from(move |e:Event|{use yew::TargetCast;let mut next=(*values).clone();next.insert(key.clone(),e.target_unchecked_into::<web_sys::HtmlInputElement>().checked().to_string());values.set(next);})}}/>}else if initial.is_array()||initial.is_object()||val.contains('\n'){<textarea value={val} oninput={onchange}/>}else{<input type={if initial.is_number(){"number"}else{"text"}} step="any" value={val} oninput={onchange}/>}</label>}
    })}<p class="error">{&*error}</p><button type="submit" class="primary" disabled={props.busy}>{props.button.clone()}</button></form></section>}
}

fn form_values(initial: &Value) -> std::collections::BTreeMap<String, String> {
    initial
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            (
                k.clone(),
                if v.is_string() {
                    text(v).to_string()
                } else {
                    pretty(v)
                },
            )
        })
        .collect()
}
