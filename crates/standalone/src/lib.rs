//! Browser bridge for main's original JavaScript views and unchanged model.
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;
thread_local! { static STATE: RefCell<Option<server::AppState>> = const { RefCell::new(None) }; }
fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}
#[wasm_bindgen]
pub fn initialize() -> Result<(), JsValue> {
    let scenario = serde_json::from_str(include_str!(
        "../../../data/scenarios/synthetic_valley.json"
    ))
    .map_err(error)?;
    let world = sim::World::with_options(
        Arc::new(scenario),
        1,
        sim::FireOptions {
            realizations: 32,
            cutoff: 0.5,
        },
    )
    .map_err(error)?;
    STATE.with(|s| *s.borrow_mut() = Some(server::browser::new(exercise::Exercise::new(world))));
    Ok(())
}
#[wasm_bindgen]
pub async fn request(role: String, url: String, body: Option<String>) -> Result<String, JsValue> {
    let state = STATE
        .with(|s| s.borrow().clone())
        .ok_or_else(|| error("Exercise not initialized"))?;
    let body = body
        .map(|b| serde_json::from_str(&b))
        .transpose()
        .map_err(error)?;
    let value = server::browser::request(state, &role, &url, body)
        .await
        .map_err(error)?;
    serde_json::to_string(&value).map_err(error)
}
#[wasm_bindgen]
pub fn configure_ai(key: String, model: String) -> Result<(), JsValue> {
    if model.trim().is_empty() {
        return Err(error("Model required"));
    }
    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        let state = slot
            .as_mut()
            .ok_or_else(|| error("Exercise not initialized"))?;
        let configured = if key.trim().is_empty() {
            llm::Llm::off()
        } else {
            llm::Llm::openrouter(
                key.trim().into(),
                model.trim().into(),
                llm::DEFAULT_BASE_URL.into(),
            )
            .with_providers(vec![])
        };
        state.llm.configure_runtime(configured);
        Ok(())
    })
}
