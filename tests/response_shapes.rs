//! Each test asserts the whole JSON body of one route.
//! The tests pin the shape of the JSON, so a change to the response types shows at once.
//! An id and a date have a random value. The helpers check the format and replace the value.
mod common;

use axum::http::{Method, StatusCode};
use chrono::DateTime;
use common::{TestApp, TestResponse};
use dearest::email::templates::extract_token;
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use uuid::Uuid;

const OWNER: &str = "alex@example.com";
const GUEST: &str = "sam@example.com";

/// Check that the string at `pointer` is a UUID v7. Replace it with `"<id>"`.
fn redact_id(body: &mut Value, pointer: &str) -> String {
    let slot = body
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("no value at {pointer}"));
    let text = slot.as_str().expect("id is a string").to_owned();
    let id: Uuid = text.parse().expect("id is a UUID");
    assert_eq!(id.get_version_num(), 7, "id {text} is not a UUID v7");
    *slot = json!("<id>");
    text
}

/// Check that the string at `pointer` is an RFC 3339 date. Replace it with `"<date>"`.
fn redact_date(body: &mut Value, pointer: &str) {
    let slot = body
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("no value at {pointer}"));
    let text = slot.as_str().expect("date is a string");
    DateTime::parse_from_rfc3339(text).expect("date is RFC 3339");
    *slot = json!("<date>");
}

fn me_body(email: &str, name: &str, verified: bool, memberships: Value) -> Value {
    json!({
        "user": { "id": "<id>", "email": email, "name": name, "email_verified": verified },
        "memberships": memberships,
    })
}

async fn signup(app: &TestApp) -> TestResponse {
    app.request_from(
        &app.unique_ip(),
        Method::POST,
        "/auth/signup",
        Some(json!({ "email": OWNER, "password": "pw", "name": "Alex" })),
        None,
    )
    .await
}

async fn invite(app: &TestApp, cookie: &str, tenant: Uuid, email: &str) -> TestResponse {
    app.request(
        Method::POST,
        &format!("/tenants/{tenant}/invites"),
        Some(json!({ "email": email })),
        Some(cookie),
    )
    .await
}

async fn invite_token(app: &TestApp, to: &str) -> String {
    app.settle().await;
    let email = app
        .email
        .sent()
        .into_iter()
        .find(|email| email.to == to && email.subject.contains("invited you"))
        .unwrap_or_else(|| panic!("no invite email to {to}"));
    extract_token(&email)
}

#[tokio::test]
async fn signup_body_has_me_shape() {
    let app = TestApp::spawn().await;
    let mut response = signup(&app).await;
    assert_eq!(response.status, StatusCode::CREATED);
    redact_id(&mut response.body, "/user/id");
    assert_eq!(response.body, me_body(OWNER, "Alex", false, json!([])));
}

#[tokio::test]
async fn login_body_has_me_shape() {
    let app = TestApp::spawn().await;
    signup(&app).await;
    let mut response = app
        .request_from(
            &app.unique_ip(),
            Method::POST,
            "/auth/login",
            Some(json!({ "email": OWNER, "password": "pw" })),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    redact_id(&mut response.body, "/user/id");
    assert_eq!(response.body, me_body(OWNER, "Alex", false, json!([])));
}

#[tokio::test]
async fn me_body_has_me_shape() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    let mut response = app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    redact_id(&mut response.body, "/user/id");
    assert_eq!(
        response.body,
        me_body(
            OWNER,
            "Alex",
            true,
            json!([{ "tenant_id": tenant.to_string(), "tenant_name": "Alex & Sam" }])
        )
    );
}

#[tokio::test]
async fn create_tenant_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let mut response = app
        .request(
            Method::POST,
            "/tenants",
            Some(json!({ "name": "Alex & Sam" })),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
    redact_id(&mut response.body, "/id");
    assert_eq!(response.body, json!({ "id": "<id>", "name": "Alex & Sam" }));
}

