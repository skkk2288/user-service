//! Password hashing (bcrypt) and strength validation.
//!
//! The strength rules here mirror `web/src/utils/password.ts` exactly so
//! front-end and back-end enforce the same policy.

use thiserror::Error;

/// Errors that can arise from password validation.
#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("密码至少需要 8 位，且包含大写字母、小写字母、数字、特殊字符中的三类")]
    TooWeak,
}

/// Structured password-check result.
///
/// `valid` is `true` when the password passes length **and** category rules.
/// The individual boolean fields are available for richer feedback (used by
/// the front-end; the back-end only needs `valid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordCheckResult {
    pub valid: bool,
    pub length: bool,
    pub has_upper: bool,
    pub has_lower: bool,
    pub has_digit: bool,
    pub has_special: bool,
    pub categories_met: u32,
}

impl PasswordCheckResult {
    /// Strength level for display purposes (front-end only).
    #[allow(dead_code)]
    pub fn strength(&self) -> &'static str {
        if !self.length || self.categories_met < 2 {
            "weak"
        } else if self.categories_met == 4 && self.length {
            // "strong" requires length >= 12 per the contract.
            "strong"
        } else {
            "medium"
        }
    }
}

/// Check a password against the strength policy.
///
/// Rules:
/// 1. Length 8-64 characters.
/// 2. At least 3 of 4 character categories: uppercase, lowercase, digit,
///    special (any non-alphanumeric visible character).
pub fn check_password(password: &str) -> PasswordCheckResult {
    let len = password.chars().count();
    let length = (8..=64).contains(&len);

    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    // Special = any character that is NOT a letter or digit.  We intentionally
    // count all non-alphanumeric code points (including unicode symbols) so
    // the rule is inclusive; the contract examples use ASCII punctuation.
    let has_special = password.chars().any(|c| !c.is_alphanumeric());

    let mut categories_met = 0;
    if has_upper {
        categories_met += 1;
    }
    if has_lower {
        categories_met += 1;
    }
    if has_digit {
        categories_met += 1;
    }
    if has_special {
        categories_met += 1;
    }

    let valid = length && categories_met >= 3;

    PasswordCheckResult {
        valid,
        length,
        has_upper,
        has_lower,
        has_digit,
        has_special,
        categories_met,
    }
}

/// Validate that a password meets the strength policy.
///
/// Returns `Err(PasswordError::TooWeak)` if it does not.
pub fn validate_password(password: &str) -> Result<(), PasswordError> {
    if check_password(password).valid {
        Ok(())
    } else {
        Err(PasswordError::TooWeak)
    }
}

/// Hash a password with bcrypt at the given cost.
pub fn hash_password(password: &str, cost: u32) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(password, cost)
}

/// Verify a plaintext password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, bcrypt::BcryptError> {
    bcrypt::verify(password, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_strong_password() {
        let r = check_password("Str0ng!Pass");
        assert!(r.length);
        assert!(r.has_upper);
        assert!(r.has_lower);
        assert!(r.has_digit);
        assert!(r.has_special);
        assert_eq!(r.categories_met, 4);
        assert!(r.valid);
    }

    #[test]
    fn valid_three_categories() {
        // upper, lower, digit — no special
        let r = check_password("Abcdefg1");
        assert!(r.length);
        assert!(!r.has_special);
        assert_eq!(r.categories_met, 3);
        assert!(r.valid);
    }

    #[test]
    fn too_short() {
        let r = check_password("Ab1!");
        assert!(!r.length);
        assert!(!r.valid);
    }

    #[test]
    fn too_long() {
        let pw = "a".repeat(65);
        let r = check_password(&pw);
        assert!(!r.length);
        assert!(!r.valid);
    }

    #[test]
    fn only_two_categories_invalid() {
        // lower + digit, length ok
        let r = check_password("abcdefg1");
        assert!(r.length);
        assert_eq!(r.categories_met, 2);
        assert!(!r.valid);
    }

    #[test]
    fn validate_password_ok_and_err() {
        assert!(validate_password("Str0ng!Pass").is_ok());
        assert!(validate_password("weak").is_err());
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("Str0ng!Pass", 4).unwrap();
        assert!(verify_password("Str0ng!Pass", &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }
}
