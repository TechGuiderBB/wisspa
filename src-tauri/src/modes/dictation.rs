use crate::{app_detector, injector, llm, settings_store};
use anyhow::Result;
use std::collections::HashMap;
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
    /// True when Haiku's output diverged enough from the raw transcript
    /// that we discarded it and used the raw text instead (guardrail
    /// against Haiku answering questions / rewriting prompts).
    pub haiku_diverged: bool,
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

    // Apply user-trained word corrections before LLM cleanup so Haiku sees
    // already-corrected text and produces better results.
    let corrected_transcript = match settings_store::load(app) {
        Ok(s) if s.word_corrections.enabled => {
            apply_corrections(raw_transcript, &s.word_corrections.entries)
        }
        _ => raw_transcript.to_string(),
    };

    let (final_text, cleaned, haiku_diverged) =
        match llm::haiku_cleanup_dictation(anthropic_api_key, &corrected_transcript, &active_app).await {
            Ok(haiku_out) => {
                if diverges_from_raw(raw_transcript, &haiku_out) {
                    log::warn!(
                        "Haiku output diverged from raw transcript — falling back to raw. raw={raw_transcript:?} haiku={haiku_out:?}"
                    );
                    (raw_transcript.to_string(), false, true)
                } else {
                    (haiku_out, true, false)
                }
            }
            Err(e) => {
                log::error!("Haiku cleanup failed, falling back to raw: {e:#}");
                (raw_transcript.to_string(), false, false)
            }
        };

    injector::inject_text(app, &final_text, Some(&active_app)).await?;

    Ok(DictationOutcome {
        inserted: final_text,
        app_detected,
        cleaned,
        long_transcript,
        haiku_diverged,
    })
}

/// Replace auto-apply corrections in `text`. Matches are case-insensitive,
/// word-boundary aware (splits on non-alphabetic characters).
fn apply_corrections(
    text: &str,
    entries: &HashMap<String, settings_store::WordCorrectionEntry>,
) -> String {
    let lookup: HashMap<String, &str> = entries
        .iter()
        .filter(|(_, e)| e.auto_apply)
        .map(|(k, v)| (k.clone(), v.replacement.as_str()))
        .collect();
    if lookup.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_alphabetic() {
            let mut end = start + c.len_utf8();
            while let Some(&(_, nc)) = chars.peek() {
                if nc.is_alphabetic() || nc == '\'' {
                    chars.next();
                    end += nc.len_utf8();
                } else {
                    break;
                }
            }
            let word = &text[start..end];
            let lower = word.to_lowercase();
            if let Some(&replacement) = lookup.get(&lower) {
                result.push_str(replacement);
            } else {
                result.push_str(word);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Decide whether Haiku's output is faithful to the raw Whisper transcript.
/// True = Haiku went off-script (answered a question, rewrote into a
/// template, added content) and we should fall back to the raw text.
///
/// Heuristics:
///   1. Length blow-up: cleaned > 1.5 × raw and absolute delta ≥ 30 chars.
///   2. Word-overlap collapse: <50% of raw's content words appear in cleaned.
fn diverges_from_raw(raw: &str, cleaned: &str) -> bool {
    let raw_trim = raw.trim();
    let cleaned_trim = cleaned.trim();
    if cleaned_trim.is_empty() || raw_trim.is_empty() {
        return false;
    }

    let raw_len = raw_trim.chars().count();
    let cleaned_len = cleaned_trim.chars().count();
    let len_ratio = cleaned_len as f32 / raw_len as f32;
    let absolute_delta = (cleaned_len as i32 - raw_len as i32).abs();
    if len_ratio > 1.5 && absolute_delta >= 30 {
        return true;
    }

    let raw_words = content_words(raw_trim);
    if raw_words.is_empty() {
        return false;
    }
    let cleaned_lower = cleaned_trim.to_lowercase();
    let kept: usize = raw_words
        .iter()
        .filter(|w| cleaned_lower.contains(w.as_str()))
        .count();
    let coverage = kept as f32 / raw_words.len() as f32;
    coverage < 0.5
}

/// Tokenize into lowercased "content words" — alphanumeric, length ≥ 3,
/// skipping common stop-words that are too easy to overlap incidentally.
fn content_words(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "you", "your", "are", "was", "but", "this",
        "that", "have", "has", "had", "not", "what", "when", "where", "why", "how",
        "from", "into", "out", "about", "can", "could", "would", "should",
    ];
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| !STOP.contains(&w.as_str()))
        .collect()
}
