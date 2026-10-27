//! Interpret Retry-After without retrying earlier than a provider allows.
use crate::application::ports::PortError;
use std::time::{Duration, SystemTime};

pub fn rate_limited(headers: &reqwest::header::HeaderMap) -> PortError {
    let retry_after = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| {
            v.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                httpdate::parse_http_date(v)
                    .ok()
                    .map(|t| t.duration_since(SystemTime::now()).unwrap_or_default())
            })
        })
        .unwrap_or(Duration::from_secs(60))
        .max(Duration::from_secs(1));
    // Do not overflow clocks or silently shorten a long provider delay.
    if retry_after > Duration::from_secs(86400) {
        PortError::Failed {
            code: "retry_after_exceeds_limit",
        }
    } else {
        PortError::RateLimited { retry_after }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_after_handles_seconds_dates_and_excessive_delays() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "120".parse().unwrap());
        assert_eq!(
            rate_limited(&h),
            PortError::RateLimited {
                retry_after: Duration::from_secs(120)
            }
        );
        h.insert(
            "retry-after",
            httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(120))
                .parse()
                .unwrap(),
        );
        assert!(
            matches!(rate_limited(&h), PortError::RateLimited { retry_after } if retry_after >= Duration::from_secs(118))
        );
        h.insert("retry-after", u64::MAX.to_string().parse().unwrap());
        assert!(matches!(
            rate_limited(&h),
            PortError::Failed {
                code: "retry_after_exceeds_limit"
            }
        ));
    }
}
