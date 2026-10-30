//! "Traduzioni" tab: the map labels (places, buildings, roads, borghi) in the
//! players' languages. Players read them on their maps; the EXCON and the
//! commands keep the territory's names. Translations are static, stored in the
//! pack; the AI can draft the missing ones, which stay editable.
use super::*;
use std::collections::BTreeMap;

/// Every name drawn on the maps: the territory's, plus places and borghi
/// added in this pack.
fn map_texts(o: &Open) -> Vec<String> {
    let e = &o.pack.edits;
    let mut out: Vec<String> = list(&o.reference["map_texts"]).iter().map(|t| text(t).to_string()).collect();
    out.extend(e.places.iter().flatten().map(|p| p.name.clone()));
    out.extend(e.districts.iter().flatten().map(|d| d.name.clone()));
    out.retain(|t| !t.trim().is_empty());
    out.sort();
    out.dedup();
    out
}

/// Languages the roles play in, beyond the working language: the ones that need labels.
fn role_languages(o: &Open) -> Vec<String> {
    let mut out: Vec<String> = o.pack.edits.roles.iter().flatten().map(|r| r.language.clone()).collect();
    out.sort();
    out.dedup();
    out
}

fn table<'a>(o: &'a Open, lang: &str) -> Option<&'a BTreeMap<String, String>> {
    o.pack.edits.translations.as_ref()?.get(lang)
}

/// How many of `texts` have a translation in `lang`.
fn covered(o: &Open, lang: &str, texts: &[String]) -> usize {
    table(o, lang).map_or(0, |t| texts.iter().filter(|x| t.get(*x).is_some_and(|v| !v.trim().is_empty())).count())
}

/// Adds `new` to the pack's table for `lang`, keeping what is already written.
fn merge(p: &mut ScenarioPack, lang: &str, new: &BTreeMap<String, String>) {
    let t = p.edits.translations.get_or_insert_with(Default::default).entry(lang.to_string()).or_default();
    for (k, v) in new {
        if t.get(k).is_none_or(|x| x.trim().is_empty()) {
            t.insert(k.clone(), v.clone());
        }
    }
}

#[derive(Properties, Clone)]
pub(super) struct TranslationsProps {
    pub o: Open,
    pub open: UseStateHandle<Option<Open>>,
}

impl PartialEq for TranslationsProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

pub(super) fn translations_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<TranslationsTab o={o.clone()} open={open.clone()}/>}
}

