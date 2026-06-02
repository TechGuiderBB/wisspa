use crate::actions::{executor, matcher};
use anyhow::Result;
use tauri::{AppHandle, Runtime};

pub struct ActionOutcome {
    pub message: String,
    pub success: bool,
    pub matched_action_id: Option<String>,
    pub suggestions: Vec<String>,
}

/// Actions whose payload is meaningless without a `{query}` body. Used to
/// short-circuit before executing an empty command (e.g. "take a note"
/// said alone would append an empty bullet).
fn needs_query(action_id: &str) -> bool {
    matches!(
        action_id,
        "new_note"
            | "open_app"
            | "search_google"
            | "search_github"
            | "search_youtube"
    )
}

/// Phase 4 pipeline:
///   transcript → match against registry → execute matched action.
///   No match → return Top-N suggestions for the toast.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    transcript: &str,
    session: u64,
) -> Result<ActionOutcome> {
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return Ok(ActionOutcome {
            message: "No speech detected.".to_string(),
            success: false,
            matched_action_id: None,
            suggestions: Vec::new(),
        });
    }

    match matcher::find_match(trimmed) {
        Some(m) => {
            // Guard: if the trigger matched but no content followed (e.g.
            // user said "take a note" with nothing after), refuse to run
            // rather than write an empty bullet.
            if m.query.trim().is_empty() && needs_query(&m.action.id) {
                return Ok(ActionOutcome {
                    message: format!(
                        "\"{}\" needs something to follow the trigger — try \"new note buy bread\".",
                        m.action.name
                    ),
                    success: false,
                    matched_action_id: Some(m.action.id.clone()),
                    suggestions: Vec::new(),
                });
            }
            // Don't run the action (or stage a destructive-action confirmation)
            // if the user cancelled or started a newer recording (issue #31).
            if crate::session::is_aborted(session) {
                return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
            }
            let exec = executor::execute(app, &m.action, &m.query).await?;
            Ok(ActionOutcome {
                message: exec.message,
                success: exec.success,
                matched_action_id: Some(m.action.id.clone()),
                suggestions: Vec::new(),
            })
        }
        None => {
            let top = matcher::suggest_top(trimmed, 2);
            let suggestions: Vec<String> =
                top.iter().map(|s| format!("\"{}\"", s.trigger)).collect();
            let suggestion_str = if suggestions.is_empty() {
                "no close matches".to_string()
            } else {
                format!("Did you mean: {}?", suggestions.join(" or "))
            };
            Ok(ActionOutcome {
                message: format!("No action matched. {suggestion_str}"),
                success: false,
                matched_action_id: None,
                suggestions,
            })
        }
    }
}
