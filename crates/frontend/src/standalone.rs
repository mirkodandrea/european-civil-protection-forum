//! One document, one in-memory Exercise, multiple authorized views.
use crate::{api, App};
use gloo_timers::callback::Interval;
use std::cell::RefCell;
use yew::prelude::*;

thread_local! { static RUNTIME: RefCell<Option<server::AppState>> = const { RefCell::new(None) }; }
const API_KEY_STORAGE: &str = crate::store::API_KEY;

fn key_storage() -> Result<web_sys::Storage, ()> {
    web_sys::window()
        .ok_or(())?
        .local_storage()
        .map_err(|_| ())?
        .ok_or(())
}

fn saved_key() -> Option<String> {
    key_storage()
        .ok()?
        .get_item(API_KEY_STORAGE)
        .ok()
        .flatten()
        .filter(|key| !key.trim().is_empty())
}

fn persist_key(key: &str) -> Result<(), ()> {
    let storage = key_storage()?;
    if key.trim().is_empty() {
        storage.remove_item(API_KEY_STORAGE)
    } else {
        storage.set_item(API_KEY_STORAGE, key.trim())
    }
    .map_err(|_| ())
}

const STANDALONE_MODEL: &str = llm::DEFAULT_MODEL;

/// Comma list from the settings field; empty lets OpenRouter choose the host.
fn parse_providers(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

pub fn runtime() -> Result<server::AppState, String> {
    RUNTIME
        .with(|r| r.borrow().clone())
        .ok_or_else(|| "Esercitazione non inizializzata".into())
}
/// Make `state` the exercise every view of this page talks to (the simulator bench).
pub fn install(state: server::AppState) {
    RUNTIME.with(|r| *r.borrow_mut() = Some(state));
}
fn initialize() -> Result<server::AppState, String> {
    let scenarios = crate::store::scenarios();
    // A saved session continues with the scenario it started from; a new one
    // takes the chosen scenario (the bare territory, unless the EXCON picked another).
    let session = crate::store::load_session();
    let pack = session
        .as_ref()
        .map(|s| s.pack.clone())
        .or_else(|| scenarios.next_pack())
        .ok_or("nessuno scenario disponibile")?;
    let mut exercise = (scenarios.build)(&pack).map_err(|e| e.to_string())?;
    // Every new exercise asks each participant for a language again (the
    // scenario's is proposed), whoever plays the role this time.
    let language_choice = session.as_ref().is_none_or(|s| s.language_choice);
    if language_choice {
        exercise = exercise.with_language_choice();
    }
    crate::store::set_language_choice(language_choice);
    scenarios.set_running(&pack);
    let mut state = server::browser::new(exercise, Some(scenarios));
    if let Some(key) = saved_key().or_else(|| {
        option_env!("OPENROUTER_API_KEY")
            .filter(|key| !key.trim().is_empty())
            .map(str::to_string)
    }) {
        let providers: Vec<String> = llm::DEFAULT_PROVIDERS.iter().map(|p| p.to_string()).collect();
        server::browser::configure(&mut state, &key, STANDALONE_MODEL, STANDALONE_MODEL, &providers);
    }
    RUNTIME.with(|r| *r.borrow_mut() = Some(state.clone()));
    if let Some(s) = &session {
        server::browser::load_tape(s.tape.clone());
    }
    crate::store::set_pending(session);
    Ok(state)
}

/// Commit, CI build number and build date stamped by `build.rs`.
fn build_label() -> String {
    format!("{} · build {} · {}", env!("TTX_BUILD_COMMIT"), env!("TTX_BUILD_NUMBER"), env!("TTX_BUILD_DATE"))
}

/// Opening screen: pick the scenario (or resume the saved session) and set
/// the options before the exercise and its interface are built. The EXCON
/// console language is chosen here; each role chooses its own in its tab.
#[function_component(Standalone)]
pub fn standalone() -> Html {
    let launched = use_state(crate::store::take_autostart);
    if *launched {
        return html! {<Session/>};
    }
    let onlaunch = {
        let launched = launched.clone();
        Callback::from(move |()| launched.set(true))
    };
    html! {<Launcher {onlaunch}/>}
}

#[derive(Properties, PartialEq)]
struct LauncherProps {
    onlaunch: Callback<()>,
}

const EXCON_LANG_KEY: &str = "ttx_excon_lang";

#[function_component(Launcher)]
fn launcher(props: &LauncherProps) -> Html {
    use server::library::ScenarioLibrary;
    let library = crate::store::BrowserLibrary;
    let packs = use_memo((), |_| crate::store::BrowserLibrary.list());
    let saved = use_memo((), |_| {
        crate::store::get(crate::store::SESSION_KEY)
            .is_some()
            .then(crate::store::load_session)
            .flatten()
            .map(|s| (s.pack.name.clone(), s.journal.len()))
    });
    let territories = use_memo((), |_| {
        let own = crate::store::authored_territories();
        let mut all: Vec<(String, String, bool)> = scenario::embedded::TERRITORIES
            .iter()
            .filter(|t| !own.contains_key(t.id))
            .map(|t| (t.id.to_string(), t.name.to_string(), false))
            .collect();
        all.extend(own.values().map(|s| (s.id.clone(), s.name.clone(), true)));
        all
    });
    let selected = use_state(|| {
        library
            .active()
            .unwrap_or_else(|| scenario::embedded::DEFAULT_TERRITORY.to_string())
    });
    let excon_lang = use_state(|| {
        crate::store::get(EXCON_LANG_KEY)
            .filter(|l| api::EXCON_LANGUAGES.iter().any(|(id, _)| id == l))
            .unwrap_or_else(|| "it".into())
    });
    let key = use_state(String::new);
    let key_notice = use_state(|| {
        if saved_key().is_some() {
            "Chiave salvata nel browser"
        } else if option_env!("OPENROUTER_API_KEY").is_some_and(|k| !k.trim().is_empty()) {
            "Chiave da .env"
        } else {
            "Nessuna chiave · AI disattivata, flusso manuale EXCON"
        }
        .to_string()
    });
    let notice = use_state(crate::store::take_notice);
    let error = use_state(String::new);
    let resume = {
        let onlaunch = props.onlaunch.clone();
        Callback::from(move |_| onlaunch.emit(()))
    };
    let start = {
        let (onlaunch, selected, error) = (props.onlaunch.clone(), selected.clone(), error.clone());
        Callback::from(move |_| match crate::store::choose(&selected) {
            Ok(()) => onlaunch.emit(()),
            Err(e) => error.set(e),
        })
    };
    let pick_lang = |id: &'static str| {
        let excon_lang = excon_lang.clone();
        Callback::from(move |_| {
            let _ = crate::store::set(EXCON_LANG_KEY, id);
            excon_lang.set(id.to_string());
        })
    };
    let save_key = {
        let (key, notice) = (key.clone(), key_notice.clone());
        Callback::from(move |_| {
            notice.set(
                match (persist_key(&key), key.trim().is_empty()) {
                    (Err(()), _) => "Archiviazione browser non disponibile",
                    (Ok(()), true) => "Chiave rimossa · AI disattivata",
                    (Ok(()), false) => "Chiave salvata nel browser",
                }
                .into(),
            );
            key.set(String::new());
        })
    };
    let clear_key = {
        let (key, notice) = (key.clone(), key_notice.clone());
        Callback::from(move |_| {
            let _ = persist_key("");
            key.set(String::new());
            notice.set("Chiave rimossa · AI disattivata".into());
        })
    };
    html! {
      <div class="standalone launcher">
        <header class="standalone-header"><div><div class="eyebrow">{"PPRD East 4 · laboratorio locale"}</div><h1>{"Esercitazione nel browser"}</h1><p>{"Scegli lo scenario e le opzioni, poi avvia."}</p></div><span class="pill">{"WebAssembly · nessun backend"}</span><span class="pill build-version" id="build-version" title="Commit · build · data di build">{build_label()}</span></header>
        if let Some(text) = &*notice {<p class="standalone-notice" role="status">{text}<button onclick={{let notice=notice.clone();Callback::from(move |_|notice.set(None))}}>{"Chiudi"}</button></p>}
        if !error.is_empty() {<p class="error" role="alert">{&*error}</p>}
        <main class="page launcher-page">
          if let Some((name, steps)) = &*saved {
            <section class="card" id="launch-resume"><h2>{"Sessione salvata"}</h2>
              <p>{format!("{name} · {steps} passi registrati")}</p>
              <button id="resume-session" class="primary" onclick={resume}>{"Riprendi sessione"}</button>
            </section>
          }
          <section class="card" id="launch-scenario"><h2>{"Nuova esercitazione"}</h2>
            <div class="launcher-scenarios" role="radiogroup" aria-label="Scenario">{for packs.iter().map(|p| {
              let id = p.id.clone();
              html!{<label class={classes!("launcher-scenario",(*selected==p.id).then_some("active"))}><input type="radio" name="launch-scenario" value={p.id.clone()} checked={*selected==p.id} onchange={{let selected=selected.clone();Callback::from(move |_|selected.set(id.clone()))}}/><span><strong>{&p.name}</strong>if !p.description.is_empty(){<br/><span class="muted">{&p.description}</span>}</span></label>}
            })}</div>
            <p class="muted">{"Ogni partecipante sceglie la propria lingua nella scheda del ruolo; quella dello scenario è proposta."}if saved.is_some(){{" La sessione salvata verrà eliminata."}}</p>
            <button id="start-session" class={classes!(saved.is_none().then_some("primary"))} onclick={start}>{"Avvia nuova esercitazione"}</button>
          </section>
          <section class="card" id="launch-territories"><h2>{"Territori"}</h2>
            <p class="muted">{"Modifica la mappa di un territorio (terreno, vegetazione, edifici, strade, luoghi) nell’editor 3D. I territori inclusi si aprono come copia; il risultato compare tra gli scenari."}</p>
            <ul class="launcher-territories">{for territories.iter().map(|(id, name, own)| html!{
              <li><span><strong>{name}</strong>if *own {{" · creato qui"}}</span><a class="button" id={format!("edit-territory-{id}")} href={format!("?editor=1&territory={id}")}>{if *own {"Modifica"} else {"Modifica copia"}}</a></li>
            })}</ul>
            <a class="button" id="new-territory" href="?editor=1">{"Nuovo territorio vuoto"}</a>
          </section>
          <section class="card" id="launch-options"><h2>{"Opzioni"}</h2>
            <h3>{"Lingua della console EXCON"}</h3>
            <div class="row" id="launch-excon-lang">{for api::EXCON_LANGUAGES.iter().map(|(id,name)| html!{<button data-lang={*id} class={classes!((*excon_lang==*id).then_some("primary"))} onclick={pick_lang(id)}>{*name}</button>})}</div>
            <h3>{"AI (OpenRouter)"}</h3>
            <p class="muted">{&*key_notice}</p>
            <div class="settings-fields"><label>{"Chiave API"}<input id="launch-openrouter-key" type="password" autocomplete="off" value={(*key).clone()} oninput={{let key=key.clone();Callback::from(move |e|key.set(api::input(e)))}}/></label>
              <button id="launch-save-key" onclick={save_key}>{"Salva chiave"}</button><button id="launch-clear-key" onclick={clear_key}>{"Rimuovi chiave"}</button></div>
            <p class="muted">{"Modelli e provider si regolano in Impostazioni dopo l’avvio."}</p>
            <crate::settings::Settings session_text={match crate::store::session_summary(){Some((kb,steps))=>format!("Sessione salvata: {steps} passi · {kb} kB"),None=>"Nessuna sessione salvata".to_string()}} save_error={None::<String>} reset_note="Elimina sessione, scenari salvati e chiave API di questo browser." backend={crate::store::backend()} revision={0}/>
          </section>
        </main>
      </div>
    }
}

