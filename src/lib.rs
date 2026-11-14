pub mod auth;
pub mod config;
pub mod email;
pub mod entities;
pub mod error;
pub mod json;
pub mod rate_limit;
pub mod tenants;

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::{Method, StatusCode, header},
    middleware,
    routing::get,
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{Value, json};
use tokio_util::task::TaskTracker;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{config::Config, email::EmailSender, rate_limit::RateLimits};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<Config>,
    pub email: Arc<dyn EmailSender>,
    /// Tracks background tasks, for example email sends. `main` waits for them on shutdown.
    pub tasks: TaskTracker,
    pub limits: Arc<RateLimits>,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: Config, email: Arc<dyn EmailSender>) -> Self {
        Self {
            db,
            config: Arc::new(config),
            email,
            tasks: TaskTracker::new(),
            limits: Arc::new(RateLimits::new()),
        }
    }
}

/// Build the router. Only a browser on one of `cors_allowed_origins` may call the API.
pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(state.config.cors_allowed_origins.clone())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .allow_credentials(true)
        .max_age(Duration::from_secs(3600));

    Router::new()
        .route("/health", get(health))
        .nest("/auth", auth::router())
        .nest("/tenants", tenants::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::session::load_session,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::origin::check_origin,
        ))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        // CORS is the outer layer, so it answers a preflight request first.
        .layer(cors)
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match state.db.execute_unprepared("SELECT 1").await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "db": "up" }))),
        Err(err) => {
            tracing::error!("health check db error: {err}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "db": "down" })),
            )
        }
    }
}
