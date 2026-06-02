use crate::{
    actions::registry, history, hotkeys, keychain, modes::action as action_mode,
    modes::dictation, modes::prompt as prompt_mode, permissions, settings_store, stt, toast,
    AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// Event consumed by the runtime pill so it can flash a brief warning state
/// even when the user has macOS notifications muted or isn't watching the
/// top-right corner of the screen.
const STATUS_EVENT: &str = "wisspa://recording-status";

fn emit_status<R: Runtime>(app: &AppHandle<R>, kind: &str, message: &str) {
    let _ = app.emit(STATUS_EVENT, serde_json::json!({
        "kind": kind,        // "no-speech" | "error"
        "message": message,
    }));
}

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[tauri::command]
pub fn get_settings<R: Runtime>(app: AppHandle<R>) -> Result<settings_store::Settings, String> {
    settings_store::load(&app).map_err(|e| format!("load settings: {e:#}"))
}

#[tauri::command]
pub fn save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: settings_store::Settings,
) -> Result<(), String> {
    settings_store::save(&app, &settings).map_err(|e| format!("save settings: {e:#}"))?;
    // Apply the verbose-logging toggle immediately so it takes effect for every
    // command path (actions, diagnostics export), not just the next dictation.
    crate::redact::set_verbose(settings.general.verbose_logging);
    // Apply pre-warm changes live so `fast_recording_start` (and any hotkey
    // change) takes effect without an app restart.
    let masks = crate::prearm::collect_masks(&[
        &settings.hotkeys.dictation,
        &settings.hotkeys.action,
        &settings.hotkeys.prompt,
    ]);
    crate::prearm::apply(&app, settings.general.fast_recording_start, masks);
    Ok(())
}

