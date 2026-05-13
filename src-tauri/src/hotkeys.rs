use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const EVENT_START: &str = "wisspa://start-recording";
pub const EVENT_STOP: &str = "wisspa://stop-recording";
pub const EVENT_CANCEL: &str = "wisspa://cancel-recording";
pub const EVENT_MODE: &str = "wisspa://recording-mode";

const OVERLAY_LABEL: &str = "overlay";

/// Action → currently registered shortcut. Used so the runtime handler can
/// dispatch any registered shortcut to the right action even after reassignment.
fn registry() -> &'static Mutex<HashMap<String, Shortcut>> {
    static R: OnceLock<Mutex<HashMap<String, Shortcut>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

#[allow(dead_code)] // surfaced by the "reset to defaults" UI in Phase 3+
pub fn default_combo(action: &str) -> &'static str {
    match action {
        "dictation" => "CmdOrCtrl+Shift+Space",
        "action" => "CmdOrCtrl+Shift+A",
        "prompt" => "CmdOrCtrl+Shift+P",
        "cancel" => "Escape",
        _ => "",
    }
}

fn parse_shortcut(combo: &str) -> Result<Shortcut> {
    Shortcut::from_str(combo).map_err(|e| anyhow!("invalid shortcut '{combo}': {e}"))
}

fn lookup_action(shortcut: &Shortcut) -> Option<String> {
    let map = registry().lock().ok()?;
    map.iter()
        .find(|(_, s)| *s == shortcut)
        .map(|(k, _)| k.clone())
}

fn show_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.show();
    }
}

fn hide_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.hide();
    }
}

pub fn build_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            let action = match lookup_action(shortcut) {
                Some(a) => a,
                None => return,
            };
            match (action.as_str(), event.state()) {
                ("dictation", ShortcutState::Pressed) => {
                    log::info!("dictation hotkey pressed");
                    show_overlay(app);
                    let _ = app.emit(EVENT_MODE, "dictation");
                    let _ = app.emit(EVENT_START, ());
                }
                ("dictation", ShortcutState::Released) => {
                    log::info!("dictation hotkey released");
                    hide_overlay(app);
                    let _ = app.emit(EVENT_STOP, ());
                }
                ("action", ShortcutState::Pressed) => {
                    log::info!("action hotkey pressed");
                    show_overlay(app);
                    let _ = app.emit(EVENT_MODE, "action");
                    let _ = app.emit(EVENT_START, ());
                }
                ("action", ShortcutState::Released) => {
                    log::info!("action hotkey released");
                    hide_overlay(app);
                    let _ = app.emit(EVENT_STOP, ());
                }
                ("prompt", ShortcutState::Pressed) => {
                    log::info!("prompt hotkey pressed");
                    show_overlay(app);
                    let _ = app.emit(EVENT_MODE, "prompt");
                    let _ = app.emit(EVENT_START, ());
                }
                ("prompt", ShortcutState::Released) => {
                    log::info!("prompt hotkey released");
                    hide_overlay(app);
                    let _ = app.emit(EVENT_STOP, ());
                }
                ("cancel", ShortcutState::Pressed) => {
                    log::info!("cancel hotkey pressed");
                    hide_overlay(app);
                    let _ = app.emit(EVENT_CANCEL, ());
                }
                _ => {}
            }
        })
        .build()
}

pub fn register_default_shortcuts<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = crate::settings_store::load(app).unwrap_or_default();
    for (action, combo) in [
        ("dictation", settings.hotkeys.dictation.as_str()),
        ("action", settings.hotkeys.action.as_str()),
        ("prompt", settings.hotkeys.prompt.as_str()),
        ("cancel", settings.hotkeys.cancel.as_str()),
    ] {
        if let Err(e) = register(app, action, combo) {
            log::warn!("failed to register {action} = '{combo}': {e:#}");
        }
    }
    log::info!("hotkeys initialised from settings");
    Ok(())
}

fn register<R: Runtime>(app: &AppHandle<R>, action: &str, combo: &str) -> Result<()> {
    let shortcut = parse_shortcut(combo)?;
    let gs = app.global_shortcut();
    gs.register(shortcut)
        .map_err(|e| anyhow!("register {combo}: {e}"))?;
    let mut map = registry().lock().map_err(|_| anyhow!("registry poisoned"))?;
    map.insert(action.to_string(), shortcut);
    Ok(())
}

pub fn reassign<R: Runtime>(app: &AppHandle<R>, action: &str, combo: &str) -> Result<()> {
    let gs = app.global_shortcut();
    // Drop the previously-registered shortcut for this action, if any.
    let previous = {
        let map = registry().lock().map_err(|_| anyhow!("registry poisoned"))?;
        map.get(action).cloned()
    };
    if let Some(prev) = previous {
        let _ = gs.unregister(prev);
    }

    let new = parse_shortcut(combo)?;
    gs.register(new)
        .map_err(|e| anyhow!("register {combo}: {e}"))?;

    let mut map = registry().lock().map_err(|_| anyhow!("registry poisoned"))?;
    map.insert(action.to_string(), new);
    log::info!("reassigned {action} → {combo}");
    Ok(())
}

/// Unregister every shortcut currently in the registry so the Settings UI can
/// capture key presses (otherwise tauri's global shortcut intercepts them
/// before the webview's onKeyDown fires).
pub fn pause_all<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let gs = app.global_shortcut();
    let shortcuts: Vec<Shortcut> = {
        let map = registry().lock().map_err(|_| anyhow!("registry poisoned"))?;
        map.values().cloned().collect()
    };
    for s in shortcuts {
        let _ = gs.unregister(s);
    }
    log::info!("global shortcuts paused for hotkey capture");
    Ok(())
}

/// Re-register every shortcut in the registry. Call after the Settings UI is
/// done capturing.
pub fn resume_all<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let gs = app.global_shortcut();
    let shortcuts: Vec<Shortcut> = {
        let map = registry().lock().map_err(|_| anyhow!("registry poisoned"))?;
        map.values().cloned().collect()
    };
    for s in shortcuts {
        let _ = gs.register(s);
    }
    log::info!("global shortcuts resumed");
    Ok(())
}
