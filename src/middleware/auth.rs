//! Session cookie parsing middleware.
//!
//! Extracts the `sid` cookie, verifies its HMAC signature, looks up the
//! session, and injects the authenticated user ID into request extensions.
//!
//! Unlike a traditional middleware layer, this is implemented as an extractor
//! (`AuthUser`) so that handlers which need authentication simply declare it
//! as a parameter, while public endpoints (register/login) skip it entirely.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::services::auth_service::{AuthError, AuthService};
use crate::utils::signed_cookie;

/// The authenticated user, extracted from a valid session cookie.
///
/// Use this as a handler parameter to require authentication:
/// ```ignore
/// async fn handler(AuthUser(user_id): AuthUser) -> impl IntoResponse { ... }
/// ```
#[derive(Debug, Clone)]
pub struct AuthUser(pub uuid::Uuid);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Arc<AuthService>: axum::extract::FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_service = Arc::<AuthService>::from_ref(state);

        // Parse the `sid` cookie from the Cookie header.
        let cookie_header = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok());

        let signed_sid = match cookie_header.and_then(extract_sid_cookie) {
            Some(v) => v,
            None => return Err(unauthenticated_response()),
        };

        // Verify HMAC signature.
        let session_id =
            match signed_cookie::verify_and_extract(&signed_sid, auth_service.session_secret()) {
                Some(id) => id,
                None => return Err(unauthenticated_response()),
            };

        // Look up session and user.
        match auth_service.get_user_by_session(&session_id).await {
            Ok((user_id, _email)) => Ok(AuthUser(user_id)),
            Err(AuthError::Unauthenticated) => Err(unauthenticated_response()),
            Err(_) => Err(internal_error_response()),
        }
    }
}

/// Extract the value of the `sid` cookie from a raw `Cookie:` header value.
fn extract_sid_cookie(cookie_header: &str) -> Option<String> {
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(rest) = pair.strip_prefix("sid=") {
            return Some(rest.to_string());
        }
    }
    None
}

fn unauthenticated_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unauthenticated",
            "message": "请先登录"
        })),
    )
        .into_response()
}

fn internal_error_response() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal_error",
            "message": "服务器内部错误"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sid() {
        assert_eq!(
            extract_sid_cookie("sid=abc.123; other=xyz"),
            Some("abc.123".into())
        );
        assert_eq!(
            extract_sid_cookie("foo=bar; sid=hello.world"),
            Some("hello.world".into())
        );
        assert_eq!(extract_sid_cookie("no cookies here"), None);
        assert_eq!(extract_sid_cookie(""), None);
    }
}
