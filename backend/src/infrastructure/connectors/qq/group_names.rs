//! Bounded in-memory group-name cache; a failed lookup never rejects a callback.
use super::{QqBotConfig, QqBotSink};
use crate::{application::ports::PortError, domain::connector::ConnectionScope};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use uuid::Uuid;

const TTL: Duration = Duration::from_secs(48 * 60 * 60);
const RETRY: Duration = Duration::from_secs(60);
const CAPACITY: usize = 1024;
const TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Default)]
struct Entry {
    value: Option<String>,
    expires: Option<Instant>,
}
struct CacheEntry {
    state: Arc<AsyncMutex<Entry>>,
    touched: Instant,
}
pub(crate) struct GroupNames {
    entries: Mutex<HashMap<(Uuid, String), CacheEntry>>,
    slots: Semaphore,
}
impl Default for GroupNames {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            slots: Semaphore::new(8),
        }
    }
}
impl GroupNames {
    #[cfg(test)]
    pub(crate) async fn seed_for_test(&self, id: Uuid, group: &str, name: &str) {
        let _ = self
            .lookup(id, group, || async { Ok(name.to_owned()) })
            .await;
    }
    pub(crate) fn clear_connection(&self, id: Uuid) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(connection, _), _| *connection != id);
    }
    pub(crate) async fn get(
        &self,
        workspace: Uuid,
        connection: Uuid,
        config: &QqBotConfig,
        group: &str,
    ) -> Option<String> {
        self.lookup(connection, group, || async {
            let sink = QqBotSink::new(
                ConnectionScope {
                    workspace_id: workspace,
                    connection_id: connection,
                },
                connection,
                config,
            )?;
            sink.group_name(group).await
        })
        .await
    }
    async fn lookup<F, Fut>(&self, connection: Uuid, group: &str, fetch: F) -> Option<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, PortError>>,
    {
        let entry = {
            let mut cache = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let key = (connection, group.to_owned());
            if !cache.contains_key(&key) && cache.len() >= CAPACITY {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.touched)
                    .map(|(key, _)| key.clone())
                {
                    cache.remove(&oldest);
                }
            }
            let value = cache.entry(key).or_insert_with(|| CacheEntry {
                state: Arc::new(AsyncMutex::new(Entry::default())),
                touched: Instant::now(),
            });
            value.touched = Instant::now();
            value.state.clone()
        };
        let mut state = entry.lock().await;
        if state
            .expires
            .is_some_and(|expires| expires > Instant::now())
        {
            tracing::debug!(
                event = "qq.group_name_cache_hit",
                has_name = state.value.is_some()
            );
            return state.value.clone();
        }
        let result = match self.slots.try_acquire() {
            Ok(_permit) => match tokio::time::timeout(TIMEOUT, fetch()).await {
                Ok(result) => result,
                Err(_) => Err(PortError::DeadlineExceeded),
            },
            Err(_) => Err(PortError::RateLimited { retry_after: RETRY }),
        };
        match result {
            Ok(value) => {
                tracing::debug!(event = "qq.group_name_lookup_succeeded");
                state.expires = Some(Instant::now() + TTL);
                state.value = Some(value);
            }
            Err(error) => {
                let reason = match error {
                    PortError::PermissionDenied => "permission_denied",
                    PortError::RateLimited { .. } => "rate_limited",
                    PortError::DeadlineExceeded => "timeout",
                    _ => "unavailable",
                };
                tracing::warn!(
                    event = "qq.group_name_lookup_failed",
                    reason,
                    stale_available = state.value.is_some()
                );
                state.expires = Some(Instant::now() + RETRY);
            }
        }
        state.value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[tokio::test]
    async fn caches_success_for_48_hours_and_isolates_connections() {
        let cache = GroupNames::default();
        let calls = AtomicUsize::new(0);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("测试群".to_owned())
        };
        assert_eq!(
            cache.lookup(a, "group", fetch).await.as_deref(),
            Some("测试群")
        );
        assert_eq!(
            cache.lookup(a, "group", fetch).await.as_deref(),
            Some("测试群")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache.lookup(b, "group", fetch).await.as_deref(),
            Some("测试群")
        );
        let entry = cache
            .entries
            .lock()
            .unwrap()
            .get(&(a, "group".to_owned()))
            .unwrap()
            .state
            .clone();
        entry.lock().await.expires = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(
            cache.lookup(a, "group", fetch).await.as_deref(),
            Some("测试群")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        cache.clear_connection(a);
        assert_eq!(
            cache.lookup(a, "group", fetch).await.as_deref(),
            Some("测试群")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        let entry = cache
            .entries
            .lock()
            .unwrap()
            .get(&(a, "group".to_owned()))
            .unwrap()
            .state
            .clone();
        entry.lock().await.expires = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(
            cache
                .lookup(a, "group", || async { Err(PortError::Unavailable) })
                .await
                .as_deref(),
            Some("测试群")
        );
        assert_eq!(TTL, Duration::from_secs(48 * 3600));
    }
    #[tokio::test]
    async fn concurrent_misses_share_one_request_and_failures_back_off() {
        let cache = GroupNames::default();
        let calls = AtomicUsize::new(0);
        let id = Uuid::new_v4();
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Err(PortError::Unavailable)
        };
        let (a, b) = tokio::join!(cache.lookup(id, "g", fetch), cache.lookup(id, "g", fetch));
        assert!(a.is_none() && b.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cache.lookup(id, "g", fetch).await.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
