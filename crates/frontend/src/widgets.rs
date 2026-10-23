use crate::{
    api::{self, list, pretty, text},
    Action,
};
use serde_json::{json, Value};
use yew::prelude::*;

pub fn details(title: impl Into<AttrValue>, value: &Value) -> Html {
    html! {<details><summary>{title.into()}</summary><pre>{pretty(value)}</pre></details>}
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
        "OpenShelter" => crate::i18n::ex("Aprire un rifugio"),
        "CloseRoad" => crate::i18n::ex("Chiudere una strada"),
        "ReopenRoad" => crate::i18n::ex("Riaprire una strada"),
        "CutFireLine" => crate::i18n::ex("Creare una linea tagliafuoco"),
        "WaterDrop" => crate::i18n::ex("Effettuare un lancio d’acqua"),
        "DeployEngine" => crate::i18n::ex("Inviare un’autobotte"),
        "RequestReinforcement" => crate::i18n::ex("Richiedere rinforzi"),
        "Ignite" => crate::i18n::ex("Innescare un incendio"),
        "Extinguish" => crate::i18n::ex("Spegnere un’area"),
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
    let mut parts = Vec::new();
    if let Some(obj) = cmd.as_object() {
        for (key, value) in obj {
            if key == "type" || value.is_null() {
                continue;
            }
            let shown = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => crate::i18n::t(lang, if *b { "yes" } else { "no" }).into(),
                Value::Object(o) if o.contains_key("x") && o.contains_key("y") => format!(
                    "({:.0}, {:.0})",
                    o["x"].as_f64().unwrap_or(0.),
                    o["y"].as_f64().unwrap_or(0.)
                ),
                other => other.to_string(),
            };
            parts.push(format!("{}: {}", crate::i18n::field(lang, key), shown));
        }
    }
    let name = if lang == "it" { command_name(text(&cmd["type"])) } else { crate::i18n::action(lang, text(&cmd["type"])) };
    if parts.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {}", parts.join(" · "))
    }
}
#[cfg(test)]
mod summary_tests {
    use super::*;
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

#[derive(Properties, PartialEq)]
pub struct ProposalProps {
    pub proposal: Value,
    pub generation: Value,
    pub preview: Value,
    pub action: Callback<Action>,
    pub busy: bool,
}
#[function_component(Proposal)]
pub fn proposal(props: &ProposalProps) -> Html {
    let p = &props.proposal;
    let commands = use_state(|| list(&p["commands"]).to_vec());
    {
        let commands = commands.clone();
        use_effect_with(p["commands"].clone(), move |next| {
            commands.set(list(next).to_vec());
        });
    }
    let note = use_state(String::new);
    let error = use_state(String::new);
    let comms = use_state(|| pretty(&p["communication_drafts"]));
    let injects = use_state(|| pretty(&p["inject_drafts"]));
    let limits = use_state(|| pretty(&p["resource_limit_draft"]));
    let id = p["id"].as_u64().unwrap_or(0);
    let editable = ["pending", "needs_clarification"].contains(&text(&p["status"]));
    let needs_preview = p["source"]["kind"] == "master" && !commands.is_empty();
    let needs_reason = p["source"]["kind"] == "player" && note.trim().is_empty();
    let valid_preview = props.preview["generation"] == props.generation
        && props.preview["_request"]["commands"] == json!(*commands)
        && !props.preview.is_null();
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
        Callback::from(move |_| {
            let mut body = json!({"approve":approve,"commands":*commands,"note":*note});
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
            action.emit(Action::post(
                format!("/api/master/proposals/{id}/decide"),
                body,
            ));
        })
    };
    let no_actions=editable&&commands.is_empty()&&list(&p["inject_drafts"]).is_empty()&&list(&p["communication_drafts"]).is_empty()&&p["resource_limit_draft"].is_null();
    let blocked=no_actions||editable&&list(&p["verdicts"]).iter().any(|v|v["verdict"]!="ok");
    html! {<section class="proposal-inline" data-proposal={id.to_string()}>
        <div class="proposal-head"><b>{crate::i18n::ex("Azioni proposte")}</b><span class="pill pending">{api::status(text(&p["status"]))}</span></div>
        <p class="proposal-request">{text(if text(&p["translation_en"]).is_empty(){&p["original"]}else{&p["translation_en"]})}</p>if text(&p["summary"])!=text(if text(&p["translation_en"]).is_empty(){&p["original"]}else{&p["translation_en"]}){<p class="proposal-summary">{text(&p["summary"])}</p>}
        if p["clarification"].is_string(){<p class="proposal-clarification">{text(&p["clarification"])}</p>}
        <div class="command-list">{for commands.iter().enumerate().map(|(i,cmd)|{
            html!{<fieldset class="command-card"><legend>{command_name(text(&cmd["type"]))}</legend>
            {for cmd.as_object().into_iter().flatten().filter(|(k,_)|k.as_str()!="type").map(|(key,value)|{
                let key=key.clone();let label_key=key.clone();let original=value.clone();let commands=commands.clone();let error=error.clone();
                let display=if value.is_string(){text(value).into()}else{value.to_string()};
                let onchange=Callback::from(move |e:Event|{use yew::TargetCast;let value=e.target_unchecked_into::<web_sys::HtmlInputElement>().value();
                    let parsed=if original.is_string(){Ok(json!(value))}else{serde_json::from_str::<Value>(&value).or_else(|err|if original.is_object()&&!value.trim().starts_with('{'){Ok(json!(value))}else{Err(err)})};
                    match parsed {Ok(value)=>{let mut next=(*commands).clone();next[i][&key]=value;commands.set(next);error.set(String::new());},Err(e)=>error.set(crate::exfmt!("Parametro non valido: {e}","Invalid parameter: {e}"))}
                });
                html!{<label class="command-field"><span>{field_name(&label_key)}</span><input aria-label={field_name(&label_key).to_string()} data-command={i.to_string()} data-field={label_key.clone()} value={display} {onchange} disabled={!editable||props.busy}/></label>}
            })}
            <small class={if p["verdicts"][i]["verdict"]=="rejected"{"danger"}else{"verdict-ok"}}>{if p["verdicts"][i]["verdict"]=="ok"{crate::exfmt!("✓ Verifica: {}","✓ Check: {}",if list(&p["verdicts"][i]["notes"]).is_empty(){crate::i18n::ex("azione valida").into()}else{list(&p["verdicts"][i]["notes"]).iter().map(text).collect::<Vec<_>>().join(" · ")})}else{text(&p["verdicts"][i]["reason"]).into()}}</small></fieldset>}
        })}</div>
        if editable{<details class="manual-commands"><summary>{crate::i18n::ex("Modifica o aggiungi comandi JSON (interpretazione manuale EXCON)")}</summary><textarea data-manual-commands="" aria-label={crate::i18n::ex("Comandi manuali EXCON")} value={pretty(&json!(*commands))} oninput={{let commands=commands.clone();let error=error.clone();Callback::from(move |e|match serde_json::from_str::<Vec<Value>>(&api::input(e)){Ok(next)=>{commands.set(next);error.set(String::new());},Err(e)=>error.set(crate::exfmt!("Comandi non validi: {e}","Invalid commands: {e}"))})}}/></details>}
        if !p["invalid"].is_null()&&!list(&p["invalid"]).is_empty(){<p class="danger">{pretty(&p["invalid"])}</p>}
        if editable&&!p["resource_limit_draft"].is_null(){<label>{crate::i18n::ex("Limiti di risorse proposti")}<textarea value={(*limits).clone()} oninput={{let limits=limits.clone();Callback::from(move|e|limits.set(api::input(e)))}}/></label>}
        if editable&&!list(&p["communication_drafts"]).is_empty(){<label>{crate::i18n::ex("Comunicazioni da revisionare")}<textarea value={(*comms).clone()} oninput={{let comms=comms.clone();Callback::from(move|e|comms.set(api::input(e)))}}/></label>}
        if editable&&!list(&p["inject_drafts"]).is_empty(){<label>{crate::i18n::ex("Inject proposti")}<textarea value={(*injects).clone()} oninput={{let injects=injects.clone();Callback::from(move|e|injects.set(api::input(e)))}}/></label>}
        <label class="proposal-note"><span>{crate::i18n::ex("Nota EXCON (facoltativa)")}</span><input data-note="" aria-label={crate::i18n::ex("Nota EXCON")} value={(*note).clone()} oninput={{let note=note.clone();Callback::from(move |e|note.set(api::input(e)))}}/></label>
        <p class="error">{&*error}</p>
        if editable&&needs_reason{<p class="danger proposal-reason-hint" role="status">{crate::i18n::ex("Spiega il motivo al player")}</p>}
        if blocked{<p class="danger proposal-blocked" role="status">{if no_actions{crate::i18n::ex("Nessuna azione interpretata: aggiungi i comandi manualmente (JSON) oppure rifiuta la proposta.")}else{crate::i18n::ex("Non approvabile: correggi azioni o risorse (vedi verifica sopra) oppure rifiuta la proposta.")}}</p>}
        if needs_preview {<button disabled={props.busy||!editable} onclick={{let cb=props.action.clone();let commands=(*commands).clone();Callback::from(move |_|cb.emit(Action::store("/api/master/preview",Some(json!({"commands":commands})),format!("proposal:{id}"))))}}>{crate::i18n::ex("Esamina anteprima fisica")}</button>
            if !props.preview.is_null(){{preview(&props.preview)}}
        }
        <div class="row proposal-actions"><button data-approve="" class="primary" disabled={props.busy||!editable||blocked||!error.is_empty()||(needs_preview&&!valid_preview)} onclick={decide(true)}>{crate::i18n::ex("Approva azione")}</button><button data-reject="" disabled={props.busy||!editable||needs_reason} title={needs_reason.then_some(crate::i18n::ex("Spiega il motivo al player"))} onclick={decide(false)}>{crate::i18n::ex("Rifiuta")}</button>
        if p["status"]=="approved"{<button disabled={props.busy} data-execute={id.to_string()} onclick={{let cb=props.action.clone();Callback::from(move |_|cb.emit(Action::post(format!("/api/master/proposals/{id}/execute"),json!({}))))}}>{crate::i18n::ex("Esegui ora ordine approvato")}</button>}
        </div>{details(crate::i18n::ex("Cronologia ed esiti effettivi"),&json!({"history":p["history"],"executions":p["executions"]}))}
    </section>}
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
