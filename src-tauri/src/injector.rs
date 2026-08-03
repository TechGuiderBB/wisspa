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
/// 1. Snapshotting all current clipboard flavors as owned bytes (issue #32).
/// 2. Writing `text` to the clipboard.
/// 3. If `target_app` is set, re-activating it (in case another app stole
///    focus when our global hotkey fired — e.g. Perplexity intercepting
///    Cmd+Shift+P alongside Wisspa).
/// 4. Simulating Cmd+V.
/// 5. Restoring the snapshotted clipboard content — finally-style: the restore
///    runs even when activation or Cmd+V fails (on success after the
///    paste-consumption wait — early once the pasteboard moves on, capped at
///    250ms — and immediately on failure).
///
/// The whole window runs under a single-flight mutex with a `session` abort
/// check (issue #31) so two pastes can't interleave the snapshot/restore.
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

    log::info!("inject step 1: snapshotting clipboard (all flavors)");
    let clipboard = app.clipboard();
    // Snapshot every pasteboard flavor as owned bytes so images, file
    // references, RTF etc. survive the paste — not just plain text (issue #32).
    let snapshot = crate::clipboard::snapshot();
    if snapshot.skipped_promised > 0 {
        log::debug!(
            "clipboard snapshot: {} item(s), {} flavor(s), {} promised flavor(s) not restorable",
            snapshot.item_count(),
            snapshot.flavor_count(),
            snapshot.skipped_promised
        );
    }

    log::info!("inject step 2: writing {} chars to clipboard", text.len());
    // Once the snapshot exists, the restore MUST run on every exit path —
    // an early `?` on the activate / Cmd+V steps previously skipped it,
    // destroying the user's original clipboard (images, files, RTF) and
    // leaving our dictation text behind. `restore_after` is the finally-style
    // guarantee; it returns the paste result so the original error still
    // propagates (unit-tested below).
    let paste = async {
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

        log::info!("inject step 4: Cmd+V dispatched, waiting for paste consumption");
        // Event-driven replacement for the old fixed 250ms sleep. Contract:
        // NSPasteboard.changeCount bumps on every pasteboard WRITE (our
        // write_text above already bumped it once, so the baseline is taken
        // here — strictly after our own writes). When the count moves again
        // the pasteboard has moved on from our injected text (the paste was
        // consumed, or another write superseded it) and restoring the
        // snapshot can no longer clobber an in-flight paste of our text — so
        // we restore early. Targets whose paste is read-only with respect to
        // the pasteboard never bump the count; for them the wait runs to
        // PASTE_WAIT_CAP, exactly the old fixed-sleep worst case.
        let baseline = crate::clipboard::change_count();
        match wait_for_paste_consumed(baseline, crate::clipboard::change_count).await {
            PasteWait::Changed => log::debug!("pasteboard moved; restoring clipboard early"),
            PasteWait::TimedOut => log::debug!("paste wait cap reached; restoring clipboard"),
        }
        Ok(())
    };

    log::info!("inject step 5: restoring previous clipboard (all flavors)");
    let result = restore_after(
        || {
            if snapshot.is_empty() {
                // Clipboard was empty (or held only un-restorable promised flavors)
                // before injection. Leave the injected text in place rather than
                // clearing, so Cmd+V still works if the user pastes again.
                log::debug!("clipboard pre-injection content was empty; leaving injected text");
            } else {
                crate::clipboard::restore(&snapshot);
            }
        },
        paste,
    )
    .await;

    log::info!("inject step 6: done");
    result
}

/// Finally-style clipboard restore: awaits `paste`, then runs `restore`
/// exactly once whether the paste succeeded or failed, and returns `paste`'s
/// result so the original error propagates. Generic so the failure-path
/// guarantee is unit-testable without an AppHandle or the real pasteboard.
async fn restore_after<R, F>(restore: R, paste: F) -> Result<()>
where
    R: FnOnce(),
    F: std::future::Future<Output = Result<()>>,
{
    let result = paste.await;
    restore();
    result
}

/// Interval between changeCount polls while waiting for the target app to
/// consume our synthetic paste.
const PASTE_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Worst-case post-paste wait before the clipboard restore — identical to the
/// previous fixed 250ms sleep, so a target that consumes the paste without
/// the pasteboard moving again costs no more than before.
const PASTE_WAIT_CAP: Duration = Duration::from_millis(250);

/// Why the post-paste wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteWait {
    /// changeCount moved off the baseline: the pasteboard has moved on from
    /// our injected text, so restoring now can't clobber an in-flight paste.
    Changed,
    /// Nothing moved within PASTE_WAIT_CAP — same timing as the old fixed
    /// sleep.
    TimedOut,
}