#[function_component(TranslationsTab)]
fn translations_view(props: &TranslationsProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let texts = map_texts(o);
    let wanted = role_languages(o);
    let lang = use_state(|| wanted.iter().find(|l| *l != "en").cloned().unwrap_or_else(|| "en".into()));
    let only_missing = use_state(|| false);
    let busy = use_state(|| false);
    let status = use_state(String::new);
    // The newest scenario, for merging an answer that arrives after further edits.
    let latest = use_mut_ref(|| None::<Open>);
    *latest.borrow_mut() = Some(o.clone());

    // Drafts the missing labels of `langs`, one language per request.
    let generate = {
        let (open, busy, status, latest) = (open.clone(), busy.clone(), status.clone(), latest.clone());
        Callback::from(move |langs: Vec<String>| {
            let (open, busy, status, latest) = (open.clone(), busy.clone(), status.clone(), latest.clone());
            busy.set(true);
            status.set(ex("Traduzione in corso…").into());
            wasm_bindgen_futures::spawn_local(async move {
                let mut failed = Vec::new();
                let mut added = 0;
                for lang in langs {
                    let Some(pack) = latest.borrow().as_ref().map(|o| o.pack.clone()) else { break };
                    match call("/api/master/scenarios/translate", Some(json!({"pack": pack, "language": lang}))).await {
                        Ok(v) => {
                            let new: BTreeMap<String, String> = serde_json::from_value(v["translations"].clone()).unwrap_or_default();
                            added += new.len();
                            if let Some(mut next) = latest.borrow().clone() {
                                merge(&mut next.pack, &lang, &new);
                                next.dirty |= !new.is_empty();
                                open.set(Some(next));
                            }
                        }
                        Err(e) => failed.push(format!("{lang}: {e}")),
                    }
                }
                busy.set(false);
                status.set(if failed.is_empty() {
                    crate::exfmt!("{} traduzioni aggiunte: rivedile e salva lo scenario.", "{} translations added: review them and save the scenario.", added)
                } else {
                    format!("{} {}", ex("Traduzione non riuscita:"), failed.join("; "))
                });
            });
        })
    };

    let lang_name = |code: &str| api::LANGUAGES.iter().find(|l| l.0 == code).map_or(code.to_string(), |l| l.1.to_string());
    let current = (*lang).clone();
    let done = table(o, &current).cloned().unwrap_or_default();
    let missing_langs: Vec<String> = wanted.iter().filter(|l| covered(o, l, &texts) < texts.len()).cloned().collect();
    let rows: Vec<&String> = texts.iter().filter(|t| !*only_missing || done.get(*t).is_none_or(|v| v.trim().is_empty())).collect();

    html! {<div class="translations-tab">
        {gen_section("Etichette della mappa", "I giocatori leggono i nomi della mappa (luoghi, edifici, strade, borghi) nella loro lingua. L’EXCON e i comandi usano sempre i nomi originali. Le traduzioni sono salvate nello scenario: un nome senza traduzione resta come nell’originale.", html! {<>
            <div class="gen-group wide translations-coverage">
                <h5>{ex("Lingue dei ruoli")}</h5>
                if wanted.is_empty() {<p class="muted">{ex("Nessun ruolo definito.")}</p>}
                <ul>{for wanted.iter().map(|l| {
                    let n = covered(o, l, &texts);
                    let lang = lang.clone();
                    let code = l.clone();
                    html!{<li><button type="button" class={classes!("link", (*l == current).then_some("active"))} title={ex("Mostra le traduzioni in questa lingua.")} onclick={Callback::from(move |_| lang.set(code.clone()))}>{lang_name(l)}</button>
                        {format!(" {n}/{}", texts.len())}{if n == texts.len() {" ✓"} else {""}}</li>}
                })}</ul>
                if !missing_langs.is_empty() {
                    <button type="button" disabled={*busy} title={ex("L’AI traduce le etichette mancanti in tutte le lingue dei ruoli. Quelle già scritte non cambiano.")}
                        onclick={{let generate = generate.clone(); let langs = missing_langs.clone(); Callback::from(move |_| generate.emit(langs.clone()))}}>{ex("Genera le mancanti per tutti i ruoli (AI)")}</button>
                }
            </div>
            <div class="gen-group wide">
                <label title={ex("Lingua delle traduzioni mostrate sotto.")}>{ex("Lingua")}
                    <select onchange={{let lang = lang.clone(); Callback::from(move |e| lang.set(api::select(e)))}}>
                        {for api::LANGUAGES.iter().map(|(code, name)| html!{<option value={*code} selected={*code == current}>{*name}</option>})}
                    </select></label>
                {check(ex("Solo le mancanti").into(), "", *only_missing, {let only_missing = only_missing.clone(); Callback::from(move |on| only_missing.set(on))})}
                <button type="button" disabled={*busy || covered(o, &current, &texts) == texts.len()} title={ex("L’AI traduce le etichette mancanti in questa lingua. Quelle già scritte non cambiano.")}
                    onclick={{let generate = generate.clone(); let l = current.clone(); Callback::from(move |_| generate.emit(vec![l.clone()]))}}>{ex("Genera le mancanti (AI)")}</button>
                if !status.is_empty() {<p role="status" class="muted">{(*status).clone()}</p>}
            </div>
        </>})}
        <table class="translations-table"><thead><tr><th>{ex("Nome originale")}</th><th>{lang_name(&current)}</th></tr></thead>
        <tbody>{for rows.into_iter().map(|t| {
            let value = done.get(t).cloned().unwrap_or_default();
            let (key, l) = (t.clone(), current.clone());
            let oninput = upd(open, move |p, e: InputEvent| {
                let v = api::input(e);
                let table = p.edits.translations.get_or_insert_with(Default::default).entry(l.clone()).or_default();
                if v.trim().is_empty() { table.remove(&key); } else { table.insert(key.clone(), v); }
            });
            html!{<tr key={t.clone()}><td>{t.clone()}</td><td><input value={value} placeholder={t.clone()} aria-label={t.clone()} {oninput}/></td></tr>}
        })}</tbody></table>
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_what_is_written() {
        let mut p: ScenarioPack = serde_json::from_value(json!({"id": "x", "name": "x", "base": "b", "edits": {"translations": {"it": {"Pine Hill": "Colle dei Pini"}}}})).unwrap();
        let new = BTreeMap::from([("Pine Hill".to_string(), "Collina dei Pini".to_string()), ("North Ridge".to_string(), "Cresta Nord".to_string())]);
        merge(&mut p, "it", &new);
        let t = &p.edits.translations.as_ref().unwrap()["it"];
        assert_eq!(t["Pine Hill"], "Colle dei Pini");
        assert_eq!(t["North Ridge"], "Cresta Nord");
    }
}
