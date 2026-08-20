//! Integration tests for the profile-management feature (个人资料管理).
//!
//! Covers the API contract additions (docs/agents/架构师/api-contract.md) and
//! the feature round flagged by the architect (frontend PR #9 + backend PR #10):
//!   - PUT /api/me/profile  — partial update (absent = unchanged, "" = clear /
//!     nickname resets to email prefix)
//!   - PUT /api/me/password — change password (wrong old → 400
//!     invalid_old_password; weak new → weak_password)
//!   - GET /api/me          — extended fields nickname / phone / avatar
//!   - register optional nickname + IP rate limit (20/min → 429)
//!
//! These tests exercise the full HTTP request/response cycle through Axum's
//! test utilities (mirroring `src/main.rs` route assembly). bcrypt cost is
//! lowered to 4 for speed.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use user_service::config::Config;
use user_service::models::{
    InMemorySessionRepository, InMemoryUserRepository, IpRateLimiter, RateLimiter,
};
use user_service::services::auth_service::AuthService;

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Build a test app mirroring `src/main.rs` route assembly.
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

/// Send a request with a given method/body/cookie; returns (status, body).
async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<&str>,
    cookie: Option<&str>,
    xff: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    if let Some(ip) = xff {
        builder = builder.header("x-forwarded-for", ip);
    }
    let body_bytes = body.unwrap_or("").to_string();
    let request = builder.body(Body::from(body_bytes)).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap_or_default())
}

/// Register a fresh user; returns the `sid` cookie value.
async fn register_user(app: &axum::Router, email: &str, pw: &str) -> String {
    let body = format!(r#"{{"email":"{email}","password":"{pw}"}}"#);
    let (status, _, set_cookie) = post_json(app, "/api/register", &body, None).await;
    assert_eq!(status, StatusCode::OK, "register {email} should succeed");
    let sid = extract_sid(&set_cookie.expect("Set-Cookie on register"));
    format!("sid={}", sid)
}

/// POST with a JSON body; returns (status, body, set_cookie).
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

/// Extract the `sid=...` value from a Set-Cookie header.
fn extract_sid(set_cookie: &str) -> String {
    let sid_part = set_cookie.split(';').next().unwrap_or("").trim();
    sid_part.strip_prefix("sid=").unwrap_or("").to_string()
}

/// Parse a response body into a JSON value (panics if not valid JSON).
fn as_json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("invalid JSON ({e}): {body}"))
}

// ---------------------------------------------------------------------------
// PUT /api/me/profile — partial update semantics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_profile_partial_update_phone_keeps_nickname() {
    let app = test_app();
    let cookie = register_user(&app, "alice@example.com", "Str0ng!Pass").await;

    // Update phone only; nickname/avatar untouched (absent = unchanged).
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"phone":"13800138000"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["email"], "alice@example.com");
    assert_eq!(v["nickname"], "alice"); // unchanged
    assert_eq!(v["phone"], "13800138000");
    assert_eq!(v["avatar"], Value::Null); // still absent
}

#[tokio::test]
async fn it_profile_partial_update_avatar_keeps_others() {
    let app = test_app();
    let cookie = register_user(&app, "bob@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"avatar":"https://example.com/b.png"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["nickname"], "bob");
    assert_eq!(v["phone"], Value::Null);
    assert_eq!(v["avatar"], "https://example.com/b.png");
}

#[tokio::test]
async fn it_profile_partial_update_nickname_only() {
    let app = test_app();
    let cookie = register_user(&app, "carol@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"Carol"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["nickname"], "Carol");
    assert_eq!(v["phone"], Value::Null);
    assert_eq!(v["avatar"], Value::Null);
}

#[tokio::test]
async fn it_profile_update_all_fields() {
    let app = test_app();
    let cookie = register_user(&app, "dave@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"Dave","phone":"13900139000","avatar":"https://example.com/d.png"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["nickname"], "Dave");
    assert_eq!(v["phone"], "13900139000");
    assert_eq!(v["avatar"], "https://example.com/d.png");
}

#[tokio::test]
async fn it_profile_empty_nickname_resets_to_email_prefix() {
    let app = test_app();
    let cookie = register_user(&app, "eve@example.com", "Str0ng!Pass").await;

    // First set a custom nickname.
    let (_, _) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"EveCustom"}"#),
        Some(&cookie),
        None,
    )
    .await;
    // Then clear it with "" → resets to email prefix.
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":""}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["nickname"], "eve");
}

#[tokio::test]
async fn it_profile_whitespace_nickname_resets_to_email_prefix() {
    let app = test_app();
    let cookie = register_user(&app, "frank@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"   "}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["nickname"], "frank");
}

#[tokio::test]
async fn it_profile_empty_phone_clears_to_null() {
    let app = test_app();
    let cookie = register_user(&app, "grace@example.com", "Str0ng!Pass").await;

    // Set a phone, then clear it.
    let _ = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"phone":"13800138000"}"#),
        Some(&cookie),
        None,
    )
    .await;
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"phone":""}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["phone"], Value::Null);
}

