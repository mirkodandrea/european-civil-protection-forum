//! Only the EXCON acts (2026-10-29), over HTTP: a player's free-text reply is
//! read by the (scripted) AI into EXCON suggestions; questions are drafts the
//! EXCON sends; the answer triggers a new reading whose diff is shown.
use rocket::{
    http::{ContentType, Header, Status},
    local::asynchronous::Client,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn call(c: &Client, who: &str, path: &str, body: Option<Value>) -> (Status, Value) {
    let req = match body {
        Some(body) => c
            .post(path)
            .header(ContentType::JSON)
            .body(body.to_string()),
        None => c.get(path),
    };
    let r = req
        .header(Header::new("Authorization", format!("Bearer {who}")))
        .dispatch()
        .await;
    (r.status(), r.into_json().await.unwrap_or(Value::Null))
}

const ANSWER: &str = "Lungo la strada da E3 a F3, con il bulldozer del Comune";

/// The model: the first reading leaves the fire line open, the second (the
/// thread holds the answer) completes it and proposes a question to the Mayor.
fn model() -> llm::Llm {
    llm::Llm::mock(|task, _, user| {
        match task {
        "translate" => Ok(user.into()),
        "review_reply" if user.contains(ANSWER) => Ok(json!({
            "actions": [
                {"ref": 1000, "type": "DeployEngine", "role": "fire_commander", "params": {"at": "F4"}, "quote": "Mandiamo due squadre AIB sul lato nord di F4"},
                {"type": "CutFireLine", "role": "fire_commander", "params": {"from": "E3", "to": "F3"}, "quote": "da E3 a F3", "notes": ["The bulldozer is the Municipality's: has the Mayor authorised it?"]}
            ],
            "questions": [{"to": "mayor", "text": "Il DOS usa il bulldozer del Comune per la linea E3–F3: lo autorizza?", "about": [1], "reason": "cross-role"}],
            "summary": "fire line completed"
        }).to_string()),
        "review_reply" => Ok(json!({
            "actions": [
                {"type": "DeployEngine", "role": "fire_commander", "params": {"at": "F4"}, "quote": "Mandiamo due squadre AIB sul lato nord di F4"},
                {"type": "CutFireLine", "role": "fire_commander", "params": {}, "quote": "apriamo una linea tagliafuoco"}
            ],
            "questions": [{"to": "fire_commander", "text": "Per la linea tagliafuoco: tra quali punti e con quali mezzi?", "about": [1]}],
            "summary": "a team and a fire line without endpoints"
        }).to_string()),
        other => anyhow::bail!("unexpected task {other}"),
    }
    })
}

async fn fixture() -> (Client, u64) {
    let mut scn = scenario::synthetic::generate(scenario::synthetic::DEFAULT_SEED);
    scn.enable_operational_model();
    let mut ex = exercise::Exercise::new(sim::World::new(Arc::new(scn), 1).unwrap());
    ex.start().unwrap();
    ex.set_language("fire_commander", "it").unwrap();
    let parent = ex
        .send_inject(None, "Il fronte scavalca il crinale", "Sala operativa", &["fire_commander".into(), "mayor".into()],
            "Ore 14:20, il fronte ha scavalcato il crinale in E3 e scende verso F4, vento W 25 km/h", &Default::default())
        .unwrap()[0];
    let (app, _) = server::router(Arc::new(Mutex::new(ex)), model(), None);
    (Client::untracked(app).await.unwrap(), parent)
}

/// Wait until the review of the newest reply has its reading `revision`.
async fn reviewed(c: &Client, revision: u64) -> Value {
    for _ in 0..200 {
        let (_, m) = call(c, "master", "/api/master", None).await;
        let r = m["reply_review"]["reviews"][0].clone();
        if r["revision"] == revision && r["status"] == "ready" {
            return r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("no reading");
}

fn item<'a>(r: &'a Value, kind: &str) -> &'a Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == kind || i["kind"] == kind)
        .unwrap()
}

#[tokio::test]
async fn the_clarification_loop_over_http() {
    let (c, parent) = fixture().await;
    let (status, body) = call(&c, "fire_commander", "/api/player/fire_commander/actions",
        Some(json!({"reply_to": parent, "kind": "communication", "narrative": "Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco"}))).await;
    assert_eq!(status, Status::Ok, "{body}");
    let r = reviewed(&c, 1).await;
    let line = item(&r, "CutFireLine");
    assert_eq!(
        line["display"], "question_draft",
        "incomplete, with a question to review"
    );
    assert_eq!(line["missing"], json!(["from", "to"]));
    let team = item(&r, "DeployEngine");
    assert_eq!(team["display"], "complete");
    let q = r["questions"][0].clone();
    assert_eq!(q["state"], "draft");
    // The player sees no action, no approval state and no question yet.
    let (_, p) = call(&c, "fire_commander", "/api/player/fire_commander", None).await;
    assert_eq!(p["proposals"][0]["status"], "informational");
    assert!(
        p["clarification_threads"].as_array().unwrap().is_empty(),
        "drafts never reach the player"
    );

    // The complete suggestion runs at once; the question is sent after review.
    let (status, run) = call(
        &c,
        "master",
        &format!("/api/master/review/execute/{}", team["id"]),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{run}");
    assert_eq!(run["execution"]["ok"], true, "{run}");
    let edited =
        "Per la linea tagliafuoco: tra quali punti e con quali mezzi? Indica la zona (es. F4).";
    let (status, sent) = call(
        &c,
        "master",
        &format!("/api/master/review/send/{}", q["id"]),
        Some(json!({"text": edited})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{sent}");
    let (_, p) = call(&c, "fire_commander", "/api/player/fire_commander", None).await;
    let thread = p["clarification_threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["review"] == true)
        .unwrap()
        .clone();
    assert_eq!(thread["status"], "awaiting_player");
    assert_eq!(
        thread["entries"][0]["text"], edited,
        "sent exactly as reviewed"
    );

    // The DOS answers through the ordinary reply box.
    let (status, a) = call(&c, "fire_commander", "/api/player/fire_commander/reply",
        Some(json!({"reply_to": sent["message"], "text": ANSWER, "evidence": {"basis": {"kind": "operational_judgment", "reference": "-"}}}))).await;
    assert_eq!(status, Status::Ok, "{a}");
    assert_eq!(a["answered"], true);
    let r = reviewed(&c, 2).await;
    let line = item(&r, "CutFireLine");
    assert_eq!(line["display"], "complete");
    assert_eq!(
        r["diff"]["completed"],
        json!([line["id"]]),
        "the diff names what the answer completed"
    );
    assert_eq!(
        item(&r, "DeployEngine")["display"],
        "executed",
        "the decision taken stays"
    );
    let cross = r["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["state"] == "draft")
        .unwrap();
    assert_eq!(
        (cross["to"].as_str(), cross["cross_role"].as_bool()),
        (Some("mayor"), Some(true))
    );
    let (_, mayor) = call(&c, "mayor", "/api/player/mayor", None).await;
    assert!(
        mayor["clarification_threads"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the cross-role question waits for EXCON"
    );
    let (status, _) = call(
        &c,
        "master",
        &format!("/api/master/review/discard/{}", cross["id"]),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (status, _) = call(
        &c,
        "master",
        &format!("/api/master/review/close/{}", r["proposal"]),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, Status::Ok);
    // Players cannot use the EXCON review.
    let (status, _) = call(
        &c,
        "fire_commander",
        &format!("/api/master/review/execute/{}", line["id"]),
        Some(json!({})),
    )
    .await;
    assert_ne!(status, Status::Ok);
}
