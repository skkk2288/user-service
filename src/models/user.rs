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
/// uniqueness checks.  `nickname` is always non-empty (defaults to the email
/// prefix when not provided); `phone` / `avatar` are optional.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub nickname: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
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
    /// Update nickname / phone / avatar. Returns the updated `User`, or `None`
    /// if no user with `id` exists.
    async fn update_profile(
        &self,
        id: Uuid,
        nickname: String,
        phone: Option<String>,
        avatar: Option<String>,
    ) -> Option<User>;
    /// Update the password hash. Returns `true` if a user was updated.
    async fn update_password(&self, id: Uuid, new_hash: String) -> bool;
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

    async fn update_profile(
        &self,
        id: Uuid,
        nickname: String,
        phone: Option<String>,
        avatar: Option<String>,
    ) -> Option<User> {
        let mut users = self.users.write().await;
        let user = users.get_mut(&id)?;
        user.nickname = nickname;
        user.phone = phone;
        user.avatar = avatar;
        user.updated_at = Utc::now();
        Some(user.clone())
    }

    async fn update_password(&self, id: Uuid, new_hash: String) -> bool {
        let mut users = self.users.write().await;
        let Some(user) = users.get_mut(&id) else {
            return false;
        };
        user.password_hash = new_hash;
        user.updated_at = Utc::now();
        true
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
            nickname: "nickname".into(),
            phone: None,
            avatar: None,
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

    #[tokio::test]
    async fn update_profile_updates_fields() {
        let repo = InMemoryUserRepository::new();
        let user = make_user("alice@example.com");
        repo.insert(user.clone()).await.unwrap();

        let updated = repo
            .update_profile(
                user.id,
                "爱丽丝".into(),
                Some("13800138000".into()),
                Some("https://example.com/a.png".into()),
            )
            .await
            .unwrap();
        assert_eq!(updated.nickname, "爱丽丝");
        assert_eq!(updated.phone.as_deref(), Some("13800138000"));
        assert_eq!(updated.avatar.as_deref(), Some("https://example.com/a.png"));

        // Missing user returns None.
        assert!(repo
            .update_profile(Uuid::new_v4(), "x".into(), None, None)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn update_password_updates_hash() {
        let repo = InMemoryUserRepository::new();
        let user = make_user("bob@example.com");
        repo.insert(user.clone()).await.unwrap();

        assert!(repo.update_password(user.id, "new-hash".into()).await);
        let found = repo.find_by_id(user.id).await.unwrap();
        assert_eq!(found.password_hash, "new-hash");

        assert!(!repo.update_password(Uuid::new_v4(), "x".into()).await);
    }
}
