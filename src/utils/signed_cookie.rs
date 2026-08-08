//! HMAC-signed session cookie helpers.
//!
//! The session ID stored in the cookie is `<session_id>.<hmac_hex>`.  The
//! HMAC provides a second layer of defense: a forged or tampered token is
//! rejected before reaching the session store, mitigating timing-attack
//! probing against the store.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Sign a session ID with the given secret, producing `sid.hmac_hex`.
pub fn sign(session_id: &str, secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(session_id.as_bytes());
    let sig = mac.finalize().into_bytes();
    format!("{}.{}", session_id, hex::encode(sig))
}

/// Verify and extract the session ID from a signed cookie value.
///
/// Returns `Some(session_id)` if the HMAC matches, `None` otherwise.
pub fn verify_and_extract(signed: &str, secret: &str) -> Option<String> {
    let (sid, sig_hex) = signed.split_once('.')?;
    let provided = hex::decode(sig_hex).ok()?;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(sid.as_bytes());
    if mac.verify_slice(&provided).is_ok() {
        Some(sid.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-key-that-is-long-enough!!";

    #[test]
    fn sign_then_verify() {
        let signed = sign("abc123", SECRET);
        let extracted = verify_and_extract(&signed, SECRET).unwrap();
        assert_eq!(extracted, "abc123");
    }

    #[test]
    fn tampered_signature_rejected() {
        let signed = sign("abc123", SECRET);
        // Flip a character in the signature portion.
        let mut tampered = signed.clone();
        let last = tampered.chars().last().unwrap();
        let replacement = if last == 'a' { 'b' } else { 'a' };
        tampered.pop();
        tampered.push(replacement);
        assert!(verify_and_extract(&tampered, SECRET).is_none());
    }

    #[test]
    fn wrong_secret_rejected() {
        let signed = sign("abc123", SECRET);
        assert!(verify_and_extract(&signed, "wrong-secret-key-also-long-enough!").is_none());
    }

    #[test]
    fn missing_dot_returns_none() {
        assert!(verify_and_extract("noseparator", SECRET).is_none());
    }
}
