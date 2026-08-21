//! Client IP extraction for rate limiting.
//!
//! Reads the client IP from, in priority order:
//! 1. `X-Forwarded-For` first hop (when behind a reverse proxy)
//! 2. `ConnectInfo<SocketAddr>` (direct connection; enabled in `main.rs`)
//! 3. `"unknown"` fallback (e.g. in-process test clients)
//!
//! This keeps IP-based rate limiting working both in production and in
//! integration tests that do not inject connect info.

use std::net::SocketAddr;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::http::header::HeaderMap;
use axum::http::request::Parts;
use axum::http::Extensions;

/// The client IP as a string, used as a rate-limit key.
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(resolve_ip(&parts.headers, &parts.extensions)))
    }
}

/// Resolve the client IP from headers (X-Forwarded-For) or connect info.
fn resolve_ip(headers: &HeaderMap, extensions: &Extensions) -> String {
    // X-Forwarded-For first hop (reverse proxy).
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next().map(str::trim) {
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }

    // Direct connection via ConnectInfo.
    if let Some(ConnectInfo(addr)) = extensions.get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }

    "unknown".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xff_takes_priority() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
        assert_eq!(resolve_ip(&headers, &Extensions::new()), "1.2.3.4");
    }

    #[test]
    fn falls_back_to_unknown() {
        assert_eq!(resolve_ip(&HeaderMap::new(), &Extensions::new()), "unknown");
    }

    #[test]
    fn connect_info_used_when_no_xff() {
        let headers = HeaderMap::new();
        let mut ext = Extensions::new();
        ext.insert(ConnectInfo::<SocketAddr>("1.2.3.4:5000".parse().unwrap()));
        assert_eq!(resolve_ip(&headers, &ext), "1.2.3.4");
    }
}
