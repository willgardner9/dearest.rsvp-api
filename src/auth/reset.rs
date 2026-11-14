use axum::{extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait, sea_query::Expr,
};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    PURPOSE_RESET_PASSWORD, mark_verified,
    password::hash_password,
    session::clear_cookie,
    token::{hash_token, new_token},
    validate::{normalize_email, validate_password},
};
use crate::{
    AppState,
    email::{spawn_send, templates},
    entities::{email_tokens, sessions, users},
    error::{AppError, AppResult},
    json::ApiJson,
    rate_limit::{ClientIp, check},
};

#[derive(Deserialize)]
pub struct ResetRequestBody {
    email: String,
}

/// Always answer `202`, so the answer does not show if an account exists.
pub async fn request_reset(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ResetRequestBody>,
) -> AppResult<StatusCode> {
    check(&state.limits.reset_ip, &ip)?;
    let Ok(email) = normalize_email(&body.email) else {
        return Ok(StatusCode::ACCEPTED);
    };
    // The email limit answers `202` too, so the limit does not show if an account exists.
    if check(&state.limits.reset_email, &email).is_err() {
        return Ok(StatusCode::ACCEPTED);
    }
    let Some(user) = users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .one(&state.db)
        .await?
    else {
        return Ok(StatusCode::ACCEPTED);
    };

    let txn = state.db.begin().await?;
    email_tokens::Entity::delete_many()
        .filter(email_tokens::Column::UserId.eq(user.id))
        .filter(email_tokens::Column::Purpose.eq(PURPOSE_RESET_PASSWORD))
        .filter(email_tokens::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    let (token, hash) = new_token();
    let now = Utc::now();
    email_tokens::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user.id),
        purpose: Set(PURPOSE_RESET_PASSWORD.to_owned()),
        token_hash: Set(hash),
        expires_at: Set((now + Duration::hours(1)).fixed_offset()),
        used_at: Set(None),
        created_at: Set(now.fixed_offset()),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    let link = format!("{}/reset-password?token={token}", state.config.app_base_url);
    spawn_send(
        &state,
        templates::reset_email(&user.email, &user.name, &link),
    );
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
pub struct ConfirmBody {
    token: String,
    password: String,
}

/// Set a new password, use the token, mark the email as verified, and end all sessions.
pub async fn confirm_reset(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ConfirmBody>,
) -> AppResult<(StatusCode, CookieJar)> {
    validate_password(&body.password)?;
    let password_hash = hash_password(body.password).await?;

    let now = Utc::now().fixed_offset();
    let txn = state.db.begin().await?;
    let token = email_tokens::Entity::find()
        .filter(email_tokens::Column::TokenHash.eq(hash_token(&body.token)))
        .filter(email_tokens::Column::Purpose.eq(PURPOSE_RESET_PASSWORD))
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
    users::Entity::update_many()
        .col_expr(users::Column::PasswordHash, Expr::value(password_hash))
        .col_expr(users::Column::UpdatedAt, Expr::value(now))
        .filter(users::Column::Id.eq(token.user_id))
        .exec(&txn)
        .await?;
    mark_verified(&txn, token.user_id).await?;
    sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(token.user_id))
        .exec(&txn)
        .await?;
    txn.commit().await?;

    let jar = CookieJar::new().add(clear_cookie(state.config.cookie_secure));
    Ok((StatusCode::NO_CONTENT, jar))
}
