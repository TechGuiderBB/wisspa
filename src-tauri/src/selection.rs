use anyhow::Result;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Read whatever text is currently selected in the focused app by:
///   1. Snapshot the clipboard.
///   2. Simulate Cmd+C via AppleScript.
///   3. Wait briefly for the target app to update the clipboard.
///   4. Read the clipboard; treat it as the selection iff it differs from
///      the snapshot (otherwise nothing was selected and the snapshot is
///      still what's there).
///   5. Restore the original clipboard.
/// Returns `Ok(None)` when no selection was detected.
pub async fn read_selected_text<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>> {
    let clipboard = app.clipboard();
    let prior = clipboard.read_text().ok();

    // Issue Cmd+C in the focused app.
    let copy_output = tokio::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "c" using command down"#,
        ])
        .output()
        .await?;
    if !copy_output.status.success() {
        // Don't propagate; some apps refuse synthetic Cmd+C. Just no selection.
        log::warn!(
            "selection Cmd+C failed: {}",
            String::from_utf8_lossy(&copy_output.stderr).trim()
        );
        return Ok(None);
    }

    // Give the target app a moment to write to the clipboard. Some apps
    // (Slack, Notion, browser-based AI tools) need a longer beat than 120ms.
    tokio::time::sleep(Duration::from_millis(280)).await;

    let after = clipboard.read_text().ok();
    let selection = match (prior.as_deref(), after.as_deref()) {
        // Clipboard didn't change → nothing was selected (or selection equalled clipboard).
        (Some(p), Some(a)) if p == a => None,
        // Clipboard now has text and is different → that's the selection.
        (_, Some(a)) if !a.is_empty() => Some(a.to_string()),
        _ => None,
    };

    // Restore prior clipboard contents.
    if let Some(prev) = prior {
        let _ = clipboard.write_text(prev);
    }

    Ok(selection)
}
