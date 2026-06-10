use crate::{app_detector, injector, llm, prompt_review, selection, settings_store, toast};
use anyhow::Result;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Routing-transparency event consumed by the recording overlay + main window.
/// Matches the frontend `PROMPT_ROUTE_EVENT` in `src/lib/promptRoute.ts`.
pub const EVENT_PROMPT_ROUTE: &str = "wisspa://prompt-route";

/// Edit-before-insert event consumed by the `review` window. Matches the
/// frontend listener in `src/components/PromptReview.tsx`.
pub const EVENT_PROMPT_REVIEW_OPEN: &str = "wisspa://prompt-review-open";

/// Payload for `EVENT_PROMPT_REVIEW_OPEN`. Field names mirror the frontend
/// `PromptReviewOpen` type exactly (serde keeps these lowercase names).
#[derive(serde::Serialize, Clone)]
struct PromptReviewPayload {
    /// Recording session id; the window keys its state by this so a superseded
    /// recording can't paste through a stale review (issue #31).
    session: u64,
    /// Sonnet's generated prompt, shown in the editable textarea.
    text: String,
    /// Resolved destination app, shown in the window header.
    app: String,
    /// "prompt" or "content" — same Branch A/B split as the routing chip.
    branch: &'static str,
}

/// Show + focus the review window so the user can edit the generated prompt.
/// A missing window is logged, not fatal: the pipeline still awaits the decision
/// and the user can abort with the global Esc cancel hotkey.
fn show_review_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("review") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    } else {
        log::warn!("review window missing; awaiting decision (cancel via Esc)");
    }
}

/// Hide the review window once the user has acted (or the session aborted).
fn hide_review_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("review") {
        let _ = w.hide();
    }
}

/// Payload for `EVENT_PROMPT_ROUTE`. Field names mirror the frontend `PromptRoute`
/// type exactly (serde keeps these lowercase names, which the TS union expects).
#[derive(serde::Serialize, Clone)]
struct PromptRoutePayload {
    /// Recording session id; the frontend's stale-event guard drops any payload
    /// whose session != the current recording (0 is legacy passthrough).
    session: u64,
    /// Resolved destination app (System Events process name, or manual override).
    app: String,
    /// Browser host when the destination is a known browser tab, else null.
    host: Option<String>,
    /// "prompt" (AI-tool destination, Sonnet Branch A) or "content" (non-AI
    /// destination, Branch B). The destination is always resolved before this
    /// fires, so we never emit "unknown".
    branch: &'static str,
    /// "manual" when a manual app override is set, else "auto".
    source: &'static str,
}

