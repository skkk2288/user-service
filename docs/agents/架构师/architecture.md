# Architecture: 用户注册 + 个人资料管理

## 1. 概述

本需求为 user-service 增加「个人资料管理」能力，并扩展注册流程。**基线**（main @046ac9f）
已有：邮箱+密码注册、登录、登出、`GET /api/me`、服务端会话（签名 Cookie）、bcrypt cost=12、
邮箱维度登录失败限流，以及 42 个 auth 集成测试。本次**增量**：

- `User` 模型扩展 `nickname` / `phone` / `avatar` 三个字段。
- `POST /api/register` 扩展支持可选 `nickname`（不填默认取邮箱前缀）。
- `GET /api/me` 扩展返回 `nickname` / `phone` / `avatar`。
- 新增 `PUT /api/me/profile`（改昵称 / 手机号 / 头像）。
- 新增 `PUT /api/me/password`（改密码，成功后旧密码立即失效）。
- 前端新增 `/profile`、`/change-password` 两页，注册页加昵称 + 确认密码。
- 注册 / 登录接口新增 IP 维度限流（20 次/分钟/IP），防批量注册。

技术栈沿用现有：后端 Rust + Axum，前端 React + TypeScript + Vite，内存 Repository
接口抽象（v0.2 可无缝切 PostgreSQL）。**已实现的 register/login/logout/me 不重写**，
只做扩展与增量。

## 2. 模块划分

| 模块 | Owner | 职责 |
|------|-------|------|
| `src/models/user.rs` | @后端 | `User` 增加 `nickname/phone/avatar`；`UserRepository` trait 增加 `update_profile` / `update_password` 及 InMemory 实现 |
| `src/routes/auth.rs` | @后端 | `register` 接收可选 `nickname`；`me` 返回扩展字段 |
| `src/routes/profile.rs` | @后端 | 新增 handler：`PUT /api/me/profile`、`PUT /api/me/password` |
| `src/services/auth_service.rs` | @后端 | 新增 `update_profile` / `change_password` 业务逻辑 + 字段校验 |
| `src/utils/validate.rs` | @后端 | 集中字段校验：昵称（1-20 字符、默认邮箱前缀）、手机号（`^1[3-9]\d{9}$`）、头像（http/https URL ≤2048）、邮箱前缀提取 |
| `src/models/rate_limit.rs` | @后端 | 扩展 IP 维度窗口限流（20 次/分钟/IP），作用于 register / login |
| `src/main.rs` | @后端 | 路由注册 `profile` 路由 + 依赖注入 |
| `web/src/pages/Register.tsx` | @前端 | 注册表单加「昵称（可选）」+「确认密码」字段 |
| `web/src/pages/Profile.tsx` | @前端 | 新增资料页：只读邮箱 + 可编辑昵称/手机号/头像 + 保存 + 修改密码入口 + 注销按钮 |
| `web/src/pages/ChangePassword.tsx` | @前端 | 新增修改密码页：原密码/新密码/确认新密码 |
| `web/src/api/auth.ts` | @前端 | 新增 `updateProfile` / `changePassword` 封装；`MeResponse` 扩展字段 |
| `web/src/App.tsx` | @前端 | 新增 `/profile`、`/change-password` 路由（均 ProtectedRoute）+ 导航 |
| `web/src/contexts/AuthContext.ts` | @前端 | `AuthUser` 扩展字段；资料保存后刷新用户状态 |

## 3. 关键决策

### 3.1 头像方案：URL 字符串（不做 base64 / DataURL）

- **选择 URL 字符串**，不做文件上传，不用 base64/DataURL。
- **理由**：PRD 明确 v0.1 不做真实文件上传。URL 字符串存储干净、无体积膨胀；
  base64/DataURL 会把图片数据内联进 JSON（体积 ×1.33 且有长度压力），且在内存 Map 里
  放大内存占用，收益为零。v0.2 接真实上传后再落 CDN URL，字段形状不变，无迁移成本。
- **校验**：仅允许 `http` / `https` 协议，长度 ≤ 2048；空串 / null 表示清除头像。

### 3.2 手机号校验：11 位大陆号段（可选字段）

- **规则**：`^1[3-9]\d{9}$`（11 位数字，第二位 3-9）。空值表示未填写，合法。
- **理由**：PRD 假设 + 目标用户是国内 demo 场景；放开国际格式（E.164）会增加校验复杂度
  且当前无国际用户需求。v0.2 需要时再扩展。