#[tauri::command]
pub fn get_api_key_present(key: String) -> Result<bool, String> {
    let known = keychain::known_keys();
    if !known.contains(&key.as_str()) {
        return Err(format!("unknown key {key}"));
    }
    // Avoid prompting Keychain on every Settings open in dev: if the env has
    // the value, treat as present without touching Keychain. The signed
    // production bundle will hit Keychain normally.
    if std::env::var(&key).map(|v| !v.is_empty()).unwrap_or(false) {
        return Ok(true);
    }
    keychain::get(&key)
        .map(|v| v.is_some_and(|s| !s.is_empty()))
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn save_api_key(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let known = keychain::known_keys();
    if !known.contains(&key.as_str()) {
        return Err(format!("unknown key {key}"));
    }
    if value.is_empty() {
        keychain::delete(&key).map_err(|e| format!("delete: {e:#}"))?;
    } else {
        keychain::set(&key, &value).map_err(|e| format!("set: {e:#}"))?;
    }
    // Propagate to in-memory state so subsequent dictations pick it up immediately.
    match key.as_str() {
        "GROQ_API_KEY" => state.set_groq_key(value),
        "ANTHROPIC_API_KEY" => state.set_anthropic_key(value),
        _ => unreachable!(),
    }
    Ok(())
}

#[tauri::command]
pub async fn test_api_key(provider: String, value: String) -> Result<String, String> {
    if value.is_empty() {
        return Err("empty key".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let (url, header_name, header_value) = match provider.as_str() {
        "groq" => (
            "https://api.groq.com/openai/v1/models",
            "Authorization".to_string(),
            format!("Bearer {value}"),
        ),
        "anthropic" => (
            "https://api.anthropic.com/v1/models",
            "x-api-key".to_string(),
            value.clone(),
        ),
        other => return Err(format!("unknown provider {other}")),
    };
    let mut req = client.get(url).header(&header_name, &header_value);
    if provider == "anthropic" {
        req = req.header("anthropic-version", "2023-06-01");
    }
    let res = req.send().await.map_err(|e| format!("send: {e}"))?;
    let status = res.status();
    if status.is_success() {
        Ok(format!("OK ({status})"))
    } else {
        let body = res.text().await.unwrap_or_default();
        Err(format!("HTTP {status}: {body}"))
    }
}

#[tauri::command]
pub fn update_hotkey<R: Runtime>(
    app: AppHandle<R>,
    action: String,
    combo: String,
) -> Result<(), String> {
    hotkeys::reassign(&app, &action, &combo).map_err(|e| format!("reassign: {e:#}"))
}

#[tauri::command]
pub fn pause_hotkeys<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    hotkeys::pause_all(&app).map_err(|e| format!("pause: {e:#}"))
}

#[tauri::command]
pub fn resume_hotkeys<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    hotkeys::resume_all(&app).map_err(|e| format!("resume: {e:#}"))
}

#[tauri::command]
pub fn open_settings_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.unminimize();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_permissions() -> permissions::PermissionsSnapshot {
    permissions::snapshot().await
}

#[tauri::command]
pub fn report_microphone_status(granted: bool) {
    permissions::set_microphone_status(granted);
}

#[tauri::command]
pub fn open_system_settings(pane: String) -> Result<(), String> {
    permissions::open_settings_for(&pane).map_err(|e| format!("open: {e}"))
}

#[tauri::command]
pub fn request_screen_recording_access() {
    permissions::request_screen_recording_access();
}

#[tauri::command]
pub async fn report_recording_timeout<R: Runtime>(
    app: AppHandle<R>,
    max_seconds: u32,
) -> Result<(), String> {
    log::warn!("recording exceeded {max_seconds}s cap — auto-stopped by frontend timer");
    crate::sounds::play(&app, crate::sounds::Cue::Timeout);
    let active_app = crate::app_detector::frontmost_app_name().await.ok();
    let _ = history::insert(history::NewEntry {
        mode: "timeout".to_string(),
        active_app,
        raw_transcript: String::new(),
        output: Some(format!("(recording exceeded {max_seconds}s cap)")),
        action_id: None,
        duration_ms: Some(max_seconds as i64 * 1000),
        status: "cancelled".to_string(),
    });
    toast::warn(&app, "Recording stopped", &format!("Exceeded the {max_seconds}s recording limit."));
    Ok(())
}

#[tauri::command]
pub async fn report_silent_recording<R: Runtime>(
    app: AppHandle<R>,
    mode: String,
    duration_ms: i64,
    peak_amplitude: f32,
    bytes: i64,
) -> Result<(), String> {
    log::info!(
        "peak={peak_amplitude:.2} bytes={bytes} duration={duration_ms}ms — silent recording suppressed (mode={mode})"
    );
    let active_app = crate::app_detector::frontmost_app_name().await.ok();
    let _ = history::insert(history::NewEntry {
        mode: mode.clone(),
        active_app,
        raw_transcript: String::new(),
        output: Some("(no speech detected)".to_string()),
        action_id: None,
        duration_ms: Some(duration_ms),
        status: "cancelled".to_string(),
    });
    // Surface only via the pill flash. The macOS banner was noisy and
    // redundant on top of the in-app indicator.
    emit_status(&app, "no-speech", "No speech detected");
    Ok(())
}

#[tauri::command]
pub fn get_history(limit: Option<i64>) -> Result<Vec<history::Entry>, String> {
    history::recent(limit.unwrap_or(100)).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn clear_history() -> Result<(), String> {
    history::clear_all().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn export_history_csv() -> Result<String, String> {
    history::export_csv().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn complete_onboarding<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let mut settings = settings_store::load(&app).map_err(|e| format!("load: {e:#}"))?;
    settings.onboarding_completed = true;
    settings_store::save(&app, &settings).map_err(|e| format!("save: {e:#}"))?;
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.hide();
    }
    Ok(())
}

#[tauri::command]
pub fn list_actions() -> Vec<crate::actions::Action> {
    registry::snapshot()
}

#[tauri::command]
pub async fn process_audio<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    audio_b64: String,
    mime_type: String,
    mode: Option<String>,
) -> Result<String, String> {
    let mode = mode.unwrap_or_else(|| "dictation".to_string());
    let bytes = STANDARD
        .decode(audio_b64.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;

    log::info!("process_audio: {} bytes, mime={mime_type}", bytes.len());

    let groq_key = state.groq_key();
    // `settings` is owned and lives for the whole function, so its fields can
    // be borrowed directly (including across the await) — no clones needed.
    let settings = settings_store::load(&app).unwrap_or_default();
    // Honour the verbose-logging toggle without requiring an app restart.
    crate::redact::set_verbose(settings.general.verbose_logging);
    let vocab_hint: Option<String> = if settings.vocabulary.is_empty() {
        None
    } else {
        let words: Vec<&str> = settings
            .vocabulary
            .iter()
            .map(|v| v.replace_with.as_str())
            .collect();
        Some(words.join(", "))
    };
    let transcript = match stt::transcribe_audio(
        &groq_key,
        bytes,
        &mime_type,
        &settings.stt.language,
        &settings.stt.model,
        vocab_hint.as_deref(),
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            log::error!("STT failed: {e:#}");
            toast::error(&app, "Transcription failed", &format!("{e:#}"));
            return Err(format!("transcribe: {e:#}"));
        }
    };

    log::info!("transcript ({mode}): {}", crate::redact::redact(&transcript));

    if transcript.is_empty() {
        log::warn!("empty transcript; no speech detected");
        emit_status(&app, "no-speech", "No speech detected");
        let _ = history::insert(history::NewEntry {
            mode: mode.clone(),
            raw_transcript: String::new(),
            status: "cancelled".to_string(),
            ..Default::default()
        });
        return Ok(String::new());
    }

    // Whisper hallucination filter: when the model is fed near-silent or
    // noise-only audio, it falls back to high-probability outro phrases from
    // its training data ("Thank you", "Thanks for watching", "Salam", etc.).
    // Suppress these before they reach the cleanup LLM (which itself can
    // hallucinate a chatbot response on top of the garbage).
    if looks_like_whisper_hallucination(&transcript) {
        log::warn!(
            "suppressing likely Whisper hallucination: {}",
            crate::redact::redact(&transcript)
        );
        emit_status(&app, "no-speech", "No speech detected");
        let _ = history::insert(history::NewEntry {
            mode: mode.clone(),
            raw_transcript: transcript.clone(),
            output: Some("(suppressed Whisper hallucination)".to_string()),
            status: "cancelled".to_string(),
            ..Default::default()
        });
        return Ok(String::new());
    }

    // Vocabulary substitution is for dictation/prompt output only. Applying it
    // before action matching would corrupt trigger-word lookups for any user
    // whose vocabulary overlaps with their action triggers.
    let transcript = match mode.as_str() {
        "action" => transcript,
        _ => settings_store::apply_vocabulary(&transcript, &settings.vocabulary),
    };

    let started = std::time::Instant::now();
    // action mode returns (text, matched_action_id) so history can record which action ran.
    let (result, matched_action_id): (Result<String, String>, Option<String>) =
        match mode.as_str() {
            "action" => {
                let r = run_action_mode(&app, &transcript).await;
                match r {
                    Ok((text, aid)) => (Ok(text), aid),
                    Err(e) => (Err(e), None),
                }
            }
            "prompt" => (run_prompt_mode(&app, &state, &transcript).await, None),
            _ => (run_dictation_mode(&app, &state, &transcript).await, None),
        };
    let duration_ms = started.elapsed().as_millis() as i64;
    let active_app = crate::app_detector::frontmost_app_name().await.ok();
    let (status, output) = match &result {
        Ok(text) if !text.is_empty() => ("success", Some(text.clone())),
        Ok(_) => ("success", None),
        Err(e) if e == crate::hotkeys::CANCELLED_MARKER => ("cancelled", None),
        Err(e) => ("failure", Some(e.clone())),
    };
    let _ = history::insert(history::NewEntry {
        mode: mode.clone(),
        active_app,
        raw_transcript: transcript.clone(),
        output,
        action_id: matched_action_id,
        duration_ms: Some(duration_ms),
        status: status.to_string(),
    });
    // User-initiated cancel is not an error from the frontend's perspective —
    // suppress the Err so processAudio doesn't surface it as a failure.
    match result {
        Err(e) if e == crate::hotkeys::CANCELLED_MARKER => Ok(String::new()),
        other => other,
    }
}

async fn run_prompt_mode<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    transcript: &str,
) -> Result<String, String> {
    let anthropic_key = state.anthropic_key();
    match prompt_mode::run(app, &anthropic_key, transcript).await {
        Ok(outcome) => {
            let preview = preview(&outcome.inserted);
            let mut suffix = Vec::new();
            if outcome.selection_captured {
                suffix.push("with selected context");
            }
            if outcome.manual_app_override_used {
                suffix.push("manual target");
            }
            let body = if suffix.is_empty() {
                preview
            } else {
                format!("{preview} ({})", suffix.join(", "))
            };
            // No success banner — the rewritten prompt appears in the focused
            // app the moment it's pasted. Banner was redundant noise.
            let _ = body;
            Ok(outcome.inserted)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg == crate::hotkeys::CANCELLED_MARKER {
                log::info!("prompt cancelled by user (Esc)");
                return Err(crate::hotkeys::CANCELLED_MARKER.to_string());
            }
            log::error!("prompt mode failed: {e:#}");
            toast::error(app, "Prompt failed", &msg);
            Err(format!("prompt: {msg}"))
        }
    }
}

