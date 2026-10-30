//! Scenario-authored content (role, resource and inject texts) is shown in the
//! reader's language, with and without an LLM.
use rocket::{http::Header, local::asynchronous::Client};
use serde_json::Value;
use std::sync::{Arc, Mutex};

async fn client(llm: llm::Llm, lang: &str) -> Client {
    client_with(llm, lang, scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED)).await
}

async fn client_with(llm: llm::Llm, lang: &str, scenario: scenario::Scenario) -> Client {
    let scenario = Arc::new(scenario);
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

/// A territory with static map-label translations, `place` being one of its places.
fn translated_territory(lang: &str, translation: &str) -> (scenario::Scenario, String) {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    let place = scn.places[0].name.clone();
    scn.translations.entry(lang.into()).or_default().insert(place.clone(), translation.into());
    (scn, place)
}

#[rocket::async_test]
async fn players_read_map_labels_in_their_language_and_the_excon_the_source_names() {
    for lang in ["fr", "en"] {
        let (scn, place) = translated_territory(lang, "Lieu traduit");
        assert!(scn.map_texts().contains(&place));
        let c = client_with(llm::Llm::off(), lang, scn).await;
        let player = get(&c, "/api/static", "mayor-test").await;
        assert!(player["places"].as_array().unwrap().iter().any(|p| p["name"] == "Lieu traduit"), "{lang}");
        assert!(!player.to_string().contains(&format!("\"{place}\"")), "{lang}: {place} left in the player map");
        // The EXCON's commands name places by their source names.
        let excon = get(&c, "/api/static", "master-test").await;
        assert!(excon["places"].as_array().unwrap().iter().any(|p| p["name"] == place.as_str()));
        assert!(!excon.to_string().contains("Lieu traduit"));
    }
}

#[test]
fn the_llm_gazetteer_knows_the_names_players_read() {
    let (scn, place) = translated_territory("fr", "Lieu traduit");
    let mut ex = exercise::Exercise::new(sim::World::new(Arc::new(scn), 1).unwrap());
    let has_alias = |g: &Value| {
        g["places"].as_array().unwrap().iter().any(|p| p["name"] == place.as_str() && p["aliases"].as_array().unwrap().iter().any(|a| a == "Lieu traduit"))
    };
    assert!(!has_alias(&ex.gazetteer()), "no French player yet");
    ex.set_language("mayor", "fr").unwrap();
    assert!(has_alias(&ex.gazetteer()));
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
