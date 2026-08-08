# Data Model: 用户注册登录功能

## 存储概述

v0.1.0 使用**内存存储**（`tokio::sync::RwLock<HashMap<...>>`），通过 Repository trait 抽象。
v0.2+ 切换 PostgreSQL 时，表结构如下设计，Repository impl 替换即可，service 层不变。

以下先给出 Rust struct / trait 定义（v0.1.0 实际使用），再给出未来 PostgreSQL 表结构。

---

## Rust 模型（v0.1.0 内存存储）

### User

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};

pub struct User {
    pub id: Uuid,
    pub email: String,           // 已 normalize 为小写
    pub password_hash: String,   // bcrypt hash, cost=12
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Session

```rust
pub struct Session {
    pub id: String,               // 256-bit 随机 token (hex 编码)
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
```

### RateLimitEntry

```rust
pub struct RateLimitEntry {
    pub fail_count: u32,
    pub first_fail_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
}
```

### Repository Traits

```rust
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn insert(&self, user: User) -> Result<User, RepoError>;
    async fn find_by_email(&self, email: &str) -> Option<User>;
    async fn find_by_id(&self, id: Uuid) -> Option<User>;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, session: Session) -> Result<Session, RepoError>;
    async fn find_by_id(&self, id: &str) -> Option<Session>;
    async fn delete(&self, id: &str) -> bool;
    async fn delete_by_user(&self, user_id: Uuid) -> u32;  // 登出所有设备
}
```

### In-Memory 实现

```rust
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

pub struct InMemoryUserRepository {
    users: Arc<RwLock<HashMap<Uuid, User>>>,
    email_index: Arc<RwLock<HashMap<String, Uuid>>>,  // email -> id 反向索引
}

pub struct InMemorySessionRepository {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}
```

---

## PostgreSQL 表结构（v0.2+ 参考设计）

### users table

| 列 | 类型 | 约束 | 说明 |
|----|------|------|------|
| id | uuid | PK | UUID v4 |
| email | text | unique, not null | 已 normalize 小写 |
| password_hash | text | not null | bcrypt hash, cost=12 |
| created_at | timestamptz | not null, default now() | |
| updated_at | timestamptz | not null, default now() | 触发器自动更新 |

```sql
CREATE TABLE users (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    email        TEXT         NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

### sessions table

| 列 | 类型 | 约束 | 说明 |
|----|------|------|------|
| id | text | PK | 256-bit hex session token |
| user_id | uuid | FK -> users(id) ON DELETE CASCADE, not null | |
| expires_at | timestamptz | not null | |
| created_at | timestamptz | not null, default now() | |

```sql
CREATE TABLE sessions (
    id          TEXT         PRIMARY KEY,
    user_id     UUID         NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ  NOT NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

### login_attempts table（v0.2+ 限流持久化）

| 列 | 类型 | 约束 | 说明 |
|----|------|------|------|
| email | text | PK | 已 normalize 小写 |
| fail_count | int | not null, default 0 | |
| first_fail_at | timestamptz | | 首次失败时间 |
| locked_until | timestamptz | nullable | 锁定截止时间，NULL=未锁定 |

```sql
CREATE TABLE login_attempts (
    email          TEXT         PRIMARY KEY,
    fail_count     INTEGER      NOT NULL DEFAULT 0,
    first_fail_at  TIMESTAMPTZ,
    locked_until   TIMESTAMPTZ
);
```

---

## Indexes

### users

| 索引 | 列 | 类型 | 说明 |
|------|-----|------|------|
| `users_pkey` | id | unique | PK 自带 |
| `users_email_key` | email | unique | 邮箱唯一性校验 |

```sql
-- email unique 索引由 UNIQUE 约束自动创建
```

### sessions

| 索引 | 列 | 类型 | 说明 |
|------|-----|------|------|
| `sessions_pkey` | id | unique | PK 自带，session 查询主路径 |
| `idx_sessions_user_id` | user_id | btree | 按用户查所有 session（登出所有设备） |
| `idx_sessions_expires_at` | expires_at | btree | 过期清理任务扫描 |

```sql
CREATE INDEX idx_sessions_user_id    ON sessions(user_id);
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);
```

---

## 数据生命周期

### Session 过期清理

- **v0.1.0 内存**：访问时惰性检查 `expires_at`，过期则删除并返回 401。可选后台 task 定期扫描清理。
- **v0.2+ PostgreSQL**：定时 cron job `DELETE FROM sessions WHERE expires_at < now()`。

### Session 吊销

- 登出：`SessionRepository.delete(session_id)` —— 单设备登出。
- 改密码 / 安全事件（v0.2+）：`SessionRepository.delete_by_user(user_id)` —— 全设备登出。

### 数据不持久化（v0.1.0）

- v0.1.0 内存存储，服务重启数据丢失（用户需重新注册）。这是 v0.1.0 的已知限制，v0.2+ 引入 PostgreSQL 后解决。
