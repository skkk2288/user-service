# Architecture: 用户注册登录功能

## 1. 概述

本次需求为 user-service v0.1.0 实现基础身份认证：邮箱+密码注册、密码登录、记住我（7天/短期会话）、密码强度校验。

技术选型核心决策：**服务端 Session + 签名 Cookie**（不用 JWT），**bcrypt cost=12** 密码哈希，**内存存储 + Repository 接口**（v0.1.0 不引入数据库，但接口抽象让 v0.2 无缝切换 PostgreSQL），**基于邮箱的登录限流**（5 次失败 → 15 分钟锁定）。

后端用 Rust + Axum（轻量、高性能、类型安全），前端用 React + TypeScript + Vite。前后端分离，API 以 JSON 交互。

## 2. 模块划分

| 模块 | Owner | 职责 |
|------|-------|------|
| `src/routes/auth.rs` | @后端 | 注册/登录 HTTP handler，请求校验 + 调用 service 层 |
| `src/services/auth_service.rs` | @后端 | 业务逻辑：密码哈希、session 管理、限流 |
| `src/models/user.rs` | @后端 | User 结构体 + Repository trait + InMemoryUserRepository |
| `src/models/session.rs` | @后端 | Session 结构体 + Repository trait + InMemorySessionRepository |
| `src/middleware/auth.rs` | @后端 | session cookie 解析 + 请求上下文注入 |
| `src/utils/password.rs` | @后端 | bcrypt 封装 + 密码强度校验 |
| `src/config.rs` | @后端 | 配置加载（session secret、cookie secure flag、TTL 等） |
| `src/main.rs` | @后端 | Axum router 组装 + 服务启动 |
| `web/src/pages/Register.tsx` | @前端 | 注册页面（表单 + 密码强度实时反馈） |
| `web/src/pages/Login.tsx` | @前端 | 登录页面（表单 + 记住我） |
| `web/src/api/auth.ts` | @前端 | API 封装（fetch /api/register、/api/login） |
| `web/src/utils/password.ts` | @前端 | 密码强度校验（与后端同规则，用于实时反馈） |
| `web/src/App.tsx` | @前端 | 路由 + 登录状态管理 |

## 3. 关键决策

### 3.1 密码哈希：bcrypt cost=12

- **选择 bcrypt**，不选 argon2。
- **理由**：bcrypt 经过 20+ 年实战验证，Rust 生态 `bcrypt` crate 成熟稳定。argon2 虽然更现代（抗 GPU/ASIC），但 v0.1.0 不需要这种级别防御，bcrypt cost=12（约 250ms/hash）已足够。argon2 参数调优复杂，容易误配。
- **cost=12**：在 2026 年的硬件上约 250ms/次，满足 PRD "<500ms 注册响应" 且提供足够算力成本。可通过 `BCRYPT_COST` 环境变量调整。
- **替代方案考虑**：argon2id（更安全但过度工程）、scrypt（与 bcrypt 同级但 Rust 生态弱）、PBKDF2（NIST 认可但已不推荐用于新系统）。

### 3.2 会话凭证：服务端 Session + 签名 Cookie（不用 JWT）

- **选择服务端 Session**，不选 JWT。
- **理由**：
  - Session 可即时吊销（从 store 删除即失效），JWT 无状态难以吊销（除非维护黑名单，反而更复杂）。
  - "记住我" 7 天 vs 短期会话通过 session TTL 灵活控制，JWT 需要双 token（access + refresh）机制，对 v0.1.0 过重。
  - Session ID 是随机 256-bit token，不携带任何用户信息，无信息泄露风险。
- **机制**：
  - 登录成功 → 生成 256-bit 随机 session ID → 存入 SessionRepository（含 user_id、expires_at）→ 通过 `Set-Cookie` 下发。
  - Cookie 属性：`HttpOnly` + `Secure` + `SameSite=Lax` + `Path=/`。
  - Session ID 本身不签名（因为 Server-side store 是 source of truth，伪造的 ID 查不到记录自然失效），但额外加 HMAC 签名做一层防御（防 timing attack 猜测）。
