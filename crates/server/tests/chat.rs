use rocket::{
    http::{ContentType, Header, Status},
    local::asynchronous::Client,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

async fn post(client: &Client, path: &str, token: &str, body: Value) -> (Status, Value) {
    let response = client
        .post(path)
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .body(body.to_string())
        .dispatch()
        .await;
    let status = response.status();
    let value = response.into_json().await.unwrap_or(Value::Null);
    (status, value)
}

#[rocket::async_test]
async fn player_chat_rejects_profanity_before_translation_and_tracks_valid_reply() {
    let translations = Arc::new(AtomicUsize::new(0));
    let seen = translations.clone();
    let llm = llm::Llm::mock(move |task, _, user| {
        if task == "translate" {
            seen.fetch_add(1, Ordering::SeqCst);
        }
        Ok(user.to_string())
    });
    let scenario = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scenario, 1).unwrap());
    let access = server::access::Access {
        master: "master-test".into(),
        players: std::collections::BTreeMap::from([
            ("mayor".into(), "mayor-test".into()),
            ("fire_commander".into(), "fire-test".into()),
            ("regional_cp".into(), "regional-test".into()),
        ]),
        evaluators: Default::default(),
    };
    let (rocket, _) = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access);
    let client = Client::tracked(rocket).await.unwrap();
    let (status, _) = post(&client, "/api/master/start", "master-test", json!({})).await;
    assert_eq!(status, Status::Ok);
    let (_, seeded) = post(
        &client,
        "/api/master/inject",
        "master-test",
        json!({
            "title":"Initial update", "recipients":["mayor"], "text":"Assess access."
        }),
    )
    .await;
    let root = seeded["messages"][0].as_u64().unwrap();
    let calls_before_missing = translations.load(Ordering::SeqCst);
    let (status, _) = post(
        &client,
        "/api/player/mayor/chat",
        "mayor-test",
        json!({"to":"fire_commander","text":"Unreferenced chat"}),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(
        translations.load(Ordering::SeqCst),
        calls_before_missing,
        "missing references are rejected before translation"
    );

    let (status, delivered) = post(
        &client,
        "/api/player/mayor/chat",
        "mayor-test",
        json!({
            "to":"fire_commander", "text":"The access road is blocked.", "reply_to":root
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let parent = delivered["message"].as_u64().unwrap();
    let calls_after_normal = translations.load(Ordering::SeqCst);
    assert!(calls_after_normal > 0, "non-English chat is translated");
    let mayor_before = client
        .get("/api/player/mayor")
        .header(Header::new("Authorization", "Bearer mayor-test"))
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    let _stored_before_same_language = mayor_before["inbox"].as_array().unwrap().len();

    let (status, _) = post(
        &client,
        "/api/player/fire_commander/language",
        "fire-test",
        json!({"language":"it"}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let before_same_language = translations.load(Ordering::SeqCst);
    let (status, same_language) = post(
        &client,
        "/api/player/mayor/chat",
        "mayor-test",
        json!({"to":"fire_commander", "text":"Ricevuto! F4 è chiusa.", "reply_to":parent}),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let fire = client
        .get("/api/player/fire_commander")
        .header(Header::new("Authorization", "Bearer fire-test"))
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    let same_message = fire["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == same_language["message"])
        .unwrap();
    assert_eq!(same_message["text"], "Ricevuto! F4 è chiusa.");
    assert_eq!(
        translations.load(Ordering::SeqCst),
        before_same_language + 1,
        "same-language delivery skips recipient translation but keeps the English working copy"
    );
    let calls_before_profanity = translations.load(Ordering::SeqCst);
    let stored_before_profanity = client
        .get("/api/player/mayor")
        .header(Header::new("Authorization", "Bearer mayor-test"))
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()["inbox"]
        .as_array()
        .unwrap()
        .len();

    let (status, _) = post(
        &client,
        "/api/player/mayor/chat",
        "mayor-test",
        json!({
            "to":"fire_commander", "text":"You are such a b!tch", "reply_to":parent
        }),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    assert_eq!(
        translations.load(Ordering::SeqCst),
        calls_before_profanity,
        "profanity is rejected before any LLM translation call"
    );
    let mayor_after = client
        .get("/api/player/mayor")
        .header(Header::new("Authorization", "Bearer mayor-test"))
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        mayor_after["inbox"].as_array().unwrap().len(),
        stored_before_profanity,
        "rejected chat is not stored"
    );

    let (status, reply) = post(
        &client,
        "/api/player/fire_commander/chat",
        "fire-test",
        json!({
            "to":"mayor", "text":"I will check it.", "reply_to":parent
        }),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let reply_id = reply["message"].as_u64().unwrap();
    let player = client
        .get("/api/player/fire_commander")
        .header(Header::new("Authorization", "Bearer fire-test"))
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    let stored = player["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == reply_id)
        .unwrap();
    assert_eq!(stored["reply_to"], parent);

    let (_, injected) = post(
        &client,
        "/api/master/inject",
        "master-test",
        json!({
            "title":"Private update", "recipients":["fire_commander"], "text":"A private update."
        }),
    )
    .await;
    let inject = injected["messages"][0].as_u64().unwrap();
    let calls_before_invalid_parent = translations.load(Ordering::SeqCst);
    let (status, _) = post(
        &client,
        "/api/player/mayor/chat",
        "mayor-test",
        json!({
            "to":"fire_commander", "text":"Thanks", "reply_to":inject
        }),
    )
    .await;
    assert_eq!(
        status,
        Status::BadRequest,
        "another role's inject cannot be used as a chat parent"
    );
    assert_eq!(
        translations.load(Ordering::SeqCst),
        calls_before_invalid_parent,
        "invalid parent is rejected before translation"
    );
}
