use axum::{extract::FromRequestParts, http::request::Parts};

use super::session::CurrentSession;
use crate::error::AppError;

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
