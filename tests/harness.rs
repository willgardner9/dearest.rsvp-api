mod common;

use sea_orm::{ConnectionTrait, Statement};

#[tokio::test]
async fn spawn_runs_migrations() {
    let app = common::TestApp::spawn().await;
    let row = app
        .db
        .query_one_raw(Statement::from_string(
            app.db.get_database_backend(),
            "SELECT count(*) AS n FROM users",
        ))
        .await
        .unwrap()
        .unwrap();
    let count: i64 = row.try_get("", "n").unwrap();
    assert_eq!(count, 0);
}
