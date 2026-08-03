use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VocabEntry {
    pub spoken: String,
    pub replace_with: String,
}

fn default_vocabulary() -> Vec<VocabEntry> {
    vec![
        VocabEntry {
            spoken: "Lisa".to_string(),
            replace_with: "LeaseR".to_string(),
        },
        VocabEntry {
            spoken: "Whisper".to_string(),
            replace_with: "Wisspa".to_string(),
        },
    ]
}

/// Replace occurrences of each `spoken` word with `replace_with`, matching
/// whole words case-insensitively (so "whisper" and "Whisper" both become
/// "Wisspa"). No regex crate required.
pub fn apply_vocabulary(text: &str, vocab: &[VocabEntry]) -> String {
    let mut result = text.to_string();
    for entry in vocab {
        if entry.spoken.is_empty() || entry.replace_with.is_empty() {
            continue;
        }
        result = replace_word_ci(&result, &entry.spoken, &entry.replace_with);
    }
    result
}

/// A character that counts as part of a word for whole-word matching.
/// Digits and `_` are included so `whisper2` and `leaseR_test` are not
/// treated as the bare words `whisper` / `leaseR`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn replace_word_ci(text: &str, from: &str, to: &str) -> String {
    let from_lower = from.to_lowercase();
    let from_chars: Vec<char> = from_lower.chars().collect();
    let from_len = from_chars.len();
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < total {
        let is_word_start = i == 0 || !is_word_char(chars[i - 1]);
        if is_word_start && i + from_len <= total {
            let slice_lower: String = chars[i..i + from_len]
                .iter()
                .collect::<String>()
                .to_lowercase();
            let is_word_end =
                i + from_len == total || !is_word_char(chars[i + from_len]);
            if slice_lower == from_lower && is_word_end {
                out.push_str(to);
                i += from_len;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

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
    #[serde(default = "default_vocabulary")]
    pub vocabulary: Vec<VocabEntry>,
    #[serde(default)]
    pub word_corrections: WordCorrections,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct General {
    pub launch_on_login: bool,
    pub show_overlay: bool,
    pub recording_mode: String, // "press_and_hold" | "toggle"
    pub play_sounds: bool,
    pub sound_volume: f32,
    #[serde(default = "default_mic_sensitivity")]
    pub mic_sensitivity: String, // "off" | "low" | "medium" | "high"
    /// Destination file for the `new_note` voice action. Tilde-expanded
    /// before use; parent directory auto-created on first write.
    #[serde(default = "default_notes_path")]
    pub notes_path: String,
    /// Frontend auto-stops and reports to history after this many seconds.
    #[serde(default = "default_max_recording_seconds")]
    pub max_recording_seconds: u32,
    /// Play the "ready" chime when capture goes live. Gated by play_sounds too.
    #[serde(default = "default_ready_chime")]
    pub ready_chime: bool,
    /// Play a subtle chime the moment dictation finishes and the cleaned
    /// text is injected. Gated by play_sounds too. Off by default so
    /// existing installs keep their current UX.
    #[serde(default)]
    pub dictation_complete_sound: bool,
    /// Opt-in: warm the mic while the hotkey's modifier key is held, so
    /// recording starts instantly. Off by default.
    #[serde(default = "default_fast_recording_start")]
    pub fast_recording_start: bool,
    /// When true, suppress info and warn toasts (the "Inserted (raw)" /
    /// "Inserted (long)" banners on dictation, the auto-learn-correction
    /// confirmation, action result messages). Error toasts always fire —
    /// the user still needs to know when something genuinely failed.
    /// Off by default so existing installs keep their current UX.
    #[serde(default)]
    pub quiet_notifications: bool,
    /// Opt-in: include raw content (transcripts, LLM output, clipboard and
    /// selection values) in the log file. Off by default — normally only a
    /// redacted summary (length + content hash) is logged. The user turns this
    /// on temporarily to capture a bug, then off again (issue #33).
    #[serde(default)]
    pub verbose_logging: bool,
}

fn default_mic_sensitivity() -> String {
    "medium".to_string()
}

fn default_max_recording_seconds() -> u32 {
    30
}

fn default_ready_chime() -> bool {
    true
}

fn default_fast_recording_start() -> bool {
    false
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
    /// Opt-in: after Sonnet rewrites the prompt, open an editable review window
    /// and paste nothing until the user approves. Supersedes `show_preview` when
    /// on. `#[serde(default)]` so existing settings.json files load unchanged.
    #[serde(default)]
    pub review_before_insert: bool,
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
    // Model choice is code-managed (`llm.rs` constants): the schema once
    // carried a `model` field here but nothing ever read it. Legacy keys in
    // existing settings.json files are ignored on load (serde default).
    // A future eval-harnessed change can reintroduce curated model choice.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordCorrectionEntry {
    /// The word or phrase to use instead.
    pub replacement: String,
    /// How many times this correction has been submitted.
    pub count: u32,
    /// True once count reaches the threshold — applied automatically in dictation.
    pub auto_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordCorrections {
    pub enabled: bool,
    /// Number of times a correction must be submitted before it auto-applies.
    pub threshold: u32,
    /// Map from lowercased original word → correction entry.
    pub entries: HashMap<String, WordCorrectionEntry>,
    /// Opt-in: after dictation, take an Accessibility snapshot of the focused
    /// field and learn any single-word edits the user makes. The observed text
    /// is used only transiently for diffing and is never persisted or sent anywhere.
    #[serde(default)]
    pub learn_from_edits: bool,
}

impl Default for WordCorrections {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 3,
            entries: HashMap::new(),
            learn_from_edits: false,
        }
    }
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
                mic_sensitivity: "medium".to_string(),
                notes_path: default_notes_path(),
                max_recording_seconds: default_max_recording_seconds(),
                ready_chime: default_ready_chime(),
                dictation_complete_sound: false,
                fast_recording_start: default_fast_recording_start(),
                quiet_notifications: false,
                verbose_logging: false,
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
                review_before_insert: false,
            },
            stt: Stt {
                provider: "groq".to_string(),
                model: "whisper-large-v3-turbo".to_string(),
                language: "en".to_string(),
            },
            cleanup_llm: Llm {
                provider: "anthropic".to_string(),
            },
            prompt_llm: Llm {
                provider: "anthropic".to_string(),
            },
            onboarding_completed: false,
            mic_calibration: None,
            vocabulary: default_vocabulary(),
            word_corrections: WordCorrections::default(),
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

/// Record a word correction (lowercased `original` → `replacement`), incrementing
/// its count and flipping `auto_apply` once the threshold is reached.
///
/// Returns `(now_auto, just_crossed)`:
/// - `now_auto`: true when `auto_apply` is currently active after this call
/// - `just_crossed`: true when this call caused `auto_apply` to flip from false to true
pub fn record_correction<R: Runtime>(
    app: &AppHandle<R>,
    original: &str,
    replacement: &str,
) -> Result<(bool, bool)> {
    let key = original.trim().to_lowercase();
    let replacement = replacement.trim().to_string();
    if key.is_empty() || replacement.is_empty() {
        return Err(anyhow::anyhow!("original and replacement must not be empty"));
    }
    let mut settings = load(app)?;
    let threshold = settings.word_corrections.threshold;
    let entry = settings
        .word_corrections
        .entries
        .entry(key)
        .or_insert_with(|| WordCorrectionEntry {
            replacement: replacement.clone(),
            count: 0,
            auto_apply: false,
        });
    entry.replacement = replacement;
    entry.count += 1;
    let was_auto = entry.auto_apply;
    if !entry.auto_apply && entry.count >= threshold {
        entry.auto_apply = true;
    }
    let now_auto = entry.auto_apply;
    let just_crossed = !was_auto && now_auto;
    save(app, &settings)?;
    Ok((now_auto, just_crossed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_complete_sound_default_is_off() {
        assert!(!Settings::default().general.dictation_complete_sound);
    }

    #[test]
    fn dictation_complete_sound_round_trips_when_true() {
        let mut settings = Settings::default();
        settings.general.dictation_complete_sound = true;
        let json = serde_json::to_string(&settings).expect("serialize settings");
        let parsed: Settings = serde_json::from_str(&json).expect("deserialize settings");
        assert!(parsed.general.dictation_complete_sound);
    }

    #[test]
    fn dictation_complete_sound_defaults_off_when_key_missing() {
        // Simulate an older settings.json that predates this field: serialize a
        // default Settings, drop the key, and confirm it still loads with the
        // field defaulting to false rather than tripping the corrupt-reset path.
        let mut value = serde_json::to_value(Settings::default()).expect("to value");
        let general = value
            .get_mut("general")
            .and_then(|g| g.as_object_mut())
            .expect("general object");
        general.remove("dictation_complete_sound");
        assert!(general.get("dictation_complete_sound").is_none());

        let parsed = serde_json::from_value::<Settings>(value);
        assert!(parsed.is_ok(), "missing key must deserialize cleanly");
        assert!(!parsed.unwrap().general.dictation_complete_sound);
    }

    #[test]
    fn legacy_theme_and_llm_model_keys_are_ignored() {
        // settings.json written by a build that still had `general.theme` and
        // `cleanup_llm`/`prompt_llm.model` must keep loading after those fields
        // were removed — serde ignores unknown keys, so no migration is needed.
        let mut value = serde_json::to_value(Settings::default()).expect("to value");
        value["general"]["theme"] = serde_json::json!("dark");
        value["cleanup_llm"]["model"] = serde_json::json!("claude-haiku-4-5-20251001");
        value["prompt_llm"]["model"] = serde_json::json!("claude-sonnet-4-6");

        let parsed = serde_json::from_value::<Settings>(value);
        assert!(
            parsed.is_ok(),
            "legacy keys must not break loading: {:?}",
            parsed.err()
        );
        let parsed = parsed.unwrap();
        assert_eq!(parsed.cleanup_llm.provider, "anthropic");
        assert_eq!(parsed.prompt_llm.provider, "anthropic");
    }

    #[test]
    fn stt_model_and_language_round_trip() {
        // The settings UI now writes these; guard the serde contract both ways.
        let mut settings = Settings::default();
        settings.stt.model = "distil-whisper-large-v3-en".to_string();
        settings.stt.language = String::new(); // empty == auto-detect
        let json = serde_json::to_string(&settings).expect("serialize settings");
        let parsed: Settings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(parsed.stt.model, "distil-whisper-large-v3-en");
        assert_eq!(parsed.stt.language, "");
    }

    #[test]
    fn stt_defaults_are_turbo_and_english() {
        let s = Settings::default();
        assert_eq!(s.stt.model, "whisper-large-v3-turbo");
        assert_eq!(s.stt.language, "en");
    }
}
