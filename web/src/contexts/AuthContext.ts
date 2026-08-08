import { createContext } from 'react';

export interface AuthUser {
  user_id: string;
  email: string;
}

export interface AuthContextValue {
  user: AuthUser | null;
  loading: boolean;       // 初始化时检查 /api/me
  refreshUser: () => Promise<void>;
  setUser: (user: AuthUser | null) => void;  // 登录/注册成功后直接设状态
  handleLogout: () => Promise<void>;
}

export const AuthContext = createContext<AuthContextValue | undefined>(undefined);