#[tokio::test]
async fn it_profile_empty_avatar_clears_to_null() {
    let app = test_app();
    let cookie = register_user(&app, "heidi@example.com", "Str0ng!Pass").await;

    let _ = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"avatar":"https://example.com/h.png"}"#),
        Some(&cookie),
        None,
    )
    .await;
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"avatar":""}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["avatar"], Value::Null);
}

// ---------------------------------------------------------------------------
// PUT /api/me/profile — validation failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_profile_invalid_phone_400() {
    let app = test_app();
    let cookie = register_user(&app, "ivan@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"phone":"12345"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("invalid_field"), "body: {body}");
    assert!(body.contains("手机号"), "body: {body}");
}

#[tokio::test]
async fn it_profile_invalid_phone_wrong_second_digit_400() {
    let app = test_app();
    let cookie = register_user(&app, "judy@example.com", "Str0ng!Pass").await;

    // 12... starts with 1 but second digit 2 is not a valid mainland prefix.
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"phone":"12800138000"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("invalid_field"));
}

#[tokio::test]
async fn it_profile_invalid_avatar_400() {
    let app = test_app();
    let cookie = register_user(&app, "karl@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"avatar":"ftp://example.com/a.png"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("invalid_field"));
    assert!(body.contains("http"), "body: {body}");
}

#[tokio::test]
async fn it_profile_invalid_avatar_non_url_400() {
    let app = test_app();
    let cookie = register_user(&app, "liam@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"avatar":"javascript:alert(1)"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("invalid_field"));
}

#[tokio::test]
async fn it_profile_nickname_too_long_400() {
    let app = test_app();
    let cookie = register_user(&app, "mia@example.com", "Str0ng!Pass").await;

    let long = "a".repeat(21);
    let body = format!(r#"{{"nickname":"{long}"}}"#);
    let (status, resp) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(&body),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {resp}");
    assert!(resp.contains("invalid_field"));
    assert!(resp.contains("20"), "body: {resp}");
}

#[tokio::test]
async fn it_profile_nickname_control_chars_400() {
    let app = test_app();
    let cookie = register_user(&app, "nick@example.com", "Str0ng!Pass").await;

    let body = r#"{"nickname":"bad\nname"}"#;
    let (status, resp) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(body),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {resp}");
    assert!(resp.contains("invalid_field"));
    assert!(resp.contains("控制字符"), "body: {resp}");
}

#[tokio::test]
async fn it_profile_invalid_field_does_not_apply_others() {
    let app = test_app();
    let cookie = register_user(&app, "oli@example.com", "Str0ng!Pass").await;

    // Valid nickname + invalid phone in the same request → whole request 400,
    // no partial application.
    let (status, _) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"Oli","phone":"bad"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Confirm nothing was applied.
    let (me_status, me_body) = send(&app, "GET", "/api/me", None, Some(&cookie), None).await;
    assert_eq!(me_status, StatusCode::OK);
    let v = as_json(&me_body);
    assert_eq!(v["nickname"], "oli");
    assert_eq!(v["phone"], Value::Null);
}

#[tokio::test]
async fn it_profile_unauthenticated_401() {
    let app = test_app();
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"X"}"#),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body: {body}");
    assert!(body.contains("unauthenticated"));
}

// ---------------------------------------------------------------------------
// PUT /api/me/password — change password
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_change_password_success_200() {
    let app = test_app();
    let cookie = register_user(&app, "pat@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Str0ng!Pass","new_password":"New!Pass456"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("密码已修改"), "body: {body}");
}

#[tokio::test]
async fn it_change_password_wrong_old_400() {
    let app = test_app();
    let cookie = register_user(&app, "quinn@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Wrong!Pass","new_password":"New!Pass456"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("invalid_old_password"), "body: {body}");
    assert!(body.contains("原密码错误"), "body: {body}");
}

#[tokio::test]
async fn it_change_password_weak_new_400() {
    let app = test_app();
    let cookie = register_user(&app, "rose@example.com", "Str0ng!Pass").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Str0ng!Pass","new_password":"weak"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("weak_password"), "body: {body}");
}

#[tokio::test]
async fn it_change_password_old_password_invalid_after_change() {
    let app = test_app();
    let cookie = register_user(&app, "sam@example.com", "Str0ng!Pass").await;

    let (status, _) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Str0ng!Pass","new_password":"New!Pass456"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Old password must no longer log in.
    let login = r#"{"email":"sam@example.com","password":"Str0ng!Pass"}"#;
    let (login_status, login_body, _) = post_json(&app, "/api/login", login, None).await;
    assert_eq!(login_status, StatusCode::UNAUTHORIZED, "body: {login_body}");
    assert!(login_body.contains("invalid_credentials"));
}

#[tokio::test]
async fn it_change_password_new_password_works_after_change() {
    let app = test_app();
    let cookie = register_user(&app, "tina@example.com", "Str0ng!Pass").await;

    let (status, _) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Str0ng!Pass","new_password":"New!Pass456"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // New password logs in successfully.
    let login = r#"{"email":"tina@example.com","password":"New!Pass456"}"#;
    let (login_status, _, _) = post_json(&app, "/api/login", login, None).await;
    assert_eq!(login_status, StatusCode::OK);
}

#[tokio::test]
async fn it_change_password_existing_session_still_valid() {
    let app = test_app();
    let cookie = register_user(&app, "uma@example.com", "Str0ng!Pass").await;

    let (status, _) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"Str0ng!Pass","new_password":"New!Pass456"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The existing session (cookie) remains valid after the change.
    let (me_status, me_body) = send(&app, "GET", "/api/me", None, Some(&cookie), None).await;
    assert_eq!(me_status, StatusCode::OK, "body: {me_body}");
    assert!(me_body.contains("uma@example.com"));
}

#[tokio::test]
async fn it_change_password_unauthenticated_401() {
    let app = test_app();
    let (status, body) = send(
        &app,
        "PUT",
        "/api/me/password",
        Some(r#"{"old_password":"x","new_password":"New!Pass456"}"#),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body: {body}");
    assert!(body.contains("unauthenticated"));
}

// ---------------------------------------------------------------------------
// GET /api/me — extended fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_me_returns_default_nickname_email_prefix() {
    let app = test_app();
    let cookie = register_user(&app, "vince@example.com", "Str0ng!Pass").await;

    let (status, body) = send(&app, "GET", "/api/me", None, Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let v = as_json(&body);
    assert_eq!(v["email"], "vince@example.com");
    assert_eq!(v["nickname"], "vince"); // default = email prefix
    assert_eq!(v["phone"], Value::Null);
    assert_eq!(v["avatar"], Value::Null);
}

#[tokio::test]
async fn it_me_returns_extended_fields_after_profile_update() {
    let app = test_app();
    let cookie = register_user(&app, "wendy@example.com", "Str0ng!Pass").await;

    let (status, _) = send(
        &app,
        "PUT",
        "/api/me/profile",
        Some(r#"{"nickname":"Wendy","phone":"13800138000","avatar":"https://example.com/w.png"}"#),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (me_status, me_body) = send(&app, "GET", "/api/me", None, Some(&cookie), None).await;
    assert_eq!(me_status, StatusCode::OK, "body: {me_body}");
    let v = as_json(&me_body);
    assert_eq!(v["nickname"], "Wendy");
    assert_eq!(v["phone"], "13800138000");
    assert_eq!(v["avatar"], "https://example.com/w.png");
}

#[tokio::test]
async fn it_me_does_not_expose_password_hash() {
    let app = test_app();
    let cookie = register_user(&app, "xena@example.com", "Str0ng!Pass").await;

    let (_, body) = send(&app, "GET", "/api/me", None, Some(&cookie), None).await;
    assert!(!body.contains("password_hash"), "body: {body}");
    assert!(!body.contains("Str0ng!Pass"), "body: {body}");
}

// ---------------------------------------------------------------------------
// register — optional nickname
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_register_with_nickname_returns_it() {
    let app = test_app();
    let body = r#"{"email":"yuki@example.com","password":"Str0ng!Pass","nickname":"雪"}"#;
    let (status, resp, set_cookie) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
    assert!(set_cookie.is_some());
    let v = as_json(&resp);
    assert_eq!(v["nickname"], "雪");
    assert_eq!(v["email"], "yuki@example.com");
}

#[tokio::test]
async fn it_register_without_nickname_defaults_to_email_prefix() {
    let app = test_app();
    let body = r#"{"email":"zoe@example.com","password":"Str0ng!Pass"}"#;
    let (status, resp, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
    let v = as_json(&resp);
    assert_eq!(v["nickname"], "zoe");
}

#[tokio::test]
async fn it_register_empty_nickname_defaults_to_email_prefix() {
    let app = test_app();
    let body = r#"{"email":"amy@example.com","password":"Str0ng!Pass","nickname":""}"#;
    let (status, resp, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
    let v = as_json(&resp);
    assert_eq!(v["nickname"], "amy");
}

#[tokio::test]
async fn it_register_whitespace_nickname_defaults_to_email_prefix() {
    let app = test_app();
    let body = r#"{"email":"ben@example.com","password":"Str0ng!Pass","nickname":"   "}"#;
    let (status, resp, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
    let v = as_json(&resp);
    assert_eq!(v["nickname"], "ben");
}

#[tokio::test]
async fn it_register_nickname_trimmed() {
    let app = test_app();
    let body = r#"{"email":"cyn@example.com","password":"Str0ng!Pass","nickname":"  Cyn  "}"#;
    let (status, resp, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
    let v = as_json(&resp);
    assert_eq!(v["nickname"], "Cyn");
}

#[tokio::test]
async fn it_register_nickname_too_long_400() {
    let app = test_app();
    let long = "a".repeat(21);
    let body =
        format!(r#"{{"email":"dan@example.com","password":"Str0ng!Pass","nickname":"{long}"}}"#);
    let (status, resp, _) = post_json(&app, "/api/register", &body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {resp}");
    assert!(resp.contains("invalid_field"), "body: {resp}");
}

#[tokio::test]
async fn it_register_nickname_control_chars_400() {
    let app = test_app();
    let body = r#"{"email":"erin@example.com","password":"Str0ng!Pass","nickname":"bad\nname"}"#;
    let (status, resp, _) = post_json(&app, "/api/register", body, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {resp}");
    assert!(resp.contains("invalid_field"), "body: {resp}");
}

// ---------------------------------------------------------------------------
// IP rate limiting on register (20/min → 429)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_register_ip_rate_limit_429_after_20_from_same_ip() {
    let app = test_app();
    // Exhaust the 20/min window for one IP; each register uses a unique email.
    for i in 0..20 {
        let body = format!(r#"{{"email":"user{i}@example.com","password":"Str0ng!Pass"}}"#);
        let (status, resp, _) =
            post_json_with_xff(&app, "/api/register", &body, "203.0.113.10").await;
        assert_eq!(status, StatusCode::OK, "request #{i} should pass: {resp}");
    }
    // 21st request from the same IP → 429 too_many_attempts.
    let body = r#"{"email":"last@example.com","password":"Str0ng!Pass"}"#;
    let (status, resp, _) = post_json_with_xff(&app, "/api/register", body, "203.0.113.10").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "body: {resp}");
    assert!(resp.contains("too_many_attempts"), "body: {resp}");
}

#[tokio::test]
async fn it_register_ip_rate_limit_independent_per_ip() {
    let app = test_app();
    // Exhaust IP A.
    for i in 0..20 {
        let body = format!(r#"{{"email":"a{i}@example.com","password":"Str0ng!Pass"}}"#);
        let (status, _, _) = post_json_with_xff(&app, "/api/register", &body, "198.51.100.5").await;
        assert_eq!(status, StatusCode::OK);
    }
    // A different IP is unaffected.
    let body = r#"{"email":"other@example.com","password":"Str0ng!Pass"}"#;
    let (status, resp, _) = post_json_with_xff(&app, "/api/register", body, "198.51.100.99").await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
}

#[tokio::test]
async fn it_register_ip_rate_limit_does_not_leak_to_login() {
    let app = test_app();
    // Exhaust register window for an IP.
    for i in 0..20 {
        let body = format!(r#"{{"email":"r{i}@example.com","password":"Str0ng!Pass"}}"#);
        let (status, _, _) = post_json_with_xff(&app, "/api/register", &body, "192.0.2.7").await;
        assert_eq!(status, StatusCode::OK);
    }
    // Login from the same IP still works (separate limiter instance).
    let body = r#"{"email":"r0@example.com","password":"Str0ng!Pass"}"#;
    let (status, resp, _) = post_json_with_xff(&app, "/api/login", body, "192.0.2.7").await;
    assert_eq!(status, StatusCode::OK, "body: {resp}");
}

/// POST with an X-Forwarded-For header (client IP for rate limiting).
async fn post_json_with_xff(
    app: &axum::Router,
    path: &str,
    body: &str,
    xff: &str,
) -> (StatusCode, String, Option<String>) {
    let builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("x-forwarded-for", xff);
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
