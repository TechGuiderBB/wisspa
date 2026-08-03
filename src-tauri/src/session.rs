//! Recording-session lifecycle + cancellation (issue #31).
//!
//! Every hotkey press starts a new *session* identified by a monotonic id. The
//! id is threaded from the press, through `process_audio`, the mode runners,
//! and into `inject_text`, so any stage can cheaply ask "is this still the
//! recording the user wants?" and bail out otherwise.
//!
//! Two things invalidate a session, both expressed with one `cancelled_through`
//! watermark — a session is aborted iff `session <= cancelled_through`:
//!   * **Cancel** — the user presses Esc. `cancel_active()` marks every session
//!     up to and including the current one as aborted.
//!   * **Supersede (latest-wins overlap policy)** — the user starts a new
//!     recording before the previous finished. `begin()` marks every prior
//!     session aborted, so only the newest recording ever injects.
//!
//! A `tokio::sync::watch` channel broadcasts watermark changes so an in-flight
//! STT/LLM call can be aborted promptly via `tokio::select!`: dropping the
//! request future cancels the underlying reqwest call, rather than merely
//! discarding its result after the fact.
//!
//! A session is *live* from `begin()` until it is aborted (cancel/supersede)
//! or retired by `complete()` when its pipeline finishes. `has_active()`
//! exposes that as a single watermark comparison so the Esc handler can tell
//! "user wants to cancel" apart from "user pressed Esc in an unrelated app".

use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::watch;

/// The session state machine. Kept as a struct (rather than bare statics) so it
/// can be unit-tested in isolation — tests instantiate their own tracker and
/// never touch the process-global one.
pub struct SessionTracker {
    generation: AtomicU64,
    cancelled_through: AtomicU64,
}

impl SessionTracker {
    pub const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            cancelled_through: AtomicU64::new(0),
        }
    }

    /// Start a new session, superseding any older in-flight session. Returns
    /// the new session id (always >= 1).
    pub fn begin(&self) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // Latest-wins: every session before this one is now superseded.
        self.cancelled_through.fetch_max(id - 1, Ordering::SeqCst);
        id
    }

    /// Mark every session up to and including the current one as aborted.
    pub fn cancel_active(&self) {
        let cur = self.generation.load(Ordering::SeqCst);
        self.cancelled_through.fetch_max(cur, Ordering::SeqCst);
    }

    /// Retire a session whose pipeline has finished (success, failure, or a
    /// terminal report such as silent/timeout). Same watermark move as an
    /// abort — the session is over either way — but driven by completion, so
    /// `has_active()` drops to false once nothing is in flight. Idempotent;
    /// session id 0 (unscoped/legacy) is a no-op.
    pub fn complete(&self, session: u64) {
        self.cancelled_through.fetch_max(session, Ordering::SeqCst);
    }

    /// True while any session is live: begun but not yet cancelled,
    /// superseded, or completed. Powers the Esc guard — without it every Esc
    /// press in any app would play the cancel sound and churn tray state.
    pub fn has_active(&self) -> bool {
        self.generation.load(Ordering::SeqCst) > self.cancelled_through.load(Ordering::SeqCst)
    }

    /// True if `session` was cancelled (Esc) or superseded by a newer recording.
    /// Session id 0 is "unscoped" (e.g. a frontend predating session plumbing)
    /// and is never auto-aborted, preserving legacy behaviour.
    pub fn is_aborted(&self, session: u64) -> bool {
        session != 0 && session <= self.cancelled_through.load(Ordering::SeqCst)
    }
}

static TRACKER: SessionTracker = SessionTracker::new();
static CANCEL_TX: Lazy<watch::Sender<u64>> = Lazy::new(|| watch::channel(0).0);

fn broadcast() {
    let _ = CANCEL_TX.send(TRACKER.cancelled_through.load(Ordering::SeqCst));
}

/// Start a new session on hotkey press. Wakes any superseded in-flight session.
pub fn begin() -> u64 {
    let id = TRACKER.begin();
    broadcast();
    id
}

/// Cancel the current (and all older) sessions — called when Esc is pressed.
pub fn cancel_active() {
    TRACKER.cancel_active();
    broadcast();
}

/// Retire a session whose pipeline has finished. No broadcast: nothing can be
/// waiting on `aborted()` for a session whose pipeline has already returned.
pub fn complete(session: u64) {
    TRACKER.complete(session);
}

