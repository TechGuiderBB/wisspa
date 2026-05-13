use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "Wisspa";

pub fn known_keys() -> &'static [&'static str] {
    &["GROQ_API_KEY", "ANTHROPIC_API_KEY"]
}

fn entry(key: &str) -> Result<Entry> {
    Entry::new(SERVICE, key).with_context(|| format!("keychain entry for {key}"))
}

pub fn get(key: &str) -> Result<Option<String>> {
    match entry(key)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn set(key: &str, value: &str) -> Result<()> {
    entry(key)?
        .set_password(value)
        .with_context(|| format!("set keychain entry for {key}"))
}

pub fn delete(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Resolve a secret. In dev (when `.env` provides the key) we use that to avoid
/// triggering the macOS Keychain password prompt on every rebuild — each fresh
/// dev binary is unsigned so macOS treats it as a new app and asks again.
/// Production: env is empty → Keychain is consulted (one "Always Allow" prompt
/// against the signed bundle).
pub fn resolve(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return v;
        }
    }
    match get(key) {
        Ok(Some(v)) => v,
        Ok(None) => String::new(),
        Err(e) => {
            log::warn!("keychain get({key}) failed: {e:#}");
            String::new()
        }
    }
}