/// Poll `change_count` until it differs from `baseline` or PASTE_WAIT_CAP
/// elapses, sleeping PASTE_POLL_INTERVAL between checks. Generic over the
/// counter source so the early-exit / cap behaviour is unit-testable without
/// a real pasteboard.
async fn wait_for_paste_consumed<F>(baseline: i64, change_count: F) -> PasteWait
where
    F: Fn() -> i64,
{
    let start = std::time::Instant::now();
    loop {
        if change_count() != baseline {
            return PasteWait::Changed;
        }
        if start.elapsed() >= PASTE_WAIT_CAP {
            return PasteWait::TimedOut;
        }
        tokio::time::sleep(PASTE_POLL_INTERVAL).await;
    }
}

/// Synthesise Cmd+V via AppleScript / System Events. We intentionally do
/// NOT use `enigo::CGEventPost` for this on macOS: enigo's keystroke path
/// aborts the host process even with Accessibility granted, bypassing
/// `catch_unwind` (see `DECISIONS.md` item 9 + the Gotchas in CLAUDE.md).
/// AppleScript via osascript is the macOS-blessed paste path. The `keystroke`
/// action type takes the same route (`combo_to_applescript` in
/// actions/executor.rs), so enigo is no longer a dependency at all.
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

#[cfg(test)]
mod tests {
    use super::restore_after;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The regression: on the activate / Cmd+V failure paths the restore used
    /// to be skipped by an early `?`, destroying the user's original clipboard.
    /// `restore_after` must run the restore exactly once and still return the
    /// original error.
    #[tokio::test]
    async fn restore_runs_on_failure_and_original_error_propagates() {
        let restores = AtomicUsize::new(0);
        let result = restore_after(
            || {
                restores.fetch_add(1, Ordering::SeqCst);
            },
            async { anyhow::bail!("Cmd+V dispatch failed") },
        )
        .await;
        assert_eq!(
            restores.load(Ordering::SeqCst),
            1,
            "restore must run even when the paste fails"
        );
        let err = result.expect_err("paste failure must propagate");
        assert!(
            err.to_string().contains("Cmd+V dispatch failed"),
            "original error, not a restore artefact: {err:#}"
        );
    }

    #[tokio::test]
    async fn restore_runs_exactly_once_on_success() {
        let restores = AtomicUsize::new(0);
        let result = restore_after(
            || {
                restores.fetch_add(1, Ordering::SeqCst);
            },
            async { Ok(()) },
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(restores.load(Ordering::SeqCst), 1);
    }

    /// Pasteboard moves before the cap → the wait exits early (Changed) well
    /// under PASTE_WAIT_CAP. The counter source is a closure over elapsed
    /// time, so no real pasteboard is needed.
    #[tokio::test]
    async fn paste_wait_exits_early_when_count_moves() {
        let start = std::time::Instant::now();
        let outcome = super::wait_for_paste_consumed(0, move || {
            if start.elapsed() >= std::time::Duration::from_millis(60) {
                1
            } else {
                0
            }
        })
        .await;
        assert_eq!(outcome, super::PasteWait::Changed);
        assert!(
            start.elapsed() < super::PASTE_WAIT_CAP,
            "early exit must beat the cap, took {:?}",
            start.elapsed()
        );
    }

    /// Pasteboard already different on the first check → no sleep at all.
    #[tokio::test]
    async fn paste_wait_returns_immediately_when_already_changed() {
        let start = std::time::Instant::now();
        let outcome = super::wait_for_paste_consumed(0, || 7).await;
        assert_eq!(outcome, super::PasteWait::Changed);
        assert!(start.elapsed() < super::PASTE_POLL_INTERVAL);
    }

    /// Pasteboard never moves → the wait runs to the cap, preserving the old
    /// fixed-sleep worst case (and no further: bounded sanity window).
    #[tokio::test]
    async fn paste_wait_caps_when_count_never_moves() {
        let start = std::time::Instant::now();
        let outcome = super::wait_for_paste_consumed(42, || 42).await;
        assert_eq!(outcome, super::PasteWait::TimedOut);
        let elapsed = start.elapsed();
        assert!(
            elapsed >= super::PASTE_WAIT_CAP,
            "cap must preserve the old 250ms worst case, took {elapsed:?}"
        );
        assert!(
            elapsed < super::PASTE_WAIT_CAP + std::time::Duration::from_millis(150),
            "cap must not overshoot wildly, took {elapsed:?}"
        );
    }
}
