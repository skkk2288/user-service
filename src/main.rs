//! user-service entry point.
//!
//! Assembles the Axum router with CORS, wires up the auth service with
//! in-memory repositories, and starts the HTTP server.

mod config;
mod middleware;
mod models;
mod routes;
mod services;
mod utils;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::models::{InMemorySessionRepository, InMemoryUserRepository, RateLimiter};
use crate::services::auth_service::AuthService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load configuration.
    let config = Config::from_env()?;
    tracing::info!(
        "Starting user-service on {} (cookie_secure={})",
        config.listen_addr,
        config.cookie_secure
    );

    // Build shared state.
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let session_repo = Arc::new(InMemorySessionRepository::new());
    let rate_limiter = Arc::new(RateLimiter::new(
        config.rate_limit_max_failures,
        config.rate_limit_lockout_minutes,
    ));
    let auth_service = Arc::new(AuthService::new(
        user_repo,
        session_repo,
        rate_limiter,
        Arc::new(config.clone()),
    ));

    // CORS: allow the configured frontend origin.
    let cors = CorsLayer::new()
        .allow_origin(config.cors_origin.parse::<axum::http::HeaderValue>()?)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(Any)
        .allow_credentials(true);

    // Build router.
    let app = Router::new()
        .route("/api/register", post(routes::auth::register))
        .route("/api/login", post(routes::auth::login))
        .route("/api/logout", post(routes::auth::logout))
        .route("/api/me", get(routes::auth::me))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(auth_service);

    // Start server.
    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!("Listening on {}", config.listen_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
