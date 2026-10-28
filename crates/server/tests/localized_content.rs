//! Scenario-authored content (role, resource and inject texts) is shown in the
//! reader's language, with and without an LLM.
use rocket::{http::Header, local::asynchronous::Client};
use serde_json::Value;
use std::sync::{Arc, Mutex};

async fn client(llm: llm::Llm, lang: &str) -> Client {
    let scenario = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = exercise::Exercise::new(sim::World::new(scenario, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", lang).unwrap();
    ex.send_inject(None, "School in session", "112 Dispatch", &["mayor".into()], "Fire.", &Default::default())
        .unwrap();
    let access = server::access::Access::with_keys(
        "master-test".into(),
        std::collections::BTreeMap::from([("mayor".into(), "mayor-test".into())]),
        Default::default(),
    );
    let (rocket, _) = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access);
    Client::tracked(rocket).await.unwrap()
}

async fn get(client: &Client, path: &str, token: &str) -> Value {
    client
        .get(path.to_string())
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await
        .into_json()
        .await
        .unwrap()
}

#[rocket::async_test]
async fn italian_player_reads_italian_without_an_llm() {
    let c = client(llm::Llm::off(), "it").await;
    let v = get(&c, "/api/player/mayor", "mayor-test").await;
    let text = v.to_string();
    for english in ["Mayor of Valverde", "Fire and Rescue Commander", "\"Buses\"", "Police patrols"] {
        assert!(!text.contains(english), "{english} left in the Italian player view");
    }
    assert_eq!(v["role"]["name"], "Sindaco di Valverde");
    let inject = v["inbox"].as_array().unwrap().iter().find(|m| m["kind"] == "inject").unwrap();
    assert_eq!(inject["title"], "Scuola in attività");
    assert_eq!(inject["from"], "Centrale 112");
    let stat = get(&c, "/api/static", "mayor-test").await;
    assert!(stat.to_string().contains("Autobus"));
    assert!(!stat.to_string().contains("\"Buses\""));
}

#[rocket::async_test]
async fn other_languages_use_the_glossary_offline_and_the_llm_for_the_rest() {
    let offline = client(llm::Llm::off(), "fr").await;
    let v = get(&offline, "/api/player/mayor", "mayor-test").await;
    assert_eq!(v["role"]["name"], "Maire de Valverde");
    // No translation without an LLM: the scenario text stays readable.
    assert_eq!(v["role"]["description"].as_str().unwrap().is_empty(), false);

    let online = client(llm::Llm::mock(|_, _, user| Ok(format!("[fr] {user}"))), "fr").await;
    let v = get(&online, "/api/player/mayor", "mayor-test").await;
    assert_eq!(v["role"]["name"], "Maire de Valverde", "glossary wins over the LLM");
    assert!(v["role"]["description"].as_str().unwrap().starts_with("[fr] "));
    let inject = v["inbox"].as_array().unwrap().iter().find(|m| m["kind"] == "inject").unwrap();
    assert!(inject["title"].as_str().unwrap().starts_with("[fr] "));
}

#[test]
fn every_player_language_translates_the_standard_roles_and_resources() {
    let scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    let names: Vec<&str> = scn
        .roles
        .iter()
        .map(|r| r.name.as_str())
        .chain(scn.resource_types.iter().map(|r| r.name.as_str()))
        .collect();
    for lang in ["it", "fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"] {
        let map = scn.display_map(lang);
        for name in &names {
            assert!(map.contains_key(*name), "{lang} lacks {name}");
        }
    }
    assert!(scn.display_map("en").is_empty());
}

#[test]
fn scenario_translations_override_the_glossary() {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.translations
        .entry("it".into())
        .or_default()
        .insert("Buses".into(), "Pullman".into());
    assert_eq!(scn.display_map("it")["Buses"], "Pullman");
    assert_eq!(scn.display_map("it")["Air tankers"], "Mezzi aerei");
}
