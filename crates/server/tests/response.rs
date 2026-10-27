//! HTTP flow of merged responses and conversations linked to an inject.
use rocket::{
    http::{ContentType, Header, Status},
    local::asynchronous::Client,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn call(c: &Client, who: &str, path: &str, body: Option<Value>) -> (Status, Value) {
    let req = if let Some(body) = body {
        c.post(path)
            .header(ContentType::JSON)
            .body(body.to_string())
    } else {
        c.get(path)
    };
    let r = req
        .header(Header::new("Authorization", format!("Bearer {who}")))
        .dispatch()
        .await;
    (r.status(), r.into_json().await.unwrap_or(Value::Null))
}

async fn fixture() -> (Client, server::AppState, u64) {
    let mut ex = exercise::Exercise::new(
        sim::World::new(
            Arc::new(scenario::synthetic::generate(
                scenario::synthetic::DEFAULT_SEED,
            )),
            1,
        )
        .unwrap(),
    );
    ex.start().unwrap();
    ex.set_language("mayor", "it").unwrap();
    let parent = ex
        .send_inject(
            None,
            "Smoke",
            "EXCON",
            &["mayor".into()],
            "Smoke over the village",
            &Default::default(),
        )
        .unwrap()[0];
    let llm = llm::Llm::mock(|task, _, user| match task {
        "translate" | "feedback" => Ok(user.into()),
        _ => anyhow::bail!("Unexpected task: {task}"),
    });
    let (app, state) = server::router(Arc::new(Mutex::new(ex)), llm, None);
    (Client::untracked(app).await.unwrap(), state, parent)
}

#[tokio::test]
async fn one_route_takes_narrative_actions_or_both_and_refuses_neither() {
    let (c, _, parent) = fixture().await;
    let path = "/api/player/mayor/actions";
    let warn = json!({"type":"IssueWarning","zone":"F4"});
    let (status, _) = call(&c, "mayor", path, Some(json!({"reply_to":parent}))).await;
    assert_eq!(status, Status::BadRequest, "neither narrative nor actions");
    let (status, body) = call(
        &c,
        "mayor",
        path,
        Some(json!({"reply_to":parent,"narrative":"Avviso i volontari"})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (status, body) = call(
        &c,
        "mayor",
        path,
        Some(json!({"reply_to":parent,"text":"Avviso F4","commands":[warn]})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (status, body) = call(
        &c,
        "mayor",
        path,
        Some(json!({"reply_to":parent,"narrative":"Avviso e informo","commands":[warn]})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (_, mine) = call(&c, "mayor", "/api/player/mayor", None).await;
    let p = mine["proposals"].as_array().unwrap();
    assert_eq!(p.len(), 3);
    assert_eq!(
        (p[0]["narrative"].clone(), p[0]["carries_actions"].clone()),
        (json!(true), json!(false))
    );
    assert_eq!(p[1]["narrative"], false);
    assert_eq!(
        (p[2]["narrative"].clone(), p[2]["carries_actions"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(p[2]["commands"][0]["type"], "IssueWarning");
    // EXCON sees all three under the inject and can approve the merged one.
    let (_, master) = call(&c, "master", "/api/master", None).await;
    assert_eq!(
        master["inject_responses"][0]["pending_proposals"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let id = p[2]["id"].as_u64().unwrap();
    let (status, body) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (status, body) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/narrative-outcome"),
        Some(json!({"text":"I volontari sono pronti"})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
}

#[tokio::test]
async fn a_linked_conversation_lets_another_player_answer_the_inject() {
    let (c, _, parent) = fixture().await;
    let respond = json!({"reply_to":parent,"narrative":"Warning from regional level","commands":[{"type":"IssueWarning","zone":"F4"}]});
    // Without a conversation the non-recipient cannot answer.
    let (status, _) = call(
        &c,
        "regional_cp",
        "/api/player/regional_cp/actions",
        Some(respond.clone()),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    // Only the recipient starts a conversation.
    let start = json!({"delivery":parent,"participants":["regional_cp"],"text":"Ci serve il vostro parere"});
    let (status, _) = call(
        &c,
        "fire_commander",
        "/api/player/fire_commander/conversations",
        Some(start.clone()),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    let (status, _) = call(
        &c,
        "fire_commander",
        "/api/player/mayor/conversations",
        Some(start.clone()),
    )
    .await;
    assert_eq!(status, Status::Forbidden, "players only act as themselves");
    let (status, body) = call(&c, "mayor", "/api/player/mayor/conversations", Some(start)).await;
    assert_eq!(status, Status::Ok, "{body}");
    let cid = body["conversation"].as_u64().unwrap();
    // The guest reads the quoted inject, the outsider sees nothing.
    let (_, guest) = call(&c, "regional_cp", "/api/player/regional_cp", None).await;
    let conv = &guest["linked_conversations"][0];
    assert_eq!(conv["quote"]["title"], "Smoke");
    assert_eq!(conv["messages"][0]["text"], "Ci serve il vostro parere");
    assert!(guest["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["kind"] != "inject"));
    let (_, outsider) = call(&c, "fire_commander", "/api/player/fire_commander", None).await;
    assert!(outsider["linked_conversations"]
        .as_array()
        .unwrap()
        .is_empty());
    let (status, _) = call(
        &c,
        "fire_commander",
        &format!("/api/player/fire_commander/conversations/{cid}/messages"),
        Some(json!({"text":"hi"})),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    // The guest replies citing the inject, then answers it.
    let (status, body) = call(
        &c,
        "regional_cp",
        &format!("/api/player/regional_cp/conversations/{cid}/messages"),
        Some(json!({"text":"We can warn zone F4","reply_to":parent})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (status, _) = call(
        &c,
        "regional_cp",
        &format!("/api/player/regional_cp/conversations/{cid}/messages"),
        Some(json!({"text":"x","reply_to":424242})),
    )
    .await;
    assert_eq!(status, Status::BadRequest);
    let mut with_conversation = respond;
    with_conversation["conversation"] = json!(cid);
    let (status, body) = call(
        &c,
        "regional_cp",
        "/api/player/regional_cp/actions",
        Some(with_conversation),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    // EXCON sees the responding role, the originating conversation and its transcript.
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let pending = &master["inject_responses"][0]["pending_proposals"][0];
    assert_eq!(pending["role"], "regional_cp");
    assert_eq!(pending["conversation"], cid);
    assert_eq!(master["linked_conversations"][0]["message_count"], 2);
    assert!(master["linked_conversations"][0]["messages"].to_string().contains("Ci serve il vostro parere"));
    // Players keep out of each other's private conversation.
    let (_, mayor) = call(&c, "mayor", "/api/player/mayor", None).await;
    assert_eq!(mayor["linked_conversations"][0]["id"], cid);
    assert_eq!(
        mayor["linked_conversations"][0]["responses"][0]["id"],
        pending["id"]
    );
}
