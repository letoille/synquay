use std::time::Duration;
use thiserror::Error;

/// Bounded, safe errors: do not embed upstream response bodies, arguments or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation deadline exceeded")]
    DeadlineExceeded,
    #[error("connection scope mismatch")]
    ScopeMismatch,
    #[error("version or state conflict")]
    Conflict,
    #[error("permission denied")]
    PermissionDenied,
    #[error("invalid input: {code}")]
    InvalidInput { code: &'static str },
    #[error("capability is not supported")]
    Unsupported,
    #[error("connection not found")]
    NotFound,
    #[error("connection already registered")]
    AlreadyRegistered,
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("backlog capacity unavailable: {reason}")]
    CapacityExceeded { reason: &'static str },
    #[error("record exceeds the supported size")]
    PayloadTooLarge,
    #[error("temporarily unavailable")]
    Unavailable,
    /// Adapter guarantees the business request was not sent. Safe to retry delivery.
    #[error("message not sent; connection unavailable")]
    NotSent { retry_after: Option<Duration> },
    #[error("external side effect has an unknown outcome")]
    OutcomeUnknown,
    #[error("adapter failed: {code}")]
    Failed { code: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAdvice {
    Never,
    /// Only if the operation is replay-safe or the upstream honors its idempotency key.
    IfIdempotent {
        after: Option<Duration>,
    },
    Reconcile,
}

impl PortError {
    pub fn retry_advice(&self) -> RetryAdvice {
        match self {
            Self::RateLimited { retry_after } => RetryAdvice::IfIdempotent {
                after: Some(*retry_after),
            },
            Self::Unavailable => RetryAdvice::IfIdempotent { after: None },
            Self::CapacityExceeded { .. } => RetryAdvice::IfIdempotent {
                after: Some(Duration::from_secs(10)),
            },
            Self::OutcomeUnknown | Self::DeadlineExceeded => RetryAdvice::Reconcile,
            _ => RetryAdvice::Never,
        }
    }
}
