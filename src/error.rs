use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// An error that a handler returns. Each variant maps to one status and one `error` code.
#[derive(Debug)]
pub enum AppError {
    InvalidInput,
    InvalidToken,
    Unauthenticated,
    InvalidCredentials,
    EmailNotVerified,
    ForbiddenOrigin,
    InviteEmailMismatch,
    NotFound,
    EmailTaken,
    AlreadyMember,
    RateLimited,
    Internal(anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidInput => (StatusCode::BAD_REQUEST, "invalid_input"),
            Self::InvalidToken => (StatusCode::BAD_REQUEST, "invalid_token"),
            Self::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::EmailNotVerified => (StatusCode::FORBIDDEN, "email_not_verified"),
            Self::ForbiddenOrigin => (StatusCode::FORBIDDEN, "forbidden_origin"),
            Self::InviteEmailMismatch => (StatusCode::FORBIDDEN, "invite_email_mismatch"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::EmailTaken => (StatusCode::CONFLICT, "email_taken"),
            Self::AlreadyMember => (StatusCode::CONFLICT, "already_member"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(cause) = &self {
            tracing::error!("internal error: {cause:#}");
        }
        let (status, code) = self.parts();
        (status, Json(json!({ "error": code }))).into_response()
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Internal(err.into())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

#[cfg(test)]
mod tests {
    use axum::{http::StatusCode, response::IntoResponse};
    use http_body_util::BodyExt;

    use super::*;

    #[tokio::test]
    async fn internal_error_hides_cause() {
        let response = AppError::Internal(anyhow::anyhow!("secret detail")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], br#"{"error":"internal"}"#);
    }
}
