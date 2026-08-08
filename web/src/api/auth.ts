/**
 * API 封装：认证相关接口
 *
 * 按 api-contract.md 定义调用：
 * - POST /api/register
 * - POST /api/login
 * - POST /api/logout
 * - GET  /api/me
 *
 * Cookie 由浏览器自动管理（credentials: 'include'），前端不操作 session cookie。
 */

const API_BASE = '/api';

/** 后端统一错误响应 */
export interface ApiError {
  error: string;
  message: string;
}

/** 注册/登录成功响应 */
export interface AuthResponse {
  user_id: string;
  email: string;
}

/** /api/me 成功响应 */
export interface MeResponse {
  user_id: string;
  email: string;
}

export interface RegisterRequest {
  email: string;
  password: string;
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
export async function register(req: RegisterRequest): Promise<AuthResponse> {
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
export async function login(req: LoginRequest): Promise<AuthResponse> {
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
