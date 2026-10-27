//! Shared bounded retry timing. Jitter spreads reconnects after a broker outage.
use std::time::Duration;
use uuid::Uuid;

/// Equal jitter: [half ceiling, ceiling], starting at 1–2 seconds, capped at 60.
/// A provider's minimum delay is never shortened by jitter or the local cap.
pub fn delay(attempt: u32, minimum: Option<Duration>) -> Duration {
    let ceiling = (2_000u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(6))).min(60_000);
    let entropy = Uuid::new_v4().as_u128() as u64;
    jitter(ceiling, entropy).max(minimum.unwrap_or_default())
}
fn jitter(ceiling_ms: u64, entropy: u64) -> Duration {
    let floor = ceiling_ms / 2;
    Duration::from_millis(floor + entropy % (ceiling_ms - floor + 1))
}

/// Only a sustained ready session resets exponential backoff; short flaps do not.
#[derive(Default)]
pub struct ReconnectBackoff {
    failures: u32,
}
impl ReconnectBackoff {
    pub fn disconnected(&mut self, ready_for: Option<Duration>) -> Duration {
        if ready_for.is_some_and(|d| d >= Duration::from_secs(60)) {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        delay(self.failures, None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_bounds_provider_floor_and_flap_reset() {
        for attempt in [1, 2, 3, 8, u32::MAX] {
            let upper = (2u64 << attempt.saturating_sub(1).min(6)).min(60);
            for _ in 0..30 {
                let d = delay(attempt, None);
                assert!(d >= Duration::from_secs(upper / 2) && d <= Duration::from_secs(upper));
            }
        }
        assert!(delay(1, Some(Duration::from_secs(120))) >= Duration::from_secs(120));
        assert_eq!(jitter(2000, 0), Duration::from_secs(1));
        assert_eq!(jitter(2000, 1000), Duration::from_secs(2));
        let mut b = ReconnectBackoff::default();
        b.disconnected(None);
        b.disconnected(Some(Duration::from_secs(1)));
        assert_eq!(b.failures, 2);
        for _ in 0..100 {
            assert!(b.disconnected(None) <= Duration::from_secs(60));
        }
        assert!(b.disconnected(Some(Duration::from_secs(60))) <= Duration::from_secs(2));
        assert_eq!(b.failures, 1);
    }
}
