mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use sea_orm::{DatabaseBackend, MockDatabase};
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> axum::Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    dearest::app(common::mock_state(db))
}

fn logout(origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(Method::POST).uri("/auth/logout");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn post_from_other_origin_is_forbidden() {
    let response = app()
        .oneshot(logout(Some("https://evil.example")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body, json!({ "error": "forbidden_origin" }));
}

#[tokio::test]
async fn post_without_origin_is_not_blocked_by_origin_check() {
    let response = app().oneshot(logout(None)).await.unwrap();
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}
