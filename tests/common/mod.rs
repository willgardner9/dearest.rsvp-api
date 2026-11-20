#![allow(dead_code)]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderValue, Method, Request, StatusCode, header},
};
use dearest::{
    AppState,
    config::{Config, SmtpConfig},
    email::{Email, EmailSender, recording::RecordingEmailSender},
};
use http_body_util::BodyExt;
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};
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

/// The application on a new Postgres schema, with all migrations applied.
///
/// Each test has its own schema in one shared database. The pool of the test sets the
/// `search_path` to that schema, so the tests do not see each other's tables. The schema
/// is dropped when the `TestApp` is dropped, also when the test fails.
///
/// A schema is better than a database here. `DROP SCHEMA` is fast, and it works while
/// the pool still has connections open. `DROP DATABASE` waits for a checkpoint, and
/// `DROP DATABASE ... WITH (FORCE)` crashed Postgres on a busy machine.
pub struct TestApp {
    pub router: Router,
    pub db: DatabaseConnection,
    pub email: Arc<RecordingEmailSender>,
    pub state: AppState,
    ip_counter: AtomicU32,
    _schema: SchemaGuard,
}

fn admin_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_owned())
}

static SWEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Drop the schemas of test processes that stopped before they could clean up. A schema
/// is old when its name shows a creation time more than 5 minutes ago. No test runs that long.
async fn remove_stale_schemas(admin: &DatabaseConnection) {
    let rows = admin
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT nspname FROM pg_namespace \
             WHERE nspname ~ '^test_[0-9]{10}_[0-9a-f]{32}$' \
             AND split_part(nspname, '_', 2)::bigint < extract(epoch FROM now()) - 300"
                .to_owned(),
        ))
        .await
        .expect("list stale schemas");
    for row in rows {
        let name: String = row.try_get("", "nspname").expect("read a name");
        // Ignore an error: another process can drop the same schema at the same time.
        let _ = admin.execute_unprepared("SET lock_timeout = '1s'").await;
        let _ = admin
            .execute_unprepared(&format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE"))
            .await;
    }
}

impl TestApp {
    pub async fn spawn() -> TestApp {
        let admin_url = admin_url();
        // The name holds the creation time, so a later run can remove an old schema.
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs();
        let name = format!("test_{seconds}_{}", Uuid::new_v4().simple());

        let admin = Database::connect(&admin_url)
            .await
            .expect("connect to Postgres");
        if !SWEPT.swap(true, Ordering::SeqCst) {
            remove_stale_schemas(&admin).await;
        }
        admin
            .execute_unprepared(&format!("CREATE SCHEMA \"{name}\""))
            .await
            .expect("create test schema");
        let mut options = ConnectOptions::new(admin_url.clone());
        options.set_schema_search_path(name.clone());
        let db = Database::connect(options)
            .await
            .expect("connect to test schema");
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
            ip_counter: AtomicU32::new(0),
            _schema: SchemaGuard { admin_url, name },
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
            .header("x-forwarded-for", forwarded_for)
            // The test requests come from the proxy, on a private address.
            .extension(ConnectInfo(SocketAddr::from(([10, 0, 0, 2], 4000))));
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

    /// Wait until every background task has finished, for example an email send.
    /// This does not depend on timing, so a slow machine cannot make a test fail.
    /// It also makes a "nothing was sent" check exact.
    pub async fn settle(&self) {
        self.state.tasks.close();
        tokio::time::timeout(Duration::from_secs(30), self.state.tasks.wait())
            .await
            .expect("background tasks finish within 30 seconds");
        self.state.tasks.reopen();
    }

    /// Return the last message to `to`, after all background sends finish.
    pub async fn wait_for_email(&self, to: &str) -> Email {
        self.settle().await;
        self.email
            .last_to(to)
            .unwrap_or_else(|| panic!("no email to {to}"))
    }
}

impl TestApp {
    /// A client IP that no earlier call used, so many sign-ups stay under the IP limits.
    pub fn unique_ip(&self) -> String {
        let n = self.ip_counter.fetch_add(1, Ordering::Relaxed);
        format!("198.18.{}.{}", n / 250, n % 250 + 1)
    }

    /// Sign up and verify a user. Return the session cookie.
    pub async fn verified_user(&self, email: &str) -> String {
        let response = self
            .request_from(
                &self.unique_ip(),
                Method::POST,
                "/auth/signup",
                Some(serde_json::json!({ "email": email, "password": "pw", "name": "Alex" })),
                None,
            )
            .await;
        let cookie = response.session_cookie().expect("signup sets a cookie");
        let to = email.trim().to_lowercase();
        // The address can already have an invite email, so find the verification email.
        self.settle().await;
        let message = self
            .email
            .sent()
            .into_iter()
            .rev()
            .find(|email| email.to == to && email.text.contains("/verify-email/confirm"))
            .unwrap_or_else(|| panic!("no verification email to {to}"));
        let token = dearest::email::templates::extract_token(&message);
        let verified = self
            .request(
                Method::POST,
                "/auth/verify-email",
                Some(serde_json::json!({ "token": token })),
                Some(&cookie),
            )
            .await;
        assert_eq!(verified.status, StatusCode::NO_CONTENT);
        cookie
    }

    pub async fn create_tenant(&self, cookie: &str, name: &str) -> Uuid {
        let response = self
            .request(
                Method::POST,
                "/tenants",
                Some(serde_json::json!({ "name": name })),
                Some(cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
        response.body["id"].as_str().unwrap().parse().unwrap()
    }
}

/// Drops the test schema. It is the LAST field of `TestApp`, so it runs after the router
/// and the state are gone.
struct SchemaGuard {
    admin_url: String,
    name: String,
}

impl Drop for SchemaGuard {
    fn drop(&mut self) {
        // `Drop` is not async, so use a thread with its own runtime. A cancelled request
        // can leave a transaction open until the test runtime runs again to roll it back.
        // `DROP SCHEMA` waits for that lock, so wait for the thread only a short time.
        // The thread keeps trying after that, with a lock timeout so it cannot hang.
        // A schema that stays is removed by a later run (see `remove_stale_schemas`).
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build a runtime");
            runtime.block_on(async {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    let _ = admin.execute_unprepared("SET lock_timeout = '1s'").await;
                    for _ in 0..30 {
                        if admin
                            .execute_unprepared(&format!(
                                "DROP SCHEMA IF EXISTS \"{name}\" CASCADE"
                            ))
                            .await
                            .is_ok()
                        {
                            break;
                        }
                    }
                }
            });
            let _ = done.send(());
        });
        let _ = finished.recv_timeout(Duration::from_millis(500));
    }
}
