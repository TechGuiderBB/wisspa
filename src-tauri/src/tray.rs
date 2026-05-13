use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

const TRAY_ID: &str = "wisspa-tray";

pub fn install(app: &tauri::App) -> Result<()> {
    let open_settings = MenuItem::with_id(app, "open_settings", "Open Settings…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Wisspa", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open_settings, &separator, &quit])?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| anyhow::anyhow!("no default window icon to use for tray"))?
        .clone();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app: &AppHandle<Wry>, event| match event.id.as_ref() {
            "open_settings" => open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    log::info!("tray icon installed");
    Ok(())
}

fn open_settings_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.unminimize();
    }
}
