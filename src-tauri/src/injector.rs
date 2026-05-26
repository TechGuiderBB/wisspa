use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
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

    log::info!("inject step 3: dispatching Cmd+V via CGEvent");
    send_cmd_v_native().context("Cmd+V dispatch failed")?;

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

/// Synthesise Cmd+V via CGEvent (Accessibility-permission only, no Apple
/// Events). Replaces a previous `tell application "System Events" to
/// keystroke "v" using command down` path which spawned an osascript
/// subprocess per paste and routed the keystroke through System Events'
/// Apple Event chain — adding latency, requiring Wisspa to hold the System
/// Events Automation permission for the keystroke itself, and depending on
/// what System Events considered frontmost at delivery time rather than
/// what the OS event queue did.
fn send_cmd_v_native() -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default())
        .context("construct Enigo for Cmd+V")?;
    // Hold Cmd, click V, release Cmd. `Click` is press+release, so this is
    // exactly one V keypress with the modifier flag held — same shape macOS
    // would see from a real human pressing the chord.
    enigo
        .key(Key::Meta, Direction::Press)
        .context("press Cmd")?;
    let v_result = enigo.key(Key::Unicode('v'), Direction::Click);
    // Always release Cmd before propagating an error — leaving it stuck
    // would corrupt the user's subsequent typing.
    let _ = enigo.key(Key::Meta, Direction::Release);
    v_result.context("click V")?;
    Ok(())
}
