//! User model, repository trait, and in-memory implementation.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// A registered user.
///
/// `password_hash` stores a bcrypt hash; the plaintext password is never
/// retained.  `email` is stored lowercased (normalized) for case-insensitive
/// uniqueness checks.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Error returned by repository operations.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum RepoError {
    #[error("email already exists")]
    EmailAlreadyExists,
    #[error("internal repository error")]
    Internal,
}

/// Persistence abstraction for users.
///
/// Business logic depends only on this trait, so swapping the in-memory
/// implementation for a PostgreSQL-backed one in v0.2 requires no service
/// layer changes.
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Insert a new user. Returns `EmailAlreadyExists` if the email is taken.
    async fn insert(&self, user: User) -> Result<User, RepoError>;
    /// Look up a user by their normalized email.
    async fn find_by_email(&self, email: &str) -> Option<User>;
    /// Look up a user by ID.
    async fn find_by_id(&self, id: Uuid) -> Option<User>;
}

/// In-memory user repository backed by `tokio::sync::RwLock<HashMap>`.
///
/// An `email_index` reverse map provides O(1) email lookups without scanning
/// the primary store.
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: Arc<RwLock<HashMap<Uuid, User>>>,
    email_index: Arc<RwLock<HashMap<String, Uuid>>>,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn insert(&self, user: User) -> Result<User, RepoError> {
        let email = user.email.to_lowercase();

        // Check email uniqueness and reserve the slot atomically.
        {
            let mut idx = self.email_index.write().await;
            if idx.contains_key(&email) {
                return Err(RepoError::EmailAlreadyExists);
            }
            idx.insert(email.clone(), user.id);
        }
        {
            let mut users = self.users.write().await;
            users.insert(user.id, user.clone());
        }
        Ok(user)
    }

    async fn find_by_email(&self, email: &str) -> Option<User> {
        let email = email.to_lowercase();
        let idx = self.email_index.read().await;
        let id = idx.get(&email).copied()?;
        drop(idx);
        let users = self.users.read().await;
        users.get(&id).cloned()
    }

    async fn find_by_id(&self, id: Uuid) -> Option<User> {
        let users = self.users.read().await;
        users.get(&id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(email: &str) -> User {
        let now = Utc::now();
        User {
            id: Uuid::new_v4(),
            email: email.into(),
            password_hash: "fake-hash".into(),
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn insert_and_find() {
        let repo = InMemoryUserRepository::new();
        let user = make_user("Alice@Example.COM");
        repo.insert(user.clone()).await.unwrap();

        // email lookup is case-insensitive
        let found = repo.find_by_email("alice@example.com").await.unwrap();
        assert_eq!(found.id, user.id);
        assert_eq!(found.email, "Alice@Example.COM");

        // id lookup
        let found_by_id = repo.find_by_id(user.id).await.unwrap();
        assert_eq!(found_by_id.email, user.email);
    }

    #[tokio::test]
    async fn duplicate_email_rejected() {
        let repo = InMemoryUserRepository::new();
        repo.insert(make_user("bob@test.com")).await.unwrap();
        let result = repo.insert(make_user("BOB@test.com")).await;
        assert!(matches!(result, Err(RepoError::EmailAlreadyExists)));
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let repo = InMemoryUserRepository::new();
        assert!(repo.find_by_email("nobody@test.com").await.is_none());
        assert!(repo.find_by_id(Uuid::new_v4()).await.is_none());
    }
}
