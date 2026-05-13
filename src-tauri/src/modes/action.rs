use crate::actions::{executor, matcher};
use anyhow::Result;
use tauri::{AppHandle, Runtime};

pub struct ActionOutcome {
    pub message: String,
    pub success: bool,
    pub matched_action_id: Option<String>,
    pub suggestions: Vec<String>,
}

/// Phase 4 pipeline:
///   transcript → match against registry → execute matched action.
///   No match → return Top-N suggestions for the toast.
pub async fn run<R: Runtime>(app: &AppHandle<R>, transcript: &str) -> Result<ActionOutcome> {
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
