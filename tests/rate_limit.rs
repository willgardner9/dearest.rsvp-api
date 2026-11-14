mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse};
use serde_json::json;

async fn login_from(app: &TestApp, forwarded: &str, email: &str) -> TestResponse {
    app.request_from(
        forwarded,
        Method::POST,
        "/auth/login",
        Some(json!({ "email": email, "password": "wrong" })),
        None,
    )
    .await
}

#[tokio::test]
async fn login_is_limited_per_email() {
    let app = TestApp::spawn().await;
    for i in 0..5 {
        let response = login_from(&app, &format!("198.51.100.{i}"), "alex@example.com").await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    let sixth = login_from(&app, "198.51.100.99", "ALEX@example.com").await;
    assert_eq!(sixth.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sixth.body, json!({ "error": "rate_limited" }));
}

#[tokio::test]
async fn login_is_limited_per_ip() {
    let app = TestApp::spawn().await;
    for i in 0..10 {
        let response = login_from(&app, "203.0.113.50", &format!("user{i}@example.com")).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    let eleventh = login_from(&app, "203.0.113.50", "user11@example.com").await;
    assert_eq!(eleventh.status, StatusCode::TOO_MANY_REQUESTS);
    let other = login_from(&app, "203.0.113.51", "user12@example.com").await;
    assert_eq!(other.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn spoofed_first_forwarded_entry_does_not_bypass_limit() {
    let app = TestApp::spawn().await;
    for i in 0..10 {
        let forwarded = format!("10.0.0.{i}, 203.0.113.60");
        let response = login_from(&app, &forwarded, &format!("user{i}@example.com")).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    let eleventh = login_from(&app, "10.9.9.9, 203.0.113.60", "user11@example.com").await;
    assert_eq!(eleventh.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn signup_is_limited_per_ip() {
    let app = TestApp::spawn().await;
    for i in 0..5 {
        let response = app
            .request_from(
                "203.0.113.70",
                Method::POST,
                "/auth/signup",
                Some(
                    json!({ "email": format!("u{i}@example.com"), "password": "pw", "name": "U" }),
                ),
                None,
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "signup {i}");
    }
    let sixth = app
        .request_from(
            "203.0.113.70",
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": "u6@example.com", "password": "pw", "name": "U" })),
            None,
        )
        .await;
    assert_eq!(sixth.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn reset_is_limited_per_ip() {
    let app = TestApp::spawn().await;
    for i in 0..5 {
        let response = app
            .request_from(
                "203.0.113.80",
                Method::POST,
                "/auth/password-reset",
                Some(json!({ "email": format!("u{i}@example.com") })),
                None,
            )
            .await;
        assert_eq!(response.status, StatusCode::ACCEPTED);
    }
    let sixth = app
        .request_from(
            "203.0.113.80",
            Method::POST,
            "/auth/password-reset",
            Some(json!({ "email": "u6@example.com" })),
            None,
        )
        .await;
    assert_eq!(sixth.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn reset_email_limit_still_returns_202() {
    let app = TestApp::spawn().await;
    app.request_from(
        "203.0.113.90",
        Method::POST,
        "/auth/signup",
        Some(json!({ "email": "alex@example.com", "password": "pw", "name": "Alex" })),
        None,
    )
    .await;
    for i in 0..4 {
        let response = app
            .request_from(
                &format!("203.0.113.{}", 100 + i),
                Method::POST,
                "/auth/password-reset",
                Some(json!({ "email": "alex@example.com" })),
                None,
            )
            .await;
        assert_eq!(response.status, StatusCode::ACCEPTED, "request {i}");
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let resets = app
        .email
        .sent()
        .into_iter()
        .filter(|email| email.subject == "Reset your Dearest password")
        .count();
    assert_eq!(resets, 3);
}

#[tokio::test]
async fn resend_is_limited_per_user() {
    let app = TestApp::spawn().await;
    let response = app
        .request_from(
            "203.0.113.110",
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": "alex@example.com", "password": "pw", "name": "Alex" })),
            None,
        )
        .await;
    let cookie = response.session_cookie().unwrap();
    for i in 0..3 {
        let response = app
            .request(
                Method::POST,
                "/auth/verify-email/resend",
                None,
                Some(&cookie),
            )
            .await;
        assert_eq!(response.status, StatusCode::NO_CONTENT, "resend {i}");
    }
    let fourth = app
        .request(
            Method::POST,
            "/auth/verify-email/resend",
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(fourth.status, StatusCode::TOO_MANY_REQUESTS);
}
