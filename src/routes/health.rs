//! HTTP handlers for health / liveness endpoints.
//!
//! Endpoints:
//! - `GET /ping` - process-level liveness probe, returns `{"status":"ok"}`
//!
//! `/ping` is intentionally liveness-only: it does not check the database or
//! downstream dependencies. Readiness semantics are out of scope for v0.1.0.

use axum::Json;
use serde_json::{json, Value};

/// GET /ping
///
/// Returns `200 OK` with body `{"status":"ok"}` and
/// `Content-Type: application/json`. Used for liveness probes.
pub async fn ping() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    fn test_router() -> axum::Router {
        axum::Router::new().route("/ping", axum::routing::get(ping))
    }

    #[tokio::test]
    async fn ping_returns_200_with_status_ok() {
        let response = test_router()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap();
        assert!(content_type.starts_with("application/json"));

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value, json!({ "status": "ok" }));
    }
}
