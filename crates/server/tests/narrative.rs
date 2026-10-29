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
async fn fixture(
    translation_available: bool,
) -> (Client, server::AppState, u64, Arc<Mutex<Vec<String>>>) {
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
            "EXCON",
            &["mayor".into()],
            "Provide support",
            &Default::default(),
        )
        .unwrap()[0];
    let tasks = Arc::new(Mutex::new(Vec::new()));
    let seen = tasks.clone();
    let llm = llm::Llm::mock(move |task, _, user| {
        seen.lock().unwrap().push(task.into());
        match task {
            "translate" | "feedback" => Ok(user.into()),
            _ => anyhow::bail!("Unexpected interpretation: {task}"),
        }
    });
    let (app, state) = server::router(
        Arc::new(Mutex::new(ex)),
        if translation_available {
            llm
        } else {
            llm::Llm::off()
        },
        None,
    );
    (Client::untracked(app).await.unwrap(), state, parent, tasks)
}
#[tokio::test]
async fn narrative_routes_are_translation_only_and_private_and_do_not_change_physics() {
    let (c, s, parent, tasks) = fixture(true).await;
    let before = s.lock().world.view();
    let (status, body) = call(
        &c,
        "mayor",
        "/api/player/mayor/actions",
        Some(json!({"kind":"decision","basis":"operational_judgment","narrative":"Attivo assistenza psicologica","reply_to":parent})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let id = body["proposal"].as_u64().unwrap();
    // Besides translation, only the EXCON-side reading (suggestions for the
    // EXCON, never executed by itself) may look at the narrative.
    assert!(tasks.lock().unwrap().iter().all(|t| t == "translate" || t == "review_reply"));
    let (_, master) = call(&c, "master", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["narrative"], true);
    assert_eq!(
        master["inject_responses"][0]["pending_proposals"][0]["id"],
        id
    );
    let (_, other) = call(&c, "fire_commander", "/api/player/fire_commander", None).await;
    assert!(other["proposals"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &c,
            "mayor",
            &format!("/api/master/proposals/{id}/narrative-outcome"),
            Some(json!({"text":"done"}))
        )
        .await
        .0,
        Status::Forbidden
    );
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
        Some(json!({"text":"Support team assigned by Sala operativa"})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{body}");
    let (_, mine) = call(&c, "mayor", "/api/player/mayor", None).await;
    assert_eq!(
        mine["proposals"][0]["narrative_outcomes"][0]["text"],
        "Support team assigned by Sala operativa"
    );
    assert_eq!(mine["proposals"][0]["status"], "approved");
    assert_eq!(s.lock().world.view(), before);
    assert!(tasks
        .lock()
        .unwrap()
        .iter()
        .all(|t| matches!(t.as_str(), "translate" | "feedback" | "review_reply")));
}
#[tokio::test]
async fn narrative_routes_reject_other_roles_injects_and_executable_payloads() {
    let (c, _, parent, _) = fixture(true).await;
    let body = json!({"kind":"decision","basis":"operational_judgment","narrative":"Support","reply_to":parent});
    assert_eq!(
        call(
            &c,
            "fire_commander",
            "/api/player/mayor/actions",
            Some(body.clone())
        )
        .await
        .0,
        Status::Forbidden
    );
    assert_eq!(
        call(
            &c,
            "fire_commander",
            "/api/player/fire_commander/actions",
            Some(body.clone())
        )
        .await
        .0,
        Status::BadRequest
    );
    // An EXCON-only command is outside the role's authority.
    let mut injected = body.clone();
    injected["commands"] = json!([{"type":"Ignite","at":"F4"}]);
    assert_ne!(
        call(&c, "mayor", "/api/player/mayor/actions", Some(injected)).await.0,
        Status::Ok
    );
    // `kind` is required: without it the request is refused.
    let mut no_kind = body;
    no_kind.as_object_mut().unwrap().remove("kind");
    assert_eq!(
        call(&c, "mayor", "/api/player/mayor/actions", Some(no_kind)).await.0,
        Status::UnprocessableEntity
    );
}

#[tokio::test]
async fn unavailable_translation_still_delivers_original_narrative_for_excon_review() {
    let (c, s, parent, _) = fixture(false).await;
    let (status, got) = call(
        &c,
        "mayor",
        "/api/player/mayor/actions",
        Some(json!({"kind":"decision","basis":"operational_judgment","narrative":"Richiedo supporto psicologico","reply_to":parent})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{got}");
    assert_eq!(got["translation_failed"], true);
    let id = got["proposal"].as_u64().unwrap();
    let ex = s.lock();
    let p = ex.proposal(id).unwrap();
    assert_eq!(p.original, "Richiedo supporto psicologico");
    assert_eq!(p.status, exercise::ProposalStatus::Pending);
    assert!(ex
        .messages
        .iter()
        .any(|m| m.proposal == Some(id) && m.translation_failed));
}
