//! EXCON settings of the server UI: the saved session and the inject library
//! live in the server's state directory (`ttx --state-dir`).
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct Props {
    /// Exercise generation; a change reloads what is stored.
    pub revision: u64,
}

#[cfg(feature = "standalone")]
#[function_component(ServerSettings)]
pub fn server_settings(_: &Props) -> Html {
    html! {}
}

#[cfg(not(feature = "standalone"))]
#[function_component(ServerSettings)]
pub fn server_settings(props: &Props) -> Html {
    use crate::{api, settings};
    use scenario::InjectTemplate;
    use serde_json::Value;
    let info = use_state(|| None::<Value>);
    let refresh = {
        let info = info.clone();
        use_callback((), move |(), _| {
            let info = info.clone();
            wasm_bindgen_futures::spawn_local(async move {
                info.set(api::request(&api::token(), "/api/master/storage", None).await.ok());
            });
        })
    };
    {
        let refresh = refresh.clone();
        use_effect_with(props.revision, move |_| {
            refresh.emit(());
            || ()
        });
    }
    let backend = {
        let refresh = refresh.clone();
        use_memo((), move |_| {
            settings::Backend(std::rc::Rc::new(move |op| {
                let refresh = refresh.clone();
                Box::pin(async move {
                    let (path, body, done) = match op {
                        settings::Op::SaveInjects(list) => (
                            "/api/master/storage/injects",
                            serde_json::json!({"injects": list}),
                            "Injects salvati sul server · valgono dalla prossima nuova sessione",
                        ),
                        settings::Op::NewSession(list) => (
                            "/api/master/storage/injects",
                            serde_json::json!({"injects": list, "new_session": true}),
                            "Nuova sessione avviata con questi injects",
                        ),
                        settings::Op::Delete(target) => (
                            "/api/master/storage/delete",
                            serde_json::json!({"target": target}),
                            "Eliminato",
                        ),
                    };
                    let result = api::request(&api::token(), path, Some(body)).await;
                    refresh.emit(());
                    result.map(|_| done.to_string())
                })
            }))
        })
    };
    let Some(info) = &*info else {
        return html! {<section class="card" id="server-settings"><h2>{"Impostazioni · archiviazione"}</h2><p>{"Caricamento…"}</p></section>};
    };
    if info["enabled"] != true {
        return html! {<section class="card" id="server-settings"><h2>{"Impostazioni · archiviazione"}</h2><p>{"Archiviazione disattivata (server avviato con --no-state): la sessione non viene salvata."}</p></section>};
    }
    let injects = |v: &Value| serde_json::from_value::<Vec<InjectTemplate>>(v.clone()).ok();
    let roles: Vec<String> = serde_json::from_value(info["roles"].clone()).unwrap_or_default();
    let session_text = match (info["session"]["steps"].as_u64(), info["session"]["bytes"].as_u64()) {
        (Some(steps), Some(bytes)) => format!("Sessione salvata: {steps} passi · {} kB", bytes.div_ceil(1024)),
        _ => "Nessuna sessione salvata".to_string(),
    };
    html! {<section class="card standalone-settings" id="server-settings"><h2>{"Impostazioni · archiviazione"}</h2>
        <crate::settings::Settings {roles} defaults={injects(&info["defaults"]).unwrap_or_default()} library={injects(&info["library"])} {session_text}
            save_error={info["error"].as_str().map(str::to_string)}
            reset_note="Elimina sessione e libreria injects salvate sul server e riparte da zero. Vale per tutti i partecipanti."
            backend={(*backend).clone()} revision={props.revision}/>
    </section>}
}