- **替代方案考虑**：JWT（无状态但吊销难）、JWT+Refresh Token（完整但 v0.1.0 过重）。

### 3.3 "记住我" 过期策略

| remember_me | Session TTL | Cookie Max-Age |
|-------------|-------------|----------------|
| `false`（默认） | 2 小时 | 2 小时（session cookie 行为） |
| `true` | 7 天 | 7 天 |

- **理由**：2 小时短期会话覆盖正常使用场景（浏览期间不中断），7 天覆盖"记住我"场景。不设 24h 是因为 2h 已足够单次会话，且更短 TTL = 更小被盗窗口。

### 3.4 登录限流：内存计数器 + 邮箱维度

- **机制**：同一邮箱连续登录失败 5 次 → 锁定 15 分钟（期间拒绝登录，返回 429）。
- **存储**：内存 HashMap<email, FailRecord{count, first_fail_at, locked_until}>。
- **重置**：登录成功立即清零；锁定期满自动解除。
- **理由**：
  - 邮箱维度（而非 IP 维度）：v0.1.0 无反向代理/CDN，IP 维度在 NAT 后会误伤。邮箱维度精准保护账户。
  - 内存存储：v0.1.0 单实例足够。Repository 接口抽象后，v0.2 可切 Redis。
  - 阈值 5 次：平衡用户体验与安全（太低误锁，太高暴力破解窗口大）。
  - 15 分钟锁定：够长以阻止自动化攻击，够短以减少用户等待。
- **注意**：限流检查在密码校验之前执行，避免锁定期间仍消耗 bcrypt 算力。

### 3.5 密码强度校验

- **规则**（注册时强制，前后端双重）：
  - 长度 8-64 字符
  - 至少包含以下四类中的三类：大写字母 `[A-Z]`、小写字母 `[a-z]`、数字 `[0-9]`、特殊字符 `[^A-Za-z0-9]`
- **前端**：实时反馈，显示满足/未满足的规则项 + 强度等级（弱/中/强）。
- **后端**：注册时强制校验，不满足返回 400 + 具体原因。
- **复用**：前端 `web/src/utils/password.ts` 和后端 `src/utils/password.rs` 共享同一套规则定义，确保一致性。

### 3.6 存储：内存 Repository 接口

- **v0.1.0**：`InMemoryUserRepository` + `InMemorySessionRepository`，用 `tokio::sync::RwLock<HashMap<...>>` 保证并发安全。
- **接口抽象**：定义 `UserRepository` / `SessionRepository` trait，业务层只依赖 trait。v0.2 切 PostgreSQL 只需新增 `PgUserRepository` impl，不改 service 层。
- **理由**：v0.1.0 不引入数据库依赖，降低部署复杂度。接口抽象让演进无成本。

## 4. 数据流

### 4.1 注册流程

```
用户 → [Register.tsx 表单]
  → 前端密码强度实时校验（password.ts）
  → POST /api/register { email, password }
    → 后端校验邮箱格式 + 密码强度
    → UserRepo.find_by_email() 检查唯一性
    → bcrypt::hash(password, 12)
    → UserRepo.insert(user)
    → 创建 session（TTL=2h，注册即登录）
    → SessionRepo.insert(session)
    → Set-Cookie: sid=...; HttpOnly; Secure; SameSite=Lax
    → 200 { user_id, email }
  → 前端跳转主页
```

### 4.2 登录流程

```
用户 → [Login.tsx 表单]
  → POST /api/login { email, password, remember_me }
    → 限流检查：RateLimiter.check(email)
      → 已锁定 → 429 { error: "too_many_attempts" }
    → UserRepo.find_by_email()
      → 不存在 → 返回统一 401（不区分）+ RateLimiter.record_fail(email)
    → bcrypt::verify(password, user.password_hash)
      → 不匹配 → 返回统一 401 + RateLimiter.record_fail(email)
    → RateLimiter.reset(email)（登录成功清零）
    → 创建 session（TTL = remember_me ? 7d : 2h）
    → SessionRepo.insert(session)
    → Set-Cookie: sid=...; HttpOnly; Secure; SameSite=Lax; Max-Age=...
    → 200 { user_id, email }
  → 前端跳转主页
```

