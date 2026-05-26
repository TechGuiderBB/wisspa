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

/// Active tab info snapshotted from a known browser at hotkey-press time.
/// Lets Prompt Mode tell e.g. Gmail apart from claude.ai when both are
/// `Google Chrome` to the OS.
#[derive(Debug, Clone)]
pub struct BrowserContext {
    /// Kept for log lines and future cancel-path diagnostics — Sonnet receives
    /// the app name through the separate `active_app` parameter.
    #[allow(dead_code)]
    pub app: String,
    pub url: String,
    pub title: String,
}

/// Browsers whose active tab Wisspa can query via AppleScript. Names match
/// what `frontmost_app_name()` returns (the System Events process name).
const KNOWN_BROWSERS: &[&str] = &[
    "Google Chrome",
    "Google Chrome Beta",
    "Google Chrome Canary",
    "Google Chrome Dev",
    "Chromium",
    "Brave Browser",
    "Brave Browser Beta",
    "Brave Browser Nightly",
    "Microsoft Edge",
    "Microsoft Edge Beta",
    "Microsoft Edge Canary",
    "Microsoft Edge Dev",
    "Arc",
    "Vivaldi",
    "Opera",
    "Safari",
    "Safari Technology Preview",
];

pub fn is_browser(name: &str) -> bool {
    KNOWN_BROWSERS.iter().any(|b| name == *b)
}

fn is_safari_family(name: &str) -> bool {
    name == "Safari" || name == "Safari Technology Preview"
}

/// Read the active tab's URL and title from a known browser via AppleScript.
/// Best-effort: returns Err for unsupported apps, no open windows, or when
/// Automation permission for the target browser has not been granted.
/// Callers treat Err as "no browser context" and degrade gracefully.
///
/// First call against a given browser triggers a TCC prompt
/// ("Wisspa would like to control Google Chrome"). If the user declines, all
/// subsequent calls return Err and Prompt Mode keeps its pre-feature behaviour.
async fn read_browser_active_tab(name: &str) -> Result<(String, String)> {
    if !is_browser(name) {
        return Err(anyhow::anyhow!("not a known browser: {name}"));
    }
    let safe = name.replace('"', "\\\"");
    let inner = if is_safari_family(name) {
        format!(
            r#"tell application "{safe}"
                set u to URL of current tab of front window
                set t to name of current tab of front window
                return u & "
" & t
            end tell"#
        )
    } else {
        format!(
            r#"tell application "{safe}"
                set u to URL of active tab of window 1
                set t to title of active tab of window 1
                return u & "
" & t
            end tell"#
        )
    };
    let script = format!(
        r#"try
    {inner}
on error errMsg
    return "ERR:" & errMsg
end try"#
    );
    let output = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .await
        .context("spawn osascript for browser tab")?;
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.starts_with("ERR:") {
        return Err(anyhow::anyhow!("browser tab query failed: {raw}"));
    }
    parse_tab_output(&raw)
}

fn parse_tab_output(raw: &str) -> Result<(String, String)> {
    let mut parts = raw.splitn(2, '\n');
    let url = parts.next().unwrap_or("").trim().to_string();
    let title = parts.next().unwrap_or("").trim().to_string();
    if url.is_empty() {
        return Err(anyhow::anyhow!("empty url in browser tab output"));
    }
    Ok((url, title))
}

/// Snapshot of the user's target app captured at hotkey-press time. Read
/// later (after STT / cleanup) so we can re-activate it before pasting —
/// even if another app stole focus when the global shortcut fired.
static TARGET_APP: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Browser tab snapshot. Set alongside `TARGET_APP` when the snapshotted
/// app is a known browser. Consumed by Prompt Mode.
static TARGET_BROWSER_CONTEXT: Lazy<Mutex<Option<BrowserContext>>> =
    Lazy::new(|| Mutex::new(None));

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
/// When the frontmost app is a known browser, also captures the active tab
/// URL + title into TARGET_BROWSER_CONTEXT so Prompt Mode can distinguish
/// AI surfaces (claude.ai, chatgpt.com) from regular surfaces (Gmail,
/// Notion) that all report as the same `Google Chrome` process name.
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
                if is_browser(&name) {
                    match read_browser_active_tab(&name).await {
                        Ok((url, title)) => {
                            log::info!(
                                "browser context snapshot: app={name} title={title:?}"
                            );
                            if let Ok(mut g) = TARGET_BROWSER_CONTEXT.lock() {
                                *g = Some(BrowserContext {
                                    app: name.clone(),
                                    url,
                                    title,
                                });
                            }
                        }
                        Err(e) => log::debug!("browser context snapshot failed: {e:#}"),
                    }
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

/// Consume the browser tab snapshot if one was captured. Returns None when
/// the target app wasn't a browser, no window was open, or Automation was
/// denied — every caller is expected to handle None as "no extra context."
pub fn take_target_browser_context() -> Option<BrowserContext> {
    TARGET_BROWSER_CONTEXT.lock().ok().and_then(|mut g| g.take())
}

/// Discard any pending snapshot — used on the cancel hotkey so a stale
/// value can't leak into a subsequent recording.
pub fn clear_target_app() {
    if let Ok(mut g) = TARGET_APP.lock() {
        *g = None;
    }
    if let Ok(mut g) = TARGET_BROWSER_CONTEXT.lock() {
        *g = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_browsers_detected() {
        assert!(is_browser("Google Chrome"));
        assert!(is_browser("Safari"));
        assert!(is_browser("Microsoft Edge"));
        assert!(is_browser("Brave Browser"));
        assert!(is_browser("Arc"));
        assert!(is_browser("Vivaldi"));
        assert!(is_browser("Safari Technology Preview"));
    }

    #[test]
    fn non_browsers_not_detected() {
        assert!(!is_browser("Slack"));
        assert!(!is_browser("Cursor"));
        assert!(!is_browser("Terminal"));
        assert!(!is_browser("iTerm2"));
        assert!(!is_browser("Gmail"));
        assert!(!is_browser(""));
        // Case sensitivity matters: System Events returns the canonical name,
        // and our list is the canonical list. Treat a casing mismatch as not
        // a browser rather than guessing.
        assert!(!is_browser("google chrome"));
    }

    #[test]
    fn parses_well_formed_tab_output() {
        let (url, title) = parse_tab_output(
            "https://mail.google.com/mail/u/0/#inbox\nInbox - brooke@techguider.com.au - Gmail",
        )
        .unwrap();
        assert_eq!(url, "https://mail.google.com/mail/u/0/#inbox");
        assert_eq!(title, "Inbox - brooke@techguider.com.au - Gmail");
    }

    #[test]
    fn parses_when_title_missing() {
        let (url, title) = parse_tab_output("https://example.com\n").unwrap();
        assert_eq!(url, "https://example.com");
        assert_eq!(title, "");
    }

    #[test]
    fn rejects_empty_url() {
        assert!(parse_tab_output("").is_err());
        assert!(parse_tab_output("\nTitle only").is_err());
    }

    #[test]
    fn keeps_title_intact_when_it_contains_newlines() {
        // splitn(2,'\n') puts everything after the first newline into title.
        let (url, title) = parse_tab_output("https://x.com\nLine 1\nLine 2").unwrap();
        assert_eq!(url, "https://x.com");
        assert_eq!(title, "Line 1\nLine 2");
    }
}
