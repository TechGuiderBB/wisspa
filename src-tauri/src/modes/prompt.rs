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
///
/// SYNC: the host/app lists below mirror the AI-tool and non-AI destination
/// lists in `src-tauri/src/prompts/sonnet_prompt.md` Step 0, and MUST stay in
/// sync with it — a tool known to the system prompt but missing here lets this
/// chip disagree with the branch Sonnet actually takes.
fn classify_branch(app: &str, host: Option<&str>) -> &'static str {
    // AI-tool web hosts (product in trailing comment). Cursor, Claude Code,
    // GitHub Copilot and Codex have no web chat host — they match via AI_APPS.
    const AI_HOSTS: &[&str] = &[
        "claude.ai",             // Claude
        "chatgpt.com",           // ChatGPT (also Codex web)
        "chat.openai.com",       // ChatGPT (legacy host)
        "gemini.google.com",     // Gemini
        "copilot.microsoft.com", // Microsoft Copilot
        "perplexity.ai",         // Perplexity
        "you.com",               // You.com
        "poe.com",               // Poe
        "mistral.ai",            // Mistral chat (Le Chat)
        "huggingface.co",        // Hugging Chat
        "grok.com",              // Grok
        "x.ai",                  // Grok (legacy host)
        "kagi.com",              // Kagi Assistant
    ];
    // Known NON-AI destination hosts from Step 0. Checked FIRST so a host that
    // could match both lists resolves to content deterministically.
    const NON_AI_HOSTS: &[&str] = &[
        "gmail.com",          // Gmail
        "mail.google.com",    // Gmail
        "outlook.live.com",   // Outlook
        "outlook.office.com", // Outlook
        "linkedin.com",       // LinkedIn
        "twitter.com",        // Twitter/X
        "x.com",              // Twitter/X
        "facebook.com",       // Facebook
        "reddit.com",         // Reddit
        "notion.so",          // Notion
        "slack.com",          // Slack
        "discord.com",        // Discord
        "docs.google.com",    // Google Docs
        "atlassian.net",      // Confluence / Jira
        "github.com",         // GitHub issues/PRs
        "stackoverflow.com",  // Stack Overflow
    ];
    // AI-tool process names (System Events name, lowercased substring match).
    const AI_APPS: &[&str] = &[
        "claude",     // Claude / Claude Code
        "chatgpt",    // ChatGPT
        "codex",      // Codex
        "cursor",     // Cursor
        "gemini",     // Gemini
        "perplexity", // Perplexity
        "copilot",    // Microsoft Copilot / GitHub Copilot
        "poe",        // Poe
        "grok",       // Grok
        "le chat",    // Mistral (Le Chat app)
    ];
    let host_l = host.unwrap_or("").to_lowercase();
    let app_l = app.to_lowercase();
    // Hosts match on a dot boundary (exact or subdomain), not bare substring,
    // so a lookalike like "notyou.com" can't pose as you.com.
    let host_matches = |list: &[&str]| {
        !host_l.is_empty()
            && list
                .iter()
                .any(|h| host_l == *h || host_l.ends_with(&format!(".{h}")))
    };
    if host_matches(NON_AI_HOSTS) {
        return "content";
    }
    let is_ai = host_matches(AI_HOSTS) || AI_APPS.iter().any(|a| app_l.contains(a));
    if is_ai {
        "prompt"
    } else {
        "content"
    }
}

/// Word count above which a transcript is treated as COMPLEX for the adaptive
/// refine pass. Below it a single Sonnet pass is enough — the second call
/// would add latency with little upside.
const COMPLEX_WORD_COUNT: usize = 120;

/// Multi-task markers for the complexity classifier, kept deliberately tiny:
/// phrases that signal a second task being stacked onto the first ("do X and
/// then do Y", "…, also, …"). Matched case-insensitively as substrings — a
/// false positive only costs one upside-only critique call, a false negative
/// just skips it.
const MULTI_TASK_MARKERS: &[&str] = &["and then", "also,"];

