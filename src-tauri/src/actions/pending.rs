//! Pending-confirmation state for destructive actions.
//!
//! When an action with `destructive: true` is triggered by voice, the
//! executor stores it here instead of running it immediately. The user then
//! has CONFIRMATION_TIMEOUT to click "Confirm" in the tray menu, after
//! which the action is dispatched with gates bypassed. A new destructive
//! trigger supersedes any existing pending entry (logged).

use super::Action;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Runtime};

pub const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
struct Pending {
    id: u64,
    action: Action,
    /// Placeholder-resolved command snapshot taken at voice-trigger time. We
    /// confirm the exact command the user spoke, not a re-resolved version
    /// (clipboard / active app may have changed during the wait).
    resolved: String,
    /// Original query — kept for success_feedback / failure_feedback rendering.
    query: String,
    #[allow(dead_code)]
    created_at: Instant,
}

static SLOT: Lazy<RwLock<Option<Pending>>> = Lazy::new(|| RwLock::new(None));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Replace the pending slot with the new action and return its id. If another
/// destructive action was already pending it is dropped (and logged).
pub fn store(action: Action, resolved: String, query: String) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let next = Pending {
        id,
        action,
        resolved,
        query,
        created_at: Instant::now(),
    };
    if let Ok(mut w) = SLOT.write() {
        if let Some(prev) = w.replace(next) {
            log::info!(
                "pending confirmation '{}' superseded by a newer pending action",
                prev.action.id
            );
        }
    }
    id
}

fn take_if(expected_id: Option<u64>) -> Option<(Action, String, String)> {
    let mut w = SLOT.write().ok()?;
    let take = match (expected_id, w.as_ref()) {
        (None, Some(_)) => true,
        (Some(want), Some(p)) if p.id == want => true,
        _ => false,
    };
    if !take {
        return None;
    }
    w.take().map(|p| (p.action, p.resolved, p.query))
}

/// User clicked "Cancel pending action" in the tray.
pub fn cancel<R: Runtime>(app: &AppHandle<R>) {
    if let Some((action, _, _)) = take_if(None) {
        log::info!("pending confirmation '{}' cancelled by user", action.id);
        crate::toast::info(app, &action.name, "Cancelled");
    }
    crate::tray::set_pending_confirmation(app, None);
}

/// User clicked "Confirm: <name>" in the tray. Runs the action with the
/// destructive gate bypassed but the permission gate re-checked defensively.
pub async fn confirm_now<R: Runtime>(app: &AppHandle<R>) {
    let Some((action, resolved, query)) = take_if(None) else {
        return;
    };
    crate::tray::set_pending_confirmation(app, None);
    log::info!("pending confirmation '{}' confirmed by user", action.id);
    let outcome = crate::actions::executor::run_confirmed(app, &action, &resolved, &query).await;
    if outcome.success {
        crate::toast::info(app, &action.name, &outcome.message);
    } else {
        crate::toast::error(app, &action.name, &outcome.message);
    }
}

/// Spawn an async task that clears the pending entry after CONFIRMATION_TIMEOUT
/// if it's still the same id (avoids clearing a newer pending).
pub fn schedule_timeout<R: Runtime>(app: AppHandle<R>, id: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CONFIRMATION_TIMEOUT).await;
        if let Some((action, _, _)) = take_if(Some(id)) {
            log::info!("pending confirmation '{}' timed out", action.id);
            crate::toast::info(&app, &action.name, "Cancelled (timeout)");
            crate::tray::set_pending_confirmation(&app, None);
        }
    });
}

/// Clear the global pending slot. The `SLOT`/`NEXT_ID` statics are process-wide,
/// so cross-module tests that drive `execute()` must reset residue here before
/// asserting. Exposes only the clear, never the private `take_if`.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    let _ = take_if(None);
}

/// Process-wide serialization gate for any test that touches the global
/// `SLOT`/`NEXT_ID`. Shared across modules — `executor`'s suppression tests
/// lock this same static so the slot can't interleave under parallel
/// `cargo test`. A single gate (not one-per-module) is what makes that safe.
#[cfg(test)]
pub(crate) static TEST_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionType;

    fn action(id: &str) -> Action {
        Action {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            triggers: Vec::new(),
            action_type: ActionType::Shell,
            command: "true".to_string(),
            working_dir: None,
            requires_permissions: Vec::new(),
            destructive: true,
            success_feedback: String::new(),
            failure_feedback: String::new(),
            enabled: true,
        }
    }

    #[test]
    fn store_then_take_returns_the_action() {
        let _g = TEST_GATE.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_if(None);

        store(action("a"), "cmd-a".to_string(), "say a".to_string());
        let taken = take_if(None).expect("pending should be retrievable");
        assert_eq!(taken.0.id, "a");
        assert_eq!(taken.1, "cmd-a");
        assert_eq!(taken.2, "say a");
        // Slot is now empty — a second take yields nothing.
        assert!(take_if(None).is_none());
    }

    #[test]
    fn newer_destructive_supersedes_older() {
        let _g = TEST_GATE.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_if(None);

        let id1 = store(action("a"), "cmd-a".to_string(), "say a".to_string());
        let _id2 = store(action("b"), "cmd-b".to_string(), "say b".to_string());
        // The superseded id1 can no longer be confirmed.
        assert!(take_if(Some(id1)).is_none());
        // The live pending is the newer 'b'.
        let taken = take_if(None).expect("newer pending should remain");
        assert_eq!(taken.0.id, "b");
    }

    #[test]
    fn stale_timeout_does_not_clear_newer_pending() {
        let _g = TEST_GATE.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_if(None);

        let id1 = store(action("a"), "cmd-a".to_string(), "say a".to_string());
        let _id2 = store(action("b"), "cmd-b".to_string(), "say b".to_string());
        // Simulate the original timer firing after supersession: id-guarded
        // take is a no-op and must NOT evict the live 'b'.
        assert!(take_if(Some(id1)).is_none());
        let taken = take_if(None).expect("newer pending survives stale timeout");
        assert_eq!(taken.0.id, "b");
    }
}
