//! Saved sessions and the inject library: replay after a restart, and the
//! storage endpoints.
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use scenario::InjectTemplate;
use server::persist::{self, Persist};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ttx-persist-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn factory() -> persist::Factory {
    Arc::new(|msel: Option<Vec<InjectTemplate>>| {
        let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
        if let Some(msel) = msel {
            scn.msel = msel;
        }
        Ok(exercise::Exercise::new(sim::World::new(Arc::new(scn), 1)?))
    })
}

fn storage(d: &std::path::Path) -> Arc<Persist> {
    let default = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED).msel;
    Persist::new(d.to_path_buf(), "test".into(), default, factory())
}

async fn boot(p: &Arc<Persist>) -> (Client, server::AppState) {
    let ex = Arc::new(Mutex::new(p.build(p.load_session().map(|s| s.msel).or_else(|| p.load_injects())).unwrap()));
    let (app, state) = server::router_persistent(ex, llm::Llm::off(), None, p.clone());
    (Client::untracked(app).await.unwrap(), state)
}

async fn post(c: &Client, s: &server::AppState, uri: &str, body: Value) -> (Status, Value) {
    let r = c
        .post(uri.to_string())
        .header(ContentType::JSON)
        .header(Header::new("Authorization", format!("Bearer {}", s.access.master)))
        .body(body.to_string())
        .dispatch()
        .await;
    let status = r.status();
    (status, r.into_json().await.unwrap_or(Value::Null))
}

fn fingerprint(s: &server::AppState) -> Value {
    let ex = s.lock();
    json!({
        "turn": ex.turn,
        "phase": format!("{:?}", ex.phase),
        "time": ex.world.time_s(),
        "messages": ex.messages.len(),
        "sent": ex.sent_injects,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn session_survives_a_restart() {
    let d = dir("session");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    assert_eq!(post(&c, &s, "/api/master/start", json!({})).await.0, Status::Ok);
    let inject = json!({"title":"Wind","recipients":["all"],"text":"Wind rising"});
    assert_eq!(post(&c, &s, "/api/master/inject", inject).await.0, Status::Ok);
    assert_eq!(post(&c, &s, "/api/master/resolve", json!({"minutes": 10})).await.0, Status::Ok);
    // A rejected request must not enter the journal.
    assert_ne!(post(&c, &s, "/api/master/rollback", json!({"turn": 99})).await.0, Status::Ok);
    p.autosave(&s, true);
    let before = fingerprint(&s);
    assert_eq!(before["turn"], 2);

    // "Restart": new process state, same directory.
    let p2 = storage(&d);
    let session = p2.load_session().expect("saved session");
    assert_eq!(session.journal.len(), 3, "start, inject, resolve");
    let (_c2, s2) = boot(&p2).await;
    persist::restore(&s2, server::replay_rocket(&s2), session).await.unwrap();
    assert_eq!(fingerprint(&s2), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn unreplayable_session_is_an_error() {
    let d = dir("bad");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    post(&c, &s, "/api/master/start", json!({})).await;
    p.autosave(&s, true);
    let mut session = storage(&d).load_session().unwrap();
    // The same request twice: the second start is refused.
    session.journal.push(session.journal[0].clone());
    let (_c2, s2) = boot(&storage(&d)).await;
    assert!(persist::restore(&s2, server::replay_rocket(&s2), session).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn inject_library_and_deletes() {
    let d = dir("library");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    let one = |recipients: Value| json!({"injects":[{"id":"mine","turn":1,"title":"T","sender":"S","recipients":recipients,"text":"x","objective":""}]});
    let (status, _) = post(&c, &s, "/api/master/storage/injects", one(json!(["nobody"]))).await;
    assert_eq!(status, Status::BadRequest, "unknown recipient");
    let mut ok = one(json!(["all"]));
    ok["new_session"] = json!(true);
    let (status, info) = post(&c, &s, "/api/master/storage/injects", ok).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(info["library"][0]["id"], "mine");
    assert_eq!(s.lock().world.scn.msel[0].id, "mine", "new session uses the library");
    assert_eq!(p.load_injects().unwrap().len(), 1);

    post(&c, &s, "/api/master/start", json!({})).await;
    assert_eq!(s.lock().turn, 1);
    let (status, _) = post(&c, &s, "/api/master/storage/delete", json!({"target":"session"})).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(format!("{:?}", s.lock().phase), "Briefing");
    assert!(!d.join("session.json").exists());
    assert_eq!(s.lock().world.scn.msel[0].id, "mine", "library survives a session delete");

    let (_, info) = post(&c, &s, "/api/master/storage/delete", json!({"target":"all"})).await;
    assert!(info["library"].is_null());
    assert!(s.lock().world.scn.msel.len() > 1, "default injects again");
    let (status, _) = post(&c, &s, "/api/master/storage/delete", json!({"target":"nonsense"})).await;
    assert_eq!(status, Status::BadRequest);
}
