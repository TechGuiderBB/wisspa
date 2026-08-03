//! Edit-before-insert review for Prompt Mode (PRD §0 backlog) and Dictation.
//!
//! When `prompt_mode.review_before_insert` (or `dictation.review_before_insert`)
//! is on, the pipeline pauses after the LLM pass and waits for the user to
//! edit/approve the generated text in the `review` window before it is pasted.
//! The wait is expressed as a `oneshot` channel keyed by the recording
//! `session`, so a superseded or cancelled recording resolves to nothing and
//! never pastes (issue #31 discipline): injection still happens at the single
//! call site in each mode runner, this module only *delivers* the user's text.
//!
//! Kept as a struct (rather than bare statics) so it can be unit-tested in
//! isolation — tests instantiate their own registry and never touch the
//! process-global one, mirroring `session::SessionTracker`.

use crate::hotkeys::CANCELLED_MARKER;
use anyhow::Result;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::oneshot;

/// Edit-before-insert event consumed by the `review` window. Matches the
/// frontend listener in `src/components/PromptReview.tsx`.
pub const EVENT_REVIEW_OPEN: &str = "wisspa://prompt-review-open";

/// Which pipeline opened the review — the window titles itself from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    Prompt,
    Dictation,
}

impl ReviewMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewMode::Prompt => "prompt",
            ReviewMode::Dictation => "dictation",
        }
    }

    fn window_title(&self) -> &'static str {
        match self {
            ReviewMode::Prompt => "Review prompt",
            ReviewMode::Dictation => "Review dictation",
        }
    }
}

/// Payload for `EVENT_REVIEW_OPEN`. Field names mirror the frontend
/// `ReviewOpen` type exactly (serde keeps these lowercase names).
#[derive(serde::Serialize, Clone)]
struct ReviewOpenPayload {
    /// Recording session id; the window keys its state by this so a superseded
    /// recording can't paste through a stale review (issue #31).
    session: u64,
    /// The LLM output, shown in the editable textarea.
    text: String,
    /// Resolved destination app, shown in the window header.
    app: String,
    /// "prompt" or "dictation" — which pipeline opened the review.
    mode: &'static str,
    /// Prompt mode's Branch A/B split ("prompt" | "content"), same split as
    /// the routing chip. None for dictation, which has no branch.
    branch: Option<&'static str>,
}

/// The user's decision on a pending review.
pub enum ReviewDecision {
    /// Insert the (possibly edited) text. Whitespace-only text is treated as a
    /// cancel by [`decision_to_text`].
    Insert(String),
    /// Cancel — paste nothing.
    Cancel,
}

/// Registry of in-flight reviews, keyed by recording session id.
pub struct ReviewRegistry {
    pending: Mutex<HashMap<u64, oneshot::Sender<ReviewDecision>>>,
}

impl ReviewRegistry {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Register a pending review for `session`, returning the receiver the
    /// pipeline awaits. A pre-existing entry for the same session is replaced
    /// (defensive — its receiver then resolves `Err`, which the caller maps to
    /// Cancel).
    pub fn register(&self, session: u64) -> oneshot::Receiver<ReviewDecision> {
        let (tx, rx) = oneshot::channel();
        // `insert` drops any prior sender for this session, closing its receiver.
        self.lock().insert(session, tx);
        rx
    }

    /// Deliver `decision` to a waiting pipeline. Returns whether a pending entry
    /// existed for `session` — a stale/superseded session has none, so this is a
    /// no-op and reports `false`. Sending into an already-dropped receiver is
    /// also a no-op (the result is intentionally ignored).
    pub fn resolve(&self, session: u64, decision: ReviewDecision) -> bool {
        match self.lock().remove(&session) {
            Some(tx) => {
                let _ = tx.send(decision);
                true
            }
            None => false,
        }
    }

    /// Drop a pending review without delivering a decision. The receiver then
    /// resolves `Err`, which the caller maps to Cancel.
    pub fn clear(&self, session: u64) {
        self.lock().remove(&session);
    }

