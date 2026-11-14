use axum::{Json, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, SqlErr, TransactionTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    extract::AnyUser,
    password::{hash_password, verify_dummy, verify_password},
    send_verification,
    session::{COOKIE_NAME, clear_cookie, create_session, session_cookie},
    token::hash_token,
    validate::{normalize_email, validate_name, validate_password},
};
use crate::{
    AppState,
    entities::{sessions, tenants, users},
    error::{AppError, AppResult},
    json::ApiJson,
};

/// The body of `GET /auth/me`, also returned by signup and login.
pub async fn me_body(db: &impl ConnectionTrait, user: &users::Model) -> AppResult<Value> {
    let memberships = user
        .find_related(tenants::Entity)
        .order_by_asc(tenants::Column::Name)
        .all(db)
        .await?;
    Ok(json!({
        "user": {
            "id": user.id,
            "email": user.email,
            "name": user.name,
            "email_verified": user.email_verified_at.is_some(),
        },
        "memberships": memberships
            .iter()
            .map(|tenant| json!({ "tenant_id": tenant.id, "tenant_name": tenant.name }))
            .collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
pub struct SignupBody {
    email: String,
    password: String,
    name: String,
}

pub async fn signup(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<SignupBody>,
) -> AppResult<(StatusCode, CookieJar, Json<Value>)> {
    let email = normalize_email(&body.email)?;
    let name = validate_name(&body.name)?;
    validate_password(&body.password)?;

    let taken = users::Entity::find()
        .filter(users::Column::Email.eq(email.as_str()))
        .one(&state.db)
        .await?
        .is_some();
    if taken {
        return Err(AppError::EmailTaken);
    }

    let password_hash = hash_password(body.password).await?;
    let txn = state.db.begin().await?;
    let now = Utc::now().fixed_offset();
    let inserted = users::ActiveModel {
        id: Set(Uuid::now_v7()),
        email: Set(email),
        name: Set(name),
        password_hash: Set(password_hash),
        email_verified_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await;
    let user = match inserted {
        Ok(user) => user,
        // Two signups for one email at the same time: the database picks one.
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            return Err(AppError::EmailTaken);
        }
        Err(err) => return Err(err.into()),
    };
    let token = create_session(&txn, user.id).await?;
    txn.commit().await?;

    // The account exists now. A failure here does not fail the signup: the user can resend.
    if let Err(err) = send_verification(&state, &user).await {
        tracing::error!("could not queue verification email: {err:?}");
    }

    let jar = CookieJar::new().add(session_cookie(&token, state.config.cookie_secure));
    let body = me_body(&state.db, &user).await?;
    Ok((StatusCode::CREATED, jar, Json(body)))
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<LoginBody>,
) -> AppResult<(CookieJar, Json<Value>)> {
    // An email that cannot be valid is an unknown email: the answer is the same.
    let user = match normalize_email(&body.email) {
        Ok(email) => {
            users::Entity::find()
                .filter(users::Column::Email.eq(email))
                .one(&state.db)
                .await?
        }
        Err(_) => None,
    };
    let Some(user) = user else {
        verify_dummy(body.password).await?;
        return Err(AppError::InvalidCredentials);
    };
    if !verify_password(body.password, user.password_hash.clone()).await? {
        return Err(AppError::InvalidCredentials);
    }
    let token = create_session(&state.db, user.id).await?;
    let jar = CookieJar::new().add(session_cookie(&token, state.config.cookie_secure));
    Ok((jar, Json(me_body(&state.db, &user).await?)))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> AppResult<(StatusCode, CookieJar)> {
    if let Some(cookie) = jar.get(COOKIE_NAME) {
        sessions::Entity::delete_many()
            .filter(sessions::Column::TokenHash.eq(hash_token(cookie.value())))
            .exec(&state.db)
            .await?;
    }
    let jar = CookieJar::new().add(clear_cookie(state.config.cookie_secure));
    Ok((StatusCode::NO_CONTENT, jar))
}

pub async fn me(
    State(state): State<AppState>,
    AnyUser(session): AnyUser,
) -> AppResult<Json<Value>> {
    Ok(Json(me_body(&state.db, &session.user).await?))
}
