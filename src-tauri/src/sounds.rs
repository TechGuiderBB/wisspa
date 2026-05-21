//! Audible cues for recording start / stop / cancel / timeout.
//!
//! Uses macOS built-in system sounds via `afplay` so we don't bundle any
//! audio files. `play_sounds` and `sound_volume` from settings.json gate
//! whether anything plays.

use tauri::{AppHandle, Runtime};

pub enum Cue {
    Start,
    Stop,
    Cancel,
    Timeout,
}

impl Cue {
    fn system_sound(&self) -> &'static str {
        match self {
            // Crisp ascending click — recording is now on.
            Cue::Start => "/System/Library/Sounds/Pop.aiff",
            // Soft descending blip — recording stopped, sending to STT.
            Cue::Stop => "/System/Library/Sounds/Tink.aiff",
            // Distinct cancel sound.
            Cue::Cancel => "/System/Library/Sounds/Funk.aiff",
            // Warning chime — recording auto-stopped.
            Cue::Timeout => "/System/Library/Sounds/Sosumi.aiff",
        }
    }
}

pub fn play<R: Runtime>(app: &AppHandle<R>, cue: Cue) {
    let settings = match crate::settings_store::load(app) {
        Ok(s) => s,
        Err(_) => return,
    };
    if !settings.general.play_sounds {
        return;
    }
    if matches!(cue, Cue::Start) && !settings.general.ready_chime {
        return;
    }
    let path = cue.system_sound();
    // Volume scales 0.0–1.0 to afplay's -v 0..2 range (1.0 is full).
    let volume = settings.general.sound_volume.clamp(0.0, 1.0);
    tauri::async_runtime::spawn(async move {
        let _ = tokio::process::Command::new("afplay")
            .args(["-v", &format!("{:.2}", volume), path])
            .output()
            .await;
    });
}
