//! Configuration loaded from environment variables.
//!
//! All settings have sensible defaults except `SESSION_SECRET`, which is
//! required and must be >= 32 bytes for HMAC security.

use std::env;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("SESSION_SECRET is required")]
    MissingSecret,
    #[error("SESSION_SECRET must be at least 32 bytes, got {0}")]
    SecretTooShort(usize),
    #[error("invalid value for {key}: {value}")]
    InvalidValue { key: &'static str, value: String },
}

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// HMAC signing key for session cookie. Must be >= 32 bytes.
    pub session_secret: String,
    /// Cookie `Secure` flag. Set to `false` for local HTTP dev.
    pub cookie_secure: bool,
    /// bcrypt cost factor.
    pub bcrypt_cost: u32,
    /// Max login failures before lockout.
    pub rate_limit_max_failures: u32,
    /// Lockout duration in minutes.
    pub rate_limit_lockout_minutes: u32,
    /// Max requests per IP per minute for register/login.
    pub rate_limit_ip_per_minute: u32,
    /// Short-lived session TTL in seconds (remember_me=false).
    pub session_ttl_short: u64,
    /// Long-lived session TTL in seconds (remember_me=true).
    pub session_ttl_long: u64,
    /// Allowed CORS origin.
    pub cors_origin: String,
    /// Server listen address.
    pub listen_addr: String,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// Reads `.env` first (via `dotenvy::dotenv`), then environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        // .env is optional; ignore error if file doesn't exist.
        let _ = dotenvy::dotenv();

        let session_secret = env::var("SESSION_SECRET").map_err(|_| ConfigError::MissingSecret)?;
        if session_secret.len() < 32 {
            return Err(ConfigError::SecretTooShort(session_secret.len()));
        }

        let cookie_secure = parse_bool_env("COOKIE_SECURE", true)?;
        let bcrypt_cost = parse_u32_env("BCRYPT_COST", 12)?;
        let rate_limit_max_failures = parse_u32_env("RATE_LIMIT_MAX_FAILURES", 5)?;
        let rate_limit_lockout_minutes = parse_u32_env("RATE_LIMIT_LOCKOUT_MINUTES", 15)?;
        let rate_limit_ip_per_minute = parse_u32_env("RATE_LIMIT_IP_PER_MINUTE", 20)?;
        let session_ttl_short = parse_u64_env("SESSION_TTL_SHORT", 7200)?;
        let session_ttl_long = parse_u64_env("SESSION_TTL_LONG", 604_800)?;
        let cors_origin =
            env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());
        let listen_addr = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());

        Ok(Self {
            session_secret,
            cookie_secure,
            bcrypt_cost,
            rate_limit_max_failures,
            rate_limit_lockout_minutes,
            rate_limit_ip_per_minute,
            session_ttl_short,
            session_ttl_long,
            cors_origin,
            listen_addr,
        })
    }
}

fn parse_bool_env(key: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(key) {
        Ok(v) => v
            .to_lowercase()
            .parse::<bool>()
            .map_err(|_| ConfigError::InvalidValue { key, value: v }),
        Err(_) => Ok(default),
    }
}

fn parse_u32_env(key: &'static str, default: u32) -> Result<u32, ConfigError> {
    match env::var(key) {
        Ok(v) => v
            .parse::<u32>()
            .map_err(|_| ConfigError::InvalidValue { key, value: v }),
        Err(_) => Ok(default),
    }
}

fn parse_u64_env(key: &'static str, default: u64) -> Result<u64, ConfigError> {
    match env::var(key) {
        Ok(v) => v
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidValue { key, value: v }),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Config tests mutate process-wide environment variables, so they must
    // not run in parallel with each other (or with any other env-touching
    // test).  This mutex serializes them.
    static CONFIG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        CONFIG_TEST_LOCK.lock().unwrap()
    }

    #[test]
    fn missing_secret_errors() {
        let _guard = lock();
        env::remove_var("SESSION_SECRET");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::MissingSecret)
        ));
    }

    #[test]
    fn short_secret_errors() {
        let _guard = lock();
        env::set_var("SESSION_SECRET", "short");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::SecretTooShort(5))
        ));
        env::remove_var("SESSION_SECRET");
    }

    #[test]
    fn defaults_with_valid_secret() {
        let _guard = lock();
        env::set_var("SESSION_SECRET", "this-is-a-very-long-secret-key-32+bytes!");
        // Clear any leftover overrides from other tests.
        env::remove_var("BCRYPT_COST");
        env::remove_var("COOKIE_SECURE");
        env::remove_var("SESSION_TTL_SHORT");
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.bcrypt_cost, 12);
        assert!(cfg.cookie_secure);
        assert_eq!(cfg.session_ttl_short, 7200);
        assert_eq!(cfg.session_ttl_long, 604_800);
        assert_eq!(cfg.rate_limit_max_failures, 5);
        assert_eq!(cfg.rate_limit_lockout_minutes, 15);
        assert_eq!(cfg.rate_limit_ip_per_minute, 20);
        env::remove_var("SESSION_SECRET");
    }

    #[test]
    fn override_defaults() {
        let _guard = lock();
        env::set_var("SESSION_SECRET", "this-is-a-very-long-secret-key-32+bytes!");
        env::set_var("BCRYPT_COST", "10");
        env::set_var("COOKIE_SECURE", "false");
        env::set_var("SESSION_TTL_SHORT", "3600");
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.bcrypt_cost, 10);
        assert!(!cfg.cookie_secure);
        assert_eq!(cfg.session_ttl_short, 3600);
        env::remove_var("SESSION_SECRET");
        env::remove_var("BCRYPT_COST");
        env::remove_var("COOKIE_SECURE");
        env::remove_var("SESSION_TTL_SHORT");
    }
}
