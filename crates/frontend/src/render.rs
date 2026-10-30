//! Readable presentation of structured data (commands, events, forecasts,
//! statistics...). Each known shape formats itself; anything else goes through
//! a generic tidy renderer. Raw JSON is only offered in a collapsed "dati grezzi".
use crate::exfmt;
use serde_json::Value;
use yew::prelude::*;

const MAX_DEPTH: usize = 6;

// ---------------------------------------------------------------- pure helpers

/// Human label for an exercise log event kind; unknown kinds are humanised.
pub fn event_label(kind: &str) -> String {
    let known = match kind {
        "proposal_declared" => crate::i18n::ex("Proposta dichiarata"),
        "proposal_decided" => crate::i18n::ex("Proposta decisa dall’EXCON"),
        "proposal_parsed" => crate::i18n::ex("Proposta interpretata"),
        "proposal_parse_failed" => crate::i18n::ex("Interpretazione non riuscita"),
        "proposal_status" => crate::i18n::ex("Stato proposta aggiornato"),
        "master_command" => crate::i18n::ex("Comando EXCON"),
        "excon_action_queued" => crate::i18n::ex("Azione EXCON in coda"),
        "excon_action_cancelled" => crate::i18n::ex("Azione EXCON annullata"),
        "excon_action_edited" => crate::i18n::ex("Azione EXCON modificata"),
        "master_prompt" => crate::i18n::ex("Richiesta EXCON all’AI"),
        "inject_scheduled" => crate::i18n::ex("Inject programmato"),
        "inject_sent" => crate::i18n::ex("Inject inviato"),
        "inject_schedule_cancelled" => crate::i18n::ex("Programmazione inject annullata"),
        "scheduled_inject_triggered" => crate::i18n::ex("Inject programmato attivato"),
        "scheduled_inject_completed" => crate::i18n::ex("Inject programmato completato"),
        "turn_opened" => crate::i18n::ex("Turno aperto"),
        "turn_resolving" => crate::i18n::ex("Risoluzione del turno"),
        "turn_resolved" => crate::i18n::ex("Turno risolto"),
        "turn_substep" => crate::i18n::ex("Avanzamento intermedio"),
        "exercise_created" => crate::i18n::ex("Esercitazione creata"),
        "exercise_ended" => crate::i18n::ex("Esercitazione terminata"),
        "exercise_phase_changed" => crate::i18n::ex("Fase dell’esercitazione cambiata"),
        "action_executed" => crate::i18n::ex("Azione eseguita"),
        "timeline_action_decided" => crate::i18n::ex("Azione pianificata decisa"),
        "timeline_action_executed" => crate::i18n::ex("Azione pianificata eseguita"),
        "timeline_action_toggled" => crate::i18n::ex("Azione anticipata attivata/disattivata"),
        "inject_skipped" => crate::i18n::ex("Inject non inviato"),
        "inject_restored" => crate::i18n::ex("Inject ripristinato"),
        "approved_communication_delivered" => crate::i18n::ex("Comunicazione approvata consegnata"),
        "communications_reviewed" => crate::i18n::ex("Comunicazioni riviste"),
        "forecast_published" => crate::i18n::ex("Previsione pubblicata"),
        "language_set" => crate::i18n::ex("Lingua impostata"),
        "map_information_acquired" => crate::i18n::ex("Informazione di mappa acquisita"),
        "physical_intervention_corrected" => crate::i18n::ex("Intervento fisico corretto"),
        "player_draft_submitted" => crate::i18n::ex("Bozza del player inviata"),
        "narrative_submitted" => crate::i18n::ex("Iniziativa narrativa ricevuta"),
        "narrative_outcome" => crate::i18n::ex("Esito narrativo EXCON"),
        "reference_materials_updated" => crate::i18n::ex("Materiali di riferimento aggiornati"),
        "resource_limit_draft_reviewed" => crate::i18n::ex("Limiti di risorse rivisti"),
        "resource_limits_changed" => crate::i18n::ex("Limiti di risorse modificati"),
        "resource_event" => crate::i18n::ex("Evento sulle risorse"),
        "rollback" => crate::i18n::ex("Ripristino a un turno precedente"),
        "settings" => crate::i18n::ex("Impostazioni modificate"),
        "training_design_configured" => crate::i18n::ex("Percorso formativo configurato"),
        "training_path_approved" => crate::i18n::ex("Percorso formativo approvato"),
        "clock_configured" => crate::i18n::ex("Orologio configurato"),
        "clock_paused" => crate::i18n::ex("Orologio in pausa"),
        "clock_physical_events" => crate::i18n::ex("Eventi fisici dell’orologio"),
        "message" => crate::i18n::ex("Messaggio"),
        "chat_source" => crate::i18n::ex("Messaggio chat"),
        "reply_source" => crate::i18n::ex("Risposta del player"),
        "clarification_requested" => crate::i18n::ex("Chiarimento richiesto"),
        "proposal_revalidated" => crate::i18n::ex("Proposta ricontrollata"),
        "proposal_superseded" => crate::i18n::ex("Proposta sostituita"),
        "approved_communication_failed" => crate::i18n::ex("Consegna comunicazione non riuscita"),
        "approved_inject_delivered" => crate::i18n::ex("Inject approvato consegnato"),
        "approved_inject_failed" => crate::i18n::ex("Consegna inject non riuscita"),
        "physical_preview_approved" => crate::i18n::ex("Anteprima fisica approvata"),
        "clock_failed" => crate::i18n::ex("Orologio non riuscito"),
        _ => return humanize_key(kind),
    };
    known.to_string()
}

