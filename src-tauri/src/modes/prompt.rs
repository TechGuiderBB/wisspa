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

/// User-facing toast body shown when Prompt Mode is triggered with no Anthropic
/// key configured. Names the provider and the exact Settings destination so the
/// user can self-serve. Must never contain a key value.
pub const ANTHROPIC_KEY_MISSING_TOAST: &str =
    "Anthropic API key missing - add it in Settings > API Keys";

/// Preflight classifier for Prompt Mode: the resolved Anthropic key is treated
/// as *missing* when it is absent or whitespace-only. Stronger than llm.rs's
/// bare `is_empty()` (catches a key set to "   "); that check stays as a
/// defence-in-depth backstop for the actual HTTP call. Pure + side-effect-free
/// so it is unit-testable without a Tauri app handle.
pub fn anthropic_key_missing(key: &str) -> bool {
    key.trim().is_empty()
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
    /// True when the Sonnet rewrite failed (or came back empty) and the raw
    /// transcript was inserted instead — mirrors dictation's `cleaned = false`,
    /// and lets history record that a fallback happened.
    pub used_fallback: bool,
}

/// Chatbot openers the Sonnet system prompt's Output contract forbids but the
/// model occasionally emits anyway ("Here is your prompt:", "Sure, …").
/// Matched case-insensitively at the start of a line, and only at a word
/// boundary, so legitimate content that merely begins with one of these words
/// mid-flow ("Surely the best plan …") is never touched.
const PREAMBLE_OPENERS: &[&str] = &["here is", "here's", "sure", "certainly", "of course"];

/// Does this line open with a chatbot preamble marker? The opener must be
/// followed by a non-alphanumeric character (or end of line) — the word
/// boundary is what keeps false strips below real catches.
fn is_preamble_line(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    PREAMBLE_OPENERS.iter().any(|opener| {
        lower.strip_prefix(opener).map_or(false, |rest| {
            rest.chars().next().map_or(true, |c| !c.is_alphanumeric())
        })
    })
}

/// Output guard: strip a leading model preamble the system prompt failed to
/// suppress. Conservative by design — the guard engages only when the FIRST
/// non-empty line opens with a `PREAMBLE_OPENERS` marker at a word boundary;
/// anything else returns byte-for-byte unchanged (false-strip prevention beats
/// completeness). When engaged, consecutive preamble lines and the blank
/// padding around them are dropped down to the first substantive line. Every
/// firing is logged: a rising rate is regression telemetry against the system
/// prompt. Pure apart from that log, so it is unit-testable without a Tauri
/// app handle.
fn strip_leading_preamble(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut idx = match lines.iter().position(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return text.to_string(), // all blank: nothing to strip
    };
    if !is_preamble_line(lines[idx]) {
        return text.to_string();
    }
    // Engaged: walk past every consecutive preamble line (and blank padding)
    // to the first substantive line.
    loop {
        match lines[idx + 1..].iter().position(|l| !l.trim().is_empty()) {
            Some(next) => {
                idx += 1 + next;
                if !is_preamble_line(lines[idx]) {
                    break;
                }
            }
            None => {
                log::warn!("prompt preamble guard fired: Sonnet output was preamble only");
                return String::new();
            }
        }
    }
    log::warn!("prompt preamble guard fired: stripped leading preamble line(s)");
    lines[idx..].join("\n")
}

