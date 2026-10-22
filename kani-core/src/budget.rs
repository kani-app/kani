//! The work one top-level operation may cause: requests, response bytes and elapsed time,
//! shared by pagination, sub-fetches, hook retries, auth refreshes and browser captures.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Leads every budget error, so no `on_failure` policy can mistake it for a failed fetch.
pub const BUDGET_EXCEEDED: &str = "budget exceeded:";

/// Whether an evaluator error is an exhausted operation budget.
pub fn is_budget_exceeded(error: &str) -> bool {
    error.contains(BUDGET_EXCEEDED)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationLimits {
    pub max_requests: u32,
    pub max_response_bytes: u64,
    pub max_elapsed: Duration,
}

impl OperationLimits {
    /// The most an extension may declare in `metadata.rate_limit`.
    pub const HARD: Self = Self {
        max_requests: kani_shared::extension::MAX_OPERATION_REQUESTS,
        max_response_bytes: kani_shared::extension::MAX_OPERATION_RESPONSE_BYTES,
        max_elapsed: Duration::from_secs(kani_shared::extension::MAX_OPERATION_SECONDS),
    };

    /// The limits an extension's rate-limit block declares.
    pub fn from_rate_limit(cfg: Option<&kani_shared::extension::RateLimitConfig>) -> Self {
        Self::declared(
            cfg.and_then(|c| c.max_requests),
            cfg.and_then(|c| c.max_response_bytes),
            cfg.and_then(|c| c.max_operation_seconds),
        )
    }

    /// The defaults with any declared limit applied, each clamped to [`Self::HARD`].
    pub fn declared(requests: Option<u32>, bytes: Option<u64>, secs: Option<u64>) -> Self {
        let d = Self::default();
        Self {
            max_requests: requests
                .unwrap_or(d.max_requests)
                .min(Self::HARD.max_requests),
            max_response_bytes: bytes
                .unwrap_or(d.max_response_bytes)
                .min(Self::HARD.max_response_bytes),
            max_elapsed: secs
                .map(Duration::from_secs)
                .unwrap_or(d.max_elapsed)
                .min(Self::HARD.max_elapsed),
        }
    }
}

impl Default for OperationLimits {
    fn default() -> Self {
        Self {
            max_requests: 128,
            max_response_bytes: 64 * 1024 * 1024,
            max_elapsed: Duration::from_secs(120),
        }
    }
}

/// Per-source limits a host instance enforces: hook retries per request and the operation
/// budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLimits {
    pub max_hook_requests: u32,
    pub operation: OperationLimits,
}

impl SourceLimits {
    pub fn from_rate_limit(cfg: Option<&kani_shared::extension::RateLimitConfig>) -> Self {
        Self {
            max_hook_requests: cfg.map_or(3, |c| c.max_hook_requests),
            operation: OperationLimits::from_rate_limit(cfg),
        }
    }
}

impl From<u32> for SourceLimits {
    fn from(max_hook_requests: u32) -> Self {
        Self {
            max_hook_requests,
            operation: OperationLimits::default(),
        }
    }
}

#[derive(Debug)]
pub struct OperationBudget {
    limits: OperationLimits,
    started: Instant,
    requests: AtomicU32,
    bytes: AtomicU64,
}

impl OperationBudget {
    pub fn new(limits: OperationLimits) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            limits,
            started: Instant::now(),
            requests: AtomicU32::new(0),
            bytes: AtomicU64::new(0),
        })
    }

    pub fn limits(&self) -> OperationLimits {
        self.limits
    }

    pub fn requests(&self) -> u32 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn charge_request(&self) -> Result<(), String> {
        self.check_time()?;
        let used = self.requests.fetch_add(1, Ordering::Relaxed) + 1;
        if used > self.limits.max_requests {
            return Err(format!(
                "{BUDGET_EXCEEDED} requests: this operation may make {} requests",
                self.limits.max_requests
            ));
        }
        Ok(())
    }

    pub fn charge_bytes(&self, len: usize) -> Result<(), String> {
        let used = self.bytes.fetch_add(len as u64, Ordering::Relaxed) + len as u64;
        if used > self.limits.max_response_bytes {
            return Err(format!(
                "{BUDGET_EXCEEDED} response bytes: this operation may read {} bytes",
                self.limits.max_response_bytes
            ));
        }
        self.check_time()
    }

    pub fn check_time(&self) -> Result<(), String> {
        if self.started.elapsed() > self.limits.max_elapsed {
            return Err(format!(
                "{BUDGET_EXCEEDED} time: this operation may run for {}s",
                self.limits.max_elapsed.as_secs()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn each_dimension_is_enforced_and_reported_as_a_budget_error() {
        let limits = OperationLimits {
            max_requests: 2,
            max_response_bytes: 10,
            max_elapsed: Duration::from_secs(60),
        };
        let b = OperationBudget::new(limits);
        assert!(b.charge_request().is_ok() && b.charge_request().is_ok());
        let err = b.charge_request().unwrap_err();
        assert!(
            is_budget_exceeded(&err) && err.contains("requests"),
            "{err}"
        );

        let b = OperationBudget::new(limits);
        assert!(b.charge_bytes(10).is_ok());
        let err = b.charge_bytes(1).unwrap_err();
        assert!(
            is_budget_exceeded(&err) && err.contains("response bytes"),
            "{err}"
        );

        let b = OperationBudget::new(OperationLimits {
            max_elapsed: Duration::ZERO,
            ..limits
        });
        std::thread::sleep(Duration::from_millis(2));
        let err = b.charge_request().unwrap_err();
        assert!(is_budget_exceeded(&err) && err.contains("time"), "{err}");
    }

    #[test]
    fn declared_limits_are_clamped_to_the_hard_caps() {
        let l = OperationLimits::declared(Some(5000), Some(u64::MAX), Some(10_000));
        assert_eq!(l, OperationLimits::HARD);
        assert_eq!(
            OperationLimits::declared(None, None, None),
            OperationLimits::default()
        );
    }
}
