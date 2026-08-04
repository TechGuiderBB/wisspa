//! Supporter License validation.
//!
//! Users buy a one-time US$19 license (sold via Lemon Squeezy) and paste the
//! key into Settings → License. The key lives in the macOS Keychain next to
//! the API keys; validation is a single POST to the wisspa.app relay, which
//! always answers HTTP 200 with a `{ valid, ... }` verdict. There is no
//! feature gating — this module records and reports status only.
//!
//! Two small files live in the app data dir beside `settings.json`:
//! - `instance_id` — a UUID v4 minted once per install, sent with every
//!   validation call so Lemon Squeezy can count activations per machine.
//! - `license_status.json` — the last verdict, so the settings UI renders
//!   without re-hitting the network on every open.
//!
//! A network failure always maps to [`LicenseState::Unreachable`] — never to
//! an invalid-license state, so an offline laptop can never look unlicensed.

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime};

/// Default validation endpoint. Overridable via `WISSPA_LICENSE_API_URL` so a
/// mock server can be pointed at in testing; read per-call (not cached) so the
/// override works however early the env var is set. Points at `www` — the apex
/// domain 308-redirects there, and skipping the hop saves a round trip on
/// every validation.
pub const LICENSE_API_URL: &str = "https://www.wisspa.app/api/license/validate";
const LICENSE_API_URL_ENV: &str = "WISSPA_LICENSE_API_URL";

/// Keychain entry holding the license key, under the same service ("Wisspa")
/// as the API keys. Also honoured as an env var in dev (via `keychain::resolve`)
/// so unsigned dev builds don't trigger a Keychain prompt on every validate.
pub const LICENSE_KEYCHAIN_KEY: &str = "WISSPA_LICENSE_KEY";

const INSTANCE_ID_FILE: &str = "instance_id";
const STATUS_FILE: &str = "license_status.json";

// Shared client re-uses TLS sessions across calls; falls back to a default
// Client (infallible) if the configured builder fails — same stance as llm.rs.
// 10s matches test_api_key: a validation answer is one small JSON body.
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// Every state the UI can render. `NoKey` is local (nothing stored); the rest
/// come from the server verdict, with `Unreachable`/`ServerError` covering the
/// failure modes. Serialised snake_case to match the frontend contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseState {
    Active,
    NoKey,
    NotFound,
    Refunded,
    Disabled,
    ActivationLimit,
    Unreachable,
    ServerError,
}

/// The verdict as the frontend renders it. `last_checked_at` is unix seconds;
/// 0 means "never checked" (a synthesised status, not a server answer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseStatus {
    pub state: LicenseState,
    pub activations_used: Option<i64>,
    pub activations_limit: Option<i64>,
    pub last_checked_at: i64,
}

impl LicenseStatus {
    fn new(state: LicenseState, checked_at: i64) -> Self {
        Self {
            state,
            activations_used: None,
            activations_limit: None,
            last_checked_at: checked_at,
        }
    }
}

/// Wire shape of the relay's 200 response. Everything optional beyond `valid`
/// so a shape drift can never fail the parse into an unrelated state.
#[derive(Debug, Deserialize)]
struct ValidateResponse {
    valid: bool,
    status: Option<String>,
    activations: Option<Activations>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Activations {
    used: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ValidateRequest<'a> {
    license_key: &'a str,
    instance_id: &'a str,
    instance_name: &'a str,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn api_url() -> String {
    std::env::var(LICENSE_API_URL_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| LICENSE_API_URL.to_string())
}

fn app_data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("app data dir unavailable")?;
    std::fs::create_dir_all(&dir).context("create app data dir")?;
    Ok(dir)
}

/// Load the per-install instance id from `dir`, minting and persisting a new
/// UUID v4 when the file is missing or holds garbage (e.g. a truncated write).
pub fn load_or_create_instance_id_in(dir: &Path) -> Result<String> {
    let path = dir.join(INSTANCE_ID_FILE);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let trimmed = raw.trim();
        if uuid::Uuid::parse_str(trimmed).is_ok() {
            return Ok(trimmed.to_string());
        }
        log::warn!("instance_id file unreadable as UUID — regenerating");
    }
    std::fs::create_dir_all(dir).context("create app data dir")?;
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::write(&path, format!("{id}\n")).context("write instance_id")?;
    Ok(id)
}

/// The machine name sent as `instance_name` so the user can recognise this
/// activation in their license portal. macOS ComputerName via scutil; "Mac"
/// when that fails (non-zero exit, empty output, non-macOS build).
fn device_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            if out.status.success() {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    "Mac".to_string()
}

