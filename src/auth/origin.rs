use axum::{
    extract::{Request, State},
    http::{Method, header},
    middleware::Next,
    response::Response,
};

use crate::{AppState, error::AppError};

/// Reject a write request from a browser origin that is not allowed.
/// A request with no `Origin` header passes: only a browser sends the cookie
/// in a CSRF attack, and a browser sends `Origin` on these methods.
pub async fn check_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let is_write = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if is_write
        && let Some(origin) = request.headers().get(header::ORIGIN)
        && !state.config.cors_allowed_origins.contains(origin)
    {
        return Err(AppError::ForbiddenOrigin);
    }
    Ok(next.run(request).await)
}