/// True while a recording or pipeline is live. Checked by the Esc handler so
/// an Esc pressed with nothing in flight is a no-op instead of playing the
/// cancel sound into an unrelated app.
pub fn has_active() -> bool {
    TRACKER.has_active()
}

/// Retires a session on drop. `process_audio` holds one so EVERY exit path
/// (success, error, abort, early return) marks the session complete without
/// enumerating returns — a missed path would leak the session and leave the
/// Esc guard stuck on "active".
pub struct SessionCompletion(u64);

impl SessionCompletion {
    pub fn new(session: u64) -> Self {
        Self(session)
    }
}

impl Drop for SessionCompletion {
    fn drop(&mut self) {
        complete(self.0);
    }
}

pub fn is_aborted(session: u64) -> bool {
    TRACKER.is_aborted(session)
}

/// Resolves as soon as `session` becomes aborted (cancelled or superseded). Use
/// with `tokio::select!` to abort an in-flight STT/LLM request — dropping the
/// request future cancels the underlying reqwest call. Never resolves for the
/// unscoped session id 0.
pub async fn aborted(session: u64) {
    if session == 0 {
        return std::future::pending().await;
    }
    let mut rx = CANCEL_TX.subscribe();
    loop {
        if is_aborted(session) {
            return;
        }
        // `watch` retains the latest value, so a send between the check above
        // and this await is not lost — `changed()` returns immediately then.
        if rx.changed().await.is_err() {
            // Sender is 'static and never dropped; treat as never-cancel.
            return std::future::pending().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionTracker;

    #[test]
    fn first_session_is_active() {
        let t = SessionTracker::new();
        let s = t.begin();
        assert_eq!(s, 1);
        assert!(!t.is_aborted(s));
    }

    #[test]
    fn cancel_aborts_current_session() {
        let t = SessionTracker::new();
        let s = t.begin();
        assert!(!t.is_aborted(s));
        t.cancel_active();
        assert!(t.is_aborted(s), "cancelled session must be aborted");
    }

    #[test]
    fn starting_a_new_recording_supersedes_the_previous() {
        let t = SessionTracker::new();
        let a = t.begin();
        let b = t.begin();
        assert_eq!((a, b), (1, 2));
        assert!(t.is_aborted(a), "older session is superseded (latest-wins)");
        assert!(!t.is_aborted(b), "newest session stays active");
    }

    #[test]
    fn cancel_aborts_all_in_flight_sessions() {
        let t = SessionTracker::new();
        let a = t.begin();
        let b = t.begin();
        t.cancel_active();
        assert!(t.is_aborted(a));
        assert!(t.is_aborted(b), "Esc cancels everything in flight");
        let c = t.begin();
        assert!(!t.is_aborted(c), "a fresh recording after cancel is active");
    }

    #[test]
    fn unscoped_session_zero_is_never_aborted() {
        let t = SessionTracker::new();
        t.begin();
        t.cancel_active();
        assert!(!t.is_aborted(0), "legacy/unscoped id 0 is passthrough");
    }

    #[test]
    fn a_completed_session_does_not_abort_the_next() {
        let t = SessionTracker::new();
        let a = t.begin();
        // session a runs to completion, then the user records again
        let b = t.begin();
        assert!(t.is_aborted(a));
        assert!(!t.is_aborted(b));
    }

    #[test]
    fn session_is_active_from_begin_until_complete() {
        let t = SessionTracker::new();
        assert!(!t.has_active(), "idle at startup — Esc must no-op");
        let s = t.begin();
        assert!(t.has_active(), "live across press → release → pipeline");
        t.complete(s);
        assert!(!t.has_active(), "pipeline finished — idle again");
    }

    #[test]
    fn cancel_clears_active() {
        let t = SessionTracker::new();
        t.begin();
        assert!(t.has_active());
        t.cancel_active();
        assert!(!t.has_active());
    }

    #[test]
    fn completing_an_older_session_keeps_the_newer_one_active() {
        let t = SessionTracker::new();
        let a = t.begin();
        let b = t.begin();
        // a's aborted pipeline returns late and retires itself.
        t.complete(a);
        assert!(t.has_active(), "newer session still live");
        t.complete(b);
        assert!(!t.has_active());
    }

    #[test]
    fn complete_is_idempotent_and_zero_is_a_noop() {
        let t = SessionTracker::new();
        t.complete(0);
        assert!(!t.has_active());
        let s = t.begin();
        t.complete(s);
        t.complete(s);
        assert!(!t.has_active());
    }
}
