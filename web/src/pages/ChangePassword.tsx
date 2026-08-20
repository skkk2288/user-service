import { useState } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { changePassword } from '@/api/auth';
import { checkPasswordStrength } from '@/utils/password';
import type { ApiError } from '@/api/auth';

export default function ChangePassword() {
  const navigate = useNavigate();

  const [oldPassword, setOldPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmNewPassword, setConfirmNewPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  // 新密码强度实时反馈（与注册页同规则）
  const pwdCheck = checkPasswordStrength(newPassword);

  const newMismatch = confirmNewPassword.length > 0 && newPassword !== confirmNewPassword;

  const canSubmit =
    oldPassword.length > 0 &&
    pwdCheck.valid &&
    !newMismatch &&
    !loading;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;

    setError('');
    setLoading(true);

    try {
      await changePassword({
        old_password: oldPassword,
        new_password: newPassword,
      });
      // 成功 -> 提示并跳转资料页（旧密码已立即失效）
      navigate('/profile', {
        state: { passwordChanged: true },
      });
    } catch (err) {
      const apiErr = err as ApiError;
      setError(apiErr.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="auth-container">
      <h1>修改密码</h1>

      {error && <div className="alert alert-error">{error}</div>}

      <form onSubmit={handleSubmit} noValidate>
        <div className="form-field">
          <label htmlFor="oldPassword">原密码</label>
          <input
            id="oldPassword"
            type="password"
            value={oldPassword}
            onChange={(e) => setOldPassword(e.target.value)}
            autoComplete="current-password"
            required
          />
        </div>

        <div className="form-field">
          <label htmlFor="newPassword">新密码</label>
          <input
            id="newPassword"
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            placeholder="8-64 位，含大写/小写/数字/特殊字符中的三类"
            autoComplete="new-password"
            required
          />

          {/* 密码强度实时反馈（复用注册页样式） */}
          {newPassword.length > 0 && (
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
                <li className={pwdCheck.categoriesMet >= 3 ? 'met' : 'unmet'}>
                  {pwdCheck.categoriesMet >= 3 ? '✓' : '✗'} 满足三类（{pwdCheck.categoriesMet}/4）
                </li>
              </ul>
            </div>
          )}
        </div>

        <div className="form-field">
          <label htmlFor="confirmNewPassword">确认新密码</label>
          <input
            id="confirmNewPassword"
            type="password"
            value={confirmNewPassword}
            onChange={(e) => setConfirmNewPassword(e.target.value)}
            placeholder="再次输入新密码"
            autoComplete="new-password"
            required
          />
          {newMismatch && (
            <span className="field-error">两次输入的新密码不一致</span>
          )}
        </div>

        <button type="submit" disabled={!canSubmit}>
          {loading ? '提交中…' : '确认修改'}
        </button>
      </form>

      <p className="auth-link">
        <Link to="/profile">返回个人资料</Link>
      </p>
    </div>
  );
}
