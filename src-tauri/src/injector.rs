use anyhow::{Context, Result};
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

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
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
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
    let prev = clipboard.read_text().ok();

    log::info!("inject step 2: writing {} chars to clipboard", text.len());
    clipboard
        .write_text(text.to_string())
        .context("clipboard write_text failed")?;

    if let Some(name) = target_app {
        log::info!("inject step 2b: re-activating target app '{name}'");
        if let Err(e) = crate::app_detector::activate_app(name).await {
            log::warn!("activate_app({name}) failed (continuing anyway): {e:#}");
        }
        // Give the OS a moment to bring the app forward and shift keyboard
        // focus into its focused field. Without this Cmd+V can land before
        // the activation completes.
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    log::info!("inject step 3: dispatching Cmd+V via AppleScript");
    send_cmd_v_applescript().await.context("Cmd+V dispatch failed")?;

    log::info!("inject step 4: Cmd+V dispatched, sleeping before restore");
    // Give the target app time to consume the paste.
    tokio::time::sleep(Duration::from_millis(250)).await;

    log::info!("inject step 5: restoring previous clipboard");
    if let Some(prev_text) = prev {
        let _ = clipboard.write_text(prev_text);
    }

    log::info!("inject step 6: done");
    Ok(())
}

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