    /// A poisoned lock here only means a thread panicked while holding the map;
    /// the map itself is still consistent, so recover the guard rather than
    /// propagating the panic into the audio pipeline.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<ReviewDecision>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for ReviewRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static REGISTRY: Lazy<ReviewRegistry> = Lazy::new(ReviewRegistry::new);

/// Register a pending review on the process-global registry.
pub fn register(session: u64) -> oneshot::Receiver<ReviewDecision> {
    REGISTRY.register(session)
}

/// Resolve a pending review on the process-global registry.
pub fn resolve(session: u64, decision: ReviewDecision) -> bool {
    REGISTRY.resolve(session, decision)
}

/// Clear a pending review on the process-global registry.
pub fn clear(session: u64) {
    REGISTRY.clear(session)
}

/// Map a [`ReviewDecision`] to the text to inject. Cancel — and an Insert whose
/// text is empty or whitespace-only — both yield `CANCELLED_MARKER`, so the
/// caller's `?` aborts the paste exactly like an Esc would. A non-empty Insert
/// returns the user's text verbatim (their own formatting is preserved).
pub fn decision_to_text(decision: ReviewDecision) -> Result<String> {
    match decision {
        ReviewDecision::Insert(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(anyhow::anyhow!(CANCELLED_MARKER)),
    }
}

/// Show + focus the review window so the user can edit the generated text.
/// A missing window is logged, not fatal: the pipeline still awaits the decision
/// and the user can abort with the global Esc cancel hotkey.
fn show_review_window<R: Runtime>(app: &AppHandle<R>, title: &str) {
    if let Some(w) = app.get_webview_window("review") {
        let _ = w.set_title(title);
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

/// The shared edit-before-insert gate, used by both prompt mode and dictation:
/// pause the pipeline, show `text` in the editable review window, and return
/// the approved text. Nothing is pasted until the user approves. Cancel — the
/// Cancel button, Esc, a superseding recording, or a whitespace-only edit —
/// resolves to `CANCELLED_MARKER` so the caller's `?` aborts before any paste,
/// exactly like an Esc.
///
/// The window steals focus while open; the caller is responsible for computing
/// the re-activation target up front via [`review_focus_target`] and passing
/// it to the injector once this returns.
pub async fn gate<R: Runtime>(
    app: &AppHandle<R>,
    session: u64,
    text: &str,
    app_label: &str,
    mode: ReviewMode,
    branch: Option<&'static str>,
) -> Result<String> {
    let rx = register(session);

    let _ = app.emit(
        EVENT_REVIEW_OPEN,
        ReviewOpenPayload {
            session,
            text: text.to_string(),
            app: app_label.to_string(),
            mode: mode.as_str(),
            branch,
        },
    );
    // Redact the body: it must never hit the log file in cleartext unless the
    // user opted into verbose logging (issue #33).
    log::info!(
        "{} review opened (session {session}): {}",
        mode.as_str(),
        crate::redact::redact(text)
    );
    show_review_window(app, mode.window_title());

    // Wait for the user's decision, but bail immediately if this session is
    // cancelled (Esc) or superseded by a newer recording. `biased` makes the
    // abort branch win a tie against a simultaneous submit (issue #31).
    let decision = tokio::select! {
        biased;
        _ = crate::session::aborted(session) => {
            clear(session);
            hide_review_window(app);
            log::info!("{} review cancelled (session {session})", mode.as_str());
            return Err(anyhow::anyhow!(CANCELLED_MARKER));
        }
        r = rx => match r {
            Ok(d) => d,
            // Sender dropped (cleared/superseded) → treat as cancel.
            Err(_) => ReviewDecision::Cancel,
        },
    };
    hide_review_window(app);

    decision_to_text(decision)
}

/// Resolve which app to bring forward before pasting the reviewed prompt.
///
/// The review window steals focus while open, so after hiding it we must
/// re-activate the user's real target. Only the pipeline's inject target
/// (the press-time auto-detected app in auto mode) is used. In manual-override
/// mode `inject_target` is `None` — we return `None` so no activation is
/// attempted, matching the non-review override path and avoiding hard activation
/// errors that would discard the user's edited text.
pub fn review_focus_target(inject_target: Option<&str>, _press_app: Option<&str>) -> Option<String> {
    inject_target.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_with_text_returns_the_text() {
        let out = decision_to_text(ReviewDecision::Insert("hello".to_string())).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn insert_with_whitespace_only_is_cancel() {
        let err = decision_to_text(ReviewDecision::Insert("   \n\t".to_string())).unwrap_err();
        assert!(err.to_string().contains(CANCELLED_MARKER));
    }

    #[test]
    fn cancel_maps_to_cancelled_marker() {
        let err = decision_to_text(ReviewDecision::Cancel).unwrap_err();
        assert!(err.to_string().contains(CANCELLED_MARKER));
    }

    #[test]
    fn focus_target_prefers_inject_target_in_auto_mode() {
        assert_eq!(
            review_focus_target(Some("Cursor"), Some("Slack")),
            Some("Cursor".to_string())
        );
    }

    #[test]
    fn focus_target_is_none_in_override_mode() {
        // Manual override → inject_target is None → return None so no activation
        // is attempted (matches non-review override path; avoids hard activation
        // errors that would discard the user's edited text).
        assert_eq!(review_focus_target(None, Some("Slack")), None);
    }

    #[test]
    fn focus_target_is_none_when_no_app_detected() {
        assert_eq!(review_focus_target(None, None), None);
    }

    #[test]
    fn review_mode_serialises_into_payload() {
        // The review window titles itself from `mode`; dictation carries no
        // branch (it has no prompt/content split), so it serialises as null.
        let dictation = ReviewOpenPayload {
            session: 9,
            text: "cleaned text".to_string(),
            app: "Slack".to_string(),
            mode: ReviewMode::Dictation.as_str(),
            branch: None,
        };
        let v = serde_json::to_value(&dictation).expect("payload serialises");
        assert_eq!(v["session"], 9);
        assert_eq!(v["mode"], "dictation");
        assert!(v["branch"].is_null(), "dictation must carry no branch");

        let prompt = ReviewOpenPayload {
            session: 9,
            text: "rewritten".to_string(),
            app: "Claude".to_string(),
            mode: ReviewMode::Prompt.as_str(),
            branch: Some("prompt"),
        };
        let v = serde_json::to_value(&prompt).expect("payload serialises");
        assert_eq!(v["mode"], "prompt");
        assert_eq!(v["branch"], "prompt");
    }

    #[test]
    fn review_mode_window_titles() {
        assert_eq!(ReviewMode::Prompt.window_title(), "Review prompt");
        assert_eq!(ReviewMode::Dictation.window_title(), "Review dictation");
    }

    #[test]
    fn register_then_resolve_delivers_decision() {
        let reg = ReviewRegistry::new();
        let mut rx = reg.register(7);
        assert!(reg.resolve(7, ReviewDecision::Insert("x".to_string())));
        match rx.try_recv() {
            Ok(ReviewDecision::Insert(t)) => assert_eq!(t, "x"),
            other => panic!("expected Insert(\"x\"), got something else: {}", label(&other)),
        }
    }

    #[test]
    fn resolve_unregistered_session_is_a_noop() {
        let reg = ReviewRegistry::new();
        assert!(!reg.resolve(42, ReviewDecision::Cancel));
    }

    #[test]
    fn clear_closes_the_receiver() {
        let reg = ReviewRegistry::new();
        let mut rx = reg.register(3);
        reg.clear(3);
        assert!(
            matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
            "cleared review must close the receiver (caller maps to Cancel)"
        );
        // And a later resolve finds nothing.
        assert!(!reg.resolve(3, ReviewDecision::Cancel));
    }

    #[test]
    fn registering_twice_replaces_the_first_sender() {
        let reg = ReviewRegistry::new();
        let mut rx1 = reg.register(5);
        let mut rx2 = reg.register(5);
        // The first receiver is closed because its sender was dropped.
        assert!(matches!(
            rx1.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        // The second resolves normally.
        assert!(reg.resolve(5, ReviewDecision::Insert("second".to_string())));
        match rx2.try_recv() {
            Ok(ReviewDecision::Insert(t)) => assert_eq!(t, "second"),
            other => panic!("expected Insert(\"second\"), got: {}", label(&other)),
        }
    }

    /// Test-only describer so panic messages don't require `Debug` on the channel
    /// result (the decision type intentionally has no `Debug` impl).
    fn label(r: &Result<ReviewDecision, oneshot::error::TryRecvError>) -> &'static str {
        match r {
            Ok(ReviewDecision::Insert(_)) => "Ok(Insert)",
            Ok(ReviewDecision::Cancel) => "Ok(Cancel)",
            Err(oneshot::error::TryRecvError::Empty) => "Err(Empty)",
            Err(oneshot::error::TryRecvError::Closed) => "Err(Closed)",
        }
    }
}
