use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

fn show<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if let Err(e) = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        log::warn!("notification show failed: {e:#}");
    }
}

/// Honour the user's "Quiet notifications" preference. Read on every call —
/// settings.json loads are cheap and we don't have an in-memory cache to
/// invalidate when the toggle flips from the settings UI. Failures to load
/// default to "not quiet" so a corrupt settings file still surfaces toasts.
/// Public so callers whose UX depends on a toast being SEEN (e.g. the prompt
/// preview wait) can tell suppression apart from delivery.
pub fn is_quiet<R: Runtime>(app: &AppHandle<R>) -> bool {
    crate::settings_store::load(app)
        .map(|s| s.general.quiet_notifications)
        .unwrap_or(false)
}

pub fn info<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    // Quiet mode means quiet — no debug log either. A log line per
    // suppressed toast records the user's activity at debug level, which
    // some users will have on, and the volume would defeat the toggle's
    // purpose for anyone debugging an unrelated subsystem.
    if is_quiet(app) {
        return;
    }
    show(app, title, body);
}

pub fn warn<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if is_quiet(app) {
        return;
    }
    show(app, &format!("⚠ {title}"), body);
}

/// Error toasts ignore `quiet_notifications` — a silent failure is exactly
/// the failure mode the toggle existed to avoid. Real errors always fire.
pub fn error<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, &format!("✗ {title}"), body);
}
