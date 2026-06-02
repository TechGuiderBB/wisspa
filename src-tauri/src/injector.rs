use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Serialises the whole clipboard read → write → paste → restore window so two
/// pipelines completing close together can't interleave and corrupt the
/// clipboard or paste each other's text (issue #31). A `tokio::sync::Mutex` is
/// required (not `std::sync::Mutex`) because the guard is held across `.await`.
static INJECT_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[cfg(target_os = "macos")]
pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_trusted() -> bool {
    true
}

/// Inject `text` into the user's intended target app by:
/// 1. Saving the current clipboard text (best-effort).
/// 2. Writing `text` to the clipboard.
/// 3. If `target_app` is set, re-activating it (in case another app stole
///    focus when our global hotkey fired — e.g. Perplexity intercepting
///    Cmd+Shift+P alongside Wisspa).
/// 4. Simulating Cmd+V.
/// 5. After ~250ms, restoring the previous clipboard content.
pub async fn inject_text<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    target_app: Option<&str>,
    session: u64,
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    // Single-flight: only one injection touches the clipboard at a time. If a
    // newer recording is already pasting, this one waits its turn here.
    let _guard = INJECT_LOCK.lock().await;

    // Re-check after acquiring the lock: the user may have pressed Esc, or a
    // newer recording may have superseded this one, while we were queued. Never
    // paste stale text into whatever field is now focused (issue #31).
    if crate::session::is_aborted(session) {
        log::info!("inject aborted before paste: session {session} cancelled/superseded");
        return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
    }

    if !accessibility_trusted() {
        return Err(anyhow::anyhow!(
            "macOS Accessibility permission not granted to this binary; \
             cannot simulate Cmd+V. Grant it in System Settings → \
             Privacy & Security → Accessibility for the binary at \
             {}",
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string())
        ));
    }

    log::info!("inject step 1: reading clipboard");
    let clipboard = app.clipboard();
    // read_text() returns Err for an empty clipboard, for one holding
    // non-text content (image, file), AND for transient plugin/OS failures.
    // Keep `prev = None` in all error cases so we never restore garbage,
    // but log the underlying error so a real read failure does not get
    // silently labelled as "empty or non-text" in step 5's restore branch.
    // Known limitation: clipboard-nontextcontent-lost-after-inject.
    let prev = match clipboard.read_text() {
        Ok(text) => Some(text),
        Err(e) => {
            log::debug!("clipboard read_text failed (treating as empty/non-text): {e}");
            None
        }
    };

    log::info!("inject step 2: writing {} chars to clipboard", text.len());
    clipboard
        .write_text(text.to_string())
        .context("clipboard write_text failed")?;

    if let Some(name) = target_app {
        log::info!("inject step 2b: re-activating target app '{name}'");
        // Hard error: a silent activate failure here is the difference between
        // pasting into Chrome and pasting into whatever else macOS thinks is
        // frontmost. Caller writes the failure into history so the user sees
        // status=failed instead of a successful-looking ghost paste.
        crate::app_detector::activate_app(name)
            .await
            .with_context(|| format!("could not re-activate target app '{name}'"))?;
        // Give the OS time to bring the app forward and shift keyboard focus
        // into its focused field. 200ms covers browsers on macOS 26 where the
        // window-activation animation is meaningfully slower than older
        // releases; under this bar, Cmd+V occasionally lands a tick before
        // the target's first responder is ready.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    log::info!("inject step 3: dispatching Cmd+V via AppleScript");
    send_cmd_v_applescript().await.context("Cmd+V dispatch failed")?;

    log::info!("inject step 4: Cmd+V dispatched, sleeping before restore");
    // Give the target app time to consume the paste.
    tokio::time::sleep(Duration::from_millis(250)).await;

    log::info!("inject step 5: restoring previous clipboard");
    if let Some(prev_text) = prev {
        let _ = clipboard.write_text(prev_text);
    } else {
        // Clipboard was empty or held non-text content before injection.
        // We cannot restore non-text content (images, files) with this API.
        // Leave the clipboard holding the injected text rather than
        // writing an empty string, so Cmd+V still works if the user pastes again.
        log::debug!("clipboard pre-injection content was empty or non-text; not restoring");
    }

    log::info!("inject step 6: done");
    Ok(())
}

/// Synthesise Cmd+V via AppleScript / System Events. We intentionally do
/// NOT use `enigo::CGEventPost` for this on macOS: enigo's keystroke path
/// aborts the host process even with Accessibility granted, bypassing
/// `catch_unwind` (see `DECISIONS.md` item 9 + the Gotchas in CLAUDE.md).
/// AppleScript via osascript is the macOS-blessed paste path. enigo
/// remains the right tool for the `keystroke` action type, but that runs
/// in an isolated child process where an abort can't take the host down.
async fn send_cmd_v_applescript() -> Result<()> {
    let output = tokio::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .output()
        .await
        .context("spawn osascript")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "osascript exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}
