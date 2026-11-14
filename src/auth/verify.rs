use axum::{extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use serde::Deserialize;

use super::{
    PURPOSE_VERIFY_EMAIL, extract::AnyUser, mark_verified, send_verification, token::hash_token,
};
use crate::{
    AppState,
    entities::email_tokens,
    error::{AppError, AppResult},
    json::ApiJson,
    rate_limit::check,
};

#[derive(Deserialize)]
pub struct TokenBody {
    token: String,
}

/// Use a verification token one time. A second use gives `invalid_token`.
pub async fn verify_email(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<TokenBody>,
) -> AppResult<StatusCode> {
    let now = Utc::now().fixed_offset();
    let txn = state.db.begin().await?;
    let token = email_tokens::Entity::find()
        .filter(email_tokens::Column::TokenHash.eq(hash_token(&body.token)))
        .filter(email_tokens::Column::Purpose.eq(PURPOSE_VERIFY_EMAIL))
        .filter(email_tokens::Column::UsedAt.is_null())
        .filter(email_tokens::Column::ExpiresAt.gt(now))
        .one(&txn)
        .await?
        .ok_or(AppError::InvalidToken)?;
    // The condition on `used_at` makes two requests at the same time safe: only one wins.
    let claimed = email_tokens::Entity::update_many()
        .col_expr(email_tokens::Column::UsedAt, Expr::value(now))
        .filter(email_tokens::Column::Id.eq(token.id))
        .filter(email_tokens::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    if claimed.rows_affected != 1 {
        return Err(AppError::InvalidToken);
    }
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
