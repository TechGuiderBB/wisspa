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

/// Marker string surfaced through `Err` to signal a user-initiated cancel,
/// distinct from a real failure. Recognised in the mode runners (skip the
/// error toast) and in `process_audio` (history status `cancelled`, no
/// frontend error).
pub const CANCELLED_MARKER: &str = "__user_cancelled__";

/// Payload for `EVENT_START`. Mode and session id travel together in a single
/// event so the frontend can never bind a recording to the wrong mode or a
/// stale session (the old design emitted mode and start as two separate
/// events — issue #31).
#[derive(serde::Serialize, Clone)]
struct StartPayload {
    mode: &'static str,
    session: u64,
}

/// Payload for `EVENT_STOP`. Carries the mode + session of the recording being
/// released so the frontend binds the captured audio to the session that this
/// key's press began — even if a different recording hotkey was pressed (and
/// overwrote the frontend's refs) before this one was released (issue #31).
#[derive(serde::Serialize, Clone)]
struct StopPayload {
    mode: &'static str,
    session: u64,
}

/// Mode → session id of the in-flight recording for that mode, so the release
/// handler can emit the session its own press began rather than whatever the
/// frontend last saw.
fn recording_sessions() -> &'static Mutex<HashMap<&'static str, u64>> {
    static M: OnceLock<Mutex<HashMap<&'static str, u64>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn on_press<R: Runtime>(app: &AppHandle<R>, mode: &'static str) {
    crate::app_detector::snapshot_target_app_now();
    show_overlay(app);
    let session = crate::session::begin();
    if let Ok(mut m) = recording_sessions().lock() {
        m.insert(mode, session);
    }
    let _ = app.emit(EVENT_MODE, mode);
    let _ = app.emit(EVENT_START, StartPayload { mode, session });
}

fn on_release<R: Runtime>(app: &AppHandle<R>, mode: &'static str) {
    crate::sounds::play(app, crate::sounds::Cue::Stop);
    hide_overlay(app);
    let session = recording_sessions()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(mode))
        .unwrap_or(0);
    let _ = app.emit(EVENT_STOP, StopPayload { mode, session });
}

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
    // Reposition before showing so the pill follows the user across
    // displays. Without this it sticks to whichever monitor it was placed
    // on at startup (usually the primary), out of sight when the user is
    // working on a secondary screen.
    crate::position_overlay_top_center(app);
    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.show();
    }
    // Tray icon tints + tooltip swaps to the recording indicator alongside
    // the overlay so the menu bar shows mic state even when the pill is occluded.
    crate::tray::set_recording_state(true);
}

fn hide_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.hide();
    }
    crate::tray::set_recording_state(false);
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
                    on_press(app, "dictation");
                }
                ("dictation", ShortcutState::Released) => {
                    log::info!("dictation hotkey released");
                    on_release(app, "dictation");
                }
                ("action", ShortcutState::Pressed) => {
                    log::info!("action hotkey pressed");
                    on_press(app, "action");
                }
                ("action", ShortcutState::Released) => {
                    log::info!("action hotkey released");
                    on_release(app, "action");
                }
                ("prompt", ShortcutState::Pressed) => {
                    log::info!("prompt hotkey pressed");
                    on_press(app, "prompt");
                }
                ("prompt", ShortcutState::Released) => {
                    log::info!("prompt hotkey released");
                    on_release(app, "prompt");
                }
                ("cancel", ShortcutState::Pressed) => {
                    log::info!("cancel hotkey pressed");
                    crate::app_detector::clear_target_app();
                    crate::sounds::play(app, crate::sounds::Cue::Cancel);
                    hide_overlay(app);
                    // Aborts the in-flight pipeline backend-side: cancels any
                    // running STT/LLM request and blocks injection (issue #31).
                    crate::session::cancel_active();
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
    if let Err(e) = gs.register(new) {
        // New combo failed (e.g. already claimed by another app). Restore the
        // previous shortcut so the action is not left without a hotkey.
        if let Some(prev) = previous {
            let _ = gs.register(prev);
        }
        return Err(anyhow!("register {combo}: {e}"));
    }

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