/// Map a parsed relay response to a [`LicenseStatus`]. Pure so it is unit
/// tested against mock JSON without any network. Unknown or missing fields
/// collapse to `ServerError` — a state the user can retry, never a verdict.
fn status_from_response(res: &ValidateResponse, checked_at: i64) -> LicenseStatus {
    if res.valid && res.status.as_deref() == Some("active") {
        let (used, limit) = res
            .activations
            .as_ref()
            .map(|a| (a.used, a.limit))
            .unwrap_or((None, None));
        return LicenseStatus {
            state: LicenseState::Active,
            activations_used: used,
            activations_limit: limit,
            last_checked_at: checked_at,
        };
    }
    if res.valid {
        // Contract says valid:true always carries status:"active"; anything
        // else is a relay shape change — treat as a server problem, not a verdict.
        log::warn!("license relay returned valid:true without status active");
        return LicenseStatus::new(LicenseState::ServerError, checked_at);
    }
    let state = match res.error.as_deref() {
        Some("not_found") => LicenseState::NotFound,
        Some("refunded") => LicenseState::Refunded,
        Some("disabled") => LicenseState::Disabled,
        Some("activation_limit") => LicenseState::ActivationLimit,
        // bad_request, server_error, unknown strings and a missing error all
        // land here: retryable server-side problems, never "invalid key".
        other => {
            if !matches!(other, None | Some("bad_request") | Some("server_error")) {
                log::warn!("license relay returned unknown error: {other:?}");
            }
            LicenseState::ServerError
        }
    };
    LicenseStatus::new(state, checked_at)
}

fn save_status<R: Runtime>(app: &AppHandle<R>, status: &LicenseStatus) -> Result<()> {
    let path = app_data_dir(app)?.join(STATUS_FILE);
    let json = serde_json::to_string_pretty(status)?;
    std::fs::write(&path, json).context("write license_status.json")?;
    Ok(())
}

/// Drop the cached verdict. Called when the stored key changes or is removed,
/// so the UI can never show the previous key's "active" against a new key.
pub fn clear_cached_status<R: Runtime>(app: &AppHandle<R>) {
    match app_data_dir(app) {
        Ok(dir) => {
            let path = dir.join(STATUS_FILE);
            if let Err(e) = std::fs::remove_file(&path) {
                // A missing file is the goal state — only real failures matter.
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("failed to remove cached license status: {e}");
                }
            }
        }
        Err(e) => log::warn!("failed to clear cached license status: {e:#}"),
    }
}

/// The last known verdict for the settings UI. No network: returns the cached
/// file, or a synthesised `NoKey` (checked at 0 = never) when nothing has been
/// validated yet. A key present without a cache also surfaces as `NoKey` here;
/// the UI re-validates on open in that case.
pub fn cached_status<R: Runtime>(app: &AppHandle<R>) -> LicenseStatus {
    if let Ok(dir) = app_data_dir(app) {
        let path = dir.join(STATUS_FILE);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<LicenseStatus>(&raw) {
                Ok(s) => return s,
                Err(e) => log::warn!("license_status.json invalid, ignoring: {e}"),
            }
        }
    }
    LicenseStatus::new(LicenseState::NoKey, 0)
}

/// Validate the stored license key against the relay and persist the verdict.
/// `Err` is reserved for internal failures (no app data dir, instance id not
/// writable) — every license/network outcome is a `LicenseStatus` state.
pub async fn validate<R: Runtime>(app: &AppHandle<R>) -> Result<LicenseStatus> {
    let now = unix_now();
    let key = crate::keychain::resolve(LICENSE_KEYCHAIN_KEY);
    let key = key.trim().to_string();
    if key.is_empty() {
        let status = LicenseStatus::new(LicenseState::NoKey, now);
        persist_best_effort(app, &status);
        return Ok(status);
    }

    let dir = app_data_dir(app)?;
    let instance_id = load_or_create_instance_id_in(&dir)?;
    let body = ValidateRequest {
        license_key: &key,
        instance_id: &instance_id,
        instance_name: &device_name(),
    };

    let status = match HTTP_CLIENT
        .post(api_url())
        .json(&body)
        .send()
        .await
    {
        Err(e) => {
            // Offline, DNS failure, timeout — the license itself is unaffected.
            log::warn!("license validation unreachable: {e}");
            LicenseStatus::new(LicenseState::Unreachable, now)
        }
        Ok(res) if !res.status().is_success() => {
            // The relay contract is always-200; anything else is a proxy/
            // deployment problem, not a verdict on the key.
            log::warn!("license relay answered HTTP {}", res.status());
            LicenseStatus::new(LicenseState::ServerError, now)
        }
        Ok(res) => match res.json::<ValidateResponse>().await {
            Ok(parsed) => status_from_response(&parsed, now),
            Err(e) => {
                log::warn!("license relay response parse failed: {e}");
                LicenseStatus::new(LicenseState::ServerError, now)
            }
        },
    };

    persist_best_effort(app, &status);
    Ok(status)
}

