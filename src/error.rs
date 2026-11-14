use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

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

/// The `error` code in the body of an error response.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
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
    Internal,
}

/// The JSON body of an error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorCode,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn parts(&self) -> (StatusCode, ErrorCode) {
        match self {
            Self::InvalidInput => (StatusCode::BAD_REQUEST, ErrorCode::InvalidInput),
            Self::InvalidToken => (StatusCode::BAD_REQUEST, ErrorCode::InvalidToken),
            Self::Unauthenticated => (StatusCode::UNAUTHORIZED, ErrorCode::Unauthenticated),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, ErrorCode::InvalidCredentials),
            Self::EmailNotVerified => (StatusCode::FORBIDDEN, ErrorCode::EmailNotVerified),
            Self::ForbiddenOrigin => (StatusCode::FORBIDDEN, ErrorCode::ForbiddenOrigin),
            Self::InviteEmailMismatch => (StatusCode::FORBIDDEN, ErrorCode::InviteEmailMismatch),
            Self::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            Self::EmailTaken => (StatusCode::CONFLICT, ErrorCode::EmailTaken),
            Self::AlreadyMember => (StatusCode::CONFLICT, ErrorCode::AlreadyMember),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimited),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(cause) = &self {
            tracing::error!("internal error: {cause:#}");
        }
        let (status, code) = self.parts();
        (status, Json(ErrorBody { error: code })).into_response()
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

    #[test]
    fn error_code_serializes_to_the_wire_string() {
        let cases = [
            (AppError::InvalidInput, "invalid_input"),
            (AppError::InvalidToken, "invalid_token"),
            (AppError::Unauthenticated, "unauthenticated"),
            (AppError::InvalidCredentials, "invalid_credentials"),
            (AppError::EmailNotVerified, "email_not_verified"),
            (AppError::ForbiddenOrigin, "forbidden_origin"),
            (AppError::InviteEmailMismatch, "invite_email_mismatch"),
            (AppError::NotFound, "not_found"),
            (AppError::EmailTaken, "email_taken"),
            (AppError::AlreadyMember, "already_member"),
            (AppError::RateLimited, "rate_limited"),
            (AppError::Internal(anyhow::anyhow!("x")), "internal"),
        ];
        for (error, wire) in cases {
            let (_, code): (_, ErrorCode) = error.parts();
            assert_eq!(serde_json::to_value(code).unwrap(), wire);
        }
    }

    #[tokio::test]
    async fn internal_error_hides_cause() {
        let response = AppError::Internal(anyhow::anyhow!("secret detail")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], br#"{"error":"internal"}"#);
    }
}
