//! Authentication business logic: registration, login, session management,
//! and rate limiting.
//!
//! This layer depends only on repository traits and the password utilities,
//! keeping it decoupled from the HTTP layer and storage implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::config::Config;
use crate::models::{
    RateLimitStatus, RateLimiter, Session, SessionRepository, User, UserRepository,
};
use crate::utils::password;
use crate::utils::session_id;

/// Errors from the auth service, each mapping to a specific API error code.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AuthError {
    #[error("邮箱格式不正确")]
    InvalidEmail,
    #[error("密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类")]
    WeakPassword,
    #[error("请求参数不正确")]
    ValidationError,
    #[error("该邮箱已注册")]
    EmailAlreadyExists,
    #[error("邮箱或密码错误")]
    InvalidCredentials,
    #[error("登录尝试次数过多，请 15 分钟后再试")]
    TooManyAttempts,
    #[error("请先登录")]
    Unauthenticated,
    #[error("internal error")]
    Internal,
}

/// Result of a successful registration or login.
#[allow(dead_code)]
pub struct AuthResult {
    pub user_id: Uuid,
    pub email: String,
    /// The signed session cookie value to set via `Set-Cookie`.
    pub signed_cookie_value: String,
    /// Cookie Max-Age in seconds.
    pub cookie_max_age: u64,
    /// The raw session ID (for server-side deletion on logout).
    pub session_id: String,
}

/// The auth service, holding shared dependencies.
#[derive(Clone)]
pub struct AuthService {
    user_repo: Arc<dyn UserRepository>,
    session_repo: Arc<dyn SessionRepository>,
    rate_limiter: Arc<RateLimiter>,
    pub(crate) config: Arc<Config>,
}

