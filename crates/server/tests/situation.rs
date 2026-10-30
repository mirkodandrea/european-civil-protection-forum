//! EXCON situation view over HTTP: impact, background preview, suggestions.
use rocket::http::{ContentType, Header, Method, Status};
use rocket::local::asynchronous::Client;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

type TestApp = Arc<Client>;

const MASTER: &str = "sit-master";

fn access() -> server::access::Access {
    server::access::Access::with_keys(
        MASTER.into(),
        BTreeMap::from([("mayor".into(), "sit-mayor".into()), ("fire_commander".into(), "sit-fire".into())]),
        BTreeMap::from([("reviewer".into(), "sit-eval".into())]),
    )
}

async fn app_with(llm: llm::Llm) -> (TestApp, server::AppState) {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.turn_minutes = 60;
    let school = ex
        .world
        .scn
        .buildings
        .iter()
        .find(|b| b.category() == "school")
        .cloned()
        .unwrap();
    ex.direct_command(&sim::Command::Ignite {
        at: sim::Loc::Point { x: school.pos[0] - 150.0, y: school.pos[1] },
        radius_m: Some(60.0),
    })
    .unwrap();
    let (rocket, state) = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access());
    (Arc::new(Client::untracked(rocket).await.unwrap()), state)
}

async fn call(app: &TestApp, key: Option<&str>, method: &str, uri: &str, body: Option<Value>) -> (Status, Value) {
    let mut req = app.req(method.parse::<Method>().unwrap(), uri).header(ContentType::JSON);
    if let Some(key) = key {
        req = req.header(Header::new("Authorization", format!("Bearer {key}")));
    }
    if let Some(body) = body {
        req = req.body(body.to_string());
    }
    let res = req.dispatch().await;
    let status = res.status();
    (status, res.into_json().await.unwrap_or(Value::Null))
}

