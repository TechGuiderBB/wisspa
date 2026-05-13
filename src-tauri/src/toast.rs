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

pub fn info<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, title, body);
}

pub fn warn<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, &format!("⚠ {title}"), body);
}

pub fn error<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, &format!("✗ {title}"), body);
}
