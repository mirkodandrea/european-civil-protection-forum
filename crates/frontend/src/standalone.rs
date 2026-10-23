//! One document, one in-memory Exercise, multiple authorized views.
use crate::{api, App};
use gloo_timers::callback::Interval;
use std::{cell::RefCell, sync::Arc};
use yew::prelude::*;

thread_local! { static RUNTIME: RefCell<Option<server::AppState>> = const { RefCell::new(None) }; }
const STANDALONE_MODEL: &str = "openai/gpt-oss-120b";

pub fn runtime() -> Result<server::AppState, String> {
    RUNTIME
        .with(|r| r.borrow().clone())
        .ok_or_else(|| "Esercitazione non inizializzata".into())
}
fn initialize() -> Result<server::AppState, String> {
    let mut scenario: scenario::Scenario = serde_json::from_str(include_str!(
        "../../../data/scenarios/valverde_factory.json"
    ))
    .map_err(|e| e.to_string())?;
    scenario.enable_operational_model();
    let world = sim::World::with_options(
        Arc::new(scenario),
        1,
        sim::FireOptions {
            realizations: 32,
            cutoff: 0.5,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut state = server::browser::new(exercise::Exercise::new(world));
    if let Some(key) = option_env!("OPENROUTER_API_KEY").filter(|key| !key.trim().is_empty()) {
        server::browser::configure(&mut state, &key, STANDALONE_MODEL, STANDALONE_MODEL);
    }
    RUNTIME.with(|r| *r.borrow_mut() = Some(state.clone()));
    Ok(state)
}

#[function_component(Standalone)]
pub fn standalone() -> Html {
    let initialized = use_state(initialize);
    let selected = use_state(|| "master".to_string());
    let show_map = use_state(|| true);
    let all_roles = use_state(|| false);
    let split = use_state(|| "30".to_string());
    let key = use_state(String::new);
    let model = use_state(|| STANDALONE_MODEL.to_string());
    let translator = use_state(|| STANDALONE_MODEL.to_string());
    let ai_notice = use_state(|| {
        if initialized
            .as_ref()
            .is_ok_and(|state| state.llm.is_available())
        {
            "AI configurata da .env · OpenRouter".to_string()
        } else {
            "AI disattivata · modalità manuale".to_string()
        }
    });
    let revision = use_state_eq(|| 0u64);
    {
        let revision = revision.clone();
        use_effect_with((), move |_| {
            let interval = Interval::new(500, move || {
                if let Ok(s) = runtime() {
                    let g = s.lock().generation;
                    if *revision != g {
                        revision.set(g);
                    }
                }
            });
            move || drop(interval)
        });
    }
    let state = match &*initialized {
        Ok(s) => s,
        Err(e) => {
            return html! {<main class="page"><h1>{"Impossibile inizializzare l’esercitazione"}</h1><p role="alert">{e}</p></main>}
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        let exercise = state.ex.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(100).await;
                viz::run(exercise, 0, "standalone".into());
            });
            || ()
        });
    }
    let (clock, phase, roles) = {
        let ex = state.lock();
        (
            ex.world.clock(),
            format!("{:?}", ex.phase),
            ex.world.scn.roles.clone(),
        )
    };
    let onconfigure = {
        let key = key.clone();
        let model = model.clone();
        let translator = translator.clone();
        let notice = ai_notice.clone();
        Callback::from(move |_| {
            if model.trim().is_empty() || translator.trim().is_empty() {
                notice.set("Inserisci entrambi i modelli".into());
                return;
            }
            if let Ok(mut state) = runtime() {
                server::browser::configure(&mut state, &key, &model, &translator);
                notice.set(
                    if key.trim().is_empty() {
                        "AI disattivata · modalità manuale"
                    } else {
                        "AI configurata · connessione diretta a OpenRouter"
                    }
                    .into(),
                );
                key.set(String::new());
            }
        })
    };
    let onclear = {
        let key = key.clone();
        let notice = ai_notice.clone();
        Callback::from(move |_| {
            if let Ok(mut s) = runtime() {
                server::browser::configure(
                    &mut s,
                    "",
                    llm::DEFAULT_MODEL,
                    llm::DEFAULT_TRANSLATION_MODEL,
                );
            }
            key.set(String::new());
            notice.set("AI disattivata · chiave rimossa".into());
        })
    };
    let mut panels = vec![("master".to_string(), "EXCON".to_string(), "chat")];
    for role in &roles {
        panels.push((role.id.clone(), role.name.clone(), "chat"));
    }
    html! {
      <div class="standalone">
        <header class="standalone-header"><div><div class="eyebrow">{"PPRD East 4 · laboratorio locale"}</div><h1>{"Esercitazione nel browser"}</h1><p>{format!("{clock} · {phase} · 32 realizzazioni")}</p></div><span class="pill">{"WebAssembly · nessun backend"}</span>
          <label><input type="checkbox" checked={*show_map} onchange={{let show=show_map.clone();Callback::from(move |_|show.set(!*show))}}/>{" Simulazione"}</label>
          <label><input type="checkbox" checked={*all_roles} onchange={{let all=all_roles.clone();Callback::from(move |_|all.set(!*all))}}/>{" Tutti i player"}</label>
          <label class="split-control">{"Spazio simulazione"}<input aria-label="Spazio simulazione" type="range" min="20" max="70" value={(*split).clone()} oninput={{let split=split.clone();Callback::from(move |e|split.set(api::input(e)))}}/></label>
        </header>
        <details class="standalone-settings"><summary>{"OpenRouter · "}{&*ai_notice}</summary>
          <p>{"La chiave resta in memoria in questa pagina. Nessun salvataggio automatico; ricaricare la pagina avvia una nuova esercitazione. Il debrief è esportabile dagli strumenti EXCON."}</p>
          <div class="settings-fields"><label>{"Chiave API"}<input id="openrouter-key" type="password" autocomplete="off" value={(*key).clone()} oninput={{let key=key.clone();Callback::from(move |e|key.set(api::input(e)))}}/></label>
          <label>{"Modello interpretazione"}<input id="openrouter-model" value={(*model).clone()} oninput={{let model=model.clone();Callback::from(move |e|model.set(api::input(e)))}}/></label>
          <label>{"Modello traduzione"}<input id="openrouter-translator" value={(*translator).clone()} oninput={{let translator=translator.clone();Callback::from(move |e|translator.set(api::input(e)))}}/></label>
          <button id="configure-ai" onclick={onconfigure}>{"Applica"}</button><button id="clear-ai" onclick={onclear}>{"Rimuovi chiave"}</button></div>
          <p>{"L’AI richiede Internet e contatta direttamente OpenRouter. Tutte le regole, la simulazione e le approvazioni sono eseguite localmente."}</p>
        </details>
        <div class={classes!("standalone-content",(!*show_map).then_some("without-sim"))} style={format!("--sim-width:{}%",*split)}>
          <aside class="standalone-simulation" hidden={!*show_map}><div class="simulation-heading"><h2>{"Situazione simulata"}</h2><p>{"Vista 3D completa EXCON · le mappe dei player mostrano solo le informazioni ricevute"}</p></div><div class="standalone-3d-frame"><canvas id="standalone-3d" aria-label="Visualizzatore 3D della situazione simulata"/></div></aside>
          <section class="standalone-views"><nav class="role-tabs" aria-label="Ruoli dell’esercitazione">{for panels.iter().map(|(id,name,_)|{
            let id=id.clone();html!{<button data-role={id.clone()} class={classes!((*selected==id).then_some("active"))} onclick={{let selected=selected.clone();let all=all_roles.clone();Callback::from(move |_|{selected.set(id.clone());all.set(false);})}}>{name}</button>}
          })}</nav>
          <div class={classes!("role-panels",(*all_roles).then_some("all-roles"))}>{for panels.iter().map(|(id,_,page)|{
             let visible = if *all_roles { roles.iter().any(|r|r.id==*id) } else { *selected==*id };
             html!{<section class={classes!("role-panel",(id!="master"&&id!="advanced"&&id!="history"&&id!="evaluator").then_some("player-ui"))} data-panel={id.clone()} key={id.clone()} hidden={!visible}><App role={Some(id.clone())} page={Some(page.to_string())}/></section>}
          })}</div></section>
        </div>
      </div>
    }
}
