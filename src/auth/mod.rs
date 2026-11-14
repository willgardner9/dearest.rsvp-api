use axum::{Router, http::StatusCode, routing::post};

use crate::AppState;

pub mod origin;

/// The routes under `/auth`.
pub fn router() -> Router<AppState> {
    Router::new().route("/logout", post(|| async { StatusCode::NO_CONTENT }))
}
