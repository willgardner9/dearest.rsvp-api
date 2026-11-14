mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn unverified_user_cannot_create_tenant() {
    let app = TestApp::spawn().await;
    let signup = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": "alex@example.com", "password": "pw", "name": "Alex" })),
            None,
        )
        .await;
    let cookie = signup.session_cookie().unwrap();
    let response = app
        .request(
            Method::POST,
            "/tenants",
            Some(json!({ "name": "Alex & Sam" })),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.body, json!({ "error": "email_not_verified" }));
}

#[tokio::test]
async fn unverified_user_can_still_use_me_and_logout() {
    let app = TestApp::spawn().await;
    let signup = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": "alex@example.com", "password": "pw", "name": "Alex" })),
            None,
        )
        .await;
    let cookie = signup.session_cookie().unwrap();
    let me = app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(me.status, StatusCode::OK);
    let logout = app
        .request(Method::POST, "/auth/logout", None, Some(&cookie))
        .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn create_tenant_without_session_returns_401() {
    let app = TestApp::spawn().await;
    let response = app
        .request(Method::POST, "/tenants", Some(json!({ "name": "X" })), None)
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_tenant_adds_membership() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    let response = app
        .request(
            Method::POST,
            "/tenants",
            Some(json!({ "name": "  Alex & Sam " })),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.body["name"], "Alex & Sam");
    let id = response.body["id"].as_str().unwrap().to_owned();
    let me = app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(
        me.body["memberships"],
        json!([{ "tenant_id": id, "tenant_name": "Alex & Sam" }])
    );
}

#[tokio::test]
async fn user_can_create_second_tenant() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    app.create_tenant(&cookie, "Zed").await;
    app.create_tenant(&cookie, "Alpha").await;
    let me = app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    let names: Vec<_> = me.body["memberships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["tenant_name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Alpha", "Zed"]);
}

#[tokio::test]
async fn create_tenant_rejects_blank_name() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    let response = app
        .request(
            Method::POST,
            "/tenants",
            Some(json!({ "name": "   " })),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_input" }));
}

#[tokio::test]
async fn member_can_read_tenant_and_members() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    let id = app.create_tenant(&cookie, "Alex & Sam").await;
    let tenant = app
        .request(Method::GET, &format!("/tenants/{id}"), None, Some(&cookie))
        .await;
    assert_eq!(tenant.status, StatusCode::OK);
    assert_eq!(
        tenant.body,
        json!({ "id": id.to_string(), "name": "Alex & Sam" })
    );
    let members = app
        .request(
            Method::GET,
            &format!("/tenants/{id}/members"),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(members.status, StatusCode::OK);
    let list = members.body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["email"], "alex@example.com");
    assert_eq!(list[0]["name"], "Alex");
    assert!(list[0]["user_id"].is_string());
}

#[tokio::test]
async fn non_member_gets_404() {
    let app = TestApp::spawn().await;
    let owner = app.verified_user("alex@example.com").await;
    let id = app.create_tenant(&owner, "Alex & Sam").await;
    let other = app.verified_user("eve@example.com").await;

    let missing = app
        .request(
            Method::GET,
            &format!("/tenants/{}", Uuid::now_v7()),
            None,
            Some(&other),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    for path in [format!("/tenants/{id}"), format!("/tenants/{id}/members")] {
        let response = app.request(Method::GET, &path, None, Some(&other)).await;
        assert_eq!(response.status, missing.status, "{path}");
        assert_eq!(response.body, missing.body, "{path}");
    }
    assert_eq!(missing.body, json!({ "error": "not_found" }));
}

#[tokio::test]
async fn invalid_tenant_id_gets_404() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    let response = app
        .request(Method::GET, "/tenants/not-a-uuid", None, Some(&cookie))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.body, json!({ "error": "not_found" }));
}

#[tokio::test]
async fn me_lists_only_own_memberships() {
    let app = TestApp::spawn().await;
    let a = app.verified_user("alex@example.com").await;
    app.create_tenant(&a, "Alex & Sam").await;
    let b = app.verified_user("eve@example.com").await;
    let me = app.request(Method::GET, "/auth/me", None, Some(&b)).await;
    assert_eq!(me.body["memberships"], json!([]));
}

#[tokio::test]
async fn unverified_user_gets_403_on_tenant_routes() {
    let app = TestApp::spawn().await;
    let signup = app
        .request(
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": "alex@example.com", "password": "pw", "name": "Alex" })),
            None,
        )
        .await;
    let cookie = signup.session_cookie().unwrap();
    let response = app
        .request(
            Method::GET,
            &format!("/tenants/{}", Uuid::now_v7()),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.body, json!({ "error": "email_not_verified" }));
}
