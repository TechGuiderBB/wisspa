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
/// frontend last saw. In toggle mode this map doubles as the "is this mode
/// recording?" flag: present = a press started it, absent = idle.
fn recording_sessions() -> &'static Mutex<HashMap<&'static str, u64>> {
    static M: OnceLock<Mutex<HashMap<&'static str, u64>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop every in-flight press→session binding. Terminal paths that end a
/// recording without a Released edge (Esc cancel, timeout auto-stop) call this
/// so toggle mode doesn't read a stale binding as "recording active" and
/// swallow the next press as a no-op stop.
fn clear_recording_sessions() {
    if let Ok(mut m) = recording_sessions().lock() {
        m.clear();
    }
}

/// Behaviour flags the global-shortcut hot path needs on every event. Cached
/// here so a hotkey press does no settings.json disk I/O; refreshed at startup
/// (`register_default_shortcuts`) and on every save (`commands::save_settings`).
#[derive(Clone)]
struct HotkeyBehavior {
    recording_mode: String,
    show_overlay: bool,
}

fn behavior_cache() -> &'static Mutex<HotkeyBehavior> {
    static B: OnceLock<Mutex<HotkeyBehavior>> = OnceLock::new();
    B.get_or_init(|| {
        Mutex::new(HotkeyBehavior {
            recording_mode: "press_and_hold".to_string(),
            show_overlay: true,
        })
    })
}

fn cached_behavior() -> HotkeyBehavior {
    behavior_cache()
        .lock()
        .map(|b| b.clone())
        .unwrap_or_else(|_| HotkeyBehavior {
            recording_mode: "press_and_hold".to_string(),
            show_overlay: true,
        })
}

/// Refresh the cached hotkey behaviour from settings so recording-mode and
/// overlay-visibility changes apply live, without an app restart.
pub fn cache_behavior(settings: &crate::settings_store::Settings) {
    if let Ok(mut b) = behavior_cache().lock() {
        b.recording_mode = settings.general.recording_mode.clone();
        b.show_overlay = settings.general.show_overlay;
    }
}

/// What a Pressed edge should do for a recording hotkey.
#[derive(Debug, PartialEq, Eq)]
enum PressAction {
    Start,
    Stop,
}

/// Toggle mode flips between start and stop on each Pressed edge;
/// press-and-hold always starts on press (its Released edge stops). Pure so it
/// can be unit-tested without an AppHandle.
fn press_action(recording_mode: &str, session_active: bool) -> PressAction {
    if recording_mode == "toggle" && session_active {
        PressAction::Stop
    } else {
        PressAction::Start
    }
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

fn handle_press<R: Runtime>(app: &AppHandle<R>, mode: &'static str) {
    log::info!("{mode} hotkey pressed");
    let behavior = cached_behavior();
    let session_active = recording_sessions()
        .lock()
        .ok()
        .map(|m| m.contains_key(mode))
        .unwrap_or(false);
    match press_action(&behavior.recording_mode, session_active) {
        PressAction::Start => on_press(app, mode),
        // Toggle mode: the second press stops and processes — exactly what a
        // push-to-talk release does — so both paths share `on_release`.
        PressAction::Stop => on_release(app, mode),
    }
}

fn handle_release<R: Runtime>(app: &AppHandle<R>, mode: &'static str) {
    if cached_behavior().recording_mode == "toggle" {
        // Toggle start/stop both live on the Pressed edge; the Released edge
        // of a toggle tap must not stop the recording that press just started.
        return;
    }
    log::info!("{mode} hotkey released");
    on_release(app, mode);
}

/// A recording ended with no Released edge to come (max-duration auto-stop):
/// clear the toggle bookkeeping, and in toggle mode hide the overlay too — in
/// press-and-hold the user's release still follows and runs `on_release`, so
/// nothing changes there beyond the binding clear it would have done anyway.
pub fn recording_ended_without_release<R: Runtime>(app: &AppHandle<R>) {
    clear_recording_sessions();
    if cached_behavior().recording_mode == "toggle" {
        hide_overlay(app);
    }
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
    // Honour the "Show recording overlay" setting: when off, the pill window
    // stays hidden — the tray icon below and the runtime pill's status flash
    // still signal that recording is live.
    if cached_behavior().show_overlay {
        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
            let _ = w.show();
        }
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
                ("dictation", ShortcutState::Pressed) => handle_press(app, "dictation"),
                ("dictation", ShortcutState::Released) => handle_release(app, "dictation"),
                ("action", ShortcutState::Pressed) => handle_press(app, "action"),
                ("action", ShortcutState::Released) => handle_release(app, "action"),
                ("prompt", ShortcutState::Pressed) => handle_press(app, "prompt"),
                ("prompt", ShortcutState::Released) => handle_release(app, "prompt"),
                ("cancel", ShortcutState::Pressed) => {
                    // Esc is registered as a GLOBAL shortcut, so it fires on
                    // every Esc press in every app. Without a live recording
                    // or pipeline there is nothing to cancel — no-op instead
                    // of playing the cancel sound and churning tray/overlay
                    // state into an unrelated app. Deliberately not logged:
                    // same activity-recording concern as suppressed toasts
                    // (toast.rs). `has_active` stays true from hotkey press
                    // until the pipeline retires the session, so cancelling
                    // mid-recording or mid-pipeline is unchanged.
                    if !crate::session::has_active() {
                        return;
                    }
                    log::info!("cancel hotkey pressed");
                    crate::app_detector::clear_target_app();
                    crate::sounds::play(app, crate::sounds::Cue::Cancel);
                    hide_overlay(app);
                    // Aborts the in-flight pipeline backend-side: cancels any
                    // running STT/LLM request and blocks injection (issue #31).
                    crate::session::cancel_active();
                    // The recording ended without a Released edge — drop the
                    // press→session bindings so the next toggle-mode press
                    // starts fresh instead of stopping a stale binding.
                    clear_recording_sessions();
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
    // Seed the hot-path behaviour cache (recording mode, overlay visibility);
    // `commands::save_settings` refreshes it on every later change.
    cache_behavior(&settings);
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

#[cfg(test)]
mod tests {
    use super::{press_action, PressAction};

    #[test]
    fn press_and_hold_always_starts_on_press() {
        assert_eq!(press_action("press_and_hold", false), PressAction::Start);
        // A stale binding never turns a press-and-hold press into a stop —
        // only its Released edge stops the recording.
        assert_eq!(press_action("press_and_hold", true), PressAction::Start);
    }

    #[test]
    fn toggle_alternates_start_and_stop_on_the_pressed_edge() {
        assert_eq!(press_action("toggle", false), PressAction::Start);
        assert_eq!(press_action("toggle", true), PressAction::Stop);
    }
}
