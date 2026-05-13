use crate::{app_detector, injector, llm};
use anyhow::Result;
use tauri::{AppHandle, Runtime};

pub struct DictationOutcome {
    /// Final text inserted into the focused field.
    pub inserted: String,
    /// True when the active-app context was successfully detected.
    #[allow(dead_code)] // surfaced in Phase 3 history UI
    pub app_detected: bool,
    /// True when the LLM cleanup ran successfully. False = fallback to raw transcript.
    pub cleaned: bool,
    /// True when the raw transcript exceeded 2000 chars per PRD §5.1.
    pub long_transcript: bool,
}

/// Phase 2 dictation pipeline:
///   raw transcript → detect active app → Haiku cleanup → inject + clipboard restore.
/// Edge cases per PRD §5.1:
///   - Empty transcript → caller short-circuits before this is called.
///   - Long transcript (>2000) → process anyway, surface the warning to the caller.
///   - LLM error → fall back to raw transcript with `cleaned = false`.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    anthropic_api_key: &str,
    raw_transcript: &str,
) -> Result<DictationOutcome> {
    let long_transcript = raw_transcript.chars().count() > 2000;

    // Prefer the press-time snapshot (taken when the user's intended app
    // was still focused); fall back to a live detection if the snapshot
    // didn't complete in time.
    let (active_app, app_detected) = match app_detector::take_target_app() {
        Some(name) => (name, true),
        None => match app_detector::frontmost_app_name().await {
            Ok(name) => (name, true),
            Err(e) => {
                log::warn!("frontmost app detect failed, using fallback context: {e:#}");
                ("a macOS app".to_string(), false)
            }
        },
    };
    log::info!("target app: {active_app}");

    let (final_text, cleaned) =
        match llm::haiku_cleanup_dictation(anthropic_api_key, raw_transcript, &active_app).await {
            Ok(cleaned) => (cleaned, true),
            Err(e) => {
                log::error!("Haiku cleanup failed, falling back to raw: {e:#}");
                (raw_transcript.to_string(), false)
            }
        };

    injector::inject_text(app, &final_text, Some(&active_app)).await?;

    Ok(DictationOutcome {
        inserted: final_text,
        app_detected,
        cleaned,
        long_transcript,
    })
}
