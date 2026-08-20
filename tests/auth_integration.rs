//! Integration tests for the user-service auth endpoints.
//!
//! These tests exercise the full HTTP request/response cycle through Axum's
//! test utilities, covering the PRD acceptance criteria:
//!   - Registration (email format, password strength, uniqueness, auto-login)
//!   - Login (valid, wrong password, nonexistent email, remember-me TTL)
//!   - Rate limiting (5 failures → 429 lockout)
//!   - Logout (session cleared, cookie cleared)
//!   - /api/me (authenticated + unauthenticated)
//!
//! The integration tests use a low bcrypt cost (4) for speed.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use user_service::config::Config;
use user_service::models::{
    InMemorySessionRepository, InMemoryUserRepository, IpRateLimiter, RateLimiter,
};
use user_service::services::auth_service::AuthService;

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Build a test app with fresh in-memory repos and low bcrypt cost.
fn test_app() -> axum::Router {
    let config = Arc::new(Config {
        session_secret: "test-secret-that-is-long-enough-ok!!".into(),
        cookie_secure: false,
        bcrypt_cost: 4,
        rate_limit_max_failures: 5,
        rate_limit_lockout_minutes: 15,
        rate_limit_ip_per_minute: 20,
        session_ttl_short: 7200,
        session_ttl_long: 604_800,
        cors_origin: "http://localhost:5173".into(),
        listen_addr: "0.0.0.0:3000".into(),
    });

    let auth_service = Arc::new(AuthService::new(
        Arc::new(InMemoryUserRepository::new()),
        Arc::new(InMemorySessionRepository::new()),
        Arc::new(RateLimiter::new(5, 15)),
        Arc::new(IpRateLimiter::new(20)),
        Arc::new(IpRateLimiter::new(20)),
        config,
    ));

    axum::Router::new()
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
        .route(
            "/api/me/profile",
            axum::routing::put(user_service::routes::auth::update_profile),
        )
        .route(
            "/api/me/password",
            axum::routing::put(user_service::routes::auth::change_password),
        )
        .with_state(auth_service)
}

/// Send a POST request with a JSON body to the app and return (status, body, set_cookie).
async fn post_json(
    app: &axum::Router,
    path: &str,
    body: &str,
    cookie: Option<&str>,
) -> (StatusCode, String, Option<String>) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    let request = builder.body(Body::from(body.to_string())).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        String::from_utf8(body.to_vec()).unwrap_or_default(),
        set_cookie,
    )
}

/// Send a GET request to the app and return (status, body).
async fn get_json(app: &axum::Router, path: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    let request = builder.body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap_or_default())
}

/// Extract the `sid=...` value from a Set-Cookie header.
fn extract_sid(set_cookie: &str) -> String {
    let sid_part = set_cookie.split(';').next().unwrap_or("").trim();
    sid_part.strip_prefix("sid=").unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// Registration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_register_success_returns_user_and_cookie() {
    let app = test_app();
    let body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (status, body, set_cookie) = post_json(&app, "/api/register", body, None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"user_id\""));
    assert!(body.contains("\"test@example.com\""));
    assert!(!body.contains("password"));
    assert!(set_cookie.is_some());
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("sid="));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Path=/"));
    assert!(cookie.contains("Max-Age=7200"));
}

#[tokio::test]
async fn it_register_auto_login_session_valid() {
    // Registration should auto-login: the Set-Cookie should work for /api/me.
    let app = test_app();
    let body = r#"{"email":"newuser@example.com","password":"Str0ng!Pass"}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);

    let cookie = set_cookie.unwrap();
    let sid = extract_sid(&cookie);
    let cookie_header = format!("sid={}", sid);

    let (me_status, me_body) = get_json(&app, "/api/me", Some(&cookie_header)).await;
    assert_eq!(me_status, StatusCode::OK);
    assert!(me_body.contains("newuser@example.com"));
}

#[tokio::test]
async fn it_register_invalid_email_no_at() {
    let app = test_app();
    let body = r#"{"email":"not-an-email","password":"Str0ng!Pass"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_email"));
}

#[tokio::test]
async fn it_register_invalid_email_no_domain() {
    let app = test_app();
    let body = r#"{"email":"user@","password":"Str0ng!Pass"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_email"));
}

