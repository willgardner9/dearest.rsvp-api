#![allow(dead_code)]

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode, header},
};
use dearest::{
    AppState,
    config::{Config, SmtpConfig},
    email::{Email, EmailSender, recording::RecordingEmailSender},
};
use http_body_util::BodyExt;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

pub const APP_ORIGIN: &str = "https://app.test";

/// A `Config` with fixed values for tests.
pub fn test_config() -> Config {
    Config {
        database_url: "postgres://unused".to_owned(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        db_max_connections: 5,
        cors_allowed_origins: vec![HeaderValue::from_static(APP_ORIGIN)],
        app_base_url: APP_ORIGIN.to_owned(),
        smtp: SmtpConfig {
            host: "localhost".to_owned(),
            port: 1025,
            username: None,
            password: None,
            tls: false,
        },
        email_from: "Dearest <hello@dearest.test>".to_owned(),
        cookie_secure: true,
    }
}

/// An `AppState` for tests that use a mock database.
pub fn mock_state(db: DatabaseConnection) -> AppState {
    AppState::new(
        db,
        test_config(),
        Arc::new(RecordingEmailSender::default()) as Arc<dyn EmailSender>,
    )
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: Value,
    pub set_cookie: Option<String>,
    pub set_cookies: Vec<String>,
}

impl TestResponse {
    /// Return `dearest_session=<value>` from the `Set-Cookie` header.
    pub fn session_cookie(&self) -> Option<String> {
        let header = self.set_cookie.as_ref()?;
        let pair = header.split(';').next()?.trim();
        pair.starts_with("dearest_session=")
            .then(|| pair.to_owned())
    }
}

/// The application on a new Postgres database, with all migrations applied.
pub struct TestApp {
    pub router: Router,
    pub db: DatabaseConnection,
    pub email: Arc<RecordingEmailSender>,
    pub state: AppState,
}

impl TestApp {
    pub async fn spawn() -> TestApp {
        let admin_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_owned());
        let name = format!("test_{}", Uuid::new_v4().simple());
        let admin = Database::connect(&admin_url)
            .await
            .expect("connect to Postgres");
        admin
            .execute_unprepared(&format!("CREATE DATABASE \"{name}\""))
            .await
            .expect("create test database");
        let (base, _) = admin_url.rsplit_once('/').expect("database URL has a path");
        let db = Database::connect(format!("{base}/{name}"))
            .await
            .expect("connect to test database");
        Migrator::up(&db, None).await.expect("run migrations");

        let email = Arc::new(RecordingEmailSender::default());
        let state = AppState::new(
            db.clone(),
            test_config(),
            email.clone() as Arc<dyn EmailSender>,
        );
        TestApp {
            router: dearest::app(state.clone()),
            db,
            email,
            state,
        }
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> TestResponse {
        self.request_from("203.0.113.1", method, path, body, cookie)
            .await
    }

    /// Like `request`, with a chosen `X-Forwarded-For` header value.
    pub async fn request_from(
        &self,
        forwarded_for: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> TestResponse {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::ORIGIN, APP_ORIGIN)
            .header("x-forwarded-for", forwarded_for);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();

        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        TestResponse {
            status,
            body,
            set_cookie: set_cookies.first().cloned(),
            set_cookies,
        }
    }

    /// Wait up to 1 second for a message to `to`. The send runs on a spawned task.
    pub async fn wait_for_email(&self, to: &str) -> Email {
        for _ in 0..100 {
            if let Some(email) = self.email.last_to(to) {
                return email;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no email to {to} within 1 second");
    }
}
