//! EXCON "Situazione" tab: impact now, deterministic preview of the end of the
//! turn, and alerts on what happens if nobody acts (situational awareness).
//! From an alert the EXCON may write an inject by hand; nothing here sends or
//! schedules by itself.
use crate::{
    api::{self, list, text},
    i18n::{ex, is_en},
    map::SituationMap,
    timeline::hhmm,
};
use gloo_timers::callback::Interval;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use yew::prelude::*;

fn token() -> String {
    #[cfg(feature = "standalone")]
    return "master".into();
    #[cfg(not(feature = "standalone"))]
    api::token()
}

async fn call(path: &str, body: Option<Value>) -> Result<Value, String> {
    api::request(&token(), path, body).await
}

/// Italian or English text of a `{it, en}` pair, by console language.
fn lt(v: &Value) -> &str {
    text(if is_en() { &v["en"] } else { &v["it"] })
}

fn signed(n: i64) -> String {
    if n > 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}

fn state_label(s: &str) -> &'static str {
    match s {
        "threatened" => ex("minacciato"),
        "damaged" => ex("danneggiato"),
        "destroyed" => ex("distrutto"),
        "cut" => ex("tagliata"),
        "closed" => ex("chiusa"),
        _ => ex("integro"),
    }
}

fn category_label(c: &str) -> String {
    match c {
        "house" => ex("abitazione").into(),
        "school" => ex("scuola").into(),
        "hospital" => ex("ospedale").into(),
        "fuel_station" => ex("distributore").into(),
        "sports_centre" => ex("centro sportivo").into(),
        "town_hall" => ex("municipio").into(),
        "industrial" => ex("industria").into(),
        other => other.into(),
    }
}

fn severity_label(s: &str) -> &'static str {
    match s {
        "high" => ex("alta"),
        "medium" => ex("media"),
        _ => ex("bassa"),
    }
}

/// `+n` chip for a change against the start of the turn.
fn delta(n: i64) -> Html {
    if n == 0 {
        return Html::default();
    }
    html! {<small class={classes!("sit-delta",(n>0).then_some("up"))} title={ex("Variazione dall’inizio del turno")}>{format!(" ({})",signed(n))}</small>}
}