/// A cache-write failure must not fail the validation itself — the caller
/// still gets the verdict; only the render-without-network optimisation is lost.
fn persist_best_effort<R: Runtime>(app: &AppHandle<R>, status: &LicenseStatus) {
    if let Err(e) = save_status(app, status) {
        log::warn!("failed to persist license status: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn parse(json: &str) -> ValidateResponse {
        serde_json::from_str(json).expect("mock response must parse")
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wisspa-license-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn active_maps_with_activations() {
        let res = parse(
            r#"{"valid":true,"status":"active","activations":{"used":2,"limit":3}}"#,
        );
        let s = status_from_response(&res, NOW);
        assert_eq!(s.state, LicenseState::Active);
        assert_eq!(s.activations_used, Some(2));
        assert_eq!(s.activations_limit, Some(3));
        assert_eq!(s.last_checked_at, NOW);
    }

    #[test]
    fn active_maps_with_null_limit() {
        // Lifetime/unlimited licenses carry limit:null — must stay None, not 0.
        let res = parse(
            r#"{"valid":true,"status":"active","activations":{"used":1,"limit":null}}"#,
        );
        let s = status_from_response(&res, NOW);
        assert_eq!(s.state, LicenseState::Active);
        assert_eq!(s.activations_used, Some(1));
        assert_eq!(s.activations_limit, None);
    }

    #[test]
    fn active_without_activations_block_still_maps() {
        let res = parse(r#"{"valid":true,"status":"active"}"#);
        let s = status_from_response(&res, NOW);
        assert_eq!(s.state, LicenseState::Active);
        assert_eq!(s.activations_used, None);
        assert_eq!(s.activations_limit, None);
    }

    #[test]
    fn each_error_string_maps_to_its_state() {
        let cases = [
            ("not_found", LicenseState::NotFound),
            ("refunded", LicenseState::Refunded),
            ("disabled", LicenseState::Disabled),
            ("activation_limit", LicenseState::ActivationLimit),
            ("server_error", LicenseState::ServerError),
            ("bad_request", LicenseState::ServerError),
        ];
        for (error, expected) in cases {
            let res = parse(&format!(r#"{{"valid":false,"error":"{error}"}}"#));
            let s = status_from_response(&res, NOW);
            assert_eq!(s.state, expected, "error {error}");
            assert_eq!(s.activations_used, None);
            assert_eq!(s.activations_limit, None);
        }
    }

    #[test]
    fn unknown_error_string_is_server_error_not_a_verdict() {
        let res = parse(r#"{"valid":false,"error":"rate_limited"}"#);
        assert_eq!(status_from_response(&res, NOW).state, LicenseState::ServerError);
    }

    #[test]
    fn invalid_without_error_is_server_error() {
        let res = parse(r#"{"valid":false}"#);
        assert_eq!(status_from_response(&res, NOW).state, LicenseState::ServerError);
    }

    #[test]
    fn valid_without_active_status_is_server_error() {
        // Contract drift: valid:true must carry status:"active". Anything else
        // is a relay bug — never synthesise an "active" the server didn't send.
        let res = parse(r#"{"valid":true}"#);
        assert_eq!(status_from_response(&res, NOW).state, LicenseState::ServerError);
    }

    #[test]
    fn status_serialises_to_frontend_contract_shape() {
        let s = LicenseStatus {
            state: LicenseState::Active,
            activations_used: Some(1),
            activations_limit: None,
            last_checked_at: NOW,
        };
        let json = serde_json::to_value(&s).expect("serialize status");
        assert_eq!(
            json,
            serde_json::json!({
                "state": "active",
                "activations_used": 1,
                "activations_limit": null,
                "last_checked_at": NOW,
            })
        );
        // And back — the persisted cache uses the same shape.
        let round: LicenseStatus = serde_json::from_value(json).expect("deserialize status");
        assert_eq!(round, s);
    }

    #[test]
    fn instance_id_is_minted_then_stable() {
        let dir = temp_dir("mint");
        let first = load_or_create_instance_id_in(&dir).expect("mint id");
        assert!(uuid::Uuid::parse_str(&first).is_ok(), "must be a UUID: {first}");
        let second = load_or_create_instance_id_in(&dir).expect("reload id");
        assert_eq!(first, second, "id must persist across reads");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn instance_id_regenerates_when_file_is_garbage() {
        let dir = temp_dir("garbage");
        std::fs::write(dir.join(INSTANCE_ID_FILE), "not-a-uuid\n").expect("seed garbage");
        let id = load_or_create_instance_id_in(&dir).expect("regenerate id");
        assert!(uuid::Uuid::parse_str(&id).is_ok(), "must be a UUID: {id}");
        // The regenerated id replaced the garbage on disk.
        let on_disk = std::fs::read_to_string(dir.join(INSTANCE_ID_FILE)).expect("read back");
        assert_eq!(on_disk.trim(), id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn instance_id_file_trailing_newline_is_tolerated() {
        let dir = temp_dir("newline");
        let id = uuid::Uuid::new_v4().to_string();
        std::fs::write(dir.join(INSTANCE_ID_FILE), format!("{id}\n")).expect("seed id");
        let loaded = load_or_create_instance_id_in(&dir).expect("load id");
        assert_eq!(loaded, id);
        std::fs::remove_dir_all(&dir).ok();
    }
}