/// The exercise and its interface, built once launched.
#[function_component(Session)]
fn session() -> Html {
    let initialized = use_state(initialize);
    let selected = use_state(|| "master".to_string());
    let show_map = use_state(|| false);
    let all_roles = use_state(|| false);
    let split = use_state(|| "30".to_string());
    let key = use_state(String::new);
    let model = use_state(|| STANDALONE_MODEL.to_string());
    let translator = use_state(|| STANDALONE_MODEL.to_string());
    let providers = use_state(|| llm::DEFAULT_PROVIDERS.join(", "));
    let ai_notice = use_state(|| {
        if initialized
            .as_ref()
            .is_ok_and(|state| state.llm.is_available())
        {
            if saved_key().is_some() {
                "AI configurata · chiave salvata nel browser".to_string()
            } else {
                "AI configurata da .env · OpenRouter".to_string()
            }
        } else {
            "AI disattivata · modalità manuale".to_string()
        }
    });
    let revision = use_state_eq(|| 0u64);
    let restoring = use_state(crate::store::restoring);
    let notice = use_state(crate::store::take_notice);
    let backend = use_memo((), |_| crate::store::backend());
    {
        // Replay the saved session into the fresh exercise, then start autosaving.
        let restoring = restoring.clone();
        let state = initialized.as_ref().ok().cloned();
        use_effect_with((), move |_| {
            if let (Some(state), Some(saved)) = (state.clone(), crate::store::take_pending()) {
                wasm_bindgen_futures::spawn_local(async move {
                    match server::browser::replay(state, saved.journal, saved.end_t_s).await {
                        Ok(()) => {
                            crate::store::finish_restore();
                            restoring.set(false);
                        }
                        Err(e) => {
                            // The state would differ from the saved one: start clean.
                            crate::store::freeze();
                            crate::store::remove(crate::store::SESSION_KEY);
                            crate::store::set_notice(&format!(
                                "La sessione salvata non è più riproducibile ed è stata scartata ({e})."
                            ));
                            let _ = api::window().location().reload();
                        }
                    }
                });
            }
            let interval = state.map(|state| {
                Interval::new(1000, move || crate::store::autosave(&state, false))
            });
            move || drop(interval)
        });
    }
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
        let started = use_mut_ref(|| false);
        use_effect_with(*show_map, move |visible| {
            if *visible && !*started.borrow() {
                *started.borrow_mut() = true;
                wasm_bindgen_futures::spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(100).await;
                    viz::run(
                        Box::new(server::view::ExerciseSource(exercise)),
                        viz::Info {
                            llm_model: "standalone".into(),
                            ..Default::default()
                        },
                    );
                });
            }
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
        let providers = providers.clone();
        let notice = ai_notice.clone();
        Callback::from(move |_| {
            if model.trim().is_empty() || translator.trim().is_empty() {
                notice.set("Inserisci entrambi i modelli".into());
                return;
            }
            if let Ok(mut state) = runtime() {
                server::browser::configure(&mut state, &key, &model, &translator, &parse_providers(&providers));
                let saved = persist_key(&key).is_ok();
                notice.set(
                    if !saved {
                        "Archiviazione browser non disponibile · modifica valida solo per questa pagina"
                    } else if key.trim().is_empty() {
                        "AI disattivata · chiave rimossa"
                    } else {
                        "AI configurata · chiave salvata nel browser"
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
                    &[],
                );
            }
            key.set(String::new());
            notice.set(
                if persist_key("").is_ok() {
                    "AI disattivata · chiave rimossa"
                } else {
                    "AI disattivata · impossibile rimuovere la chiave dal browser"
                }
                .into(),
            );
        })
    };
    let mut panels = vec![("master".to_string(), "EXCON".to_string(), "chat")];
    for role in &roles {
        panels.push((role.id.clone(), role.name.clone(), "chat"));
    }
    let settings_html = html! {<>
        <p class="muted">{"AI · "}{&*ai_notice}</p>
          <p>{"La chiave viene salvata nel browser su questo dispositivo e riutilizzata alle prossime aperture. “Rimuovi chiave” elimina la chiave salvata. La sessione viene salvata nel browser e ripresa al ricaricamento della pagina. Il debrief è esportabile dagli strumenti EXCON."}</p>
          <div class="settings-fields"><label>{"Chiave API"}<input id="openrouter-key" type="password" autocomplete="off" value={(*key).clone()} oninput={{let key=key.clone();Callback::from(move |e|key.set(api::input(e)))}}/></label>
          <label>{"Modello interpretazione"}<input id="openrouter-model" value={(*model).clone()} oninput={{let model=model.clone();Callback::from(move |e|model.set(api::input(e)))}}/></label>
          <label>{"Modello traduzione"}<input id="openrouter-translator" value={(*translator).clone()} oninput={{let translator=translator.clone();Callback::from(move |e|translator.set(api::input(e)))}}/></label>
          <label>{"Provider OpenRouter (separati da virgola, vuoto = scelta automatica)"}<input id="openrouter-providers" value={(*providers).clone()} oninput={{let providers=providers.clone();Callback::from(move |e|providers.set(api::input(e)))}}/></label>
          <button id="configure-ai" onclick={onconfigure}>{"Applica"}</button><button id="clear-ai" onclick={onclear}>{"Rimuovi chiave"}</button></div>
          <p>{"L’AI richiede Internet e contatta direttamente OpenRouter. Tutte le regole, la simulazione e le approvazioni sono eseguite localmente."}</p>
          <crate::settings::Settings session_text={match crate::store::session_summary(){Some((kb,steps))=>format!("Sessione salvata: {steps} passi · {kb} kB"),None=>"Nessuna sessione salvata".to_string()}} save_error={crate::store::save_error()} reset_note="Elimina sessione, scenari salvati e chiave API di questo browser." backend={(*backend).clone()} revision={*revision}/>
    </>};
    html! {
      <ContextProvider<crate::scenario_editor::SettingsSlot> context={crate::scenario_editor::SettingsSlot(settings_html)}>
      <div class="standalone">
        <header class="standalone-header"><div><div class="eyebrow">{"PPRD East 4 · laboratorio locale"}</div><h1>{"Esercitazione nel browser"}</h1><p>{format!("{clock} · {phase} · 32 realizzazioni")}</p></div><span class="pill">{"WebAssembly · nessun backend"}</span><span class="pill build-version" id="build-version" title="Commit · build · data di build">{build_label()}</span>
          <label><input type="checkbox" checked={*show_map} onchange={{let show=show_map.clone();Callback::from(move |_|show.set(!*show))}}/>{" Simulazione"}</label>
          <label><input type="checkbox" checked={*all_roles} onchange={{let all=all_roles.clone();Callback::from(move |_|all.set(!*all))}}/>{" Tutti i player"}</label>
          <label class="split-control">{"Spazio simulazione"}<input aria-label="Spazio simulazione" type="range" min="20" max="70" value={(*split).clone()} oninput={{let split=split.clone();Callback::from(move |e|split.set(api::input(e)))}}/></label>
        </header>
        if let Some(text) = &*notice {<p class="standalone-notice" role="status">{text}<button onclick={{let notice=notice.clone();Callback::from(move |_|notice.set(None))}}>{"Chiudi"}</button></p>}
        if *restoring {<p class="standalone-notice" role="status" id="restoring">{"Ripristino della sessione salvata…"}</p>} else {
        <div class={classes!("standalone-content",(!*show_map).then_some("without-sim"))} style={format!("--sim-width:{}%",*split)}>
          <aside class="standalone-simulation" hidden={!*show_map}><div class="simulation-heading"><h2>{"Situazione simulata"}</h2><p>{"Vista 3D completa EXCON · le mappe dei player mostrano solo le informazioni ricevute"}</p></div><div class="standalone-3d-frame"><canvas id="standalone-3d" aria-label="Visualizzatore 3D della situazione simulata"/></div></aside>
          <section class="standalone-views"><nav class="role-tabs" aria-label="Ruoli dell’esercitazione">{for panels.iter().map(|(id,name,_)|{
            let id=id.clone();html!{<button data-role={id.clone()} class={classes!((*selected==id).then_some("active"))} onclick={{let selected=selected.clone();let all=all_roles.clone();Callback::from(move |_|{selected.set(id.clone());all.set(false);})}}>{name}</button>}
          })}</nav>
          <div class={classes!("role-panels",(*all_roles).then_some("all-roles"))}>{for panels.iter().map(|(id,_,page)|{
             let visible = if *all_roles { roles.iter().any(|r|r.id==*id) } else { *selected==*id };
             html!{<section class={classes!("role-panel",(id!="master"&&id!="advanced"&&id!="history"&&id!="evaluator").then_some("player-ui"))} data-panel={id.clone()} key={id.clone()} hidden={!visible}><App visible={visible} role={Some(id.clone())} page={Some(page.to_string())}/></section>}
          })}</div></section>
        </div>
        }
      </div>
      </ContextProvider<crate::scenario_editor::SettingsSlot>>
    }
}
