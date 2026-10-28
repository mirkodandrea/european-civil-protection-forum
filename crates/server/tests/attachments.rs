//! Basis of a response and its PDF attachment: limits, content checks, who
//! may read the document, and what EXCON sees on the reply.
use base64::Engine;
use rocket::{
    http::{ContentType, Header, Status},
    local::asynchronous::Client,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn call(c: &Client, who: &str, path: &str, body: Option<Value>) -> (Status, Value) {
    let req = if let Some(body) = body {
        c.post(path).header(ContentType::JSON).body(body.to_string())
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
            Arc::new(scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED)),
            1,
        )
        .unwrap(),
    );
    ex.start().unwrap();
    ex.set_language("mayor", "it").unwrap();
    let parent = ex
        .send_inject(None, "Smoke", "EXCON", &["mayor".into()], "Smoke over the village", &Default::default())
        .unwrap()[0];
    let llm = llm::Llm::mock(|task, _, user| match task {
        "translate" | "feedback" => Ok(user.into()),
        _ => anyhow::bail!("Unexpected task: {task}"),
    });
    let (app, state) = server::router(Arc::new(Mutex::new(ex)), llm, None);
    (Client::untracked(app).await.unwrap(), state, parent)
}

fn pdf(extra: usize) -> String {
    let mut bytes = b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n".to_vec();
    bytes.resize(bytes.len() + extra, b' ');
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn body(parent: u64, basis: &str, attachment: Value) -> Value {
    json!({"reply_to": parent, "narrative": "Il sindaco avvisa la popolazione", "basis": basis, "attachment": attachment})
}

#[tokio::test]
async fn basis_and_pdf_reach_excon_and_only_excon_and_the_author_read_it() {
    let (c, _, parent) = fixture().await;
    let path = "/api/player/mayor/actions";
    let file = json!({"name": "../piano comunale.pdf", "content_type": "application/pdf", "data": pdf(100)});
    let (status, answer) = call(&c, "mayor", path, Some(body(parent, "plan", file))).await;
    assert_eq!(status, Status::Ok, "{answer}");

    // EXCON sees the basis and the document reference (never the bytes) on the reply.
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let row = &master["inject_responses"][0];
    let reply = &row["replies"][0];
    assert_eq!(reply["basis"], "plan");
    assert_eq!(reply["document"]["name"], "piano comunale.pdf");
    assert!(reply["document"].get("data").is_none());
    assert_eq!(row["pending_proposals"][0]["basis"], "plan");
    let thread_reply = row["thread"].as_array().unwrap().iter().find(|m| m["kind"] == "reply").unwrap();
    assert_eq!(thread_reply["document"]["id"], reply["document"]["id"]);
    let id = reply["document"]["id"].as_u64().unwrap();

    // The same reference sits in the proposal's evidence.
    let (_, mine) = call(&c, "mayor", "/api/player/mayor", None).await;
    assert_eq!(mine["proposals"][0]["evidence"]["basis"]["kind"], "plan");

    // EXCON and the author read it; nobody else.
    let url = format!("/api/attachments/{id}");
    let (status, doc) = call(&c, "master", &url, None).await;
    assert_eq!(status, Status::Ok);
    assert_eq!(doc["content_type"], "application/pdf");
    let bytes = base64::engine::general_purpose::STANDARD.decode(doc["data"].as_str().unwrap()).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(call(&c, "mayor", &url, None).await.0, Status::Ok);
    assert_eq!(call(&c, "fire_commander", &url, None).await.0, Status::BadRequest);
    assert_eq!(call(&c, "evaluator-1", &url, None).await.0, Status::Forbidden);
    assert_eq!(call(&c, "master", "/api/attachments/99999", None).await.0, Status::BadRequest);
}

#[tokio::test]
async fn attachment_is_checked_for_type_size_and_basis() {
    let (c, _, parent) = fixture().await;
    let path = "/api/player/mayor/actions";
    let ok = |data: String| json!({"name": "a.pdf", "content_type": "application/pdf", "data": data});
    // Operational judgement carries no document.
    let (status, _) = call(&c, "mayor", path, Some(body(parent, "operational_judgment", ok(pdf(10))))).await;
    assert_eq!(status, Status::BadRequest);
    // Not a PDF by content, or by declared type.
    let fake = base64::engine::general_purpose::STANDARD.encode(b"<html>not a pdf</html>");
    assert_eq!(call(&c, "mayor", path, Some(body(parent, "law", ok(fake.clone())))).await.0, Status::BadRequest);
    let wrong_type = json!({"name": "a.pdf", "content_type": "text/html", "data": pdf(10)});
    assert_eq!(call(&c, "mayor", path, Some(body(parent, "law", wrong_type))).await.0, Status::BadRequest);
    assert_eq!(call(&c, "mayor", path, Some(body(parent, "law", ok("%%%".into())))).await.0, Status::BadRequest);
    // Over the limit; just under it passes.
    let max = exercise::files::MAX_PDF_BYTES;
    assert_eq!(call(&c, "mayor", path, Some(body(parent, "law", ok(pdf(max))))).await.0, Status::BadRequest);
    let (status, answer) = call(&c, "mayor", path, Some(body(parent, "law", ok(pdf(max - 100))))).await;
    assert_eq!(status, Status::Ok, "{answer}");
    // Nothing was stored for the refused ones.
    let (_, master) = call(&c, "master", "/api/master", None).await;
    assert_eq!(master["inject_responses"][0]["replies"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn basis_without_document_and_the_ai_context() {
    let (c, state, parent) = fixture().await;
    let (status, _) = call(&c, "mayor", "/api/player/mayor/actions", Some(body(parent, "law", Value::Null))).await;
    assert_eq!(status, Status::Ok);
    let (_, master) = call(&c, "master", "/api/master", None).await;
    let reply = &master["inject_responses"][0]["replies"][0];
    assert_eq!(reply["basis"], "law");
    assert!(reply["document"].is_null());
    // The player's AI conversation keeps what the decision rests on.
    let ex = state.lock();
    let turns = ex.conversations.get("mayor");
    assert!(format!("{turns:?}").contains("[basis] laws"));
    // An unknown basis is refused.
    drop(ex);
    let (status, _) = call(&c, "mayor", "/api/player/mayor/actions", Some(body(parent, "gossip", Value::Null))).await;
    assert_eq!(status, Status::UnprocessableEntity);
}
