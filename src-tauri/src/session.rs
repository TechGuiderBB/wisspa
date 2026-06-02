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
}