fn n(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

/// Stat tile with the value and its change since the turn start.
fn tile(label: &str, value: String, d: i64, tone: &'static str) -> Html {
    html! {<div class={classes!("sit-tile",tone)}><b>{value}{delta(d)}</b><span>{label.to_string()}</span></div>}
}

#[derive(Properties, PartialEq)]
pub struct SituationProps {
    pub stat: Value,
    pub state: Value,
    pub master: Value,
}

#[function_component(Situation)]
pub fn situation(props: &SituationProps) -> Html {
    let impact = use_state(|| Value::Null);
    let prev = use_state(|| Value::Null);
    let sugg = use_state(|| Value::Null);
    let error = use_state(String::new);
    let end_mode = use_state(|| false);
    let selected = use_state(|| None::<String>);
    let focus = use_state(|| None::<(f64, f64, u32)>);
    let opened = use_state(|| None::<String>);
    // AI brief: what the players did about each alert (on request only).
    let brief = use_state(|| Value::Null);
    let briefing = use_state(|| false);
    let held: Rc<RefCell<(Value, Value, Value)>> =
        use_mut_ref(|| (Value::Null, Value::Null, Value::Null));
    let inflight = use_mut_ref(|| Rc::new(Cell::new(false)));

    let refresh = {
        let (impact, prev, sugg, error, held) = (
            impact.clone(),
            prev.clone(),
            sugg.clone(),
            error.clone(),
            held.clone(),
        );
        let inflight = inflight.borrow().clone();
        Callback::from(move |_: ()| {
            if inflight.replace(true) {
                return;
            }
            let (impact, prev, sugg, error, held, inflight) = (
                impact.clone(),
                prev.clone(),
                sugg.clone(),
                error.clone(),
                held.clone(),
                inflight.clone(),
            );
            wasm_bindgen_futures::spawn_local(async move {
                let have = held.borrow().1["result_key"].as_u64();
                let path = format!(
                    "/api/master/situation/preview{}",
                    have.map(|k| format!("?have={k}")).unwrap_or_default()
                );
                let (a, b, c) = (
                    call("/api/master/impact", None).await,
                    call(&path, None).await,
                    call("/api/master/situation/suggestions", None).await,
                );
                match (a, b, c) {
                    (Ok(a), Ok(mut b), Ok(c)) => {
                        if b["preview"].is_null() && b["result_key"].is_number() {
                            b["preview"] = held.borrow().1["preview"].clone();
                        }
                        let mut h = held.borrow_mut();
                        if h.0 != a {
                            h.0 = a.clone();
                            impact.set(a);
                        }
                        if h.1 != b {
                            h.1 = b.clone();
                            prev.set(b);
                        }
                        if h.2 != c {
                            h.2 = c.clone();
                            sugg.set(c);
                        }
                        if !error.is_empty() {
                            error.set(String::new());
                        }
                    }
                    (a, b, c) => {
                        let e = a.err().or(b.err()).or(c.err()).unwrap_or_default();
                        error.set(e);
                    }
                }
                inflight.set(false);
            });
        })
    };
    {
        // Poll while the tab is open; the preview recomputes in the background.
        let refresh = refresh.clone();
        use_effect_with((), move |_| {
            refresh.emit(());
            let tick = Interval::new(1500, move || refresh.emit(()));
            move || drop(tick)
        });
    }
    {
        let refresh = refresh.clone();
        use_effect_with(props.state["generation"].as_u64(), move |_| {
            refresh.emit(());
            || ()
        });
    }

    let origin = props.stat["start_clock_s"].as_i64().unwrap_or(0);
    let pv_state = text(&prev["state"]).to_string();
    let pv = &prev["preview"];
    let have_preview = pv.is_object();
    let current = prev["current"] == true;
    let end = *end_mode && have_preview;
    let report: &Value = if end { &pv["end"] } else { &*impact };
    let now_s = props.state["world"]["stats"]["time_s"]
        .as_i64()
        .unwrap_or(0);
    let to_s = pv["to_s"].as_i64().unwrap_or(now_s);
    let now_clock = hhmm(origin, now_s);
    let end_clock = hhmm(origin, to_s);

    // Map: the present world, or the world as it will be at the end of the turn.
    let mut world = props.state["world"].clone();
    if end {
        world["fire"] = pv["fire"]["raster"].clone();
        world["buildings"] = pv["buildings"].clone();
        world["roads"] = pv["end_roads"].clone();
    }
    let marker = selected.as_ref().and_then(|id| {
        list(&impact["rows"])
            .iter()
            .chain(list(&pv["end"]["rows"]))
            .find(|r| text(&r["id"]) == id)
            .map(|r| json!({"x": r["pos"][0], "y": r["pos"][1]}))
    });

    // Rows: everything listed now or at the end, with what is predicted for it.
    let mut ids: Vec<String> = Vec::new();
    for r in list(&impact["rows"]).iter().chain(list(&pv["end"]["rows"])) {
        let id = text(&r["id"]).to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let find = |rep: &Value, id: &str| {
        list(&rep["rows"])
            .iter()
            .find(|r| text(&r["id"]) == id)
            .cloned()
    };
    let rows: Vec<(String, Option<Value>, Option<Value>)> = ids
        .into_iter()
        .map(|id| {
            let a = find(&impact, &id);
            let b = find(&pv["end"], &id);
            (id, a, b)
        })
        .collect();
    let sectors: Vec<Value> = list(&report["places"]).to_vec();
    let mut groups: Vec<(Value, Vec<&(String, Option<Value>, Option<Value>)>)> = sectors
        .iter()
        .map(|s| {
            let mine = rows
                .iter()
                .filter(|(_, a, b)| {
                    let r = a.as_ref().or(b.as_ref());
                    r.is_some_and(|r| {
                        text(&r["sector"]) == text(&s["id"]) && text(&r["kind"]) == "building"
                    })
                })
                .collect();
            (s.clone(), mine)
        })
        .collect();
    groups.retain(|(s, r)| {
        !r.is_empty()
            || n(&s["counts"]["threatened"])
                + n(&s["counts"]["damaged"])
                + n(&s["counts"]["destroyed"])
                > 0
            || n(&s["counts"]["buildings"]) > 0
    });
    let road_rows: Vec<&(String, Option<Value>, Option<Value>)> = rows
        .iter()
        .filter(|(_, a, b)| {
            a.as_ref()
                .or(b.as_ref())
                .is_some_and(|r| text(&r["kind"]) == "road")
        })
        .collect();

    let pick = |id: String, pos: Value| {
        let (selected, focus) = (selected.clone(), focus.clone());
        Callback::from(move |_: MouseEvent| {
            selected.set(Some(id.clone()));
            if let (Some(x), Some(y)) = (pos[0].as_f64(), pos[1].as_f64()) {
                let k = focus.map(|f| f.2 + 1).unwrap_or(1);
                focus.set(Some((x, y, k)));
            }
        })
    };

    let row_html = |(id, a, b): &(String, Option<Value>, Option<Value>)| -> Html {
        let shown = if end {
            b.as_ref().or(a.as_ref())
        } else {
            a.as_ref().or(b.as_ref())
        };
        let Some(r) = shown else {
            return Html::default();
        };
        let state_now = a.as_ref().map(|r| text(&r["state"])).unwrap_or("normal");
        let state_end = b
            .as_ref()
            .map(|r| text(&r["state"]))
            .unwrap_or(if have_preview { state_now } else { "" });
        let tl = &pv["timelines"][id.as_str()];
        let mut when: Vec<String> = Vec::new();
        for (key, label) in [
            ("threatened_s", ex("minacciato")),
            ("damaged_s", ex("danneggiato")),
            ("destroyed_s", ex("distrutto")),
            ("cut_s", ex("tagliata")),
            ("emptied_s", ex("svuotato")),
        ] {
            if let Some(t) = tl[key].as_i64() {
                when.push(format!("{label} {}", hhmm(origin, t)));
            }
        }
        let shown_state = if end { state_end } else { state_now };
        let people = if text(&r["kind"]) == "road" {
            String::new()
        } else {
            format!(
                "{} / {} / {}",
                n(&r["people_inside"]),
                n(&r["evacuating"]),
                n(&r["arrived"])
            )
        };
        let sel = selected.as_deref() == Some(id.as_str());
        let tone = |s: &str| match s {
            "destroyed" | "cut" => "bad",
            "damaged" => "warn",
            "threatened" | "closed" => "amber",
            _ => "",
        };
        html! {<tr key={id.clone()} class={classes!("sit-row",sel.then_some("sel"))} data-row={id.clone()} onclick={pick(id.clone(),r["pos"].clone())}>
        <td><b>{text(&r["name"])}</b>{" "}<small class="muted">{category_label(text(&r["category"]))}</small></td>
        <td><span class={classes!("pill","sit-state",tone(shown_state))}>{state_label(shown_state)}</span></td>
        <td class="num">{people}{if text(&r["kind"])!="road"&&n(&r["at_risk"])>0{html!{<small class="sit-risk">{crate::exfmt!(" · a rischio {}"," · at risk {}",n(&r["at_risk"]))}</small>}}else{Html::default()}}</td>
        <td class="sit-pred">{if have_preview{html!{<><span class={classes!("pill","sit-state",tone(state_end))}>{state_label(state_end)}</span>{" "}<small class="muted">{when.join(" · ")}</small></>}}else{html!{<small class="muted">{"…"}</small>}}}</td></tr>}
    };

    let t = &report["totals"];
    let d = &report["delta"];
    let totals = html! {<div class="sit-tiles">
        {tile(ex("edifici minacciati"),n(&t["threatened"]).to_string(),n(&d["threatened"]),"amber")}
        {tile(ex("danneggiati"),n(&t["damaged"]).to_string(),n(&d["damaged"]),"warn")}
        {tile(ex("distrutti"),n(&t["destroyed"]).to_string(),n(&d["destroyed"]),"bad")}
        {tile(ex("persone presenti"),n(&t["people_present"]).to_string(),n(&d["people_present"]),"")}
        {tile(ex("in evacuazione"),n(&t["people_evacuating"]).to_string(),n(&d["people_evacuating"]),"")}
        {tile(ex("evacuate (arrivate)"),n(&t["people_arrived"]).to_string(),n(&d["people_arrived"]),"")}
        {tile(ex("persone a rischio"),n(&t["people_at_risk"]).to_string(),n(&d["people_at_risk"]),"bad")}
        {tile(ex("strade tagliate"),format!("{} / {}",n(&t["roads_cut"]),n(&t["roads_total"])),n(&d["roads_cut"]),"bad")}
        {tile(ex("rifugi: accolti / posti"),format!("{} / {}",n(&t["sheltered"]),n(&t["shelter_capacity"])),n(&d["sheltered"]),"")}
    </div>};

    let status = match (pv_state.as_str(), have_preview, current) {
        ("paused", _, _) => {
            html! {<span class="pill">{ex("anteprima in pausa durante l’avanzamento")}</span>}
        }
        (_, false, _) if prev["error"].is_string() => {
            html! {<span class="pill bad">{text(&prev["error"])}</span>}
        }
        (_, false, _) => {
            html! {<span class="pill amber">{ex("calcolo dell’anteprima in corso…")}</span>}
        }
        (_, true, true) => html! {<span class="pill ok">{ex("anteprima aggiornata")}</span>},
        (_, true, false) => {
            html! {<span class="pill amber">{ex("anteprima in aggiornamento…")}</span>}
        }
    };
    let included = if have_preview {
        let applied = list(&pv["applied"]);
        let actions = list(&pv["actions"]);
        html! {<details class="sit-included"><summary>{crate::exfmt!("L’anteprima include {} proposte approvate e {} azioni dei piani · {} ms","The preview includes {} approved proposals and {} planned actions · {} ms",applied.len(),actions.len(),pv["wall_ms"])}</summary>
        <ul>{for applied.iter().map(|a|html!{<li>{format!("{} ({}) — {}",text(&a["action"]),text(&a["by"]),if a["ok"]==true{"ok"}else{text(&a["detail"])})}</li>})}
        {for actions.iter().map(|a|html!{<li>{format!("{} · {}",hhmm(origin,a["at_s"].as_i64().unwrap_or(0)),text(&a["text"]))}</li>})}</ul>
        <small class="muted">{ex("Simulazione identica a quella dell’avanzamento a fine turno, con le stesse decisioni già prese: se nessuno fa altro, accade questo. Non invia né pianifica nulla.")}</small></details>}
    } else {
        Html::default()
    };

    let roles: Vec<Value> = list(&props.stat["roles"])
        .iter()
        .filter(|r| text(&r["id"]) != "master")
        .cloned()
        .collect();
    let items = list(&sugg["suggestions"]);
    let on_done = {
        let (refresh, opened) = (refresh.clone(), opened.clone());
        Callback::from(move |_: ()| {
            opened.set(None);
            refresh.emit(());
        })
    };
    let ask_brief = {
        let (brief, briefing, error) = (brief.clone(), briefing.clone(), error.clone());
        Callback::from(move |_: MouseEvent| {
            let (brief, briefing, error) = (brief.clone(), briefing.clone(), error.clone());
            briefing.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let body = json!({"language": if is_en() {"en"} else {"it"}});
                match call("/api/master/situation/brief", Some(body)).await {
                    Ok(v) => brief.set(v),
                    Err(e) => error.set(e),
                }
                briefing.set(false);
            });
        })
    };
    let ignore = |id: String| {
        let (on_done, error) = (on_done.clone(), error.clone());
        Callback::from(move |_: MouseEvent| {
            let (on_done, error, id) = (on_done.clone(), error.clone(), id.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let p = json!({"id": id, "status": "dismissed"});
                match call("/api/master/situation/mark", Some(p)).await {
                    Ok(_) => on_done.emit(()),
                    Err(e) => error.set(e),
                }
            });
        })
    };

    html! {<main class="page sit-page" id="situation">
        <section class="card sit-head">
            <div class="row"><h2>{ex("Situazione")}</h2>
                <div class="sit-toggle" role="group" aria-label={ex("Momento mostrato")}>
                    <button id="sit-now" class={classes!((!end).then_some("active"))} onclick={{let m=end_mode.clone();Callback::from(move |_|m.set(false))}}>{crate::exfmt!("Ora ({})","Now ({})",now_clock)}</button>
                    <button id="sit-end" class={classes!(end.then_some("active"))} disabled={!have_preview} onclick={{let m=end_mode.clone();Callback::from(move |_|m.set(true))}}>{if have_preview{crate::exfmt!("Fine turno ({})","End of turn ({})",end_clock)}else{ex("Fine turno").to_string()}}</button>
                </div>
                {status}<div class="grow"/>
                <small class="muted">{ex("Gli inject restano manuali: qui non parte né si pianifica nulla da solo.")}</small></div>
            if !error.is_empty(){<p class="error" role="alert">{(*error).clone()}</p>}
            {totals}{included}
        </section>
        <div class="sit-grid">
            <section class="card sit-map"><h3>{if end{crate::exfmt!("Mappa · previsione alle {}","Map · forecast at {}",end_clock)}else{crate::exfmt!("Mappa · ora ({})","Map · now ({})",now_clock)}}</h3>
                <SituationMap id="sit-map-canvas" stat={props.stat.clone()} view={world} pending={marker} focus={*focus} lang={if is_en(){"en"}else{"it"}}/>
                <small class="muted">{ex("Clic su una riga della tabella per centrare la mappa. Colori: minacciato (ambra), danneggiato, distrutto, strada tagliata.")}</small></section>
            <section class="card sit-table"><h3>{ex("Impatto per luogo")}</h3>
                <div class="sit-scroll"><table><thead><tr><th>{ex("Elemento")}</th><th>{if end{ex("Stato a fine turno")}else{ex("Stato ora")}}</th><th title={ex("presenti / in evacuazione / evacuate")}>{ex("Persone")}</th><th>{if have_preview{crate::exfmt!("Previsto entro {}","Expected by {}",end_clock)}else{ex("Previsto").to_string()}}</th></tr></thead>
                <tbody>
                {for groups.iter().map(|(s,rs)|{
                    let c=&s["counts"];let dd=&s["delta"];
                    html!{<>
                    <tr class="sit-group" key={format!("g-{}",text(&s["id"]))}><th colspan="4"><b>{text(&s["name"])}</b>
                        <span>{crate::exfmt!(" · {} edifici · minacciati {}"," · {} buildings · threatened {}",n(&c["buildings"]),n(&c["threatened"]))}{delta(n(&dd["threatened"]))}
                        {crate::exfmt!(" · danneggiati {}"," · damaged {}",n(&c["damaged"]))}{delta(n(&dd["damaged"]))}
                        {crate::exfmt!(" · distrutti {}"," · destroyed {}",n(&c["destroyed"]))}{delta(n(&dd["destroyed"]))}
                        {crate::exfmt!(" · persone presenti {}"," · people present {}",n(&c["people_present"]))}{delta(n(&dd["people_present"]))}
                        {crate::exfmt!(" · in evacuazione {}"," · evacuating {}",n(&c["people_evacuating"]))}{delta(n(&dd["people_evacuating"]))}
                        {crate::exfmt!(" · arrivate {}"," · arrived {}",n(&c["people_arrived"]))}{delta(n(&dd["people_arrived"]))}
                        {crate::exfmt!(" · a rischio {}"," · at risk {}",n(&c["people_at_risk"]))}{delta(n(&dd["people_at_risk"]))}</span></th></tr>
                    {for rs.iter().map(|r|row_html(r))}</>}
                })}
                if !road_rows.is_empty(){<tr class="sit-group"><th colspan="4"><b>{ex("Strade")}</b><span>{crate::exfmt!(" · tagliate {} su {}"," · cut {} of {}",n(&t["roads_cut"]),n(&t["roads_total"]))}{delta(n(&d["roads_cut"]))}</span></th></tr>
                    {for road_rows.iter().map(|r|row_html(r))}}
                </tbody></table></div>
                <small class="muted">{ex("Variazioni tra parentesi: rispetto all’inizio del turno. Le abitazioni semplici compaiono solo quando il fuoco le raggiunge.")}</small></section>
            <section class="card sit-sugg"><div class="row"><h3>{ex("Allerte per l’EXCON")}</h3><span class="pill" id="sit-count">{items.len()}</span><div class="grow"/>
                    <button class="small" id="sit-brief" disabled={*briefing||items.is_empty()} onclick={ask_brief} title={ex("L’AI legge risposte, messaggi e conversazioni dei ruoli coinvolti e dice, per ogni allerta, cosa hanno fatto o detto")}>
                        {if *briefing{html!{<><span class="loader" aria-hidden="true"></span>{ex("Quadro in preparazione…")}</>}}else{html!{ex("Cosa stanno facendo i giocatori?")}}}</button></div>
                <small class="muted">{ex("Cosa succede se nessuno interviene, secondo la situazione e la previsione. Sono per te, non per i giocatori: decidi tu se e come creare un inject.")}</small>
                if brief["llm"]==true{<div class="sit-brief" id="sit-brief-summary"><small class="muted">{crate::exfmt!("Quadro AI delle {} · da risposte, messaggi e conversazioni","AI brief of {} · from responses, messages and conversations",text(&brief["clock"]).rsplit(' ').next().unwrap_or_default())}</small>
                    if !text(&brief["summary"]).is_empty(){<p>{text(&brief["summary"])}</p>}</div>}
                else if brief["llm"]==false{<p><small class="muted">{crate::exfmt!("Quadro AI non disponibile: {}","AI brief unavailable: {}",text(&brief["reason"]))}</small></p>}
                if sugg["preview_current"]==false&&!sugg.is_null(){<p><small class="muted">{ex("Basate su una previsione non aggiornata.")}</small></p>}
                if items.is_empty(){<p class="muted sit-none">{ex("Nessuna situazione critica da segnalare.")}</p>}
                {for items.iter().map(|s|{
                    let id=text(&s["id"]).to_string();
                    let is_open=opened.as_deref()==Some(id.as_str());
                    let toggle={let (opened,id)=(opened.clone(),id.clone());Callback::from(move |_|opened.set(if is_open{None}else{Some(id.clone())}))};
                    let when=match s["reach_s"].as_i64(){
                        Some(t) if t>now_s=>crate::exfmt!("alle {}","at {}",hhmm(origin,t)),
                        _=>ex("già in corso").to_string(),
                    };
                    let act_by=(s["now"]!=true).then(||crate::exfmt!("per reagire in tempo: entro le {}","to react in time: by {}",hhmm(origin,s["at_s"].as_i64().unwrap_or(0))));
                    let locate={let pos=s["object"]["pos"].clone();let oid=text(&s["object"]["id"]).to_string();let (selected,focus)=(selected.clone(),focus.clone());
                        Callback::from(move |_|{ selected.set(Some(oid.clone())); if let (Some(x),Some(y))=(pos[0].as_f64(),pos[1].as_f64()){let k=focus.map(|f|f.2+1).unwrap_or(1);focus.set(Some((x,y,k)));} })};
                    html!{<article key={id.clone()} class={classes!("sit-card",text(&s["severity"]).to_string(),is_open.then_some("open"))} data-suggestion={id.clone()}>
                        <div class="row"><span class={classes!("pill","sev",text(&s["severity"]).to_string())}>{severity_label(text(&s["severity"]))}</span><span class="pill">{when}</span><div class="grow"/>
                            <button class="small" onclick={locate} title={ex("Centra la mappa")}>{"⌖"}</button></div>
                        <b class="sit-title">{lt(&s["alert"])}</b>
                        if let Some(by)=act_by{<p class="sit-evidence">{by}</p>}
                        if let Some(note)=brief["items"][id.as_str()].as_str(){<p class="sit-note" data-brief-note={id.clone()}>{note.to_string()}</p>}
                        <div class="row"><small class="muted">{ex("Chi può intervenire:")}</small>{for list(&s["recipients"]).iter().map(|r|html!{<span class="pill rcpt">{crate::timeline::role_name(&roles,text(r))}</span>})}<div class="grow"/>
                            if !is_open{<button class="small" onclick={ignore(id.clone())} title={ex("Non segnalare più questa situazione")}>{ex("Ignora")}</button>}
                            <button class="small" onclick={toggle} title={ex("Scrivi un inject a partire da questa situazione: parte da una bozza che puoi cambiare del tutto")}>{if is_open{ex("Chiudi")}else{ex("Crea inject")}}</button></div>
                        if is_open{<SuggestionEditor key={id.clone()} suggestion={s.clone()} roles={roles.clone()} now_s={now_s} origin={origin} ondone={on_done.clone()}/>}
                    </article>}
                })}
            </section>
        </div>
    </main>}
}

