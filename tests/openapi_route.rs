//! The server sends the OpenAPI document at `/openapi.json`.
//! Frontend builds read this document from the deployed API.
//! A bad document breaks each frontend build, so these tests run in CI.
mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use sea_orm::{DatabaseBackend, MockDatabase};
use serde_json::Value;
use tower::ServiceExt;

async fn get_spec() -> (StatusCode, Option<String>, Value) {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let response = dearest::app(common::mock_state(db))
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        content_type,
        serde_json::from_slice(&bytes).unwrap(),
    )
}

#[tokio::test]
async fn openapi_route_sends_the_spec() {
    let (status, content_type, body) = get_spec().await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(
        body,
        serde_json::to_value(dearest::openapi::spec()).unwrap()
    );
}

#[tokio::test]
async fn openapi_route_sends_a_usable_document() {
    let (_, _, body) = get_spec().await;

    assert!(body["openapi"].as_str().unwrap().starts_with("3."));
    assert!(!body["paths"].as_object().unwrap().is_empty());
    assert!(body["components"]["schemas"].get("ErrorBody").is_some());
}
