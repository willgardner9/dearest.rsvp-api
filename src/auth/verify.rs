use axum::{extract::State, http::StatusCode};
use sea_orm::TransactionTrait;
use serde::Deserialize;

use super::{
    PURPOSE_VERIFY_EMAIL, claim_token, extract::AnyUser, mark_verified, send_verification,
};
use crate::{AppState, error::AppResult, json::ApiJson, rate_limit::check};

#[derive(Deserialize)]
pub struct TokenBody {
    token: String,
}

/// Use a verification token one time. A second use gives `invalid_token`.
pub async fn verify_email(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<TokenBody>,
) -> AppResult<StatusCode> {
    let txn = state.db.begin().await?;
    let token = claim_token(&txn, &body.token, PURPOSE_VERIFY_EMAIL).await?;
    mark_verified(&txn, token.user_id).await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

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
