//! macOS modifier-key monitor for mic pre-warm (opt-in `fast_recording_start`).
//!
//! Polls the current keyboard modifier state ~25x/sec via CoreGraphics'
//! `CGEventSourceFlagsState`. This reads the *current* modifier state only —
//! it is not keystroke monitoring, so it needs no Input Monitoring permission.
//! Polling at 40 ms adds negligible latency next to the 200-800 ms cold mic
//! start it lets us avoid.
//!
//! When the modifier portion of a recording hotkey is fully held, the frontend
//! is told to warm the mic (`wisspa://prewarm-mic`) so that completing the
//! combo starts capture instantly. When the modifiers are released without a
//! recording starting, `wisspa://prewarm-cancel` tells the frontend to release
//! the warm stream so the macOS mic indicator clears.

use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

// CGEventFlags modifier bit masks.
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
const MOD_MASK: u64 = FLAG_SHIFT | FLAG_CONTROL | FLAG_ALTERNATE | FLAG_COMMAND;

// kCGEventSourceStateHIDSystemState
const HID_SYSTEM_STATE: i32 = 1;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

fn current_modifiers() -> u64 {
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & MOD_MASK }
}

/// Parse the modifier portion of a hotkey accelerator string
/// (e.g. `"CmdOrCtrl+Shift+Space"`) into a CGEventFlags modifier mask.
/// Non-modifier tokens (the main key) are ignored. Returns 0 if the
/// accelerator has no modifiers — such hotkeys cannot be pre-warmed.
pub fn modifier_mask(accelerator: &str) -> u64 {
    let mut mask = 0u64;
    for token in accelerator.split('+') {
        match token.trim().to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" | "meta" | "cmdorctrl" => mask |= FLAG_COMMAND,
            "ctrl" | "control" => mask |= FLAG_CONTROL,
            "alt" | "option" => mask |= FLAG_ALTERNATE,
            "shift" => mask |= FLAG_SHIFT,
            _ => {}
        }
    }
    mask
}

/// Spawn the modifier monitor. `masks` are the distinct modifier masks of the
/// recording hotkeys; an empty list disables the monitor.
pub fn start<R: Runtime>(app: AppHandle<R>, masks: Vec<u64>) {
    if masks.is_empty() {
        log::info!("prearm: no modifier-bearing hotkeys, monitor not started");
        return;
    }
    log::info!("prearm: modifier monitor started ({} mask(s))", masks.len());

    std::thread::spawn(move || {
        const POLL: Duration = Duration::from_millis(40);
        // Debounce: a brief gap between releasing the modifier and the next
        // press should not tear the warm stream down.
        const COOL_DELAY: Duration = Duration::from_millis(900);
        // Cap how long the mic stays warm if the modifier is held for an
        // unrelated reason, to bound the time the mic indicator is lit.
        const MAX_WARM: Duration = Duration::from_secs(4);

        let mut warm = false;
        let mut warm_since: Option<Instant> = None;
        let mut released_at: Option<Instant> = None;

        loop {
            std::thread::sleep(POLL);
            let mods = current_modifiers();
            let matched = mods != 0 && masks.iter().any(|m| *m == mods);

            if matched && !warm {
                warm = true;
                warm_since = Some(Instant::now());
                released_at = None;
                let _ = app.emit("wisspa://prewarm-mic", ());
            } else if matched && warm {
                released_at = None;
                if warm_since.map(|t| t.elapsed() >= MAX_WARM).unwrap_or(false) {
                    warm = false;
                    warm_since = None;
                    let _ = app.emit("wisspa://prewarm-cancel", ());
                }
            } else if !matched && warm {
                match released_at {
                    None => released_at = Some(Instant::now()),
                    Some(t) if t.elapsed() >= COOL_DELAY => {
                        warm = false;
                        warm_since = None;
                        released_at = None;
                        let _ = app.emit("wisspa://prewarm-cancel", ());
                    }
                    _ => {}
                }
            }
        }
    });
}
