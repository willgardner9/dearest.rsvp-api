use std::{env, net::SocketAddr};

use anyhow::{Context, Result};
use axum::http::HeaderValue;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:3000";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_CORS_ALLOWED_ORIGINS: &str = "https://app.dearest.rsvp";
const DEFAULT_APP_BASE_URL: &str = "https://app.dearest.rsvp";
const DEFAULT_SMTP_PORT: u16 = 465;
const DEFAULT_EMAIL_FROM: &str = "Dearest <hello@dearest.rsvp>";

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: bool,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("tls", &self.tls)
            .finish()
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => anyhow::bail!("{key} must be true or false"),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub db_max_connections: u32,
    /// Browser origins that may call the API. Each origin has a scheme and a host.
    pub cors_allowed_origins: Vec<HeaderValue>,
    /// Base URL for links in email. No trailing slash.
    pub app_base_url: String,
    pub smtp: SmtpConfig,
    pub email_from: String,
    /// Set the `Secure` attribute on the session cookie.
    pub cookie_secure: bool,
}

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Read the configuration from any key lookup. Tests use this
    /// function, so they do not change the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let database_url = lookup("DATABASE_URL").context("DATABASE_URL is not set")?;
        let bind_addr = lookup("BIND_ADDR")
            .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned())
            .parse()
            .context("BIND_ADDR is not a valid socket address")?;
        let db_max_connections = match lookup("DB_MAX_CONNECTIONS") {
            Some(value) => value
                .parse()
                .context("DB_MAX_CONNECTIONS is not a valid number")?,
            None => DEFAULT_DB_MAX_CONNECTIONS,
        };

        let cors_allowed_origins = lookup("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|| DEFAULT_CORS_ALLOWED_ORIGINS.to_owned())
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| {
                origin.parse().with_context(|| {
                    format!("CORS_ALLOWED_ORIGINS has an invalid origin: {origin}")
                })
            })
            .collect::<Result<Vec<HeaderValue>>>()?;

        let app_base_url = lookup("APP_BASE_URL")
            .unwrap_or_else(|| DEFAULT_APP_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let bool_setting = |key: &str, default: bool| -> Result<bool> {
            match lookup(key) {
                Some(value) => parse_bool(key, &value),
                None => Ok(default),
            }
        };
        let smtp = SmtpConfig {
            host: lookup("SMTP_HOST").context("SMTP_HOST is not set")?,
            port: match lookup("SMTP_PORT") {
                Some(value) => value.parse().context("SMTP_PORT is not a valid port")?,
                None => DEFAULT_SMTP_PORT,
            },
            username: lookup("SMTP_USERNAME"),
            password: lookup("SMTP_PASSWORD"),
            tls: bool_setting("SMTP_TLS", true)?,
        };
        let email_from = lookup("EMAIL_FROM").unwrap_or_else(|| DEFAULT_EMAIL_FROM.to_owned());
        let cookie_secure = bool_setting("COOKIE_SECURE", true)?;

        Ok(Self {
            database_url,
            bind_addr,
            db_max_connections,
            cors_allowed_origins,
            app_base_url,
            smtp,
            email_from,
            cookie_secure,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn uses_default_bind_addr() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
        ]))
        .unwrap();
        assert_eq!(config.database_url, "postgres://x");
        assert_eq!(config.bind_addr, DEFAULT_BIND_ADDR.parse().unwrap());
        assert_eq!(config.db_max_connections, DEFAULT_DB_MAX_CONNECTIONS);
        assert_eq!(
            config.cors_allowed_origins,
            vec![HeaderValue::from_static("https://app.dearest.rsvp")]
        );
    }

    #[test]
    fn reads_cors_allowed_origins() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            (
                "CORS_ALLOWED_ORIGINS",
                "http://localhost:5173, https://app.dearest.rsvp",
            ),
        ]))
        .unwrap();
        assert_eq!(
            config.cors_allowed_origins,
            vec![
                HeaderValue::from_static("http://localhost:5173"),
                HeaderValue::from_static("https://app.dearest.rsvp"),
            ]
        );
    }

    #[test]
    fn rejects_invalid_cors_allowed_origins() {
        let result = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("CORS_ALLOWED_ORIGINS", "bad\nvalue"),
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn reads_db_max_connections() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("DB_MAX_CONNECTIONS", "25"),
        ]))
        .unwrap();
        assert_eq!(config.db_max_connections, 25);
    }

    #[test]
    fn rejects_invalid_db_max_connections() {
        let result = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("DB_MAX_CONNECTIONS", "many"),
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn reads_bind_addr() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("BIND_ADDR", "127.0.0.1:8080"),
        ]))
        .unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn requires_database_url() {
        assert!(Config::from_lookup(lookup(&[])).is_err());
    }

    #[test]
    fn rejects_invalid_bind_addr() {
        let result = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("BIND_ADDR", "not-an-address"),
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn requires_smtp_host() {
        assert!(Config::from_lookup(lookup(&[("DATABASE_URL", "postgres://x")])).is_err());
    }

    #[test]
    fn uses_auth_defaults() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.resend.com"),
        ]))
        .unwrap();
        assert_eq!(config.app_base_url, "https://app.dearest.rsvp");
        assert_eq!(config.smtp.host, "smtp.resend.com");
        assert_eq!(config.smtp.port, 465);
        assert!(config.smtp.tls);
        assert!(config.smtp.username.is_none());
        assert!(config.smtp.password.is_none());
        assert_eq!(config.email_from, "Dearest <hello@dearest.rsvp>");
        assert!(config.cookie_secure);
    }

    #[test]
    fn reads_auth_settings() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "localhost"),
            ("APP_BASE_URL", "http://localhost:5173/"),
            ("SMTP_PORT", "1025"),
            ("SMTP_USERNAME", "resend"),
            ("SMTP_PASSWORD", "secret-key"),
            ("SMTP_TLS", "false"),
            ("EMAIL_FROM", "Test <t@example.com>"),
            ("COOKIE_SECURE", "false"),
        ]))
        .unwrap();
        assert_eq!(config.app_base_url, "http://localhost:5173");
        assert_eq!(config.smtp.host, "localhost");
        assert_eq!(config.smtp.port, 1025);
        assert_eq!(config.smtp.username.as_deref(), Some("resend"));
        assert_eq!(config.smtp.password.as_deref(), Some("secret-key"));
        assert!(!config.smtp.tls);
        assert_eq!(config.email_from, "Test <t@example.com>");
        assert!(!config.cookie_secure);
    }

    #[test]
    fn rejects_invalid_smtp_port() {
        let result = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("SMTP_PORT", "abc"),
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_bool() {
        let result = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("SMTP_TLS", "yes"),
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn debug_hides_smtp_password() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("SMTP_HOST", "smtp.test"),
            ("SMTP_PASSWORD", "secret-key"),
        ]))
        .unwrap();
        let text = format!("{config:?}");
        assert!(!text.contains("secret-key"));
    }
}