async fn run_dictation_mode<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    transcript: &str,
) -> Result<String, String> {
    let anthropic_key = state.anthropic_key();
    match dictation::run(app, &anthropic_key, transcript).await {
        Ok(outcome) => {
            // Success path: the cleaned transcript appears in the focused app
            // the moment it's pasted, so a macOS banner is redundant. Keep the
            // raw/long warnings — those are meaningful state the user can't
            // see in the pasted text alone.
            let preview = preview(&outcome.inserted);
            if !outcome.cleaned {
                toast::warn(app, "Inserted (raw)", &format!("Cleanup unavailable — pasted raw. {preview}"));
            } else if outcome.long_transcript {
                toast::warn(app, "Inserted (long)", &format!("Transcript >2000 chars. {preview}"));
            }
            Ok(outcome.inserted)
        }
        Err(e) => {
            log::error!("dictation pipeline failed: {e:#}");
            toast::error(app, "Insertion failed", &format!("{e:#}"));
            Err(format!("dictation: {e:#}"))
        }
    }
}

async fn run_action_mode<R: Runtime>(
    app: &AppHandle<R>,
    transcript: &str,
) -> Result<(String, Option<String>), String> {
    match action_mode::run(app, transcript).await {
        Ok(outcome) => {
            let action_id = outcome.matched_action_id.clone();
            if outcome.success {
                toast::info(app, "Action", &outcome.message);
                Ok((
                    outcome.matched_action_id.unwrap_or_else(|| outcome.message.clone()),
                    action_id,
                ))
            } else {
                toast::warn(app, "Action", &outcome.message);
                Ok((outcome.message, action_id))
            }
        }
        Err(e) => {
            log::error!("action mode failed: {e:#}");
            toast::error(app, "Action failed", &format!("{e:#}"));
            Err(format!("action: {e:#}"))
        }
    }
}

