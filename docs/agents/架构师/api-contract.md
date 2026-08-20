# API Contract: 用户注册 + 个人资料管理

> 基线（main @046ac9f）已有的 `POST /api/register`、`POST /api/login`、`POST /api/logout`
> 保持不变，本次仅扩展 register 请求体与 me 响应；新增 `PUT /api/me/profile`、
> `PUT /api/me/password`。所有接口统一 JSON，错误统一 `{ error, message }`。
> 会话 cookie 由浏览器自动携带（credentials: 'include'）。

## 通用约定

- 认证：`sid` cookie（HttpOnly + SameSite=Lax + Secure；dev 用 `COOKIE_SECURE=false`）。
- 错误响应格式：

```json
{ "error": "invalid_field", "message": "昵称不能超过 20 个字符" }
```

- 受保护接口（`/api/me/*`）未登录 → `401 { "error": "unauthenticated" }`。
- 限流：`POST /api/register`、`POST /api/login` 每 IP 每分钟 20 次，超限 →
  `429 { "error": "too_many_attempts" }`。

---

## 1. POST /api/register（扩展）

> 已有接口，**新增可选 `nickname` 字段**。确认密码 `confirm_password` 为前端字段，
> 后端不接收。

**Request:**

```json
{
  "email": "alice@example.com",
  "password": "Str0ng!Pass",
  "nickname": "爱丽丝"
}
```

| 字段 | 类型 | 必填 | 规则 |
|------|------|------|------|
| `email` | string | 是 | 邮箱格式（沿用现有校验） |
| `password` | string | 是 | 8-64 字符，大写/小写/数字/特殊字符四类中至少三类 |
| `nickname` | string | 否 | 1-20 字符（trim 后）；空/空白 → 默认取邮箱前缀 |

**Response 200**（注册即登录，返回 `Set-Cookie: sid=...`）:

```json
{
  "user_id": "uuid",
  "email": "alice@example.com",
  "nickname": "爱丽丝"
}
```

**错误：**

| HTTP | error | 说明 |
|------|-------|------|
| 400 | `invalid_email` | 邮箱格式不正确 |
| 400 | `weak_password` | 密码强度不足 |
| 400 | `invalid_field` | 昵称超长 / 含控制字符 |
| 400 | `validation_error` | 请求体格式错误 |
| 409 | `email_already_exists` | 该邮箱已注册 |
| 429 | `too_many_attempts` | IP 限流命中 |

---

## 2. GET /api/me（扩展）

> 已有接口，**响应新增 `nickname` / `phone` / `avatar`**。

**Response 200:**

```json
{
  "user_id": "uuid",
  "email": "alice@example.com",
  "nickname": "爱丽丝",
  "phone": "13800138000",
  "avatar": "https://example.com/avatar.png"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `nickname` | string | 始终非空（默认为邮箱前缀） |
| `phone` | string \| null | 未填写为 `null` |
| `avatar` | string \| null | 未设置为 `null` |

**错误：** `401 unauthenticated`（未登录）。

---

## 3. PUT /api/me/profile（新增）

> 修改昵称 / 手机号 / 头像。**部分更新**：字段缺席 = 保持不变；
> 空值语义：`nickname=""` → 重置为邮箱前缀，`phone=""` → 清除，`avatar=""` → 清除。

**Request:**

```json
{
  "nickname": "新昵称",
  "phone": "13900139000",
  "avatar": "https://example.com/new-avatar.png"
}
```

| 字段 | 类型 | 必填 | 规则 |
|------|------|------|------|
| `nickname` | string | 否 | 1-20 字符；空 → 邮箱前缀 |
| `phone` | string | 否 | `^1[3-9]\d{9}$` 或空串（清除） |
| `avatar` | string | 否 | `http(s)://` URL，≤2048 字符；空串（清除） |

**Response 200**（返回更新后的完整资料）:

```json
{
  "user_id": "uuid",
  "email": "alice@example.com",
  "nickname": "新昵称",
  "phone": "13900139000",
  "avatar": "https://example.com/new-avatar.png"
}
```

**错误：**

| HTTP | error | 说明 |
|------|-------|------|
| 401 | `unauthenticated` | 未登录 |
| 400 | `invalid_field` | 任一字段校验失败（message 指明具体字段与原因） |
| 400 | `validation_error` | 请求体格式错误 |

---

## 4. PUT /api/me/password（新增）

> 修改密码，成功后旧密码立即失效（既有会话保持有效）。确认密码为前端字段，后端不接收。

**Request:**

```json
{
  "old_password": "Old!Pass123",
  "new_password": "New!Pass456"
}
```

| 字段 | 类型 | 必填 | 规则 |
|------|------|------|------|
| `old_password` | string | 是 | 当前密码，需与已存 hash 匹配 |
| `new_password` | string | 是 | 8-64 字符，四类字符至少三类 |

**Response 200:**

```json
{ "message": "密码已修改" }
```

**错误：**

| HTTP | error | 说明 |
|------|-------|------|
| 401 | `unauthenticated` | 未登录 |
| 400 | `invalid_old_password` | 原密码错误 |
| 400 | `weak_password` | 新密码强度不足 |
| 400 | `validation_error` | 请求体格式错误（缺字段等） |

---

## 5. POST /api/login / POST /api/logout（不变）

> 沿用既有契约，仅 register / login 新增 IP 限流层。登录失败锁定（5 次/邮箱 → 15 分钟）不变。

---

## 字段校验规则汇总

- `email`: 沿用现有校验（`^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`，≤254 字符）。
- `password` / `new_password`: 8-64 字符，至少包含大写字母、小写字母、数字、特殊字符中的三类。
- `nickname`: trim 后 1-20 字符；禁止控制字符（`\x00-\x1F`）；空 → 邮箱前缀（`@` 前部分）。
- `phone`: 空 或 `^1[3-9]\d{9}$`。
- `avatar`: 空 或 `http://` / `https://` 开头，总长 ≤2048。
- 前端与后端**双重执行**校验，后端为最终防线。
