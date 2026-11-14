mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse};
use dearest::{auth::token::new_token, email::templates::extract_token};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::{Value, json};
use uuid::Uuid;

const OWNER: &str = "alex@example.com";
const GUEST: &str = "sam@example.com";

struct Setup {
    app: TestApp,
    owner: String,
    tenant: Uuid,
}

async fn setup() -> Setup {
    let app = TestApp::spawn().await;
    let owner = app.verified_user(OWNER).await;
    let tenant = app.create_tenant(&owner, "Alex & Sam").await;
    Setup { app, owner, tenant }
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

/// Wait for `count` invite emails to `to`. Return the token in the last one.
async fn invite_token(app: &TestApp, to: &str, count: usize) -> String {
    for _ in 0..100 {
        let emails: Vec<_> = app
            .email
            .sent()
            .into_iter()
            .filter(|email| email.to == to && email.subject.contains("invited you"))
            .collect();
        if emails.len() >= count {
            return extract_token(emails.last().unwrap());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected {count} invite emails to {to}");
}

async fn preview(app: &TestApp, token: &str) -> TestResponse {
    app.request(Method::GET, &format!("/invites/{token}"), None, None)
        .await
}

async fn accept(
    app: &TestApp,
    token: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> TestResponse {
    app.request(
        Method::POST,
        &format!("/invites/{token}/accept"),
        body,
        cookie,
    )
    .await
}

async fn signup_unverified(app: &TestApp, email: &str) -> String {
    app.request_from(
        &app.unique_ip(),
        Method::POST,
        "/auth/signup",
        Some(json!({ "email": email, "password": "pw", "name": "Sam" })),
        None,
    )
    .await
    .session_cookie()
    .unwrap()
}

async fn count(app: &TestApp, query: &str) -> i64 {
    let row = app
        .db
        .query_one_raw(Statement::from_string(app.db.get_database_backend(), query))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "n").unwrap()
}

#[tokio::test]
async fn invite_sends_email_with_link() {
    let s = setup().await;
    let response = invite(&s.app, &s.owner, s.tenant, GUEST).await;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.body["email"], GUEST);
    assert!(response.body["id"].is_string());
    assert!(response.body["expires_at"].is_string());
    for _ in 0..100 {
        if s.app.email.last_to(GUEST).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let email = s.app.email.last_to(GUEST).unwrap();
    assert!(email.text.contains("https://app.test/invite/"));
    assert!(email.subject.contains("Alex"));
    assert!(email.subject.contains("Alex & Sam"));
}

#[tokio::test]
async fn invite_existing_member_returns_409() {
    let s = setup().await;
    let response = invite(&s.app, &s.owner, s.tenant, "ALEX@example.com").await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.body, json!({ "error": "already_member" }));
}

#[tokio::test]
async fn invite_rejects_invalid_email() {
    let s = setup().await;
    let response = invite(&s.app, &s.owner, s.tenant, "nope").await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, json!({ "error": "invalid_input" }));
}

#[tokio::test]
async fn reinvite_replaces_token() {
    let s = setup().await;
    let first = invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let first_token = invite_token(&s.app, GUEST, 1).await;
    let second = invite(&s.app, &s.owner, s.tenant, GUEST).await;
    assert_eq!(second.status, StatusCode::CREATED);
    let second_token = invite_token(&s.app, GUEST, 2).await;
    assert_ne!(first_token, second_token);
    assert_eq!(first.body["id"], second.body["id"]);

    let list = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", s.tenant),
            None,
            Some(&s.owner),
        )
        .await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(list.body[0]["email"], GUEST);
    assert_eq!(list.body[0]["invited_by_name"], "Alex");
    assert_eq!(
        preview(&s.app, &first_token).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(preview(&s.app, &second_token).await.status, StatusCode::OK);
}

#[tokio::test]
async fn preview_shows_tenant_and_inviter() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let response = preview(&s.app, &token).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.body,
        json!({ "tenant_name": "Alex & Sam", "inviter_name": "Alex", "email": GUEST, "account_exists": false })
    );
    signup_unverified(&s.app, GUEST).await;
    let after = preview(&s.app, &token).await;
    assert_eq!(after.body["account_exists"], true);
}

