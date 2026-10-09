//! The full Master ↔ player workflow over HTTP, with a mock LLM.
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
            "parse_reply" if user.contains("where?") => r#"{"detected_language":"it","translation_en":"where?","actions":[],"messages":[],"clarification":"Quale zona?","summary":"unclear"}"#.into(),
            "parse_reply" => r#"{"detected_language":"it","translation_en":"Warn F4 and tell the fire commander","actions":[{"type":"IssueWarning","zone":"F4"}],"messages":[{"to":"fire_commander","text":"We warned F4"}],"clarification":null,"summary":"Warn F4"}"#.into(),
            "master_prompt" => r#"{"actions":[{"type":"SetWeather","wind_speed_kmh":60}],"injects":[{"recipients":["all"],"title":"Wind","sender":"Weather service","text":"Wind rising to 60 km/h"}],"clarification":null,"summary":"stronger wind"}"#.into(),
            "feedback" => format!("[feedback] {user}"),
            "situation_update" => "[situation] all quiet".into(),
            other => anyhow::bail!("unexpected task {other}"),
        })
    })
}

async fn app() -> Router {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    server::router(Arc::new(Mutex::new(ex)), mock_llm(), None).0
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or(Body::empty())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn reply_parent(app: &Router, role: &str) -> u64 {
    let (_, p) = call(app, "GET", &format!("/api/player/{role}"), None).await;
    if let Some(m) = p["inbox"].as_array().unwrap().iter().rev().find(|m| m["kind"] == "inject") {
        return m["id"].as_u64().unwrap();
    }
    if p["phase"] != "orders" { return 0; }
    let (_, sent) = call(app, "POST", "/api/master/inject", Some(json!({"title":"Task", "recipients":[role], "text":"Exercise task"}))).await;
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
    let (s, _) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "hi", "reply_to": reply_parent(&app, "mayor").await}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    assert_eq!(call(&app, "POST", "/api/master/start", None).await.0, StatusCode::OK);

    // MSEL inject, translated for the Italian-speaking mayor only.
    let (s, _) = call(&app, "POST", "/api/master/msel/I-01/send", None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, mayor) = call(&app, "GET", "/api/player/mayor", None).await;
    assert!(mayor["inbox"][0]["text"].as_str().unwrap().starts_with("[it] "));
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!fc["inbox"][0]["text"].as_str().unwrap().starts_with("[it]"));

    // Player reply -> parsed proposal + message delivered to the fire commander.
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "Allertate F4 e avvisate i pompieri", "reply_to": reply_parent(&app, "mayor").await}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    let m = eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["id"] == pid && p["status"] == "pending")).await;
    let p = m["proposals"].as_array().unwrap().iter().find(|p| p["id"] == pid).unwrap().clone();
    assert_eq!(p["commands"][0]["type"], "IssueWarning");
    assert_eq!(p["verdicts"][0]["verdict"], "ok");
    let (_, before) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!before["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message"), "operational AI drafts await Master approval and execution");

    // Master approves -> translated feedback to the player.
    let (s, out) = call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve": true}))).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    eventually(&app, "/api/player/mayor", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "feedback" && m["text"].as_str().unwrap().starts_with("[it]"))).await;

    // Unclear reply -> clarification question back to the player.
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "evacuate where?", "reply_to": reply_parent(&app, "mayor").await}))).await;
    let pid2 = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |v| v["proposals"].as_array().unwrap().iter().any(|p| p["id"] == pid2 && p["status"] == "needs_clarification")).await;
    let (s, _) = call(&app, "POST", &format!("/api/master/proposals/{pid2}/decide"), Some(json!({"approve": true, "note": "Send the clarification"}))).await;
    assert_eq!(s, StatusCode::OK);
    eventually(&app, "/api/player/mayor", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "clarification")).await;

    // Master prompt -> proposal with a command and an inject draft; approval applies both.
    let (_, r) = call(&app, "POST", "/api/master/prompt", Some(json!({"text": "make the wind much stronger and tell everyone"}))).await;
    let mp = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["id"] == mp && p["status"] == "pending")).await;
    call(&app, "POST", &format!("/api/master/proposals/{mp}/decide"), Some(json!({"approve": true}))).await;
    let st = eventually(&app, "/api/state", |v| v["world"]["weather"]["wind_speed_kmh"] == 60.0).await;
    assert_eq!(st["turn"], 1);
    eventually(&app, "/api/player/regional_cp", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["title"] == "Wind")).await;

    // Direct commands "by code": a list, one of which fails.
    let (s, r) = call(&app, "POST", "/api/master/command", Some(json!([{"type":"Extinguish","area":"G3"},{"type":"WaterDrop","at":"Atlantis"}]))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["results"][0]["ok"], true);
    assert_eq!(r["results"][1]["ok"], false);
    let (s, _) = call(&app, "POST", "/api/master/command", Some(json!({"type":"NotACommand"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Resolve the turn -> applied actions, time advanced, situation updates to every player.
    let (s, rep) = call(&app, "POST", "/api/master/resolve", Some(json!({"minutes": 30}))).await;
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(rep["to_s"], 1800);
    assert!(rep["applied"].as_array().unwrap().iter().any(|a| a["action"] == "Issue warning to zone F4" && a["ok"] == true));
    eventually(&app, "/api/player/fire_commander", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message")).await;
    for role in ["mayor", "fire_commander", "regional_cp"] {
        eventually(&app, &format!("/api/player/{role}"), |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "situation")).await;
    }
    let (_, st) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(st["turn"], 2);
    assert_eq!(st["world"]["stats"]["clock"], "Day 1 13:30");

    // Rollback and debrief.
    assert_eq!(call(&app, "POST", "/api/master/rollback", Some(json!({"turn": 1}))).await.0, StatusCode::OK);
    let (_, st) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(st["turn"], 1);
    assert_eq!(call(&app, "POST", "/api/master/end", None).await.0, StatusCode::OK);
    let (_, d) = call(&app, "GET", "/api/debrief", None).await;
    assert!(d["log"].as_array().unwrap().len() > 10);
}

#[tokio::test]
async fn works_without_llm() {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let app = server::router(Arc::new(Mutex::new(ex)), llm::Llm::off(), None).0;
    call(&app, "POST", "/api/master/start", None).await;
    call(&app, "POST", "/api/master/inject", Some(json!({"title": "t", "recipients": ["mayor"], "text": "english only"}))).await;
    let (_, m) = call(&app, "GET", "/api/player/mayor", None).await;
    assert_eq!(m["inbox"][0]["text"], "english only");
    let (_, r) = call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "line along G3 south edge", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["id"] == pid && p["status"] == "pending")).await;
    // The Master types the command for the player.
    let (s, out) = call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve": true, "commands": [{"type":"CutFireLine","along":"G3:S"}]}))).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    let (_, rep) = call(&app, "POST", "/api/master/resolve", None).await;
    assert_eq!(rep["applied"][0]["ok"], true);
    eventually(&app, "/api/player/fire_commander", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "situation")).await;
}