/// Extract a clean host from a browser tab URL: strip scheme, userinfo, port,
/// path/query/fragment and a leading `www.`. Returns "" when there's nothing
/// host-like, so callers can treat empty as "no host".
fn host_from_url(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Classify the destination the same way Sonnet's Branch A/B split does: an AI
/// chat tool (by host or app name) takes a *prompt*; anything else takes finished
/// *content*. Host is the stronger signal when present (a browser tab).
fn classify_branch(app: &str, host: Option<&str>) -> &'static str {
    const AI_HOSTS: &[&str] = &[
        "claude.ai",
        "chatgpt.com",
        "chat.openai.com",
        "gemini.google.com",
        "perplexity.ai",
        "copilot.microsoft.com",
        "poe.com",
        "grok.com",
    ];
    const AI_APPS: &[&str] = &["claude", "chatgpt", "cursor", "gemini", "perplexity", "copilot"];
    let host_l = host.unwrap_or("").to_lowercase();
    let app_l = app.to_lowercase();
    let is_ai =
        AI_HOSTS.iter().any(|h| host_l.contains(h)) || AI_APPS.iter().any(|a| app_l.contains(a));
    if is_ai {
        "prompt"
    } else {
        "content"
    }
}

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
    session: u64,
) -> Result<PromptOutcome> {
    let settings = settings_store::load(app).unwrap_or_default();
    let pm = &settings.prompt_mode;

    // Press-time snapshot wins: this is what the user was focused on when
    // they pressed the hotkey, even if another app (Perplexity) stole focus
    // by sharing the same global shortcut.
    let snapshot = app_detector::take_target_app();
    // The real frontmost process at hotkey-press time, kept regardless of which
    // resolution branch wins below. Used as the focus-restore fallback after the
    // review window steals focus when no explicit inject target exists (manual
    // override mode). Cloned now because `snapshot` is consumed by the match.
    let press_app = snapshot.clone();
    // Browser tab snapshot (only Some when frontmost was a known browser at
    // press time). Drained alongside the app snapshot so a stale value
    // doesn't leak into a subsequent recording. Suppressed when the user has
    // set a manual app override — they've told us the target explicitly.
    let browser_context = app_detector::take_target_browser_context();
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
    let browser_context = if manual_override_used {
        None
    } else {
        browser_context
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

    // Routing transparency: the destination is now resolved, so tell the overlay +
    // main window where this recording is headed and how it's being treated. Fire-
    // and-forget — a failed emit must never affect the actual rewrite/inject.
    let route_host = browser_context
        .as_ref()
        .map(|c| host_from_url(&c.url))
        .filter(|h| !h.is_empty());
    let _ = app.emit(
        EVENT_PROMPT_ROUTE,
        PromptRoutePayload {
            session,
            app: active_app.clone(),
            host: route_host.clone(),
            branch: classify_branch(&active_app, route_host.as_deref()),
            source: if manual_override_used { "manual" } else { "auto" },
        },
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

    // Race the rewrite against cancellation: an Esc (or a newer recording)
    // drops the request future, cancelling the in-flight HTTP call (issue #31).
    let rewritten = tokio::select! {
        biased;
        _ = crate::session::aborted(session) => {
            return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
        }
        r = llm::sonnet_prompt_rewrite(
            anthropic_api_key,
            raw_transcript,
            &active_app,
            browser_context.as_ref(),
            &selected_text,
        ) => r?,
    };

    if rewritten.trim().is_empty() {
        return Err(anyhow::anyhow!("Sonnet returned empty rewrite"));
    }

    // Resolve the text to paste and the app to paste it into. Review and the
    // passive preview are mutually exclusive gates between the rewrite and the
    // single inject below; whichever (if either) runs, it may abort the paste by
    // returning `CANCELLED_MARKER` early. Injection itself happens exactly once,
    // at the call site after this block.
    let (final_text, inject_to): (String, Option<String>) = if pm.review_before_insert {
        // Edit-before-insert review (PRD §0 backlog): pause the pipeline and show
        // the generated prompt in an editable window; nothing is pasted until the
        // user approves. Review supersedes the passive preview toast — an
        // interactive gate makes a timed auto-paste redundant.
        //
        // The review window steals focus, so restore the user's real target
        // before pasting: inject target (auto) wins, else the press-time app
        // (manual override), else nothing (no regression).
        let focus_target =
            prompt_review::review_focus_target(inject_target.as_deref(), press_app.as_deref());
        let rx = prompt_review::register(session);

        let _ = app.emit(
            EVENT_PROMPT_REVIEW_OPEN,
            PromptReviewPayload {
                session,
                text: rewritten.clone(),
                app: active_app.clone(),
                branch: classify_branch(&active_app, route_host.as_deref()),
            },
        );
        // Redact the prompt body: it must never hit the log file in cleartext
        // unless the user opted into verbose logging (issue #33).
        log::info!(
            "prompt review opened (session {session}): {}",
            crate::redact::redact(&rewritten)
        );
        show_review_window(app);

        // Wait for the user's decision, but bail immediately if this session is
        // cancelled (Esc) or superseded by a newer recording. `biased` makes the
        // abort branch win a tie against a simultaneous submit (issue #31).
        let decision = tokio::select! {
            biased;
            _ = crate::session::aborted(session) => {
                prompt_review::clear(session);
                hide_review_window(app);
                log::info!("prompt review cancelled (session {session})");
                return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
            }
            r = rx => match r {
                Ok(d) => d,
                // Sender dropped (cleared/superseded) → treat as cancel.
                Err(_) => prompt_review::ReviewDecision::Cancel,
            },
        };
        hide_review_window(app);

        // Cancel — or a whitespace-only edit — yields CANCELLED_MARKER, so `?`
        // aborts before any paste, exactly like an Esc.
        let final_text = prompt_review::decision_to_text(decision)?;
        (final_text, focus_target)
    } else {
        // Preview-before-insert: PRD §5.3 step 6. If enabled, show a toast with
        // the first 50 chars and wait the configured timeout before pasting.
        if pm.show_preview {
            let preview = first_chars(&rewritten, 50);
            toast::info(
                app,
                "Prompt generated",
                &format!("Inserting in {}s — {preview}", pm.preview_timeout_seconds),
            );
            // Wait out the preview window, but bail immediately if this session
            // is cancelled (Esc) or superseded by a newer recording (issue #31).
            let deadline = tokio::time::Instant::now()
                + Duration::from_secs(pm.preview_timeout_seconds as u64);
            loop {
                if crate::session::is_aborted(session) {
                    log::info!("prompt preview cancelled (session {session})");
                    return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
                }
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        (rewritten, inject_target)
    };

    injector::inject_text(app, &final_text, inject_to.as_deref(), session).await?;

    Ok(PromptOutcome {
        inserted: final_text,
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

#[cfg(test)]
mod tests {
    use super::{classify_branch, host_from_url};

    #[test]
    fn host_from_url_strips_scheme_path_port_and_www() {
        assert_eq!(host_from_url("https://www.claude.ai/chat/abc"), "claude.ai");
        assert_eq!(host_from_url("https://chatgpt.com:443/?q=1"), "chatgpt.com");
        assert_eq!(host_from_url("http://user@docs.google.com/d/1#h"), "docs.google.com");
        assert_eq!(host_from_url("not a url"), "not a url");
        assert_eq!(host_from_url(""), "");
    }

    #[test]
    fn classify_branch_routes_ai_hosts_and_apps_to_prompt() {
        assert_eq!(classify_branch("Google Chrome", Some("claude.ai")), "prompt");
        assert_eq!(classify_branch("Google Chrome", Some("chatgpt.com")), "prompt");
        assert_eq!(classify_branch("Cursor", None), "prompt");
        assert_eq!(classify_branch("Claude", None), "prompt");
    }

    #[test]
    fn classify_branch_routes_everything_else_to_content() {
        assert_eq!(classify_branch("Slack", None), "content");
        assert_eq!(classify_branch("Google Chrome", Some("docs.google.com")), "content");
        assert_eq!(classify_branch("Notes", None), "content");
        // Host is the stronger signal: a non-AI tab in a browser is content.
        assert_eq!(classify_branch("Safari", Some("github.com")), "content");
    }
}
