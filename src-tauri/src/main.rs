// Hide console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app_detector;
mod audio;
mod ax_snapshot;
mod commands;
mod history;
mod hotkeys;
mod injector;
mod keychain;
mod learning;
mod llm;
mod modes;
mod permissions;
mod prearm;
mod selection;
mod settings_store;
mod sounds;
mod stt;
mod toast;
mod tray;

use std::path::PathBuf;
use tauri::{Listener, LogicalPosition};

/// Choose which monitor the pill should appear on right now: the one
/// containing the mouse cursor, falling back to the primary, falling back
/// to whatever the window currently reports. This gets called both at
/// startup (so the pill at least lands somewhere visible) and on every
/// hotkey press (so it follows the user across displays — the pill needs
/// to be where the user is looking, not pinned to the laptop screen).
fn pick_pill_monitor<R: tauri::Runtime>(
    overlay: &tauri::WebviewWindow<R>,
) -> Option<tauri::Monitor> {
    if let Ok(cursor) = overlay.cursor_position() {
        if let Ok(monitors) = overlay.available_monitors() {
            // Monitor positions/sizes are reported in physical (device)
            // pixels; `cursor_position` returns the same coordinate space.
            let cx = cursor.x as i32;
            let cy = cursor.y as i32;
            if let Some(m) = monitors.into_iter().find(|m| {
                let p = m.position();
                let s = m.size();
                cx >= p.x
                    && cx < p.x + s.width as i32
                    && cy >= p.y
                    && cy < p.y + s.height as i32
            }) {
                return Some(m);
            }
        }
    }
    overlay
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| overlay.current_monitor().ok().flatten())
}

pub(crate) fn position_overlay_top_center<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.set_visible_on_all_workspaces(true);
        if let Some(monitor) = pick_pill_monitor(&overlay) {
            let scale = monitor.scale_factor();
            let logical_width = monitor.size().width as f64 / scale;
            let mp = monitor.position();
            let mon_x = mp.x as f64 / scale;
            let mon_y = mp.y as f64 / scale;
            let overlay_w = 220.0;
            let x = mon_x + (logical_width - overlay_w) / 2.0;
            let y = mon_y + 36.0;
            let _ = overlay.set_position(LogicalPosition::new(x, y));
        }
    }
}

fn maybe_show_onboarding<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let completed = settings_store::load(app)
        .map(|s| s.onboarding_completed)
        .unwrap_or(false);
    if completed {
        return;
    }
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.show();
        let _ = w.set_focus();
        log::info!("first launch: showing onboarding window");
    }
}

fn stack_main_under_overlay<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) {
    // The runtime window has to stay visible so WKWebView keeps JS running.
    // We park it at the same top-center position as the overlay so the
    // alwaysOnTop overlay covers it during recording. When idle, the runtime's
    // own dim "Wisspa" pill is what the user sees.
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_visible_on_all_workspaces(true);
        let _ = main.set_always_on_top(true);
        let mon = main
            .primary_monitor()
            .ok()
            .flatten()
            .or_else(|| main.current_monitor().ok().flatten());
        if let Some(monitor) = mon {
            let scale = monitor.scale_factor();
            let logical_width = monitor.size().width as f64 / scale;
            let mp = monitor.position();
            let mon_x = mp.x as f64 / scale;
            let mon_y = mp.y as f64 / scale;
            let main_w = 220.0;
            let x = mon_x + (logical_width - main_w) / 2.0;
            let y = mon_y + 36.0;
            let _ = main.set_position(LogicalPosition::new(x, y));
        }
    }
}

use std::sync::RwLock;

pub struct AppState {
    pub groq_api_key: RwLock<String>,
    pub anthropic_api_key: RwLock<String>,
}

impl AppState {
    pub fn groq_key(&self) -> String {
        self.groq_api_key.read().map(|g| g.clone()).unwrap_or_default()
    }
    pub fn anthropic_key(&self) -> String {
        self.anthropic_api_key.read().map(|g| g.clone()).unwrap_or_default()
    }
    pub fn set_groq_key(&self, v: String) {
        if let Ok(mut g) = self.groq_api_key.write() {
            *g = v;
        }
    }
    pub fn set_anthropic_key(&self, v: String) {
        if let Ok(mut g) = self.anthropic_api_key.write() {
            *g = v;
        }
    }
}

fn load_env() {
    // Walk up from CARGO_MANIFEST_DIR / current dir looking for a .env.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".env"));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(".env"));
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest);
        candidates.push(p.join(".env"));
        if let Some(parent) = p.parent() {
            candidates.push(parent.join(".env"));
        }
    }
    for path in candidates {
        if path.exists() {
            let _ = dotenvy::from_path(&path);
            log::info!("Loaded .env from {}", path.display());
            return;
        }
    }
    log::warn!("No .env file found; API keys will be empty");
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    load_env();

    let state = AppState {
        groq_api_key: RwLock::new(keychain::resolve("GROQ_API_KEY")),
        anthropic_api_key: RwLock::new(keychain::resolve("ANTHROPIC_API_KEY")),
    };

    log::info!(
        "binary: {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string())
    );
    log::info!(
        "macOS Accessibility trusted: {}",
        injector::accessibility_trusted()
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(hotkeys::build_plugin())
        .manage(state)
        .setup(|app| {
            // macOS: be an accessory app — no dock icon, no menu bar focus.
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            position_overlay_top_center(app);
            stack_main_under_overlay(app);
            tray::install(app)?;
            if let Err(e) = actions::registry::initialize(&app.handle()) {
                log::warn!("action registry init failed: {e:#}");
            }
            if let Err(e) = history::initialize(&app.handle()) {
                log::warn!("history init failed: {e:#}");
            }
            hotkeys::register_default_shortcuts(&app.handle())?;
            // The "ready" chime plays when capture is genuinely live (the
            // frontend emits this once MediaRecorder fires `onstart`), not at
            // hotkey-press time — so the cue honestly means "speak now".
            let armed_handle = app.handle().clone();
            app.handle().listen("wisspa://recording-armed", move |_| {
                sounds::play(&armed_handle, sounds::Cue::Start);
            });
            // Opt-in pre-warm: watch for the modifier portion of a recording
            // hotkey so the mic can be warmed before the full combo completes.
            if let Ok(s) = settings_store::load(&app.handle()) {
                let masks = prearm::collect_masks(&[
                    &s.hotkeys.dictation,
                    &s.hotkeys.action,
                    &s.hotkeys.prompt,
                ]);
                prearm::apply(app.handle(), s.general.fast_recording_start, masks);
            }
            maybe_show_onboarding(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::process_audio,
            commands::list_actions,
            commands::ping,
            commands::get_settings,
            commands::save_settings,
            commands::get_api_key_present,
            commands::save_api_key,
            commands::test_api_key,
            commands::update_hotkey,
            commands::pause_hotkeys,
            commands::resume_hotkeys,
            commands::open_settings_window,
            commands::get_permissions,
            commands::report_microphone_status,
            commands::open_system_settings,
            commands::request_screen_recording_access,
            commands::complete_onboarding,
            commands::get_history,
            commands::clear_history,
            commands::export_history_csv,
            commands::report_silent_recording,
            commands::report_recording_timeout,
            commands::get_word_corrections,
            commands::submit_word_correction,
            commands::save_word_corrections,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
