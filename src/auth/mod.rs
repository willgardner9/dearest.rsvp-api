use axum::{
    Router,
    routing::{get, post},
};
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};
use uuid::Uuid;

use crate::{
    AppState,
    email::{spawn_send, templates},
    entities::{email_tokens, users},
    error::AppResult,
};

pub mod extract;
pub mod handlers;
pub mod origin;
pub mod password;
pub mod session;
pub mod token;
pub mod validate;

pub const PURPOSE_VERIFY_EMAIL: &str = "verify_email";
pub const PURPOSE_RESET_PASSWORD: &str = "reset_password";

/// The routes under `/auth`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(handlers::signup))
        .route("/login", post(handlers::login))
        .route("/logout", post(handlers::logout))
        .route("/me", get(handlers::me))
}

/// Replace the unused verification tokens of the user with a new one (24 hours).
/// After the commit, send the verification email in the background.
pub async fn send_verification(state: &AppState, user: &users::Model) -> AppResult<()> {
    let txn = state.db.begin().await?;
    email_tokens::Entity::delete_many()
        .filter(email_tokens::Column::UserId.eq(user.id))
        .filter(email_tokens::Column::Purpose.eq(PURPOSE_VERIFY_EMAIL))
        .filter(email_tokens::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    let (token, hash) = token::new_token();
    let now = Utc::now();
    email_tokens::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user.id),
        purpose: Set(PURPOSE_VERIFY_EMAIL.to_owned()),
        token_hash: Set(hash),
        expires_at: Set((now + Duration::hours(24)).fixed_offset()),
        used_at: Set(None),
        created_at: Set(now.fixed_offset()),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

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
