use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub general: General,
    pub hotkeys: Hotkeys,
    pub prompt_mode: PromptMode,
    pub stt: Stt,
    pub cleanup_llm: Llm,
    pub prompt_llm: Llm,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub mic_calibration: Option<MicCalibration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct General {
    pub launch_on_login: bool,
    pub show_overlay: bool,
    pub recording_mode: String, // "press_and_hold" | "toggle"
    pub play_sounds: bool,
    pub sound_volume: f32,
    pub theme: String, // "system" | "light" | "dark"
    #[serde(default = "default_mic_sensitivity")]
    pub mic_sensitivity: String, // "off" | "low" | "medium" | "high"
    /// Destination file for the `new_note` voice action. Tilde-expanded
    /// before use; parent directory auto-created on first write.
    #[serde(default = "default_notes_path")]
    pub notes_path: String,
    /// Frontend auto-stops and reports to history after this many seconds.
    #[serde(default = "default_max_recording_seconds")]
    pub max_recording_seconds: u32,
}

fn default_mic_sensitivity() -> String {
    "medium".to_string()
}

fn default_max_recording_seconds() -> u32 {
    30
}

fn default_notes_path() -> String {
    "~/Documents/voice-notes.md".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicCalibration {
    pub silence_peak: f32,
    pub min_bytes_per_second: u32,
    pub calibrated_at: i64, // unix ms
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkeys {
    pub dictation: String,
    pub action: String,
    pub prompt: String,
    pub cancel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMode {
    pub include_selected_text: bool,
    pub show_preview: bool,
    pub preview_timeout_seconds: u32,
    pub manual_app_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stt {
    pub provider: String,
    pub model: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Llm {
    pub provider: String,
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            general: General {
                launch_on_login: false,
                show_overlay: true,
                recording_mode: "press_and_hold".to_string(),
                play_sounds: true,
                sound_volume: 0.5,
                theme: "system".to_string(),
                mic_sensitivity: "medium".to_string(),
                notes_path: default_notes_path(),
                max_recording_seconds: 30,
            },
            hotkeys: Hotkeys {
                // Phase 1/2 ships with safe combos; PRD §4.2 defaults to `fn` etc.
                // but those need the Phase 3 key-capture component to assign reliably.
                dictation: "CmdOrCtrl+Shift+Space".to_string(),
                action: "CmdOrCtrl+Shift+A".to_string(),
                prompt: "CmdOrCtrl+Shift+P".to_string(),
                cancel: "Escape".to_string(),
            },

            prompt_mode: PromptMode {
                include_selected_text: true,
                show_preview: true,
                preview_timeout_seconds: 5,
                manual_app_override: None,
            },
            stt: Stt {
                provider: "groq".to_string(),
                model: "whisper-large-v3-turbo".to_string(),
                language: "en".to_string(),
            },
            cleanup_llm: Llm {
                provider: "anthropic".to_string(),
                model: "claude-haiku-4-5-20251001".to_string(),
            },
            prompt_llm: Llm {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-6".to_string(),
            },
            onboarding_completed: false,
            mic_calibration: None,
        }
    }
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("app data dir unavailable")?;
    std::fs::create_dir_all(&dir).context("create app data dir")?;
    Ok(dir.join(SETTINGS_FILE))
}

pub fn load<R: Runtime>(app: &AppHandle<R>) -> Result<Settings> {
    let path = settings_path(app)?;
    if !path.exists() {
        let defaults = Settings::default();
        save(app, &defaults)?;
        return Ok(defaults);
    }
    let raw = std::fs::read_to_string(&path).context("read settings.json")?;
    // Best-effort deserialize; on schema drift, fall back to defaults and rewrite.
    match serde_json::from_str::<Settings>(&raw) {
        Ok(s) => Ok(s),
        Err(e) => {
            log::warn!("settings.json invalid, resetting to defaults: {e}");
            // Best-effort backup so the user can recover custom hotkeys /
            // notes_path after a parse failure. Failures here must not block
            // the reset — but they must leave a log trace.
            let bak = backup_path(&path);
            match std::fs::copy(&path, &bak) {
                Ok(_) => log::info!(
                    "backed up corrupt settings.json to {}",
                    bak.display()
                ),
                Err(copy_err) => log::warn!(
                    "failed to back up corrupt settings.json to {}: {copy_err}",
                    bak.display()
                ),
            }
            let defaults = Settings::default();
            save(app, &defaults)?;
            Ok(defaults)
        }
    }
}

fn backup_path(path: &std::path::Path) -> PathBuf {
    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    // Nanosecond resolution so two near-simultaneous corruption-recovery
    // attempts in the same second cannot clobber each other's backups.
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => parent.join(format!("settings.{}.json.bak", d.as_nanos())),
        Err(_) => parent.join("settings.json.bak"),
    }
}

pub fn save<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<()> {
    let path = settings_path(app)?;
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, json).context("write settings.json")?;
    Ok(())
}
