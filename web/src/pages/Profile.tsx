import { useEffect, useState } from 'react';
import { Link, useNavigate, useLocation } from 'react-router-dom';
import { fetchMe, updateProfile } from '@/api/auth';
import type { ApiError } from '@/api/auth';
import { useAuth } from '@/hooks/useAuth';

// 与 api-contract.md 对齐的字段校验（后端为最终防线）
const NICKNAME_MAX = 20;
const PHONE_RE = /^1[3-9]\d{9}$/;
const AVATAR_MAX = 2048;

/** 是否含控制字符（\x00-\x1f） */
function hasControlChars(value: string): boolean {
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code < 0x20) return true;
  }
  return false;
}

/** 校验昵称：trim 后 1-20 字符，禁止控制字符；空串合法（= 重置为邮箱前缀） */
function validateNickname(value: string): string | null {
  if (hasControlChars(value)) {
    return '昵称不能包含控制字符';
  }
  const trimmed = value.trim();
  if (trimmed.length > NICKNAME_MAX) {
    return `昵称不能超过 ${NICKNAME_MAX} 个字符`;
  }
  return null;
}

/** 校验手机号：空 或 11 位大陆号段 */
function validatePhone(value: string): string | null {
  if (value === '' || PHONE_RE.test(value)) {
    return null;
  }
  return '手机号格式不正确（11 位大陆号段）';
}

/** 校验头像 URL：空 或 http(s):// 开头且长度 ≤2048 */
function validateAvatar(value: string): string | null {
  if (value === '') {
    return null;
  }
  if (!/^https?:\/\//.test(value)) {
    return '头像链接必须以 http:// 或 https:// 开头';
  }
  if (value.length > AVATAR_MAX) {
    return `头像链接不能超过 ${AVATAR_MAX} 个字符`;
  }
  return null;
}

export default function Profile() {
  const navigate = useNavigate();
  const location = useLocation();
  const { user, setUser, handleLogout } = useAuth();

  const [nickname, setNickname] = useState('');
  const [phone, setPhone] = useState('');
  const [avatar, setAvatar] = useState('');
  const [loading, setLoading] = useState(true);      // 挂载时拉取最新资料
  const [saving, setSaving] = useState(false);
  // 从修改密码页跳转回来时显示「密码已修改」提示
  const [success, setSuccess] = useState(
    (location.state as { passwordChanged?: boolean } | null)?.passwordChanged
      ? '密码已修改'
      : '',
  );
  const [error, setError] = useState('');

  // 挂载时 GET /api/me 拉取最新资料并预填表单（邮箱只读）
  useEffect(() => {
    let cancelled = false;
    fetchMe()
      .then((me) => {
        if (cancelled) return;
        setNickname(me.nickname);
        setPhone(me.phone ?? '');
        setAvatar(me.avatar ?? '');
      })
      .catch((err) => {
        if (cancelled) return;
        const apiErr = err as ApiError;
        setError(apiErr.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // 行内校验错误（在输入框下方展示，不清空已填字段）
  const nicknameErr = nickname === '' ? null : validateNickname(nickname);
  const phoneErr = validatePhone(phone);
  const avatarErr = validateAvatar(avatar);

  const canSubmit = !loading && !saving && !nicknameErr && !phoneErr && !avatarErr;

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;

    setError('');
    setSuccess('');
    setSaving(true);

    try {
      // 发送 trim 后的值；空串 = 清除（nickname 空串 = 重置为邮箱前缀）
      const updated = await updateProfile({
        nickname: nickname.trim(),
        phone: phone.trim(),
        avatar: avatar.trim(),
      });
      setUser(updated);  // 更新全局用户状态（结构 = AuthUser）
      setNickname(updated.nickname);
      setPhone(updated.phone ?? '');
      setAvatar(updated.avatar ?? '');
      setSuccess('资料已保存');
      // 3 秒后自动清除成功提示
      window.setTimeout(() => setSuccess(''), 3000);
    } catch (err) {
      const apiErr = err as ApiError;
      setError(apiErr.message);
    } finally {
      setSaving(false);
    }
  }

  async function handleLogoutClick() {
    await handleLogout();
    navigate('/login');
  }

  return (
    <div className="auth-container">
      <h1>个人资料</h1>

      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}

      {loading ? (
        <p className="page-note">加载中…</p>
      ) : (
        <form onSubmit={handleSave} noValidate>
          <div className="form-field">
            <label htmlFor="email">邮箱（不可修改）</label>
            <input
              id="email"
              type="email"
              value={user?.email ?? ''}
              disabled
              className="input-readonly"
            />
          </div>

          <div className="form-field">
            <label htmlFor="nickname">昵称</label>
            <input
              id="nickname"
              type="text"
              value={nickname}
              onChange={(e) => setNickname(e.target.value)}
              placeholder="1-20 个字符；留空则使用邮箱前缀"
              maxLength={NICKNAME_MAX}
              autoComplete="nickname"
            />
            {nicknameErr && <span className="field-error">{nicknameErr}</span>}
          </div>

          <div className="form-field">
            <label htmlFor="phone">手机号（可选）</label>
            <input
              id="phone"
              type="tel"
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
              placeholder="11 位大陆手机号；留空则不填写"
              autoComplete="tel"
            />
            {phoneErr && <span className="field-error">{phoneErr}</span>}
          </div>

          <div className="form-field">
            <label htmlFor="avatar">头像链接（可选）</label>
            <input
              id="avatar"
              type="url"
              value={avatar}
              onChange={(e) => setAvatar(e.target.value)}
              placeholder="http(s):// 图片地址；留空则不设置"
              autoComplete="off"
            />
            {avatarErr && <span className="field-error">{avatarErr}</span>}
            {avatar !== '' && !avatarErr && (
              <div className="avatar-preview">
                <img src={avatar} alt="头像预览" />
              </div>
            )}
          </div>

          <button type="submit" disabled={!canSubmit}>
            {saving ? '保存中…' : '保存'}
          </button>
        </form>
      )}

      <p className="auth-link">
        <Link to="/change-password">修改密码</Link>
      </p>

      <p className="auth-link">
        <button type="button" className="btn-link-danger" onClick={handleLogoutClick}>
          退出登录
        </button>
      </p>
    </div>
  );
}
