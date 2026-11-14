use std::{net::SocketAddr, sync::Arc, time::Duration};

use dearest::{AppState, config::Config, email::smtp::SmtpEmailSender};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, Database};
use tokio::{net::TcpListener, signal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load a local .env file if one exists. Ignore the error if it does not.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let config = Config::from_env()?;

    let mut options = ConnectOptions::new(&config.database_url);
    options.max_connections(config.db_max_connections);
    let db = Database::connect(options).await?;
    Migrator::up(&db, None).await?;

    let email = Arc::new(SmtpEmailSender::new(&config.smtp, &config.email_from)?);
    let bind_addr = config.bind_addr;
    let state = AppState::new(db, config, email);

    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!("listening on {bind_addr}");
    axum::serve(
        listener,
        dearest::app(state.clone()).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // Let email sends that are in progress finish before the process ends.
    state.tasks.close();
    if tokio::time::timeout(Duration::from_secs(10), state.tasks.wait())
        .await
        .is_err()
    {
        tracing::warn!("background tasks did not finish within 10 seconds");
    }
    Ok(())
}

/// Wait for Ctrl-C or, on Unix, SIGTERM. `docker stop` sends SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sigterm) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            sigterm.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
