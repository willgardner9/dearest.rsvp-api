use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use sea_orm::{ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    AppState,
    entities::{email_tokens, invites, sessions},
    error::AppResult,
};

const CLEANUP_PERIOD: Duration = Duration::from_secs(3600);
const KEEP_EXPIRED_INVITES_DAYS: i64 = 30;

/// The number of rows that one cleanup run deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupCounts {
    pub sessions: u64,
    pub email_tokens: u64,
    pub invites: u64,
}

/// Delete old rows. The token checks also compare `expires_at` with the current
/// time, so the API does not need these deletes to be correct. They keep the tables small.
pub async fn run_cleanup_once(db: &DatabaseConnection) -> AppResult<CleanupCounts> {
    let now = Utc::now();
    let sessions = sessions::Entity::delete_many()
        .filter(sessions::Column::ExpiresAt.lt(now))
        .exec(db)
        .await?
        .rows_affected;
    let email_tokens = email_tokens::Entity::delete_many()
        .filter(
            Condition::any()
                .add(email_tokens::Column::ExpiresAt.lt(now))
                .add(email_tokens::Column::UsedAt.is_not_null()),
        )
        .exec(db)
        .await?
        .rows_affected;
    let invites = invites::Entity::delete_many()
        .filter(
            invites::Column::ExpiresAt.lt(now - ChronoDuration::days(KEEP_EXPIRED_INVITES_DAYS)),
        )
        .exec(db)
        .await?
        .rows_affected;
    Ok(CleanupCounts {
        sessions,
        email_tokens,
        invites,
    })
}

/// Run the cleanup every hour. The first run starts at once. The task ends when `cancel` fires.
pub fn spawn_cleanup(state: AppState, cancel: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_PERIOD);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    match run_cleanup_once(&state.db).await {
                        Ok(counts) => tracing::info!(
                            sessions = counts.sessions,
                            email_tokens = counts.email_tokens,
                            invites = counts.invites,
                            "cleanup done"
                        ),
                        Err(err) => tracing::error!("cleanup failed: {err:?}"),
                    }
                    state.limits.retain_recent();
                }
            }
        }
    })
}
