import { useEffect, useState, useCallback, type ReactNode } from 'react';
import { BrowserRouter, Routes, Route, Navigate, Link } from 'react-router-dom';
import { fetchMe, logout } from '@/api/auth';
import { AuthContext, type AuthUser, type AuthContextValue } from '@/contexts/AuthContext';
import { useAuth } from '@/hooks/useAuth';
import Login from '@/pages/Login';
import Register from '@/pages/Register';
import Profile from '@/pages/Profile';
import ChangePassword from '@/pages/ChangePassword';

// ---------------------------------------------------------------------------
// AuthProvider: 登录状态管理（通过 cookie 调用 /api/me）
// ---------------------------------------------------------------------------

function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [loading, setLoading] = useState(true);

  // 应用启动时检查当前登录状态（通过 cookie）
  const refreshUser = useCallback(async () => {
    try {
      const me = await fetchMe();
      setUser(me);
    } catch {
      setUser(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshUser();
  }, [refreshUser]);

  const handleLogout = useCallback(async () => {
    await logout();
    setUser(null);
  }, []);

  // 暴露 setUser 供 Login/Register 成功后直接设状态（无需额外 /api/me 请求）
  const setUserStable = useCallback((u: AuthUser | null) => {
    setUser(u);
  }, []);

  const value: AuthContextValue = { user, loading, refreshUser, setUser: setUserStable, handleLogout };

  return (
    <AuthContext.Provider value={value}>
      {children}
    </AuthContext.Provider>
  );
}

// ---------------------------------------------------------------------------
// 受保护路由：未登录跳转 /login
// ---------------------------------------------------------------------------

function ProtectedRoute({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();

  if (loading) {
    return <div className="auth-container"><p>加载中…</p></div>;
  }

  if (!user) {
    return <Navigate to="/login" replace />;
  }

  return <>{children}</>;
}

// ---------------------------------------------------------------------------
// 公开页守卫：已登录访问 /login 自动重定向到首页
// ---------------------------------------------------------------------------

function PublicOnlyRoute({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();

  if (loading) {
    return <div className="auth-container"><p>加载中…</p></div>;
  }

  if (user) {
    return <Navigate to="/" replace />;
  }

  return <>{children}</>;
}

// ---------------------------------------------------------------------------
// 主页（已登录视图）
// ---------------------------------------------------------------------------

function Home() {
  const { user, handleLogout } = useAuth();

  return (
    <div className="auth-container">
      <h1>欢迎，{user?.nickname || user?.email}</h1>
      <p>你已登录。</p>
      <p className="auth-link">
        <Link to="/profile">个人资料</Link>
      </p>
      <button onClick={() => handleLogout()}>登出</button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// App: 路由组装
// ---------------------------------------------------------------------------

export default function App() {
  return (
    <AuthProvider>
      <BrowserRouter>
        <Routes>
          <Route
            path="/"
            element={
              <ProtectedRoute>
                <Home />
              </ProtectedRoute>
            }
          />
          <Route
            path="/profile"
            element={
              <ProtectedRoute>
                <Profile />
              </ProtectedRoute>
            }
          />
          <Route
            path="/change-password"
            element={
              <ProtectedRoute>
                <ChangePassword />
              </ProtectedRoute>
            }
          />
          <Route path="/login" element={<PublicOnlyRoute><Login /></PublicOnlyRoute>} />
          <Route path="/register" element={<Register />} />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}
