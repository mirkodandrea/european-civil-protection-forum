//! JSON request bodies that are read whole, so the session journal (see
//! [`crate::persist`]) records exactly what the route received.
//!
//! Rocket's `Data::peek` returns at most 512 bytes; recording from the fairing
//! alone cut every longer body, and a saved session then failed to replay.
use rocket::data::{Data, FromData, Outcome};
use rocket::http::Status;
use rocket::response::{self, Responder};
use rocket::{Request, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub struct Json<T>(pub T);

impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[rocket::async_trait]
impl<'r, T: DeserializeOwned> FromData<'r> for Json<T> {
    type Error = String;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        let limit = req.limits().get("json").unwrap_or(rocket::data::Limits::JSON);
        let bytes = match data.open(limit).into_bytes().await {
            Ok(b) if b.is_complete() => b.into_inner(),
            Ok(_) => return Outcome::Error((Status::PayloadTooLarge, "request body too large".into())),
            Err(e) => return Outcome::Error((Status::BadRequest, e.to_string())),
        };
        crate::persist::record_body(req, &bytes);
        match serde_json::from_slice(&bytes) {
            Ok(v) => Outcome::Success(Json(v)),
            Err(e) if e.is_data() => Outcome::Error((Status::UnprocessableEntity, e.to_string())),
            Err(e) => Outcome::Error((Status::BadRequest, e.to_string())),
        }
    }
}

impl<'r, T: Serialize> Responder<'r, 'static> for Json<T> {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        let body = serde_json::to_vec(&self.0).map_err(|_| Status::InternalServerError)?;
        Response::build_from(body.respond_to(req)?)
            .header(rocket::http::ContentType::JSON)
            .ok()
    }
}