#[tokio::test]
async fn preview_of_unknown_token_returns_404() {
    let s = setup().await;
    let response = preview(&s.app, "nothing").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.body, json!({ "error": "not_found" }));
}

#[tokio::test]
async fn new_user_accepts_with_password() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let response = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "pw" })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["tenant_id"], s.tenant.to_string());
    let cookie = response.session_cookie().expect("session cookie");
    let me = s
        .app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(me.body["user"]["email_verified"], true);
    assert_eq!(me.body["user"]["name"], "Sam");
    assert_eq!(me.body["memberships"][0]["tenant_id"], s.tenant.to_string());
}

#[tokio::test]
async fn new_user_accept_needs_name_and_password() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let none = accept(&s.app, &token, None, None).await;
    assert_eq!(none.status, StatusCode::BAD_REQUEST);
    let empty = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "" })),
        None,
    )
    .await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    // The failed tries do not use the invite.
    assert_eq!(preview(&s.app, &token).await.status, StatusCode::OK);
}

#[tokio::test]
async fn accept_without_session_for_existing_account_returns_401() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    signup_unverified(&s.app, GUEST).await;
    let response = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "new" })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body, json!({ "error": "unauthenticated" }));
}

#[tokio::test]
async fn existing_user_accepts_when_logged_in() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let cookie = signup_unverified(&s.app, GUEST).await;
    let response = accept(&s.app, &token, None, Some(&cookie)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["tenant_id"], s.tenant.to_string());
    let me = s
        .app
        .request(Method::GET, "/auth/me", None, Some(&cookie))
        .await;
    assert_eq!(me.body["user"]["email_verified"], true);
    assert_eq!(me.body["memberships"][0]["tenant_name"], "Alex & Sam");
}

#[tokio::test]
async fn accept_with_wrong_account_returns_403() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let eve = s.app.verified_user("eve@example.com").await;
    let response = accept(&s.app, &token, None, Some(&eve)).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.body, json!({ "error": "invite_email_mismatch" }));
    assert_eq!(
        count(&s.app, "SELECT count(*) AS n FROM user_tenant_memberships").await,
        1
    );
    // The invite stays open for the right person.
    assert_eq!(preview(&s.app, &token).await.status, StatusCode::OK);
}

#[tokio::test]
async fn accept_when_already_member_returns_409() {
    let s = setup().await;
    let sam = s.app.verified_user(GUEST).await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let first = invite_token(&s.app, GUEST, 1).await;
    assert_eq!(
        accept(&s.app, &first, None, Some(&sam)).await.status,
        StatusCode::OK
    );

    // A second invite for the same tenant, made with a direct insert.
    let (plain, hash) = new_token();
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    s.app
        .db
        .execute_unprepared(&format!(
            "INSERT INTO invites (id, tenant_id, email, invited_by, token_hash, expires_at) \
             SELECT gen_random_uuid(), tenant_id, '{GUEST}', user_id, decode('{hex}', 'hex'), now() + interval '7 days' \
             FROM user_tenant_memberships LIMIT 1"
        ))
        .await
        .unwrap();
    let response = accept(&s.app, &plain, None, Some(&sam)).await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.body, json!({ "error": "already_member" }));
    assert_eq!(
        count(
            &s.app,
            &format!(
                "SELECT count(*) AS n FROM user_tenant_memberships WHERE tenant_id = '{}'",
                s.tenant
            )
        )
        .await,
        2
    );
}

