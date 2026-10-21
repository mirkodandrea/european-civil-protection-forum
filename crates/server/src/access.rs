//! Per-run capability keys. Player identities are bound to keys, never URL claims.
use crate::AppState;
use rand::RngCore;
use rocket::http::{Method, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::Request;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal {
    Master,
    Player { role: String },
    Evaluator { id: String },
}

#[derive(Clone)]
pub struct Access {
    pub master: String,
    pub players: BTreeMap<String, String>,
    pub evaluators: BTreeMap<String, String>,
}

fn key() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Access {
    pub fn new(roles: impl IntoIterator<Item = String>) -> Self {
        Self {
            master: key(),
            players: roles.into_iter().map(|role| (role, key())).collect(),
            evaluators: (1..=3).map(|n| (format!("evaluator-{n}"), key())).collect(),
        }
    }
    pub fn principal(&self, token: &str) -> Option<Principal> {
        if token == self.master {
            return Some(Principal::Master);
        }
        if let Some((id, _)) = self
            .evaluators
            .iter()
            .find(|(_, key)| key.as_str() == token)
        {
            return Some(Principal::Evaluator { id: id.clone() });
        }
        self.players
            .iter()
            .find(|(_, key)| key.as_str() == token)
            .map(|(role, _)| Principal::Player { role: role.clone() })
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Principal {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, ()> {
        let Some(s) = req.rocket().state::<AppState>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let path = req.uri().path();
        let path = path.as_str();
        let bearer = req
            .headers()
            .get_one("authorization")
            .and_then(|h| h.strip_prefix("Bearer "));
        // Browser WebSockets cannot set Authorization. Query keys are accepted only here.
        let ws_token = if path == "/ws" {
            req.query_value::<String>("token").and_then(Result::ok)
        } else {
            None
        };
        let Some(principal) = bearer
            .or(ws_token.as_deref())
            .and_then(|t| s.access.principal(t))
        else {
            return Outcome::Error((Status::Unauthorized, ()));
        };
        let allowed = match &principal {
            Principal::Master => true,
            Principal::Evaluator { .. } => {
                matches!(req.method(), Method::Get | Method::Head)
                    && matches!(
                        path,
                        "/api/access"
                            | "/api/static"
                            | "/api/state"
                            | "/api/master"
                            | "/api/debrief"
                            | "/api/catalogue"
                            | "/api/evaluator"
                            | "/api/evaluator/assessment"
                            | "/ws"
                    )
            }
            Principal::Player { role } => {
                let own = format!("/api/player/{role}");
                path == "/api/static"
                    || path == "/api/access"
                    || path == "/ws"
                    || path == own
                    || ["draft", "draft/submit", "chat", "reply", "language"]
                        .iter()
                        .any(|suffix| path == format!("{own}/{suffix}"))
            }
        };
        if !allowed {
            return Outcome::Error((Status::Forbidden, ()));
        }
        Outcome::Success(principal)
    }
}
