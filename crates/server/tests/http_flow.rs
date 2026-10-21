//! The full EXCON ↔ player workflow over HTTP, with a mock LLM.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn mock_llm() -> llm::Llm {
    llm::Llm::mock(|task, _system, user| {
        Ok(match task {
            "translate" => format!("[it] {user}"),
            "prepare_decision" if user.contains("where?") && !user.contains("F4 confirmed") => r#"{"message":"Warn the area","detected_language":"en","translation_en":"Warn the area","actions":[],"messages":[],"clarification":"Which zone should receive the warning?","summary":"A zone is missing"}"#.into(),
            "prepare_decision" => r#"{"message":"Warn F4","detected_language":"en","translation_en":"Warn F4","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[],"clarification":null,"summary":"Warn F4"}"#.into(),
            "parse_reply" if user.contains("where?") => r#"{"detected_language":"it","translation_en":"where?","actions":[],"messages":[],"clarification":"Quale zona?","summary":"unclear"}"#.into(),
            "parse_reply" => r#"{"detected_language":"it","translation_en":"Warn F4 and tell the fire commander","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[{"to":"fire_commander","text":"We warned F4"}],"clarification":null,"summary":"Warn F4"}"#.into(),
            "master_prompt" => r#"{"actions":[{"type":"SetWeather","wind_speed_kmh":60}],"injects":[{"recipients":["all"],"title":"Wind","sender":"Weather service","text":"Wind rising to 60 km/h"}],"clarification":null,"summary":"stronger wind"}"#.into(),
            "feedback" => format!("[feedback] {user}"),
            "situation_update" => "[situation] all quiet".into(),
            other => anyhow::bail!("unexpected task {other}"),
        })
    })
}

fn access() -> server::access::Access {
    server::access::Access {
        master: "test-master".into(),
        players: std::collections::BTreeMap::from([
            ("mayor".into(), "test-mayor".into()),
            ("fire_commander".into(), "test-fire".into()),
        ]),
        evaluators: std::collections::BTreeMap::from([
            ("eval-one".into(), "test-eval-one".into()),
            ("eval-two".into(), "test-eval-two".into()),
        ]),
    }
}

