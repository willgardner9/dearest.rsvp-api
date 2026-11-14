mod common;

use std::time::Duration;

use common::TestApp;
use dearest::cleanup::{run_cleanup_once, spawn_cleanup};
use sea_orm::{ConnectionTrait, Statement};
use tokio_util::sync::CancellationToken;

async fn count(app: &TestApp, query: &str) -> i64 {
    let row = app
        .db
        .query_one_raw(Statement::from_string(app.db.get_database_backend(), query))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "n").unwrap()
}

async fn sql(app: &TestApp, statement: &str) {
    app.db.execute_unprepared(statement).await.unwrap();
}

#[tokio::test]
async fn cleanup_deletes_expired_sessions_and_keeps_live_ones() {
    let app = TestApp::spawn().await;
    app.verified_user("alex@example.com").await;
    sql(
        &app,
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
         SELECT gen_random_uuid(), id, sha256('expired'::bytea), now() - interval '1 minute' FROM users",
    )
    .await;
    assert_eq!(count(&app, "SELECT count(*) AS n FROM sessions").await, 2);
    let counts = run_cleanup_once(&app.db).await.unwrap();
    assert_eq!(counts.sessions, 1);
    assert_eq!(count(&app, "SELECT count(*) AS n FROM sessions").await, 1);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) AS n FROM sessions WHERE expires_at > now()"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn cleanup_deletes_used_and_expired_email_tokens_and_keeps_open_ones() {
    let app = TestApp::spawn().await;
    // The signup leaves one used token (the verification) after `verified_user`.
    app.verified_user("alex@example.com").await;
    sql(
        &app,
        "INSERT INTO email_tokens (id, user_id, purpose, token_hash, expires_at) \
         SELECT gen_random_uuid(), id, 'reset_password', sha256('expired'::bytea), now() - interval '1 minute' FROM users",
    )
    .await;
    sql(
        &app,
        "INSERT INTO email_tokens (id, user_id, purpose, token_hash, expires_at) \
         SELECT gen_random_uuid(), id, 'reset_password', sha256('open'::bytea), now() + interval '1 hour' FROM users",
    )
    .await;
    assert_eq!(
        count(&app, "SELECT count(*) AS n FROM email_tokens").await,
        3
    );
    let counts = run_cleanup_once(&app.db).await.unwrap();
    assert_eq!(counts.email_tokens, 2);
    assert_eq!(
        count(&app, "SELECT count(*) AS n FROM email_tokens").await,
        1
    );
    assert_eq!(
        count(
            &app,
            "SELECT count(*) AS n FROM email_tokens WHERE used_at IS NULL AND expires_at > now()"
        )
        .await,
        1
    );
}

async fn insert_invite(app: &TestApp, email: &str, expired_days_ago: i32) {
    sql(
        app,
        &format!(
            "INSERT INTO invites (id, tenant_id, email, invited_by, token_hash, expires_at) \
             SELECT gen_random_uuid(), tenant_id, '{email}', user_id, sha256('{email}'::bytea), \
             now() - interval '{expired_days_ago} days' FROM user_tenant_memberships LIMIT 1"
        ),
    )
    .await;
}

#[tokio::test]
async fn cleanup_keeps_invites_expired_less_than_30_days() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    app.create_tenant(&cookie, "Alex & Sam").await;
    insert_invite(&app, "a@example.com", 10).await;
    let counts = run_cleanup_once(&app.db).await.unwrap();
    assert_eq!(counts.invites, 0);
    assert_eq!(count(&app, "SELECT count(*) AS n FROM invites").await, 1);
}

#[tokio::test]
async fn cleanup_deletes_invites_expired_more_than_30_days() {
    let app = TestApp::spawn().await;
    let cookie = app.verified_user("alex@example.com").await;
    app.create_tenant(&cookie, "Alex & Sam").await;
    insert_invite(&app, "a@example.com", 40).await;
    insert_invite(&app, "b@example.com", 10).await;
    let counts = run_cleanup_once(&app.db).await.unwrap();
    assert_eq!(counts.invites, 1);
    assert_eq!(count(&app, "SELECT count(*) AS n FROM invites").await, 1);
}

#[tokio::test]
async fn spawn_cleanup_runs_at_start() {
    let app = TestApp::spawn().await;
    app.verified_user("alex@example.com").await;
    sql(
        &app,
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
         SELECT gen_random_uuid(), id, sha256('expired'::bytea), now() - interval '1 minute' FROM users",
    )
    .await;
    let cancel = CancellationToken::new();
    let handle = spawn_cleanup(app.state.clone(), cancel.clone());
    for _ in 0..100 {
        if count(&app, "SELECT count(*) AS n FROM sessions").await == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(count(&app, "SELECT count(*) AS n FROM sessions").await, 1);
    cancel.cancel();
    handle.await.unwrap();
}

#[tokio::test]
async fn spawn_cleanup_stops_on_cancel() {
    let app = TestApp::spawn().await;
    let cancel = CancellationToken::new();
    let handle = spawn_cleanup(app.state.clone(), cancel.clone());
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .expect("cleanup task stops within 1 second")
        .unwrap();
}
