//! Profile field validation: nickname, phone, avatar.
//!
//! Mirrors the rules in `web/src/pages/Profile.tsx` and `api-contract.md`;
//! the back-end is the final line of defense, so these are enforced here.

use thiserror::Error;

/// Maximum nickname length (trimmed).
pub const NICKNAME_MAX: usize = 20;
/// Maximum avatar URL length.
pub const AVATAR_MAX: usize = 2048;

/// Validation failures for profile fields, each carrying a user-facing message.
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("昵称不能包含控制字符")]
    ControlChars,
    #[error("昵称不能超过 20 个字符")]
    NicknameTooLong,
    #[error("手机号格式不正确（11 位大陆号段）")]
    InvalidPhone,
    #[error("头像链接必须以 http:// 或 https:// 开头")]
    InvalidAvatar,
    #[error("头像链接不能超过 2048 个字符")]
    AvatarTooLong,
}

/// Default nickname: the email prefix (text before `@`).
pub fn default_nickname(email: &str) -> String {
    email.split('@').next().unwrap_or("").to_string()
}

/// Validate a (trimmed, non-empty) nickname: no control chars, ≤ 20 chars.
pub fn validate_nickname(nickname: &str) -> Result<(), ProfileError> {
    if nickname.chars().any(|c| (c as u32) < 0x20) {
        return Err(ProfileError::ControlChars);
    }
    if nickname.chars().count() > NICKNAME_MAX {
        return Err(ProfileError::NicknameTooLong);
    }
    Ok(())
}

/// Normalize a phone value. Empty string clears the field (`None`); otherwise
/// it must match the 11-digit mainland format `1[3-9]\d{9}`.
pub fn normalize_phone(phone: &str) -> Result<Option<String>, ProfileError> {
    if phone.is_empty() {
        return Ok(None);
    }
    if !is_valid_phone(phone) {
        return Err(ProfileError::InvalidPhone);
    }
    Ok(Some(phone.to_string()))
}

/// Normalize an avatar URL. Empty string clears the field (`None`); otherwise
/// it must be an `http(s)://` URL of length ≤ 2048.
pub fn normalize_avatar(avatar: &str) -> Result<Option<String>, ProfileError> {
    if avatar.is_empty() {
        return Ok(None);
    }
    if !avatar.starts_with("http://") && !avatar.starts_with("https://") {
        return Err(ProfileError::InvalidAvatar);
    }
    if avatar.len() > AVATAR_MAX {
        return Err(ProfileError::AvatarTooLong);
    }
    Ok(Some(avatar.to_string()))
}

/// `true` if `phone` matches the 11-digit mainland format `1[3-9]\d{9}`.
fn is_valid_phone(phone: &str) -> bool {
    let bytes = phone.as_bytes();
    if bytes.len() != 11 {
        return false;
    }
    if bytes[0] != b'1' {
        return false;
    }
    if !(b'3'..=b'9').contains(&bytes[1]) {
        return false;
    }
    bytes.iter().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_nickname_from_email() {
        assert_eq!(default_nickname("alice@example.com"), "alice");
        assert_eq!(default_nickname("a.b+c@d.co"), "a.b+c");
        assert_eq!(default_nickname("no-at-sign"), "no-at-sign");
    }

    #[test]
    fn nickname_validation() {
        assert!(validate_nickname("爱丽丝").is_ok());
        assert!(validate_nickname("a".repeat(20).as_str()).is_ok());
        assert!(validate_nickname("a".repeat(21).as_str()).is_err());
        assert!(validate_nickname("has\ncontrol").is_err());
    }

    #[test]
    fn phone_normalization() {
        assert_eq!(normalize_phone("").unwrap(), None);
        assert_eq!(
            normalize_phone("13800138000").unwrap(),
            Some("13800138000".into())
        );
        assert!(normalize_phone("2380013800").is_err()); // wrong length
        assert!(normalize_phone("12800138000").is_err()); // second digit 2
        assert!(normalize_phone("1380013800a").is_err()); // non-digit
        assert!(normalize_phone("128001380001").is_err()); // too long
    }

    #[test]
    fn avatar_normalization() {
        assert_eq!(normalize_avatar("").unwrap(), None);
        assert!(normalize_avatar("https://example.com/a.png").is_ok());
        assert!(normalize_avatar("http://example.com/a.png").is_ok());
        assert!(normalize_avatar("ftp://example.com/a.png").is_err());
        assert!(normalize_avatar("javascript:alert(1)").is_err());
        assert!(normalize_avatar(format!("https://e.com/{}", "a".repeat(2100)).as_str()).is_err());
    }
}
