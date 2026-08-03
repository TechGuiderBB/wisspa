use crate::{app_detector, injector, llm, selection, toast};
use anyhow::Result;
use tauri::{AppHandle, Runtime};

/// Marker surfaced through `Err` when Command Mode was triggered with no text
/// selected. Recognised in `process_audio` (history status `cancelled` with a
/// "(no text selected)" note, and no error surfaced to the frontend) — the
/// warn toast has already fired here in the mode and the LLM was never called.
pub const NO_SELECTION_MARKER: &str = "__no_selection__";

/// User-facing toast body for the no-selection early exit. Actionable: names
/// the missing precondition, not a failure.
pub const NO_SELECTION_TOAST: &str = "No text selected — select text first";

pub struct CommandOutcome {
    /// Final text pasted over the user's selection.
    pub inserted: String,
}

/// Command Mode pipeline:
///   transcript (the spoken instruction) → resolve press-time app
///   → capture the current selection (lossless Cmd+C, clipboard restored)
///   → Haiku applies the instruction to the selected text
///   → inject the transformed text over the still-active selection.
///
/// No review gate and no preview toast for v1: the paste lands directly and
/// the user can Cmd+Z in their app. The escape hatch for a bad transform.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    anthropic_api_key: &str,
    raw_transcript: &str,
    session: u64,
) -> Result<CommandOutcome> {
    // Press-time snapshot wins: this is what the user was focused on when they
    // pressed the hotkey, even if focus moved during the recording. Same
    // target resolution as dictation.
    let active_app = match app_detector::take_target_app() {
        Some(name) => name,
        None => match app_detector::frontmost_app_name().await {
            Ok(name) => name,
            Err(e) => {
                log::warn!("frontmost app detect failed: {e:#}");
                "Generic".to_string()
            }
        },
    };
    log::info!("command mode → app={active_app}");

    // Cheap abort checkpoint before touching the user's clipboard: a cancelled
    // session must not issue the synthetic Cmd+C below.
    if crate::session::is_aborted(session) {
        return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
    }

    // Command Mode is meaningless without a selection. No selection (or a
    // failed capture) is a graceful no-op: warn toast (suppressed by quiet
    // notifications), cancelled history row via the marker, and crucially no
    // LLM call. The transcript is the instruction, not content — there is
    // nothing sensible to paste without text to transform.
    let selected_text = match selection::read_selected_text(app).await {
        Ok(Some(text)) if !text.trim().is_empty() => text,
        Ok(_) => {
            log::info!("command mode: no text selected");
            toast::warn(app, "Command mode", NO_SELECTION_TOAST);
            return Err(anyhow::anyhow!(NO_SELECTION_MARKER));
        }
        Err(e) => {
            log::warn!("command mode: selection capture failed: {e:#}");
            toast::warn(app, "Command mode", NO_SELECTION_TOAST);
            return Err(anyhow::anyhow!(NO_SELECTION_MARKER));
        }
    };
    log::info!("command mode: captured selection ({} chars)", selected_text.len());

    // Race the transform against cancellation: an Esc (or a newer recording)
    // drops the request future, cancelling the in-flight HTTP call (issue #31).
    let transform = tokio::select! {
        biased;
        _ = crate::session::aborted(session) => {
            return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
        }
        r = llm::command_transform(anthropic_api_key, raw_transcript, &selected_text) => r,
    };
    let final_text = transform_or_error(transform)?;

    // Paste over the still-active selection in the press-time app. The
    // selection capture (Cmd+C + clipboard restore) leaves the selection
    // intact, so the injector's Cmd+V replaces exactly the captured text.
    injector::inject_text(app, &final_text, Some(&active_app), session).await?;

    Ok(CommandOutcome { inserted: final_text })
}

/// Fold the LLM result into the text to paste. A successful, non-empty
/// transform wins outright. Empty output and request failures are both errors:
/// pasting empty text would DELETE the user's selection, and silently pasting
/// the original back would pretend the command ran — an honest failure beats
/// both (nothing is pasted, so there is nothing to undo).
fn transform_or_error(transform: Result<String>) -> Result<String> {
    match transform {
        Ok(text) if !text.trim().is_empty() => Ok(text),
        Ok(_) => {
            log::warn!("command transform returned empty — not pasting");
            Err(anyhow::anyhow!("command transform returned empty"))
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::{transform_or_error, NO_SELECTION_MARKER, NO_SELECTION_TOAST};

    #[test]
    fn successful_transform_is_used_verbatim() {
        let out = transform_or_error(Ok("Bonjour le monde.".to_string())).expect("transform");
        assert_eq!(out, "Bonjour le monde.");
    }

    #[test]
    fn empty_or_whitespace_transform_is_an_error_never_a_paste() {
        // Pasting empty text would delete the user's selection — the worst
        // possible outcome. Empty output must surface as a failure instead.
        for empty in ["", "   \n\t  "] {
            let err = transform_or_error(Ok(empty.to_string()));
            assert!(err.is_err(), "empty transform {empty:?} must not paste");
        }
    }

    #[test]
    fn failed_transform_propagates_the_error() {
        let err = transform_or_error(Err(anyhow::anyhow!("Anthropic HTTP 500")));
        assert!(err.is_err());
        assert!(format!("{:#}", err.unwrap_err()).contains("HTTP 500"));
    }

    #[test]
    fn no_selection_marker_is_distinct_from_user_cancel() {
        // process_audio maps the two markers to different history shapes;
        // they must never collapse into one another.
        assert_ne!(NO_SELECTION_MARKER, crate::hotkeys::CANCELLED_MARKER);
    }

    #[test]
    fn no_selection_toast_is_actionable() {
        // Names the precondition so the user can self-serve on the next press.
        assert!(NO_SELECTION_TOAST.contains("No text selected"));
        assert!(NO_SELECTION_TOAST.contains("select text first"));
    }
}