#[tokio::test]
async fn accepted_invite_cannot_be_used_again() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let first = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "pw" })),
        None,
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(preview(&s.app, &token).await.status, StatusCode::NOT_FOUND);
    let again = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "pw" })),
        None,
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancelled_invite_returns_404() {
    let s = setup().await;
    let created = invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    let id = created.body["id"].as_str().unwrap();
    let cancel = s
        .app
        .request(
            Method::DELETE,
            &format!("/tenants/{}/invites/{id}", s.tenant),
            None,
            Some(&s.owner),
        )
        .await;
    assert_eq!(cancel.status, StatusCode::NO_CONTENT);
    assert_eq!(preview(&s.app, &token).await.status, StatusCode::NOT_FOUND);
    let accept_response = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "pw" })),
        None,
    )
    .await;
    assert_eq!(accept_response.status, StatusCode::NOT_FOUND);
    let list = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", s.tenant),
            None,
            Some(&s.owner),
        )
        .await;
    assert_eq!(list.body, json!([]));
    // A new invite for the same email works after a cancel.
    assert_eq!(
        invite(&s.app, &s.owner, s.tenant, GUEST).await.status,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn expired_invite_returns_404() {
    let s = setup().await;
    invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let token = invite_token(&s.app, GUEST, 1).await;
    s.app
        .db
        .execute_unprepared("UPDATE invites SET expires_at = now() - interval '1 minute'")
        .await
        .unwrap();
    assert_eq!(preview(&s.app, &token).await.status, StatusCode::NOT_FOUND);
    let response = accept(
        &s.app,
        &token,
        Some(json!({ "name": "Sam", "password": "pw" })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    let list = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", s.tenant),
            None,
            Some(&s.owner),
        )
        .await;
    assert_eq!(list.body, json!([]));
    // An expired invite can be sent again.
    assert_eq!(
        invite(&s.app, &s.owner, s.tenant, GUEST).await.status,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn cancel_invite_of_other_tenant_returns_404() {
    let s = setup().await;
    let created = invite(&s.app, &s.owner, s.tenant, GUEST).await;
    let id = created.body["id"].as_str().unwrap();
    let eve = s.app.verified_user("eve@example.com").await;
    let eve_tenant = s.app.create_tenant(&eve, "Eve's").await;
    let response = s
        .app
        .request(
            Method::DELETE,
            &format!("/tenants/{eve_tenant}/invites/{id}"),
            None,
            Some(&eve),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    // The invite is still open.
    let list = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", s.tenant),
            None,
            Some(&s.owner),
        )
        .await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn invite_matches_email_case_insensitively() {
    let s = setup().await;
    let response = invite(&s.app, &s.owner, s.tenant, "Sam@Example.com").await;
    assert_eq!(response.body["email"], GUEST);
    let token = invite_token(&s.app, GUEST, 1).await;
    let sam = s.app.verified_user("sam@example.com").await;
    let accepted = accept(&s.app, &token, None, Some(&sam)).await;
    assert_eq!(accepted.status, StatusCode::OK);
}

#[tokio::test]
async fn non_member_cannot_list_invites() {
    let s = setup().await;
    let eve = s.app.verified_user("eve@example.com").await;
    let response = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", s.tenant),
            None,
            Some(&eve),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    let create = invite(&s.app, &eve, s.tenant, GUEST).await;
    assert_eq!(create.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unverified_user_gets_403_on_invite_routes() {
    let s = setup().await;
    let cookie = signup_unverified(&s.app, "eve@example.com").await;
    let response = s
        .app
        .request(
            Method::GET,
            &format!("/tenants/{}/invites", Uuid::now_v7()),
            None,
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.body, json!({ "error": "email_not_verified" }));
}

#[tokio::test]
async fn invite_is_limited_per_user() {
    let s = setup().await;
    for i in 0..20 {
        let response = invite(&s.app, &s.owner, s.tenant, &format!("g{i}@example.com")).await;
        assert_eq!(response.status, StatusCode::CREATED, "invite {i}");
    }
    let last = invite(&s.app, &s.owner, s.tenant, "g21@example.com").await;
    assert_eq!(last.status, StatusCode::TOO_MANY_REQUESTS);
}
