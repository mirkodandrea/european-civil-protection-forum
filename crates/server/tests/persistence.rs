//! Saved sessions and the inject library: replay after a restart, and the
//! storage endpoints.
use rocket::http::{ContentType, Header, Status};
use rocket::local::asynchronous::Client;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use server::library::{MemoryLibrary, Scenarios};
use server::persist::{self, Persist};

/// `flow::REPLAYING` is process-wide: while one test replays a session, the
/// autosave of another is skipped and its saved session never appears. The
/// tests of this file therefore run one at a time.
fn gate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: Mutex<()> = Mutex::new(());
    GATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ttx-persist-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A library over the synthetic territory, kept in memory.
fn scenarios() -> Arc<Scenarios> {
    let base = Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED));
    let library = Arc::new(MemoryLibrary::new(vec!["synthetic".into()]));
    Arc::new(Scenarios {
        library,
        base: Arc::new(move |id| {
            anyhow::ensure!(id == "synthetic", "unknown territory {id}");
            Ok(base.clone())
        }),
        build: Arc::new(|pack| {
            let base = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
            let scn = pack.build(base)?;
            Ok(exercise::Exercise::new(sim::World::new(Arc::new(scn), 1)?))
        }),
        fingerprint: Arc::new(|pack| format!("test|{}", pack.base)),
        default_id: "synthetic".into(),
        running: Mutex::new(None),
    })
}

fn storage(d: &std::path::Path) -> Arc<Persist> {
    Persist::new(d.to_path_buf(), scenarios())
}

