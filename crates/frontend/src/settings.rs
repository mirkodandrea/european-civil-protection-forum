//! Settings: the inject library editor and the buttons that delete each stored
//! object. Shared by the standalone app (browser storage) and the server UI
//! (server files); the host supplies the stored data and a [`Backend`].
use crate::api;
use futures::future::LocalBoxFuture;
use scenario::InjectTemplate;
use std::rc::Rc;
use yew::prelude::*;

/// What the settings can ask the host to do.
pub enum Op {
    SaveInjects(Vec<InjectTemplate>),
    /// Save, then start a new session with these injects.
    NewSession(Vec<InjectTemplate>),
    /// `session`, `injects` or `all`.
    Delete(&'static str),
}

/// Runs an [`Op`]; the text is shown as the outcome.
#[derive(Clone)]
pub struct Backend(pub Rc<dyn Fn(Op) -> LocalBoxFuture<'static, Result<String, String>>>);
impl PartialEq for Backend {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Properties, PartialEq)]
pub struct Props {
    /// Role ids an inject can be addressed to (besides `all`).
    pub roles: Vec<String>,
    /// Injects of the scenario shipped with the app.
    pub defaults: Vec<InjectTemplate>,
    /// Saved inject library, if any.
    pub library: Option<Vec<InjectTemplate>>,
    /// One line about the saved session.
    pub session_text: String,
    pub save_error: Option<String>,
    /// What "delete everything" removes, for the explanation under it.
    pub reset_note: String,
    pub backend: Backend,
    /// Re-render trigger: the exercise changed.
    pub revision: u64,
}

#[derive(Clone, PartialEq)]
struct Draft {
    id: String,
    turn: String,
    title: String,
    sender: String,
    recipients: String,
    text: String,
    objective: String,
}

impl From<&InjectTemplate> for Draft {
    fn from(i: &InjectTemplate) -> Draft {
        Draft {
            id: i.id.clone(),
            turn: i.turn.to_string(),
            title: i.title.clone(),
            sender: i.sender.clone(),
            recipients: i.recipients.join(", "),
            text: i.text.clone(),
            objective: i.objective.clone(),
        }
    }
}

fn drafts(list: &[InjectTemplate]) -> Vec<Draft> {
    list.iter().map(Draft::from).collect()
}

fn validate(rows: &[Draft], roles: &[String]) -> Result<Vec<InjectTemplate>, String> {
    let mut out: Vec<InjectTemplate> = Vec::new();
    for (n, r) in rows.iter().enumerate() {
        let at = |what: &str| format!("Inject {}: {what}", n + 1);
        let id = r.id.trim().to_string();
        if id.is_empty() || r.title.trim().is_empty() || r.text.trim().is_empty() {
            return Err(at("identificativo, titolo e testo sono obbligatori"));
        }
        if out.iter().any(|o| o.id == id) {
            return Err(at(&format!("identificativo «{id}» duplicato")));
        }
        let turn = r
            .turn
            .trim()
            .parse::<u32>()
            .map_err(|_| at("il turno deve essere un numero intero"))?;
        let recipients: Vec<String> = r
            .recipients
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if recipients.is_empty() {
            return Err(at("indica almeno un destinatario (o «all»)"));
        }
        if let Some(bad) = recipients.iter().find(|x| *x != "all" && !roles.contains(x)) {
            return Err(at(&format!(
                "destinatario «{bad}» sconosciuto (ruoli: {})",
                roles.join(", ")
            )));
        }
        out.push(InjectTemplate {
            id,
            turn,
            title: r.title.trim().into(),
            sender: r.sender.trim().into(),
            recipients,
            text: r.text.trim().into(),
            objective: r.objective.trim().into(),
            event: String::new(),
        });
    }
    Ok(out)
}

#[function_component(Settings)]
pub fn settings(props: &Props) -> Html {
    let library = props.library.clone();
    let rows = {
        let initial = library.clone().unwrap_or_else(|| props.defaults.clone());
        use_state(move || drafts(&initial))
    };
    {
        // The stored library can arrive or change after the first render.
        let rows = rows.clone();
        let current = library.clone().unwrap_or_else(|| props.defaults.clone());
        use_effect_with(current, move |list| {
            rows.set(drafts(list));
            || ()
        });
    }
    let message = use_state(String::new);
    let confirm = use_state(|| None::<&'static str>);
    let json = use_state(String::new);
    let _ = props.revision;
    let run = {
        let (backend, message) = (props.backend.clone(), message.clone());
        move |op: Op| {
            let (backend, message) = (backend.clone(), message.clone());
            wasm_bindgen_futures::spawn_local(async move {
                message.set(match (backend.0)(op).await {
                    Ok(text) | Err(text) => text,
                });
            });
        }
    };

    let update = |index: usize, set: fn(&mut Draft, String)| {
        let rows = rows.clone();
        Callback::from(move |e: InputEvent| {
            let mut next = (*rows).clone();
            if let Some(row) = next.get_mut(index) {
                set(row, api::input(e));
            }
            rows.set(next);
        })
    };
    // Validate the editor, then hand the list to the host.
    let save = {
        let (rows, roles, message) = (rows.clone(), props.roles.clone(), message.clone());
        let run = run.clone();
        move |new_session: bool| match validate(&rows, &roles) {
            Ok(list) => run(if new_session { Op::NewSession(list) } else { Op::SaveInjects(list) }),
            Err(e) => message.set(e),
        }
    };
    let ask = |what: &'static str| {
        let confirm = confirm.clone();
        Callback::from(move |_| confirm.set(Some(what)))
    };
    let cancel = {
        let confirm = confirm.clone();
        Callback::from(move |_| confirm.set(None))
    };
    // A delete button that needs a second, explicit click.
    let delete = |what: &'static str, label: &'static str, id: &'static str, run: Callback<()>| {
        if *confirm == Some(what) {
            html! {<span class="confirm-delete">
                <button id={format!("{id}-confirm")} class="danger" onclick={Callback::from(move |_| run.emit(()))}>{format!("Conferma: {label}")}</button>
                <button onclick={cancel.clone()}>{"Annulla"}</button>
            </span>}
        } else {
            html! {<button id={id} class="danger" onclick={ask(what)}>{label}</button>}
        }
    };

