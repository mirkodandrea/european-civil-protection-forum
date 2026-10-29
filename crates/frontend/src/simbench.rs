//! Simulator bench (`?simbench=1`, standalone only): the 3D simulator on the
//! left, the EXCON Controllo console on the right. The clock runs, pauses or
//! jumps; every EXCON tool applies at once, also while the clock runs.
//! Nothing is saved: the TTX session stored in this browser stays untouched.
//! See `docs/banco-simulatore.md`.
use crate::{api, App};
use server::library::ScenarioLibrary;
use yew::prelude::*;

pub fn requested() -> bool {
    params().get("simbench").is_some()
}

fn params() -> web_sys::UrlSearchParams {
    let href = api::window().location().href().unwrap_or_default();
    web_sys::Url::new(&href)
        .map(|u| u.search_params())
        .unwrap_or_else(|_| web_sys::UrlSearchParams::new().unwrap())
}

/// Simulated seconds per real second offered by the clock.
const RATIOS: [f64; 6] = [1.0, 10.0, 30.0, 60.0, 300.0, 900.0];

/// A fresh exercise of scenario `id` (else the default one), turn 1 open,
/// installed as the exercise the EXCON views of this page talk to.
fn build(id: Option<String>) -> Result<(server::AppState, String), String> {
    // The bench never writes the saved session of the TTX.
    crate::store::freeze();
    let scenarios = crate::store::scenarios();
    let pack = id
        .and_then(|id| crate::store::BrowserLibrary.load(&id))
        .or_else(|| scenarios.next_pack())
        .ok_or("nessuno scenario disponibile")?;
    let mut ex = (scenarios.build)(&pack).map_err(|e| e.to_string())?;
    ex.start().map_err(|e| e.to_string())?;
    scenarios.set_running(&pack);
    let state = server::browser::new(ex, Some(scenarios));
    crate::standalone::install(state.clone());
    Ok((state, pack.id))
}

/// The clock as the bar shows it, read under a short lock.
#[derive(Clone, PartialEq, Default)]
struct Clock {
    text: String,
    running: bool,
    ratio: f64,
    lag_s: i64,
}

fn clock(s: &server::AppState) -> Clock {
    let ex = s.lock();
    Clock {
        text: ex.world.clock(),
        running: ex.clock_control.running,
        ratio: ex.clock_control.ratio,
        lag_s: ex.clock_control.lag_s,
    }
}

#[function_component(SimBench)]
pub fn sim_bench() -> Html {
    let built = use_memo((), |_| build(params().get("scn")));
    match &*built {
        Ok((state, pack_id)) => html! {<Bench state={state.clone()} pack_id={pack_id.clone()}/>},
        Err(e) => html! {<main class="page"><h1>{"Banco simulatore"}</h1><p role="alert" class="error">{e}</p></main>},
    }
}

#[derive(Properties)]
struct BenchProps {
    state: server::AppState,
    pack_id: String,
}
impl PartialEq for BenchProps {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.state.ex, &other.state.ex)
    }
}

#[function_component(Bench)]
fn bench(p: &BenchProps) -> Html {
    let state = p.state.clone();
    let now = use_state_eq(|| clock(&state));
    let busy = use_state(|| false);
    let error = use_state(String::new);
    let packs = use_memo((), |_| crate::store::BrowserLibrary.list());
    {
        let (state, now) = (state.clone(), now.clone());
        use_effect_with((), move |_| {
            let interval = gloo_timers::callback::Interval::new(250, move || now.set(clock(&state)));
            move || drop(interval)
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let exercise = state.ex.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(100).await;
                viz::run(
                    Box::new(server::view::ExerciseSource(exercise)),
                    viz::Info { llm_model: "banco simulatore".into(), ..Default::default() },
                );
            });
            || ()
        });
    }
    let c = (*now).clone();
    let set_clock = |running: bool, ratio: f64| {
        let (state, error) = (state.clone(), error.clone());
        Callback::from(move |_| {
            let r = state.lock().configure_clock(running, ratio, "Simulator bench");
            error.set(r.err().map(|e| e.to_string()).unwrap_or_default());
        })
    };
    let on_ratio = {
        let (state, running, error) = (state.clone(), c.running, error.clone());
        Callback::from(move |e: Event| {
            if let Ok(r) = api::select(e).parse::<f64>() {
                let r = state.lock().configure_clock(running, r, "Simulator bench");
                error.set(r.err().map(|e| e.to_string()).unwrap_or_default());
            }
        })
    };
    let step = |minutes: u32| {
        let (state, busy, error) = (state.clone(), busy.clone(), error.clone());
        Callback::from(move |_| {
            busy.set(true);
            let (state, busy, error) = (state.clone(), busy.clone(), error.clone());
            // One simulated minute per tick, so the 3D view follows the jump.
            wasm_bindgen_futures::spawn_local(async move {
                for _ in 0..minutes {
                    let r = state.lock().direct_command(&sim::Command::AdvanceTime { minutes: 1 });
                    if let Err(e) = r {
                        error.set(e.to_string());
                        break;
                    }
                    gloo_timers::future::TimeoutFuture::new(0).await;
                }
                busy.set(false);
            });
        })
    };
    let reload = |id: String| {
        let _ = api::window().location().set_search(&format!("simbench=1&scn={id}"));
    };
    let on_scenario = Callback::from(move |e: Event| reload(api::select(e)));
    let restart = {
        let id = p.pack_id.clone();
        Callback::from(move |_| reload(id.clone()))
    };
    let ratio_label = |r: f64| if r >= 60.0 { format!("{r:.0}× · {:.0} min/s", r / 60.0) } else { format!("{r:.0}×") };
    html! {
      <div class="simbench" data-simbench="">
        <section class="simbench-sim"><canvas id="standalone-3d" aria-label="Simulatore 3D"/></section>
        <aside class="simbench-panel">
          <header class="simbench-head" data-simbench-clock-controls="">
            <div class="eyebrow">{"Banco simulatore"}</div>
            <strong class="simbench-clock" data-simbench-clock="">{&c.text}</strong>
            <small class="muted">{if c.running {format!("in corso{}", if c.lag_s > 0 {format!(" · ritardo {} s", c.lag_s)} else {String::new()})} else {"in pausa".into()}}</small>
            if c.running {<button class="primary" data-simbench-pause="" onclick={set_clock(false, c.ratio)}>{"⏸ Pausa"}</button>}
            else {<button class="primary" data-simbench-play="" onclick={set_clock(true, c.ratio)}>{"▶ Avvia"}</button>}
            <select aria-label="Velocità" onchange={on_ratio}>{for RATIOS.iter().map(|x| html!{<option value={x.to_string()} selected={(*x - c.ratio).abs() < 1e-6}>{ratio_label(*x)}</option>})}</select>
            {for [1u32, 5, 15, 60].iter().map(|m| html!{<button data-simbench-step={m.to_string()} disabled={*busy || c.running} onclick={step(*m)}>{format!("+{m} min")}</button>})}
            <span class="grow"/>
            <select aria-label="Scenario" onchange={on_scenario}>{for packs.iter().map(|x| html!{<option value={x.id.clone()} selected={x.id==p.pack_id}>{&x.name}</option>})}</select>
            <button onclick={restart} title="Riparte dallo stesso scenario">{"Ricomincia"}</button>
          </header>
          if !error.is_empty() {<p class="error" role="alert">{&*error}</p>}
          <section class="role-panel simbench-console" data-panel="master"><App visible={true} role={Some("master".to_string())} page={Some("advanced".to_string())}/></section>
        </aside>
      </div>
    }
}
