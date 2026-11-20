mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse};
use dearest::email::{Email, templates::extract_token};
use sea_orm::ConnectionTrait;
use serde_json::json;

const EMAIL: &str = "alex@example.com";

async fn signup(app: &TestApp, email: &str) -> String {
    let response = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": email, "password": "old-pw", "name": "Alex" })),
            None,
        )
        .await;
    response.session_cookie().unwrap()
}

async fn login(app: &TestApp, password: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/login",
        Some(json!({ "email": EMAIL, "password": password })),
        None,
    )
    .await
}

async fn request_reset(app: &TestApp, email: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/password-reset",
        Some(json!({ "email": email })),
        None,
    )
    .await
}

async fn confirm(app: &TestApp, token: &str, password: &str) -> TestResponse {
    app.request(
        Method::POST,
        "/auth/password-reset/confirm",
        Some(json!({ "token": token, "password": password })),
        None,
    )
    .await
}

fn reset_emails(app: &TestApp, to: &str) -> Vec<Email> {
    app.email
        .sent()
        .into_iter()
        .filter(|email| email.to == to && email.subject == "Reset your Dearest password")
        .collect()
}

/// Wait until `count` reset emails exist for `to`, then return them.
async fn wait_for_resets(app: &TestApp, to: &str, count: usize) -> Vec<Email> {
    app.settle().await;
    let emails = reset_emails(app, to);
    assert!(
        emails.len() >= count,
        "expected {count} reset emails to {to}, got {}",
        emails.len()
    );
    emails
}

#[tokio::test]
async fn reset_request_for_unknown_email_returns_202_and_sends_nothing() {
    let app = TestApp::spawn().await;
    let response = request_reset(&app, "nobody@example.com").await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    app.settle().await;
    assert!(app.email.sent().is_empty());
}

#[tokio::test]
async fn reset_request_with_invalid_email_returns_202() {
    let app = TestApp::spawn().await;
    assert_eq!(
        request_reset(&app, "not-an-email").await.status,
        StatusCode::ACCEPTED
    );
}

#[tokio::test]
async fn reset_request_sends_link() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    assert_eq!(
        request_reset(&app, EMAIL).await.status,
        StatusCode::ACCEPTED
    );
    let emails = wait_for_resets(&app, EMAIL, 1).await;
    assert!(
        emails[0]
            .text
            .contains("https://app.test/reset-password?token=")
    );
}

#[tokio::test]
async fn reset_changes_password() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    let response = confirm(&app, &token, "new-pw").await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(response.set_cookie.unwrap().contains("Max-Age=0"));
    assert_eq!(login(&app, "old-pw").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, "new-pw").await.status, StatusCode::OK);
}

#[tokio::test]
async fn reset_ends_all_sessions() {
    let app = TestApp::spawn().await;
    let signup_cookie = signup(&app, EMAIL).await;
    let first = login(&app, "old-pw").await.session_cookie().unwrap();
    let second = login(&app, "old-pw").await.session_cookie().unwrap();
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    assert_eq!(
        confirm(&app, &token, "new-pw").await.status,
        StatusCode::NO_CONTENT
    );
    for cookie in [signup_cookie, first, second] {
        let me = app
            .request(Method::GET, "/auth/me", None, Some(&cookie))
            .await;
        assert_eq!(me.status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn reset_marks_email_verified() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    confirm(&app, &token, "new-pw").await;
    let response = login(&app, "new-pw").await;
    assert_eq!(response.body["user"]["email_verified"], true);
}

#[tokio::test]
async fn reset_token_works_once() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    assert_eq!(
        confirm(&app, &token, "new-pw").await.status,
        StatusCode::NO_CONTENT
    );
    let second = confirm(&app, &token, "other-pw").await;
    assert_eq!(second.status, StatusCode::BAD_REQUEST);
    assert_eq!(second.body, json!({ "error": "invalid_token" }));
    assert_eq!(login(&app, "new-pw").await.status, StatusCode::OK);
}

#[tokio::test]
async fn reset_token_expires() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    app.db
        .execute_unprepared(
            "UPDATE email_tokens SET expires_at = now() - interval '1 minute' WHERE purpose = 'reset_password'",
        )
        .await
        .unwrap();
    let response = confirm(&app, &token, "new-pw").await;
    assert_eq!(response.body, json!({ "error": "invalid_token" }));
}

#[tokio::test]
async fn new_reset_request_invalidates_old_token() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let first = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    request_reset(&app, EMAIL).await;
    let second = extract_token(&wait_for_resets(&app, EMAIL, 2).await[1]);
    assert_ne!(first, second);
    assert_eq!(
        confirm(&app, &first, "new-pw").await.body,
        json!({ "error": "invalid_token" })
    );
    assert_eq!(
        confirm(&app, &second, "new-pw").await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn reset_request_matches_email_case() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, "ALEX@EXAMPLE.COM").await;
    let emails = wait_for_resets(&app, EMAIL, 1).await;
    assert_eq!(emails[0].to, EMAIL);
}

#[tokio::test]
async fn confirm_rejects_empty_password() {
    let app = TestApp::spawn().await;
    signup(&app, EMAIL).await;
    request_reset(&app, EMAIL).await;
    let token = extract_token(&wait_for_resets(&app, EMAIL, 1).await[0]);
    let response = confirm(&app, &token, "").await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_input" }));
    // The failed attempt does not use the token.
    assert_eq!(
        confirm(&app, &token, "new-pw").await.status,
        StatusCode::NO_CONTENT
    );
}
