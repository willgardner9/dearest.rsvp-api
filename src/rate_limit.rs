use std::{net::SocketAddr, num::NonZeroU32};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::error::{AppError, AppResult};

/// The client IP, as a key for a rate limit.
///
/// The API runs behind the Dokploy proxy. The proxy adds the client address as the
/// LAST entry of `X-Forwarded-For`, so a client cannot fake that entry. The first
/// entries can come from the client, so this code never reads them.
pub struct ClientIp(pub String);

fn client_ip(parts: &Parts) -> String {
    if let Some(value) = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        && let Some(last) = value.rsplit(',').next()
        && !last.trim().is_empty()
    {
        return last.trim().to_owned();
    }
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(client_ip(parts)))
    }
}

pub struct RateLimits {
    pub login_ip: DefaultKeyedRateLimiter<String>,
    pub login_email: DefaultKeyedRateLimiter<String>,
    pub signup_ip: DefaultKeyedRateLimiter<String>,
    pub reset_ip: DefaultKeyedRateLimiter<String>,
    pub reset_email: DefaultKeyedRateLimiter<String>,
    pub resend_user: DefaultKeyedRateLimiter<String>,
    pub invite_user: DefaultKeyedRateLimiter<String>,
}

fn per_minute(count: u32) -> DefaultKeyedRateLimiter<String> {
    RateLimiter::keyed(Quota::per_minute(
        NonZeroU32::new(count).expect("count > 0"),
    ))
}

fn per_hour(count: u32) -> DefaultKeyedRateLimiter<String> {
    RateLimiter::keyed(Quota::per_hour(NonZeroU32::new(count).expect("count > 0")))
}

impl RateLimits {
    pub fn new() -> Self {
        Self {
            login_ip: per_minute(10),
            login_email: per_minute(5),
            signup_ip: per_hour(5),
            reset_ip: per_hour(5),
            reset_email: per_hour(3),
            resend_user: per_hour(3),
            invite_user: per_hour(20),
        }
    }

    /// Forget keys that no longer limit anything, so memory does not grow.
    pub fn retain_recent(&self) {
        for limiter in [
            &self.login_ip,
            &self.login_email,
            &self.signup_ip,
            &self.reset_ip,
            &self.reset_email,
            &self.resend_user,
            &self.invite_user,
        ] {
            limiter.retain_recent();
        }
    }
}

impl Default for RateLimits {
    fn default() -> Self {
        Self::new()
    }
}

/// Return `RateLimited` if the key used its quota.
pub fn check(limiter: &DefaultKeyedRateLimiter<String>, key: &str) -> AppResult<()> {
    limiter
        .check_key(&key.to_owned())
        .map_err(|_| AppError::RateLimited)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use axum::http::Request;
    use governor::{Quota, RateLimiter};

    use super::*;
    use crate::error::AppError;

    fn parts(forwarded: Option<&str>) -> axum::http::request::Parts {
        let mut builder = Request::builder().uri("/");
        if let Some(value) = forwarded {
            builder = builder.header("x-forwarded-for", value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn client_ip_uses_last_forwarded_entry() {
        assert_eq!(
            client_ip(&parts(Some("1.1.1.1, 203.0.113.9"))),
            "203.0.113.9"
        );
    }

    #[test]
    fn client_ip_trims_spaces() {
        assert_eq!(
            client_ip(&parts(Some("1.1.1.1,   203.0.113.9  "))),
            "203.0.113.9"
        );
    }

    #[test]
    fn client_ip_without_header_is_unknown() {
        assert_eq!(client_ip(&parts(None)), "unknown");
    }

    #[test]
    fn check_blocks_after_limit() {
        let limiter = RateLimiter::keyed(Quota::per_minute(NonZeroU32::new(2).unwrap()));
        assert!(check(&limiter, "a").is_ok());
        assert!(check(&limiter, "a").is_ok());
        assert!(matches!(check(&limiter, "a"), Err(AppError::RateLimited)));
        assert!(check(&limiter, "b").is_ok());
    }
}
