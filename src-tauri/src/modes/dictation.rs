use crate::{app_detector, ax_snapshot, injector, learning, llm, prompt_review, settings_store, toast};
use anyhow::Result;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

const EDIT_SNAPSHOT_DELAY_SECS: u64 = 8;

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
    /// Token accounting for the Haiku cleanup call (history metering).
    /// Default (no usage) when the cleanup call itself failed — even when its
    /// output was discarded as diverged, the call still consumed tokens.
    pub usage: llm::TokenUsage,
}

/// Phase 2 dictation pipeline:
///   raw transcript → detect active app → Haiku cleanup → inject + clipboard restore.
/// Edge cases per PRD §5.1:
///   - Empty transcript → caller short-circuits before this is called.
///   - Long transcript (>2000) → process anyway, surface the warning to the caller.
///   - LLM error → fall back to the corrected transcript with `cleaned = false`.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    anthropic_api_key: &str,
    raw_transcript: &str,
    session: u64,
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

    // Load settings once and reuse — the correction-application step, the
    // per-app profile match, and the post-paste opt-in check below all read
    // it, so a single load avoids extra disk I/O and a TOCTOU window between
    // them.
    let settings = settings_store::load(app).ok();
    let word_corrections = settings.as_ref().map(|s| s.word_corrections.clone());
    // Per-app profile: matched against the resolved target app. The profile's
    // tone note is handed to the Haiku cleanup below; its vocabulary was
    // already merged into the substitution + STT hint by `process_audio`
    // (which matched against the press-time snapshot before dispatch).
    let profile = settings
        .as_ref()
        .and_then(|s| settings_store::match_profile(&s.profiles, &active_app));
    if let Some(p) = profile {
        log::info!("app profile matched: {}", p.app);
    }

    // Apply user-trained word corrections before LLM cleanup so Haiku sees
    // already-corrected text and produces better results.
    let corrected_transcript = match &word_corrections {
        Some(wc) if wc.enabled => apply_corrections(raw_transcript, &wc.entries),
        _ => raw_transcript.to_string(),
    };

    // Race the cleanup call against cancellation: an Esc (or a newer
    // recording) drops the request future, cancelling the in-flight HTTP call
    // rather than waiting for it to finish (issue #31).
    let cleanup = tokio::select! {
        biased;
        _ = crate::session::aborted(session) => {
            return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
        }
        r = llm::haiku_cleanup_dictation(
            anthropic_api_key,
            &corrected_transcript,
            &active_app,
            profile.map(|p| p.tone.as_str()),
        ) => r,
    };
    let (final_text, cleaned, haiku_diverged, usage) =
        match cleanup {
            Ok((haiku_out, usage)) => {
                // Compare against the text Haiku actually saw (the corrected
                // transcript). Using the raw transcript here would flag the
                // user's own corrections as "divergence" and discard them.
                if diverges_from_raw(&corrected_transcript, &haiku_out) {
                    log::warn!(
                        "Haiku output diverged from transcript — falling back to corrected transcript. corrected={} haiku={}",
                        crate::redact::redact(&corrected_transcript),
                        crate::redact::redact(&haiku_out)
                    );
                    (corrected_transcript.clone(), false, true, usage)
                } else {
                    (haiku_out, true, false, usage)
                }
            }
            Err(e) => {
                log::error!("Haiku cleanup failed, falling back to corrected transcript: {e:#}");
                (
                    corrected_transcript.clone(),
                    false,
                    false,
                    llm::TokenUsage::default(),
                )
            }
        };

    // Edit-before-insert review (opt-in via `dictation.review_before_insert`,
    // default off): route the cleaned text through the same session-keyed
    // review window prompt mode uses — nothing is pasted until the user
    // approves, and Cancel/Esc aborts like any other cancellation. With the
    // gate off the pipeline pastes immediately, byte-for-byte unchanged.
    let review_before_insert = settings
        .as_ref()
        .map(|s| s.dictation.review_before_insert)
        .unwrap_or(false);
    let (final_text, inject_to): (String, Option<String>) = if review_before_insert {
        let focus_target = dictation_review_focus_target(app_detected, &active_app);
        let reviewed = prompt_review::gate(
            app,
            session,
            &final_text,
            &active_app,
            prompt_review::ReviewMode::Dictation,
            None,
        )
        .await?;
        (reviewed, focus_target)
    } else {
        // When detection failed, active_app is the "a macOS app" placeholder —
        // activating it errors and fails the whole paste (the placeholder is
        // not a real process). Inject with no target instead: the text lands
        // wherever macOS has focus, which is right in practice — the user
        // just dictated into it. Same policy as the review path above.
        let inject_to = dictation_inject_target(app_detected, &active_app);
        (final_text, inject_to)
    };

    // Internal-insert channel: when a Wisspa window's editable field holds
    // focus (e.g. the onboarding test box), deliver the text straight to that
    // webview. The system path can't handle this case — as an Accessory app,
    // clicking our window doesn't reliably make Wisspa frontmost, so the
    // press-time snapshot points at whatever app was previously active and
    // the injector would paste there instead. See internal_insert.rs.
    if let Some(window) = crate::internal_insert::current() {
        // Consume the press-time snapshot so it can't leak into the next
        // dictation and hijack its target.
        let _ = app_detector::take_target_app();
        app.emit_to(&window, crate::internal_insert::EVENT_INTERNAL_INSERT, &final_text)
            .map_err(|e| anyhow::anyhow!("internal insert emit to '{window}': {e}"))?;
        crate::sounds::play(app, crate::sounds::Cue::Complete);
        return Ok(DictationOutcome {
            inserted: final_text,
            app_detected: false,
            cleaned,
            long_transcript,
            haiku_diverged,
            usage,
        });
    }

    injector::inject_text(app, &final_text, inject_to.as_deref(), session).await?;

    crate::sounds::play(app, crate::sounds::Cue::Complete);

    // Spawn auto-learn snapshot task if the user has opted in.
    if let Some(wc) = &word_corrections {
        if wc.enabled && wc.learn_from_edits {
            let app_clone = app.clone();
            let inserted = final_text.clone();
            let raw = raw_transcript.to_string();
            // Pass None when app detection failed so the focus-moved check
            // is skipped — otherwise the placeholder "a macOS app" would
            // never match the live frontmost app and auto-learn would
            // silently no-op on the error path.
            let target_app = if app_detected { Some(active_app.clone()) } else { None };
            tauri::async_runtime::spawn(async move {
                snapshot_and_learn(app_clone, inserted, raw, target_app).await;
            });
        }
    }

    Ok(DictationOutcome {
        inserted: final_text,
        app_detected,
        cleaned,
        long_transcript,
        haiku_diverged,
        usage,
    })
}

