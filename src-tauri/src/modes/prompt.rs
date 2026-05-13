use crate::{app_detector, injector, llm, selection, settings_store, toast};
use anyhow::Result;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

pub struct PromptOutcome {
    pub inserted: String,
    pub selection_captured: bool,
    pub manual_app_override_used: bool,
}

/// Phase 5 prompt-mode pipeline:
///   transcript → resolve active app (manual override or AppleScript)
///              → optionally capture selected text
///              → Sonnet rewrite
///              → optional preview toast
///              → inject
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    anthropic_api_key: &str,
    raw_transcript: &str,
) -> Result<PromptOutcome> {
    let settings = settings_store::load(app).unwrap_or_default();
    let pm = &settings.prompt_mode;

    // Press-time snapshot wins: this is what the user was focused on when
    // they pressed the hotkey, even if another app (Perplexity) stole focus
    // by sharing the same global shortcut.
    let snapshot = app_detector::take_target_app();
    let (active_app, manual_override_used) = match pm.manual_app_override.as_deref() {
        Some(name) if !name.is_empty() => (name.to_string(), true),
        _ => match snapshot {
            Some(name) => (name, false),
            None => match app_detector::frontmost_app_name().await {
                Ok(name) => (name, false),
                Err(e) => {
                    log::warn!("frontmost app detect failed: {e:#}");
                    ("Generic".to_string(), false)
                }
            },
        },
    };
    let inject_target = if manual_override_used {
        // Manual override is used as a Sonnet format hint, not necessarily an
        // app to activate. Don't bring it forward.
        None
    } else {
        Some(active_app.clone())
    };
    log::info!(
        "prompt mode → app={active_app} (override={manual_override_used})"
    );

    let selected_text = if pm.include_selected_text {
        match selection::read_selected_text(app).await {
            Ok(Some(text)) => {
                log::info!("captured selection ({} chars)", text.len());
                text
            }
            Ok(None) => String::new(),
            Err(e) => {
                log::warn!("selection capture failed: {e:#}");
                String::new()
            }
        }
    } else {
        String::new()
    };
    let selection_captured = !selected_text.is_empty();

    let rewritten = llm::sonnet_prompt_rewrite(
        anthropic_api_key,
        raw_transcript,
        &active_app,
        &selected_text,
    )
    .await?;

    if rewritten.trim().is_empty() {
        return Err(anyhow::anyhow!("Sonnet returned empty rewrite"));
    }

    // Preview-before-insert: PRD §5.3 step 6. If enabled, show a toast with
    // the first 50 chars and wait the configured timeout before pasting.
    // (Edit-before-insert UI is Phase 7 polish.)
    if pm.show_preview {
        let preview = first_chars(&rewritten, 50);
        toast::info(
            app,
            "Prompt generated",
            &format!("Inserting in {}s — {preview}", pm.preview_timeout_seconds),
        );
        tokio::time::sleep(Duration::from_secs(pm.preview_timeout_seconds as u64)).await;
    }

    injector::inject_text(app, &rewritten, inject_target.as_deref()).await?;

    Ok(PromptOutcome {
        inserted: rewritten,
        selection_captured,
        manual_app_override_used: manual_override_used,
    })
}

fn first_chars(s: &str, n: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= n {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(n).collect();
        format!("{cut}…")
    }
}
