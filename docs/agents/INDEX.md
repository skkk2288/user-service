# Agent 产物索引

| 日期 | Agent | 路径 | 说明 |
|------|-------|------|------|
| 2026-08-08 | 需求编写 | docs/agents/需求编写/prd-draft.md | 用户注册登录功能 PRD 初稿 |
| 2026-08-08 | 架构师 | docs/agents/架构师/architecture.md | 用户注册登录功能架构设计 |
| 2026-08-08 | 架构师 | docs/agents/架构师/api-contract.md | 用户注册登录 API 契约 |
| 2026-08-08 | 架构师 | docs/agents/架构师/data-model.md | 用户注册登录数据模型 |
| 2026-08-08 | 前端 | web/src/pages/Register.tsx | 注册表单 + 密码强度实时反馈 |
| 2026-08-08 | 前端 | web/src/pages/Login.tsx | 登录表单 + 记住我复选框 |
| 2026-08-08 | 前端 | web/src/api/auth.ts | API 封装（register/login/logout/me） |
| 2026-08-08 | 前端 | web/src/utils/password.ts | 密码强度校验（与后端同规则） |
| 2026-08-08 | 前端 | web/src/App.tsx | 路由 + 登录状态管理（AuthProvider） |
| 2026-08-08 | 后端 | src/config.rs | 配置加载（SESSION_SECRET / BCRYPT_COST / TTLs / CORS） |
| 2026-08-08 | 后端 | src/models/user.rs | User struct + UserRepository trait + InMemoryUserRepository |
| 2026-08-08 | 后端 | src/models/session.rs | Session struct + SessionRepository trait + InMemorySessionRepository |
| 2026-08-08 | 后端 | src/models/rate_limit.rs | 邮箱维度登录限流（5次失败 -> 15分钟锁定） |
| 2026-08-08 | 后端 | src/utils/password.rs | bcrypt hash/verify + 密码强度校验（8-64位，四类满足三类） |
| 2026-08-08 | 后端 | src/utils/session_id.rs | 256-bit 随机 session ID 生成 |
| 2026-08-08 | 后端 | src/utils/signed_cookie.rs | HMAC-SHA256 签名 cookie（sid.hmac_hex） |
| 2026-08-08 | 后端 | src/services/auth_service.rs | 注册/登录/登出/session查询 业务逻辑 |
| 2026-08-08 | 后端 | src/middleware/auth.rs | AuthUser extractor（cookie 解析 + HMAC 校验） |
| 2026-08-08 | 后端 | src/routes/auth.rs | POST /api/register, /api/login, /api/logout, GET /api/me |
| 2026-08-08 | 后端 | src/main.rs | Axum router 组装 + CORS + tracing |
| 2026-08-08 | QA | tests/auth_integration.rs | 用户注册登录集成测试（42 cases） |
| 2026-08-08 | QA | docs/agents/QA/test-report.md | 用户注册登录功能测试报告 |
| 2026-08-08 | QA | src/lib.rs | 库 crate 入口（集成测试依赖） |
| 2026-08-08 | QA | Cargo.toml | 添加 [lib] + dev-dependencies (http-body-util, tower) |
| 2026-08-14 | 后端 | src/routes/health.rs | GET /ping liveness 端点 + 单元测试 |
| 2026-08-14 | QA | tests/health_integration.rs | /ping 健康检查集成测试（7 cases） |
| 2026-08-14 | QA | docs/agents/QA/test-report.md | /ping 健康检查功能测试报告 |

| 2026-08-20 | 后端 | src/routes/auth.rs | PUT /api/me/profile, PUT /api/me/password, register nickname, GET /api/me 扩展 |
| 2026-08-20 | 后端 | src/services/auth_service.rs | 资料更新/改密码业务逻辑 + IP 维度限流 |
| 2026-08-20 | 后端 | src/models/user.rs | User 增加 nickname/phone/avatar + update_profile/update_password |
| 2026-08-20 | 后端 | src/models/rate_limit.rs | IpRateLimiter（IP 滑动窗口限流） |
| 2026-08-20 | 后端 | src/utils/profile.rs | 昵称/手机号/头像字段校验 |
| 2026-08-20 | 后端 | src/middleware/client_ip.rs | ClientIp extractor（X-Forwarded-For / ConnectInfo） |
| 2026-08-20 | 前端 | web/src/pages/Register.tsx | 注册表单扩展：可选昵称字段 + 确认密码 |
| 2026-08-20 | 前端 | web/src/pages/Profile.tsx | 资料页：只读邮箱 + 编辑昵称/手机号/头像 + 保存 + 改密入口 + 退出登录 |
| 2026-08-20 | 前端 | web/src/pages/ChangePassword.tsx | 修改密码页：原/新/确认新密码 + 强度反馈 |
| 2026-08-20 | 前端 | web/src/api/auth.ts | API 封装扩展：updateProfile/changePassword + MeResponse 新字段 |
| 2026-08-20 | 前端 | web/src/contexts/AuthContext.ts | AuthUser 扩展 nickname/phone/avatar |
| 2026-08-20 | 前端 | web/src/App.tsx | 新增 /profile、/change-password 路由 + 导航 |
