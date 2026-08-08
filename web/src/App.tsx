import { useEffect, useState, useCallback, type ReactNode } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { fetchMe, logout } from '@/api/auth';
import { AuthContext, type AuthUser, type AuthContextValue } from '@/contexts/AuthContext';
import { useAuth } from '@/hooks/useAuth';
import Login from '@/pages/Login';
import Register from '@/pages/Register';

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

  const value: AuthContextValue = { user, loading, refreshUser, handleLogout };

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
// 主页（已登录视图）
// ---------------------------------------------------------------------------

function Home() {
  const { user, handleLogout } = useAuth();

  return (
    <div className="auth-container">
      <h1>欢迎，{user?.email}</h1>
      <p>你已登录。</p>
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
          <Route path="/login" element={<Login />} />
          <Route path="/register" element={<Register />} />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}
