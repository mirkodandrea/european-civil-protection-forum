//! Per-run capability keys. Player identities are bound to keys, never URL claims.
use std::collections::BTreeMap;
use axum::{extract::{Request, State}, http::StatusCode, middleware::Next, response::{IntoResponse, Response}, Json};
use rand::RngCore;
use serde::Serialize;
use serde_json::json;
use crate::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal { Master, Player { role: String } }

#[derive(Clone)]
pub struct Access {
    pub master: String,
    pub players: BTreeMap<String, String>,
}

fn key() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Access {
    pub fn new(roles: impl IntoIterator<Item = String>) -> Self {
        Self { master: key(), players: roles.into_iter().map(|role| (role, key())).collect() }
    }
    pub fn principal(&self, token: &str) -> Option<Principal> {
        if token == self.master { return Some(Principal::Master); }
        self.players.iter().find(|(_, key)| key.as_str() == token).map(|(role, _)| Principal::Player { role: role.clone() })
    }
}

pub async fn authorize(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path == "/api/health" { return next.run(req).await; }
    let bearer = req.headers().get("authorization").and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer "));
    // Browser WebSockets cannot set Authorization. Query keys are accepted only here.
    let ws_token = if path == "/ws" {
        req.uri().query().and_then(|q| q.split('&').find_map(|p| p.strip_prefix("token=")))
    } else { None };
    let Some(principal) = bearer.or(ws_token).and_then(|t| s.access.principal(t)) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"access key required"}))).into_response();
    };
    let allowed = match &principal {
        Principal::Master => true,
        Principal::Player { role } => {
            let own = format!("/api/player/{role}");
            path == "/api/static" || path == "/api/access" || path == "/ws" || path == own
                || path == format!("{own}/reply") || path == format!("{own}/language")
        }
    };
    if !allowed { return (StatusCode::FORBIDDEN, Json(json!({"error":"access key does not authorize this endpoint"}))).into_response(); }
    req.extensions_mut().insert(principal);
    next.run(req).await
}