/// Role display name; `master` is shown as EXCON and unknown ids pass through.
pub fn role_label(roles: &[Value], id: &str) -> String {
    if id == "master" {
        return "EXCON".into();
    }
    roles
        .iter()
        .find(|r| r["id"] == id)
        .and_then(|r| r["name"].as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// "1 squadra" / "3 squadre" (or the English pair) for a counted noun.
fn counted(n: &str, en: bool, it: (&str, &str), eng: (&str, &str)) -> String {
    let one = n.trim().parse::<f64>().is_ok_and(|v| v == 1.0);
    let (s, p) = if en { eng } else { it };
    format!("{} {}", n.trim(), if one { s } else { p })
}

/// Localises the English validation notes produced by the simulator
/// ("500 m line, 1 crew(s)") for the console; unknown notes pass through.
pub fn localize_note(note: &str) -> String {
    localize_note_in(note, crate::i18n::is_en())
}

fn localize_note_in(note: &str, en: bool) -> String {
    let n = note.trim();
    let crews = |c: &str| counted(c, en, ("squadra", "squadre"), ("crew", "crews"));
    let people = |p: &str| counted(p, en, ("persona", "persone"), ("person", "people"));
    if let Some((a, b)) = n.split_once(" m line, ") {
        if let Some(c) = b.strip_suffix(" crew(s)") {
            let c = crews(c);
            return if en { format!("{a} m line, {c}") } else { format!("linea di {a} m, {c}") };
        }
    }
    if let Some((a, b)) = n.split_once(" buildings, ") {
        if let Some(p) = b.strip_suffix(" people") {
            let bld = counted(a, en, ("edificio", "edifici"), ("building", "buildings"));
            return format!("{bld}, {}", people(p));
        }
    }
    if let Some((a, b)) = n.split_once(" bus(es) for ") {
        if let Some(p) = b.strip_suffix(" people") {
            let bus = counted(a, en, ("autobus", "autobus"), ("bus", "buses"));
            return if en { format!("{bus} for {}", people(p)) } else { format!("{bus} per {}", people(p)) };
        }
    }
    if let Some(d) = n.strip_prefix("to ") {
        return if en { format!("to {d}") } else { format!("verso {d}") };
    }
    if let Some((q, rest)) = n.split_once(" arrive in ") {
        if let Some((m, src)) = rest.split_once(" min from ") {
            return if en {
                format!("{q} arrive in {m} min from {src}")
            } else {
                format!("{q} in arrivo tra {m} min da {src}")
            };
        }
    }
    n.to_string()
}

/// "fire_front_km" -> "Fire front km" (Italian labels for the common keys).
pub fn humanize_key(k: &str) -> String {
    let known = match k {
        "wind_dir_deg" => Some(crate::i18n::ex("Direzione vento")),
        "wind_speed_kmh" => Some(crate::i18n::ex("Velocità vento")),
        "moisture_pct" => Some(crate::i18n::ex("Umidità combustibile")),
        "turn" => Some(crate::i18n::ex("Turno")),
        "clock" => Some(crate::i18n::ex("Ora")),
        "actor" => Some(crate::i18n::ex("Autore")),
        "status" => Some(crate::i18n::ex("Stato")),
        "note" => Some(crate::i18n::ex("Nota")),
        "message" => Some(crate::i18n::ex("Messaggio")),
        "qty" => Some(crate::i18n::ex("Quantità")),
        "resource" => Some(crate::i18n::ex("Risorsa")),
        "zone" => Some(crate::i18n::ex("Zona")),
        "building" => Some(crate::i18n::ex("Edificio")),
        "destination" => Some(crate::i18n::ex("Destinazione")),
        "role" => Some(crate::i18n::ex("Ruolo")),
        "title" => Some(crate::i18n::ex("Titolo")),
        "text" => Some(crate::i18n::ex("Testo")),
        _ => None,
    };
    if let Some(s) = known {
        return s.to_string();
    }
    let s = k.trim_start_matches('_').replace(['_', '-'], " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Numbers rounded to at most two decimals, no trailing zeros.
pub fn fmt_num(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    let f = n.as_f64().unwrap_or(0.0);
    if !f.is_finite() {
        return "—".into();
    }
    let s = format!("{f:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Seconds as "1 h 05 min", "3 min 20 s" or "45 s".
pub fn fmt_duration(secs: f64) -> String {
    let neg = secs < 0.0;
    let s = secs.abs().round() as u64;
    let body = if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        if s % 60 == 0 {
            format!("{} min", s / 60)
        } else {
            format!("{} min {} s", s / 60, s % 60)
        }
    } else {
        format!("{s} s")
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// A command location: zone code / name as is, point as "(x, y) m".
pub fn fmt_loc(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) if o.contains_key("x") && o.contains_key("y") => {
            let g = |k: &str| o[k].as_f64().map(|f| format!("{f:.0}")).unwrap_or_default();
            format!("({}, {}) m", g("x"), g("y"))
        }
        Value::Object(o) if o.contains_key("at") => {
            let r = o.get("radius_m").and_then(Value::as_f64).unwrap_or(0.0);
            exfmt!("{} (raggio {:.0} m)", "{} (radius {:.0} m)", fmt_loc(&o["at"]), r)
        }
        Value::Null => "—".into(),
        other => scalar_text(other),
    }
}

/// "Vento: 270° · 12 km/h" from the provided fields.
pub fn fmt_weather(v: &Value) -> String {
    let mut p = vec![];
    if let Some(d) = v["wind_dir_deg"].as_f64() {
        p.push(exfmt!("Vento: {:.0}°", "Wind: {:.0}°", d));
    }
    if let Some(s) = v["wind_speed_kmh"].as_f64() {
        p.push(format!("{s:.0} km/h"));
    }
    if let Some(m) = v["moisture_pct"].as_f64() {
        p.push(exfmt!("umidità combustibile {:.0}%", "fuel moisture {:.0}%", m));
    }
    p.join(" · ")
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::Bool(true) => crate::i18n::ex("Sì").into(),
        Value::Bool(false) => crate::i18n::ex("No").into(),
        Value::Number(n) => fmt_num(n),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn s(v: &Value) -> String {
    scalar_text(v)
}

/// One-line human summary of a `sim::Command` JSON (`{"type": ...}`); None if not a command.
pub fn summarize_command(c: &Value) -> Option<String> {
    let t = c["type"].as_str()?;
    let opt = |k: &str| (!c[k].is_null()).then(|| s(&c[k]));
    Some(match t {
        "CutFireLine" => {
            let w = c["width_m"].as_f64().map(|w| format!(" · {w:.0} m")).unwrap_or_default();
            if let Some(a) = opt("along") {
                exfmt!("Linea tagliafuoco lungo {}{}", "Fire line along {}{}", a, w)
            } else {
                exfmt!(
                    "Linea tagliafuoco da {} a {}{}",
                    "Fire line from {} to {}{}",
                    fmt_loc(&c["from"]),
                    fmt_loc(&c["to"]),
                    w
                )
            }
        }
        "WaterDrop" => exfmt!("Lancio d’acqua su {}", "Water drop on {}", fmt_loc(&c["at"])),
        "MonitorArea" => {
            let zones = c["zones"].as_array().map(|z| z.iter().map(s).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            exfmt!("Drone di monitoraggio su {}", "Monitoring drone over {}", zones)
        }
        "Reconnoitre" => exfmt!("Ricognizione a terra in {}", "Ground reconnaissance of {}", s(&c["zone"])),
        "DeployEngine" => exfmt!("Squadra su strada verso {}", "Engine deployed to {}", fmt_loc(&c["at"])),
        "EvacuateZone" => with_dest(exfmt!("Evacuazione zona {}", "Evacuate zone {}", s(&c["zone"])), opt("destination")),
        "EvacuateBuilding" => with_dest(exfmt!("Evacuazione di {}", "Evacuate {}", s(&c["building"])), opt("destination")),
        "IssueWarning" => exfmt!("Allerta alla popolazione in {}", "Warn the population in {}", s(&c["zone"])),
        "CloseRoad" => exfmt!("Chiusura strada {}", "Close road {}", s(&c["road"])),
        "ReopenRoad" => exfmt!("Riapertura strada {}", "Reopen road {}", s(&c["road"])),
        "OpenShelter" => exfmt!("Apertura rifugio {}", "Open shelter {}", s(&c["building"])),
        "RequestReinforcement" => exfmt!(
            "Richiesta rinforzi: {} × {}",
            "Reinforcement request: {} × {}",
            s(&c["qty"]),
            s(&c["resource"])
        ),
        "Ignite" => {
            let r = c["radius_m"].as_f64().map(|r| format!(" · {r:.0} m")).unwrap_or_default();
            exfmt!("Innesco in {}{}", "Ignite at {}{}", fmt_loc(&c["at"]), r)
        }
        "Extinguish" => exfmt!("Spegnimento in {}", "Extinguish {}", fmt_loc(&c["area"])),
        "ExtinguishArea" => exfmt!(
            "Spegnimento dell’area disegnata ({} vertici, vegetazione invariata)",
            "Extinguish the drawn area ({} vertices, vegetation unchanged)",
            c["polygon"].as_array().map_or(0, |a| a.len())
        ),
        "SetWeather" => fmt_weather(c),
        "SetBuildingStatus" => {
            let mut p = vec![];
            if let Some(d) = opt("damage") {
                p.push(d);
            }
            if let Some(e) = c["evacuated"].as_bool() {
                p.push(if e { crate::i18n::ex("evacuato").into() } else { crate::i18n::ex("non evacuato").into() });
            }
            exfmt!("Stato di {}: {}", "Status of {}: {}", s(&c["building"]), p.join(", "))
        }
        _ => {
            let mut p = vec![humanize_key(t)];
            if let Some(o) = c.as_object() {
                for (k, v) in o.iter().filter(|(k, v)| *k != "type" && !v.is_null()) {
                    p.push(format!("{}: {}", humanize_key(k), inline_text(v)));
                }
            }
            p.join(" · ")
        }
    })
}

fn with_dest(base: String, d: Option<String>) -> String {
    match d {
        Some(d) => format!("{base} → {d}"),
        None => base,
    }
}

/// Compact one-line text for any value (short arrays/objects inside summaries).
pub fn inline_text(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().map(inline_text).collect::<Vec<_>>().join(", "),
        Value::Object(_) if v["type"].is_string() => summarize_command(v).unwrap_or_default(),
        Value::Object(o) if o.contains_key("x") && o.contains_key("y") => fmt_loc(v),
        Value::Object(o) => o
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| format!("{}: {}", humanize_key(k), inline_text(v)))
            .collect::<Vec<_>>()
            .join(" · "),
        other => s(other),
    }
}

fn is_id_key(k: &str) -> bool {
    k == "id" || k == "seq" || k.ends_with("_id") || k.ends_with("_ids")
}

fn is_duration_key(k: &str) -> bool {
    k.ends_with("_sim_s") || k.ends_with("_secs") || k == "duration_s" || k.ends_with("_delay_s")
}

fn is_status_key(k: &str) -> bool {
    matches!(k, "status" | "state" | "outcome" | "severity" | "damage")
}

fn is_scalar(v: &Value) -> bool {
    !v.is_array() && !v.is_object()
}

fn is_command_list(a: &[Value]) -> bool {
    !a.is_empty() && a.iter().all(|c| c["type"].is_string() && !c["ok"].is_boolean())
}

// ------------------------------------------------------------------- rendering

/// Presents `v` according to the data structure named by `kind` ("" = detect).
pub fn render_value(kind: &str, v: &Value) -> Html {
    match kind {
        "commands" | "command" => commands(v),
        "weather" => html! {<p class="rv-line">{fmt_weather(v)}</p>},
        "executions" => executions(v),
        _ => value(v, 0, ""),
    }
}

/// Same as `render_value` plus a collapsed raw-data block for debugging.
pub fn render_with_raw(kind: &str, v: &Value) -> Html {
    html! {<>{render_value(kind, v)}{raw(v)}</>}
}

/// Small collapsed "dati grezzi" block.
pub fn raw(v: &Value) -> Html {
    html! {<details class="rv-raw"><summary>{crate::i18n::ex("dati tecnici (JSON)")}</summary><pre>{crate::api::pretty(v)}</pre></details>}
}

/// Collapsible titled block with the readable rendering of `v`.
pub fn details(title: impl Into<AttrValue>, v: &Value) -> Html {
    html! {<details class="rv-details"><summary>{title.into()}</summary>{value(v, 0, "")}</details>}
}

fn commands(v: &Value) -> Html {
    let items: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Null => vec![],
        o => vec![o],
    };
    if items.is_empty() {
        return html! {<span class="rv-empty">{"—"}</span>};
    }
    html! {<ul class="rv-list">{for items.into_iter().map(|c| html!{<li class="rv-command">{summarize_command(c).unwrap_or_else(|| inline_text(c))}</li>})}</ul>}
}

fn executions(v: &Value) -> Html {
    let a = v.as_array().cloned().unwrap_or_default();
    if a.is_empty() {
        return html! {<span class="rv-empty">{"—"}</span>};
    }
    html! {<ul class="rv-list">{for a.iter().map(|e| {
        let ok = e["ok"].as_bool().unwrap_or(false);
        let cmd = summarize_command(&e["command"]).unwrap_or_else(|| inline_text(&e["command"]));
        let msg = e["message"].as_str().or(e["error"].as_str()).unwrap_or("");
        html!{<li class="rv-exec"><span class={classes!("pill", if ok {"rv-ok"} else {"rv-bad"})}>{if ok {"✓"} else {"✗"}}</span>{" "}<b>{cmd}</b>if !msg.is_empty(){<span class="rv-msg">{format!(" — {msg}")}</span>}</li>}
    })}</ul>}
}

fn leaf(k: &str, v: &Value) -> Html {
    match v {
        Value::Number(n) if is_duration_key(k) => html! {<span>{fmt_duration(n.as_f64().unwrap_or(0.0))}</span>},
        Value::String(t) if is_status_key(k) => html! {<span class="pill">{crate::api::status(t).to_string()}</span>},
        Value::String(t) if t.contains('\n') => html! {<span class="rv-text">{t.clone()}</span>},
        other => html! {<span>{scalar_text(other)}</span>},
    }
}

fn value(v: &Value, depth: usize, key: &str) -> Html {
    if depth > MAX_DEPTH {
        return html! {<span>{inline_text(v)}</span>};
    }
    match v {
        Value::Null => html! {<span class="rv-empty">{"—"}</span>},
        Value::Array(a) if a.is_empty() => html! {<span class="rv-empty">{"—"}</span>},
        Value::Array(a) if is_command_list(a) => commands(v),
        Value::Array(a) if a.iter().all(|e| e["command"].is_object() && e["ok"].is_boolean()) => executions(v),
        Value::Array(a) if a.iter().all(is_scalar) => {
            html! {<ul class="rv-list">{for a.iter().map(|x| html!{<li>{leaf(key, x)}</li>})}</ul>}
        }
        Value::Array(a) => html! {<ul class="rv-list">{for a.iter().map(|x| html!{<li>{value(x, depth + 1, "")}</li>})}</ul>},
        Value::Object(o) => {
            if v["type"].is_string() && summarize_command(v).is_some() && o.len() <= 6 && !v["ok"].is_boolean() {
                return commands(v);
            }
            if o.contains_key("wind_dir_deg") && o.len() <= 4 {
                return html! {<span>{fmt_weather(v)}</span>};
            }
            if o.contains_key("command") && o.contains_key("ok") {
                return executions(&Value::Array(vec![v.clone()]));
            }
            let rows: Vec<_> = o
                .iter()
                .filter(|(_, x)| !x.is_null() && !matches!(x, Value::Array(a) if a.is_empty()))
                .collect();
            if rows.is_empty() {
                return html! {<span class="rv-empty">{"—"}</span>};
            }
            html! {<dl class="rv-dl">{for rows.into_iter().map(|(k, x)| {
                let idc = is_id_key(k).then_some("rv-id");
                html!{<><dt class={classes!(idc)}>{humanize_key(k)}</dt><dd class={classes!(idc)}>{
                    if is_scalar(x) { leaf(k, x) } else { value(x, depth + 1, k) }
                }</dd></>}
            })}</dl>}
        }
        other => leaf(key, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_humanised() {
        assert_eq!(humanize_key("fire_front_km"), "Fire front km");
        assert_eq!(humanize_key("wind_speed_kmh"), "Velocità vento");
        assert_eq!(humanize_key("_x"), "X");
    }
    #[test]
    fn numbers_are_rounded() {
        assert_eq!(fmt_num(&serde_json::Number::from_f64(3.14159).unwrap()), "3.14");
        assert_eq!(fmt_num(&serde_json::Number::from_f64(2.0).unwrap()), "2");
        assert_eq!(fmt_num(&7.into()), "7");
    }
    #[test]
    fn durations() {
        assert_eq!(fmt_duration(45.0), "45 s");
        assert_eq!(fmt_duration(200.0), "3 min 20 s");
        assert_eq!(fmt_duration(3900.0), "1 h 05 min");
        assert_eq!(fmt_duration(120.0), "2 min");
    }
    #[test]
    fn locations() {
        assert_eq!(fmt_loc(&json!("F4")), "F4");
        assert_eq!(fmt_loc(&json!({"x": 2800, "y": 2400.4})), "(2800, 2400) m");
        assert_eq!(fmt_loc(&json!({"at": "F4", "radius_m": 120})), "F4 (raggio 120 m)");
    }
    #[test]
    fn weather() {
        let w = json!({"wind_dir_deg": 270, "wind_speed_kmh": 12.0});
        assert_eq!(fmt_weather(&w), "Vento: 270° · 12 km/h");
    }
    #[test]
    fn commands_summarised() {
        assert_eq!(summarize_command(&json!({"type": "WaterDrop", "at": "F4"})).unwrap(), "Lancio d’acqua su F4");
        assert_eq!(
            summarize_command(&json!({"type": "EvacuateZone", "zone": "C2", "destination": "Scuola"})).unwrap(),
            "Evacuazione zona C2 → Scuola"
        );
        assert_eq!(
            summarize_command(&json!({"type": "RequestReinforcement", "resource": "engine", "qty": 2})).unwrap(),
            "Richiesta rinforzi: 2 × engine"
        );
        assert!(summarize_command(&json!({"foo": 1})).is_none());
        assert_eq!(summarize_command(&json!({"type": "Frobnicate", "level": 3})).unwrap(), "Frobnicate · Level: 3");
    }
    #[test]
    fn inline_never_braces() {
        let t = inline_text(&json!({"a": 1, "b": {"c": [1, 2]}, "d": null}));
        assert!(!t.contains('{') && !t.contains('"'), "{t}");
        assert_eq!(t, "A: 1 · B: C: 1, 2");
    }
    #[test]
    fn notes_and_event_labels() {
        assert_eq!(localize_note_in("500 m line, 1 crew(s)", false), "linea di 500 m, 1 squadra");
        assert_eq!(localize_note_in("500 m line, 3 crew(s)", false), "linea di 500 m, 3 squadre");
        assert_eq!(localize_note_in("3 buildings, 40 people", false), "3 edifici, 40 persone");
        assert_eq!(localize_note_in("1 buildings, 1 people", false), "1 edificio, 1 persona");
        assert_eq!(localize_note_in("1 bus(es) for 1 people", false), "1 autobus per 1 persona");
        assert_eq!(localize_note_in("2 bus(es) for 90 people", false), "2 autobus per 90 persone");
        assert_eq!(localize_note_in("500 m line, 1 crew(s)", true), "500 m line, 1 crew");
        assert_eq!(localize_note_in("500 m line, 2 crew(s)", true), "500 m line, 2 crews");
        assert_eq!(localize_note_in("1 buildings, 1 people", true), "1 building, 1 person");
        assert_eq!(localize_note_in("2 bus(es) for 90 people", true), "2 buses for 90 people");
        assert_eq!(localize_note_in("to Pineta", false), "verso Pineta");
        assert_eq!(localize_note_in("2 crews arrive in 12 min from base", false), "2 crews in arrivo tra 12 min da base");
        assert_eq!(localize_note_in("something else", false), "something else");
        assert_eq!(event_label("proposal_decided"), "Proposta decisa dall’EXCON");
        assert_eq!(event_label("brand_new_kind"), "Brand new kind");
        let roles = vec![json!({"id":"fire_commander","name":"Comandante VVF"})];
        assert_eq!(role_label(&roles, "fire_commander"), "Comandante VVF");
        assert_eq!(role_label(&roles, "master"), "EXCON");
        assert_eq!(role_label(&roles, "x"), "x");
    }
    #[test]
    fn id_keys() {
        assert!(is_id_key("msel_id") && is_id_key("id") && !is_id_key("identity"));
    }
}
