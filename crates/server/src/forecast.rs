//! EXCON-only computation and explicit, role-specific publication of forecasts.
use super::*;
use fire::forecast::{Forecast, ForecastJob, ForecastSpec};
use serde::Deserialize;

#[derive(Clone)]
pub(crate) struct Draft {
    generation: u64,
    forecast: Forecast,
}

pub(crate) async fn compute(
    State(s): State<AppState>,
    Json(spec): Json<ForecastSpec>,
) -> ApiResult {
    let permit = s.forecast_slot.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "A forecast is already running".into(),
        )
    })?;
    let (generation, job) = {
        let ex = s.lock();
        if ex.resolving.is_some() || ex.phase == exercise::Phase::Debrief {
            return Err(bad("Forecast requires a stable active exercise"));
        }
        (
            ex.generation,
            ForecastJob::capture(&ex.world.fire, ex.world.scn.clone(), spec)?,
        )
    };
    let forecast = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job.run()
    })
    .await
    .map_err(|_| bad("Forecast worker failed"))??;
    if s.lock().generation != generation {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Exercise changed while forecasting; compute a fresh product".into(),
        ));
    }
    let id = format!("{:016x}", rand::random::<u64>());
    let mut cache = s.forecasts.lock().await;
    cache.retain(|_, d| d.generation == generation);
    while cache.len() >= 8 {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
    cache.insert(
        id.clone(),
        Draft {
            generation,
            forecast: forecast.clone(),
        },
    );
    Ok(Json(
        json!({"id":id,"generation":generation,"forecast":forecast,"review_required":true}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Publish {
    id: String,
    generation: u64,
    roles: Vec<String>,
    source: String,
}
pub(crate) async fn publish(State(s): State<AppState>, Json(b): Json<Publish>) -> ApiResult {
    if b.source.trim().is_empty()
        || b.source.chars().count() > 2000
        || b.roles.is_empty()
        || b.roles.len() > 100
    {
        return Err(bad(
            "Select recipients and provide the reviewed information source",
        ));
    }
    let draft = s
        .forecasts
        .lock()
        .await
        .get(&b.id)
        .cloned()
        .ok_or_else(|| bad("Unknown forecast draft"))?;
    let mut ex = s.lock();
    if ex.generation != b.generation || draft.generation != b.generation {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Exercise changed; review a fresh forecast".into(),
        ));
    }
    if ex.phase == exercise::Phase::Debrief {
        return Err(bad("Exercise ended"));
    }
    let roles = b
        .roles
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if roles.iter().any(|r| !ex.players.contains_key(r)) {
        return Err(bad("Unknown forecast recipient"));
    }
    let observation = exercise::MapObservation {
        forecast_products: vec![draft.forecast.clone()],
        ..Default::default()
    };
    ex.validate_observation(&observation)?;
    let mut messages = Vec::new();
    for role in &roles {
        messages.push(ex.acquire_map(role, &b.source, observation.clone(), None)?);
    }
    ex.record("forecast_published",exercise::MASTER,json!({"draft":b.id,"source":b.source,"roles":roles,"messages":messages,"reviewed_generation":b.generation,"issued_s":draft.forecast.issued_s,"spec":draft.forecast.spec,"assumptions":draft.forecast.assumptions}));
    Ok(Json(json!({"messages":messages})))
}