/// The app to re-activate before pasting a reviewed dictation. The review
/// window steals focus, so a real detection must be brought back forward —
/// but when detection failed there is no target to activate (the "a macOS app"
/// placeholder would hard-fail the activation and discard the user's edited
/// text), matching prompt mode's manual-override stance.
fn dictation_review_focus_target(app_detected: bool, active_app: &str) -> Option<String> {
    prompt_review::review_focus_target(
        if app_detected { Some(active_app) } else { None },
        None,
    )
}

/// The app to re-activate before pasting a non-reviewed dictation. Same rule
/// as the review path: a failed detection yields the "a macOS app" placeholder,
/// and activating that placeholder hard-fails the paste for no benefit — so
/// no target, and the text lands wherever macOS has focus (which is where the
/// user just dictated).
fn dictation_inject_target(app_detected: bool, active_app: &str) -> Option<String> {
    if app_detected {
        Some(active_app.to_string())
    } else {
        None
    }
}

/// Replace auto-apply corrections in `text`. Matches are case-insensitive and
/// word-boundary aware: a word is a run of alphanumeric characters (plus
/// apostrophes), so corrections like `gpt4` match but space-separated phrases
/// are not supported.
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
        if c.is_alphanumeric() {
            let mut end = start + c.len_utf8();
            while let Some(&(_, nc)) = chars.peek() {
                if nc.is_alphanumeric() || nc == '\'' {
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

    // Assistant-acknowledgement check. When the cleanup model answers a
    // question instead of transcribing it, word overlap can't tell — the
    // acknowledgement parrots the topic vocabulary ("I'm ready to help you
    // work through your morning brief items" shares most content words with
    // the question that produced it). The shape gives it away: a question
    // went in, a non-question came out, and it talks like an assistant.
    if raw_trim.ends_with('?')
        && !cleaned_trim.ends_with('?')
        && contains_assistant_phrasing(cleaned_trim)
    {
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

/// Lowercased, word-boundary-padded check against phrases a dictation editor
/// must never produce: assistant offers, acknowledgements, and
/// throat-clearing. Punctuation is normalised to spaces and the string is
/// padded, so "where is" can never match "here is". Only consulted when a
/// question came in and a non-question went out, so real dictation
/// containing these words mid-sentence is unaffected.
fn contains_assistant_phrasing(cleaned: &str) -> bool {
    const PHRASES: &[&str] = &[
        " i'm ", " i'll ", " i'd ", " let me know ", " please share ",
        " please provide ", " feel free ", " happy to ", " glad to ", " here's ",
        " here is ", " sure thing ", " of course ", " certainly ", " absolutely ",
        " assist you ", " help you ",
    ];
    let normalised: String = cleaned
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect();
    let padded = format!(" {} ", normalised);
    PHRASES.iter().any(|p| padded.contains(p))
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

/// Background task: wait, snapshot the focused field, diff against what we
/// pasted, and feed any valid single-word corrections into the learning system.
/// Every error path is silent — never crashes, never shows UI noise.
///
/// `active_app` is `None` when initial detection failed; in that case we
/// can't tell whether focus moved, so we skip the focus-changed gate and
/// take the snapshot anyway rather than dropping the learning opportunity.
async fn snapshot_and_learn<R: Runtime>(
    app: AppHandle<R>,
    inserted: String,
    raw_transcript: String,
    active_app: Option<String>,
) {
    tokio::time::sleep(Duration::from_secs(EDIT_SNAPSHOT_DELAY_SECS)).await;

    // Abort if focus moved to a different app — but only when we have a
    // known starting app to compare against.
    if let Some(expected) = active_app.as_deref() {
        match app_detector::frontmost_app_name().await {
            Ok(current) if current != expected => {
                log::debug!("auto-learn: focus moved to '{current}', skipping snapshot");
                return;
            }
            Err(e) => {
                log::debug!("auto-learn: frontmost app check failed: {e:#}");
                return;
            }
            _ => {}
        }
    }

    let field = match ax_snapshot::focused_field_value() {
        Some(f) => f,
        None => return,
    };

    let candidates = learning::diff_corrections(&inserted, &field.value, &raw_transcript);

    for candidate in candidates {
        match settings_store::record_correction(&app, &candidate.heard, &candidate.corrected) {
            Ok((_, true)) => {
                toast::info(
                    &app,
                    "Wisspa learned a correction",
                    &format!(
                        "Will now auto-correct \"{}\" → \"{}\"",
                        candidate.heard, candidate.corrected
                    ),
                );
            }
            Ok(_) => {}
            Err(e) => log::warn!("auto-learn: record_correction failed: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{contains_assistant_phrasing, dictation_inject_target, dictation_review_focus_target, diverges_from_raw};

    #[test]
    fn review_focus_target_uses_the_detected_app() {
        // The review window steals focus, so the detected target must be
        // re-activated before the paste.
        assert_eq!(
            dictation_review_focus_target(true, "Slack"),
            Some("Slack".to_string())
        );
    }

    #[test]
    fn review_focus_target_is_none_when_detection_failed() {
        // The "a macOS app" fallback placeholder must never be activated: the
        // hard activation error would discard the user's edited text.
        assert_eq!(dictation_review_focus_target(false, "a macOS app"), None);
    }

    #[test]
    fn inject_target_is_none_when_detection_failed() {
        // Non-review path, same rule: activating the placeholder fails the
        // whole paste; pasting into the focused app is the right fallback.
        assert_eq!(dictation_inject_target(false, "a macOS app"), None);
        assert_eq!(
            dictation_inject_target(true, "Notes"),
            Some("Notes".to_string())
        );
    }

    #[test]
    fn divergence_flags_assistant_acknowledgement_of_a_question() {
        // The production failure: user dictated a question, Haiku answered
        // it with an assistant offer that parrots the topic words. Length
        // ratio and word coverage both pass — the shape check must catch it.
        let raw = "All the below items are coming up in my morning brief. Can you help me systematically go through them and fix them?";
        let answered = "I'm ready to help you work through your morning brief items. Please share the list of items you'd like to go through, and I'll provide clear steps for each one.";
        assert!(diverges_from_raw(raw, answered));
    }

    #[test]
    fn divergence_allows_cleaned_questions_and_assistant_sounding_dictation() {
        // A question that stays a question is never flagged.
        assert!(!diverges_from_raw(
            "What's the desktop application going to look like?",
            "What's the desktop application going to look like?"
        ));
        // Real dictation starting with "I'm" — only an issue when a question
        // becomes an answer-shaped non-question.
        assert!(!diverges_from_raw(
            "I'm going to close out this session. Is there anything else that needs to be remembered?",
            "I'm going to close out this session. Is there anything else that needs to be remembered?"
        ));
        // Legitimate dictation containing assistant-ish words, no question.
        assert!(!diverges_from_raw(
            "Yes, proceed. You can pause a couple and see if it works.",
            "Yes, proceed. You can pause a couple and see if it works."
        ));
        // Question mark lost in cleanup but no assistant phrasing — allowed
        // (punctuation-only change, not an answer).
        assert!(!diverges_from_raw(
            "Where is the settings file?",
            "Where is the settings file."
        ));
    }

    #[test]
    fn assistant_phrasing_matches_assistant_tells_only() {
        assert!(contains_assistant_phrasing("I'm ready to help you."));
        assert!(contains_assistant_phrasing("Sure thing, here is the list."));
        assert!(contains_assistant_phrasing("Please share the file when you can."));
        assert!(!contains_assistant_phrasing("Can you help me fix these items?"));
        assert!(!contains_assistant_phrasing("Tell them I am on my way home now"));
    }
}
