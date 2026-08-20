/**
 * API 封装：认证相关接口
 *
 * 按 api-contract.md 定义调用：
 * - POST /api/register
 * - POST /api/login
 * - POST /api/logout
 * - GET  /api/me
 * - PUT  /api/me/profile
 * - PUT  /api/me/password
 *
 * Cookie 由浏览器自动管理（credentials: 'include'），前端不操作 session cookie。
 */

const API_BASE = '/api';

/** 后端统一错误响应 */
export interface ApiError {
  error: string;
  message: string;
}

/** 注册成功响应（含 nickname，后端默认取邮箱前缀） */
export interface RegisterResponse {
  user_id: string;
  email: string;
  nickname: string;
}

/** 登录成功响应（沿用既有契约：不含资料字段） */
export interface LoginResponse {
  user_id: string;
  email: string;
}

/** /api/me 成功响应（扩展 nickname/phone/avatar） */
export interface MeResponse {
  user_id: string;
  email: string;
  nickname: string;
  phone: string | null;
  avatar: string | null;
}

export interface RegisterRequest {
  email: string;
  password: string;
  /** 可选，1-20 字符；不填由后端默认取邮箱前缀 */
  nickname?: string;
}

/** PUT /api/me/profile 请求（全部可选：缺席 = 保持不变） */
export interface UpdateProfileRequest {
  /** 1-20 字符；空串 = 重置为邮箱前缀 */
  nickname?: string;
  /** ^1[3-9]\d{9}$；空串 = 清除 */
  phone?: string;
  /** http(s):// URL ≤2048；空串 = 清除 */
  avatar?: string;
}

/** PUT /api/me/password 请求 */
export interface ChangePasswordRequest {
  old_password: string;
  new_password: string;
}

export interface LoginRequest {
  email: string;
  password: string;
  remember_me?: boolean;
}

/**
 * 将 Response 转为 ApiError（或 fallback）
 */
async function parseError(response: Response): Promise<ApiError> {
  try {
    const body = await response.json();
    if (body.error && body.message) {
      return body as ApiError;
    }
  } catch {
    // 非 JSON 响应，fallback
  }
  return {
    error: 'unknown',
    message: `请求失败（${response.status}）`,
  };
}

/**
 * 注册
 *
 * 注册成功后自动创建会话（后端下发 Set-Cookie），前端无需额外登录。
 */
export async function register(req: RegisterRequest): Promise<RegisterResponse> {
  const response = await fetch(`${API_BASE}/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify(req),
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return response.json();
}

/**
 * 登录
 *
 * 登录成功后后端下发 Set-Cookie（含 remember_me 控制的 Max-Age）。
 */
export async function login(req: LoginRequest): Promise<LoginResponse> {
  const response = await fetch(`${API_BASE}/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify(req),
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return response.json();
}

/**
 * 登出
 *
 * 后端清除服务端 session，清除 cookie。
 */
export async function logout(): Promise<void> {
  const response = await fetch(`${API_BASE}/logout`, {
    method: 'POST',
    credentials: 'include',
  });

  if (!response.ok) {
    throw await parseError(response);
  }
}

/**
 * 获取当前登录用户信息
 *
 * 需要有效的 session cookie（浏览器自动携带）。
 * 401 表示未登录或 session 过期。
 */
export async function fetchMe(): Promise<MeResponse> {
  const response = await fetch(`${API_BASE}/me`, {
    method: 'GET',
    credentials: 'include',
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return response.json();
}

/**
 * 更新个人资料（昵称/手机号/头像）
 *
 * 部分更新：未传字段保持不变；空串表示清除（nickname 空串=重置为邮箱前缀）。
 * 成功后返回更新后的完整资料。
 */
export async function updateProfile(req: UpdateProfileRequest): Promise<MeResponse> {
  const response = await fetch(`${API_BASE}/me/profile`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify(req),
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return response.json();
}

/**
 * 修改密码
 *
 * 成功后旧密码立即失效（既有会话保持有效）。
 */
export async function changePassword(req: ChangePasswordRequest): Promise<{ message: string }> {
  const response = await fetch(`${API_BASE}/me/password`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify(req),
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return response.json();
}
