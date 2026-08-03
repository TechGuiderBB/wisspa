use crate::{
    actions::registry, history, hotkeys, keychain, llm, modes::action as action_mode,
    modes::command as command_mode, modes::dictation, modes::prompt as prompt_mode, permissions,
    settings_store, stt, toast, AppState,
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

/// Toast that an app update is available. Fired by the frontend's silent
/// launch-time update check; the check never auto-downloads — the user
/// installs from Settings → About. Routed through `toast::info` so the
/// quiet-notifications preference is honoured.
#[tauri::command]
pub fn notify_update_available<R: Runtime>(app: AppHandle<R>, version: String) {
    toast::info(
        &app,
        "Update available",
        &format!("Wisspa {version} is available — install it from Settings → About."),
    );
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
    // Refresh the cached hotkey behaviour (recording mode, overlay visibility)
    // so toggle mode and the overlay gate apply without an app restart.
    hotkeys::cache_behavior(&settings);
    // Apply pre-warm changes live so `fast_recording_start` (and any hotkey
    // change) takes effect without an app restart.
    let masks = crate::prearm::collect_masks(&[
        &settings.hotkeys.dictation,
        &settings.hotkeys.action,
        &settings.hotkeys.prompt,
        &settings.hotkeys.command,
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
        // A `known_keys()` entry with no AppState setter means the key is
        // stored but would never take effect in this session. Surface that as
        // a logged error instead of panicking the whole process.
        other => {
            log::error!("save_api_key: known key '{other}' has no AppState setter");
            return Err(format!("key {other} is stored but not wired into app state"));
        }
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
    session: Option<u64>,
) -> Result<(), String> {
    log::warn!("recording exceeded {max_seconds}s cap — auto-stopped by frontend timer");
    // The timeout path never reaches process_audio, so retire the session
    // here — otherwise it stays "live" and the Esc guard keeps firing.
    crate::session::complete(session.unwrap_or(0));
    // No Released edge follows a timeout auto-stop: clear the toggle-mode
    // bookkeeping (and, in toggle mode, hide the overlay — press-and-hold
    // still hides it on the user's release, unchanged).
    hotkeys::recording_ended_without_release(&app);
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
        ..Default::default()
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
    session: Option<u64>,
) -> Result<(), String> {
    log::info!(
        "peak={peak_amplitude:.2} bytes={bytes} duration={duration_ms}ms — silent recording suppressed (mode={mode})"
    );
    // The silence guard short-circuits before process_audio, so retire the
    // session here — otherwise it stays "live" and the Esc guard keeps firing.
    crate::session::complete(session.unwrap_or(0));
    let active_app = crate::app_detector::frontmost_app_name().await.ok();
    let _ = history::insert(history::NewEntry {
        mode: mode.clone(),
        active_app,
        raw_transcript: String::new(),
        output: Some("(no speech detected)".to_string()),
        action_id: None,
        duration_ms: Some(duration_ms),
        status: "cancelled".to_string(),
        ..Default::default()
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

/// Re-inject history text into whatever app is frontmost right now. Unlike the
/// recording pipeline (which captures a target app at hotkey time), re-inject
/// deliberately targets the *live* frontmost app — the user picks the
/// destination by focusing it before clicking. A fresh session is minted so the
/// abort check inside `inject_text` passes; per latest-wins this also supersedes
/// any in-flight recording, which then won't paste stale text on top.
#[tauri::command]
pub async fn reinject_text<R: Runtime>(app: AppHandle<R>, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to re-inject: entry has no text".to_string());
    }
    let session = crate::session::begin();
    log::info!(
        "re-injecting {} chars from history (session {session})",
        text.len()
    );
    crate::injector::inject_text(&app, &text, None, session)
        .await
        .map_err(|e| format!("re-inject: {e:#}"))
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

/// Insert the (possibly edited) reviewed prompt: resolve the backend wait for
/// this recording so `prompt_mode::run` pastes `text`. A stale/superseded
/// session has no pending review, so this is a harmless no-op (still `Ok`).
#[tauri::command]
pub fn submit_prompt_review(session: u64, text: String) -> Result<(), String> {
    crate::prompt_review::resolve(session, crate::prompt_review::ReviewDecision::Insert(text));
    Ok(())
}

/// Cancel the reviewed prompt: resolve the backend wait with Cancel so nothing
/// is pasted. No-op for a stale/superseded session.
#[tauri::command]
pub fn cancel_prompt_review(session: u64) -> Result<(), String> {
    crate::prompt_review::resolve(session, crate::prompt_review::ReviewDecision::Cancel);
    Ok(())
}

/// Upper bound for the comma-joined vocabulary hint sent as Whisper's
/// `prompt`. Whisper's prompt window is ~224 tokens; at roughly 2–3 chars per
/// token for short vocabulary words, ~800 chars stays comfortably inside it.
/// Past the window Groq truncates the prompt arbitrarily (or rejects the
/// request), which could silently drop the hint entirely.
const VOCAB_HINT_MAX_CHARS: usize = 800;

/// Build the comma-joined vocabulary hint for Whisper's `prompt` field,
/// capped at [`VOCAB_HINT_MAX_CHARS`]. When the joined words exceed the cap,
/// the FIRST entries are kept (the user's most-established vocabulary) and
/// the cut is made at a word boundary so no partial word is sent.
fn build_vocab_hint(vocabulary: &[settings_store::VocabEntry]) -> Option<String> {
    if vocabulary.is_empty() {
        return None;
    }
    let words: Vec<&str> = vocabulary
        .iter()
        .map(|v| v.replace_with.as_str())
        .collect();
    let joined = words.join(", ");
    if joined.len() <= VOCAB_HINT_MAX_CHARS {
        return Some(joined);
    }
    // Cut at the last ", " separator under the cap so only whole words are
    // kept; fall back to a hard char-boundary cut for a single pathological
    // word longer than the cap.
    let mut end = VOCAB_HINT_MAX_CHARS;
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = match joined[..end].rfind(", ") {
        Some(idx) => &joined[..idx],
        None => &joined[..end],
    };
    log::debug!(
        "vocab hint truncated: {} -> {} chars ({} of {} words kept)",
        joined.len(),
        truncated.len(),
        truncated.split(", ").count(),
        words.len(),
    );
    Some(truncated.to_string())
}

#[tauri::command]
pub async fn process_audio<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    audio_b64: String,
    mime_type: String,
    mode: Option<String>,
    session: Option<u64>,
) -> Result<String, String> {
    let mode = mode.unwrap_or_else(|| "dictation".to_string());
    // Session id minted on hotkey press and echoed back by the frontend. 0 =
    // an older frontend with no session plumbing (treated as never-cancelled).
    let session = session.unwrap_or(0);
    // Retire the session on every exit path below (success, error, abort) so
    // the Esc guard's `has_active()` drops once nothing is in flight.
    let _session_completion = crate::session::SessionCompletion::new(session);
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
    // Per-app profiles apply to dictation only (prompt/action modes keep the
    // global vocabulary). The match runs against the press-time app snapshot,
    // peeked rather than consumed — the dictation pipeline takes it later.
    // When the snapshot hasn't landed yet there is simply no profile this run.
    // Profile words go FIRST in the merged list, so the 800-char hint cap
    // below can only ever truncate global entries, never profile words.
    let effective_vocabulary = if mode == "dictation" {
        match crate::app_detector::peek_target_app()
            .and_then(|app| settings_store::match_profile(&settings.profiles, &app))
        {
            Some(p) => settings_store::merge_profile_vocabulary(&settings.vocabulary, &p.vocab),
            None => settings.vocabulary.clone(),
        }
    } else {
        settings.vocabulary.clone()
    };
    let vocab_hint = build_vocab_hint(&effective_vocabulary);
    // Latency clock for the history entry: starts BEFORE the STT round trip so
    // `duration_ms` reflects the real release→insert latency, including the
    // Groq call (previously the clock started after transcription and
    // understated the latency by the whole STT request).
    let started = std::time::Instant::now();
    // Race STT against cancellation. An Esc (or a newer recording) drops the
    // transcribe future, cancelling the in-flight Groq request rather than
    // letting it run to completion and discarding the result (issue #31).
    let transcription = tokio::select! {
        biased;
        _ = crate::session::aborted(session) => {
            log::info!("process_audio aborted during STT (session {session})");
            let _ = history::insert(history::NewEntry {
                mode: mode.clone(),
                status: "cancelled".to_string(),
                ..Default::default()
            });
            return Ok(String::new());
        }
        res = stt::transcribe_audio(
            &groq_key,
            bytes,
            &mime_type,
            &settings.stt.language,
            &settings.stt.model,
            vocab_hint.as_deref(),
        ) => match res {
            Ok(t) => t,
            Err(e) => {
                log::error!("STT failed: {e:#}");
                toast::error(&app, "Transcription failed", &format!("{e:#}"));
                // Persist the failure so it's visible in the History tab —
                // previously an STT error flashed a toast and the utterance
                // vanished without a trace. Failure-row convention: the error
                // summary goes in `output` (there is no transcript to store,
                // and raw_transcript is NOT NULL so it gets an empty string).
                let active_app = crate::app_detector::frontmost_app_name().await.ok();
                let _ = history::insert(history::NewEntry {
                    mode: mode.clone(),
                    active_app,
                    raw_transcript: String::new(),
                    output: Some(format!("transcription failed: {e:#}")),
                    duration_ms: Some(started.elapsed().as_millis() as i64),
                    status: "failure".to_string(),
                    ..Default::default()
                });
                return Err(format!("transcribe: {e:#}"));
            }
        },
    };

    let stt::Transcription {
        text: transcript,
        segments,
    } = transcription;

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

    // Confidence gate: when the model is fed near-silent or noise-only audio,
    // it falls back to high-probability outro phrases from its training data
    // ("Thank you.", "Thanks for watching", etc.). Instead of the old phrase
    // denylist — which also nuked genuine short dictations — suppress only
    // when Whisper's own segment confidence says the audio carried no speech.
    if is_low_confidence(&transcript, &segments) {
        log::warn!(
            "suppressing low-confidence transcript ({} segments): {}",
            segments.len(),
            crate::redact::redact(&transcript)
        );
        emit_status(&app, "no-speech", "No speech detected");
        let _ = history::insert(history::NewEntry {
            mode: mode.clone(),
            raw_transcript: transcript.clone(),
            output: Some("(suppressed low-confidence transcript)".to_string()),
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
        _ => settings_store::apply_vocabulary(&transcript, &effective_vocabulary),
    };

    // Checkpoint between the (now-finished) STT call and the side-effectful
    // mode work: if the user cancelled or started a newer recording while STT
    // was resolving, stop here — no LLM call, no action, no paste (issue #31).
    if crate::session::is_aborted(session) {
        log::info!("process_audio aborted before {mode} dispatch (session {session})");
        let _ = history::insert(history::NewEntry {
            mode: mode.clone(),
            raw_transcript: transcript.clone(),
            status: "cancelled".to_string(),
            ..Default::default()
        });
        return Ok(String::new());
    }

    // action mode returns (text, matched_action_id) so history can record which action ran.
    // prompt mode additionally reports whether it fell back to the raw transcript,
    // so the history row can say so instead of claiming a clean success.
    // `llm_usage` carries the Anthropic token accounting for the run (summed when
    // the mode made two calls); action mode never calls the LLM, and a cancelled
    // or pre-LLM failure path reports no usage — the row's columns stay NULL.
    let (result, matched_action_id, prompt_fallback, llm_usage): (
        Result<String, String>,
        Option<String>,
        bool,
        llm::TokenUsage,
    ) = match mode.as_str() {
        "action" => {
            let r = run_action_mode(&app, &transcript, session).await;
            match r {
                Ok((text, aid)) => (Ok(text), aid, false, llm::TokenUsage::default()),
                Err(e) => (Err(e), None, false, llm::TokenUsage::default()),
            }
        }
        "prompt" => {
            let (r, fallback, usage) = run_prompt_mode(&app, &state, &transcript, session).await;
            (r, None, fallback, usage)
        }
        "command" => {
            let (r, usage) = run_command_mode(&app, &state, &transcript, session).await;
            (r, None, false, usage)
        }
        _ => {
            let (r, usage) = run_dictation_mode(&app, &state, &transcript, session).await;
            (r, None, false, usage)
        }
    };
    let duration_ms = started.elapsed().as_millis() as i64;
    let active_app = crate::app_detector::frontmost_app_name().await.ok();
    let (status, output) = match &result {
        Ok(text) if !text.is_empty() => (
            // A prompt-mode fallback inserted the raw transcript: the paste
            // succeeded but the rewrite did not, so record it distinctly.
            if prompt_fallback { "fallback" } else { "success" },
            Some(text.clone()),
        ),
        Ok(_) => ("success", None),
        Err(e) if e == crate::hotkeys::CANCELLED_MARKER => ("cancelled", None),
        // Command Mode with nothing selected: the warn toast already fired in
        // the mode and no LLM call happened — a cancelled row with the reason,
        // mirroring the "(no speech detected)" convention.
        Err(e) if e == command_mode::NO_SELECTION_MARKER => {
            ("cancelled", Some("(no text selected)".to_string()))
        }
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
        input_tokens: llm_usage.input_tokens.map(|v| v as i64),
        output_tokens: llm_usage.output_tokens.map(|v| v as i64),
    });
    // User-initiated cancel is not an error from the frontend's perspective —
    // suppress the Err so processAudio doesn't surface it as a failure. The
    // no-selection early exit is likewise already communicated by the mode's
    // warn toast, so it returns Ok(empty) to the frontend too.
    match result {
        Err(e) if e == crate::hotkeys::CANCELLED_MARKER => Ok(String::new()),
        Err(e) if e == command_mode::NO_SELECTION_MARKER => Ok(String::new()),
        other => other,
    }
}

/// Returns the pipeline result plus whether the run fell back to the raw
/// transcript (rewrite failed or empty) — the caller records that distinction
/// in the history row's status — plus the run's Anthropic token usage.
async fn run_prompt_mode<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    transcript: &str,
    session: u64,
) -> (Result<String, String>, bool, llm::TokenUsage) {
    let anthropic_key = state.anthropic_key();
    // Up-front preflight: if there's no Anthropic key, fail fast with a
    // specific, actionable toast BEFORE app detection / selection capture
    // (Cmd+C) / route emit / any HTTP call. Never log or surface the key value.
    if prompt_mode::anthropic_key_missing(&anthropic_key) {
        log::warn!("prompt mode aborted: Anthropic API key not configured");
        toast::error(app, "Prompt mode unavailable", prompt_mode::ANTHROPIC_KEY_MISSING_TOAST);
        return (
            Err(format!("prompt: {}", prompt_mode::ANTHROPIC_KEY_MISSING_TOAST)),
            false,
            llm::TokenUsage::default(),
        );
    }
    match prompt_mode::run(app, &anthropic_key, transcript, session).await {
        Ok(outcome) => {
            let fallback = outcome.used_fallback;
            let usage = outcome.usage;
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
            // app the moment it's pasted. Banner was redundant noise. The
            // fallback warning toast already fired in prompt_mode::run.
            let _ = body;
            (Ok(outcome.inserted), fallback, usage)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg == crate::hotkeys::CANCELLED_MARKER {
                log::info!("prompt cancelled by user (Esc)");
                return (
                    Err(crate::hotkeys::CANCELLED_MARKER.to_string()),
                    false,
                    llm::TokenUsage::default(),
                );
            }
            log::error!("prompt mode failed: {e:#}");
            toast::error(app, "Prompt failed", &msg);
            (Err(format!("prompt: {msg}")), false, llm::TokenUsage::default())
        }
    }
}

async fn run_command_mode<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    transcript: &str,
    session: u64,
) -> (Result<String, String>, llm::TokenUsage) {
    let anthropic_key = state.anthropic_key();
    // Up-front preflight: fail fast with a specific, actionable toast BEFORE
    // the selection capture's synthetic Cmd+C touches the user's clipboard or
    // any HTTP call is made. Never log or surface the key value.
    if prompt_mode::anthropic_key_missing(&anthropic_key) {
        log::warn!("command mode aborted: Anthropic API key not configured");
        toast::error(app, "Command mode unavailable", prompt_mode::ANTHROPIC_KEY_MISSING_TOAST);
        return (
            Err(format!("command: {}", prompt_mode::ANTHROPIC_KEY_MISSING_TOAST)),
            llm::TokenUsage::default(),
        );
    }
    match command_mode::run(app, &anthropic_key, transcript, session).await {
        Ok(outcome) => {
            // No success toast — the transformed text appears over the
            // selection the moment it's pasted, exactly like dictation.
            (Ok(outcome.inserted), outcome.usage)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg == crate::hotkeys::CANCELLED_MARKER {
                log::info!("command cancelled by user (Esc)");
                return (
                    Err(crate::hotkeys::CANCELLED_MARKER.to_string()),
                    llm::TokenUsage::default(),
                );
            }
            if msg == command_mode::NO_SELECTION_MARKER {
                // Warn toast already fired in the mode; no error toast here.
                log::info!("command mode: no selection, nothing pasted");
                return (
                    Err(command_mode::NO_SELECTION_MARKER.to_string()),
                    llm::TokenUsage::default(),
                );
            }
            log::error!("command mode failed: {e:#}");
            toast::error(app, "Command failed", &msg);
            (Err(format!("command: {msg}")), llm::TokenUsage::default())
        }
    }
}

async fn run_dictation_mode<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    transcript: &str,
    session: u64,
) -> (Result<String, String>, llm::TokenUsage) {
    let anthropic_key = state.anthropic_key();
    match dictation::run(app, &anthropic_key, transcript, session).await {
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
            (Ok(outcome.inserted), outcome.usage)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg == crate::hotkeys::CANCELLED_MARKER {
                log::info!("dictation cancelled by user (Esc) or superseded");
                return (
                    Err(crate::hotkeys::CANCELLED_MARKER.to_string()),
                    llm::TokenUsage::default(),
                );
            }
            log::error!("dictation pipeline failed: {e:#}");
            toast::error(app, "Insertion failed", &msg);
            (Err(format!("dictation: {msg}")), llm::TokenUsage::default())
        }
    }
}

async fn run_action_mode<R: Runtime>(
    app: &AppHandle<R>,
    transcript: &str,
    session: u64,
) -> Result<(String, Option<String>), String> {
    match action_mode::run(app, transcript, session).await {
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
            let msg = format!("{e:#}");
            if msg == crate::hotkeys::CANCELLED_MARKER {
                log::info!("action cancelled by user (Esc) or superseded");
                return Err(crate::hotkeys::CANCELLED_MARKER.to_string());
            }
            log::error!("action mode failed: {e:#}");
            toast::error(app, "Action failed", &msg);
            Err(format!("action: {msg}"))
        }
    }
}

/// Segment `no_speech_prob` at/above which Whisper itself judges the audio to
/// contain no speech. 0.6 is the cutoff the Whisper decoder uses internally
/// to flag a segment as likely silence.
const NO_SPEECH_PROB_REJECT: f64 = 0.6;

/// Mean segment `avg_logprob` below which a decode is treated as very low
/// confidence. Real speech typically decodes well above -0.5; noise-induced
/// hallucinations sit at or below -1.0.
const AVG_LOGPROB_REJECT: f64 = -1.0;

/// The low-logprob rule only fires on short transcripts: the classic
/// noise-induced hallucination ("Thank you.", "Bye.") is 1–3 words, and a
/// genuine longer dictation must never be suppressed on logprob alone.
const LOW_CONFIDENCE_MAX_WORDS: usize = 6;

/// Pure gate deciding whether a transcript is a silence/noise hallucination
/// to discard, using only the STT provider's own confidence signals:
///
/// 1. empty/whitespace transcript → reject (long-standing behaviour, kept);
/// 2. no segments, or segments missing the relevant fields → ACCEPT — fail
///    open so a provider response-shape change can never start dropping real
///    dictation;
/// 3. every segment reports `no_speech_prob >= NO_SPEECH_PROB_REJECT` →
///    reject (the model is confident there was no speech);
/// 4. mean `avg_logprob` below `AVG_LOGPROB_REJECT` AND the transcript is
///    shorter than `LOW_CONFIDENCE_MAX_WORDS` → reject (the classic
///    noise-induced "Thank you." shape).
///
/// Anything else is kept, so dictating "thank you" as a Slack reply survives.
fn is_low_confidence(transcript: &str, segments: &[stt::TranscriptSegment]) -> bool {
    if transcript.trim().is_empty() {
        return true;
    }
    if segments.is_empty() {
        return false;
    }
    // A segment missing no_speech_prob cannot support a rejection — it counts
    // as evidence against, keeping the gate fail-open on partial data.
    let all_no_speech = segments
        .iter()
        .all(|s| s.no_speech_prob.is_some_and(|p| p >= NO_SPEECH_PROB_REJECT));
    if all_no_speech {
        return true;
    }
    let logprobs: Vec<f64> = segments.iter().filter_map(|s| s.avg_logprob).collect();
    if logprobs.is_empty() {
        return false;
    }
    let mean_logprob = logprobs.iter().sum::<f64>() / logprobs.len() as f64;
    let word_count = transcript.split_whitespace().count();
    mean_logprob < AVG_LOGPROB_REJECT && word_count < LOW_CONFIDENCE_MAX_WORDS
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
         hotkeys: dictation={} action={} prompt={} command={} cancel={}\n\
         \nNote: transcripts, LLM output and clipboard/selection values are\n\
         redacted in the logs unless verbose logging was enabled.\n",
        env!("CARGO_PKG_VERSION"),
        crate::logging::log_path().display(),
        settings.general.verbose_logging,
        settings.hotkeys.dictation,
        settings.hotkeys.action,
        settings.hotkeys.prompt,
        settings.hotkeys.command,
        settings.hotkeys.cancel,
    )
}