#[derive(Properties, PartialEq)]
struct EditorProps {
    suggestion: Value,
    roles: Vec<Value>,
    now_s: i64,
    origin: i64,
    ondone: Callback<()>,
}

const VOICES: &[(&str, &str, &str)] = &[
    (
        "Chiamata al 112",
        "112 Dispatch",
        "a caller phoning the 112 emergency line",
    ),
    ("Cittadino", "Citizen", "a worried citizen"),
    (
        "Squadra sul campo",
        "Field team",
        "a field team reporting by radio",
    ),
    (
        "Sala operativa",
        "Operations room",
        "an officer of the operations room",
    ),
];

/// The draft being edited: send it now, plan it, rewrite it, or drop it.
#[function_component(SuggestionEditor)]
fn suggestion_editor(props: &EditorProps) -> Html {
    let s = &props.suggestion;
    let title = use_state(|| lt(&s["title"]).to_string());
    let body = use_state(|| lt(&s["text"]).to_string());
    let sender = use_state(|| text(&s["sender"]).to_string());
    let voice = use_state(String::new);
    let recipients = use_state(|| {
        list(&s["recipients"])
            .iter()
            .map(|r| text(r).to_string())
            .collect::<BTreeSet<_>>()
    });
    let at = use_state(|| {
        let t = s["at_s"]
            .as_i64()
            .filter(|_| s["now"] != true)
            .unwrap_or(props.now_s);
        hhmm(props.origin, t)
    });
    let busy = use_state(|| false);
    let error = use_state(String::new);
    let note = use_state(String::new);
    let id = text(&s["id"]).to_string();
    // The AI writes the draft as an in-world report; typing first keeps your text.
    let drafting = use_state(|| true);
    let touched = use_mut_ref(|| false);
    {
        let (title, body, sender, note, drafting, touched, id) = (
            title.clone(),
            body.clone(),
            sender.clone(),
            note.clone(),
            drafting.clone(),
            touched.clone(),
            id.clone(),
        );
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                let req = json!({"id": id, "language": if is_en() {"en"} else {"it"}});
                match call("/api/master/situation/draft", Some(req)).await {
                    Ok(v) if v["llm"] == true => {
                        if !*touched.borrow() {
                            title.set(text(&v["title"]).to_string());
                            body.set(text(&v["text"]).to_string());
                            sender.set(text(&v["sender"]).to_string());
                        }
                        note.set(
                            ex("Bozza scritta dall’AI a partire dall’allerta: controllala prima di inviare.")
                                .to_string(),
                        );
                    }
                    Ok(v) => note.set(crate::exfmt!(
                        "AI non disponibile: bozza standard ({}).",
                        "AI unavailable: standard draft ({}).",
                        text(&v["reason"])
                    )),
                    Err(e) => note.set(e),
                }
                drafting.set(false);
            });
            || ()
        });
    }
    let all_ids: Vec<String> = props
        .roles
        .iter()
        .map(|r| text(&r["id"]).to_string())
        .collect();
    let recips_now = || -> Vec<String> {
        let r = (*recipients).clone();
        if r.iter().any(|x| x == "all") {
            all_ids.clone()
        } else {
            r.into_iter().collect()
        }
    };

    let run = |path: &'static str, body_json: Value, then_mark: Option<&'static str>| {
        let (busy, error, ondone, id) = (
            busy.clone(),
            error.clone(),
            props.ondone.clone(),
            id.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let (busy, error, ondone, id, body_json) = (
                busy.clone(),
                error.clone(),
                ondone.clone(),
                id.clone(),
                body_json.clone(),
            );
            busy.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let mut result = call(path, Some(body_json)).await;
                if let (Ok(_), Some(status)) = (&result, then_mark) {
                    result = call(
                        "/api/master/situation/mark",
                        Some(json!({"id": id, "status": status})),
                    )
                    .await;
                }
                busy.set(false);
                match result {
                    Ok(_) => ondone.emit(()),
                    Err(e) => error.set(e),
                }
            });
        })
    };

    let payload = json!({"title": (*title).clone(), "sender": (*sender).clone(), "recipients": recips_now(), "text": (*body).clone()});
    let send = run("/api/master/inject", payload, Some("sent"));
    let dismiss = {
        let p = json!({"id": id, "status": "dismissed"});
        run("/api/master/situation/mark", p, None)
    };
    // "HH:MM" → simulated second: the next time of day not before now.
    let at_s = {
        let (h, m) = at
            .split_once(':')
            .map(|(h, m)| (h.parse::<i64>().ok(), m.parse::<i64>().ok()))
            .unwrap_or((None, None));
        h.zip(m)
            .filter(|(h, m)| (0..24).contains(h) && (0..60).contains(m))
            .map(|(h, m)| {
                let now_abs = props.origin + props.now_s;
                let base = now_abs.div_euclid(86_400) * 86_400 + h * 3600 + m * 60;
                let abs = if base < now_abs { base + 86_400 } else { base };
                abs - props.origin
            })
    };
    let plan = {
        let p = json!({"title": (*title).clone(), "sender": (*sender).clone(), "recipients": recips_now(), "text": (*body).clone(), "at_s": at_s, "suggestion": id});
        run("/api/master/situation/plan", p, None)
    };
    let rewrite = {
        let (title, body, voice, busy, error, note) = (
            title.clone(),
            body.clone(),
            voice.clone(),
            busy.clone(),
            error.clone(),
            note.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let req = json!({"title": (*title).clone(), "text": (*body).clone(), "voice": (*voice).clone(), "language": if is_en() {"en"} else {"it"}});
            let (title, body, busy, error, note) = (
                title.clone(),
                body.clone(),
                busy.clone(),
                error.clone(),
                note.clone(),
            );
            busy.set(true);
            error.set(String::new());
            note.set(String::new());
            wasm_bindgen_futures::spawn_local(async move {
                match call("/api/master/situation/rewrite", Some(req)).await {
                    Ok(v) if v["llm"] == true => {
                        title.set(text(&v["title"]).to_string());
                        body.set(text(&v["text"]).to_string());
                        note.set(
                            ex("Testo riscritto dall’AI: controllalo prima di inviare.")
                                .to_string(),
                        );
                    }
                    Ok(v) => note.set(crate::exfmt!(
                        "AI non disponibile: il testo resta quello della bozza ({}).",
                        "AI unavailable: the draft text is unchanged ({}).",
                        text(&v["reason"])
                    )),
                    Err(e) => error.set(e),
                }
                busy.set(false);
            });
        })
    };
    let toggle = |role: String| {
        let recipients = recipients.clone();
        Callback::from(move |_: Event| {
            let mut r = (*recipients).clone();
            r.remove("all");
            if !r.remove(&role) {
                r.insert(role.clone());
            }
            recipients.set(r);
        })
    };
    let ready = !title.trim().is_empty()
        && !body.trim().is_empty()
        && !recipients.is_empty()
        && !*busy
        && !*drafting;
    let typed = |state: UseStateHandle<String>| {
        let touched = touched.clone();
        Callback::from(move |e: InputEvent| {
            *touched.borrow_mut() = true;
            state.set(api::input(e));
        })
    };
    html! {<div class="sit-editor">
        if *drafting{<p class="sit-drafting"><span class="loader" aria-hidden="true"></span><small class="muted">{ex("L’AI sta scrivendo la bozza dell’inject…")}</small></p>}
        <label>{ex("Titolo")}<input type="text" value={(*title).clone()} oninput={typed(title.clone())}/></label>
        <label>{ex("Testo")}<textarea rows="5" value={(*body).clone()} oninput={typed(body.clone())}/></label>
        <label>{ex("Mittente")}<input type="text" value={(*sender).clone()} oninput={typed(sender.clone())}/></label>
        <fieldset class="sit-rcpt"><legend>{ex("Destinatari")}</legend>
            {for props.roles.iter().map(|r|{let rid=text(&r["id"]).to_string();html!{<label class="inline-check" key={rid.clone()}><input type="checkbox" checked={recipients.contains(&rid)||recipients.contains("all")} onchange={toggle(rid.clone())}/>{text(&r["name"])}</label>}})}</fieldset>
        <div class="row sit-voice"><label>{ex("Riscrivi con la voce di")}<select onchange={{let (v,s)=(voice.clone(),sender.clone());Callback::from(move |e|{let k=api::select(e);if let Some((_,label,v_en))=VOICES.iter().find(|(it,..)|*it==k){v.set(v_en.to_string());s.set(label.to_string());}})}}>
                <option value="">{"—"}</option>{for VOICES.iter().map(|(it,..)|html!{<option value={*it}>{ex(it)}</option>})}</select></label>
            <input type="text" placeholder={ex("oppure descrivi chi parla (es. un genitore)")} value={(*voice).clone()} oninput={{let v=voice.clone();Callback::from(move |e|v.set(api::input(e)))}}/>
            <button disabled={voice.trim().is_empty()||*busy} onclick={rewrite} title={ex("Riformula la bozza con il modello linguistico; senza AI il testo resta com’è")}>{ex("Riscrivi con AI")}</button></div>
        if !note.is_empty(){<p><small class="muted">{(*note).clone()}</small></p>}
        if !error.is_empty(){<p class="error" role="alert">{(*error).clone()}</p>}
        <div class="row"><button class="primary" id="sit-send" disabled={!ready} onclick={send} title={ex("Invia subito come inject personalizzato")}>{ex("Invia ora")}</button>
            <label class="inline-check">{ex("oppure pianifica alle")}<input type="text" size="5" class="sit-time" value={(*at).clone()} oninput={{let a=at.clone();Callback::from(move |e|a.set(api::input(e)))}}/></label>
            <button id="sit-plan" disabled={!ready||at_s.is_none()} onclick={plan} title={ex("Aggiunge l’inject alla timeline all’ora indicata: resta da inviare a mano")}>{ex("Pianifica")}</button>
            <div class="grow"/><button disabled={*busy} onclick={dismiss} title={ex("Non proporre più questa segnalazione")}>{ex("Scarta")}</button></div>
        <small class="muted">{ex("Nessun invio automatico: l’inject parte solo da “Invia ora”; “Pianifica” lo mette in timeline.")}</small>
    </div>}
}

