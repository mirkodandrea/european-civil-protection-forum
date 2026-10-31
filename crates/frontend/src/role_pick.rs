//! `player.html` opened without `?role=`: list the player roles to choose
//! from (in English: the role, and so its language, is not known yet),
//! instead of falling back to the EXCON identity of the server UI.
use serde_json::Value;
use yew::prelude::*;

use crate::api::{self, list, text};

#[function_component(RolePick)]
pub fn role_pick() -> Html {
    let data = use_state(|| None::<Result<(Value, Value), String>>);
    {
        let data = data.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                // The room is trusted and has no keys: the links come from the EXCON list.
                let result = async {
                    let stat = api::request("master", "/api/static", None).await?;
                    let links = api::request("master", "/api/master/access", None).await?;
                    Ok((stat, links))
                }
                .await;
                data.set(Some(result));
            });
            || ()
        });
    }
    let body = match &*data {
        None => html! {<p>{crate::i18n::ex("Connessione all’esercitazione…")}</p>},
        Some(Err(e)) => html! {<p class="error" role="alert">{e.clone()}</p>},
        Some(Ok((stat, links))) => {
            let name = |id: &str| {
                list(&stat["roles"])
                    .iter()
                    .chain(list(&stat["contacts"]))
                    .find(|r| r["id"] == id)
                    .map(|r| text(&r["name"]).to_string())
                    .unwrap_or_else(|| id.to_string())
            };
            html! {<>
                <h2>{text(&stat["name"])}</h2>
                <p>{"Choose your role"}</p>
                <div id="links">{for list(&links["players"]).iter().map(|p| {
                    html! {<p><a href={text(&p["url"]).to_string()}>{name(text(&p["role"]))}</a></p>}
                })}</div>
            </>}
        }
    };
    html! {<main class="page"><section class="card">{body}</section></main>}
}
