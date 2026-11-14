use axum::{
    Router,
    routing::{get, post},
};
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait, sea_query::Expr,
};
use uuid::Uuid;

use crate::{
    AppState,
    email::{spawn_send, templates},
    entities::{email_tokens, users},
    error::{AppError, AppResult},
};

pub mod extract;
pub mod handlers;
pub mod origin;
pub mod password;
pub mod reset;
pub mod session;
pub mod token;
pub mod validate;
pub mod verify;

pub const PURPOSE_VERIFY_EMAIL: &str = "verify_email";
pub const PURPOSE_RESET_PASSWORD: &str = "reset_password";

/// The routes under `/auth`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(handlers::signup))
        .route("/login", post(handlers::login))
        .route("/logout", post(handlers::logout))
        .route("/me", get(handlers::me))
        .route("/verify-email", post(verify::verify_email))
        .route("/verify-email/resend", post(verify::resend))
        .route("/password-reset", post(reset::request_reset))
        .route("/password-reset/confirm", post(reset::confirm_reset))
}

/// Replace the unused tokens of the user for `purpose` with a new one that
/// expires after `ttl`. Return the plain token.
pub async fn issue_token(
    db: &DatabaseConnection,
    user_id: Uuid,
    purpose: &str,
    ttl: Duration,
) -> AppResult<String> {
    let txn = db.begin().await?;
    email_tokens::Entity::delete_many()
        .filter(email_tokens::Column::UserId.eq(user_id))
        .filter(email_tokens::Column::Purpose.eq(purpose))
        .filter(email_tokens::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    let (token, hash) = token::new_token();
    let now = Utc::now();
    email_tokens::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        purpose: Set(purpose.to_owned()),
        token_hash: Set(hash),
        expires_at: Set((now + ttl).fixed_offset()),
        used_at: Set(None),
        created_at: Set(now.fixed_offset()),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;
    Ok(token)
}

/// Find a token for `purpose` that is not used and not expired.
pub async fn find_open_token(
    db: &impl ConnectionTrait,
    token: &str,
    purpose: &str,
) -> AppResult<email_tokens::Model> {
    email_tokens::Entity::find()
        .filter(email_tokens::Column::TokenHash.eq(token::hash_token(token)))
        .filter(email_tokens::Column::Purpose.eq(purpose))
        .filter(email_tokens::Column::UsedAt.is_null())
        .filter(email_tokens::Column::ExpiresAt.gt(Utc::now()))
        .one(db)
        .await?
        .ok_or(AppError::InvalidToken)
}

/// Find an open token for `purpose` and mark it as used. Call it inside a transaction.
/// The condition on `used_at` makes two requests at the same time safe: only one wins.
pub async fn claim_token(
    db: &impl ConnectionTrait,
    token: &str,
    purpose: &str,
) -> AppResult<email_tokens::Model> {
    let found = find_open_token(db, token, purpose).await?;
    let claimed = email_tokens::Entity::update_many()
        .col_expr(
            email_tokens::Column::UsedAt,
            Expr::value(Utc::now().fixed_offset()),
        )
        .filter(email_tokens::Column::Id.eq(found.id))
        .filter(email_tokens::Column::UsedAt.is_null())
        .exec(db)
        .await?;
    if claimed.rows_affected != 1 {
        return Err(AppError::InvalidToken);
    }
    Ok(found)
}

/// Replace the unused verification tokens of the user with a new one (24 hours).
/// After the commit, send the verification email in the background.
pub async fn send_verification(state: &AppState, user: &users::Model) -> AppResult<()> {
    let token = issue_token(
        &state.db,
        user.id,
        PURPOSE_VERIFY_EMAIL,
        Duration::hours(24),
    )
    .await?;
    let link = format!(
        "{}/verify-email/confirm?token={token}",
        state.config.app_base_url
    );
    spawn_send(
        state,
        templates::verification_email(&user.email, &user.name, &link),
    );
    Ok(())
}

/// Set `email_verified_at` to now if it is not set. Call it inside a transaction.
pub async fn mark_verified(db: &impl ConnectionTrait, user_id: Uuid) -> AppResult<()> {
    let now = Utc::now().fixed_offset();
    users::Entity::update_many()
        .col_expr(users::Column::EmailVerifiedAt, Expr::value(now))
        .col_expr(users::Column::UpdatedAt, Expr::value(now))
        .filter(users::Column::Id.eq(user_id))
        .filter(users::Column::EmailVerifiedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}