/// Complexity classifier for the adaptive refine pass: a transcript is COMPLEX
/// when it is long (> COMPLEX_WORD_COUNT words), stacks multiple tasks
/// (MULTI_TASK_MARKERS), or dictates a numbered list (`1.` / `2)` style).
/// Pure + side-effect-free so it is unit-testable without a Tauri app handle.
fn is_complex_transcript(transcript: &str) -> bool {
    if transcript.split_whitespace().count() > COMPLEX_WORD_COUNT {
        return true;
    }
    let lower = transcript.to_lowercase();
    if MULTI_TASK_MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    has_numbered_list(&lower)
}

/// Detect a dictated numbered list — `1.` / `2)` style enumerations signal the
/// user is dictating multiple items or tasks. A run of digits at a word start,
/// then `.` or `)`, then whitespace (or end of text). The trailing-whitespace
/// rule keeps version strings like "2.0" from counting as lists.
fn has_numbered_list(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if !chars[i].is_ascii_digit() {
            continue;
        }
        let word_start = i == 0 || chars[i - 1].is_whitespace();
        if !word_start {
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j < chars.len()
            && (chars[j] == '.' || chars[j] == ')')
            && (j + 1 >= chars.len() || chars[j + 1].is_whitespace())
        {
            return true;
        }
    }
    false
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

/// Fold the adaptive critique result into the pipeline: a successful, non-empty
/// refined draft wins; an empty or failed critique keeps the first draft.
/// Refinement is upside-only — the user never loses a working first draft to a
/// bad second pass. Pure apart from the warn logs, so unit-testable without a
/// Tauri app handle.
fn refine_or_draft(refined: Result<String>, draft: String) -> String {
    match refined {
        Ok(text) if !text.trim().is_empty() => text,
        Ok(_) => {
            log::warn!("adaptive refine returned empty — keeping first draft");
            draft
        }
        Err(e) => {
            log::warn!("adaptive refine failed, keeping first draft: {e:#}");
            draft
        }
    }
}

/// Phase 5 prompt-mode pipeline:
///   transcript → resolve active app (manual override or AppleScript)
///              → optionally capture selected text
///              → Sonnet rewrite
///              → optional adaptive critique pass (complex transcripts only)
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
            &pm.user_profile,
        ) => r,
    };

    // Output guard: strip any chatbot preamble the system prompt failed to
    // suppress. Runs before the fallback check, so a rewrite the guard empties
    // (preamble only, nothing substantive) takes the same raw-transcript path
    // as a natively empty one.
    let rewrite = rewrite.map(|text| strip_leading_preamble(&text));

    // Adaptive second pass (pm.adaptive_refine, default on): for COMPLEX
    // transcripts, a critique call checks the draft against the quality bar and
    // returns the improved (or unchanged) draft. Upside-only: a failed/empty
    // critique keeps the first draft, and it only runs on a rewrite that
    // already succeeded — the raw-transcript fallback path below is untouched.
    let rewrite = match rewrite {
        Ok(draft)
            if pm.adaptive_refine
                && !draft.trim().is_empty()
                && is_complex_transcript(raw_transcript) =>
        {
            log::info!("adaptive refine engaged (complex transcript)");
            // Same cancellation race as the first pass: an Esc (or a newer
            // recording) drops the in-flight critique request (issue #31).
            let refined = tokio::select! {
                biased;
                _ = crate::session::aborted(session) => {
                    return Err(anyhow::anyhow!(crate::hotkeys::CANCELLED_MARKER));
                }
                r = llm::sonnet_critique_refine(
                    anthropic_api_key,
                    raw_transcript,
                    &active_app,
                    browser_context.as_ref(),
                    &selected_text,
                    &pm.user_profile,
                    &draft,
                ) => r,
            };
            // The refined output passes through the same preamble guard as the
            // first draft; if that empties it, refine_or_draft keeps the draft.
            Ok(refine_or_draft(
                refined.map(|text| strip_leading_preamble(&text)),
                draft,
            ))
        }
        other => other,
    };

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
        anthropic_key_missing, classify_branch, has_numbered_list, host_from_url,
        is_complex_transcript, refine_or_draft, rewrite_or_fallback, strip_leading_preamble,
        ANTHROPIC_KEY_MISSING_TOAST, COMPLEX_WORD_COUNT,
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
    fn classify_branch_matches_sonnet_step0_tool_lists() {
        // Sync guard: every AI-tool host named in sonnet_prompt.md Step 0 must
        // route to "prompt" here, and every non-AI destination host to
        // "content". If this test fails after a prompt edit, the two lists
        // drifted — update both files together.
        for host in [
            "claude.ai",
            "chatgpt.com",
            "chat.openai.com",
            "gemini.google.com",
            "copilot.microsoft.com",
            "perplexity.ai",
            "you.com",
            "poe.com",
            "chat.mistral.ai",
            "huggingface.co",
            "grok.com",
            "x.ai",
            "kagi.com",
        ] {
            assert_eq!(
                classify_branch("Google Chrome", Some(host)),
                "prompt",
                "AI host {host} must route to prompt"
            );
        }
        for host in [
            "gmail.com",
            "mail.google.com",
            "outlook.live.com",
            "linkedin.com",
            "twitter.com",
            "x.com",
            "facebook.com",
            "reddit.com",
            "notion.so",
            "slack.com",
            "discord.com",
            "docs.google.com",
            "github.com",
            "stackoverflow.com",
            "confluence.atlassian.net",
        ] {
            assert_eq!(
                classify_branch("Google Chrome", Some(host)),
                "content",
                "non-AI host {host} must route to content"
            );
        }
    }

    #[test]
    fn classify_branch_host_matching_is_boundary_aware() {
        // A lookalike host is not the real tool…
        assert_eq!(classify_branch("Google Chrome", Some("notyou.com")), "content");
        assert_eq!(classify_branch("Google Chrome", Some("claude.ai.evil.example")), "content");
        // …but a genuine subdomain of an AI host still matches.
        assert_eq!(classify_branch("Google Chrome", Some("auth.claude.ai")), "prompt");
        // A non-AI host wins over an AI app name: the tab is the stronger signal.
        assert_eq!(classify_branch("Copilot", Some("mail.google.com")), "content");
    }

    #[test]
    fn short_simple_transcript_is_not_complex() {
        assert!(!is_complex_transcript("summarise this email in two sentences"));
        assert!(!is_complex_transcript(""));
        // A lone "also" without the comma is prose, not a stacked task.
        assert!(!is_complex_transcript("tell him I also want the report"));
    }

    #[test]
    fn long_transcript_is_complex_by_word_count() {
        let at_limit = "word ".repeat(COMPLEX_WORD_COUNT);
        assert!(
            !is_complex_transcript(at_limit.trim()),
            "exactly {COMPLEX_WORD_COUNT} words stays simple"
        );
        let over_limit = "word ".repeat(COMPLEX_WORD_COUNT + 1);
        assert!(
            is_complex_transcript(over_limit.trim()),
            "{} words is complex",
            COMPLEX_WORD_COUNT + 1
        );
    }

    #[test]
    fn multi_task_markers_make_transcript_complex() {
        assert!(is_complex_transcript("fix the login bug and then update the tests"));
        // Case-insensitive.
        assert!(is_complex_transcript("email Priya, Also, book the room"));
    }

    #[test]
    fn numbered_list_makes_transcript_complex() {
        assert!(is_complex_transcript("three things 1. fix login 2. update tests 3. deploy"));
        assert!(is_complex_transcript("1) first item 2) second item"));
        // Version strings are not lists: no whitespace after the dot.
        assert!(!is_complex_transcript("we upgraded to version 2.0 last week"));
        // A digit run not at a word start is not a list marker.
        assert!(!has_numbered_list("abc1. def"));
    }

    #[test]
    fn successful_refine_replaces_the_draft() {
        let out = refine_or_draft(Ok("better draft".to_string()), "first draft".to_string());
        assert_eq!(out, "better draft");
    }

    #[test]
    fn failed_refine_keeps_first_draft() {
        let out = refine_or_draft(
            Err(anyhow::anyhow!("Anthropic HTTP 500")),
            "first draft".to_string(),
        );
        assert_eq!(out, "first draft", "refinement is upside-only");
    }

    #[test]
    fn empty_or_whitespace_refine_keeps_first_draft() {
        for empty in ["", "   \n\t  "] {
            let out = refine_or_draft(Ok(empty.to_string()), "first draft".to_string());
            assert_eq!(out, "first draft", "empty refine {empty:?} must keep the draft");
        }
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
