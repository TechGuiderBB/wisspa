use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Returns the name of the frontmost application process, e.g. "TextEdit", "Cursor", "Slack".
/// Implemented via AppleScript; requires Automation permission (granted on first prompt).
pub async fn frontmost_app_name() -> Result<String> {
    let output = tokio::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
        ])
        .output()
        .await
        .context("spawn osascript for frontmost app")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "osascript frontmost failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return Err(anyhow::anyhow!("empty frontmost app name"));
    }
    Ok(name)
}

/// Bring an application to the foreground. Used to restore the user's
/// original focus before pasting, in case another app stole focus when the
/// global hotkey fired (e.g. Perplexity intercepting Cmd+Shift+P).
///
/// Uses System Events GUI scripting (`set frontmost to true` on the target
/// *process*) rather than a direct `tell application "X" to activate`. The
/// direct activate is an Apple Event to the target app and requires a
/// per-target Automation permission — but macOS silently skips the TCC
/// prompt when the target app is already frontmost, so Wisspa never gets
/// added to e.g. Chrome's Automation list and the activate permanently
/// no-ops the moment focus drifts. The GUI-scripting path routes through
/// System Events (Wisspa already holds that Automation grant) and works
/// for any process the user can see.
pub async fn activate_app(name: &str) -> Result<()> {
    // Escape any embedded quotes so the AppleScript stays valid.
    let safe = name.replace('"', "\\\"");
    let script = format!(
        r#"tell application "System Events" to tell process "{safe}" to set frontmost to true"#
    );
    let output = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .await
        .context("spawn osascript for activate")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "osascript activate {name} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Snapshot of the user's target app captured at hotkey-press time. Read
/// later (after STT / cleanup) so we can re-activate it before pasting —
/// even if another app stole focus when the global shortcut fired.
static TARGET_APP: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Wisspa-internal apps that should never be the "target app" — pasting
/// back into our own pill or onboarding window would be a no-op or worse.
const WISSPA_APP_NAMES: &[&str] = &["Wisspa", "wisspa", "Wisspa Recording"];

fn is_self_app(name: &str) -> bool {
    WISSPA_APP_NAMES
        .iter()
        .any(|w| name.eq_ignore_ascii_case(w))
}

/// Capture frontmost app asynchronously and store it as the target for the
/// in-flight recording. Idempotent across the press/release cycle: the
/// snapshot is cleared via `take_target_app` once consumed.
///
/// Note: the global-shortcut handler runs on a thread without a Tokio
/// runtime in scope, so we hop onto Tauri's managed runtime via
/// `async_runtime::spawn`. Calling `tokio::spawn` here panics.
pub fn snapshot_target_app_now() {
    tauri::async_runtime::spawn(async {
        match frontmost_app_name().await {
            Ok(name) => {
                if is_self_app(&name) {
                    // The user pressed the hotkey while Wisspa itself was
                    // focused (rare — pill click etc.). Drop the snapshot
                    // so callers fall back to a live detection.
                    return;
                }
                if let Ok(mut g) = TARGET_APP.lock() {
                    log::info!("target app snapshot: {name}");
                    *g = Some(name);
                }
            }
            Err(e) => log::warn!("target app snapshot failed: {e:#}"),
        }
    });
}

/// Consume the snapshot. Returns None if the press-time capture didn't
/// complete (caller should fall back to a live `frontmost_app_name`).
pub fn take_target_app() -> Option<String> {
    TARGET_APP.lock().ok().and_then(|mut g| g.take())
}

/// Discard any pending snapshot — used on the cancel hotkey so a stale
/// value can't leak into a subsequent recording.
pub fn clear_target_app() {
    if let Ok(mut g) = TARGET_APP.lock() {
        *g = None;
    }
}