### 4.3 受保护资源访问

```
请求 → auth middleware
  → 解析 Cookie: sid=...
  → HMAC 签名校验
  → SessionRepo.find_by_id(sid)
    → 不存在/已过期 → 401（未登录）
    → 有效 → 注入 user_id 到请求上下文
  → handler 正常处理
```

## 5. 错误处理

### 5.1 错误响应格式

所有 API 错误统一 JSON 格式：

```json
{
  "error": "<error_code>",
  "message": "<human_readable_message>"
}
```

### 5.2 错误码约定

| HTTP | error code | 场景 | message |
|------|-----------|------|---------|
| 400 | `invalid_email` | 邮箱格式不合法 | 邮箱格式不正确 |
| 400 | `weak_password` | 密码强度不足 | 密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类 |
| 400 | `validation_error` | 请求体格式错误（缺字段、类型错误） | 请求参数不正确 |
| 401 | `invalid_credentials` | 邮箱或密码错误（统一，不区分） | 邮箱或密码错误 |
| 401 | `unauthenticated` | 未登录访问受保护资源 | 请先登录 |
| 409 | `email_already_exists` | 邮箱已注册 | 该邮箱已注册 |
| 429 | `too_many_attempts` | 登录限流锁定中 | 登录尝试次数过多，请 15 分钟后再试 |

## 6. 安全考虑

- **密码存储**：bcrypt cost=12 单向哈希，不存储明文、不存储可逆加密。
- **登录错误不区分**：邮箱不存在与密码错误统一返回 401 `invalid_credentials`，防止账户枚举。
- **限流防暴力破解**：5 次/邮箱 → 15 分钟锁定。限流检查在密码校验前，避免锁定期间浪费 bcrypt 算力。
- **Cookie 安全**：`HttpOnly`（防 XSS 窃取）+ `Secure`（仅 HTTPS 传输）+ `SameSite=Lax`（防 CSRF）。
  - dev 环境 HTTP 需设 `COOKIE_SECURE=false`（仅开发用，生产必须 true）。
- **Session ID**：256-bit cryptographically secure random，HMAC 签名防篡改。
- **密码不出现在响应**：User 序列化时永远排除 password_hash 字段。
- **输入校验**：后端对所有输入做格式 + 长度校验，不信任前端。
- **CORS**：仅允许前端 origin（dev: `localhost:5173`，prod: 配置项）。

## 7. 性能考虑

| 指标 | 目标 | 说明 |
|------|------|------|
| 注册响应时间 | < 500ms | bcrypt cost=12 约 250ms，其余 < 50ms |
| 登录响应时间 | < 200ms | 正常登录：bcrypt verify ~250ms... |

**修正**：bcrypt cost=12 的 verify 与 hash 耗时相当（~250ms），超过 PRD 200ms 目标。这是 bcrypt 的固有特性（故意慢）。方案：

1. **接受 ~250ms**：PRD 200ms 目标适用于"正常路径"，密码校验是安全必要开销。登录是低频操作，250ms 用户无感。
2. 如需优化，可降 cost=11（~120ms），但降低安全强度。v0.1.0 保持 cost=12，在文档中标注实际 ~250ms。

| 接口 | 预期耗时 | 说明 |
|------|---------|------|
| POST /api/register | ~300ms | bcrypt hash ~250ms + 内存写入 < 10ms |
| POST /api/login | ~280ms | bcrypt verify ~250ms + 内存查询 < 10ms |
| 受保护资源 | < 10ms | session 查询 + HMAC 校验 |

- **并发**：内存存储用 `tokio::sync::RwLock`，读多写少场景性能足够。v0.1.0 单实例无水平扩展需求。
- **无缓存层**：session 直接内存查询，无需 Redis/缓存。
