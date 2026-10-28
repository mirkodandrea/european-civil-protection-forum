//! EXCON speaks to players as a persona; players never read "EXCON".
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
            "Assistance",
            "112 Dispatch",
            &["mayor".into()],
            "Provide support",
            &Default::default(),
        )
        .unwrap()[0];
    let llm = llm::Llm::mock(|task, _, user| match task {
        "translate" | "feedback" => Ok(user.into()),
        _ => anyhow::bail!("Unexpected interpretation: {task}"),
    });
    let (app, state) = server::router(Arc::new(Mutex::new(ex)), llm, None);
    (Client::untracked(app).await.unwrap(), state, parent)
}

#[tokio::test]
async fn players_see_personas_and_never_excon_across_the_response_flow() {
    let (c, s, parent) = fixture().await;
    let (st, body) = call(
        &c,
        "mayor",
        "/api/player/mayor/actions",
        Some(json!({"narrative":"Attivo assistenza","reply_to":parent})),
    )
    .await;
    assert_eq!(st, Status::Ok, "{body}");
    let id = body["proposal"].as_u64().unwrap();

    // A question asked as a chosen persona.
    let (st, _) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/clarify"),
        Some(json!({"note":"Which shelter?","persona":"Sindaco di Valverde"})),
    )
    .await;
    assert_eq!(st, Status::Ok);
    for _ in 0..40 {
        if s.lock().messages.iter().any(|m| m.kind == exercise::MsgKind::Clarification) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (_, mine) = call(&c, "mayor", "/api/player/mayor", None).await;
    let asked = mine["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "clarification")
        .expect("clarification delivered");
    assert_eq!(asked["persona"], "Sindaco di Valverde");
    assert_eq!(
        mine["clarification_threads"][0]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["author"] == "excon")
            .unwrap()["persona"],
        "Sindaco di Valverde"
    );

    // Approval without a persona: the inject sender speaks.
    let (st, out) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/decide"),
        Some(json!({"approve":true,"note":"Go ahead"})),
    )
    .await;
    assert_eq!(st, Status::Ok, "{out}");
    assert_eq!(out["persona"], "112 Dispatch");

    // Outcome with narrative and an intervention, spoken as another persona.
    let school = json!({"type":"SetBuildingStatus","building":"Pineta Primary School","evacuated":true});
    let (st, out) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/narrative-outcome"),
        Some(json!({"text":"Team sent by EXCON","persona":"Prefettura","commands":[school]})),
    )
    .await;
    assert_eq!(st, Status::Ok, "{out}");
    assert_eq!(out["interventions"].as_array().unwrap().len(), 1);
    // An outcome with neither narrative nor actions is refused.
    let (st, _) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/narrative-outcome"),
        Some(json!({})),
    )
    .await;
    assert_eq!(st, Status::BadRequest);

    let (_, mine) = call(&c, "mayor", "/api/player/mayor", None).await;
    let outcome = &mine["proposals"][0]["narrative_outcomes"][0];
    assert_eq!(outcome["persona"], "Prefettura");
    assert!(!outcome["effects"].as_array().unwrap().is_empty());
    assert_eq!(outcome["text"], "Team sent by Sala operativa");
    // The Italian player reads the scenario sender in Italian.
    assert_eq!(mine["proposals"][0]["decision"]["persona"], "Centrale 112");
    let feedback = mine["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "feedback" && m["persona"] == "Prefettura")
        .expect("outcome message carries its persona");
    assert!(feedback["text"].is_string());
    // The whole player view is free of the word.
    assert!(!mine.to_string().contains("EXCON"), "{mine}");

    // The EXCON side keeps the link to the interventions.
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let linked = &master["proposals"][0]["narrative_outcomes"][0]["interventions"][0];
    let seq = linked["log_seq"].as_u64().unwrap();
    assert!(master["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["seq"] == seq && l["data"]["origin"] == "narrative_outcome"));
}

#[tokio::test]
async fn a_refused_outcome_action_leaves_the_world_and_the_outcome_untouched() {
    let (c, s, parent) = fixture().await;
    let (_, body) = call(
        &c,
        "mayor",
        "/api/player/mayor/actions",
        Some(json!({"narrative":"Attivo assistenza","reply_to":parent})),
    )
    .await;
    let id = body["proposal"].as_u64().unwrap();
    call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    let before = s.lock().world.view();
    let bad = json!({"type":"SetBuildingStatus","building":"Nowhere Hall","evacuated":true});
    let (st, _) = call(
        &c,
        "master",
        &format!("/api/master/proposals/{id}/narrative-outcome"),
        Some(json!({"text":"x","commands":[bad]})),
    )
    .await;
    assert_eq!(st, Status::BadRequest);
    assert_eq!(s.lock().world.view(), before);
    assert!(s.lock().proposal(id).unwrap().narrative_outcomes.is_empty());
}