#[tokio::test]
async fn get_tenant_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    let response = app
        .request(
            Method::GET,
            &format!("/tenants/{tenant}"),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.body,
        json!({ "id": tenant.to_string(), "name": "Alex & Sam" })
    );
}

#[tokio::test]
async fn list_members_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    let mut response = app
        .request(
            Method::GET,
            &format!("/tenants/{tenant}/members"),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    redact_id(&mut response.body, "/0/user_id");
    assert_eq!(
        response.body,
        json!([{ "user_id": "<id>", "name": "Alex", "email": OWNER }])
    );
}

#[tokio::test]
async fn create_invite_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    let mut response = invite(&app, &cookie, tenant, GUEST).await;
    assert_eq!(response.status, StatusCode::CREATED);
    redact_id(&mut response.body, "/id");
    redact_date(&mut response.body, "/expires_at");
    assert_eq!(
        response.body,
        json!({ "id": "<id>", "email": GUEST, "expires_at": "<date>" })
    );
}

#[tokio::test]
async fn list_invites_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    invite(&app, &cookie, tenant, GUEST).await;
    let mut response = app
        .request(
            Method::GET,
            &format!("/tenants/{tenant}/invites"),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    redact_id(&mut response.body, "/0/id");
    redact_date(&mut response.body, "/0/expires_at");
    assert_eq!(
        response.body,
        json!([{
            "id": "<id>",
            "email": GUEST,
            "invited_by_name": "Alex",
            "expires_at": "<date>",
        }])
    );
}

#[tokio::test]
async fn list_invites_inviter_deleted_gives_null() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    invite(&app, &cookie, tenant, GUEST).await;
    // The foreign key cascades, so a deleted inviter also deletes the invite.
    // Drop the key and point the invite at a user that does not exist.
    app.db
        .execute_unprepared("ALTER TABLE invites DROP CONSTRAINT invites_invited_by_fkey")
        .await
        .unwrap();
    app.db
        .execute_unprepared(&format!(
            "UPDATE invites SET invited_by = '{}'",
            Uuid::new_v4()
        ))
        .await
        .unwrap();
    let mut response = app
        .request(
            Method::GET,
            &format!("/tenants/{tenant}/invites"),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    redact_id(&mut response.body, "/0/id");
    redact_date(&mut response.body, "/0/expires_at");
    assert_eq!(
        response.body,
        json!([{
            "id": "<id>",
            "email": GUEST,
            "invited_by_name": null,
            "expires_at": "<date>",
        }])
    );
}

#[tokio::test]
async fn invite_preview_body() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    invite(&app, &cookie, tenant, GUEST).await;
    let token = invite_token(&app, GUEST).await;
    let response = app
        .request(Method::GET, &format!("/invites/{token}"), None, None)
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.body,
        json!({
            "tenant_id": tenant.to_string(),
            "tenant_name": "Alex & Sam",
            "inviter_name": "Alex",
            "email": GUEST,
            "account_exists": false,
        })
    );
}

#[tokio::test]
async fn accept_invite_body_signed_in() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    invite(&app, &cookie, tenant, GUEST).await;
    let token = invite_token(&app, GUEST).await;
    let guest_cookie = app
        .request_from(
            &app.unique_ip(),
            Method::POST,
            "/auth/signup",
            Some(json!({ "email": GUEST, "password": "pw", "name": "Sam" })),
            None,
        )
        .await
        .session_cookie()
        .unwrap();
    let response = app
        .request(
            Method::POST,
            &format!("/invites/{token}/accept"),
            None,
            Some(&guest_cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, json!({ "tenant_id": tenant.to_string() }));
}

#[tokio::test]
async fn accept_invite_body_new_account() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&cookie, "Alex & Sam").await;
    invite(&app, &cookie, tenant, GUEST).await;
    let token = invite_token(&app, GUEST).await;
    let response = app
        .request(
            Method::POST,
            &format!("/invites/{token}/accept"),
            Some(json!({ "name": "Sam", "password": "pw" })),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, json!({ "tenant_id": tenant.to_string() }));
}