#[tokio::test]
async fn it_register_invalid_email_short_tld() {
    let app = test_app();
    let body = r#"{"email":"user@example.c","password":"Str0ng!Pass"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_email"));
}

#[tokio::test]
async fn it_register_weak_password_too_short() {
    let app = test_app();
    let body = r#"{"email":"test@example.com","password":"Ab1!"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("weak_password"));
}

#[tokio::test]
async fn it_register_weak_password_too_long() {
    let app = test_app();
    let long_pw = "a".repeat(65);
    let body = format!(r#"{{"email":"test@example.com","password":"{}"}}"#, long_pw);
    let (status, body, _) = post_json(&app, "/api/register", &body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("weak_password"));
}

#[tokio::test]
async fn it_register_weak_password_only_two_categories() {
    let app = test_app();
    // lower + digit only (2 categories, need 3)
    let body = r#"{"email":"test@example.com","password":"abcdefg1"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("weak_password"));
}

#[tokio::test]
async fn it_register_weak_password_three_categories_ok() {
    let app = test_app();
    // upper + lower + digit (3 categories, no special) = valid
    let body = r#"{"email":"test@example.com","password":"Abcdefg1"}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(set_cookie.is_some());
}

#[tokio::test]
async fn it_register_duplicate_email_case_insensitive() {
    let app = test_app();
    let body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (status, _, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);

    let body = r#"{"email":"TEST@example.com","password":"Str0ng!Pass"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("email_already_exists"));
}

#[tokio::test]
async fn it_register_no_password_in_response() {
    let app = test_app();
    let body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("password_hash"));
    assert!(!body.contains("Str0ng!Pass"));
}

// ---------------------------------------------------------------------------
// Login tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_login_success_returns_user_and_cookie() {
    let app = test_app();
    // Register first
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // Login
    let login_body = r#"{"email":"test@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, body, set_cookie) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"user_id\""));
    assert!(body.contains("test@example.com"));
    assert!(set_cookie.is_some());
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("Max-Age=7200")); // 2h TTL
}

#[tokio::test]
async fn it_login_remember_me_true_7day_ttl() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    let login_body = r#"{"email":"test@example.com","password":"Str0ng!Pass","remember_me":true}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::OK);
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("Max-Age=604800")); // 7d TTL
}

#[tokio::test]
async fn it_login_remember_me_false_2h_ttl() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    let login_body = r#"{"email":"test@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::OK);
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("Max-Age=7200")); // 2h TTL
}

#[tokio::test]
async fn it_login_remember_me_default_is_false() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // No remember_me field → defaults to false → 2h TTL
    let login_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::OK);
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("Max-Age=7200"));
}

#[tokio::test]
async fn it_login_wrong_password_returns_401() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    let login_body = r#"{"email":"test@example.com","password":"WrongPass!1","remember_me":false}"#;
    let (status, body, set_cookie) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("invalid_credentials"));
    assert!(set_cookie.is_none());
}

#[tokio::test]
async fn it_login_nonexistent_user_returns_401_same_error() {
    let app = test_app();

    let login_body =
        r#"{"email":"nobody@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, body, _) = post_json(&app, "/api/login", login_body, None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("invalid_credentials"));
    // Error message should be identical to wrong-password case (anti-enumeration)
    assert!(body.contains("邮箱或密码错误"));
}

#[tokio::test]
async fn it_login_wrong_password_and_nonexistent_same_error() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // Wrong password
    let login_body = r#"{"email":"test@example.com","password":"wrong","remember_me":false}"#;
    let (_, body_wrong, _) = post_json(&app, "/api/login", login_body, None).await;

    // Nonexistent user
    let login_body = r#"{"email":"nobody@example.com","password":"wrong","remember_me":false}"#;
    let (_, body_nonexistent, _) = post_json(&app, "/api/login", login_body, None).await;

    // Both should return the same error code and message (anti-enumeration)
    assert_eq!(body_wrong, body_nonexistent);
}

