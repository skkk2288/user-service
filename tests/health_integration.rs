//! Integration tests for the `/ping` liveness endpoint.
//!
//! Covers the PRD acceptance criteria (docs/agents/需求编写/prd-draft.md §3):
//!   - `GET /ping` returns 200 with body `{"status":"ok"}`
//!   - `Content-Type` is `application/json`
//!   - Liveness semantics: no auth required, no rate limiting, no DB dependency
//!   - No regression: existing `/api/auth/*` routes keep their behavior
//!
//! The app under test mirrors `src/main.rs` (both `/ping` and auth routes),
//! so the liveness endpoint is exercised through the same router assembly.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use user_service::config::Config;
use user_service::models::{InMemorySessionRepository, InMemoryUserRepository, RateLimiter};
use user_service::services::auth_service::AuthService;

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Build a full app (mirroring `main.rs`): `/ping` + all `/api/auth/*` routes.
fn test_app() -> axum::Router {
    let config = Arc::new(Config {
        session_secret: "test-secret-that-is-long-enough-ok!!".into(),
        cookie_secure: false,
        bcrypt_cost: 4,
        rate_limit_max_failures: 5,
        rate_limit_lockout_minutes: 15,
        session_ttl_short: 7200,
        session_ttl_long: 604_800,
        cors_origin: "http://localhost:5173".into(),
        listen_addr: "0.0.0.0:3000".into(),
    });

    let auth_service = Arc::new(AuthService::new(
        Arc::new(InMemoryUserRepository::new()),
        Arc::new(InMemorySessionRepository::new()),
        Arc::new(RateLimiter::new(5, 15)),
        config,
    ));

    axum::Router::new()
        .route(
            "/ping",
            axum::routing::get(user_service::routes::health::ping),
        )
        .route(
            "/api/register",
            axum::routing::post(user_service::routes::auth::register),
        )
        .route(
            "/api/login",
            axum::routing::post(user_service::routes::auth::login),
        )
        .route(
            "/api/logout",
            axum::routing::post(user_service::routes::auth::logout),
        )
        .route(
            "/api/me",
            axum::routing::get(user_service::routes::auth::me),
        )
        .with_state(auth_service)
}

/// Send a GET request (no auth) and return (status, content-type, body).
async fn get_ping(app: &axum::Router, uri: &str) -> (StatusCode, Option<String>, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        content_type,
        String::from_utf8(body.to_vec()).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// /ping contract tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ping_returns_200_with_status_ok() {
    let app = test_app();
    let (status, _, body) = get_ping(&app, "/ping").await;

    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body).expect("body is valid JSON");
    assert_eq!(value, serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn ping_returns_json_content_type() {
    let app = test_app();
    let (_, content_type, _) = get_ping(&app, "/ping").await;

    let ct = content_type.expect("Content-Type header present");
    assert!(ct.starts_with("application/json"), "got: {ct}");
}

#[tokio::test]
async fn ping_body_is_exact_contract() {
    let app = test_app();
    let (_, _, body) = get_ping(&app, "/ping").await;

    // Exact contract: a JSON object with only the `status` key.
    let value: serde_json::Value = serde_json::from_str(&body).expect("body is valid JSON");
    let obj = value.as_object().expect("body is a JSON object");
    assert_eq!(obj.len(), 1, "no extra keys: {body}");
    assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("ok"));
}

#[tokio::test]
async fn ping_requires_no_auth_or_cookie() {
    let app = test_app();
    // No Cookie / Authorization headers at all — liveness must be open.
    let (status, _, _) = get_ping(&app, "/ping").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn ping_ok_on_repeated_probes() {
    let app = test_app();
    // Health probes hit frequently; every probe must succeed (no rate limiting).
    for _ in 0..5 {
        let (status, _, body) = get_ping(&app, "/ping").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"status":"ok"}"#);
    }
}

// ---------------------------------------------------------------------------
// No-regression checks (PRD acceptance: existing /api/auth/* unaffected)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn me_unauth_still_401_no_regression() {
    let app = test_app();
    let request = Request::builder()
        .method("GET")
        .uri("/api/me")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_wrong_password_still_401_no_regression() {
    let app = test_app();
    let request = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"email":"nobody@example.com","password":"wrong-password"}"#.to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
