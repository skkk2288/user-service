# Data Model: 用户注册 + 个人资料管理

## 存储概述

v0.1.0 使用**内存存储**（`tokio::sync::RwLock<HashMap<...>>`），通过 Repository trait 抽象。
v0.2+ 切换 PostgreSQL 时，表结构如下设计，Repository impl 替换即可，service 层不变。

本次在既有 User 模型上**新增三个字段**：`nickname`（非空，默认邮箱前缀）、`phone`
（可空）、`avatar`（可空）；`UserRepository` trait 新增 `update_profile` / `update_password`
两个方法。Session / 邮箱限流结构不变。

---

## Rust 模型（v0.1.0 内存存储）

### User（扩展）

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};

pub struct User {
    pub id: Uuid,
    pub email: String,           // 已 normalize 为小写
    pub password_hash: String,   // bcrypt hash, cost=12
    pub nickname: String,        // 非空；注册时为空 → 默认取邮箱前缀
    pub phone: Option<String>,   // 大陆 11 位手机号；None = 未填写
    pub avatar: Option<String>,  // http(s) URL；None = 未设置
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Session（不变）

```rust
pub struct Session {
    pub id: String,               // 256-bit 随机 token (hex 编码)
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
```

### RateLimitEntry（不变，邮箱维度）

```rust
pub struct RateLimitEntry {
    pub fail_count: u32,
    pub first_fail_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
}
```

### IpRateLimitEntry（新增，IP 维度窗口限流）

```rust
pub struct IpRateLimitEntry {
    pub window_start: DateTime<Utc>,   // 窗口起点（1 分钟滑动窗口）
    pub count: u32,                     // 窗口内请求计数
}
```

### Repository Traits（UserRepository 扩展）

```rust
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn insert(&self, user: User) -> Result<User, RepoError>;
    async fn find_by_email(&self, email: &str) -> Option<User>;
    async fn find_by_id(&self, id: Uuid) -> Option<User>;

    /// 更新昵称/手机号/头像，返回更新后的 User；用户不存在返回 None。
    async fn update_profile(
        &self,
        id: Uuid,
        nickname: String,
        phone: Option<String>,
        avatar: Option<String>,
    ) -> Option<User>;

    /// 更新密码 hash（修改密码），返回是否成功。
    async fn update_password(&self, id: Uuid, new_hash: String) -> bool;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, session: Session) -> Result<Session, RepoError>;
    async fn find_by_id(&self, id: &str) -> Option<Session>;
    async fn delete(&self, id: &str) -> bool;
    async fn delete_by_user(&self, user_id: Uuid) -> u32;  // 登出所有设备
}
```

### In-Memory 实现（不变，新增方法沿用同一 HashMap）

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

> `update_profile` / `update_password` 在 `users` map 上做「读 → 改 → 写回」，并刷新
> `updated_at`。并发安全由 `RwLock` 保证；同一用户并发更新时后者覆盖（v0.1 单用户编辑，
> 可接受）。

---

## PostgreSQL 表结构（v0.2+ 参考设计）

### users table（新增三列）

| 列 | 类型 | 约束 | 说明 |
|----|------|------|------|
| id | uuid | PK | UUID v4 |
| email | text | unique, not null | 已 normalize 小写 |
| password_hash | text | not null | bcrypt hash, cost=12 |
| nickname | text | not null | 默认取邮箱前缀，service 层保证非空 |
| phone | text | nullable | `^1[3-9]\d{9}$`，NULL=未填写 |
| avatar | text | nullable | `http(s)://` URL，NULL=未设置 |
| created_at | timestamptz | not null, default now() | |
| updated_at | timestamptz | not null, default now() | 触发器自动更新 |

```sql
CREATE TABLE users (
    id            UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    email         TEXT         NOT NULL UNIQUE,
    password_hash TEXT         NOT NULL,
    nickname      TEXT         NOT NULL,
    phone         TEXT,
    avatar        TEXT,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

### sessions table（不变）

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

### login_attempts table（不变，邮箱限流持久化）

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

### ip_rate_limits table（新增，IP 窗口限流持久化）

| 列 | 类型 | 约束 | 说明 |
|----|------|------|------|
| ip | text | PK | 客户端 IP（直连取 ConnectInfo，反代取 X-Forwarded-For 首段） |
| window_start | timestamptz | not null | 当前窗口起点 |
| count | int | not null, default 0 | 窗口内计数 |

```sql
CREATE TABLE ip_rate_limits (
    ip           TEXT         PRIMARY KEY,
    window_start TIMESTAMPTZ  NOT NULL,
    count        INTEGER      NOT NULL DEFAULT 0
);
```

---

## Indexes

### users

| 索引 | 列 | 类型 | 说明 |
|------|-----|------|------|
| `users_pkey` | id | unique | PK 自带 |
| `users_email_key` | email | unique | 邮箱唯一性校验 |

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

> 无新增索引需求：profile 更新按 PK 定位；phone/avatar/nickname 不做查询条件。

---

## 数据生命周期

### 资料更新

- 修改资料 / 密码 → `update_profile` / `update_password` 原地更新内存 User，刷新 `updated_at`。
- 修改密码**不吊销已有会话**（PRD out-of-scope：多设备会话管理 / 会话撤销清单）。

### Session 过期清理

- **v0.1.0 内存**：访问时惰性检查 `expires_at`，过期则删除并返回 401。可选后台 task 定期扫描清理。
- **v0.2+ PostgreSQL**：定时 cron job `DELETE FROM sessions WHERE expires_at < now()`。

### Session 吊销

- 登出：`SessionRepository.delete(session_id)` —— 单设备登出。
- 改密码 / 安全事件（v0.2+，本次不做）：`SessionRepository.delete_by_user(user_id)` —— 全设备登出。

### 数据不持久化（v0.1.0）

- v0.1.0 内存存储，服务重启数据丢失（用户需重新注册）。这是 v0.1.0 的已知限制，v0.2+ 引入 PostgreSQL 后解决。资料修改同样不跨重启保留（PRD 验收「刷新页面数据不丢失（重启服务除外）」）。