/// Known low-information Whisper outputs that the model emits when fed
/// silence or noise. Matching is case-insensitive, punctuation-tolerant.
const WHISPER_HALLUCINATIONS: &[&str] = &[
    "thank you",
    "thanks",
    "thanks for watching",
    "thank you for watching",
    "thank you for listening",
    "thanks for listening",
    "thanks for joining",
    "thank you so much",
    "bye",
    "goodbye",
    "subscribe",
    "please subscribe",
    "like and subscribe",
    "you",
    "music",
    "applause",
    "silence",
    "salam",
    "salam forgiveness",
    "the end",
    "end of recording",
    "amen",
];

fn normalise_for_match(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_whisper_hallucination(text: &str) -> bool {
    let n = normalise_for_match(text);
    if n.is_empty() {
        return true;
    }
    // Denylist-only: legitimate short dictations like "Yes", "No", "OK"
    // must not be suppressed. The list captures Whisper's known fallback
    // phrases for silent / noise input.
    WHISPER_HALLUCINATIONS.contains(&n.as_str())
}

fn preview(text: &str) -> String {
    const MAX: usize = 50;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

// ── Word corrections ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_word_corrections<R: Runtime>(
    app: AppHandle<R>,
) -> Result<settings_store::WordCorrections, String> {
    settings_store::load(&app)
        .map(|s| s.word_corrections)
        .map_err(|e| format!("{e:#}"))
}

/// Record a correction (original → replacement). Increments the count; marks
/// `auto_apply` once count reaches the configured threshold. Returns whether
/// this correction is now auto-applying.
#[tauri::command]
pub fn submit_word_correction<R: Runtime>(
    app: AppHandle<R>,
    original: String,
    replacement: String,
) -> Result<bool, String> {
    settings_store::record_correction(&app, &original, &replacement)
        .map(|(now_auto, _)| now_auto)
        .map_err(|e| format!("{e:#}"))
}

/// Overwrite the full word corrections object — used by the settings UI.
#[tauri::command]
pub fn save_word_corrections<R: Runtime>(
    app: AppHandle<R>,
    corrections: settings_store::WordCorrections,
) -> Result<(), String> {
    let mut settings = settings_store::load(&app).map_err(|e| format!("{e:#}"))?;
    settings.word_corrections = corrections;
    settings_store::save(&app, &settings).map_err(|e| format!("{e:#}"))
}

// ── Diagnostics export ───────────────────────────────────────────────────────

/// Bundle the (already-redacted) log files plus app/version/permission/hotkey
/// context into a `.zip` the user can attach to a bug report. Returns the path.
///
/// Deliberately excludes `history.db` (holds transcripts) and the raw
/// `settings.json` (holds vocabulary, paths, future licence state) — only the
/// hotkey config and a couple of booleans are surfaced. The log files are safe
/// to include because content is redacted at write time (issue #33).
#[tauri::command]
pub fn export_diagnostics<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    use std::io::Write;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let out_dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| format!("no output directory: {e}"))?;
    let zip_path = out_dir.join(format!("wisspa-diagnostics-{ts}.zip"));

    let file = std::fs::File::create(&zip_path).map_err(|e| format!("create zip: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let summary = diagnostics_summary(&app);
    zip.start_file("diagnostics.txt", opts)
        .map_err(|e| format!("zip entry: {e}"))?;
    zip.write_all(summary.as_bytes())
        .map_err(|e| format!("zip write: {e}"))?;

    let log_dir = crate::logging::log_dir();
    for name in ["wisspa.log", "wisspa.1.log", "wisspa.2.log", "wisspa.3.log"] {
        // A rotated file may legitimately not exist yet — skip those. But once a
        // file is read, surface any zip write failure rather than returning a
        // silently-incomplete archive.
        let Ok(bytes) = std::fs::read(log_dir.join(name)) else {
            continue;
        };
        zip.start_file(name, opts)
            .map_err(|e| format!("zip entry {name}: {e}"))?;
        zip.write_all(&bytes)
            .map_err(|e| format!("zip write {name}: {e}"))?;
    }

    zip.finish().map_err(|e| format!("finalise zip: {e}"))?;
    Ok(zip_path.display().to_string())
}

fn diagnostics_summary<R: Runtime>(app: &AppHandle<R>) -> String {
    let settings = settings_store::load(app).unwrap_or_default();
    let ax = crate::injector::accessibility_trusted();
    format!(
        "Wisspa diagnostics\n\
         version: {}\n\
         log file: {}\n\
         accessibility_trusted: {ax}\n\
         verbose_logging: {}\n\
         hotkeys: dictation={} action={} prompt={} cancel={}\n\
         \nNote: transcripts, LLM output and clipboard/selection values are\n\
         redacted in the logs unless verbose logging was enabled.\n",
        env!("CARGO_PKG_VERSION"),
        crate::logging::log_path().display(),
        settings.general.verbose_logging,
        settings.hotkeys.dictation,
        settings.hotkeys.action,
        settings.hotkeys.prompt,
        settings.hotkeys.cancel,
    )
}
