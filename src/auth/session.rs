use axum::{
    extract::{Request, State},
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use super::token::{hash_token, new_token};
use crate::{
    AppState,
    entities::{sessions, users},
    error::{AppError, AppResult},
};

pub const COOKIE_NAME: &str = "dearest_session";
const SESSION_DAYS: i64 = 30;
const RENEW_AFTER_HOURS: i64 = 1;

/// The session and the user of the current request.
#[derive(Clone)]
pub struct CurrentSession {
    pub session_id: Uuid,
    pub user: users::Model,
}

/// Insert a session row. Return the plain token. The database keeps only the hash.
pub async fn create_session(db: &impl ConnectionTrait, user_id: Uuid) -> AppResult<String> {
    let (token, hash) = new_token();
    let now = Utc::now();
    sessions::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        token_hash: Set(hash),
        created_at: Set(now.fixed_offset()),
        last_seen_at: Set(now.fixed_offset()),
        expires_at: Set((now + Duration::days(SESSION_DAYS)).fixed_offset()),
    }
    .insert(db)
    .await?;
    Ok(token)
}

fn base_cookie(value: String, secure: bool) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, value))
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .path("/")
        .build()
}

pub fn session_cookie(token: &str, secure: bool) -> Cookie<'static> {
    let mut cookie = base_cookie(token.to_owned(), secure);
    cookie.set_max_age(time::Duration::days(SESSION_DAYS));
    cookie
}

pub fn clear_cookie(secure: bool) -> Cookie<'static> {
    let mut cookie = base_cookie(String::new(), secure);
    cookie.set_max_age(time::Duration::ZERO);
    cookie
}

fn sets_session_cookie(response: &Response) -> bool {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .any(|value| value.as_bytes().starts_with(b"dearest_session="))
}

/// Load the session of the request into the request extensions as
/// `Option<CurrentSession>`. Renew a session that the user did not use for an hour.
pub async fn load_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = CookieJar::from_headers(request.headers())
        .get(COOKIE_NAME)
        .map(|cookie| cookie.value().to_owned())
        .filter(|value| !value.is_empty());

    let mut current = None;
    let mut renewed_token = None;
    if let Some(token) = token {
        let now = Utc::now();
        let found = sessions::Entity::find()
            .filter(sessions::Column::TokenHash.eq(hash_token(&token)))
            .filter(sessions::Column::ExpiresAt.gt(now))
            .find_also_related(users::Entity)
            .one(&state.db)
            .await?;
        if let Some((session, Some(user))) = found {
            if now - session.last_seen_at.with_timezone(&Utc) > Duration::hours(RENEW_AFTER_HOURS) {
                sessions::Entity::update_many()
                    .col_expr(
                        sessions::Column::LastSeenAt,
                        Expr::value(now.fixed_offset()),
                    )
                    .col_expr(
                        sessions::Column::ExpiresAt,
                        Expr::value((now + Duration::days(SESSION_DAYS)).fixed_offset()),
                    )
                    .filter(sessions::Column::Id.eq(session.id))
                    .exec(&state.db)
                    .await?;
                renewed_token = Some(token);
            }
            current = Some(CurrentSession {
                session_id: session.id,
                user,
            });
        }
    }
    request.extensions_mut().insert(current);

    let mut response = next.run(request).await;
    // Do not put back a cookie that the handler set or cleared.
    if let Some(token) = renewed_token
        && !sets_session_cookie(&response)
    {
        let cookie = session_cookie(&token, state.config.cookie_secure);
        let value = HeaderValue::from_str(&cookie.to_string())
            .map_err(|err| AppError::Internal(err.into()))?;
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}