/// Parse a `spoken,replacement` CSV (header optional) and return a preview of
/// which terms would be added, which already exist, and which rows were skipped.
/// Pure: no disk I/O and no persistence — the frontend owns the in-memory vocab
/// and persists confirmed additions through the existing `save_settings` path.
#[tauri::command]
pub fn import_vocabulary_csv(
    csv_text: String,
    existing: Vec<settings_store::VocabEntry>,
) -> Result<crate::vocab_import::VocabImport, String> {
    Ok(crate::vocab_import::compute_vocab_import(&csv_text, &existing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(no_speech_prob: Option<f64>, avg_logprob: Option<f64>) -> stt::TranscriptSegment {
        stt::TranscriptSegment {
            no_speech_prob,
            avg_logprob,
        }
    }

    #[test]
    fn genuine_thank_you_with_decent_confidence_is_kept() {
        // The phrase denylist nuked this outright; the confidence gate keeps
        // it because the model reports real speech.
        let segments = [seg(Some(0.02), Some(-0.15))];
        assert!(!is_low_confidence("Thank you.", &segments));
    }

    #[test]
    fn all_segments_high_no_speech_prob_is_rejected() {
        let segments = [seg(Some(0.9), Some(-0.3)), seg(Some(0.85), Some(-0.4))];
        assert!(is_low_confidence("Thank you.", &segments));
    }

    #[test]
    fn missing_segments_are_accepted() {
        // A provider response without a segments array must never start
        // dropping real dictation.
        assert!(!is_low_confidence("Thank you.", &[]));
        assert!(!is_low_confidence("Bye", &[]));
    }

    #[test]
    fn empty_or_whitespace_transcript_is_rejected() {
        assert!(is_low_confidence("", &[]));
        assert!(is_low_confidence("  \n ", &[seg(Some(0.1), Some(-0.2))]));
    }

    #[test]
    fn very_low_logprob_short_transcript_is_rejected() {
        // The classic noise-induced "Thank you." hallucination shape:
        // no_speech_prob below the silence cutoff but a very poor decode.
        let segments = [seg(Some(0.4), Some(-1.4))];
        assert!(is_low_confidence("Thank you.", &segments));
    }

    #[test]
    fn very_low_logprob_long_transcript_is_kept() {
        // Logprob alone never suppresses a genuine longer dictation.
        let segments = [seg(Some(0.4), Some(-1.4))];
        assert!(!is_low_confidence(
            "please remind me to call the plumber tomorrow morning at nine",
            &segments
        ));
    }

    #[test]
    fn missing_confidence_fields_fail_open() {
        // One segment lacks no_speech_prob, so the all-no-speech rule cannot
        // fire; with no avg_logprob anywhere the logprob rule cannot either.
        let segments = [seg(Some(0.9), None), seg(None, None)];
        assert!(!is_low_confidence("Thank you.", &segments));
    }

    fn vocab(entries: &[&str]) -> Vec<settings_store::VocabEntry> {
        entries
            .iter()
            .map(|w| settings_store::VocabEntry {
                spoken: (*w).to_string(),
                replace_with: (*w).to_string(),
            })
            .collect()
    }

    #[test]
    fn empty_vocabulary_produces_no_hint() {
        assert_eq!(build_vocab_hint(&[]), None);
    }

    #[test]
    fn short_vocabulary_passes_through_unchanged() {
        let v = vocab(&["LeaseR", "TechGuider", "Wisspa"]);
        assert_eq!(
            build_vocab_hint(&v).as_deref(),
            Some("LeaseR, TechGuider, Wisspa")
        );
    }

    #[test]
    fn hint_exactly_at_cap_passes_through_unchanged() {
        // A single word of exactly VOCAB_HINT_MAX_CHARS chars is under the
        // cap and must survive untouched.
        let word = "a".repeat(VOCAB_HINT_MAX_CHARS);
        let v = vocab(&[&word]);
        assert_eq!(build_vocab_hint(&v).as_deref(), Some(word.as_str()));
    }

    #[test]
    fn oversized_vocabulary_truncates_at_word_boundary_under_cap() {
        // 100 words of 10 chars joined with ", " ≈ 1198 chars — over the cap.
        let words: Vec<String> = (0..100).map(|i| format!("word{i:06}")).collect();
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let v = vocab(&refs);
        let hint = build_vocab_hint(&v).expect("hint");
        assert!(hint.len() <= VOCAB_HINT_MAX_CHARS);
        // Every kept entry is a complete word from the list (no partial word,
        // no dangling separator)...
        for w in hint.split(", ") {
            assert!(words.iter().any(|x| x == w), "partial word {w:?} in hint");
        }
        // ...and truncation kept a strict prefix: the FIRST entries only.
        let kept = hint.split(", ").count();
        assert!(kept < words.len());
        assert_eq!(hint, words[..kept].join(", "));
        // The next word would not have fit under the cap.
        let with_next = format!("{hint}, {}", words[kept]);
        assert!(with_next.len() > VOCAB_HINT_MAX_CHARS);
    }

    #[test]
    fn truncation_of_single_multibyte_word_respects_char_boundaries() {
        // '€' is 3 bytes; the 800-char cap lands mid-character, exercising
        // the char-boundary backoff on the hard-cut path.
        let word = "€".repeat(400);
        let v = vocab(&[word.as_str()]);
        let hint = build_vocab_hint(&v).expect("hint");
        assert!(hint.len() <= VOCAB_HINT_MAX_CHARS);
        assert!(hint.chars().all(|c| c == '€'));
    }

    #[test]
    fn merged_profile_words_survive_hint_truncation() {
        // A matched profile's words are merged ahead of the global list, so
        // when the combined hint exceeds the cap only global entries are cut.
        let globals: Vec<String> = (0..100).map(|i| format!("global{i:06}")).collect();
        let global_entries: Vec<settings_store::VocabEntry> = globals
            .iter()
            .map(|w| settings_store::VocabEntry {
                spoken: w.clone(),
                replace_with: w.clone(),
            })
            .collect();
        let profile_vocab = vec!["standup".to_string(), "retro".to_string()];
        let merged =
            settings_store::merge_profile_vocabulary(&global_entries, &profile_vocab);
        let hint = build_vocab_hint(&merged).expect("hint");
        assert!(hint.len() <= VOCAB_HINT_MAX_CHARS);
        assert!(
            hint.starts_with("standup, retro"),
            "profile words must lead the hint: {}",
            &hint[..hint.len().min(80)]
        );
    }
}