async fn ready_preview(app: &TestApp) -> Value {
    for _ in 0..400 {
        let (status, v) = call(app, Some(MASTER), "GET", "/api/master/situation/preview", None).await;
        assert_eq!(status, Status::Ok, "{v}");
        if v["state"] == "ready" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the preview never became ready");
}

#[tokio::test]
async fn situation_endpoints_are_excon_only() {
    let (app, _) = app_with(llm::Llm::off()).await;
    for (method, path, body) in [
        ("GET", "/api/master/impact", None),
        ("GET", "/api/master/situation/preview", None),
        ("GET", "/api/master/situation/suggestions", None),
        ("POST", "/api/master/situation/mark", Some(json!({"id":"x","status":"dismissed"}))),
        ("POST", "/api/master/situation/plan", Some(json!({"title":"t","recipients":["mayor"],"text":"t","at_s":900}))),
        ("POST", "/api/master/situation/rewrite", Some(json!({"title":"t","text":"t","voice":"a citizen"}))),
        ("POST", "/api/master/situation/draft", Some(json!({"id":"x"}))),
        ("POST", "/api/master/situation/brief", Some(json!({}))),
    ] {
        assert_eq!(call(&app, Some("sit-mayor"), method, path, body.clone()).await.0, Status::Forbidden, "{path}");
        assert_eq!(call(&app, Some("sit-eval"), method, path, body.clone()).await.0, Status::Forbidden, "{path}");
        assert_eq!(call(&app, None, method, path, body).await.0, Status::Unauthorized, "{path}");
    }
}

#[tokio::test]
async fn impact_preview_and_suggestions_flow() {
    let (app, state) = app_with(llm::Llm::off()).await;
    let (_, impact) = call(&app, Some(MASTER), "GET", "/api/master/impact", None).await;
    assert!(impact["totals"]["buildings"].as_i64().unwrap() > 0);
    assert!(impact["places"].as_array().unwrap().len() >= 1);

    // The preview is computed in the background and then current.
    let first = call(&app, Some(MASTER), "GET", "/api/master/situation/preview", None).await.1;
    assert!(matches!(first["state"].as_str(), Some("computing" | "ready")), "{first}");
    let ready = ready_preview(&app).await;
    assert_eq!(ready["current"], true);
    let preview = &ready["preview"];
    assert_eq!(preview["from_s"], 0);
    assert_eq!(preview["to_s"], 3600);
    assert!(preview["fire"]["raster"].as_str().unwrap().len() > 100);
    assert!(preview["end"]["totals"]["threatened"].as_i64().unwrap() >= 1, "{}", preview["end"]["totals"]);
    let key = ready["result_key"].as_u64().unwrap();

    // Asking again with the key we hold leaves the body out.
    let (_, same) = call(&app, Some(MASTER), "GET", &format!("/api/master/situation/preview?have={key}"), None).await;
    assert_eq!(same["unchanged"], true);
    assert!(same["preview"].is_null());

    // Suggestions come with evidence, and the school is among them.
    let (_, sg) = call(&app, Some(MASTER), "GET", "/api/master/situation/suggestions", None).await;
    assert_eq!(sg["preview_current"], true);
    let list = sg["suggestions"].as_array().unwrap();
    let school = list.iter().find(|s| s["rule"] == "building_reached").unwrap_or_else(|| panic!("{sg}"));
    assert!(school["evidence"]["it"].as_str().unwrap().contains("persone"));
    let id = school["id"].as_str().unwrap().to_string();

    // Nothing was sent or scheduled by computing any of this.
    assert!(state.lock().messages.iter().all(|m| m.kind != exercise::MsgKind::Inject));
    assert!(state.lock().scheduled_injects.is_empty());

    // Planning adds a pending timeline item; it is not sent.
    let at = school["at_s"].as_i64().unwrap_or(900).max(900);
    let (st, plan) = call(
        &app,
        Some(MASTER),
        "POST",
        "/api/master/situation/plan",
        Some(json!({"title":"Evacuate","sender":"112","recipients":["mayor"],"text":"Evacuate the school","at_s":at,"suggestion":id})),
    )
    .await;
    assert_eq!(st, Status::Ok, "{plan}");
    let (_, master) = call(&app, Some(MASTER), "GET", "/api/master", None).await;
    let row = master["planned_injects"].as_array().unwrap().iter().find(|i| i["id_inject"] == plan["id"]).unwrap();
    assert_eq!(row["sent"], false);
    assert!(state.lock().messages.iter().all(|m| m.kind != exercise::MsgKind::Inject));

    // A planned or dismissed suggestion is not proposed again.
    let (_, sg) = call(&app, Some(MASTER), "GET", "/api/master/situation/suggestions", None).await;
    assert!(sg["suggestions"].as_array().unwrap().iter().all(|s| s["id"] != id.as_str()));
    let (bad, _) = call(&app, Some(MASTER), "POST", "/api/master/situation/mark", Some(json!({"id":"x","status":"bogus"}))).await;
    assert_eq!(bad, Status::BadRequest);

    // A decision changes the key, so the old preview is no longer current and gets replaced.
    state.lock().direct_command(&sim::Command::IssueWarning { zone: "F4".into() }).unwrap();
    let (_, after) = call(&app, Some(MASTER), "GET", "/api/master/situation/preview", None).await;
    assert_ne!(after["key"].as_u64().unwrap(), key);
    assert_eq!(after["current"], false);
    let renewed = ready_preview(&app).await;
    assert_ne!(renewed["result_key"].as_u64().unwrap(), key);
}

#[tokio::test]
async fn preview_is_paused_while_a_turn_resolves_and_never_blocks_the_lock() {
    let (app, state) = app_with(llm::Llm::off()).await;
    ready_preview(&app).await;
    state.lock().begin_resolution_until(600, true).unwrap();
    let (_, v) = call(&app, Some(MASTER), "GET", "/api/master/situation/preview", None).await;
    assert_eq!(v["state"], "paused");
    // The last result is still served, flagged as not current.
    assert_eq!(v["current"], false);
}

#[tokio::test]
async fn rewrite_degrades_gracefully_and_uses_the_model_when_there() {
    let (app, _) = app_with(llm::Llm::off()).await;
    let draft = json!({"title":"School","text":"The school is threatened.","voice":"a worried parent calling 112","language":"en"});
    let (st, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/rewrite", Some(draft.clone())).await;
    assert_eq!(st, Status::Ok);
    assert_eq!(v["llm"], false);
    assert_eq!(v["text"], "The school is threatened.");

    let mock = llm::Llm::mock(|task, system, _| {
        assert_eq!(task, "rewrite_inject");
        assert!(system.contains("worried parent"));
        Ok(r#"{"title":"Call from a parent","text":"Please help, the school is in danger!"}"#.into())
    });
    let (app, _) = app_with(mock).await;
    let (_, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/rewrite", Some(draft.clone())).await;
    assert_eq!(v["llm"], true);
    assert_eq!(v["title"], "Call from a parent");

    let broken = llm::Llm::mock(|_, _, _| Ok("not json".into()));
    let (app, _) = app_with(broken).await;
    let (st, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/rewrite", Some(draft)).await;
    assert_eq!(st, Status::Ok);
    assert_eq!(v["llm"], false);
}

/// The open school alert, once the preview is ready.
async fn school_alert(app: &TestApp) -> Value {
    ready_preview(app).await;
    let (_, sg) = call(app, Some(MASTER), "GET", "/api/master/situation/suggestions", None).await;
    sg["suggestions"].as_array().unwrap().iter().find(|s| s["rule"] == "building_reached").cloned().unwrap_or_else(|| panic!("{sg}"))
}

#[tokio::test]
async fn ai_draft_falls_back_to_the_rule_draft_and_uses_the_model_when_there() {
    let (app, state) = app_with(llm::Llm::off()).await;
    let school = school_alert(&app).await;
    let id = school["id"].clone();
    let (st, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/draft", Some(json!({"id": id, "language": "en"}))).await;
    assert_eq!(st, Status::Ok, "{v}");
    assert_eq!(v["llm"], false);
    assert_eq!(v["text"], school["text"]["en"]);
    let (st, _) = call(&app, Some(MASTER), "POST", "/api/master/situation/draft", Some(json!({"id": "building_reached:nope"}))).await;
    assert_eq!(st, Status::BadRequest);

    let mock = llm::Llm::mock(|task, system, user| {
        assert_eq!(task, "draft_inject");
        assert!(system.contains("Do not tell the players what to do"));
        let ctx: Value = serde_json::from_str(user).unwrap();
        assert_eq!(ctx["rule"], "building_reached");
        assert!(ctx["minutes_until_impact"].is_number());
        Ok(r#"{"sender":"Head teacher","title":"Smoke near the school","text":"Smoke is drifting over the playground; 120 pupils are still in class."}"#.into())
    });
    let (app, state2) = app_with(mock).await;
    let id = school_alert(&app).await["id"].clone();
    let (_, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/draft", Some(json!({"id": id}))).await;
    assert_eq!(v["llm"], true, "{v}");
    assert_eq!(v["sender"], "Head teacher");
    assert_eq!(v["title"], "Smoke near the school");
    // A draft is never sent.
    for st in [&state, &state2] {
        assert!(st.lock().messages.iter().all(|m| m.kind != exercise::MsgKind::Inject));
    }
}

#[tokio::test]
async fn ai_brief_crosses_alerts_with_what_the_players_did() {
    let (app, _) = app_with(llm::Llm::off()).await;
    school_alert(&app).await;
    let (st, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/brief", Some(json!({"language":"it"}))).await;
    assert_eq!(st, Status::Ok);
    assert_eq!(v["llm"], false);
    assert!(v["reason"].as_str().unwrap().contains("not configured"));

    let mock = llm::Llm::mock(|task, system, user| {
        assert_eq!(task, "situation_brief");
        assert!(system.contains("Italian"));
        let ctx: Value = serde_json::from_str(user).unwrap();
        let alert = ctx["alerts"].as_array().unwrap().iter().find(|a| a["rule"] == "building_reached").unwrap();
        assert!(ctx["roles"].as_array().unwrap().iter().any(|r| r["activity"]["responses"].is_array()));
        Ok(json!({"summary": "Nessuno sta gestendo la scuola.",
            "items": [{"id": alert["id"], "note": "Nessuno ha nominato la scuola."}, {"id": "invented:1", "note": "x"}]}).to_string())
    });
    let (app, _) = app_with(mock).await;
    let id = school_alert(&app).await["id"].as_str().unwrap().to_string();
    let (_, v) = call(&app, Some(MASTER), "POST", "/api/master/situation/brief", Some(json!({"language":"it"}))).await;
    assert_eq!(v["llm"], true, "{v}");
    assert_eq!(v["items"][&id], "Nessuno ha nominato la scuola.");
    // Notes for alerts that do not exist are dropped.
    assert!(v["items"].get("invented:1").is_none());
    assert_eq!(v["summary"], "Nessuno sta gestendo la scuola.");
}
