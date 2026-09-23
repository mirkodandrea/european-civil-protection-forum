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
    let (s, _) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "hi"}))).await;
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
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "Allertate F4 e avvisate i pompieri"}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    let m = eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["id"] == pid && p["status"] == "pending")).await;
    let p = m["proposals"].as_array().unwrap().iter().find(|p| p["id"] == pid).unwrap().clone();
    assert_eq!(p["commands"][0]["type"], "IssueWarning");
    assert_eq!(p["verdicts"][0]["verdict"], "ok");
    eventually(&app, "/api/player/fire_commander", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "role_message")).await;

    // Master approves -> translated feedback to the player.
    let (s, out) = call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve": true}))).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    eventually(&app, "/api/player/mayor", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "feedback" && m["text"].as_str().unwrap().starts_with("[feedback]"))).await;

    // Unclear reply -> clarification question back to the player.
    let (_, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text": "evacuate where?"}))).await;
    let pid2 = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/player/mayor", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "clarification")).await;
    let (s, _) = call(&app, "POST", &format!("/api/master/proposals/{pid2}/decide"), Some(json!({"approve": false, "note": "be specific"}))).await;
    assert_eq!(s, StatusCode::OK);

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
    let (_, r) = call(&app, "POST", "/api/player/fire_commander/reply", Some(json!({"text": "line along G3 south edge"}))).await;
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"].as_array().unwrap().iter().any(|p| p["id"] == pid && p["status"] == "pending")).await;
    // The Master types the command for the player.
    let (s, out) = call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve": true, "commands": [{"type":"CutFireLine","along":"G3:S"}]}))).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    let (_, rep) = call(&app, "POST", "/api/master/resolve", None).await;
    assert_eq!(rep["applied"][0]["ok"], true);
    eventually(&app, "/api/player/fire_commander", |v| v["inbox"].as_array().unwrap().iter().any(|m| m["kind"] == "situation")).await;
}
