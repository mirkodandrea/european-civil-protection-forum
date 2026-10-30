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
        Some(json!({"reply_to": parent, "kind": "communication", "basis": "operational_judgment", "rationale": "Il fronte avanza verso nord",
            "narrative": "Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco"}))).await;
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

    // The complete suggestion is queued for play (the world does not change
    // under the players); "now" runs it at once. The question is sent after review.
    let (status, run) = call(
        &c,
        "master",
        &format!("/api/master/review/execute/{}", team["id"]),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, Status::Ok, "{run}");
    assert!(run["execution"].is_null() && run["queued"].is_u64(), "{run}");
    let (status, _) = call(
        &c,
        "master",
        &format!("/api/master/excon-actions/{}/cancel", run["queued"]),
        Some(json!({"reason": "run it now instead"})),
    )
    .await;
    assert_eq!(status, Status::Ok);
    let (status, run) = call(
        &c,
        "master",
        &format!("/api/master/review/execute/{}", team["id"]),
        Some(json!({"when": "now"})),
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

/// Open (not handled) replies of the inject row of `role`.
fn row_pending(m: &Value, role: &str) -> Vec<Value> {
    m["inject_responses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == role)
        .map(|r| r["pending_proposals"].as_array().unwrap().iter().map(|p| p["id"].clone()).collect())
        .unwrap_or_default()
}

fn row<'a>(m: &'a Value, role: &str) -> &'a Value {
    m["inject_responses"].as_array().unwrap().iter().find(|r| r["role"] == role).unwrap()
}

async fn send_decision(c: &Client, parent: u64, text: &str) -> Value {
    let (status, body) = call(c, "fire_commander", "/api/player/fire_commander/actions",
        Some(json!({"reply_to": parent, "kind": "decision", "basis": "operational_judgment", "rationale": "Stated rationale", "narrative": text}))).await;
    assert_eq!(status, Status::Ok, "{body}");
    body["proposal"].clone()
}

/// Playtest 30/10 ("le risposte si incasinano man mano che arrivano"):
/// successive replies to the same inject stay distinct. Once the EXCON has
/// handled a reply's suggestions it is closed and leaves every pending list;
/// the next reply is a new item with its own suggestions, never merged.
#[tokio::test]
async fn successive_replies_stay_distinct() {
    let (c, parent) = fixture().await;
    let p1 = send_decision(&c, parent, "Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco").await;
    let r1 = reviewed(&c, 1).await;
    assert_eq!(r1["items"].as_array().unwrap().len(), 2, "{r1}");
    assert_eq!(r1["proposal"], p1);
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(row_pending(&m, "fire_commander"), vec![p1.clone()]);

    // The EXCON queues the complete suggestion and ignores the open one...
    for i in r1["items"].as_array().unwrap() {
        let op = if i["display"] == "complete" { "execute" } else { "ignore" };
        let (s, b) = call(&c, "master", &format!("/api/master/review/{op}/{}", i["id"]), Some(json!({}))).await;
        assert_eq!(s, Status::Ok, "{b}");
    }
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(m["reply_review"]["reviews"][0]["status"], "ready", "a draft question is still open");
    // ...and discards the draft question: nothing is left to decide.
    for q in r1["questions"].as_array().unwrap() {
        let (s, b) = call(&c, "master", &format!("/api/master/review/discard/{}", q["id"]), Some(json!({}))).await;
        assert_eq!(s, Status::Ok, "{b}");
    }
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(m["reply_review"]["reviews"][0]["status"], "closed", "a handled reply closes itself");
    assert!(row_pending(&m, "fire_commander").is_empty(), "a handled reply is no longer pending");
    let counts = &m["reply_review"]["counts"];
    assert_eq!((counts["complete"].as_u64(), counts["drafts"].as_u64(), counts["incomplete"].as_u64()), (Some(0), Some(0), Some(0)));
    assert_eq!(row(&m, "fire_commander")["decided_proposals"][0]["id"], p1);
    assert_eq!(row(&m, "fire_commander")["decided_proposals"][0]["review"], "partially_approved", "one run, one ignored");

    // A second reply to the same inject: a new item; the old one stays closed.
    let p2 = send_decision(&c, parent, "Apriamo una linea tagliafuoco").await;
    assert_ne!(p2, p1);
    let r2 = reviewed(&c, 1).await;
    assert_eq!(r2["proposal"], p2);
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(row_pending(&m, "fire_commander"), vec![p2.clone()], "only the new reply is pending");
    let reviews = m["reply_review"]["reviews"].as_array().unwrap();
    assert_eq!(reviews.len(), 2);
    let old = reviews.iter().find(|r| r["proposal"] == p1).unwrap();
    assert_eq!(old["status"], "closed", "the handled reply does not resurface");
    assert_eq!(old["revision"], 1, "the handled reply is not read again");
    for r in reviews {
        for i in r["items"].as_array().unwrap() {
            assert_eq!(i["proposal"], r["proposal"], "suggestions stay with their reply");
        }
    }
    // The first reply's words are not suggestions of the second.
    assert!(r2["items"].as_array().unwrap().iter().all(|i| i["kind"] != "DeployEngine"), "{r2}");
    assert_eq!(m["reply_review"]["counts"]["incomplete"], 1, "only the new reply's gap counts");

    // Closing by hand settles the reply too (nothing run: acknowledged).
    let (s, _) = call(&c, "master", &format!("/api/master/review/close/{p2}"), Some(json!({}))).await;
    assert_eq!(s, Status::Ok);
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert!(row_pending(&m, "fire_commander").is_empty());
    let decided = row(&m, "fire_commander")["decided_proposals"].as_array().unwrap().clone();
    assert_eq!(decided.len(), 2);
    assert_eq!(decided[1]["review"], "acknowledged");
}

