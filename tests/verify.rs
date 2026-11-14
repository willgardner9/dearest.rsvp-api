mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse};
use dearest::{auth::token::new_token, email::templates::extract_token};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::json;

const EMAIL: &str = "alex@example.com";

/// Sign up and return the session cookie and the verification token from the email.
async fn signup(app: &TestApp) -> (String, String) {
    let response = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": EMAIL, "password": "pw", "name": "Alex" })),
            None,
        )
        .await;
    let cookie = response.session_cookie().unwrap();
    let token = extract_token(&app.wait_for_email(EMAIL).await);
    (cookie, token)
}

async fn verify(app: &TestApp, token: &str, cookie: Option<&str>) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/verify-email",
        Some(json!({ "token": token })),
        cookie,
    )
    .await
}

async fn resend(app: &TestApp, cookie: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/verify-email/resend",
        None,
        Some(cookie),
    )
    .await
}

async fn query_string(app: &TestApp, query: &str) -> String {
    let row = app
        .db
        .query_one_raw(Statement::from_string(app.db.get_database_backend(), query))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "v").unwrap()
}

#[tokio::test]
async fn verify_marks_user_verified() {
    let app = TestApp::spawn().await;
    let (cookie, token) = signup(&app).await;
    let response = verify(&app, &token, Some(&cookie)).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let me = app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(me.body["user"]["email_verified"], true);
}

#[tokio::test]
async fn verify_twice_returns_invalid_token() {
    let app = TestApp::spawn().await;
    let (_, token) = signup(&app).await;
    assert_eq!(
        verify(&app, &token, None).await.status,
        StatusCode::NO_CONTENT
    );
    let before = query_string(&app, "SELECT email_verified_at::text AS v FROM users").await;
    let second = verify(&app, &token, None).await;
    assert_eq!(second.status, StatusCode::BAD_REQUEST);
    assert_eq!(second.body, json!({ "error": "invalid_token" }));
    let after = query_string(&app, "SELECT email_verified_at::text AS v FROM users").await;
    assert_eq!(before, after);
}

#[tokio::test]
async fn verify_with_expired_token_returns_invalid_token() {
    let app = TestApp::spawn().await;
    let (_, token) = signup(&app).await;
    app.db
        .execute_unprepared("UPDATE email_tokens SET expires_at = now() - interval '1 minute'")
        .await
        .unwrap();
    let response = verify(&app, &token, None).await;
    assert_eq!(response.body, json!({ "error": "invalid_token" }));
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn verify_with_unknown_token_returns_invalid_token() {
    let app = TestApp::spawn().await;
    let response = verify(&app, "not-a-real-token", None).await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_token" }));
}

#[tokio::test]
async fn verify_works_without_session() {
    let app = TestApp::spawn().await;
    let (_, token) = signup(&app).await;
    assert_eq!(
        verify(&app, &token, None).await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn resend_invalidates_older_token() {
    let app = TestApp::spawn().await;
    let (cookie, first) = signup(&app).await;
    assert_eq!(resend(&app, &cookie).await.status, StatusCode::NO_CONTENT);
    for _ in 0..100 {
        if app.email.sent().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let second = extract_token(&app.email.last_to(EMAIL).unwrap());
    assert_ne!(first, second);
    assert_eq!(
        verify(&app, &first, None).await.body,
        json!({ "error": "invalid_token" })
    );
    assert_eq!(
        verify(&app, &second, None).await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn resend_when_verified_sends_nothing() {
    let app = TestApp::spawn().await;
    let (cookie, token) = signup(&app).await;
    verify(&app, &token, None).await;
    let count = app.email.sent().len();
    assert_eq!(resend(&app, &cookie).await.status, StatusCode::NO_CONTENT);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(app.email.sent().len(), count);
}

#[tokio::test]
async fn resend_without_session_returns_401() {
    let app = TestApp::spawn().await;
    let response = app
        .request(Method::POST, "/auth/verify-email/resend", None, None)
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn reset_token_cannot_verify_email() {
    let app = TestApp::spawn().await;
    signup(&app).await;
    let (plain, hash) = new_token();
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    app.db
        .execute_unprepared(&format!(
            "INSERT INTO email_tokens (id, user_id, purpose, token_hash, expires_at) \
             SELECT gen_random_uuid(), id, 'reset_password', decode('{hex}', 'hex'), now() + interval '1 hour' FROM users"
        ))
        .await
        .unwrap();
    let response = verify(&app, &plain, None).await;
    assert_eq!(response.body, json!({ "error": "invalid_token" }));
}
