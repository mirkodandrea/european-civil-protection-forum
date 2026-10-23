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
        return html! {<p>{"Tutti i ruoli sono disponibili nelle schede di questa app. Ogni apertura del file avvia un’esercitazione indipendente."}</p>};
    }
    html! {<div id="links">{for list(&links["players"]).iter().chain(list(&links["evaluators"])).map(|p|{
        let id=text(if p["role"].is_null(){&p["id"]}else{&p["role"]});
        let name=roles.iter().find(|r|r["id"]==id).map(|r|text(&r["name"])).unwrap_or(id);
        html!{<p><a href={text(&p["url"]).to_string()} target="_blank" rel="noopener noreferrer">{name}</a></p>}
    })}</div>}
}
pub fn stats(v: &Value) -> Html {
    html! {<p>{format!("Bruciato {:.1} ha · edifici danneggiati {} · distrutti {} · minacciati {} · evacuati {} · a rischio {} · nei rifugi {}",v["burned_ha"].as_f64().unwrap_or(0.),v["damaged"],v["destroyed"],v["threatened"],v["evacuated_people"],v["people_at_risk"],v["sheltered"])}</p>}
}
pub fn command_name(s: &str) -> &str {
    match s {
        "IssueWarning" => "Avvisare la popolazione",
        "EvacuateZone" => "Evacuare una zona",
        "EvacuateBuilding" => "Evacuare un edificio",
        "OpenShelter" => "Aprire un rifugio",
        "CloseRoad" => "Chiudere una strada",
        "ReopenRoad" => "Riaprire una strada",
        "CutFireLine" => "Creare una linea tagliafuoco",
        "WaterDrop" => "Effettuare un lancio d’acqua",
        "DeployEngine" => "Inviare un’autobotte",
        "RequestReinforcement" => "Richiedere rinforzi",
        "Ignite" => "Innescare un incendio",
        "Extinguish" => "Spegnere un’area",
        "ReturnResource" => "Restituire risorse",
        "SetWeather" => "Modificare il meteo",
        "SetBuildingStatus" => "Aggiornare lo stato di un edificio",
        "SetDeploymentTravel" => "Configurare trasferimento risorse",
        "AdjustResource" => "Modificare le risorse",
        "AdvanceTime" => "Avanzare il tempo",
        _ => s,
    }
}
pub fn field_name(s: &str) -> &str {
    match s {
        "zone" => "Zona",
        "destination" => "Destinazione",
        "building" => "Edificio o struttura",
        "road" => "Strada",
        "from" => "Da",
        "to" => "A",
        "width_m" => "Larghezza (metri)",
        "at" => "Luogo",
        "qty" => "Quantità",
        "resource" => "Risorsa",
        "radius_m" => "Raggio (metri)",
        "area" => "Area",
        "role" => "Ruolo",
        "minutes" => "Minuti",
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
        <div class="proposal-head"><b>{"Azioni proposte"}</b><span class="pill pending">{api::status(text(&p["status"]))}</span></div>
        <p class="proposal-request">{text(if text(&p["translation_en"]).is_empty(){&p["original"]}else{&p["translation_en"]})}</p>if text(&p["summary"])!=text(if text(&p["translation_en"]).is_empty(){&p["original"]}else{&p["translation_en"]}){<p class="proposal-summary">{text(&p["summary"])}</p>}
        if p["clarification"].is_string(){<p class="proposal-clarification">{text(&p["clarification"])}</p>}
        <div class="command-list">{for commands.iter().enumerate().map(|(i,cmd)|{
            html!{<fieldset class="command-card"><legend>{command_name(text(&cmd["type"]))}</legend>
            {for cmd.as_object().into_iter().flatten().filter(|(k,_)|k.as_str()!="type").map(|(key,value)|{
                let key=key.clone();let label_key=key.clone();let original=value.clone();let commands=commands.clone();let error=error.clone();
                let display=if value.is_string(){text(value).into()}else{value.to_string()};
                let onchange=Callback::from(move |e:Event|{use yew::TargetCast;let value=e.target_unchecked_into::<web_sys::HtmlInputElement>().value();
                    let parsed=if original.is_string(){Ok(json!(value))}else{serde_json::from_str::<Value>(&value).or_else(|err|if original.is_object()&&!value.trim().starts_with('{'){Ok(json!(value))}else{Err(err)})};
                    match parsed {Ok(value)=>{let mut next=(*commands).clone();next[i][&key]=value;commands.set(next);error.set(String::new());},Err(e)=>error.set(format!("Parametro non valido: {e}"))}
                });
                html!{<label class="command-field"><span>{field_name(&label_key)}</span><input aria-label={field_name(&label_key).to_string()} data-command={i.to_string()} data-field={label_key.clone()} value={display} {onchange} disabled={!editable||props.busy}/></label>}
            })}
            <small class={if p["verdicts"][i]["verdict"]=="rejected"{"danger"}else{"verdict-ok"}}>{if p["verdicts"][i]["verdict"]=="ok"{format!("✓ Verifica: {}",if list(&p["verdicts"][i]["notes"]).is_empty(){"azione valida".into()}else{list(&p["verdicts"][i]["notes"]).iter().map(text).collect::<Vec<_>>().join(" · ")})}else{text(&p["verdicts"][i]["reason"]).into()}}</small></fieldset>}
        })}</div>
        if editable{<details class="manual-commands"><summary>{"Modifica o aggiungi comandi JSON (interpretazione manuale EXCON)"}</summary><textarea data-manual-commands="" aria-label="Comandi manuali EXCON" value={pretty(&json!(*commands))} oninput={{let commands=commands.clone();let error=error.clone();Callback::from(move |e|match serde_json::from_str::<Vec<Value>>(&api::input(e)){Ok(next)=>{commands.set(next);error.set(String::new());},Err(e)=>error.set(format!("Comandi non validi: {e}"))})}}/></details>}
        if !p["invalid"].is_null()&&!list(&p["invalid"]).is_empty(){<p class="danger">{pretty(&p["invalid"])}</p>}
        if editable&&!p["resource_limit_draft"].is_null(){<label>{"Limiti di risorse proposti"}<textarea value={(*limits).clone()} oninput={{let limits=limits.clone();Callback::from(move|e|limits.set(api::input(e)))}}/></label>}
        if editable&&!list(&p["communication_drafts"]).is_empty(){<label>{"Comunicazioni da revisionare"}<textarea value={(*comms).clone()} oninput={{let comms=comms.clone();Callback::from(move|e|comms.set(api::input(e)))}}/></label>}
        if editable&&!list(&p["inject_drafts"]).is_empty(){<label>{"Inject proposti"}<textarea value={(*injects).clone()} oninput={{let injects=injects.clone();Callback::from(move|e|injects.set(api::input(e)))}}/></label>}
        <label class="proposal-note"><span>{"Nota EXCON (facoltativa)"}</span><input data-note="" aria-label="Nota EXCON" value={(*note).clone()} oninput={{let note=note.clone();Callback::from(move |e|note.set(api::input(e)))}}/></label>
        <p class="error">{&*error}</p>
        if blocked{<p class="danger proposal-blocked" role="status">{if no_actions{"Nessuna azione interpretata: aggiungi i comandi manualmente (JSON) oppure rifiuta la proposta."}else{"Non approvabile: correggi azioni o risorse (vedi verifica sopra) oppure rifiuta la proposta."}}</p>}
        if needs_preview {<button disabled={props.busy||!editable} onclick={{let cb=props.action.clone();let commands=(*commands).clone();Callback::from(move |_|cb.emit(Action::store("/api/master/preview",Some(json!({"commands":commands})),format!("proposal:{id}"))))}}>{"Esamina anteprima fisica"}</button>
            if !props.preview.is_null(){{preview(&props.preview)}}
        }
        <div class="row proposal-actions"><button data-approve="" class="primary" disabled={props.busy||!editable||blocked||!error.is_empty()||(needs_preview&&!valid_preview)} onclick={decide(true)}>{"Approva azione"}</button><button data-reject="" disabled={props.busy||!editable} onclick={decide(false)}>{"Rifiuta"}</button>
        if p["status"]=="approved"{<button disabled={props.busy} data-execute={id.to_string()} onclick={{let cb=props.action.clone();Callback::from(move |_|cb.emit(Action::post(format!("/api/master/proposals/{id}/execute"),json!({}))))}}>{"Esegui ora ordine approvato"}</button>}
        </div>{details("Cronologia ed esiti effettivi",&json!({"history":p["history"],"executions":p["executions"]}))}
    </section>}
}
pub fn preview(value: &Value) -> Html {
    html! {<div class="preview"><h3>{"Effetto immediato previsto"}</h3>
    <p>{format!("Celle in fiamme {} → {} · bruciato {:.2} → {:.2} ha",value["before"]["burning_cells"],value["after"]["burning_cells"],value["before"]["burned_ha"].as_f64().unwrap_or(0.),value["after"]["burned_ha"].as_f64().unwrap_or(0.))}</p>
    {for list(&value["results"]).iter().map(|r|{let g=&r["geometry"];html!{<div class="card"><b>{command_name(text(&r["command"]["type"]))}</b>
        if g["centre_m"].is_array(){<p>{format!("Centro: {}, {} m est/nord",g["centre_m"][0],g["centre_m"][1])}</p>}
        if let Some(radius)=g["radius_m"].as_f64(){<p>{format!("Raggio: {radius:.0} m")}</p>}
        if g["bounds_m"].is_array(){<p>{format!("Limiti (metri): {}",list(&g["bounds_m"]).iter().map(Value::to_string).collect::<Vec<_>>().join(", "))}</p>}
        if let Some(length)=g["length_m"].as_f64(){<p>{format!("Lunghezza: {length:.0} m · larghezza: {} m",g["width_m"])}</p>}
        if let Some(area)=g["nominal_area_ha"].as_f64(){<p>{format!("Area nominale: {area:.2} ha")}</p>}
        <p class={if r["result"]["Err"].is_null(){""}else{"danger"}}>{if let Some(err)=r["result"]["Err"].as_str(){err.to_string()}else{list(&r["result"]["Ok"]).iter().map(text).collect::<Vec<_>>().join(" · ")}}</p><p>{text(&g["effect"])}</p>
    </div>}})}<p class="muted">{text(&value["warning"])}</p>
    {details("Dati completi anteprima",value)}</div>}
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
    #[prop_or("Invia per revisione / applica".into())]
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