### 3.3 昵称：可选，后端归一化默认邮箱前缀

- 注册时昵称可选；**后端**收到空 / 纯空白昵称时，默认取邮箱前缀（`@` 之前部分）落库。
- **理由**：默认值逻辑放后端做单一事实源，前端只负责预填展示（PRD 要求"默认取邮箱前缀"）。
  前端不填 → 前端输入框预填邮箱前缀（体验），后端仍做兜底归一化（正确性）。
- **规则**：trim 后 1-20 字符，禁止控制字符；超长 / 含控制字符 → 400 `invalid_field`。

### 3.4 确认密码：纯前端校验，后端不接收

- 注册页与改密码页的「确认密码」字段**仅前端比对**，后端 request 结构**不含** confirm 字段。
- **理由**：确认密码的用途是防手误，不是安全边界；后端只认最终 `password` 字段，
  减少无效字段与校验分支。前端两次不一致 → 行内提示、不发请求。

### 3.5 改密码会话语义：旧密码立即失效，会话保持有效

- 改成功后重写 `password_hash`，用**旧密码登录立即被拒**（验收标准 8）。
- **已有会话保持有效**，不做会话吊销。
- **理由**：PRD out-of-scope 明确「多设备会话管理、会话撤销清单」不做。验收标准的
  「旧密码立即失效」指登录凭据，不指已签发会话。此实现最小且完全满足验收。

### 3.6 原密码错误 → 400 `invalid_old_password`（不用 401）

- 用户已通过 `AuthUser` 认证，401 语义是「未登录」，会误导前端。
- 独立错误码 `invalid_old_password` 便于前端在输入框下行内提示。

### 3.7 密码策略：沿用现有 3/4 类策略，不改动

- 沿用现有 `8-64 字符，大写/小写/数字/特殊字符四类中至少三类`（`src/utils/password.rs`）。
- **理由**：该策略是 PRD「8-64 字符，含字母和数字」的严格超集，且已被 42 个既有测试
  锁定。改弱会破坏既有测试与既有安全承诺，不必要。QA 用例请用满足 3/4 类规则的密码
  （如 `Str0ng!Pass`）。

### 3.8 限流：新增 IP 维度窗口限流（并行于邮箱维度）

- **机制**：内存窗口计数，每 IP 每分钟最多 **20 次**请求（register 与 login 各自独立计数），
  超限返回 429 `too_many_attempts`。
- **与既有邮箱失败锁定并行**：邮箱维度（5 次失败 → 15 分钟）继续防单账户暴力破解；
  IP 维度新增防批量注册（邮箱维度拦不住用新邮箱轰炸的脚本）。
- **理由**：PRD 非功能要求「注册/登录接口加简单限流（单 IP 每分钟 20 次）防批量注册」。
  v0.1 单实例 + 内存实现足够；本地 dev 同 IP 下 20/min 对正常测试非常宽松。
- **注意**：IP 提取用 `ConnectInfo`（直连）或 `X-Forwarded-For` 首段（有反代时），
  需在 main.rs 开启 `IntoMakeServiceWithConnectInfo`。

### 3.9 `PUT /api/me/profile` 部分更新语义

- 请求字段全部可选：**缺席 = 保持不变**。
- 显式空值语义：`nickname=""` → 重置为邮箱前缀；`phone=""` → 清除（null）；
  `avatar=""` → 清除（null）。
- **理由**：资料页保存时通常只改部分字段；显式空值语义让「清除手机号/头像」可表达。

## 4. 数据流

### 4.1 注册流程（扩展）

```
用户 → [Register.tsx 表单（邮箱/密码/确认密码/昵称可选）]
  → 前端校验：密码强度 + 两次密码一致 + 昵称长度
  → POST /api/register { email, password, nickname? }
    → IP 限流检查（20/min/IP）
    → 后端校验：邮箱格式 + 密码强度 + 昵称（1-20，空→邮箱前缀）
    → UserRepo.find_by_email() 唯一性检查
    → bcrypt::hash(password, 12)
    → User { id, email, nickname, phone: None, avatar: None, ... }
    → UserRepo.insert()
    → 创建 session（TTL=2h，注册即登录）→ SessionRepo.insert()
    → Set-Cookie: sid=...
    → 200 { user_id, email, nickname }
  → 前端 AuthContext.setUser → 跳 /profile
```