/// "N allerte" chip for the Timeline, linking to the Situation tab.
#[derive(Properties, PartialEq)]
pub struct BadgeProps {
    pub onopen: Callback<()>,
    /// Refetch when this changes (the exercise generation).
    pub generation: u64,
}

#[function_component(SuggestionBadge)]
pub fn suggestion_badge(props: &BadgeProps) -> Html {
    let count = use_state(|| 0usize);
    let inflight = use_mut_ref(|| false);
    let refresh = {
        let count = count.clone();
        let inflight = inflight.clone();
        Callback::from(move |_: ()| {
            if *inflight.borrow() {
                return;
            }
            *inflight.borrow_mut() = true;
            let c = count.clone();
            let inflight = inflight.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(v) = call("/api/master/situation/suggestions", None).await {
                    c.set(v["count"].as_u64().unwrap_or(0) as usize);
                }
                *inflight.borrow_mut() = false;
            });
        })
    };
    {
        // The alerts depend on the background turn preview and on EXCON marks,
        // which settle after the generation changes: poll while mounted too.
        let refresh = refresh.clone();
        use_effect_with((), move |_| {
            let tick = Interval::new(4000, move || refresh.emit(()));
            move || drop(tick)
        });
    }
    {
        let refresh = refresh.clone();
        use_effect_with(props.generation, move |_| {
            refresh.emit(());
            || ()
        });
    }
    if *count == 0 {
        return Html::default();
    }
    let onopen = props.onopen.clone();
    html! {<button class="tp-suggest" id="tp-suggest" title={ex("Apri la scheda Situazione: cosa succede se nessuno interviene")} onclick={Callback::from(move |_|onopen.emit(()))}>
    <b>{*count}</b>{" "}{if *count==1{ex("allerta")}else{ex("allerte")}}</button>}
}
