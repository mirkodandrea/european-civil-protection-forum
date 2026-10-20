//! Forecast drafts are EXCON-only until an explicit, current-generation publication.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

fn app() -> Router {
    // A real, small fire grid keeps default +1h/+3h projections inexpensive.
    let mut scn = scenario::synthetic::generate(1);
    scn.grid = scenario::GridSpec {
        rows: 16,
        cols: 16,
        cellsize: 20.0,
    };
    scn.fuel = vec![scenario::fuel::GRASSLAND; scn.grid.len()];
    scn.dem = vec![0.0; scn.grid.len()];
    scn.buildings.clear();
    scn.roads.clear();
    scn.sectors.clear();
    scn.places.clear();
    scn.msel.clear();
    scn.communication_sites.clear();
    scn.initial_ignitions = vec![scenario::IgnitionSpec {
        at: [160.0, 160.0],
        radius_m: 25.0,
    }];
    scn.zones.cols = 1;
    scn.zones.rows = 1;
    scn.zones.size_m = 320.0;
    scn.check().unwrap();
    let world = sim::World::with_options(
        Arc::new(scn),
        3,
        sim::FireOptions {
            realizations: 1,
            cutoff: 0.5,
        },
    )
    .unwrap();
    let exercise = Arc::new(Mutex::new(exercise::Exercise::new(world)));
    let llm = llm::Llm::mock(|task, _, _| anyhow::bail!("unexpected LLM task {task}"));
    let access = server::access::Access {
        master: "forecast-master".into(),
        players: BTreeMap::from([
            ("mayor".into(), "forecast-mayor".into()),
            ("fire_commander".into(), "forecast-fire".into()),
        ]),
        evaluators: BTreeMap::from([("reviewer".into(), "forecast-evaluator".into())]),
    };
    server::router_with_access(exercise, llm, None, access).0
}
async fn request(
    app: &Router,
    token: Option<&str>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(
                    body.map(|b| Body::from(b.to_string()))
                        .unwrap_or(Body::empty()),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn master(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    request(app, Some("forecast-master"), method, path, body).await
}
async fn start(app: &Router) {
    assert_eq!(
        master(app, "POST", "/api/master/start", None).await.0,
        StatusCode::OK
    );
}
async fn draft(app: &Router) -> Value {
    let (status, value) = master(
        app,
        "POST",
        "/api/master/forecast",
        Some(json!({"members":1,"horizons_s":[60]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["review_required"], true);
    assert!(value["id"].is_string());
    assert!(value["generation"].is_u64());
    value
}
fn publication(draft: &Value, roles: Value) -> Value {
    json!({"id":draft["id"],"generation":draft["generation"],"roles":roles,"source":"Reviewed exercise weather bulletin"})
}
async fn player(app: &Router, role: &str) -> Value {
    let key = if role == "mayor" {
        "forecast-mayor"
    } else {
        "forecast-fire"
    };
    let (status, value) =
        request(app, Some(key), "GET", &format!("/api/player/{role}"), None).await;
    assert_eq!(status, StatusCode::OK);
    value
}

#[tokio::test]
async fn roles_and_evaluators_cannot_compute_or_publish_forecasts() {
    let app = app();
    for token in [
        Some("forecast-mayor"),
        Some("forecast-fire"),
        Some("forecast-evaluator"),
        None,
    ] {
        for path in ["/api/master/forecast", "/api/master/forecast/publish"] {
            let (status, _) = request(&app, token, "POST", path, Some(json!({}))).await;
            assert_eq!(
                status,
                if token.is_none() {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                },
                "{token:?} {path}"
            );
        }
    }
}

#[tokio::test]
async fn defaults_are_reviewable_and_only_selected_role_receives_published_product() {
    let app = app();
    start(&app).await;
    let (_, before) = master(&app, "GET", "/api/state", None).await;
    let (status, default) = master(&app, "POST", "/api/master/forecast", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{default}");
    assert_eq!(default["review_required"], true);
    assert_eq!(
        default["forecast"]["spec"]["horizons_s"],
        json!([3600, 10800])
    );
    let (_, after) = master(&app, "GET", "/api/state", None).await;
    assert_eq!(
        before["world"], after["world"],
        "projection must not advance/mutate the authority"
    );
    for role in ["mayor", "fire_commander"] {
        let view = player(&app, role).await;
        assert_eq!(view["map"]["forecast_products"], json!([]));
        assert!(!view.to_string().contains(default["id"].as_str().unwrap()));
    }
    let (status, published) = master(
        &app,
        "POST",
        "/api/master/forecast/publish",
        Some(publication(&default, json!(["mayor"]))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["messages"].as_array().unwrap().len(), 1);
    let mayor = player(&app, "mayor").await;
    assert!(mayor["inbox"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["title"] == "Reviewed forecast"
            && m["text_en"]
                .as_str()
                .is_some_and(|s| s.contains("projection"))));
    assert_eq!(
        mayor["map"]["forecast_products"],
        json!([default["forecast"].clone()])
    );
    assert!(mayor["knowledge_events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["observation"]["forecast_products"]
            .as_array()
            .is_some_and(|v| !v.is_empty())));
    assert_eq!(
        player(&app, "fire_commander").await["map"]["forecast_products"],
        json!([])
    );
}

#[tokio::test]
async fn changed_generation_and_rollback_invalidate_drafts_and_map_products() {
    let app = app();
    start(&app).await;
    let old = draft(&app).await;
    let (status, applied) = master(
        &app,
        "POST",
        "/api/master/command",
        Some(json!({"type":"SetWeather","wind_speed_kmh":15})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(applied["results"][0]["ok"], true);
    assert_eq!(
        master(
            &app,
            "POST",
            "/api/master/forecast/publish",
            Some(publication(&old, json!(["mayor"])))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let current = draft(&app).await;
    assert_eq!(
        master(
            &app,
            "POST",
            "/api/master/forecast/publish",
            Some(publication(&current, json!(["mayor"])))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        player(&app, "mayor").await["map"]["forecast_products"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Compute a draft against the published generation, then rewind to T+0.
    let pending = draft(&app).await;
    assert_eq!(
        master(
            &app,
            "POST",
            "/api/master/rollback",
            Some(json!({"turn":1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        player(&app, "mayor").await["map"]["forecast_products"],
        json!([])
    );
    assert_eq!(
        master(
            &app,
            "POST",
            "/api/master/forecast/publish",
            Some(publication(&pending, json!(["mayor"])))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let fresh = draft(&app).await;
    assert_ne!(fresh["generation"], pending["generation"]);
    assert_eq!(
        player(&app, "mayor").await["map"]["forecast_products"],
        json!([])
    );
}

#[tokio::test]
async fn invalid_specs_and_publications_are_rejected_without_leaking_maps() {
    let app = app();
    start(&app).await;
    for spec in [
        json!({"members":0}),
        json!({"members":17}),
        json!({"horizons_s":[]}),
        json!({"horizons_s":[0]}),
        json!({"horizons_s":[21601]}),
        json!({"horizons_s":[60,60]}),
        json!({"horizons_s":[120,60]}),
        json!({"direction_spread_deg":91}),
        json!({"speed_spread_fraction":-0.1}),
        json!({"unknown":true}),
        json!({"bulletin":[{"after_s":0,"weather":{"wind_dir_deg":45,"wind_speed_kmh":10,"moisture_pct":4}}]}),
    ] {
        let (status, _) = master(&app, "POST", "/api/master/forecast", Some(spec.clone())).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{spec}: {status}"
        );
    }
    let product = draft(&app).await;
    for body in [
        publication(&product, json!([])),
        publication(&product, json!(["unknown"])),
        {
            let mut b = publication(&product, json!(["mayor"]));
            b["source"] = json!("");
            b
        },
        {
            let mut b = publication(&product, json!(["mayor"]));
            b["source"] = json!("x".repeat(2001));
            b
        },
        {
            let mut b = publication(&product, json!(["mayor"]));
            b["generation"] = json!(0);
            b
        },
    ] {
        let (status, _) = master(&app, "POST", "/api/master/forecast/publish", Some(body)).await;
        assert!(status.is_client_error(), "{status}");
        assert_eq!(
            player(&app, "mayor").await["map"]["forecast_products"],
            json!([])
        );
    }
    assert_eq!(
        master(
            &app,
            "POST",
            "/api/master/forecast/publish",
            Some(publication(&product, json!(["mayor"])))
        )
        .await
        .0,
        StatusCode::OK
    );
}