/// Resolve the text that flows through the review/preview gates: Sonnet's
/// rewrite when it succeeded and is non-empty, else the raw transcript. Never
/// lose the utterance — the same philosophy as dictation mode's Haiku
/// fallback. Pure so the decision is unit-testable without a Tauri app.
fn rewrite_or_fallback(rewrite: Result<String>, raw_transcript: &str) -> (String, bool) {
    match rewrite {
        Ok(text) if !text.trim().is_empty() => (text, false),
        Ok(_) => {
            log::warn!("Sonnet returned empty rewrite — falling back to raw transcript");
            (raw_transcript.to_string(), true)
        }
        Err(e) => {
            log::error!("Sonnet rewrite failed, falling back to raw transcript: {e:#}");
            (raw_transcript.to_string(), true)
        }
    }
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
    let rewrite = tokio::select! {
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
        ) => r,
    };

    // Output guard: strip any chatbot preamble the system prompt failed to
    // suppress. Runs before the fallback check, so a rewrite the guard empties
    // (preamble only, nothing substantive) takes the same raw-transcript path
    // as a natively empty one.
    let rewrite = rewrite.map(|text| strip_leading_preamble(&text));

    // On rewrite failure, fall back to the raw transcript rather than losing
    // the utterance. The fallback flows through the SAME gates below (review
    // window if enabled, else the preview countdown), so the user can still
    // cancel or edit before anything is pasted.
    let (rewritten, used_fallback) = rewrite_or_fallback(rewrite, raw_transcript);
    if used_fallback {
        toast::warn(app, "Prompt rewrite failed", "Pasting raw transcript.");
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
        //
        // Quiet notifications suppress that toast (toast.rs), so waiting out
        // the timeout would be an invisible multi-second stall with zero
        // feedback — nothing on screen explains why the paste hasn't landed.
        // When the toast can't be seen, skip the wait and paste immediately.
        if pm.show_preview && !toast::is_quiet(app) {
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
        used_fallback,
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
    use super::{
        anthropic_key_missing, classify_branch, host_from_url, rewrite_or_fallback,
        strip_leading_preamble, ANTHROPIC_KEY_MISSING_TOAST,
    };

    #[test]
    fn anthropic_key_missing_detects_absent_and_whitespace_only() {
        assert!(anthropic_key_missing(""));
        assert!(anthropic_key_missing("   "));
        assert!(anthropic_key_missing("\t\n"));
        assert!(!anthropic_key_missing("sk-ant-abc123"));
        // Padding does not make a real key "missing".
        assert!(!anthropic_key_missing("  sk-ant-xyz  "));
    }

    #[test]
    fn missing_key_toast_names_provider_and_settings_path() {
        let m = ANTHROPIC_KEY_MISSING_TOAST;
        assert!(m.contains("Anthropic"));
        assert!(m.contains("Settings"));
        assert!(m.contains("API Keys"));
        // No key material ever embedded in the message.
        assert!(!m.contains("sk-ant"));
    }

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

    #[test]
    fn successful_rewrite_is_used_verbatim() {
        let (text, fallback) = rewrite_or_fallback(Ok("rewritten prompt".to_string()), "raw words");
        assert_eq!(text, "rewritten prompt");
        assert!(!fallback);
    }

    #[test]
    fn failed_rewrite_falls_back_to_raw_transcript() {
        let (text, fallback) =
            rewrite_or_fallback(Err(anyhow::anyhow!("Anthropic HTTP 500")), "raw words");
        assert_eq!(text, "raw words");
        assert!(fallback, "utterance must not be lost on rewrite failure");
    }

    #[test]
    fn empty_or_whitespace_rewrite_falls_back_to_raw_transcript() {
        for empty in ["", "   \n\t  "] {
            let (text, fallback) = rewrite_or_fallback(Ok(empty.to_string()), "raw words");
            assert_eq!(text, "raw words");
            assert!(fallback, "empty rewrite {empty:?} must fall back");
        }
    }

    #[test]
    fn guard_strips_single_preamble_line_and_blank_padding() {
        let out = strip_leading_preamble("Here is your prompt:\n\nDo the thing.");
        assert_eq!(out, "Do the thing.");
    }

    #[test]
    fn guard_matches_every_opener_case_insensitively() {
        for (input, expected) in [
            ("HERE IS the prompt:\nContent", "Content"),
            ("Here's a polished version:\nContent", "Content"),
            ("Sure!\nContent", "Content"),
            ("Certainly, here it is:\nContent", "Content"),
            ("Of course — here you go:\nContent", "Content"),
        ] {
            assert_eq!(strip_leading_preamble(input), expected, "input: {input}");
        }
    }

    #[test]
    fn guard_strips_stacked_preamble_lines() {
        let out = strip_leading_preamble("Sure!\nHere is your prompt:\n\nContent");
        assert_eq!(out, "Content");
    }

    #[test]
    fn guard_skips_leading_blank_lines_before_matching() {
        let out = strip_leading_preamble("\n\n  \nHere's your prompt:\n\nContent");
        assert_eq!(out, "Content");
    }

    #[test]
    fn guard_leaves_substantive_output_untouched() {
        // No preamble opener on the first line: returned byte-for-byte.
        let content = "Hi Sam,\n\nThanks for the update — looks good to me.\n\nBest,\nAlex";
        assert_eq!(strip_leading_preamble(content), content);
    }

    #[test]
    fn guard_only_inspects_the_first_non_empty_line() {
        // An opener mid-content is legitimate text, not a preamble.
        let content = "Hi Sam,\n\nSure, I can make it on Friday.\n\nBest,\nAlex";
        assert_eq!(strip_leading_preamble(content), content);
    }

    #[test]
    fn guard_requires_a_word_boundary() {
        // "Surely" merely starts with the letters of "sure" — not a preamble.
        let content = "Surely the best plan is to wait.";
        assert_eq!(strip_leading_preamble(content), content);
    }

    #[test]
    fn guard_returns_preamble_only_output_as_empty() {
        assert_eq!(strip_leading_preamble("Sure thing!"), "");
        // All-blank input has no preamble to strip: returned unchanged.
        assert_eq!(strip_leading_preamble("  \n  "), "  \n  ");
    }

    #[test]
    fn guard_emptied_rewrite_falls_back_to_raw_transcript() {
        // Composition check: a preamble-only rewrite is stripped to empty, and
        // the empty result takes the existing raw-transcript fallback path.
        let guarded = strip_leading_preamble("Here is your prompt:");
        let (text, fallback) = rewrite_or_fallback(Ok(guarded), "raw words");
        assert_eq!(text, "raw words");
        assert!(fallback, "guard-emptied rewrite must fall back");
    }
}
