import { useState } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { login } from '@/api/auth';
import type { ApiError } from '@/api/auth';
import { useAuth } from '@/hooks/useAuth';

export default function Login() {
  const navigate = useNavigate();
  const { refreshUser } = useAuth();

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const canSubmit = email.length > 0 && password.length > 0 && !loading;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;

    setError('');
    setLoading(true);

    try {
      await login({ email, password, remember_me: rememberMe });
      // 登录成功后重新拉取完整资料（昵称/手机号/头像）并更新全局状态
      await refreshUser();
      navigate('/');
    } catch (err) {
      const apiErr = err as ApiError;
      setError(apiErr.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="auth-container">
      <h1>登录</h1>

      {error && <div className="alert alert-error">{error}</div>}

      <form onSubmit={handleSubmit} noValidate>
        <div className="form-field">
          <label htmlFor="email">邮箱</label>
          <input
            id="email"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="user@example.com"
            autoComplete="email"
            required
          />
        </div>

        <div className="form-field">
          <label htmlFor="password">密码</label>
          <input
            id="password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            required
          />
        </div>

        <div className="form-field form-field-checkbox">
          <label>
            <input
              type="checkbox"
              checked={rememberMe}
              onChange={(e) => setRememberMe(e.target.checked)}
            />
            记住我（7 天免登录）
          </label>
        </div>

        <button type="submit" disabled={!canSubmit}>
          {loading ? '登录中…' : '登录'}
        </button>
      </form>

      <p className="auth-link">
        没有账号？<Link to="/register">去注册</Link>
      </p>
    </div>
  );
}
