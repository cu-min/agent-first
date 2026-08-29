use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(Default)]
pub(crate) struct RateLimiter {
    entries: Mutex<HashMap<String, RateWindow>>,
}

struct RateWindow {
    opened_at: Instant,
    count: u32,
}

impl RateLimiter {
    // ponytail: 单进程限流；扩容为多个实例时改由网关或共享存储限流。
    pub(crate) async fn allow(&self, key: String, maximum: u32, window: Duration) -> bool {
        let mut entries = self.entries.lock().await;
        if entries.len() >= 4096 {
            // ponytail: 单进程内存保护；公网多实例时由网关按 IP/密钥限流。
            entries.retain(|_, item| item.opened_at.elapsed() < Duration::from_secs(3600));
        }
        if !entries.contains_key(&key) && entries.len() >= 8192 {
            return false;
        }
        let entry = entries.entry(key).or_insert(RateWindow {
            opened_at: Instant::now(),
            count: 0,
        });
        if entry.opened_at.elapsed() >= window {
            entry.opened_at = Instant::now();
            entry.count = 0;
        }
        entry.count += 1;
        entry.count <= maximum
    }
}

pub(crate) async fn ensure_rate(
    state: &AppState,
    key: String,
    maximum: u32,
    window: Duration,
) -> ApiResult<()> {
    if state.limiter.allow(key, maximum, window).await {
        Ok(())
    } else {
        Err(ApiError::rate_limited())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_rejects_after_the_configured_limit() {
        let limiter = RateLimiter::default();
        assert!(
            limiter
                .allow("test-agent".to_owned(), 1, Duration::from_secs(60))
                .await
        );
        assert!(
            !limiter
                .allow("test-agent".to_owned(), 1, Duration::from_secs(60))
                .await
        );
    }

    #[tokio::test]
    async fn distinct_keys_do_not_interfere() {
        let limiter = RateLimiter::default();
        for round in 0..3 {
            assert!(
                limiter
                    .allow(format!("key-{round}"), 1, Duration::from_secs(60))
                    .await
            );
        }
    }

    #[tokio::test]
    async fn exhausted_window_resets_after_expiry() {
        let limiter = RateLimiter::default();
        let long = Duration::from_secs(60);
        assert!(limiter.allow("k".to_owned(), 1, long).await);
        assert!(!limiter.allow("k".to_owned(), 1, long).await);
        // ZERO 窗口下 elapsed >= window 恒成立，等价于"窗口已过期"：配额重置
        assert!(limiter.allow("k".to_owned(), 1, Duration::ZERO).await);
    }
}