impl AuthService {
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        session_repo: Arc<dyn SessionRepository>,
        rate_limiter: Arc<RateLimiter>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            user_repo,
            session_repo,
            rate_limiter,
            config,
        }
    }

    /// The session signing secret (for cookie HMAC verification).
    pub fn session_secret(&self) -> &str {
        &self.config.session_secret
    }

    /// Whether the Secure flag should be set on cookies.
    pub fn cookie_secure(&self) -> bool {
        self.config.cookie_secure
    }

    /// Register a new user and automatically create a session.
    ///
    /// The session uses the short TTL (2h) since registration does not involve
    /// a "remember me" choice.
    pub async fn register(&self, email: &str, password: &str) -> Result<AuthResult, AuthError> {
        // Validate email format.
        if !is_valid_email(email) {
            return Err(AuthError::InvalidEmail);
        }

        // Validate password strength.
        password::validate_password(password).map_err(|_| AuthError::WeakPassword)?;

        let normalized_email = email.to_lowercase();

        // Check uniqueness before hashing (fast path).
        if self
            .user_repo
            .find_by_email(&normalized_email)
            .await
            .is_some()
        {
            return Err(AuthError::EmailAlreadyExists);
        }

        // Hash password with bcrypt.
        let password_hash = password::hash_password(password, self.config.bcrypt_cost)
            .map_err(|_| AuthError::Internal)?;

        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            email: normalized_email.clone(),
            password_hash,
            created_at: now,
            updated_at: now,
        };

        self.user_repo
            .insert(user.clone())
            .await
            .map_err(|e| match e {
                crate::models::user::RepoError::EmailAlreadyExists => AuthError::EmailAlreadyExists,
                _ => AuthError::Internal,
            })?;

        // Create session (registration auto-login, short TTL).
        let result = self
            .create_session(user.id, &normalized_email, false, now)
            .await?;
        Ok(result)
    }

    /// Authenticate a user and create a session.
    ///
    /// Rate limiting is checked *before* password verification to avoid
    /// burning bcrypt cycles during a lockout.  Failed attempts (whether
    /// email not found or password mismatch) are recorded identically to
    /// prevent account enumeration.
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        remember_me: bool,
    ) -> Result<AuthResult, AuthError> {
        let now = Utc::now();
        let normalized_email = email.to_lowercase();

        // Rate limit check (before bcrypt).
        match self.rate_limiter.check(&normalized_email, now).await {
            RateLimitStatus::Locked(_) => {
                return Err(AuthError::TooManyAttempts);
            }
            RateLimitStatus::Allowed => {}
        }

        // Find user.
        let user = match self.user_repo.find_by_email(&normalized_email).await {
            Some(u) => u,
            None => {
                // Record fail but don't reveal whether the email exists.
                self.rate_limiter.record_fail(&normalized_email, now).await;
                return Err(AuthError::InvalidCredentials);
            }
        };

        // Verify password.
        let verified = password::verify_password(password, &user.password_hash)
            .map_err(|_| AuthError::Internal)?;

        if !verified {
            self.rate_limiter.record_fail(&normalized_email, now).await;
            return Err(AuthError::InvalidCredentials);
        }

        // Success - reset rate limiter.
        self.rate_limiter.reset(&normalized_email).await;

        let result = self
            .create_session(user.id, &user.email, remember_me, now)
            .await?;
        Ok(result)
    }

    /// Log out by deleting the session from the store.
    ///
    /// Returns `Ok(())` if the session was found and deleted, or
    /// `Err(Unauthenticated)` if no valid session exists.
    pub async fn logout(&self, session_id: &str) -> Result<(), AuthError> {
        let deleted = self.session_repo.delete(session_id).await;
        if deleted {
            Ok(())
        } else {
            Err(AuthError::Unauthenticated)
        }
    }

    /// Look up the user associated with a session ID.
    ///
    /// Expired sessions are treated as invalid.
    pub async fn get_user_by_session(&self, session_id: &str) -> Result<(Uuid, String), AuthError> {
        let session = self
            .session_repo
            .find_by_id(session_id)
            .await
            .ok_or(AuthError::Unauthenticated)?;

        let user = self
            .user_repo
            .find_by_id(session.user_id)
            .await
            .ok_or(AuthError::Unauthenticated)?;

        Ok((user.id, user.email))
    }

    /// Look up a user's email by their user ID.
    ///
    /// Used by the `/api/me` endpoint, which already has the user ID from the
    /// session extractor but needs the email for the response.
    pub async fn get_email_by_user_id(&self, user_id: Uuid) -> Option<String> {
        self.user_repo.find_by_id(user_id).await.map(|u| u.email)
    }

    /// Internal helper: create a session and produce the signed cookie value.
    async fn create_session(
        &self,
        user_id: Uuid,
        email: &str,
        remember_me: bool,
        now: DateTime<Utc>,
    ) -> Result<AuthResult, AuthError> {
        let ttl_secs = if remember_me {
            self.config.session_ttl_long
        } else {
            self.config.session_ttl_short
        };

        let session_id = session_id::generate_session_id();
        let expires_at = now + chrono::Duration::seconds(ttl_secs as i64);

        let session = Session {
            id: session_id.clone(),
            user_id,
            expires_at,
            created_at: now,
        };
        self.session_repo.insert(session).await;

        let signed_value =
            crate::utils::signed_cookie::sign(&session_id, &self.config.session_secret);

        Ok(AuthResult {
            user_id,
            email: email.to_string(),
            signed_cookie_value: signed_value,
            cookie_max_age: ttl_secs,
            session_id,
        })
    }
}

