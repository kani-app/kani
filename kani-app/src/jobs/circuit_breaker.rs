use crate::jobs::error::DownloadErrorKind;

const FAILURE_THRESHOLD: u32 = 5;
const OPEN_DURATION_SECS: i64 = 120;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half_open"),
        }
    }
}

pub struct CircuitBreaker {
    pub state: CircuitState,
    pub failure_count: u32,
    pub last_failure_at: Option<i64>,
    pub next_retry_at: Option<i64>,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            last_failure_at: None,
            next_retry_at: None,
        }
    }

    pub(crate) fn is_open_at(&self, now: i64) -> bool {
        match &self.state {
            CircuitState::Open => self.next_retry_at.is_none_or(|t| now < t),
            _ => false,
        }
    }

    pub(crate) fn maybe_transition_to_half_open(&mut self, now: i64) {
        if matches!(&self.state, CircuitState::Open) && self.next_retry_at.is_some_and(|t| now >= t)
        {
            self.state = CircuitState::HalfOpen;
        }
    }

    fn counts_toward_threshold(kind: &DownloadErrorKind) -> bool {
        matches!(
            kind,
            DownloadErrorKind::Network { .. }
                | DownloadErrorKind::ParseError { .. }
                | DownloadErrorKind::ExtensionError { .. }
                | DownloadErrorKind::Unknown { .. }
        )
    }

    pub fn record_failure(&mut self, kind: &DownloadErrorKind, now: i64) {
        if matches!(
            kind,
            DownloadErrorKind::NotFound | DownloadErrorKind::Cancelled
        ) {
            return;
        }

        self.last_failure_at = Some(now);

        if matches!(kind, DownloadErrorKind::AuthRequired) {
            self.failure_count = FAILURE_THRESHOLD;
            self.state = CircuitState::Open;
            self.next_retry_at = Some(now + OPEN_DURATION_SECS);
            return;
        }

        if Self::counts_toward_threshold(kind) {
            self.failure_count = self.failure_count.saturating_add(1);
        }

        match &self.state {
            CircuitState::Closed => {
                if self.failure_count >= FAILURE_THRESHOLD {
                    self.state = CircuitState::Open;
                    self.next_retry_at = Some(now + OPEN_DURATION_SECS);
                }
            }
            CircuitState::HalfOpen => {
                self.state = CircuitState::Open;
                self.next_retry_at = Some(now + OPEN_DURATION_SECS);
            }
            CircuitState::Open => {
                self.next_retry_at = Some(now + OPEN_DURATION_SECS);
            }
        }
    }

    pub fn record_success(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.last_failure_at = None;
        self.next_retry_at = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn now() -> i64 {
        1_000_000_000i64
    }

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new();
        assert_eq!(cb.state, CircuitState::Closed);
        assert!(!cb.is_open_at(now()));
    }

    #[test]
    fn opens_after_threshold_network_failures() {
        let mut cb = CircuitBreaker::new();
        let kind = DownloadErrorKind::Network { retryable: true };
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(&kind, now());
        }
        assert_eq!(cb.state, CircuitState::Open);
        assert!(cb.is_open_at(now()));
    }

    #[test]
    fn auth_required_opens_immediately() {
        let mut cb = CircuitBreaker::new();
        cb.record_failure(&DownloadErrorKind::AuthRequired, now());
        assert_eq!(cb.state, CircuitState::Open);
        assert!(cb.is_open_at(now()));
    }

    #[test]
    fn not_found_does_not_count() {
        let mut cb = CircuitBreaker::new();
        for _ in 0..10 {
            cb.record_failure(&DownloadErrorKind::NotFound, now());
        }
        assert_eq!(cb.state, CircuitState::Closed);
    }

    #[test]
    fn transitions_to_half_open_after_timeout() {
        let mut cb = CircuitBreaker::new();
        let kind = DownloadErrorKind::Network { retryable: true };
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(&kind, now());
        }
        let future = now() + OPEN_DURATION_SECS + 1;
        cb.maybe_transition_to_half_open(future);
        assert_eq!(cb.state, CircuitState::HalfOpen);
        assert!(!cb.is_open_at(future));
    }

    #[test]
    fn half_open_success_closes_circuit() {
        let mut cb = CircuitBreaker::new();
        cb.state = CircuitState::HalfOpen;
        cb.failure_count = FAILURE_THRESHOLD;
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn half_open_failure_reopens() {
        let mut cb = CircuitBreaker::new();
        cb.state = CircuitState::HalfOpen;
        cb.failure_count = FAILURE_THRESHOLD;
        cb.record_failure(&DownloadErrorKind::Network { retryable: true }, now());
        assert_eq!(cb.state, CircuitState::Open);
        assert!(cb.is_open_at(now()));
    }

    #[test]
    fn success_resets_failure_count() {
        let mut cb = CircuitBreaker::new();
        let kind = DownloadErrorKind::Network { retryable: true };
        for _ in 0..3 {
            cb.record_failure(&kind, now());
        }
        cb.record_success();
        assert_eq!(cb.failure_count, 0);
        assert_eq!(cb.state, CircuitState::Closed);
    }
}
