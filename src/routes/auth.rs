//! HTTP handlers for authentication and profile endpoints.
//!
//! Endpoints:
//! - `POST /api/register` - register + auto-login (optional nickname)
//! - `POST /api/login` - login with rate limiting
//! - `POST /api/logout` - logout (clear session + cookie)
//! - `GET /api/me` - get current user info (incl. profile)
//! - `PUT /api/me/profile` - update nickname/phone/avatar (partial)
//! - `PUT /api/me/password` - change password

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::middleware::auth::AuthUser;
use crate::middleware::client_ip::ClientIp;
use crate::services::auth_service::{AuthError, AuthService};

// ---------------------------------------------------------------------------
// Request / Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub remember_me: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

/// Register response (includes nickname; phone/avatar not exposed at signup).
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub user_id: uuid::Uuid,
    pub email: String,
    pub nickname: String,
}

/// Login response (unchanged contract: no profile fields).
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub user_id: uuid::Uuid,
    pub email: String,
}

/// Full profile response for `/api/me` and `/api/me/profile`.
#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub user_id: uuid::Uuid,
    pub email: String,
    pub nickname: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/register
pub async fn register(
    State(auth_service): State<Arc<AuthService>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<RegisterRequest>,
) -> Response {
    match auth_service
        .register(&ip, &req.email, &req.password, req.nickname.as_deref())
        .await
    {
        Ok(result) => {
            let cookie = build_cookie(
                &result.signed_cookie_value,
                result.cookie_max_age,
                auth_service.cookie_secure(),
            );
            (
                StatusCode::OK,
                [(SET_COOKIE, cookie)],
                Json(RegisterResponse {
                    user_id: result.user_id,
                    email: result.email,
                    nickname: result.nickname,
                }),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// POST /api/login
pub async fn login(
    State(auth_service): State<Arc<AuthService>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<LoginRequest>,
) -> Response {
    match auth_service
        .login(&ip, &req.email, &req.password, req.remember_me)
        .await
    {
        Ok(result) => {
            let cookie = build_cookie(
                &result.signed_cookie_value,
                result.cookie_max_age,
                auth_service.cookie_secure(),
            );
            (
                StatusCode::OK,
                [(SET_COOKIE, cookie)],
                Json(UserResponse {
                    user_id: result.user_id,
                    email: result.email,
                }),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// POST /api/logout
pub async fn logout(
    State(auth_service): State<Arc<AuthService>>,
    AuthUser(_user_id): AuthUser,
    headers: axum::http::HeaderMap,
) -> Response {
    // Extract session ID from cookie to delete it server-side.
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok());

    let signed_sid = match cookie_header.and_then(extract_sid_cookie) {
        Some(v) => v,
        None => return error_response(AuthError::Unauthenticated),
    };

    let session_id = match crate::utils::signed_cookie::verify_and_extract(
        &signed_sid,
        auth_service.session_secret(),
    ) {
        Some(id) => id,
        None => return error_response(AuthError::Unauthenticated),
    };

    match auth_service.logout(&session_id).await {
        Ok(()) => {
            // Clear cookie with Max-Age=0.
            let cookie = build_cookie("", 0, auth_service.cookie_secure());
            (
                StatusCode::OK,
                [(SET_COOKIE, cookie)],
                Json(MessageResponse {
                    message: "已登出".into(),
                }),
            )
                .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// GET /api/me
pub async fn me(
    State(auth_service): State<Arc<AuthService>>,
    AuthUser(user_id): AuthUser,
) -> Response {
    match auth_service.get_user(user_id).await {
        Some(user) => Json(ProfileResponse {
            user_id: user.id,
            email: user.email,
            nickname: user.nickname,
            phone: user.phone,
            avatar: user.avatar,
        })
        .into_response(),
        None => error_response(AuthError::Unauthenticated),
    }
}

/// PUT /api/me/profile
pub async fn update_profile(
    State(auth_service): State<Arc<AuthService>>,
    AuthUser(user_id): AuthUser,
    Json(req): Json<UpdateProfileRequest>,
) -> Response {
    match auth_service
        .update_profile(user_id, req.nickname, req.phone, req.avatar)
        .await
    {
        Ok(user) => Json(ProfileResponse {
            user_id: user.id,
            email: user.email,
            nickname: user.nickname,
            phone: user.phone,
            avatar: user.avatar,
        })
        .into_response(),
        Err(e) => error_response(e),
    }
}

/// PUT /api/me/password
pub async fn change_password(
    State(auth_service): State<Arc<AuthService>>,
    AuthUser(user_id): AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> Response {
    match auth_service
        .change_password(user_id, &req.old_password, &req.new_password)
        .await
    {
        Ok(()) => Json(MessageResponse {
            message: "密码已修改".into(),
        })
        .into_response(),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the `sid` cookie value from a raw `Cookie:` header value.
fn extract_sid_cookie(cookie_header: &str) -> Option<String> {
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(rest) = pair.strip_prefix("sid=") {
            return Some(rest.to_string());
        }
    }
    None
}

/// Build a `Set-Cookie` header value for the session.
fn build_cookie(value: &str, max_age: u64, secure: bool) -> String {
    let secure_flag = if secure { "Secure; " } else { "" };
    format!(
        "sid={}; HttpOnly; {}SameSite=Lax; Path=/; Max-Age={}",
        value, secure_flag, max_age
    )
}

/// Map an `AuthError` to the appropriate HTTP error response.
fn error_response(e: AuthError) -> Response {
    let (status, error_code, message): (StatusCode, &str, String) = match e {
        AuthError::InvalidEmail => (
            StatusCode::BAD_REQUEST,
            "invalid_email",
            "邮箱格式不正确".to_string(),
        ),
        AuthError::WeakPassword => (
            StatusCode::BAD_REQUEST,
            "weak_password",
            "密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类".to_string(),
        ),
        AuthError::ValidationError => (
            StatusCode::BAD_REQUEST,
            "validation_error",
            "请求参数不正确".to_string(),
        ),
        AuthError::InvalidCredentials => (
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "邮箱或密码错误".to_string(),
        ),
        AuthError::Unauthenticated => (
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "请先登录".to_string(),
        ),
        AuthError::EmailAlreadyExists => (
            StatusCode::CONFLICT,
            "email_already_exists",
            "该邮箱已注册".to_string(),
        ),
        AuthError::TooManyAttempts => (
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "登录尝试次数过多，请 15 分钟后再试".to_string(),
        ),
        AuthError::IpRateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "请求过于频繁，请稍后再试".to_string(),
        ),
        AuthError::InvalidField(msg) => (StatusCode::BAD_REQUEST, "invalid_field", msg),
        AuthError::InvalidOldPassword => (
            StatusCode::BAD_REQUEST,
            "invalid_old_password",
            "原密码错误".to_string(),
        ),
        AuthError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "服务器内部错误".to_string(),
        ),
    };

    (
        status,
        Json(json!({ "error": error_code, "message": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_cookie_secure() {
        let c = build_cookie("abc.123", 7200, true);
        assert!(c.contains("sid=abc.123"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("Secure"));
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Path=/"));
        assert!(c.contains("Max-Age=7200"));
    }

    #[test]
    fn build_cookie_insecure() {
        let c = build_cookie("abc.123", 7200, false);
        assert!(c.contains("sid=abc.123"));
        assert!(!c.contains("Secure"));
    }

    #[test]
    fn build_cookie_clear() {
        let c = build_cookie("", 0, true);
        assert!(c.contains("sid=;"));
        assert!(c.contains("Max-Age=0"));
    }
}
