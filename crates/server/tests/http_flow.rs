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

fn access() -> server::access::Access {
    server::access::Access { master:"test-master".into(), players: std::collections::BTreeMap::from([("mayor".into(), "test-mayor".into()), ("fire_commander".into(), "test-fire".into())]), evaluators: std::collections::BTreeMap::from([("eval-one".into(), "test-eval-one".into()), ("eval-two".into(), "test-eval-two".into())]) }
}

async fn app() -> Router {
    let scn = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex = exercise::Exercise::new(sim::World::new(scn, 1).unwrap());
    server::router_with_access(Arc::new(Mutex::new(ex)), mock_llm(), None, access()).0
}

async fn call(app: &Router, method: &str, uri: &str, mut body: Option<Value>) -> (StatusCode, Value) {
    if method == "POST" && uri.ends_with("/reply") {
        if let Some(body) = body.as_mut() {
            if body.get("evidence").is_none() { body["evidence"] = json!({"basis":{"kind":"operational_judgment", "reference":"Operational judgment based on the received inject"}}); }
        }
    }
    let req = Request::builder().method(method).uri(uri).header("content-type", "application/json").header("authorization", "Bearer test-master");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or(Body::empty())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn reply_parent(app: &Router, role: &str) -> u64 {
    let (_, p) = call(app, "GET", &format!("/api/player/{role}"), None).await;
    call(app, "POST", &format!("/api/player/{role}/language"), Some(json!({"language":p["player"]["language"]}))).await;
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
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm::Llm::off(), None, access()).0;
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
    let app = server::router_with_access(Arc::new(Mutex::new(ex)), llm, None, access()).0;
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
    call(&app, "POST", "/api/player/mayor/language", Some(json!({"language":"it"}))).await;
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
    let (app, state) = server::router_with_access(Arc::new(Mutex::new(ex)), mock_llm(), None, access());
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

#[tokio::test]
async fn api_requires_basis_and_exposes_supporting_evidence_through_lifecycle() {
    let app = app().await;
    call(&app, "POST", "/api/master/start", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let req = Request::builder().method("POST").uri("/api/player/mayor/reply").header("content-type", "application/json").header("authorization", "Bearer test-master")
        .body(Body::from(json!({"text":"warn F4","reply_to":parent}).to_string())).unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    let evidence = json!({"basis":{"kind":"plan","reference":"Municipal warning plan, annex 3"},"attachments":[{"title":"Area map","url":"https://example.test/map.pdf"}]});
    let invalid = json!({"basis":{"kind":"plan","reference":""}});
    assert_eq!(call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"warn F4","reply_to":parent,"evidence":invalid}))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, "POST", "/api/player/mayor/language", Some(json!({"language":"unsupported"}))).await.0, StatusCode::BAD_REQUEST);
    let (s, r) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"warn F4","reply_to":parent,"evidence":evidence}))).await;
    assert_eq!(s, StatusCode::OK);
    let pid = r["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"][0]["status"] == "pending").await;
    call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve":true}))).await;
    call(&app, "POST", "/api/master/resolve", Some(json!({"minutes":1}))).await;
    let (_, p) = call(&app, "GET", "/api/player/mayor", None).await;
    assert_eq!(p["proposals"][0]["evidence"], evidence);
    let (_, master) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(master["proposals"][0]["evidence"], evidence);
    call(&app, "POST", "/api/master/rollback", Some(json!({"turn":1}))).await;
    let (_, d) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(d["proposals"][0]["evidence"], evidence);
}

