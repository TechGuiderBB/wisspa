//! Shared retry policy for transient API failures (Groq STT, Anthropic).
//!
//! Both providers get at most ONE retry: this module answers the two policy
//! questions — is this failure worth another attempt, and how long to wait
//! first — so stt.rs and llm.rs share the classification instead of drifting
//! apart. Waits are deliberately short: the voice pipeline is
//! latency-sensitive, and the HTTP client's total timeout still bounds every
//! attempt.

use reqwest::header::HeaderMap;
use reqwest::StatusCode;
use std::time::Duration;

/// Total attempts per call, i.e. one retry after the first failure.
pub const MAX_ATTEMPTS: u8 = 2;

/// Backoff before retrying a 5xx or a transport-level failure.
pub const TRANSIENT_BACKOFF: Duration = Duration::from_millis(400);

/// Backoff for a 429 whose Retry-After header is absent or unparseable.
pub const RATE_LIMIT_FALLBACK: Duration = Duration::from_secs(1);

/// Hard cap on the 429 wait: a provider's Retry-After must not park the
/// pipeline for its full rate-limit window.
pub const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(2);

/// Is a failed attempt worth one retry? `None` = transport-level failure
/// (DNS, connect, TLS, timeout) where no HTTP response ever arrived — treated
/// as transient. Of the HTTP statuses, only 429 (rate limit) and 5xx (server
/// error) are transient; other 4xx (400/401/403/…) are client or auth
/// problems a retry would only repeat.
pub fn should_retry(status: Option<StatusCode>) -> bool {
    match status {
        None => true,
        Some(s) => s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error(),
    }
}

/// How long to wait before the single retry. A 429 honours the provider's
/// Retry-After hint clamped to MAX_RATE_LIMIT_WAIT (defaulting to
/// RATE_LIMIT_FALLBACK when absent); everything else — 5xx and transport
/// failures — uses the fixed short backoff, ignoring Retry-After.
pub fn retry_delay(status: Option<StatusCode>, headers: Option<&HeaderMap>) -> Duration {
    if status == Some(StatusCode::TOO_MANY_REQUESTS) {
        return retry_after_secs(headers)
            .map(Duration::from_secs)
            .map(|d| d.min(MAX_RATE_LIMIT_WAIT))
            .unwrap_or(RATE_LIMIT_FALLBACK);
    }
    TRANSIENT_BACKOFF
}

/// Parse a Retry-After header value as whole seconds. The integer form is
/// what both providers send; fractional seconds are rounded up rather than
/// truncated so we never retry inside the provider's window. The HTTP-date
/// form is not expected from these APIs and yields `None`, i.e. the caller
/// falls back to the default wait.
fn retry_after_secs(headers: Option<&HeaderMap>) -> Option<u64> {
    let raw = headers?
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(secs);
    }
    raw.parse::<f64>().ok().map(|f| f.ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_retry_after(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, value.parse().unwrap());
        h
    }

    #[test]
    fn transport_error_is_retryable_with_short_backoff() {
        // No HTTP response ever arrived (DNS/connect/TLS/timeout) — the
        // classic transient failure, retried after the fixed backoff.
        assert!(should_retry(None));
        assert_eq!(retry_delay(None, None), TRANSIENT_BACKOFF);
    }

    #[test]
    fn server_errors_are_retryable_with_short_backoff() {
        for code in [500u16, 502, 503, 529] {
            let status = StatusCode::from_u16(code).unwrap();
            assert!(should_retry(Some(status)), "{code} must be retryable");
            assert_eq!(
                retry_delay(Some(status), None),
                TRANSIENT_BACKOFF,
                "{code} must use the fixed backoff"
            );
        }
    }

    #[test]
    fn server_error_ignores_retry_after() {
        // Retry-After honouring is scoped to 429 only; a 5xx always waits
        // the fixed backoff even when the header is present.
        let h = headers_with_retry_after("5");
        let status = StatusCode::SERVICE_UNAVAILABLE;
        assert_eq!(retry_delay(Some(status), Some(&h)), TRANSIENT_BACKOFF);
    }

    #[test]
    fn client_errors_are_not_retryable() {
        for code in [400u16, 401, 403, 404, 413, 422] {
            let status = StatusCode::from_u16(code).unwrap();
            assert!(!should_retry(Some(status)), "{code} must not be retried");
        }
    }

    #[test]
    fn rate_limit_is_retryable_and_honours_retry_after() {
        let status = StatusCode::TOO_MANY_REQUESTS;
        assert!(should_retry(Some(status)));
        let h = headers_with_retry_after("1");
        assert_eq!(retry_delay(Some(status), Some(&h)), Duration::from_secs(1));
    }

    #[test]
    fn rate_limit_retry_after_is_capped() {
        // A provider asking for a long wait must not park the pipeline.
        let h = headers_with_retry_after("30");
        assert_eq!(
            retry_delay(Some(StatusCode::TOO_MANY_REQUESTS), Some(&h)),
            MAX_RATE_LIMIT_WAIT
        );
    }

    #[test]
    fn rate_limit_without_retry_after_uses_default() {
        let status = StatusCode::TOO_MANY_REQUESTS;
        // No headers at all.
        assert_eq!(retry_delay(Some(status), None), RATE_LIMIT_FALLBACK);
        // Header absent from the map.
        assert_eq!(
            retry_delay(Some(status), Some(&HeaderMap::new())),
            RATE_LIMIT_FALLBACK
        );
        // Unparseable value (e.g. the HTTP-date form) falls back too.
        let h = headers_with_retry_after("Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(retry_delay(Some(status), Some(&h)), RATE_LIMIT_FALLBACK);
    }

    #[test]
    fn fractional_retry_after_rounds_up() {
        // 1.5s must not become 1s (still inside the provider's window).
        let h = headers_with_retry_after("1.5");
        assert_eq!(
            retry_delay(Some(StatusCode::TOO_MANY_REQUESTS), Some(&h)),
            Duration::from_secs(2)
        );
    }
}
