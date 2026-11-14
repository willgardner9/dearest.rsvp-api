mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::{Value, json};

async fn signup(app: &TestApp, email: &str, password: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/signup",
        Some(json!({ "email": email, "password": password, "name": "Alex" })),
        None,
    )
    .await
}

async fn login(app: &TestApp, email: &str, password: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/login",
        Some(json!({ "email": email, "password": password })),
        None,
    )
    .await
}

async fn me(app: &TestApp, cookie: &str) -> TestResponse {
    app.request(Method::GET, "/auth/me", None, Some(cookie))
        .await
}

async fn sql(app: &TestApp, statement: &str) {
    app.db.execute_unprepared(statement).await.unwrap();
}

async fn sql_bool(app: &TestApp, query: &str) -> bool {
    let row = app
        .db
        .query_one_raw(Statement::from_string(app.db.get_database_backend(), query))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "v").unwrap()
}

#[tokio::test]
async fn signup_creates_unverified_user_and_session() {
    let app = TestApp::spawn().await;
    let response = signup(&app, "alex@example.com", "pw").await;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.body["user"]["email"], "alex@example.com");
    assert_eq!(response.body["user"]["email_verified"], false);
    assert_eq!(response.body["memberships"], json!([]));
    let cookie = response.session_cookie().expect("session cookie");

    let current = me(&app, &cookie).await;
    assert_eq!(current.status, StatusCode::OK);
    assert_eq!(current.body["user"], response.body["user"]);
}

#[tokio::test]
async fn signup_cookie_has_required_attributes() {
    let app = TestApp::spawn().await;
    let header = signup(&app, "alex@example.com", "pw")
        .await
        .set_cookie
        .unwrap();
    for part in [
        "HttpOnly",
        "Secure",
        "SameSite=Lax",
        "Path=/",
        "Max-Age=2592000",
    ] {
        assert!(header.contains(part), "{part} missing in {header}");
    }
    assert!(!header.contains("Domain"), "{header}");
}

#[tokio::test]
async fn signup_sends_verification_email() {
    let app = TestApp::spawn().await;
    signup(&app, "alex@example.com", "pw").await;
    let email = app.wait_for_email("alex@example.com").await;
    assert!(
        email
            .text
            .contains("https://app.test/verify-email/confirm?token=")
    );
}

#[tokio::test]
async fn signup_stores_only_password_hash() {
    let app = TestApp::spawn().await;
    signup(&app, "alex@example.com", "hunter2-plain").await;
    assert!(
        sql_bool(
            &app,
            "SELECT (password_hash LIKE '$argon2id$%' AND position('hunter2-plain' in password_hash) = 0) AS v FROM users"
        )
        .await
    );
}

#[tokio::test]
async fn signup_with_taken_email_returns_409() {
    let app = TestApp::spawn().await;
    signup(&app, "alex@example.com", "pw").await;
    let second = signup(&app, "ALEX@example.com", "pw").await;
    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(second.body, json!({ "error": "email_taken" }));
}

#[tokio::test]
async fn signup_rejects_empty_password() {
    let app = TestApp::spawn().await;
    let response = signup(&app, "alex@example.com", "").await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_input" }));
}

#[tokio::test]
async fn login_with_correct_password_sets_cookie() {
    let app = TestApp::spawn().await;
    signup(&app, "alex@example.com", "pw").await;
    let response = login(&app, "alex@example.com", "pw").await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["user"]["email"], "alex@example.com");
    assert!(response.session_cookie().is_some());
}

#[tokio::test]
async fn login_with_wrong_password_returns_401() {
    let app = TestApp::spawn().await;
    signup(&app, "alex@example.com", "pw").await;
    let response = login(&app, "alex@example.com", "wrong").await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body, json!({ "error": "invalid_credentials" }));
}

#[tokio::test]
async fn login_with_unknown_email_returns_401() {
    let app = TestApp::spawn().await;
    let response = login(&app, "nobody@example.com", "pw").await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body, json!({ "error": "invalid_credentials" }));
}

#[tokio::test]
async fn login_ignores_email_case_and_spaces() {
    let app = TestApp::spawn().await;
    let created = signup(&app, " Alex@Example.COM ", "pw").await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["user"]["email"], "alex@example.com");
    let response = login(&app, "alex@example.com", "pw").await;
    assert_eq!(response.status, StatusCode::OK);
    let response = login(&app, "  ALEX@example.com", "pw").await;
    assert_eq!(response.status, StatusCode::OK);
}

#[tokio::test]
async fn me_without_cookie_returns_401() {
    let app = TestApp::spawn().await;
    let response = app.request(Method::GET, "/auth/me", None, None).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body, json!({ "error": "unauthenticated" }));
}

#[tokio::test]
async fn logout_deletes_session() {
    let app = TestApp::spawn().await;
    let cookie = signup(&app, "alex@example.com", "pw")
        .await
        .session_cookie()
        .unwrap();
    let response = app
        .request(Method::POST, "/auth/logout", None, Some(&cookie))
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(me(&app, &cookie).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_without_session_returns_204() {
    let app = TestApp::spawn().await;
    let response = app.request(Method::POST, "/auth/logout", None, None).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn expired_session_returns_401() {
    let app = TestApp::spawn().await;
    let cookie = signup(&app, "alex@example.com", "pw")
        .await
        .session_cookie()
        .unwrap();
    sql(
        &app,
        "UPDATE sessions SET expires_at = now() - interval '1 minute'",
    )
    .await;
    assert_eq!(me(&app, &cookie).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn stale_session_is_renewed() {
    let app = TestApp::spawn().await;
    let cookie = signup(&app, "alex@example.com", "pw")
        .await
        .session_cookie()
        .unwrap();
    sql(&app, "UPDATE sessions SET last_seen_at = now() - interval '2 hours', expires_at = now() + interval '1 day'").await;
    let response = me(&app, &cookie).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.session_cookie().as_deref(), Some(cookie.as_str()));
    assert!(
        sql_bool(
            &app,
            "SELECT (expires_at > now() + interval '29 days' AND expires_at < now() + interval '31 days') AS v FROM sessions"
        )
        .await
    );
}

#[tokio::test]
async fn fresh_session_is_not_renewed() {
    let app = TestApp::spawn().await;
    let cookie = signup(&app, "alex@example.com", "pw")
        .await
        .session_cookie()
        .unwrap();
    let response = me(&app, &cookie).await;
    assert!(response.set_cookie.is_none());
}

#[tokio::test]
async fn logout_clears_cookie_even_when_session_renews() {
    let app = TestApp::spawn().await;
    let cookie = signup(&app, "alex@example.com", "pw")
        .await
        .session_cookie()
        .unwrap();
    sql(
        &app,
        "UPDATE sessions SET last_seen_at = now() - interval '2 hours'",
    )
    .await;
    let response = app
        .request(Method::POST, "/auth/logout", None, Some(&cookie))
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let session_cookies: Vec<_> = response
        .set_cookies
        .iter()
        .filter(|c| c.starts_with("dearest_session="))
        .collect();
    assert_eq!(session_cookies.len(), 1, "{:?}", response.set_cookies);
    assert!(
        session_cookies[0].contains("Max-Age=0"),
        "{}",
        session_cookies[0]
    );
}

#[tokio::test]
async fn invalid_json_returns_invalid_input() {
    let app = TestApp::spawn().await;
    let response = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(Value::String("x".into())),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_input" }));
}