/// The player page sends words as a communication: the review still closes
/// once handled, its suggestions stop counting, and a later reply is a new
/// open group next to the closed one.
#[tokio::test]
async fn handled_communication_closes_its_review() {
    let (c, parent) = fixture().await;
    let say = |text: &'static str| json!({"reply_to": parent, "kind": "communication", "narrative": text,
        "basis": "operational_judgment", "rationale": "Il fronte avanza"});
    let (s, _) = call(&c, "fire_commander", "/api/player/fire_commander/actions",
        Some(say("Mandiamo due squadre AIB sul lato nord di F4 e apriamo una linea tagliafuoco"))).await;
    assert_eq!(s, Status::Ok);
    let r1 = reviewed(&c, 1).await;
    let (team, line) = (item(&r1, "DeployEngine")["id"].clone(), item(&r1, "CutFireLine")["id"].clone());
    for (op, id) in [("execute", &team), ("ignore", &line), ("discard", &r1["questions"][0]["id"])] {
        let (s, b) = call(&c, "master", &format!("/api/master/review/{op}/{id}"), Some(json!({}))).await;
        assert_eq!(s, Status::Ok, "{op}: {b}");
    }
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(m["reply_review"]["reviews"][0]["status"], "closed");
    // Restoring the ignored suggestion reopens the review; ignoring it again closes it.
    let (s, _) = call(&c, "master", &format!("/api/master/review/restore/{line}"), Some(json!({}))).await;
    assert_eq!(s, Status::Ok);
    let (_, m) = call(&c, "master", "/api/master", None).await;
    assert_eq!(m["reply_review"]["reviews"][0]["status"], "ready");
    assert_eq!(m["reply_review"]["counts"]["incomplete"], 1);
    let (s, _) = call(&c, "master", &format!("/api/master/review/ignore/{line}"), Some(json!({}))).await;
    assert_eq!(s, Status::Ok);
    let (s, _) = call(&c, "fire_commander", "/api/player/fire_commander/actions", Some(say("Apriamo una linea tagliafuoco"))).await;
    assert_eq!(s, Status::Ok);
    let r2 = reviewed(&c, 1).await;
    assert_ne!(r2["proposal"], r1["proposal"]);
    let (_, m) = call(&c, "master", "/api/master", None).await;
    let states: Vec<_> = m["reply_review"]["reviews"].as_array().unwrap().iter().map(|r| (r["proposal"].clone(), r["status"].clone())).collect();
    assert_eq!(states, vec![(r2["proposal"].clone(), json!("ready")), (r1["proposal"].clone(), json!("closed"))]);
}
