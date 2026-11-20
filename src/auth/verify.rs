use axum::{extract::State, http::StatusCode};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use utoipa::ToSchema;

use super::{
    PURPOSE_VERIFY_EMAIL, claim_token, extract::AnyUser, mark_verified, send_verification,
};
use crate::{
    AppState,
    error::{AppResult, ErrorBody},
    json::ApiJson,
    rate_limit::{ClientIp, check},
};

#[derive(Deserialize, ToSchema)]
pub struct TokenBody {
    token: String,
}

/// Use a verification token one time. A second use gives `invalid_token`.
#[utoipa::path(post, path = "/verify-email", operation_id = "verifyEmail", tag = "auth", request_body = TokenBody, responses(
    (status = 204, description = "Email verified."),
    (status = 400, description = "invalid_input, invalid_token", body = ErrorBody),
    (status = 403, description = "forbidden_origin", body = ErrorBody),
    (status = 429, description = "rate_limited", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
pub async fn verify_email(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<TokenBody>,
) -> AppResult<StatusCode> {
    check(&state.limits.verify_ip, &ip)?;
    let txn = state.db.begin().await?;
    let token = claim_token(&txn, &body.token, PURPOSE_VERIFY_EMAIL).await?;
    mark_verified(&txn, token.user_id).await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/verify-email/resend", operation_id = "resendVerificationEmail", tag = "auth", security(("session" = [])), responses(
    (status = 204, description = "Email sent, or the email is already verified."),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "forbidden_origin", body = ErrorBody),
    (status = 429, description = "rate_limited", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
pub async fn resend(
    State(state): State<AppState>,
    AnyUser(session): AnyUser,
) -> AppResult<StatusCode> {
    check(&state.limits.resend_user, &session.user.id.to_string())?;
    if session.user.email_verified_at.is_none() {
        send_verification(&state, &session.user).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
