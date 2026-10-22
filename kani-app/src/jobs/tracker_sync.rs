use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

/// Default per-token spacing; provider `Retry-After` extends it dynamically.
pub(crate) const MIN_TOKEN_SPACING: Duration = Duration::from_millis(700);

/// Maximum number of stale entries processed in a single run, to bound work.
pub(crate) const MAX_ENTRIES_PER_RUN: usize = 200;

/// Per-token call throttle: enforces a minimum spacing between calls and honours a
/// rate-limit backoff window (driven by HTTP 429 `Retry-After`). Pure and clock-injected
/// so it can be unit-tested without sleeping.
#[derive(Default)]
pub(crate) struct TokenThrottle {
    last_call: HashMap<i64, Instant>,
    backoff_until: HashMap<i64, Instant>,
}

impl TokenThrottle {
    /// How long to wait before the next call for `token_key`, given `now` and the minimum
    /// spacing. Returns the larger of the spacing gap and any active backoff window.
    pub(crate) fn delay_before(
        &self,
        token_key: i64,
        now: Instant,
        min_spacing: Duration,
    ) -> Duration {
        let mut wait = Duration::ZERO;
        if let Some(&last) = self.last_call.get(&token_key) {
            let next_allowed = last + min_spacing;
            if next_allowed > now {
                wait = next_allowed - now;
            }
        }
        if let Some(&until) = self.backoff_until.get(&token_key)
            && until > now
        {
            wait = wait.max(until - now);
        }
        wait
    }

    pub(crate) fn record_call(&mut self, token_key: i64, at: Instant) {
        self.last_call.insert(token_key, at);
    }

    /// Register a rate-limit backoff for `token_key` lasting `retry_after` from `at`.
    pub(crate) fn record_rate_limited(
        &mut self,
        token_key: i64,
        at: Instant,
        retry_after: Duration,
    ) {
        self.backoff_until.insert(token_key, at + retry_after);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct TrackerSyncJob {
    id: JobId,
}

impl TrackerSyncJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for TrackerSyncJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for TrackerSyncJob {
    const JOB_TYPE: &'static str = "tracker_sync";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Sync stale tracker entries".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        let interval_hours = svc.settings.read().await.tracker_sync_interval_hours;
        svc.sync_stale_trackers(
            interval_hours,
            MAX_ENTRIES_PER_RUN,
            MIN_TOKEN_SPACING,
            &ctx.cancel,
        )
        .await
        .map(|_| ())
        .map_err(|e| JobError::Internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_spaces_consecutive_calls() {
        let mut t = TokenThrottle::default();
        let now = Instant::now();
        assert_eq!(t.delay_before(1, now, MIN_TOKEN_SPACING), Duration::ZERO);
        t.record_call(1, now);
        let wait = t.delay_before(1, now, MIN_TOKEN_SPACING);
        assert_eq!(wait, MIN_TOKEN_SPACING);
    }

    #[test]
    fn throttle_backs_off_on_simulated_429() {
        let mut t = TokenThrottle::default();
        let now = Instant::now();
        t.record_rate_limited(7, now, Duration::from_secs(30));
        let wait = t.delay_before(7, now, MIN_TOKEN_SPACING);
        assert!(
            wait >= Duration::from_secs(30),
            "expected backoff >= 30s, got {wait:?}"
        );
    }

    #[test]
    fn throttle_is_per_token() {
        let mut t = TokenThrottle::default();
        let now = Instant::now();
        t.record_rate_limited(1, now, Duration::from_secs(60));
        assert!(t.delay_before(1, now, MIN_TOKEN_SPACING) >= Duration::from_secs(60));
        assert_eq!(t.delay_before(2, now, MIN_TOKEN_SPACING), Duration::ZERO);
    }
}