    let sources = if library.is_some() {
        "Libreria personalizzata"
    } else {
        "Injects dello scenario (nessuna libreria salvata)"
    };

    html! {
      <div class="settings-panel">
        <section class="settings-section" id="settings-session">
          <h3>{"Sessione"}</h3>
          <p>{&props.session_text}{" · salvata automaticamente e ripresa alla riapertura."}</p>
          if let Some(error) = &props.save_error {<p role="alert">{format!("Salvataggio non riuscito: {error}")}</p>}
          {delete("session","Elimina sessione salvata","delete-session",{let run=run.clone();Callback::from(move |_|run(Op::Delete("session")))})}
        </section>
        <section class="settings-section" id="settings-injects">
          <h3>{"Injects"}</h3>
          <p>{sources}{" · le modifiche valgono dalla prossima nuova sessione; quella in corso mantiene i propri."}</p>
          {for rows.iter().enumerate().map(|(i,row)| html!{
            <fieldset class="inject-row" data-inject={row.id.clone()}>
              <legend>{format!("{} · turno {}",if row.id.is_empty(){"nuovo"}else{&row.id},row.turn)}</legend>
              <div class="settings-fields">
                <label>{"Identificativo"}<input class="inject-id" value={row.id.clone()} oninput={update(i,|r,v|r.id=v)}/></label>
                <label>{"Turno"}<input class="inject-turn" type="number" min="1" value={row.turn.clone()} oninput={update(i,|r,v|r.turn=v)}/></label>
                <label>{"Mittente"}<input class="inject-sender" value={row.sender.clone()} oninput={update(i,|r,v|r.sender=v)}/></label>
                <label>{"Destinatari (ruoli separati da virgola, o all)"}<input class="inject-recipients" value={row.recipients.clone()} oninput={update(i,|r,v|r.recipients=v)}/></label>
              </div>
              <label>{"Titolo"}<input class="inject-title" value={row.title.clone()} oninput={update(i,|r,v|r.title=v)}/></label>
              <label>{"Testo"}<textarea class="inject-text" rows="3" value={row.text.clone()} oninput={update(i,|r,v|r.text=v)}/></label>
              <label>{"Obiettivo"}<input class="inject-objective" value={row.objective.clone()} oninput={update(i,|r,v|r.objective=v)}/></label>
              <button class="danger delete-inject" onclick={{let rows=rows.clone();Callback::from(move |_|{let mut next=(*rows).clone();next.remove(i);rows.set(next);})}}>{"Elimina inject"}</button>
            </fieldset>
          })}
          <div class="settings-fields">
            <button id="add-inject" onclick={{let rows=rows.clone();let next_turn=rows.iter().filter_map(|r|r.turn.parse::<u32>().ok()).max().unwrap_or(1);Callback::from(move |_|{let mut next=(*rows).clone();next.push(Draft{id:format!("inject-{}",next.len()+1),turn:next_turn.to_string(),title:String::new(),sender:String::new(),recipients:"all".into(),text:String::new(),objective:String::new()});rows.set(next);})}}>{"Aggiungi inject"}</button>
            <button id="save-injects" onclick={{let save=save.clone();Callback::from(move |_|save(false))}}>{"Salva nella libreria"}</button>
            <button id="new-session-injects" onclick={{let save=save.clone();Callback::from(move |_|save(true))}}>{"Salva e inizia nuova sessione"}</button>
            <button id="reset-injects-editor" onclick={{let rows=rows.clone();let defaults=props.defaults.clone();Callback::from(move |_|rows.set(drafts(&defaults)))}}>{"Carica injects dello scenario"}</button>
          </div>
          <div class="settings-fields">
            <button id="export-injects" onclick={{let (rows,roles,json,message)=(rows.clone(),props.roles.clone(),json.clone(),message.clone());Callback::from(move |_|match validate(&rows,&roles){Ok(list)=>json.set(serde_json::to_string_pretty(&list).unwrap_or_default()),Err(e)=>message.set(e)})}}>{"Esporta JSON"}</button>
            <button id="import-injects" onclick={{let (rows,json,message)=(rows.clone(),json.clone(),message.clone());Callback::from(move |_|match serde_json::from_str::<Vec<InjectTemplate>>(&json){Ok(list)=>{rows.set(drafts(&list));message.set("JSON importato nell’editor · salva per conservarlo".into());}Err(e)=>message.set(format!("JSON non valido: {e}"))})}}>{"Importa JSON"}</button>
            {delete("injects","Elimina libreria injects","delete-injects",{let run=run.clone();Callback::from(move |_|run(Op::Delete("injects")))})}
          </div>
          <textarea id="injects-json" rows="4" aria-label="JSON degli injects" value={(*json).clone()} oninput={{let json=json.clone();Callback::from(move |e|json.set(api::input(e)))}}/>
          if !message.is_empty() {<p id="injects-message" role="status">{(*message).clone()}</p>}
        </section>
        <section class="settings-section" id="settings-reset">
          <h3>{"Ripartire da zero"}</h3>
          <p>{&props.reset_note}</p>
          {delete("all","Elimina tutto e riparti da zero","delete-all",{let run=run.clone();Callback::from(move |_|run(Op::Delete("all")))})}
        </section>
      </div>
    }
}