async fn boot(p: &Arc<Persist>) -> (Client, server::AppState) {
    let pack = p
        .load_session()
        .map(|s| s.pack)
        .or_else(|| p.scenarios.next_pack())
        .unwrap();
    let ex = Arc::new(Mutex::new((p.scenarios.build)(&pack).unwrap()));
    p.scenarios.set_running(&pack);
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
    let _gate = gate();
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
    let _gate = gate();
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

async fn get(c: &Client, s: &server::AppState, uri: &str) -> (Status, Value) {
    let r = c
        .get(uri.to_string())
        .header(Header::new("Authorization", format!("Bearer {}", s.access.master)))
        .dispatch()
        .await;
    let status = r.status();
    (status, r.into_json().await.unwrap_or(Value::Null))
}

/// A pack that renames a role, adds a resource and a new inject.
async fn designed(c: &Client, s: &server::AppState) -> Value {
    let (status, got) = get(c, s, "/api/master/scenarios/synthetic").await;
    assert_eq!(status, Status::Ok);
    let mut pack = got["pack"].clone();
    pack["id"] = json!("night-shift");
    pack["name"] = json!("Night shift");
    pack["edits"]["roles"][0]["name"] = json!("Night mayor");
    pack["edits"]["msel"] = json!([{
        "id": "n-1", "turn": 1, "title": "Night call", "sender": "112",
        "recipients": ["all"], "text": "Smoke seen at night.", "objective": "", "event": ""
    }]);
    pack["edits"]["weather"]["wind_speed_kmh"] = json!(12.0);
    pack
}

#[tokio::test(flavor = "multi_thread")]
async fn scenarios_are_listed_edited_saved_and_started() {
    let _gate = gate();
    let d = dir("scenarios");
    let p = storage(&d);
    let (c, s) = boot(&p).await;

    let (_, list) = get(&c, &s, "/api/master/scenarios").await;
    assert_eq!(list["packs"][0]["id"], "synthetic");
    assert_eq!(list["running"], "synthetic");
    let (_, one) = get(&c, &s, "/api/master/scenarios/synthetic").await;
    assert_eq!(one["builtin"], true);
    assert!(one["pack"]["edits"]["roles"].as_array().unwrap().len() >= 3, "filled in from the base");
    assert!(one["reference"]["zones"].as_array().unwrap().len() > 10);

    // A bare territory cannot be overwritten.
    let (status, _) = post(&c, &s, "/api/master/scenarios/save", json!({"pack": one["pack"]})).await;
    assert_eq!(status, Status::BadRequest);

    // Every problem comes back at once, and nothing is stored.
    let mut broken = designed(&c, &s).await;
    broken["edits"]["roles"][1]["id"] = json!("Not Valid");
    broken["edits"]["roles"][0]["id"] = json!("Also Bad");
    let (_, v) = post(&c, &s, "/api/master/scenarios/validate", json!({"pack": broken})).await;
    assert!(v["problems"].as_array().unwrap().len() >= 2, "{v}");
    let (status, _) = post(&c, &s, "/api/master/scenarios/save", json!({"pack": broken})).await;
    assert_eq!(status, Status::BadRequest);

    let pack = designed(&c, &s).await;
    let (status, saved) = post(&c, &s, "/api/master/scenarios/save", json!({"pack": pack})).await;
    assert_eq!(status, Status::Ok, "{saved}");
    assert!(p.scenarios.library.list().iter().any(|i| i.id == "night-shift"));
    // Saving does not touch the running exercise.
    assert_ne!(s.lock().world.scn.roles[0].name, "Night mayor");

    // Starting it builds a new exercise: new MSEL, weather, role name.
    post(&c, &s, "/api/master/start", json!({})).await;
    let (status, _) = post(&c, &s, "/api/master/scenarios/activate", json!({"id": "night-shift"})).await;
    assert_eq!(status, Status::Ok);
    {
        let ex = s.lock();
        assert_eq!(ex.world.scn.roles[0].name, "Night mayor");
        assert_eq!(ex.world.scn.msel.len(), 1);
        assert_eq!(ex.world.scn.weather.wind_speed_kmh, 12.0);
        assert_eq!(format!("{:?}", ex.phase), "Briefing", "a new exercise, not a continued one");
    }
    assert_eq!(p.scenarios.library.active().as_deref(), Some("night-shift"));
    assert_eq!(p.scenarios.running_pack().unwrap().id, "night-shift");

    // In use: not deletable. Another one running: deletable.
    let (status, _) = post(&c, &s, "/api/master/scenarios/delete", json!({"id": "night-shift"})).await;
    assert_eq!(status, Status::BadRequest);
    post(&c, &s, "/api/master/scenarios/activate", json!({"id": "synthetic"})).await;
    let (status, _) = post(&c, &s, "/api/master/scenarios/delete", json!({"id": "night-shift"})).await;
    assert_eq!(status, Status::Ok);
    let (status, _) = post(&c, &s, "/api/master/scenarios/activate", json!({"id": "night-shift"})).await;
    assert_eq!(status, Status::BadRequest, "deleted");
}

#[tokio::test(flavor = "multi_thread")]
async fn roles_of_the_new_scenario_can_log_in() {
    let _gate = gate();
    let d = dir("roles");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    let mut pack = designed(&c, &s).await;
    let mut role = pack["edits"]["roles"][0].clone();
    role["id"] = json!("night_chief");
    role["name"] = json!("Night chief");
    pack["edits"]["roles"].as_array_mut().unwrap().push(role);
    assert_eq!(post(&c, &s, "/api/master/scenarios/save", json!({"pack": pack})).await.0, Status::Ok);
    assert!(!s.access.has_player("night_chief"));
    post(&c, &s, "/api/master/scenarios/activate", json!({"id": "night-shift"})).await;
    assert!(s.access.has_player("night_chief"));
    let r = c
        .get("/api/player/night_chief")
        .header(Header::new("Authorization", "Bearer night_chief"))
        .dispatch()
        .await;
    assert_eq!(r.status(), Status::Ok);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_replays_into_the_scenario_it_started_from() {
    let _gate = gate();
    let d = dir("pack-session");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    let pack = designed(&c, &s).await;
    post(&c, &s, "/api/master/scenarios/save", json!({"pack": pack})).await;
    post(&c, &s, "/api/master/scenarios/activate", json!({"id": "night-shift"})).await;
    post(&c, &s, "/api/master/start", json!({})).await;
    p.autosave(&s, true);

    // A restart finds the pack in the session even though the new library lacks it.
    let p2 = storage(&d);
    let session = p2.load_session().expect("saved session");
    assert_eq!(session.pack.id, "night-shift");
    let (_c2, s2) = boot(&p2).await;
    persist::restore(&s2, server::replay_rocket(&s2), session).await.unwrap();
    assert_eq!(s2.lock().world.scn.msel[0].id, "n-1");
    assert_eq!(format!("{:?}", s2.lock().phase), "Orders");
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_the_session_restarts_from_the_running_scenario() {
    let _gate = gate();
    let d = dir("deletes");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    let pack = designed(&c, &s).await;
    post(&c, &s, "/api/master/scenarios/save", json!({"pack": pack})).await;
    post(&c, &s, "/api/master/scenarios/activate", json!({"id": "night-shift"})).await;
    post(&c, &s, "/api/master/start", json!({})).await;
    let (status, _) = post(&c, &s, "/api/master/storage/delete", json!({"target":"session"})).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(format!("{:?}", s.lock().phase), "Briefing");
    assert_eq!(s.lock().world.scn.msel[0].id, "n-1", "same scenario");
    let (status, _) = post(&c, &s, "/api/master/storage/delete", json!({"target":"all"})).await;
    assert_eq!(status, Status::Ok);
    assert!(s.lock().world.scn.msel.len() > 1, "default scenario again");
    assert_eq!(p.scenarios.library.active().as_deref(), Some("synthetic"));
    let (status, _) = post(&c, &s, "/api/master/storage/delete", json!({"target":"nonsense"})).await;
    assert_eq!(status, Status::BadRequest);
}

#[tokio::test(flavor = "multi_thread")]
async fn narrative_approval_and_outcome_survive_restart_without_physical_execution() {
    let _gate = gate();
    let d=dir("narrative");let p=storage(&d);let(c,s)=boot(&p).await;
    assert_eq!(post(&c,&s,"/api/master/start",json!({})).await.0,Status::Ok);
    assert_eq!(post(&c,&s,"/api/player/mayor/language",json!({"language":"en"})).await.0,Status::Ok);
    post(&c,&s,"/api/master/inject",json!({"title":"Support","sender":"112","recipients":["mayor"],"text":"Psychological support requested"})).await;
    let parent=s.lock().messages.iter().find(|m|m.kind==exercise::MsgKind::Inject&&m.to=="mayor").unwrap().id;
    let(status,got)=post(&c,&s,"/api/player/mayor/actions",json!({"kind":"decision","basis":"operational_judgment","narrative":"Activate support","reply_to":parent})).await;
    assert_eq!(status,Status::Ok,"{got}");let id=got["proposal"].as_u64().unwrap();
    assert_eq!(post(&c,&s,&format!("/api/master/proposals/{id}/decide"),json!({"approve":true})).await.0,Status::Ok);
    assert_eq!(post(&c,&s,&format!("/api/master/proposals/{id}/narrative-outcome"),json!({"text":"EXCON assigns a support team"})).await.0,Status::Ok);
    server::flow::settle(&s).await;
    p.autosave(&s,true);
    let saved=storage(&d).load_session().unwrap();
    let p2=storage(&d);let(_,s2)=boot(&p2).await;
    persist::restore(&s2,server::replay_rocket(&s2),saved).await.unwrap();
    let ex=s2.lock();let restored=ex.proposal(id).unwrap();
    assert!(restored.narrative);assert_eq!(restored.status,exercise::ProposalStatus::Approved);
    assert_eq!(restored.narrative_outcomes[0].text,"EXCON assigns a support team");
    assert!(restored.commands.is_empty());assert!(restored.executions.is_empty());
    assert!(ex.world.d.allocations.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn attached_pdf_survives_a_restart() {
    use base64::Engine;
    let _gate = gate();
    let d = dir("attachment");
    let p = storage(&d);
    let (c, s) = boot(&p).await;
    assert_eq!(post(&c, &s, "/api/master/start", json!({})).await.0, Status::Ok);
    assert_eq!(post(&c, &s, "/api/player/mayor/language", json!({"language":"en"})).await.0, Status::Ok);
    post(&c, &s, "/api/master/inject", json!({"title":"Plan","sender":"112","recipients":["mayor"],"text":"Which plan?"})).await;
    let parent = s.lock().messages.iter().find(|m| m.kind == exercise::MsgKind::Inject && m.to == "mayor").unwrap().id;
    // Larger than the old 2 MB body limit: it must still be saved and replayed.
    let mut bytes = b"%PDF-1.4\n".to_vec();
    bytes.resize(3 * 1024 * 1024, b'x');
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let (status, got) = post(&c, &s, "/api/player/mayor/actions", json!({"kind":"decision","reply_to":parent,"narrative":"We follow the plan","basis":"plan",
        "attachment":{"name":"plan.pdf","content_type":"application/pdf","data":data}})).await;
    assert_eq!(status, Status::Ok, "{got}");
    let id = got["proposal"].as_u64().unwrap();
    server::flow::settle(&s).await;
    p.autosave(&s, true);
    let saved = storage(&d).load_session().unwrap();
    let p2 = storage(&d);
    let (_, s2) = boot(&p2).await;
    persist::restore(&s2, server::replay_rocket(&s2), saved).await.unwrap();
    let ex = s2.lock();
    let doc = ex.document_of(id).expect("document restored").clone();
    assert_eq!(doc.name, "plan.pdf");
    assert_eq!(ex.file_for(None, doc.id).unwrap().bytes.len(), bytes.len());
}