#[tokio::test]
async fn it_login_success_creates_valid_session() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    let login_body = r#"{"email":"test@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/login", login_body, None).await;
    assert_eq!(status, StatusCode::OK);

    let cookie = set_cookie.unwrap();
    let sid = extract_sid(&cookie);
    let cookie_header = format!("sid={}", sid);

    let (me_status, me_body) = get_json(&app, "/api/me", Some(&cookie_header)).await;
    assert_eq!(me_status, StatusCode::OK);
    assert!(me_body.contains("test@example.com"));
}

// ---------------------------------------------------------------------------
// Rate limiting tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_login_rate_limit_lockout_after_5_failures() {
    let app = test_app();
    let reg_body = r#"{"email":"locked@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // 5 failed attempts
    let login_body = r#"{"email":"locked@example.com","password":"wrong","remember_me":false}"#;
    for _ in 0..5 {
        let (status, _, _) = post_json(&app, "/api/login", login_body, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // 6th attempt - even correct password should be locked out
    let correct_body =
        r#"{"email":"locked@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, body, _) = post_json(&app, "/api/login", correct_body, None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("too_many_attempts"));
}

#[tokio::test]
async fn it_login_rate_limit_allows_correct_before_threshold() {
    let app = test_app();
    let reg_body = r#"{"email":"ok@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // 4 failed attempts (below threshold of 5)
    let login_body = r#"{"email":"ok@example.com","password":"wrong","remember_me":false}"#;
    for _ in 0..4 {
        let (status, _, _) = post_json(&app, "/api/login", login_body, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // 5th attempt with correct password should succeed and reset counter
    let correct_body = r#"{"email":"ok@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (status, _, _) = post_json(&app, "/api/login", correct_body, None).await;
    assert_eq!(status, StatusCode::OK);

    // After success, 4 more fails should NOT lock (counter was reset)
    for _ in 0..4 {
        let (status, _, _) = post_json(&app, "/api/login", login_body, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    // 5th fail after reset should still be allowed (only 4 fails)
    let (status, _, _) = post_json(&app, "/api/login", login_body, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn it_login_rate_limit_is_per_email() {
    let app = test_app();
    let reg_body = r#"{"email":"user1@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // Lock out user1
    let login_body = r#"{"email":"user1@example.com","password":"wrong","remember_me":false}"#;
    for _ in 0..5 {
        let _ = post_json(&app, "/api/login", login_body, None).await;
    }

    // user2 (nonexistent) should still be able to attempt (not locked)
    let login_body2 = r#"{"email":"user2@example.com","password":"wrong","remember_me":false}"#;
    let (status, _, _) = post_json(&app, "/api/login", login_body2, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED); // not 429
}

// ---------------------------------------------------------------------------
// Logout tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_logout_success_clears_session() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (_, _, set_cookie) = post_json(&app, "/api/register", reg_body, None).await;
    let sid = extract_sid(&set_cookie.unwrap());
    let cookie_header = format!("sid={}", sid);

    // Logout
    let (status, body, logout_cookie) =
        post_json(&app, "/api/logout", "", Some(&cookie_header)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("已登出"));
    let lc = logout_cookie.unwrap();
    assert!(lc.contains("sid=;"));
    assert!(lc.contains("Max-Age=0"));

    // After logout, /api/me should return 401
    let (me_status, _) = get_json(&app, "/api/me", Some(&cookie_header)).await;
    assert_eq!(me_status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn it_logout_without_session_returns_401() {
    let app = test_app();
    let (status, body, _) = post_json(&app, "/api/logout", "", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("unauthenticated"));
}

#[tokio::test]
async fn it_logout_with_invalid_cookie_returns_401() {
    let app = test_app();
    let (status, _, _) = post_json(&app, "/api/logout", "", Some("sid=invalid.nohmac")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn it_logout_twice_second_fails() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (_, _, set_cookie) = post_json(&app, "/api/register", reg_body, None).await;
    let sid = extract_sid(&set_cookie.unwrap());
    let cookie_header = format!("sid={}", sid);

    // First logout succeeds
    let (status, _, _) = post_json(&app, "/api/logout", "", Some(&cookie_header)).await;
    assert_eq!(status, StatusCode::OK);

    // Second logout fails (session already deleted)
    let (status, _, _) = post_json(&app, "/api/logout", "", Some(&cookie_header)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// /api/me tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_me_without_cookie_returns_401() {
    let app = test_app();
    let (status, body) = get_json(&app, "/api/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("unauthenticated"));
}

#[tokio::test]
async fn it_me_with_invalid_cookie_returns_401() {
    let app = test_app();
    let (status, body) = get_json(&app, "/api/me", Some("sid=bogus.nohmac")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("unauthenticated"));
}

#[tokio::test]
async fn it_me_with_tampered_cookie_returns_401() {
    let app = test_app();
    let reg_body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (_, _, set_cookie) = post_json(&app, "/api/register", reg_body, None).await;
    let sid = extract_sid(&set_cookie.unwrap());

    // Tamper with the HMAC signature
    let mut tampered = sid.clone();
    let last = tampered.chars().last().unwrap();
    let replacement = if last == 'a' { 'b' } else { 'a' };
    tampered.pop();
    tampered.push(replacement);

    let cookie_header = format!("sid={}", tampered);
    let (status, body) = get_json(&app, "/api/me", Some(&cookie_header)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("unauthenticated"));
}

#[tokio::test]
async fn it_me_after_login_returns_correct_user() {
    let app = test_app();
    let reg_body = r#"{"email":"me@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    let login_body = r#"{"email":"me@example.com","password":"Str0ng!Pass","remember_me":true}"#;
    let (status, _, set_cookie) = post_json(&app, "/api/login", login_body, None).await;
    assert_eq!(status, StatusCode::OK);

    let sid = extract_sid(&set_cookie.unwrap());
    let cookie_header = format!("sid={}", sid);

    let (me_status, me_body) = get_json(&app, "/api/me", Some(&cookie_header)).await;
    assert_eq!(me_status, StatusCode::OK);
    assert!(me_body.contains("me@example.com"));
    assert!(me_body.contains("\"user_id\""));
    assert!(!me_body.contains("password"));
}

// ---------------------------------------------------------------------------
// Edge case tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_register_missing_fields_returns_422_or_400() {
    let app = test_app();
    // Missing password field entirely - Axum returns 422 for deserialization failure
    let body = r#"{"email":"test@example.com"}"#;
    let (status, _, _) = post_json(&app, "/api/register", body, None).await;
    assert!(status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn it_register_empty_body_returns_422_or_400() {
    let app = test_app();
    let body = r#"{}"#;
    let (status, _, _) = post_json(&app, "/api/register", body, None).await;
    assert!(status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn it_login_missing_fields_returns_422_or_400() {
    let app = test_app();
    let body = r#"{"email":"test@example.com"}"#;
    let (status, _, _) = post_json(&app, "/api/login", body, None).await;
    assert!(status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn it_register_email_normalized_to_lowercase() {
    let app = test_app();
    // Register with mixed case
    let body = r#"{"email":"Mixed.Case@Example.COM","password":"Str0ng!Pass"}"#;
    let (status, reg_body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);
    // Email should be normalized to lowercase in the response
    assert!(reg_body.contains("mixed.case@example.com"));

    // Login with lowercase should work
    let login_body =
        r#"{"email":"mixed.case@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (login_status, _, _) = post_json(&app, "/api/login", login_body, None).await;
    assert_eq!(login_status, StatusCode::OK);
}

#[tokio::test]
async fn it_multiple_sessions_for_same_user() {
    let app = test_app();
    let reg_body = r#"{"email":"multi@example.com","password":"Str0ng!Pass"}"#;
    let _ = post_json(&app, "/api/register", reg_body, None).await;

    // Login twice to get two different sessions
    let login_body =
        r#"{"email":"multi@example.com","password":"Str0ng!Pass","remember_me":false}"#;
    let (_, _, cookie1) = post_json(&app, "/api/login", login_body, None).await;
    let (_, _, cookie2) = post_json(&app, "/api/login", login_body, None).await;

    let sid1 = extract_sid(&cookie1.unwrap());
    let sid2 = extract_sid(&cookie2.unwrap());

    // Both sessions should be different
    assert_ne!(sid1, sid2);

    // Both should work for /api/me
    let (s1, _) = get_json(&app, "/api/me", Some(&format!("sid={}", sid1))).await;
    assert_eq!(s1, StatusCode::OK);

    let (s2, _) = get_json(&app, "/api/me", Some(&format!("sid={}", sid2))).await;
    assert_eq!(s2, StatusCode::OK);

    // Logout session 1, session 2 should still work
    let _ = post_json(&app, "/api/logout", "", Some(&format!("sid={}", sid1))).await;
    let (s1_after, _) = get_json(&app, "/api/me", Some(&format!("sid={}", sid1))).await;
    assert_eq!(s1_after, StatusCode::UNAUTHORIZED);

    let (s2_after, _) = get_json(&app, "/api/me", Some(&format!("sid={}", sid2))).await;
    assert_eq!(s2_after, StatusCode::OK);
}

#[tokio::test]
async fn it_cookie_has_httponly_and_samesite() {
    let app = test_app();
    let body = r#"{"email":"test@example.com","password":"Str0ng!Pass"}"#;
    let (_, _, set_cookie) = post_json(&app, "/api/register", body, None).await;
    let cookie = set_cookie.unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
}

// ---------------------------------------------------------------------------
// Password strength boundary tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_password_exactly_8_chars_valid() {
    let app = test_app();
    // 8 chars, upper + lower + digit (3 categories) = valid
    let body = r#"{"email":"test8@example.com","password":"Abcdefg1"}"#;
    let (status, _, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn it_password_exactly_64_chars_valid() {
    let app = test_app();
    // 64 chars, upper + lower + digit + special = valid
    let pw = format!("A{}1!", "a".repeat(61));
    assert_eq!(pw.len(), 64);
    let body = format!(r#"{{"email":"test64@example.com","password":"{}"}}"#, pw);
    let (status, _, _) = post_json(&app, "/api/register", &body, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn it_password_7_chars_invalid() {
    let app = test_app();
    // 7 chars, upper + lower + digit + special = categories ok but length fails
    let body = r#"{"email":"test7@example.com","password":"Ab1!xyz"}"#;
    let (status, body, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("weak_password"));
}

#[tokio::test]
async fn it_password_65_chars_invalid() {
    let app = test_app();
    let pw = format!("A{}1!", "a".repeat(62)); // 1+62+1+1 = 65
    assert_eq!(pw.len(), 65);
    let body = format!(r#"{{"email":"test65@example.com","password":"{}"}}"#, pw);
    let (status, body, _) = post_json(&app, "/api/register", &body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("weak_password"));
}

#[tokio::test]
async fn it_password_all_four_categories_valid() {
    let app = test_app();
    let body = r#"{"email":"test4cat@example.com","password":"Str0ng!Pass"}"#;
    let (status, _, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Frontend password utility parity test (logic-level, no DOM)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_password_strength_rules_match_contract() {
    use user_service::utils::password::check_password;

    // Valid: 4 categories, 8+ chars
    let r = check_password("Str0ng!Pass");
    assert!(r.valid);
    assert_eq!(r.categories_met, 4);

    // Valid: 3 categories (upper, lower, digit), 8+ chars
    let r = check_password("Abcdefg1");
    assert!(r.valid);
    assert_eq!(r.categories_met, 3);

    // Invalid: too short
    let r = check_password("Ab1!");
    assert!(!r.valid);
    assert!(!r.length);

    // Invalid: too long (65 chars)
    let r = check_password(&"a".repeat(65));
    assert!(!r.valid);
    assert!(!r.length);

    // Invalid: only 2 categories
    let r = check_password("abcdefg1");
    assert!(!r.valid);
    assert_eq!(r.categories_met, 2);

    // Invalid: only 1 category
    let r = check_password("abcdefgh");
    assert!(!r.valid);
    assert_eq!(r.categories_met, 1);

    // Valid: exactly 8 chars, 3 categories
    let r = check_password("Abcdefg1");
    assert!(r.valid);
    assert!(r.length);

    // Valid: exactly 64 chars, 4 categories
    let pw = format!("A{}1!", "a".repeat(61));
    assert_eq!(pw.chars().count(), 64);
    let r = check_password(&pw);
    assert!(r.valid);
    assert!(r.length);
    assert_eq!(r.categories_met, 4);
}