/// With a pace set, a resolving turn is visible from outside: phase
/// `resolving`, simulated time advancing minute by minute, replies refused.
#[tokio::test]
async fn resolution_runs_live_and_is_observable() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    // 60x: one simulated minute per second, a 20-minute turn in ~20 s... too slow for a test: 600x → ~2 s.
    let (_, set) = call(&app, "POST", "/api/master/settings", Some(json!({"speed": 600.0}))).await;
    assert_eq!(set["speed"], 600.0);
    let a2 = app.clone();
    let resolver = tokio::spawn(async move { call(&a2, "POST", "/api/master/resolve", Some(json!({"minutes": 20}))).await });

    let mut seen_times = std::collections::BTreeSet::new();
    for _ in 0..200 {
        let (_, st) = call(&app, "GET", "/api/state", None).await;
        if st["phase"] == "resolving" {
            assert_eq!(st["resolving"]["to_s"], 1200);
            seen_times.insert(st["world"]["stats"]["time_s"].as_i64().unwrap());
            let (s, _) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "hi", "reply_to": reply_parent(&app, "mayor").await}))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "replies are refused while resolving");
        }
        if resolver.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let (s, rep) = resolver.await.unwrap();
    assert_eq!(s, StatusCode::OK, "{rep}");
    assert_eq!(rep["to_s"], 1200);
    assert!(seen_times.len() >= 5, "intermediate times should be observable, saw {seen_times:?}");
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
            return Ok(if user.contains("over there") && !user.contains("east edge") {
                r#"{"actions":[],"clarification":"Which zone edge?","summary":"unclear"}"#.into()
            } else {
                r#"{"actions":[{"type":"CutFireLine","along":"F4:E"}],"summary":"line"}"#.into()
            });
        }
        Ok(format!("ok {task}"))
    });
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let app = server::router(Arc::new(Mutex::new(ex)), llm, None).0;
    call(&app, "POST", "/api/master/start", None).await;
    call(&app, "POST", "/api/master/msel/I-03/send", None).await;
    call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "send the crews over there", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["status"] == "needs_clarification")).await;
    call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "the east edge of F4", "reply_to": reply_parent(&app, "fire_commander").await}))).await;
    eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().filter(|p| p["status"] == "pending").count() == 1).await;
    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("INJECT") && calls[0].contains("Fire behaviour estimate"), "the inject is context: {}", calls[0]);
    assert!(calls[1].contains("send the crews over there"), "earlier reply in history");
    assert!(calls[1].contains("Which zone edge?"), "earlier clarification in history");
    assert!(calls[1].contains("the east edge of F4"));
}

