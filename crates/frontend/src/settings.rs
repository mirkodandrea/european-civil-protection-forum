//! Settings: the buttons that delete each stored object. Shared by the
//! standalone app (browser storage) and the server UI (server files); the
//! host supplies the stored data and a [`Backend`]. Scenarios and their
//! injects are edited in the scenario view (`scenario_editor`).
use futures::future::LocalBoxFuture;
use std::rc::Rc;
use yew::prelude::*;

/// What the settings can ask the host to do.
pub enum Op {
    /// `session`, or `all` (also the saved scenarios and the chosen one).
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
    /// One line about the saved session.
    pub session_text: String,
    pub save_error: Option<String>,
    /// What "delete everything" removes, for the explanation under it.
    pub reset_note: String,
    pub backend: Backend,
    /// Re-render trigger: the exercise changed.
    pub revision: u64,
}

#[function_component(Settings)]
pub fn settings(props: &Props) -> Html {
    let message = use_state(String::new);
    let confirm = use_state(|| None::<&'static str>);
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

    html! {
      <div class="settings-panel">
        <section class="settings-section" id="settings-session">
          <h3>{"Sessione"}</h3>
          <p>{&props.session_text}{" · salvata automaticamente e ripresa alla riapertura."}</p>
          if let Some(error) = &props.save_error {<p role="alert">{format!("Salvataggio non riuscito: {error}")}</p>}
          {delete("session","Elimina sessione salvata","delete-session",{let run=run.clone();Callback::from(move |_|run(Op::Delete("session")))})}
        </section>
        <section class="settings-section" id="settings-reset">
          <h3>{"Ripartire da zero"}</h3>
          <p>{&props.reset_note}</p>
          {delete("all","Elimina tutto e riparti da zero","delete-all",{let run=run.clone();Callback::from(move |_|run(Op::Delete("all")))})}
          if !message.is_empty() {<p id="settings-message" role="status">{(*message).clone()}</p>}
        </section>
      </div>
    }
}
