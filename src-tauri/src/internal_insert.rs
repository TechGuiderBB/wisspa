//! Internal-insert channel: lets a Wisspa window that currently holds an
//! editable field (today: the onboarding test box) register itself as the
//! dictation target. Dictation then emits the final text straight to that
//! webview instead of the system clipboard + Cmd+V path.
//!
//! Why this exists: Wisspa runs as an Accessory (LSUIElement) app, and
//! clicking one of our windows does not reliably make Wisspa frontmost — so
//! the press-time app snapshot captures whatever app was previously active
//! and the injector would paste THERE (observed: dictating into the
//! onboarding test box re-activated System Settings and pasted into it).
//! Even when Wisspa *is* frontmost, `app_detector` drops self-snapshots by
//! design to protect the recording pill. Opting in from the webview sidesteps
//! the whole focus dance: the flag is set on field focus and cleared on blur,
//! so it can only fire while the user is genuinely editing inside Wisspa.

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Tauri event carrying the final text to the registered window.
pub const EVENT_INTERNAL_INSERT: &str = "internal-insert";

/// Window label of the Wisspa window currently accepting internal inserts.
static TARGET_WINDOW: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Register (Some(label)) or clear (None) the internal-insert target.
/// Called from the webview on field focus / blur.
pub fn set(window: Option<String>) {
    if let Ok(mut g) = TARGET_WINDOW.lock() {
        *g = window;
    }
}

/// The currently registered target window, if any. Peeked (not taken) on each
/// dictation: the registration lives as long as the field stays focused, so
/// consecutive dictations into the same field all land.
pub fn current() -> Option<String> {
    TARGET_WINDOW.lock().ok().and_then(|g| g.clone())
}