### 4.2 资料页查看与修改

```
用户 → [/profile (ProtectedRoute)]
  → 挂载时 GET /api/me → 200 { user_id, email, nickname, phone, avatar }
  → 表单预填（邮箱只读，昵称/手机号/头像可编辑）
  → 用户点「保存」→ PUT /api/me/profile { nickname?, phone?, avatar? }
    → AuthUser 认证（无效 → 401 unauthenticated）
    → 后端校验 → UserRepo.update_profile()
    → 200 { user_id, email, nickname, phone, avatar }
  → 前端更新 AuthContext 状态 + 行内成功提示（3 秒内）
```

### 4.3 修改密码

```
用户 → [/change-password (ProtectedRoute)]
  → PUT /api/me/password { old_password, new_password }
    → AuthUser 认证（无效 → 401）
    → 新密码强度校验（weak → 400 weak_password）
    → bcrypt.verify(old_password, hash)（失败 → 400 invalid_old_password）
    → bcrypt::hash(new_password, 12) → UserRepo.update_password()
    → 200 { message: "密码已修改" }
  → 前端提示成功 → 跳 /profile
  → 之后用旧密码登录 → 401 invalid_credentials（旧密码已失效）
```

### 4.4 未登录访问保护页

```
请求 /profile 或 PUT /api/me/* → AuthUser 提取器
  → 无 sid cookie / 签名校验失败 / session 不存在 → 401 unauthenticated
  → 前端 401 → 跳 /login
```

## 5. 错误处理

### 5.1 格式

沿用现有统一格式：

```json
{ "error": "<error_code>", "message": "<human_readable_message>" }
```

### 5.2 错误码表（新增/复用）

| HTTP | error code | 场景 | message |
|------|-----------|------|---------|
| 400 | `invalid_field` | 昵称超长/含控制字符、手机号格式错、头像 URL 非法 | 字段校验失败：{具体原因} |
| 400 | `invalid_old_password` | 修改密码时原密码错误 | 原密码错误 |
| 400 | `weak_password` | 新密码强度不足（复用现有码） | 密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类 |
| 400 | `validation_error` | 请求体格式错误（复用） | 请求参数不正确 |
| 401 | `unauthenticated` | 未登录访问 /api/me/*（复用） | 请先登录 |
| 409 | `email_already_exists` | 邮箱已注册（复用） | 该邮箱已注册 |
| 429 | `too_many_attempts` | IP 限流命中（复用码，message 区分场景） | 请求过于频繁，请稍后再试 |

## 6. 安全考虑

- **新字段后端校验为最终防线**：昵称 trim + 1-20 长度 + 禁止控制字符；手机号正则；
  头像仅允许 `http(s)://` 协议 + ≤2048 字符。不信任前端。
- **头像 URL 协议白名单**：前端渲染 `<img src>` 时后端已保证仅 http/https，
  杜绝 `javascript:` 等危险协议。URL 不用于任何服务端请求（无 SSRF 面）。
- **密码**：改密码仍走 bcrypt（新 hash 覆盖旧 hash），明文不落盘、不出现在任何响应。
- **错误不泄露信息**：改密码失败统一 `invalid_old_password`（用户已认证，不存在枚举面）。
- **IP 限流**：防批量注册；与邮箱失败锁定并行防御暴力破解。
- **会话**：沿用 HttpOnly + SameSite=Lax + Secure（dev 用 COOKIE_SECURE=false）。
- **前端行内提示**：校验错误输入框下方展示，不弹窗、不清空已填字段（PRD UI 期望）。

## 7. 性能考虑

| 接口 | 预期耗时 | 说明 |
|------|---------|------|
| PUT /api/me/profile | < 10ms | 内存读改写，无 bcrypt |
| PUT /api/me/password | ~250ms | bcrypt hash cost=12（与登录验证同量级） |
| GET /api/me | < 10ms | 内存查询 |
| POST /api/register | ~300ms | bcrypt hash ~250ms + 内存写入 |

- 全部满足 PRD「接口响应 < 200ms（P95）」，除 bcrypt 相关（注册/登录/改密码）固有 ~250ms，
  与既有设计一致（安全必要开销，登录/改密码低频）。
- IP 限流为内存计数，无额外 IO。v0.1 单实例，无缓存层、无水平扩展需求。