async fn call_key(app: &Router, method: &str, uri: &str, body: Option<Value>, key: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(key) = key { req = req.header("authorization", format!("Bearer {key}")); }
    let resp = app.clone().oneshot(req.body(body.map(|v| Body::from(v.to_string())).unwrap_or(Body::empty())).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn role_keys_protect_all_authoritative_reads_and_actions() {
    let app = app().await;
    let (_, mut before) = call(&app, "GET", "/api/state", None).await;
    before.as_object_mut().unwrap().remove("wall_ms");
    for path in ["/api/state", "/api/master", "/api/debrief", "/api/catalogue", "/api/master/access", "/api/player/fire_commander"] {
        assert_eq!(call_key(&app, "GET", path, None, Some("test-mayor")).await.0, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(call_key(&app, "GET", path, None, None).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    for path in ["/api/master/start", "/api/master/end", "/api/master/command", "/api/master/inject", "/api/master/msel/I-01/send", "/api/master/proposals/1/decide", "/api/master/prompt", "/api/master/resolve", "/api/master/rollback", "/api/master/settings", "/api/master/resource-limits", "/api/master/clock", "/api/master/phase", "/api/master/schedules", "/api/master/schedules/1/cancel", "/api/master/proposals/1/execute", "/api/master/knowledge", "/api/player/fire_commander/reply", "/api/player/fire_commander/language"] {
        assert_eq!(call_key(&app, "POST", path, Some(json!({})), Some("test-mayor")).await.0, StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(call_key(&app, "GET", "/api/state?token=test-master", None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call_key(&app, "GET", "/ws", None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call_key(&app, "GET", "/api/health", None, None).await, (StatusCode::OK, json!({"ok":true})));
    let (_, mut after) = call(&app, "GET", "/api/state", None).await;
    after.as_object_mut().unwrap().remove("wall_ms");
    assert_eq!(before, after);
    let (_, own) = call_key(&app, "GET", "/api/static", None, Some("test-mayor")).await;
    assert!(own.get("msel").is_none() && own.get("reinforcements").is_none() && own.get("description").is_none());
    assert_eq!(own["roles"].as_array().unwrap().len(), 1);
    assert!(own["buildings"][0].get("occupants").is_none());
    assert_eq!(call_key(&app, "POST", "/api/player/mayor/language", Some(json!({"language":"it"})), Some("test-mayor")).await.0, StatusCode::OK);
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
    assert!(mayor.get("world").is_none() && mayor["map"].get("fire").is_none() && mayor["map"].get("stats").is_none());
    let (_, fire) = call_key(&app, "GET", "/api/player/fire_commander", None, Some("test-fire")).await;
    assert!(fire["map"]["cells"].as_array().unwrap().is_empty());
    assert!(fire["knowledge_events"].as_array().unwrap().is_empty());
    call(&app, "POST", "/api/master/command", Some(json!({"type":"Ignite","at":"G5","radius_m":60}))).await;
    call(&app, "POST", "/api/master/resolve", Some(json!({"minutes":1}))).await;
    let (_, unchanged) = call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor")).await;
    assert_eq!(unchanged["map"], mayor["map"]);
    let update = json!({"role":"mayor","source":"Field team observation","observation":{"cells":[{"row":3,"col":4,"state":2},{"row":3,"col":5,"state":1}]}});
    assert_eq!(call(&app, "POST", "/api/master/knowledge", Some(update)).await.0, StatusCode::OK);
    let (_, updated) = call_key(&app, "GET", "/api/player/mayor", None, Some("test-mayor")).await;
    assert_eq!(updated["map"]["cells"].as_array().unwrap().len(), 2);
    assert_eq!(updated["map"]["cells"][0]["state"], 2);
    let (_, before_invalid) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(call(&app, "POST", "/api/master/inject", Some(json!({"title":"Invalid","recipients":["mayor"],"text":"bad report","observation":{"cells":[{"row":999999,"col":0,"state":1}]}}))).await.0, StatusCode::BAD_REQUEST);
    let (_, after_invalid) = call(&app, "GET", "/api/master", None).await;
    assert_eq!(before_invalid["messages"], after_invalid["messages"]);
    assert_eq!(call(&app, "POST", "/api/master/knowledge", Some(json!({"role":"fire_commander","source":"Reviewed aerial perimeter","current_perimeter":true}))).await.0, StatusCode::OK);
    let (_, acquired) = call_key(&app, "GET", "/api/player/fire_commander", None, Some("test-fire")).await;
    assert!(!acquired["map"]["cells"].as_array().unwrap().is_empty());
    let (_, debrief) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(debrief["knowledge_events"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn multiple_evaluators_can_read_master_evidence_but_never_mutate() {
    let app = app().await;
    let (_, before) = call(&app, "GET", "/api/debrief", None).await;
    for key in ["test-eval-one", "test-eval-two"] {
        for path in ["/api/static", "/api/state", "/api/master", "/api/catalogue", "/api/debrief", "/api/evaluator"] {
            assert_eq!(call_key(&app, "GET", path, None, Some(key)).await.0, StatusCode::OK, "{key} {path}");
        }
        assert_eq!(call_key(&app, "GET", "/api/master/access", None, Some(key)).await.0, StatusCode::FORBIDDEN);
        for path in ["/api/master/start", "/api/master/end", "/api/master/command", "/api/master/inject", "/api/master/msel/I-01/send", "/api/master/proposals/1/decide", "/api/master/prompt", "/api/master/resolve", "/api/master/rollback", "/api/master/settings", "/api/master/resource-limits", "/api/master/clock", "/api/master/phase", "/api/master/schedules", "/api/master/schedules/1/cancel", "/api/master/proposals/1/execute", "/api/master/knowledge", "/api/master/references", "/api/player/mayor/reply", "/api/player/mayor/language", "/api/evaluator"] {
            assert_eq!(call_key(&app, "POST", path, Some(json!({})), Some(key)).await.0, StatusCode::FORBIDDEN, "{key} {path}");
        }
    }
    let (_, after) = call(&app, "GET", "/api/debrief", None).await;
    assert_eq!(before, after);
    let (s, identity) = call_key(&app, "GET", "/api/access", None, Some("test-eval-two")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(identity, json!({"kind":"evaluator","id":"eval-two"}));
    assert_eq!(call_key(&app, "GET", "/api/evaluator", None, Some("test-mayor")).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn evaluator_filters_trace_retained_chain_objectives_and_reference_materials() {
    let app = app().await;
    let references = json!([{"basis":{"kind":"procedure","reference":"Warning procedure, section 3"},"attachments":[{"title":"Procedure","url":"https://example.test/procedure.pdf"}]}]);
    assert_eq!(call(&app, "POST", "/api/master/references", Some(json!({"references":references}))).await.0, StatusCode::OK);
    call(&app, "POST", "/api/master/start", None).await;
    call(&app, "POST", "/api/master/msel/I-01/send", None).await;
    let parent = reply_parent(&app, "mayor").await;
    let evidence = json!({"basis":{"kind":"procedure","reference":"Warning procedure, section 3"},"attachments":[{"title":"Map","url":"https://example.test/map.pdf"}]});
    let (_, response) = call(&app, "POST", "/api/player/mayor/reply", Some(json!({"text":"warn F4","reply_to":parent,"evidence":evidence}))).await;
    let pid = response["proposal"].as_u64().unwrap();
    eventually(&app, "/api/master", |m| m["proposals"][0]["status"] == "pending").await;
    call(&app, "POST", &format!("/api/master/proposals/{pid}/decide"), Some(json!({"approve":true,"note":"Follow the procedure"}))).await;
    call(&app, "POST", "/api/master/resolve", Some(json!({"minutes":1}))).await;
    let (s, view) = call_key(&app, "GET", "/api/evaluator?role=mayor&turn=1&phase=orders&status=applied", None, Some("test-eval-one")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(view["chains"].as_array().unwrap().len(), 1);
    let chain = &view["chains"][0];
    assert_eq!(chain["proposal"]["evidence"], evidence);
    assert_eq!(chain["proposal"]["decision"]["approve"], true);
    assert_eq!(chain["proposal"]["executions"][0]["ok"], true);
    assert!(chain["reply_delay_wall_ms"].is_number() && chain["master_queue_wall_ms"].is_number());
    assert!(chain["messages"].as_array().unwrap().iter().any(|m| m["id"] == parent));
    assert!(view["messages"].as_array().unwrap().iter().all(|m| m["to"] == "mayor" || m["from"] == "mayor"));
    assert_eq!(view["reference_materials"], references);
    assert_eq!(view["objectives"][0]["msel_id"], "I-01");
    assert!(!view["objectives"][0]["objective"].as_str().unwrap().is_empty());
    let inject = chain["proposal"]["inject_id"].as_u64().unwrap();
    let (_, filtered) = call_key(&app, "GET", &format!("/api/evaluator?inject={inject}&kind=message&limit=1"), None, Some("test-eval-two")).await;
    assert_eq!(filtered["timeline"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["timeline"][0]["kind"], "message");
    assert!(filtered["timeline_total"].as_u64().unwrap() > 1);
    let (_, older) = call_key(&app, "GET", &format!("/api/evaluator?inject={inject}&kind=message&limit=1&offset=1"), None, Some("test-eval-two")).await;
    assert_ne!(filtered["timeline"][0]["seq"], older["timeline"][0]["seq"]);
    call(&app, "POST", "/api/master/rollback", Some(json!({"turn":1}))).await;
    let (_, rolled) = call_key(&app, "GET", "/api/evaluator?role=mayor&status=rolled_back", None, Some("test-eval-one")).await;
    assert_eq!(rolled["chains"][0]["proposal"]["evidence"], evidence);
    assert_eq!(rolled["chains"][0]["proposal"]["executions"][0]["ok"], true);
    let (_, export) = call_key(&app, "GET", "/api/debrief", None, Some("test-eval-two")).await;
    assert_eq!(export["reference_materials"], references);
    assert_eq!(export["proposals"][0]["evidence"], evidence);
    assert!(export["log"].as_array().unwrap().iter().any(|l| l["kind"] == "rollback"));
    assert_eq!(call_key(&app, "GET", "/api/evaluator?role=unknown", None, Some("test-eval-one")).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call_key(&app, "GET", "/api/evaluator?status=unknown", None, Some("test-eval-one")).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn three_scheduled_injects_arrive_in_one_turn_and_training_phase() {
    let app=app().await;
    assert_eq!(call(&app,"POST","/api/master/clock",Some(json!({"running":true,"ratio":60}))).await.0,StatusCode::BAD_REQUEST);
    call(&app,"POST","/api/master/start",None).await;
    for (id,clock) in [("I-01","13:00"),("I-02","13:30"),("I-03","14:00")] {
        assert_eq!(call(&app,"POST","/api/master/schedules",Some(json!({"msel_id":id,"at_clock":clock}))).await.0,StatusCode::OK);
    }
    assert_eq!(call(&app,"POST","/api/master/clock",Some(json!({"running":true,"ratio":86400,"reason":"Compressed acceptance exercise"}))).await.0,StatusCode::OK);
    let delivered=eventually(&app,"/api/state",|s|s["scheduled_injects"].as_array().unwrap().iter().all(|i|i["status"]=="delivered")).await;
    assert_eq!(delivered["turn"],1);assert_eq!(delivered["phase"],"orders");assert_eq!(delivered["exercise_phase"],"early_warning");
    assert_eq!(delivered["scheduled_injects"].as_array().unwrap().iter().map(|i|i["triggered_s"].as_i64().unwrap()).collect::<Vec<_>>(),vec![0,1800,3600]);
    for i in delivered["scheduled_injects"].as_array().unwrap() {
        assert!(i["delivered_s"].as_i64().unwrap()>=i["due_s"].as_i64().unwrap());
        assert!(i["delivered_wall_ms"].as_u64().unwrap()>=i["triggered_wall_ms"].as_u64().unwrap());
        assert!(!i["messages"].as_array().unwrap().is_empty());
    }
    call(&app,"POST","/api/master/clock",Some(json!({"running":false,"ratio":60,"reason":"Review phase"}))).await;
    let (_,paused)=call(&app,"GET","/api/state",None).await;
    tokio::time::sleep(Duration::from_millis(70)).await;
    let (_,still)=call(&app,"GET","/api/state",None).await;assert_eq!(paused["world"],still["world"]);
    call(&app,"POST","/api/master/phase",Some(json!({"phase":"early_action","reason":"Begin protective measures"}))).await;
    let (_,next)=call(&app,"GET","/api/state",None).await;assert_eq!(next["turn"],1);assert_eq!(next["world"],paused["world"]);
    assert_eq!(next["exercise_phase"],"early_action");
    assert_eq!(call(&app,"POST","/api/master/phase",Some(json!({"phase":"escalation","reason":""}))).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,"POST","/api/master/clock",Some(json!({"running":true,"ratio":0}))).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,"POST","/api/master/schedules",Some(json!({"msel_id":"I-01","at_clock":"25:00"}))).await.0,StatusCode::BAD_REQUEST);
    let (_,debrief)=call(&app,"GET","/api/debrief",None).await;
    assert_eq!(debrief["scheduled_injects"].as_array().unwrap().len(),3);
    assert!(debrief["log"].as_array().unwrap().iter().any(|l|l["kind"]=="clock_configured"));
    assert!(debrief["log"].as_array().unwrap().iter().any(|l|l["kind"]=="exercise_phase_changed"));
}

#[tokio::test]
async fn scheduled_delivery_cancellation_and_approved_execution_are_master_only() {
    let app=app().await;call(&app,"POST","/api/master/start",None).await;
    let (_,s)=call(&app,"POST","/api/master/schedules",Some(json!({"msel_id":"I-01","at_s":3600}))).await;
    let id=s["schedule"].as_u64().unwrap();
    assert_eq!(call(&app,"POST",&format!("/api/master/schedules/{id}/cancel"),Some(json!({"reason":"Change the scenario sequence"}))).await.0,StatusCode::OK);
    let parent=reply_parent(&app,"mayor").await;
    let (_,r)=call(&app,"POST","/api/player/mayor/reply",Some(json!({"text":"warn F4","reply_to":parent}))).await;
    let pid=r["proposal"].as_u64().unwrap();
    assert_eq!(call(&app,"POST",&format!("/api/master/proposals/{pid}/execute"),None).await.0,StatusCode::BAD_REQUEST);
    eventually(&app,"/api/master",|m|m["proposals"][0]["status"]=="pending").await;
    call(&app,"POST",&format!("/api/master/proposals/{pid}/decide"),Some(json!({"approve":true}))).await;
    assert_eq!(call(&app,"POST",&format!("/api/master/proposals/{pid}/execute"),None).await.0,StatusCode::OK);
    assert_eq!(call(&app,"POST",&format!("/api/master/proposals/{pid}/execute"),None).await.0,StatusCode::BAD_REQUEST);
    let (_,state)=call(&app,"GET","/api/state",None).await;assert_eq!(state["turn"],1);assert_eq!(state["scheduled_injects"][0]["status"],"cancelled");
    let (_,e)=call_key(&app,"GET","/api/evaluator?exercise_phase=early_warning",None,Some("test-eval-one")).await;
    assert_eq!(e["chains"][0]["proposal"]["status"],"applied");
    assert!(e["timeline"].as_array().unwrap().iter().all(|l|l["exercise_phase"]=="early_warning"));
}

#[tokio::test]
async fn ai_resource_limits_are_recommendations_until_reviewed_and_approved() {
    let scn=Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let ex=exercise::Exercise::new(sim::World::new(scn,1).unwrap());
    let llm=llm::Llm::mock(|task,_,_|if task=="master_prompt"{Ok(r#"{"resource_limits":{"total":{"ground_crews":1},"phases":{"early_warning":{"ground_crews":1}}},"actions":[],"summary":"Limit first-phase deployment to one crew"}"#.into())}else{anyhow::bail!("unexpected task")});
    let app=server::router_with_access(Arc::new(Mutex::new(ex)),llm,None,access()).0;
    call(&app,"POST","/api/master/start",None).await;
    let (_,before)=call(&app,"GET","/api/state",None).await;
    let (_,p)=call(&app,"POST","/api/master/prompt",Some(json!({"text":"Recommend initial crew deployment limits"}))).await;
    let pid=p["proposal"].as_u64().unwrap();
    let proposal=eventually(&app,"/api/master",|m|m["proposals"][0]["status"]=="pending").await;
    assert_eq!(proposal["proposals"][0]["resource_limit_draft"]["total"]["ground_crews"],1);
    let (_,pending)=call(&app,"GET","/api/state",None).await;
    assert_eq!(pending["resource_limits"],before["resource_limits"]);assert_eq!(pending["world"],before["world"]);
    assert_eq!(call(&app,"POST",&format!("/api/master/proposals/{pid}/decide"),Some(json!({"approve":true,"resource_limits":{"total":{"ground_crews":2},"phases":{"early_warning":{"ground_crews":2}}},"note":"Master edits recommendation to two crews"}))).await.0,StatusCode::OK);
    let (_,effective)=call(&app,"GET","/api/state",None).await;assert_eq!(effective["resource_limits"]["total"]["ground_crews"],2);
    assert_eq!(effective["resource_accounting"]["fire_commander"]["ground_crews"]["available"],2);
    let (_,evidence)=call_key(&app,"GET","/api/evaluator",None,Some("test-eval-one")).await;
    assert_eq!(evidence["chains"][0]["proposal"]["decision"]["resource_limits"]["total"]["ground_crews"],2);
    assert!(evidence["timeline"].as_array().unwrap().iter().any(|l|l["kind"]=="resource_limit_draft_reviewed"));
    assert!(evidence["timeline"].as_array().unwrap().iter().any(|l|l["kind"]=="resource_limits_changed" && l["data"]["proposal"]==pid));
    let (_,p)=call(&app,"POST","/api/master/prompt",Some(json!({"text":"Recommend another limit"}))).await;
    let pid=p["proposal"].as_u64().unwrap();
    eventually(&app,"/api/master",|m|m["proposals"][1]["status"]=="pending").await;
    call(&app,"POST",&format!("/api/master/proposals/{pid}/decide"),Some(json!({"approve":false}))).await;
    let (_,after)=call(&app,"GET","/api/state",None).await;assert_eq!(after["resource_limits"],effective["resource_limits"]);
}

#[tokio::test]
async fn configured_limit_controls_enforce_capacity_and_reject_unknown_resources() {
    let app=app().await;call(&app,"POST","/api/master/start",None).await;
    let body=json!({"limits":{"total":{"ground_crews":1},"phases":{"early_warning":{"ground_crews":0}}},"reason":"Initial stand-by phase"});
    assert_eq!(call(&app,"POST","/api/master/resource-limits",Some(body.clone())).await.0,StatusCode::OK);
    let (_,fc)=call_key(&app,"GET","/api/player/fire_commander",None,Some("test-fire")).await;
    assert_eq!(fc["resource_accounting"]["ground_crews"]["available"],0);
    assert_eq!(fc["resource_accounting"]["ground_crews"]["inventory_available"],3);
    assert_eq!(call(&app,"POST","/api/master/resource-limits",Some(json!({"limits":{"total":{"invented":2}},"reason":"invalid"}))).await.0,StatusCode::BAD_REQUEST);
    let (_,state)=call(&app,"GET","/api/state",None).await;assert_eq!(state["resource_limits"],body["limits"]);
    assert_eq!(call_key(&app,"POST","/api/master/resource-limits",Some(body),Some("test-eval-one")).await.0,StatusCode::FORBIDDEN);
}