#[tokio::test]
async fn reply_api_rejects_missing_unknown_and_other_roles_parent() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, sent) = call(&app, "POST", "/api/master/inject", Some(json!({"title":"Fire", "recipients":["mayor"], "text":"Reported fire"}))).await;
    let parent = sent["messages"][0].as_u64().unwrap();
    assert_eq!(call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"hello"}))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"hello", "reply_to":99999}))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text":"hello", "reply_to":parent}))).await.0, StatusCode::BAD_REQUEST);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert!(master["proposals"].as_array().unwrap().is_empty());
    let (status, reply) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"hello", "reply_to":parent, "declaration_kind":"request"}))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["reply_to"], parent);
    assert_eq!(master["proposals"][0]["declaration_kind"], "request");
    assert_eq!(master["proposals"][0]["history"][0]["status"], "parsing");
    assert_eq!(master["messages"][1]["proposal"], reply["proposal"]);
    assert_eq!(master["messages"][1]["inject_id"], master["messages"][0]["inject_id"]);
}

#[tokio::test]
async fn operational_drafts_require_master_review_and_deliver_edited_text_once() {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    let (app, state) = server::router(Arc::new(Mutex::new(ex)), mock_llm(), None);
    call(&app, "POST", "/api/master/start", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"warn F4", "reply_to":parent}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"][0]["status"] == "pending").await;
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!fc["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message"));
    let edited = json!([{"to":"fire_commander","from":"mayor","kind":"role_message","title":"Warning coordination","text_en":"The warning to F4 is now issued; please confirm receipt."}]);
    let (s, _) = call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve":true, "communication_drafts":edited}))).await;
    assert_eq!(s, StatusCode::OK);
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    assert!(!fc["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message"), "action-dependent message waits for actual execution");
    assert_eq!(call(&app, "POST", "/api/master/resolve", Some(json!({"minutes":1}))).await.0, StatusCode::OK);
    eventually(&app, "/api/player/fire_commander", |p| p["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message")).await;
    server::flow::send_drafts(&state, pid).await;
    server::flow::send_drafts(&state, pid).await;
    let (_, fc) = call(&app, "GET", "/api/player/fire_commander", None).await;
    let delivered = fc["inbox"].as_array().unwrap().iter().filter(|m| m["kind"] == "role_message").collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["text_en"], "The warning to F4 is now issued; please confirm receipt.");
    assert_eq!(delivered[0]["proposal"], pid);
    let (_, m) = call(&app, "GET", "/api/master", None).await;
    assert!(m["log"].as_array().unwrap().iter().any(|l| l["kind"] == "communications_reviewed"));
    assert!(m["log"].as_array().unwrap().iter().any(|l| l["kind"] == "approved_communication_delivered"));
}

#[tokio::test]
async fn rejected_ai_suggestions_never_deliver_or_change_world() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let (_, before) = call(&app, "GET", "/api/state", None).await;
    let (_, r) = call(&app, "POST", "/api/master/prompt", Some(json!({"text":"stronger wind and tell everyone"}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"][0]["status"] == "pending").await;
    let (_, proposed) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(proposed["world"], before["world"]);
    call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve":false}))).await;
    call(&app, "POST", "/api/master/resolve", Some(json!({"minutes":1}))).await;
    let (_, state) = call(&app, "GET", "/api/state", None).await;
    assert_eq!(state["world"]["weather"], before["world"]["weather"]);
    for role in ["mayor", "fire_commander", "regional_cp"] {
        let (_, p) = call(&app, "GET", &format!("/api/player/{role}"), None).await;
        assert!(!p["inbox"].as_array().unwrap().iter().any(|m| m["title"] == "Wind"));
    }
}
