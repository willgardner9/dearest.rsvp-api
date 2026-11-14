use axum::{extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait, sea_query::Expr};
use serde::Deserialize;

use super::{
    PURPOSE_RESET_PASSWORD, claim_token, find_open_token, issue_token, mark_verified,
    password::hash_password,
    session::clear_cookie,
    validate::{normalize_email, validate_password},
};
use crate::{
    AppState,
    email::{spawn_send, templates},
    entities::{sessions, users},
    error::AppResult,
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

    let token = issue_token(
        &state.db,
        user.id,
        PURPOSE_RESET_PASSWORD,
        Duration::hours(1),
    )
    .await?;
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
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ConfirmBody>,
) -> AppResult<(StatusCode, CookieJar)> {
    check(&state.limits.reset_confirm_ip, &ip)?;
    validate_password(&body.password)?;
    // Cheap check first, so a bad token does not cost a password hash.
    find_open_token(&state.db, &body.token, PURPOSE_RESET_PASSWORD).await?;
    let password_hash = hash_password(body.password).await?;

    let now = Utc::now().fixed_offset();
    let txn = state.db.begin().await?;
    let token = claim_token(&txn, &body.token, PURPOSE_RESET_PASSWORD).await?;
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