async fn app() -> Router {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    server::router_with_access(Arc::new(Mutex::new(ex)), mock_llm(), None, access()).0
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    mut body: Option<Value>,
) -> (StatusCode, Value) {
    if method == "POST" && uri.ends_with("/reply") {
        if let Some(body) = body.as_mut() {
            if body.get("evidence").is_none() {
                body["evidence"] = json!({"basis":{"kind":"operational_judgment", "reference":"Operational judgment based on the received inject"}});
            }
        }
    }
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-master");
    let req = req
        .body(
            body.map(|b| Body::from(b.to_string()))
                .unwrap_or(Body::empty()),
        )
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn reply_parent(app: &Router, role: &str) -> u64 {
    let (_, p) = call(app, "GET", &format!("/api/player/{role}"), None).await;
    call(
        app,
        "POST",
        &format!("/api/player/{role}/language"),
        Some(json!({"language":p["player"]["language"]})),
    )
    .await;
    if let Some(m) = p["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["kind"] == "inject")
    {
        return m["id"].as_u64().unwrap();
    }
    if p["phase"] != "orders" {
        return 0;
    }
    let (_, sent) = call(
        app,
        "POST",
        "/api/master/inject",
        Some(json!({"title":"Task", "recipients":[role], "text":"Exercise task"})),
    )
    .await;
    sent["messages"][0].as_u64().unwrap()
}

/// Poll `GET uri` until `pred` holds (LLM work runs in spawned tasks).
async fn eventually(app: &Router, uri: &str, pred: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..100 {
        let (_, v) = call(app, "GET", uri, None).await;
        if pred(&v) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition never met for {uri}");
}

#[tokio::test]
async fn master_player_llm_round_trip() {
    let app = app().await;
    let (s, v) = call(&app, "GET", "/api/static", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["roles"].as_array().unwrap().len(), 3);
    assert!(v["msel"].as_array().unwrap().len() >= 5);

    // Replies are refused before the exercise starts.
    let (s, _) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text": "hi", "reply_to": reply_parent(&app, "mayor").await})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    assert_eq!(
        call(&app, "POST", "/api/master/start", None).await.0,
        StatusCode::OK
    );

    // MSEL inject, translated for the Italian-speaking mayor only.
    let (s, _) = call(&app, "POST", "/api/master/msel/I-01/send", None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, mayor) = call(&app, "GET", "/api/player/mayor", None).await;
    assert!(mayor["inbox"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("[it] "));
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!fc["inbox"][0]["text"].as_str().unwrap().starts_with("[it]"));

    // Player reply -> parsed proposal + message delivered to the fire commander.
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "Allertate F4 e avvisate i pompieri", "reply_to": reply_parent(&app, "mayor").await}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    let m = eventually(&app, "/api/master", |m| {
        m["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == pid && p["status"] == "pending")
    })
    .await;
    let p = m["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == pid)
        .unwrap()
        .clone();
    assert_eq!(p["commands"][0]["type"], "IssueWarning");
    assert_eq!(p["verdicts"][0]["verdict"], "ok");
    let (_, before) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(
        !before["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "role_message"),
        "operational AI drafts await EXCON approval and execution"
    );

    // EXCON approves -> translated feedback to the player.
    let (s, out) = call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve": true})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{out}");
    eventually(&app, "/api/player/mayor", |v| {
        v["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "feedback" && m["text"].as_str().unwrap().starts_with("[it]"))
    })
    .await;

    // Unclear reply -> clarification question back to the player.
    let (_, r) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text": "evacuate where?", "reply_to": reply_parent(&app, "mayor").await})),
    )
    .await;
    let pid2 = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |v| {
        v["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == pid2 && p["status"] == "needs_clarification")
    })
    .await;
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid2}/decide"),
        Some(json!({"approve": true, "note": "Send the clarification"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    eventually(&app, "/api/player/mayor", |v| {
        v["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "clarification")
    })
    .await;

    // EXCON prompt -> proposal with a command and an inject draft; approval applies both.
    let (_, r) = call(
        &app,
        "POST",
        "/api/master/prompt",
        Some(json!({"text": "make the wind much stronger and tell everyone"})),
    )
    .await;
    let mp = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == mp && p["status"] == "pending")
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{mp}/decide"),
        Some(json!({"approve": true})),
    )
    .await;
    let st = eventually(&app, "/api/state", |v| {
        v["world"]["weather"]["wind_speed_kmh"] == 60.0
    })
    .await;
    assert_eq!(st["turn"], 1);
    eventually(&app, "/api/player/regional_cp", |v| {
        v["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["title"] == "Wind")
    })
    .await;

    // Direct commands "by code": a list, one of which fails.
    let (s, r) = call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!([{"type":"Extinguish","area":"G3"},{"type":"WaterDrop","at":"Atlantis"}])),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["results"][0]["ok"], true);
    assert_eq!(r["results"][1]["ok"], false);
    let (s, _) = call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"NotACommand"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Resolve the turn -> applied actions, time advanced, situation updates to every player.
    let (s, rep) = call(
        &app,
        "POST",
        "/api/master/resolve",
        Some(json!({"minutes": 30})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(rep["to_s"], 1800);
    assert!(rep["applied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["action"] == "Issue warning to zone F4" && a["ok"] == true));
    eventually(&app, "/api/player/fire_commander", |v| {
        v["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "role_message")
    })
    .await;
    for role in ["mayor", "fire_commander", "regional_cp"] {
        eventually(&app, &format!("/api/player/{role}"), |v| {
            v["inbox"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["kind"] == "situation")
        })
        .await;
    }
    let (_, st) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(st["turn"], 2);
    assert_eq!(st["world"]["stats"]["clock"], "Day 1 13:30");

    // Rollback and debrief.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/rollback",
            Some(json!({"turn": 1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, st) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(st["turn"], 1);
    assert_eq!(
        call(&app, "POST", "/api/master/end", None).await.0,
        StatusCode::OK
    );
    let (_, d) = call(&app, "GET", "/api/debrief", None).await;
    assert!(d["log"].as_array().unwrap().len() > 10);
}

#[tokio::test]
async fn works_without_llm() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let app =
        server::router_with_access(Arc::new(Mutex::new(ex)), llm::Llm::off(), None, access()).0;
    call(&app, "POST", "/api/master/start", None).await;
    call(
        &app,
        "POST",
        "/api/master/inject",
        Some(json!({"title": "t", "recipients": ["mayor"], "text": "english only"})),
    )
    .await;
    let (_, m) = call(&app, "GET", "/api/player/mayor", None).await;
    assert_eq!(m["inbox"][0]["text"], "english only");
    let (_, r) = call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "line along G3 south edge", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == pid && p["status"] == "pending")
    })
    .await;
    // The EXCON types the command for the player.
    let (s, out) = call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve": true, "commands": [{"type":"CutFireLine","along":"G3:S"}]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{out}");
    let (_, rep) = call(&app, "POST", "/api/master/resolve", None).await;
    assert_eq!(rep["applied"][0]["ok"], true);
    eventually(&app, "/api/player/fire_commander", |v| {
        v["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "situation")
    })
    .await;
}

/// With a pace set, a resolving turn is visible from outside: phase
/// `resolving`, simulated time advancing minute by minute, replies refused.
#[tokio::test]
async fn resolution_runs_live_and_is_observable() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    // 60x: one simulated minute per second, a 20-minute turn in ~20 s... too slow for a test: 600x → ~2 s.
    let (_, set) = call(
        &app,
        "POST",
        "/api/master/settings",
        Some(json!({"speed": 600.0})),
    )
    .await;
    assert_eq!(set["speed"], 600.0);
    let a2 = app.clone();
    let resolver = tokio::spawn(async move {
        call(
            &a2,
            "POST",
            "/api/master/resolve",
            Some(json!({"minutes": 20})),
        )
        .await
    });

    let mut seen_times = std::collections::BTreeSet::new();
    for _ in 0..200 {
        let (_, st) = call(&app, "GET", "/api/state", None).await;
        if st["phase"] == "resolving" {
            assert_eq!(st["resolving"]["to_s"], 1200);
            seen_times.insert(st["world"]["stats"]["time_s"].as_i64().unwrap());
            let (s, _) = call(
                &app,
                "POST",
                "/api/player/mayor/reply",
                Some(json!({"text": "hi", "reply_to": reply_parent(&app, "mayor").await})),
            )
            .await;
            assert_eq!(
                s,
                StatusCode::BAD_REQUEST,
                "replies are refused while resolving"
            );
        }
        if resolver.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let (s, rep) = resolver.await.unwrap();
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(rep["to_s"], 1200);
    assert!(
        seen_times.len() >= 5,
        "intermediate times should be observable, saw {seen_times:?}"
    );
    assert!(seen_times.iter().all(|t| *t < 1200), "{seen_times:?}");
    let (_, st) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(st["phase"], "orders");
    assert_eq!(st["turn"], 2);
}

/// The AI sees each player's conversation: what they received, their earlier
/// replies and its own earlier interpretations (a clarification is answered
/// in context).
#[tokio::test]
async fn player_conversation_keeps_context() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    let llm = llm::Llm::mock(move |task, _s, user| {
        if task == "parse_reply" {
            log.lock().unwrap().push(user.to_string());
            return Ok(
                if user.contains("over there") && !user.contains("east edge") {
                    r#"{"actions":[],"clarification":"Which zone edge?","summary":"unclear"}"#
                        .into()
                } else {
                    r#"{"actions":[{"type":"CutFireLine","along":"F4:E"}],"summary":"line"}"#.into()
                },
            );
        }
        Ok(format!("ok {task}"))
    });
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access()).0;
    call(&app, "POST", "/api/master/start", None).await;
    call(&app, "POST", "/api/master/msel/I-03/send", None).await;
    call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "send the crews over there", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    eventually(&app, "/api/master", |m| {
        m["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["status"] == "needs_clarification")
    })
    .await;
    call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "the east edge of F4", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    eventually(&app, "/api/master", |m| {
        m["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["status"] == "pending")
            .count()
            == 1
    })
    .await;
    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[0].contains("INJECT") && calls[0].contains("Fire behaviour estimate"),
        "the inject is context: {}",
        calls[0]
    );
    assert!(
        calls[1].contains("send the crews over there"),
        "earlier reply in history"
    );
    assert!(
        calls[1].contains("Which zone edge?"),
        "earlier clarification in history"
    );
    assert!(calls[1].contains("the east edge of F4"));
}

#[tokio::test]
async fn reply_api_rejects_missing_unknown_and_other_roles_parent() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, sent) = call(
        &app,
        "POST",
        "/api/master/inject",
        Some(json!({"title":"Fire", "recipients":["mayor"], "text":"Reported fire"})),
    )
    .await;
    let parent = sent["messages"][0].as_u64().unwrap();
    call(
        &app,
        "POST",
        "/api/player/mayor/language",
        Some(json!({"language":"it"})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/reply",
            Some(json!({"text":"hello"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/reply",
            Some(json!({"text":"hello", "reply_to":99999}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/fire_commander/reply",
            Some(json!({"text":"hello", "reply_to":parent}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["proposals"].as_array().unwrap().is_empty());
    let (status, reply) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"hello", "reply_to":parent, "declaration_kind":"request"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["reply_to"], parent);
    assert_eq!(master["proposals"][0]["declaration_kind"], "request");
    assert_eq!(master["proposals"][0]["history"][0]["status"], "parsing");
    assert_eq!(master["messages"][1]["proposal"], reply["proposal"]);
    assert_eq!(
        master["messages"][1]["inject_id"],
        master["messages"][0]["inject_id"]
    );
}

#[tokio::test]
async fn operational_drafts_require_master_review_and_deliver_edited_text_once() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let (app, state) =
        server::router_with_access(Arc::new(Mutex::new(ex)), mock_llm(), None, access());
    call(&app, "POST", "/api/master/start", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let (_, r) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"warn F4", "reply_to":parent})),
    )
    .await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!fc["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["kind"] == "role_message"));
    let edited = json!([{"to":"fire_commander","from":"mayor","kind":"role_message","title":"Warning coordination","text_en":"The warning to F4 is now issued; please confirm receipt."}]);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true, "communication_drafts":edited})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(
        !fc["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "role_message"),
        "action-dependent message waits for actual execution"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/resolve",
            Some(json!({"minutes":1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    eventually(&app, "/api/player/fire_commander", |p| {
        p["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "role_message")
    })
    .await;
    server::flow::send_drafts(&state, pid).await;
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    let delivered = fc["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["kind"] == "role_message")
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0]["text_en"],
        "The warning to F4 is now issued; please confirm receipt."
    );
    assert_eq!(delivered[0]["proposal"], pid);
    let (_, m) = call(&app, "GET", "/api/master", None).await;
    assert!(m["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "communications_reviewed"));
    assert!(m["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "approved_communication_delivered"));
}

#[tokio::test]
async fn rejected_ai_suggestions_never_deliver_or_change_world() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, before) = call(&app, "GET", "/api/state", None).await;
    let (_, r) = call(
        &app,
        "POST",
        "/api/master/prompt",
        Some(json!({"text":"stronger wind and tell everyone"})),
    )
    .await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    let (_, proposed) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(proposed["world"], before["world"]);
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":false})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/master/resolve",
        Some(json!({"minutes":1})),
    )
    .await;
    let (_, state) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(state["world"]["weather"], before["world"]["weather"]);
    for role in ["mayor", "fire_commander", "regional_cp"] {
        let (_, p) = call(&app, "GET", &format!("/api/player/{role}"), None).await;
        assert!(!p["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["title"] == "Wind"));
    }
}

#[tokio::test]
async fn api_requires_basis_and_exposes_supporting_evidence_through_lifecycle() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/player/mayor/reply")
        .header("content-type", "application/json")
        .header("authorization", "Bearer test-master")
        .body(Body::from(
            json!({"text":"warn F4","reply_to":parent}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let evidence = json!({"basis":{"kind":"plan","reference":"Municipal warning plan, annex 3"},"attachments":[{"title":"Area map","url":"https://example.test/map.pdf"}]});
    let invalid = json!({"basis":{"kind":"plan","reference":""}});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/reply",
            Some(json!({"text":"warn F4","reply_to":parent,"evidence":invalid}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/language",
            Some(json!({"language":"unsupported"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (s, r) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"warn F4","reply_to":parent,"evidence":evidence})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/master/resolve",
        Some(json!({"minutes":1})),
    )
    .await;
    let (_, p) = call(&app, "GET", "/api/player/mayor", None).await;
    assert_eq!(p["proposals"][0]["evidence"], evidence);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["evidence"], evidence);
    call(
        &app,
        "POST",
        "/api/master/rollback",
        Some(json!({"turn":1})),
    )
    .await;
    let (_, d) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(d["proposals"][0]["evidence"], evidence);
}

async fn call_key(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(key) = key {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let resp = app
        .clone()
        .oneshot(
            req.body(
                body.map(|v| Body::from(v.to_string()))
                    .unwrap_or(Body::empty()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn role_keys_protect_all_authoritative_reads_and_actions() {
    let app = app().await;
    let (_, mut before) = call(&app, "GET", "/api/state", None).await;
    before.as_object_mut().unwrap().remove("wall_ms");
    for path in [
        "/api/state",
        "/api/master",
        "/api/debrief",
        "/api/catalogue",
        "/api/master/access",
        "/api/player/fire_commander",
    ] {
        assert_eq!(
            call_key(&app, "GET", path, None, Some("test-mayor"))
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
        assert_eq!(
            call_key(&app, "GET", path, None, None).await.0,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    for path in [
        "/api/master/start",
        "/api/master/end",
        "/api/master/command",
        "/api/master/inject",
        "/api/master/msel/I-01/send",
        "/api/master/proposals/1/decide",
        "/api/master/prompt",
        "/api/master/resolve",
        "/api/master/rollback",
        "/api/master/settings",
        "/api/master/resource-limits",
        "/api/master/clock",
        "/api/master/phase",
        "/api/master/schedules",
        "/api/master/schedules/1/cancel",
        "/api/master/proposals/1/execute",
        "/api/master/knowledge",
        "/api/player/fire_commander/reply",
        "/api/player/fire_commander/language",
    ] {
        assert_eq!(
            call_key(&app, "POST", path, Some(json!({})), Some("test-mayor"))
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    assert_eq!(
        call_key(&app, "GET", "/api/state?token=test-master", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call_key(&app, "GET", "/ws", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call_key(&app, "GET", "/api/health", None, None).await,
        (StatusCode::OK, json!({"ok":true}))
    );
    let (_, mut after) = call(&app, "GET", "/api/state", None).await;
    after.as_object_mut().unwrap().remove("wall_ms");
    assert_eq!(before, after);
    let (_, own) = call_key(&app, "GET", "/api/static", None, Some("test-mayor")).await;
    assert!(
        own.get("msel").is_none()
            && own.get("reinforcements").is_none()
            && own.get("description").is_none()
    );
    assert_eq!(own["roles"].as_array().unwrap().len(), 1);
    assert!(own["buildings"][0].get("occupants").is_none());
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/player/mayor/language",
            Some(json!({"language":"it"})),
            Some("test-mayor")
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn map_reports_are_delivered_per_role_and_never_track_hidden_world_changes() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let report = json!({"locations":[[2500,2500]],"cells":[{"row":3,"col":4,"state":1}]});
    let (status, sent) = call(&app, "POST", "/api/master/inject", Some(json!({"title":"Field report","recipients":["mayor"],"text":"A fire is reported at these coordinates.","observation":report}))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, mayor) = call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor")).await;
    assert_eq!(mayor["map"]["cells"], report["cells"]);
    assert_eq!(mayor["knowledge_events"][0]["message"], sent["messages"][0]);
    assert!(
        mayor.get("world").is_none()
            && mayor["map"].get("fire").is_none()
            && mayor["map"].get("stats").is_none()
    );
    let (_, fire) = call_key(
        &app,
        "GET",
        "/api/player/fire_commander",
        None,
        Some("test-fire"),
    )
    .await;
    assert!(fire["map"]["cells"].as_array().unwrap().is_empty());
    assert!(fire["knowledge_events"].as_array().unwrap().is_empty());
    call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"Ignite","at":"G5","radius_m":60})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/master/resolve",
        Some(json!({"minutes":1})),
    )
    .await;
    let (_, unchanged) = call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor")).await;
    assert_eq!(unchanged["map"], mayor["map"]);
    let update = json!({"role":"mayor","source":"Field team observation","observation":{"cells":[{"row":3,"col":4,"state":2},{"row":3,"col":5,"state":1}]}});
    assert_eq!(
        call(&app, "POST", "/api/master/knowledge", Some(update))
            .await
            .0,
        StatusCode::OK
    );
    let (_, updated) = call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor")).await;
    assert_eq!(updated["map"]["cells"].as_array().unwrap().len(), 2);
    assert_eq!(updated["map"]["cells"][0]["state"], 2);
    let (_, before_invalid) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(call(&app, "POST", "/api/master/inject", Some(json!({"title":"Invalid","recipients":["mayor"],"text":"bad report","observation":{"cells":[{"row":999999,"col":0,"state":1}]}}))).await.0, StatusCode::BAD_REQUEST);
    let (_, after_invalid) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(before_invalid["messages"], after_invalid["messages"]);
    assert_eq!(call(&app, "POST", "/api/master/knowledge", Some(json!({"role":"fire_commander","source":"Reviewed aerial perimeter","current_perimeter":true}))).await.0, StatusCode::OK);
    let (_, acquired) = call_key(
        &app,
        "GET",
        "/api/player/fire_commander",
        None,
        Some("test-fire"),
    )
    .await;
    assert!(!acquired["map"]["cells"].as_array().unwrap().is_empty());
    let (_, debrief) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(debrief["knowledge_events"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn multiple_evaluators_can_read_master_evidence_but_never_mutate() {
    let app = app().await;
    let (_, before) = call(&app, "GET", "/api/debrief", None).await;
    for key in ["test-eval-one", "test-eval-two"] {
        for path in [
            "/api/static",
            "/api/state",
            "/api/master",
            "/api/catalogue",
            "/api/debrief",
            "/api/evaluator",
        ] {
            assert_eq!(
                call_key(&app, "GET", path, None, Some(key)).await.0,
                StatusCode::OK,
                "{key} {path}"
            );
        }
        assert_eq!(
            call_key(&app, "GET", "/api/master/access", None, Some(key))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        for path in [
            "/api/master/start",
            "/api/master/end",
            "/api/master/command",
            "/api/master/inject",
            "/api/master/msel/I-01/send",
            "/api/master/proposals/1/decide",
            "/api/master/prompt",
            "/api/master/resolve",
            "/api/master/rollback",
            "/api/master/settings",
            "/api/master/resource-limits",
            "/api/master/clock",
            "/api/master/phase",
            "/api/master/schedules",
            "/api/master/schedules/1/cancel",
            "/api/master/proposals/1/execute",
            "/api/master/knowledge",
            "/api/master/references",
            "/api/player/mayor/reply",
            "/api/player/mayor/language",
            "/api/evaluator",
        ] {
            assert_eq!(
                call_key(&app, "POST", path, Some(json!({})), Some(key))
                    .await
                    .0,
                StatusCode::FORBIDDEN,
                "{key} {path}"
            );
        }
    }
    let (_, after) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(before, after);
    let (s, identity) = call_key(&app, "GET", "/api/access", None, Some("test-eval-two")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(identity, json!({"kind":"evaluator","id":"eval-two"}));
    assert_eq!(
        call_key(&app, "GET", "/api/evaluator", None, Some("test-mayor"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn evaluator_filters_trace_retained_chain_objectives_and_reference_materials() {
    let app = app().await;
    let references = json!([{"basis":{"kind":"procedure","reference":"Warning procedure, section 3"},"attachments":[{"title":"Procedure","url":"https://example.test/procedure.pdf"}]}]);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/references",
            Some(json!({"references":references}))
        )
        .await
        .0,
        StatusCode::OK
    );
    call(&app, "POST", "/api/master/start", None).await;
    call(&app, "POST", "/api/master/msel/I-01/send", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let evidence = json!({"basis":{"kind":"procedure","reference":"Warning procedure, section 3"},"attachments":[{"title":"Map","url":"https://example.test/map.pdf"}]});
    let (_, response) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"warn F4","reply_to":parent,"evidence":evidence})),
    )
    .await;
    let pid = response["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true,"note":"Follow the procedure"})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/master/resolve",
        Some(json!({"minutes":1})),
    )
    .await;
    let (s, view) = call_key(
        &app,
        "GET",
        "/api/evaluator?role=mayor&turn=1&phase=orders&status=applied",
        None,
        Some("test-eval-one"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(view["chains"].as_array().unwrap().len(), 1);
    let chain = &view["chains"][0];
    assert_eq!(chain["proposal"]["evidence"], evidence);
    assert_eq!(chain["proposal"]["decision"]["approve"], true);
    assert_eq!(chain["proposal"]["executions"][0]["ok"], true);
    assert!(chain["reply_delay_wall_ms"].is_number() && chain["master_queue_wall_ms"].is_number());
    assert!(chain["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == parent));
    assert!(view["messages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["to"] == "mayor" || m["from"] == "mayor"));
    assert_eq!(view["reference_materials"], references);
    assert_eq!(view["objectives"][0]["msel_id"], "I-01");
    assert!(!view["objectives"][0]["objective"]
        .as_str()
        .unwrap()
        .is_empty());
    let inject = chain["proposal"]["inject_id"].as_u64().unwrap();
    let (_, filtered) = call_key(
        &app,
        "GET",
        &format!("/api/evaluator?inject={inject}&kind=message&limit=1"),
        None,
        Some("test-eval-two"),
    )
    .await;
    assert_eq!(filtered["timeline"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["timeline"][0]["kind"], "message");
    assert!(filtered["timeline_total"].as_u64().unwrap() > 1);
    let (_, older) = call_key(
        &app,
        "GET",
        &format!("/api/evaluator?inject={inject}&kind=message&limit=1&offset=1"),
        None,
        Some("test-eval-two"),
    )
    .await;
    assert_ne!(filtered["timeline"][0]["seq"], older["timeline"][0]["seq"]);
    call(
        &app,
        "POST",
        "/api/master/rollback",
        Some(json!({"turn":1})),
    )
    .await;
    let (_, rolled) = call_key(
        &app,
        "GET",
        "/api/evaluator?role=mayor&status=rolled_back",
        None,
        Some("test-eval-one"),
    )
    .await;
    assert_eq!(rolled["chains"][0]["proposal"]["evidence"], evidence);
    assert_eq!(rolled["chains"][0]["proposal"]["executions"][0]["ok"], true);
    let (_, export) = call_key(&app, "GET", "/api/debrief", None, Some("test-eval-two")).await;
    assert_eq!(export["reference_materials"], references);
    assert_eq!(export["proposals"][0]["evidence"], evidence);
    assert!(export["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "rollback"));
    assert_eq!(
        call_key(
            &app,
            "GET",
            "/api/evaluator?role=unknown",
            None,
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call_key(
            &app,
            "GET",
            "/api/evaluator?status=unknown",
            None,
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn three_scheduled_injects_arrive_in_one_turn_and_training_phase() {
    let app = app().await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/clock",
            Some(json!({"running":true,"ratio":60}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    call(&app, "POST", "/api/master/start", None).await;
    for (id, clock) in [("I-01", "13:00"), ("I-02", "13:30"), ("I-03", "14:00")] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/master/schedules",
                Some(json!({"msel_id":id,"at_clock":clock}))
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/clock",
            Some(json!({"running":true,"ratio":86400,"reason":"Compressed acceptance exercise"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let delivered = eventually(&app, "/api/state", |s| {
        s["scheduled_injects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["status"] == "delivered")
    })
    .await;
    assert_eq!(delivered["turn"], 1);
    assert_eq!(delivered["phase"], "orders");
    assert_eq!(delivered["exercise_phase"], "early_warning");
    assert_eq!(
        delivered["scheduled_injects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["triggered_s"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1800, 3600]
    );
    for i in delivered["scheduled_injects"].as_array().unwrap() {
        assert!(i["delivered_s"].as_i64().unwrap() >= i["due_s"].as_i64().unwrap());
        assert!(
            i["delivered_wall_ms"].as_u64().unwrap() >= i["triggered_wall_ms"].as_u64().unwrap()
        );
        assert!(!i["messages"].as_array().unwrap().is_empty());
    }
    call(
        &app,
        "POST",
        "/api/master/clock",
        Some(json!({"running":false,"ratio":60,"reason":"Review phase"})),
    )
    .await;
    let (_, paused) = call(&app, "GET", "/api/state", None).await;
    tokio::time::sleep(Duration::from_millis(70)).await;
    let (_, still) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(paused["world"], still["world"]);
    call(
        &app,
        "POST",
        "/api/master/phase",
        Some(json!({"phase":"early_action","reason":"Begin protective measures"})),
    )
    .await;
    let (_, next) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(next["turn"], 1);
    assert_eq!(next["world"], paused["world"]);
    assert_eq!(next["exercise_phase"], "early_action");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/phase",
            Some(json!({"phase":"escalation","reason":""}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/clock",
            Some(json!({"running":true,"ratio":0}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/schedules",
            Some(json!({"msel_id":"I-01","at_clock":"25:00"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, debrief) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(debrief["scheduled_injects"].as_array().unwrap().len(), 3);
    assert!(debrief["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "clock_configured"));
    assert!(debrief["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "exercise_phase_changed"));
}

#[tokio::test]
async fn scheduled_delivery_cancellation_and_approved_execution_are_master_only() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, s) = call(
        &app,
        "POST",
        "/api/master/schedules",
        Some(json!({"msel_id":"I-01","at_s":3600})),
    )
    .await;
    let id = s["schedule"].as_u64().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/master/schedules/{id}/cancel"),
            Some(json!({"reason":"Change the scenario sequence"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let parent = reply_parent(&app, "mayor").await;
    let (_, r) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"warn F4","reply_to":parent})),
    )
    .await;
    let pid = r["proposal"].as_u64().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/master/proposals/{pid}/execute"),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/master/proposals/{pid}/execute"),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/master/proposals/{pid}/execute"),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, state) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(state["turn"], 1);
    assert_eq!(state["scheduled_injects"][0]["status"], "cancelled");
    let (_, e) = call_key(
        &app,
        "GET",
        "/api/evaluator?exercise_phase=early_warning",
        None,
        Some("test-eval-one"),
    )
    .await;
    assert_eq!(e["chains"][0]["proposal"]["status"], "applied");
    assert!(e["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .all(|l| l["exercise_phase"] == "early_warning"));
}

#[tokio::test]
async fn ai_resource_limits_are_recommendations_until_reviewed_and_approved() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let llm = llm::Llm::mock(|task, _, _| {
        if task == "master_prompt" {
            Ok(r#"{"resource_limits":{"total":{"ground_crews":1},"phases":{"early_warning":{"ground_crews":1}}},"actions":[],"summary":"Limit first-phase deployment to one crew"}"#.into())
        } else {
            anyhow::bail!("unexpected task")
        }
    });
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access()).0;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, before) = call(&app, "GET", "/api/state", None).await;
    let (_, p) = call(
        &app,
        "POST",
        "/api/master/prompt",
        Some(json!({"text":"Recommend initial crew deployment limits"})),
    )
    .await;
    let pid = p["proposal"].as_u64().unwrap();
    let proposal = eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    assert_eq!(
        proposal["proposals"][0]["resource_limit_draft"]["total"]["ground_crews"],
        1
    );
    let (_, pending) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(pending["resource_limits"], before["resource_limits"]);
    assert_eq!(pending["world"], before["world"]);
    assert_eq!(call(&app,"POST",&format!("/api/master/proposals/{pid}/decide"),Some(json!({"approve":true,"resource_limits":{"total":{"ground_crews":2},"phases":{"early_warning":{"ground_crews":2}}},"note":"EXCON edits recommendation to two crews"}))).await.0,StatusCode::OK);
    let (_, effective) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(effective["resource_limits"]["total"]["ground_crews"], 2);
    assert_eq!(
        effective["resource_accounting"]["fire_commander"]["ground_crews"]["available"],
        2
    );
    let (_, evidence) = call_key(&app, "GET", "/api/evaluator", None, Some("test-eval-one")).await;
    assert_eq!(
        evidence["chains"][0]["proposal"]["decision"]["resource_limits"]["total"]["ground_crews"],
        2
    );
    assert!(evidence["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "resource_limit_draft_reviewed"));
    assert!(evidence["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "resource_limits_changed" && l["data"]["proposal"] == pid));
    let (_, p) = call(
        &app,
        "POST",
        "/api/master/prompt",
        Some(json!({"text":"Recommend another limit"})),
    )
    .await;
    let pid = p["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][1]["status"] == "pending"
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":false})),
    )
    .await;
    let (_, after) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(after["resource_limits"], effective["resource_limits"]);
}

#[tokio::test]
async fn configured_limit_controls_enforce_capacity_and_reject_unknown_resources() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let body = json!({"limits":{"total":{"ground_crews":1},"phases":{"early_warning":{"ground_crews":0}}},"reason":"Initial stand-by phase"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/resource-limits",
            Some(body.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, fc) = call_key(
        &app,
        "GET",
        "/api/player/fire_commander",
        None,
        Some("test-fire"),
    )
    .await;
    assert_eq!(fc["resource_accounting"]["ground_crews"]["available"], 0);
    assert_eq!(
        fc["resource_accounting"]["ground_crews"]["inventory_available"],
        3
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/resource-limits",
            Some(json!({"limits":{"total":{"invented":2}},"reason":"invalid"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, state) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(state["resource_limits"], body["limits"]);
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/master/resource-limits",
            Some(body),
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn evaluator_assessment_cites_snapshot_is_cached_and_never_mutates_exercise() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    let model = llm::Llm::mock(move |task, system, user| {
        if task == "parse_reply" {
            return Ok(r#"{"detected_language":"en","translation_en":"Warn F4","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[],"clarification":null,"summary":"Warn F4"}"#.into());
        }
        if task == "translate" {
            return Ok(user.into());
        }
        assert_eq!(task, "evaluator_assessment");
        assert!(system.contains("external documents have NOT been fetched"));
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let pack: Value = serde_json::from_str(user)?;
        assert_eq!(
            pack["evidence"]["reference:0"]["basis"]["reference"],
            "Plan section 3"
        );
        let chain = pack["evidence"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(id, _)| id.starts_with("proposal:"))
            .unwrap()
            .1;
        assert!(chain["reply_delay_wall_ms"].is_number());
        assert!(chain["master_queue_wall_ms"].is_null());
        assert_eq!(
            chain["proposal"]["evidence"]["basis"]["reference"],
            "Plan section 3"
        );
        let id = pack["evidence"]
            .as_object()
            .unwrap()
            .keys()
            .find(|id| id.starts_with("message:"))
            .unwrap();
        Ok(json!({"findings":[{"category":"coordination","assessment":"A message was delivered","evidence":[id],"uncertainty":"Delivery does not establish acknowledgement or plan compliance","debrief_question":"How was receipt confirmed?"}]}).to_string())
    });
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.set_reference_materials(serde_json::from_value(json!([{"basis":{"kind":"plan","reference":"Plan section 3"},"attachments":[{"title":"Plan","url":"https://example.test/plan.pdf"}]}])).unwrap()).unwrap();
    ex.start().unwrap();
    let (app, _) = server::router_with_access(Arc::new(Mutex::new(ex)), model, None, access());
    let parent = reply_parent(&app, "mayor").await;
    call(&app,"POST","/api/player/mayor/reply",Some(json!({"text":"Warn F4","reply_to":parent,"evidence":{"basis":{"kind":"plan","reference":"Plan section 3"}}}))).await;
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    let (_, before) = call(&app, "GET", "/api/debrief", None).await;
    let (status, report) = call_key(
        &app,
        "GET",
        "/api/evaluator/assessment",
        None,
        Some("test-eval-one"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["error"], Value::Null);
    let citation = report["derived_assessment"]["findings"][0]["evidence"][0]
        .as_str()
        .unwrap();
    assert!(report["snapshot"]["evidence"].get(citation).is_some());
    let (_, second) = call_key(
        &app,
        "GET",
        "/api/evaluator/assessment",
        None,
        Some("test-eval-two"),
    )
    .await;
    assert_eq!(second, report);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (_, after) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(before, after);
    assert_eq!(
        call_key(
            &app,
            "GET",
            "/api/evaluator/assessment",
            None,
            Some("test-mayor")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/evaluator/assessment",
            Some(json!({})),
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn invalid_ai_assessment_keeps_recorded_evidence_and_no_operational_effects() {
    let model =
        llm::Llm::mock(|_, _, _| Ok(r#"{"findings":[],"commands":[{"type":"Ignite"}]}"#.into()));
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let (app, _) = server::router_with_access(Arc::new(Mutex::new(ex)), model, None, access());
    let (_, before) = call(&app, "GET", "/api/debrief", None).await;
    let (_, report) = call(&app, "GET", "/api/evaluator/assessment", None).await;
    assert_eq!(report["derived_assessment"], Value::Null);
    assert!(report["error"].as_str().unwrap().contains("validation"));
    assert!(!report["snapshot"]["evidence"]
        .as_object()
        .unwrap()
        .is_empty());
    assert_eq!(call(&app, "GET", "/api/debrief", None).await.1, before);
}

#[tokio::test]
async fn decision_paths_observe_actual_outcomes_and_preserve_containment_on_adaptation() {
    let app = app().await;
    let design = json!({"objectives":["Contain fire"],"phases":{"early_warning":"Mobilise","early_action":"Contained","escalation":"Escalated"},"decision_points":[{"id":"fire","phase":"early_warning","branches":[{"path":"contained","phase":"early_action","conditions":[{"type":"burning_at_most","cells":0},{"type":"executed_action","kind":"IssueWarning"}]},{"path":"escalation","phase":"escalation","conditions":[{"type":"burning_at_least","cells":1},{"type":"executed_action","kind":"IssueWarning"}]}]}]});
    assert_eq!(
        call(&app, "POST", "/api/master/design", Some(design.clone()))
            .await
            .0,
        StatusCode::OK
    );
    call(&app, "POST", "/api/master/start", None).await;
    assert_eq!(
        call(&app, "POST", "/api/master/design", Some(design.clone()))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/path",
            Some(json!({"point":"fire","path":"contained","reason":"Premature"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let parent = reply_parent(&app, "mayor").await;
    let (_, declared) = call(
        &app,
        "POST",
        "/api/player/mayor/reply",
        Some(json!({"text":"Warn F4","reply_to":parent})),
    )
    .await;
    let pid = declared["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/path",
            Some(json!({"point":"fire","path":"escalation","reason":"Approved but not executed"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/execute"),
        None,
    )
    .await;
    let (_, state) = call(&app, "GET", "/api/state", None).await;
    assert!(state["world"]["stats"]["burning_cells"].as_u64().unwrap() > 0);
    call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"Extinguish","area":{"at":{"x":2000,"y":2000},"radius_m":6000}})),
    )
    .await;
    assert_eq!(call(&app,"POST","/api/master/path",Some(json!({"point":"fire","path":"contained","reason":"Observed all cells extinguished"}))).await.0,StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/path",
            Some(json!({"point":"fire","path":"escalation","reason":"Force escalation"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(call(&app,"POST","/api/master/path",Some(json!({"point":"fire","path":"escalation","reason":"Exercise a plausible second incident while retaining original containment","adaptation":true}))).await.0,StatusCode::OK);
    let (_, e) = call_key(&app, "GET", "/api/evaluator", None, Some("test-eval-one")).await;
    assert_eq!(e["context"]["stats"]["burning_cells"], 0);
    assert_eq!(e["training_design"]["original"], design);
    assert_eq!(e["training_design"]["current_path"], "escalation");
    assert_eq!(e["training_design"]["history"][1]["deviation"], true);
    assert_eq!(e["training_design"]["history"][1]["approved_by"], "master");
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/master/path",
            Some(json!({})),
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn physical_preview_is_read_only_apply_rechecks_generation_and_correction_is_traceable() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let cmds = json!([{"type":"Extinguish","area":{"at":{"x":2000,"y":2000},"radius_m":6000}}]);
    let (_, before) = call(&app, "GET", "/api/debrief", None).await;
    let (_, preview) = call(
        &app,
        "POST",
        "/api/master/preview",
        Some(json!({"commands":cmds})),
    )
    .await;
    assert_eq!(preview["after"]["burning_cells"], 0);
    assert!(
        preview["results"][0]["geometry"]["nominal_area_ha"]
            .as_f64()
            .unwrap()
            > 10000.0
    );
    assert_eq!(call(&app, "GET", "/api/debrief", None).await.1, before);
    let (_,applied)=call(&app,"POST","/api/master/preview/apply",Some(json!({"commands":cmds,"generation":preview["generation"],"reason":"Reviewed whole-map extinction for exercise demonstration"}))).await;
    assert_eq!(applied["results"][0]["ok"], true);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/preview/apply",
            Some(json!({"commands":cmds,"generation":preview["generation"],"reason":"Duplicate"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/correct-intervention",
            Some(json!({"reason":"Whole area was unintended"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, corrected) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(corrected["stats"], before["stats"]);
    assert!(corrected["log"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "physical_intervention_corrected"));
    // A subsequent recorded change protects intervening state from checkpoint restore.
    call(&app, "POST", "/api/master/command", Some(cmds[0].clone())).await;
    call(
        &app,
        "POST",
        "/api/master/phase",
        Some(json!({"phase":"early_action","reason":"Subsequent progress"})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/correct-intervention",
            Some(json!({"reason":"Too late"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/master/preview",
            Some(json!({"commands":cmds})),
            Some("test-eval-one")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn only_executed_approved_deployments_move_and_player_sees_only_own_tasks() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    call(&app,"POST","/api/master/command",Some(json!({"type":"SetDeploymentTravel","role":"fire_commander","resource":"ground_crews","at":{"x":1000,"y":1000},"speed_mps":10}))).await;
    let parent = reply_parent(&app, "fire_commander").await;
    let (_, declared) = call(
        &app,
        "POST",
        "/api/player/fire_commander/reply",
        Some(json!({"text":"Cut a line","reply_to":parent})),
    )
    .await;
    let pid = declared["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| {
        m["proposals"][0]["status"] == "pending"
    })
    .await;
    assert_eq!(
        call(&app, "GET", "/api/state", None).await.1["world"]["deployments"],
        json!([])
    );
    call(&app,"POST",&format!("/api/master/proposals/{pid}/decide"),Some(json!({"approve":true,"commands":[{"type":"CutFireLine","from":{"x":2000,"y":1000},"to":{"x":2500,"y":1000}}]}))).await;
    assert_eq!(
        call(&app, "GET", "/api/state", None).await.1["world"]["deployments"],
        json!([]),
        "approval does not dispatch"
    );
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/execute"),
        None,
    )
    .await;
    let (_, player) = call_key(
        &app,
        "GET",
        "/api/player/fire_commander",
        None,
        Some("test-fire"),
    )
    .await;
    assert_eq!(player["deployments"][0]["task"]["status"], "en_route");
    assert_eq!(
        player["deployments"][0]["position_m"],
        json!([1000.0, 1000.0])
    );
    assert_eq!(player["resource_accounting"]["ground_crews"]["deployed"], 1);
    assert_eq!(
        call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor"))
            .await
            .1["deployments"],
        json!([])
    );
    call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"AdvanceTime","minutes":1})),
    )
    .await;
    let (_, moved) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert_eq!(
        moved["deployments"][0]["position_m"],
        json!([1600.0, 1000.0])
    );
    assert_eq!(moved["deployments"][0]["task"]["status"], "en_route");
    call(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"AdvanceTime","minutes":1})),
    )
    .await;
    let (_, arrived) = call_key(
        &app,
        "GET",
        "/api/evaluator?role=fire_commander",
        None,
        Some("test-eval-one"),
    )
    .await;
    assert_eq!(arrived["deployments"][0]["task"]["arrived_s"], 100);
    assert_eq!(arrived["deployments"][0]["task"]["status"], "completed");
    assert_eq!(arrived["deployments"][0]["active_qty"], 1);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/master/proposals/{pid}/execute"),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    call(
        &app,
        "POST",
        "/api/master/rollback",
        Some(json!({"turn":1})),
    )
    .await;
    assert_eq!(
        call(&app, "GET", "/api/state", None).await.1["world"]["deployments"],
        json!([]),
        "rollback removes actual deployments but retains audit"
    );
}

#[tokio::test]
async fn chat_is_direct_private_and_orders_wait_for_excon() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    for role in ["mayor", "fire_commander"] {
        call(
            &app,
            "POST",
            &format!("/api/player/{role}/language"),
            Some(json!({"language":"en"})),
        )
        .await;
    }
    let (_, before) = call(&app, "GET", "/api/state", None).await;
    // Role-bound keys cannot impersonate another player.
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/player/fire_commander/chat",
            Some(json!({"to":"master","text":"spoof"})),
            Some("test-mayor")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, prepared) = call_key(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"fire_commander","text":"Warn F4"})),
        Some("test-mayor"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, private) = call(&app, "GET", "/api/master", None).await;
    assert!(private["messages"].as_array().unwrap().is_empty());
    assert!(private["proposals"].as_array().unwrap().is_empty());
    let (status, delivered) = call_key(
        &app,
        "POST",
        "/api/player/mayor/draft/submit",
        Some(json!({"draft":prepared["draft"]["id"],"revision":prepared["draft"]["revision"]})),
        Some("test-mayor"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pid = delivered["proposal"].as_u64().unwrap();
    let master = eventually(&app, "/api/master", |v| {
        v["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == pid && p["status"] == "pending")
    })
    .await;
    let proposal = master["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == pid)
        .unwrap();
    assert!(
        proposal["communication_drafts"]
            .as_array()
            .unwrap()
            .is_empty(),
        "direct chat must not be delivered again as an inferred message"
    );
    for role in ["mayor", "fire_commander"] {
        let (_, player) = call(&app, "GET", &format!("/api/player/{role}"), None).await;
        assert!(player["inbox"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == delivered["message"] && m["original"] == "Warn F4"));
    }
    let (_, other) = call(&app, "GET", "/api/player/regional_cp", None).await;
    assert!(!other["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == delivered["message"]));
    let (_, after) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(
        before["world"], after["world"],
        "chat and AI interpretation cannot change physics"
    );
    call(
        &app,
        "POST",
        &format!("/api/master/proposals/{pid}/decide"),
        Some(json!({"approve":true})),
    )
    .await;
    let (_, approved) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(
        before["world"], approved["world"],
        "approval queues the order"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/resolve",
            Some(json!({"minutes":1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, resolved) = call(&app, "GET", "/api/master", None).await;
    assert!(resolved["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == pid && p["status"] == "applied"));
    call(
        &app,
        "POST",
        "/api/master/rollback",
        Some(json!({"turn":1})),
    )
    .await;
    let (_, rolled) = call(&app, "GET", "/api/master", None).await;
    assert!(rolled["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == delivered["message"]));
    assert!(rolled["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == pid && p["status"] == "rolled_back"));
}

#[tokio::test]
async fn chat_translation_and_no_ai_fallback_preserve_the_original() {
    let app = app().await;
    call(
        &app,
        "POST",
        "/api/player/mayor/language",
        Some(json!({"language":"it"})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/player/fire_commander/language",
        Some(json!({"language":"fr"})),
    )
    .await;
    let (status, sent) = call(
        &app,
        "POST",
        "/api/player/mayor/chat",
        Some(json!({"to":"fire_commander","text":"Ricevuto"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, recipient) = call(&app, "GET", "/api/player/fire_commander", None).await;
    let message = recipient["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == sent["message"])
        .unwrap();
    assert_eq!(message["original"], "Ricevuto");
    assert_eq!(message["language_src"], "it");
    assert_eq!(message["text_en"], "[it] Ricevuto");
    assert_eq!(message["text"], "[it] [it] Ricevuto");
    assert!(sent["proposal"].is_null());
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.set_language("mayor", "it").unwrap();
    let app =
        server::router_with_access(Arc::new(Mutex::new(ex)), llm::Llm::off(), None, access()).0;
    let (status, sent) = call(
        &app,
        "POST",
        "/api/player/mayor/chat",
        Some(json!({"to":"master","text":"Ricevuto"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent["translation_failed"], true);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["messages"][0]["original"], "Ricevuto");
    assert_eq!(master["messages"][0]["text"], "Ricevuto");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/master/chat",
            Some(json!({"to":"unknown","text":"Hi"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/chat",
            Some(json!({"to":"master","text":"order","interpret":true}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn informational_chat_does_not_enter_the_approval_queue() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", "en").unwrap();
    let llm = llm::Llm::mock(|_, _, _| {
        Ok(
            r#"{"message":"Understood","actions":[],"messages":[],"summary":"Acknowledged"}"#
                .into(),
        )
    });
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access()).0;
    let (_, prepared) = call(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"master","text":"Understood"})),
    )
    .await;
    let (status, sent) = call(
        &app,
        "POST",
        "/api/player/mayor/draft/submit",
        Some(json!({"draft":prepared["draft"]["id"],"revision":prepared["draft"]["revision"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(sent["proposal"].is_null());
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["proposals"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_chat_cannot_cross_a_rollback() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let started = Arc::new(AtomicBool::new(false));
    let signal = started.clone();
    let llm = llm::Llm::mock(move |_, _, text| {
        signal.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        Ok(text.into())
    });
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", "it").unwrap();
    let ex = Arc::new(Mutex::new(ex));
    let app = server::router_with_access(ex.clone(), llm, None, access()).0;
    let worker = app.clone();
    let request = tokio::spawn(async move {
        call(
            &worker,
            "POST",
            "/api/player/mayor/chat",
            Some(json!({"to":"master", "text":"Warn F4", "interpret":false})),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    ex.lock().unwrap().rollback(1).unwrap();
    assert_eq!(request.await.unwrap().0, StatusCode::BAD_REQUEST);
    let ex = ex.lock().unwrap();
    assert!(ex.proposals.is_empty());
    assert!(!ex
        .messages
        .iter()
        .any(|m| m.kind == exercise::MsgKind::Chat));
}

#[tokio::test]
async fn player_clarifies_privately_then_confirms_the_exact_revision() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    call(
        &app,
        "POST",
        "/api/player/mayor/language",
        Some(json!({"language":"en"})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/chat",
            Some(json!({"to":"master","text":"where?","interpret":true}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, first) = call_key(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"master","text":"where?"})),
        Some("test-mayor"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["draft"]["status"], "needs_clarification");
    assert_eq!(
        first["draft"]["question"],
        "Which zone should receive the warning?"
    );
    let first = first["draft"].clone();
    for endpoint in ["/api/master", "/api/debrief"] {
        let (_, public) = call(&app, "GET", endpoint, None).await;
        assert!(
            !public.to_string().contains("where?"),
            "private drafting must not reach EXCON or the committed log"
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/draft/submit",
            Some(json!({"draft":first["id"],"revision":first["revision"]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call_key(
            &app,
            "POST",
            "/api/player/mayor/draft/submit",
            Some(json!({"draft":first["id"],"revision":first["revision"]})),
            Some("test-fire")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, second)=call(&app,"POST","/api/player/mayor/draft",Some(json!({"to":"master","text":"F4 confirmed","draft":first["id"],"revision":first["revision"]}))).await;
    assert_eq!(status, StatusCode::OK);
    let second = second["draft"].clone();
    assert_eq!(second["status"], "ready");
    assert_eq!(second["message"], "Warn F4");
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["messages"].as_array().unwrap().is_empty());
    assert!(master["proposals"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/draft/submit",
            Some(json!({"draft":first["id"],"revision":first["revision"]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let submission = json!({"draft":second["id"],"revision":second["revision"]});
    let (status, sent) = call(
        &app,
        "POST",
        "/api/player/mayor/draft/submit",
        Some(submission.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(sent["proposal"].is_number());
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["original"], "Warn F4");
    assert_eq!(master["proposals"][0]["status"], "pending");
    assert_eq!(master["messages"][0]["original"], "Warn F4");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/draft/submit",
            Some(submission)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn unavailable_private_ai_requires_explicit_manual_submission_and_rollback_expires_drafts() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", "en").unwrap();
    let app =
        server::router_with_access(Arc::new(Mutex::new(ex)), llm::Llm::off(), None, access()).0;
    let (_, prepared) = call(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"master","text":"Warn F4"})),
    )
    .await;
    assert_eq!(prepared["draft"]["status"], "ai_unavailable");
    let submission =
        json!({"draft":prepared["draft"]["id"],"revision":prepared["draft"]["revision"]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/player/mayor/draft/submit",
            Some(submission.clone())
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["messages"].as_array().unwrap().is_empty());
    let mut manual = submission.clone();
    manual["manual"] = json!(true);
    assert_eq!(
        call(&app, "POST", "/api/player/mayor/draft/submit", Some(manual))
            .await
            .0,
        StatusCode::OK
    );
    let (_, again) = call(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"master","text":"Warn F4"})),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/master/rollback",
        Some(json!({"turn":1})),
    )
    .await;
    assert_eq!(call(&app,"POST","/api/player/mayor/draft/submit",Some(json!({"draft":again["draft"]["id"],"revision":again["draft"]["revision"],"manual":true}))).await.0,StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_interpretation_finishing_after_rollback_is_discarded() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let started = Arc::new(AtomicBool::new(false));
    let signal = started.clone();
    let llm = llm::Llm::mock(move |_, _, _| {
        signal.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        Ok(r#"{"message":"Warn F4","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[]}"#.into())
    });
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", "en").unwrap();
    let ex = Arc::new(Mutex::new(ex));
    let app = server::router_with_access(ex.clone(), llm, None, access()).0;
    let worker = app.clone();
    let request = tokio::spawn(async move {
        call(
            &worker,
            "POST",
            "/api/player/mayor/draft",
            Some(json!({"to":"master","text":"Warn F4"})),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    ex.lock().unwrap().rollback(1).unwrap();
    assert_eq!(request.await.unwrap().0, StatusCode::BAD_REQUEST);
    let ex = ex.lock().unwrap();
    assert!(ex.player_drafts("mayor").is_empty());
    assert!(ex.proposals.is_empty());
    assert!(!ex
        .messages
        .iter()
        .any(|message| message.kind == exercise::MsgKind::Chat));
}

#[tokio::test]
async fn a_missing_working_translation_is_not_presented_as_ready() {
    let scn = Arc::new(scenario::synthetic::generate(
        scenario::synthetic::DEFAULT_SEED,
    ));
    let mut ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    ex.start().unwrap();
    ex.set_language("mayor", "it").unwrap();
    let llm = llm::Llm::mock(|_, _, _| {
        Ok(r#"{"message":"Allerta F4","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[]}"#.into())
    });
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access()).0;
    let (_, prepared) = call(
        &app,
        "POST",
        "/api/player/mayor/draft",
        Some(json!({"to":"master","text":"Allerta F4"})),
    )
    .await;
    assert_eq!(prepared["draft"]["status"], "ai_unavailable");
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["messages"].as_array().unwrap().is_empty());
    assert!(master["proposals"].as_array().unwrap().is_empty());
}
