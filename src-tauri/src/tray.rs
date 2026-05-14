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
        .on_menu_event(|app: &AppHandle<Wry>, event| {
            log::info!("tray menu event: {}", event.id.as_ref());
            match event.id.as_ref() {
                "open_settings" => open_settings_window(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    log::info!("tray icon installed");
    Ok(())
}

fn open_settings_window(app: &AppHandle<Wry>) {
    let Some(window) = app.get_webview_window("settings") else {
        log::warn!("settings window not found");
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    // Re-centre on whichever monitor is currently active and briefly force
    // always-on-top so the window can't hide behind other apps. Without
    // this the show() can put it somewhere off-screen if the user has
    // moved monitors since launch.
    if let Ok(Some(monitor)) = window.current_monitor() {
        let scale = monitor.scale_factor();
        let logical_w = monitor.size().width as f64 / scale;
        let logical_h = monitor.size().height as f64 / scale;
        let win_w = 820.0;
        let win_h = 600.0;
        let mp = monitor.position();
        let mon_x = mp.x as f64 / scale;
        let mon_y = mp.y as f64 / scale;
        let x = mon_x + (logical_w - win_w) / 2.0;
        let y = mon_y + (logical_h - win_h) / 2.0;
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    // Drop always-on-top after a beat so the window behaves normally once
    // the user is interacting with it.
    let win = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let _ = win.set_always_on_top(false);
    });
    log::info!("settings window shown + focused");
}
