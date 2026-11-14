use std::collections::HashMap;

use axum::{
    extract::{FromRequestParts, Path},
    http::request::Parts,
};
use sea_orm::{ColumnTrait, ModelTrait, QueryFilter};
use uuid::Uuid;

use super::session::CurrentSession;
use crate::{AppState, entities::tenants, error::AppError};

fn current(parts: &Parts) -> Option<CurrentSession> {
    parts
        .extensions
        .get::<Option<CurrentSession>>()
        .cloned()
        .flatten()
}

/// A logged-in user. Verified or not.
pub struct AnyUser(pub CurrentSession);

/// A logged-in user with a verified email.
pub struct VerifiedUser(pub CurrentSession);

/// A user who may be logged out. This extractor never fails.
pub struct MaybeUser(pub Option<CurrentSession>);

impl<S: Send + Sync> FromRequestParts<S> for AnyUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        current(parts).map(Self).ok_or(AppError::Unauthenticated)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for VerifiedUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AnyUser(session) = AnyUser::from_request_parts(parts, state).await?;
        if session.user.email_verified_at.is_none() {
            return Err(AppError::EmailNotVerified);
        }
        Ok(Self(session))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(current(parts)))
    }
}

/// A verified user who is a member of the tenant in the path (`tenant_id`).
/// An ID that is not valid, a tenant that does not exist, and a tenant of
/// someone else all give the same `404`.
pub struct TenantMember {
    pub session: CurrentSession,
    pub tenant: tenants::Model,
}

impl FromRequestParts<AppState> for TenantMember {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let VerifiedUser(session) = VerifiedUser::from_request_parts(parts, state).await?;
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::NotFound)?;
        let tenant_id: Uuid = params
            .get("tenant_id")
            .and_then(|value| value.parse().ok())
            .ok_or(AppError::NotFound)?;
        let tenant = session
            .user
            .find_related(tenants::Entity)
            .filter(tenants::Column::Id.eq(tenant_id))
            .one(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(Self { session, tenant })
    }
}