/// Simplified email format validation.
///
/// Uses the regex from the API contract:
/// `^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`
fn is_valid_email(email: &str) -> bool {
    if email.len() > 254 {
        return false;
    }
    let local_end = match email.find('@') {
        Some(i) => {
            if i == 0 {
                return false;
            }
            i
        }
        None => return false,
    };
    let (local, domain) = email.split_at(local_end);
    let domain = &domain[1..]; // skip '@'

    if domain.is_empty() {
        return false;
    }

    let dot = match domain.rfind('.') {
        Some(i) => i,
        None => return false,
    };
    let tld = &domain[dot + 1..];
    if tld.len() < 2 {
        return false;
    }

    // Validate character sets.
    local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
        && domain[..dot]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{InMemorySessionRepository, InMemoryUserRepository};

    fn make_service(cost: u32) -> AuthService {
        let config = Arc::new(Config {
            session_secret: "test-secret-that-is-long-enough-ok!!".into(),
            cookie_secure: false,
            bcrypt_cost: cost,
            rate_limit_max_failures: 5,
            rate_limit_lockout_minutes: 15,
            session_ttl_short: 7200,
            session_ttl_long: 604_800,
            cors_origin: "http://localhost:5173".into(),
            listen_addr: "0.0.0.0:3000".into(),
        });
        AuthService::new(
            Arc::new(InMemoryUserRepository::new()),
            Arc::new(InMemorySessionRepository::new()),
            Arc::new(RateLimiter::new(5, 15)),
            config,
        )
    }

    #[tokio::test]
    async fn register_success() {
        let svc = make_service(4);
        let result = svc.register("test@example.com", "Str0ng!Pass").await;
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.email, "test@example.com");
        assert!(r.signed_cookie_value.contains('.'));
        assert_eq!(r.cookie_max_age, 7200);
    }

    #[tokio::test]
    async fn register_invalid_email() {
        let svc = make_service(4);
        let result = svc.register("not-an-email", "Str0ng!Pass").await;
        assert!(matches!(result, Err(AuthError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_weak_password() {
        let svc = make_service(4);
        let result = svc.register("test@example.com", "weak").await;
        assert!(matches!(result, Err(AuthError::WeakPassword)));
    }

    #[tokio::test]
    async fn register_duplicate_email() {
        let svc = make_service(4);
        svc.register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();
        let result = svc.register("TEST@example.com", "Str0ng!Pass").await;
        assert!(matches!(result, Err(AuthError::EmailAlreadyExists)));
    }

    #[tokio::test]
    async fn login_success() {
        let svc = make_service(4);
        svc.register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();

        let result = svc.login("test@example.com", "Str0ng!Pass", false).await;
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.cookie_max_age, 7200);
    }

    #[tokio::test]
    async fn login_remember_me() {
        let svc = make_service(4);
        svc.register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();

        let result = svc.login("test@example.com", "Str0ng!Pass", true).await;
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.cookie_max_age, 604_800);
    }

    #[tokio::test]
    async fn login_wrong_password() {
        let svc = make_service(4);
        svc.register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();

        let result = svc.login("test@example.com", "wrong", false).await;
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn login_nonexistent_user() {
        let svc = make_service(4);
        let result = svc.login("nobody@example.com", "Str0ng!Pass", false).await;
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn login_lockout_after_5_fails() {
        let svc = make_service(4);
        svc.register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();

        for _ in 0..5 {
            let _ = svc.login("test@example.com", "wrong", false).await;
        }
        let result = svc.login("test@example.com", "Str0ng!Pass", false).await;
        assert!(matches!(result, Err(AuthError::TooManyAttempts)));
    }

    #[tokio::test]
    async fn logout_and_session_lookup() {
        let svc = make_service(4);
        let r = svc
            .register("test@example.com", "Str0ng!Pass")
            .await
            .unwrap();

        // Session is valid.
        let (uid, email) = svc.get_user_by_session(&r.session_id).await.unwrap();
        assert_eq!(email, "test@example.com");
        assert_eq!(uid, r.user_id);

        // Logout.
        svc.logout(&r.session_id).await.unwrap();

        // Session no longer valid.
        let result = svc.get_user_by_session(&r.session_id).await;
        assert!(matches!(result, Err(AuthError::Unauthenticated)));
    }

    #[tokio::test]
    async fn logout_invalid_session() {
        let svc = make_service(4);
        let result = svc.logout("nonexistent").await;
        assert!(matches!(result, Err(AuthError::Unauthenticated)));
    }

    #[test]
    fn email_validation() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("a.b+c@d.co"));
        assert!(!is_valid_email("not-an-email"));
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("user@"));
        assert!(!is_valid_email("user@example"));
        assert!(!is_valid_email("user@example.c"));
    }
}
