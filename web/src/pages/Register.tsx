import { useState, useMemo } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { register } from '@/api/auth';
import { checkPasswordStrength, type PasswordCheckResult } from '@/utils/password';
import type { ApiError } from '@/api/auth';
import { useAuth } from '@/hooks/useAuth';

export default function Register() {
  const navigate = useNavigate();
  const { setUser } = useAuth();

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  // 密码强度实时反馈
  const pwdCheck: PasswordCheckResult = useMemo(
    () => checkPasswordStrength(password),
    [password],
  );

  const passwordsMismatch = confirmPassword.length > 0 && password !== confirmPassword;

  const canSubmit =
    email.length > 0 &&
    pwdCheck.valid &&
    !passwordsMismatch &&
    !loading;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;

    setError('');
    setLoading(true);

    try {
      const res = await register({ email, password });
      setUser(res);  // 用 register 返回的 { user_id, email } 直接设状态
      // 注册成功 -> 后端已自动创建会话 -> 跳转主页
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
      <h1>注册</h1>

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
            placeholder="8-64 位，含大写/小写/数字/特殊字符中的三类"
            autoComplete="new-password"
            required
          />

          {/* 密码强度实时反馈 */}
          {password.length > 0 && (
            <div className="password-strength">
              <div className={`strength-bar strength-${pwdCheck.strength}`}>
                <span className="strength-label">
                  {pwdCheck.strength === 'weak' && '弱'}
                  {pwdCheck.strength === 'medium' && '中'}
                  {pwdCheck.strength === 'strong' && '强'}
                </span>
              </div>
              <ul className="strength-rules">
                <li className={pwdCheck.length ? 'met' : 'unmet'}>
                  {pwdCheck.length ? '✓' : '✗'} 长度 8-64 位
                </li>
                <li className={pwdCheck.hasUpper ? 'met' : 'unmet'}>
                  {pwdCheck.hasUpper ? '✓' : '✗'} 包含大写字母
                </li>
                <li className={pwdCheck.hasLower ? 'met' : 'unmet'}>
                  {pwdCheck.hasLower ? '✓' : '✗'} 包含小写字母
                </li>
                <li className={pwdCheck.hasDigit ? 'met' : 'unmet'}>
                  {pwdCheck.hasDigit ? '✓' : '✗'} 包含数字
                </li>
                <li className={pwdCheck.hasSpecial ? 'met' : 'unmet'}>
                  {pwdCheck.hasSpecial ? '✓' : '✗'} 包含特殊字符
                </li>
                <li className={pwdCheck.categoriesMet >= 3 ? 'met' : 'unmet'}>
                  {pwdCheck.categoriesMet >= 3 ? '✓' : '✗'} 满足三类（{pwdCheck.categoriesMet}/4）
                </li>
              </ul>
            </div>
          )}
        </div>

        <div className="form-field">
          <label htmlFor="confirmPassword">确认密码</label>
          <input
            id="confirmPassword"
            type="password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
            placeholder="再次输入密码"
            autoComplete="new-password"
            required
          />
          {passwordsMismatch && (
            <span className="field-error">两次输入的密码不一致</span>
          )}
        </div>

        <button type="submit" disabled={!canSubmit}>
          {loading ? '注册中…' : '注册'}
        </button>
      </form>

      <p className="auth-link">
        已有账号？<Link to="/login">去登录</Link>
      </p>
    </div>
  );
}
