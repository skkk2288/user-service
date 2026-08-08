# API Contract: 用户注册登录功能

Base URL: `/api`

所有请求/响应均为 `application/json`（除 cookie 外）。

---

## Endpoints

### POST /api/register

用户注册。注册成功后自动创建会话（即自动登录），通过 `Set-Cookie` 下发 session cookie。

**Request:**
```json
{
  "email": "user@example.com",
  "password": "Str0ng!Pass"
}
```

**Response 200:**
```
Set-Cookie: sid=<signed_session_id>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=7200
```
```json
{
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com"
}
```

**Response 400:**
```json
{ "error": "invalid_email", "message": "邮箱格式不正确" }
```
```json
{ "error": "weak_password", "message": "密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类" }
```
```json
{ "error": "validation_error", "message": "请求参数不正确" }
```

**Response 409:**
```json
{ "error": "email_already_exists", "message": "该邮箱已注册" }
```

---

### POST /api/login

用户登录。通过 `Set-Cookie` 下发 session cookie。

**Request:**
```json
{
  "email": "user@example.com",
  "password": "Str0ng!Pass",
  "remember_me": false
}
```

**Response 200:**
```
remember_me=false:
Set-Cookie: sid=<signed_session_id>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=7200

remember_me=true:
Set-Cookie: sid=<signed_session_id>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800
```
```json
{
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com"
}
```

**Response 400:**
```json
{ "error": "validation_error", "message": "请求参数不正确" }
```

**Response 401:**
```json
{ "error": "invalid_credentials", "message": "邮箱或密码错误" }
```

**Response 429:**
```json
{ "error": "too_many_attempts", "message": "登录尝试次数过多，请 15 分钟后再试" }
```

---

### POST /api/logout

用户登出。清除服务端 session，清除 cookie。

**Request:** 无 body（通过 cookie 识别 session）

**Response 200:**
```
Set-Cookie: sid=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0
```
```json
{ "message": "已登出" }
```

**Response 401:**
```json
{ "error": "unauthenticated", "message": "请先登录" }
```

---

### GET /api/me

获取当前登录用户信息。需要有效的 session cookie。

**Request:** 无 body，需携带 `Cookie: sid=<signed_session_id>`

**Response 200:**
```json
{
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com"
}
```

**Response 401:**
```json
{ "error": "unauthenticated", "message": "请先登录" }
```

---

## 字段约定

### email
- 类型：`string`
- 格式：RFC 5322 合法邮箱（后端用正则校验，简化版：`^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`）
- 长度：≤ 254 字符
- 大小写：存储时转小写（normalize），比较时大小写不敏感

### password
- 类型：`string`
- 长度：8-64 字符
- 强度：至少包含以下四类中的三类：
  - 大写字母 `[A-Z]`
  - 小写字母 `[a-z]`
  - 数字 `[0-9]`
  - 特殊字符 `[^A-Za-z0-9]`（即非字母数字的可见字符）

### remember_me
- 类型：`boolean`
- 可选，默认 `false`
- `true`：session TTL = 7 天（604800 秒）
- `false`：session TTL = 2 小时（7200 秒）

### user_id
- 类型：`string`（UUID v4 格式）
- 示例：`550e8400-e29b-41d4-a716-446655440000`

### sid (cookie)
- 名称：`sid`
- 值：HMAC 签名的 session ID（`<session_id>.<hmac_signature>`）
- 属性：`HttpOnly; Secure; SameSite=Lax; Path=/`
- `Max-Age`：7200（2h）或 604800（7d）

---

## 错误响应格式

所有错误响应统一格式：

```json
{
  "error": "<error_code>",
  "message": "<human_readable_message_zh>"
}
```

| error code | HTTP | 说明 |
|-----------|------|------|
| `invalid_email` | 400 | 邮箱格式不合法 |
| `weak_password` | 400 | 密码强度不足 |
| `validation_error` | 400 | 请求体格式错误（缺字段、类型错误等） |
| `invalid_credentials` | 401 | 邮箱或密码错误（统一，不区分） |
| `unauthenticated` | 401 | 未登录访问受保护资源 |
| `email_already_exists` | 409 | 邮箱已注册 |
| `too_many_attempts` | 429 | 登录限流锁定中 |

---

## 密码强度校验规则（前后端共享）

以下规则在 `web/src/utils/password.ts`（前端）和 `src/utils/password.rs`（后端）中实现，逻辑完全一致：

```
规则：
1. 长度 >= 8 且 <= 64
2. 字符类别计数（满足 3 类即可）：
   - 大写字母：至少 1 个 [A-Z]
   - 小写字母：至少 1 个 [a-z]
   - 数字：至少 1 个 [0-9]
   - 特殊字符：至少 1 个 [^A-Za-z0-9]

校验通过条件：规则 1 满足 AND 规则 2 中至少 3 类满足
```

前端返回结构化结果（供实时反馈）：

```typescript
interface PasswordCheckResult {
  valid: boolean;          // 是否通过全部校验
  length: boolean;         // 长度 8-64
  hasUpper: boolean;       // 有大写
  hasLower: boolean;       // 有小写
  hasDigit: boolean;       // 有数字
  hasSpecial: boolean;     // 有特殊字符
  categoriesMet: number;   // 满足的类别数（0-4）
  strength: 'weak' | 'medium' | 'strong';  // 强度等级
}
```

强度等级计算（前端展示用，后端不返回）：
- `weak`：长度 < 8，或 categoriesMet < 2
- `medium`：长度 >= 8 且 categoriesMet == 2 或 3
- `strong`：长度 >= 12 且 categoriesMet == 4

---

## 环境变量（后端）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `SESSION_SECRET` | （必填） | HMAC 签名密钥，≥ 32 字节 |
| `COOKIE_SECURE` | `true` | Cookie Secure flag（dev HTTP 设 `false`） |
| `BCRYPT_COST` | `12` | bcrypt cost factor |
| `RATE_LIMIT_MAX_FAILURES` | `5` | 登录失败锁定阈值 |
| `RATE_LIMIT_LOCKOUT_MINUTES` | `15` | 锁定时长（分钟） |
| `SESSION_TTL_SHORT` | `7200` | 短期 session TTL（秒） |
| `SESSION_TTL_LONG` | `604800` | 记住我 session TTL（秒） |
| `CORS_ORIGIN` | `http://localhost:5173` | 允许的前端 origin |
