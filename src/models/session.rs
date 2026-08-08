//! Session model, repository trait, and in-memory implementation.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

/// A server-side session.
///
/// `id` is a 256-bit cryptographically random token (hex-encoded).  The token
/// itself is never signed here; HMAC signing is applied at the cookie layer
/// (see `src/middleware/auth.rs`) so that forged tokens are rejected before
/// hitting the store.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub id: String,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Returns `true` if the session has expired relative to `now`.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

/// Persistence abstraction for sessions.
#[allow(dead_code)]
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Insert a new session.
    async fn insert(&self, session: Session) -> Session;
    /// Look up a session by its ID.  Expired sessions are treated as absent
    /// (returned as `None`) and lazily deleted.
    async fn find_by_id(&self, id: &str) -> Option<Session>;
    /// Delete a session by ID.  Returns `true` if a session was removed.
    async fn delete(&self, id: &str) -> bool;
    /// Delete all sessions belonging to a user (logout all devices).
    /// Returns the number of sessions removed.
    async fn delete_by_user(&self, user_id: Uuid) -> u32;
}

/// In-memory session repository.
#[derive(Debug, Default)]
pub struct InMemorySessionRepository {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl InMemorySessionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionRepository for InMemorySessionRepository {
    async fn insert(&self, session: Session) -> Session {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        session
    }

    async fn find_by_id(&self, id: &str) -> Option<Session> {
        let now = Utc::now();
        // Lock once, check expiry, and remove expired entries lazily.
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get(id) {
            if session.is_expired(now) {
                sessions.remove(id);
                return None;
            }
            return Some(session.clone());
        }
        None
    }

    async fn delete(&self, id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id).is_some()
    }

    async fn delete_by_user(&self, user_id: Uuid) -> u32 {
        let mut sessions = self.sessions.write().await;
        let to_remove: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.user_id == user_id)
            .map(|(k, _)| k.clone())
            .collect();
        let count = to_remove.len() as u32;
        for id in to_remove {
            sessions.remove(&id);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(ttl_secs: i64) -> Session {
        let now = Utc::now();
        Session {
            id: "test-session-id".into(),
            user_id: Uuid::new_v4(),
            expires_at: now + chrono::Duration::seconds(ttl_secs),
            created_at: now,
        }
    }

    #[tokio::test]
    async fn insert_and_find() {
        let repo = InMemorySessionRepository::new();
        let session = make_session(7200);
        repo.insert(session.clone()).await;
        let found = repo.find_by_id("test-session-id").await.unwrap();
        assert_eq!(found.user_id, session.user_id);
    }

    #[tokio::test]
    async fn expired_session_is_none_and_deleted() {
        let repo = InMemorySessionRepository::new();
        let session = make_session(-10); // already expired
        repo.insert(session).await;
        assert!(repo.find_by_id("test-session-id").await.is_none());
        // Should have been lazily deleted.
        assert!(repo.find_by_id("test-session-id").await.is_none());
    }

    #[tokio::test]
    async fn delete_session() {
        let repo = InMemorySessionRepository::new();
        repo.insert(make_session(7200)).await;
        assert!(repo.delete("test-session-id").await);
        assert!(!repo.delete("test-session-id").await);
    }

    #[tokio::test]
    async fn delete_by_user() {
        let repo = InMemorySessionRepository::new();
        let uid = Uuid::new_v4();
        let now = Utc::now();
        for i in 0..3 {
            repo.insert(Session {
                id: format!("sess-{i}"),
                user_id: uid,
                expires_at: now + chrono::Duration::seconds(3600),
                created_at: now,
            })
            .await;
        }
        // Another user's session.
        repo.insert(Session {
            id: "other".into(),
            user_id: Uuid::new_v4(),
            expires_at: now + chrono::Duration::seconds(3600),
            created_at: now,
        })
        .await;

        let removed = repo.delete_by_user(uid).await;
        assert_eq!(removed, 3);
        assert!(repo.find_by_id("sess-0").await.is_none());
        assert!(repo.find_by_id("other").await.is_some());
    }
}
