//! HTTP flow of the "Risposte" rework (playtest 28/10): communications and
//! decisions, explicit recipients.
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

/// Scenario A over HTTP: the narrated decision is stored and visible to EXCON
/// and to the player; a communication needs no approval.
#[tokio::test]
async fn narrated_decisions_and_communications_are_recorded_under_the_inject() {
    let (c, _, parent) = fixture().await;
    let path = "/api/player/mayor/actions";
    let warning = "Pubblichiamo un avviso alla popolazione";
    let (status, _) = call(&c, "mayor", path, Some(json!({"reply_to":parent,"kind":"decision","narrative":warning}))).await;
    assert_eq!(status, Status::BadRequest, "a decision needs its basis");
    let (status, body) = call(&c, "mayor", path,
        Some(json!({"reply_to":parent,"kind":"decision","basis":"operational_judgment","rationale":"Stated rationale","narrative":warning}))).await;
    assert_eq!(status, Status::Ok, "{body}");
    let (status, body) = call(&c, "mayor", path,
        Some(json!({"reply_to":parent,"kind":"communication","narrative":"Ricevuto","basis":"operational_judgment","rationale":"Presa visione"}))).await;
    assert_eq!(status, Status::Ok, "{body}");
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let row = master["inject_responses"].as_array().unwrap().iter().find(|r| r["inject_message_id"] == parent).unwrap().clone();
    assert_eq!(row["pending_proposals"].as_array().unwrap().len(), 1, "only the decision waits for EXCON");
    let replies = row["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["text"], warning);
    assert_eq!(replies[0]["response"]["addressed_to"]["persona"], "EXCON");
    assert_eq!(replies[1]["response"]["communication"], true);
    let (_, player) = call(&c, "mayor", "/api/player/mayor", None).await;
    let statuses: Vec<_> = player["proposals"].as_array().unwrap().iter().map(|p| p["status"].clone()).collect();
    assert_eq!(statuses, vec![json!("pending"), json!("informational")]);
}

#[tokio::test]
async fn a_response_to_a_player_reaches_them_through_a_conversation() {
    let (c, _, parent) = fixture().await;
    // It opens a conversation, so it states why (personal initiative here).
    let (status, body) = call(&c, "mayor", "/api/player/mayor/actions",
        Some(json!({"reply_to":parent,"kind":"communication","narrative":"Vi giro il messaggio","to_players":["regional_cp"]}))).await;
    assert_eq!(status, Status::BadRequest, "{body}");
    let (status, body) = call(&c, "mayor", "/api/player/mayor/actions",
        Some(json!({"reply_to":parent,"kind":"communication","narrative":"Vi giro il messaggio","to_players":["regional_cp"],"basis":"operational_judgment","rationale":"La Regione deve saperlo"}))).await;
    assert_eq!(status, Status::Ok, "{body}");
    let (_, guest) = call(&c, "regional_cp", "/api/player/regional_cp", None).await;
    let conv = &guest["linked_conversations"][0];
    assert_eq!(conv["messages"][0]["proposal"], body["proposal"]);
    assert_eq!(conv["quote"]["title"], "Smoke", "the inject reference travels with it");
}

/// 30/10 playtest: a reply to the sender states what it rests on (plan,
/// procedure, law or operational judgement), the reference of a plan or law,
/// and why (rationale); EXCON reads all of it next to the reply.
#[tokio::test]
async fn a_reply_carries_its_basis_and_rationale_to_excon() {
    let (c, _, parent) = fixture().await;
    let path = "/api/player/mayor/actions";
    let (status, _) = call(&c, "mayor", path,
        Some(json!({"reply_to":parent,"kind":"communication","narrative":"Apriamo la palestra","basis_note":"Piano comunale"}))).await;
    assert_eq!(status, Status::BadRequest, "a reference says which kind of basis it is");
    let missing = |extra: Value| {
        let mut b = json!({"reply_to":parent,"kind":"communication","narrative":"Apriamo la palestra","basis":"plan",
            "basis_note":"Piano comunale, scheda 4","rationale":"Le famiglie devono attendere al coperto"});
        for (k, v) in extra.as_object().unwrap() { if v.is_null() { b.as_object_mut().unwrap().remove(k); } else { b[k] = v.clone(); } }
        b
    };
    for (gap, error) in [(json!({"basis":null,"basis_note":null}), "basis"), (json!({"basis_note":null}), "which plan"), (json!({"rationale":"  "}), "rationale")] {
        let (status, body) = call(&c, "mayor", path, Some(missing(gap))).await;
        assert_eq!(status, Status::BadRequest, "{body}");
        assert!(body["error"].as_str().unwrap().contains(error), "{body}");
    }
    let (status, body) = call(&c, "mayor", path, Some(missing(json!({})))).await;
    assert_eq!(status, Status::Ok, "{body}");
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let row = master["inject_responses"].as_array().unwrap().iter().find(|r| r["inject_message_id"] == parent).unwrap().clone();
    let reply = &row["replies"][0];
    assert_eq!((reply["basis"].as_str(), reply["basis_note"].as_str()), (Some("plan"), Some("Piano comunale, scheda 4")));
    assert_eq!(reply["rationale"], "Le famiglie devono attendere al coperto");
    assert_eq!(row["pending_proposals"].as_array().unwrap().len(), 0, "a communication waits for nobody");
}
