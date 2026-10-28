//! Bounded process-local cache of accepted messages; no persistent replay.
use super::*;
use crate::{
    application::ports::{
        PortFuture,
        recent_messages::{RecentMessages, validate_limit},
    },
    domain::{
        event::{EventEnvelope, EventPayload},
        recent_messages::{ConversationKey, RecentMessage, RecentPage},
    },
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex as StdMutex, OnceLock, Weak};
use std::time::Duration;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_ITEM_BYTES: usize = 8192;
const MAX_PER_CONVERSATION: usize = 100;
const MAX_TOTAL_ITEMS: usize = 2000;
#[cfg(test)]
const MAX_AGE: Duration = Duration::from_secs(600);
use crate::infrastructure::connectors::message_config::RecentMessagesConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentSource {
    Qq,
    Mqtt,
    Webhook,
}

struct Entry {
    key: ConversationKey,
    value: RecentMessage,
    bytes: usize,
    inserted: Instant,
    ttl: Duration,
}
struct Cache {
    generation: Uuid,
    entries: VecDeque<Entry>,
    bytes: usize,
    sequence: u64,
    policies:
        HashMap<crate::domain::connector::ConnectionScope, (RecentMessagesConfig, RecentSource)>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            generation: Uuid::new_v4(),
            entries: VecDeque::new(),
            bytes: 0,
            sequence: 0,
            policies: HashMap::new(),
        }
    }
}
#[derive(Default)]
pub(super) struct RecentCache(StdMutex<Cache>);

pub(super) fn shared(path: &Path) -> Arc<RecentCache> {
    static CACHES: OnceLock<StdMutex<HashMap<PathBuf, Weak<RecentCache>>>> = OnceLock::new();
    let mut caches = CACHES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    caches.retain(|_, value| value.strong_count() > 0);
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(cache) = caches.get(&key).and_then(Weak::upgrade) {
        return cache;
    }
    let cache = Arc::new(RecentCache::default());
    caches.insert(key, Arc::downgrade(&cache));
    // A weak task does not extend the cache's lifetime. Sweep idle conversations as
    // well as those being queried; memory may remain for at most one sweep interval.
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let weak = Arc::downgrade(&cache);
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(cache) = weak.upgrade() else { break };
                cache.0.lock().unwrap_or_else(|e| e.into_inner()).expire();
            }
        });
    }
    cache
}
impl Cache {
    fn remove(&mut self, index: usize) {
        if let Some(entry) = self.entries.remove(index) {
            self.bytes -= entry.bytes;
        }
    }
    fn expire(&mut self) {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].inserted.elapsed() >= self.entries[i].ttl {
                self.remove(i);
            } else {
                i += 1;
            }
        }
    }
}
impl RecentCache {
    pub(super) fn observe(&self, event: &EventEnvelope) {
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some((policy, source)) = c
            .policies
            .get(&event.scope)
            .filter(|(p, _)| p.enabled)
            .cloned()
        else {
            return;
        };
        let EventPayload::Json(payload) = &event.payload else {
            return;
        };
        let (account, kind, id, sender, message_id, text) = match source {
            RecentSource::Qq => {
                let Some(binding) = event
                    .route_context
                    .as_ref()
                    .filter(|b| b.adapter == "qq" && b.version == 1)
                else {
                    return;
                };
                let Some((account, kind, id, sender, message_id, text)) = (|| {
                    Some((
                        binding.config["app_id"].as_str()?,
                        binding.config["kind"].as_str()?,
                        binding.config["id"].as_str()?,
                        binding.config["sender_openid"].as_str()?,
                        event.source_event_id.as_deref()?,
                        payload["content"].as_str()?,
                    ))
                })() else {
                    return;
                };
                if !matches!(kind, "group" | "direct")
                    || text.len() > MAX_ITEM_BYTES
                    || [account, id, sender, message_id]
                        .iter()
                        .any(|v| v.is_empty() || v.len() > 256)
                {
                    return;
                }
                (
                    account.to_owned(),
                    kind.to_owned(),
                    id.to_owned(),
                    sender.to_owned(),
                    message_id.to_owned(),
                    text.to_owned(),
                )
            }
            RecentSource::Mqtt | RecentSource::Webhook => {
                if event.state_report {
                    return;
                }
                let binary = |v: &serde_json::Value| {
                    v.get("encoding").and_then(|x| x.as_str()) == Some("base64")
                        && v.get("data").is_some()
                };
                if matches!(source, RecentSource::Mqtt)
                    && (binary(payload) || payload.get("payload").is_some_and(binary))
                {
                    return;
                }
                let topic = match source {
                    RecentSource::Mqtt => match event
                        .route_context
                        .as_ref()
                        .filter(|b| b.adapter == "mqtt" && b.version == 1)
                        .and_then(|b| b.config["topic"].as_str())
                    {
                        Some(topic) if !topic.is_empty() && topic.len() <= 512 => Some(topic),
                        _ => return,
                    },
                    _ => None,
                };
                #[derive(serde::Serialize)]
                struct StreamRecord<'a> {
                    event_type: &'a str,
                    topic: Option<&'a str>,
                    payload: &'a serde_json::Value,
                }
                struct Limited(Vec<u8>);
                impl std::io::Write for Limited {
                    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                        if bytes.len() > MAX_ITEM_BYTES.saturating_sub(self.0.len()) {
                            return Err(std::io::Error::other("cache item too large"));
                        }
                        self.0.extend_from_slice(bytes);
                        Ok(bytes.len())
                    }
                    fn flush(&mut self) -> std::io::Result<()> {
                        Ok(())
                    }
                }
                let mut out = Limited(Vec::new());
                if serde_json::to_writer(
                    &mut out,
                    &StreamRecord {
                        event_type: &event.event_type,
                        topic,
                        payload,
                    },
                )
                .is_err()
                {
                    return;
                }
                let Ok(text) = String::from_utf8(out.0) else {
                    return;
                };
                (
                    "connection".into(),
                    "stream".into(),
                    "default".into(),
                    String::new(),
                    event.source_event_id.clone().unwrap_or_default(),
                    text,
                )
            }
        };
        if text.len() > MAX_ITEM_BYTES {
            return;
        }
        let Ok(received) = event.received_at.duration_since(UNIX_EPOCH) else {
            return;
        };
        let Ok(received_at_ms) = u64::try_from(received.as_millis()) else {
            return;
        };
        let key = ConversationKey {
            scope: event.scope,
            account: account.clone(),
            kind: kind.clone(),
            id: id.clone(),
        };
        let group_name = if matches!(source, RecentSource::Qq) && kind == "group" {
            payload["group_name"]
                .as_str()
                .filter(|s| {
                    !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
                })
                .map(str::to_owned)
        } else {
            None
        };
        let bytes = group_name.as_ref().map_or(0, String::len)
            + text.len()
            + account.len()
            + kind.len()
            + id.len()
            + sender.len()
            + message_id.len()
            + 256;
        if bytes > MAX_BYTES {
            return;
        }
        c.expire();
        while c
            .entries
            .iter()
            .filter(|entry| entry.key.scope == event.scope)
            .count()
            >= policy.max_items as usize
        {
            if let Some(pos) = c
                .entries
                .iter()
                .position(|entry| entry.key.scope == event.scope)
            {
                c.remove(pos);
            }
        }
        while c.entries.iter().filter(|entry| entry.key == key).count() >= MAX_PER_CONVERSATION {
            if let Some(pos) = c.entries.iter().position(|entry| entry.key == key) {
                c.remove(pos);
            }
        }
        while c.bytes + bytes > MAX_BYTES || c.entries.len() >= MAX_TOTAL_ITEMS {
            c.remove(0);
        }
        c.sequence = c.sequence.saturating_add(1);
        let sequence = c.sequence;
        c.bytes += bytes;
        c.entries.push_back(Entry {
            key,
            value: RecentMessage {
                sequence,
                event_id: event.event_id,
                source_message_id: message_id,
                sender,
                received_at_ms,
                text,
                group_name,
            },
            bytes,
            inserted: Instant::now(),
            ttl: Duration::from_secs(u64::from(policy.ttl_seconds)),
        });
    }
    fn read(
        &self,
        key: &ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> Result<RecentPage, PortError> {
        validate_limit(limit)?;
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        c.expire();
        let mut found = c
            .entries
            .iter()
            .rev()
            .filter(|entry| &entry.key == key && before.is_none_or(|n| entry.value.sequence < n))
            .take(limit + 1);
        let messages: Vec<_> = found
            .by_ref()
            .take(limit)
            .map(|entry| entry.value.clone())
            .collect();
        let next_before = found
            .next()
            .and_then(|_| messages.last().map(|value| value.sequence));
        Ok(RecentPage {
            generation: c.generation,
            messages,
            next_before,
            history_complete: false,
        })
    }
}
impl SqliteStore {
    pub(crate) fn configure_recent(
        &self,
        scope: crate::domain::connector::ConnectionScope,
        policy: Option<(RecentMessagesConfig, RecentSource)>,
    ) {
        let mut cache = self.recent.0.lock().unwrap_or_else(|e| e.into_inner());
        if cache.policies.get(&scope) != policy.as_ref() {
            let mut i = 0;
            while i < cache.entries.len() {
                if cache.entries[i].key.scope == scope {
                    cache.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        if let Some(policy) = policy.filter(|(p, _)| p.enabled) {
            cache.policies.insert(scope, policy);
        } else {
            cache.policies.remove(&scope);
        }
    }
    pub(crate) fn clear_recent(&self, scope: crate::domain::connector::ConnectionScope) {
        let mut cache = self.recent.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut i = 0;
        while i < cache.entries.len() {
            if cache.entries[i].key.scope == scope {
                cache.remove(i);
            } else {
                i += 1;
            }
        }
    }
}
impl RecentMessages for SqliteStore {
    fn read_recent(
        &self,
        conversation: ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> PortFuture<'_, RecentPage> {
        Box::pin(async move { self.recent.read(&conversation, before, limit) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{connection_routing::RouteBinding, connector::ConnectionScope};
    use serde_json::json;

    fn message(scope: ConnectionScope, conversation: &str, content: &str) -> EventEnvelope {
        EventEnvelope {
            state_report: false,
            scope,
            event_id: Uuid::new_v4(),
            source_event_id: Some(Uuid::new_v4().to_string()),
            event_type: "qq.group.message".into(),
            schema_version: 1,
            occurred_at: None,
            received_at: SystemTime::now(),
            correlation_id: Uuid::new_v4(),
            causation_id: None,
            route_trace: vec![],
            route_context: Some(RouteBinding::new(
                "qq",
                json!({"app_id":"bot","kind":"group","id":conversation,"sender_openid":"sender"}),
            )),
            payload: EventPayload::Json(
                json!({"content":content,"attachments":[{"url":"https://example.invalid/secret"}]}),
            ),
        }
    }
    fn key(scope: ConnectionScope, id: &str) -> ConversationKey {
        ConversationKey {
            scope,
            account: "bot".into(),
            kind: "group".into(),
            id: id.into(),
        }
    }
    #[test]
    fn conversation_scope_pagination_and_size_are_bounded() {
        let cache = RecentCache::default();
        let scope = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        let other = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: scope.connection_id,
        };
        {
            let mut c = cache.0.lock().unwrap();
            let policy = RecentMessagesConfig {
                enabled: true,
                max_items: 1000,
                ttl_seconds: 600,
            };
            c.policies.insert(scope, (policy.clone(), RecentSource::Qq));
            c.policies.insert(other, (policy, RecentSource::Qq));
        }
        for n in 0..105 {
            cache.observe(&message(scope, "one", &format!("message {n}")));
        }
        cache.observe(&message(scope, "two", "private"));
        cache.observe(&message(other, "one", "other workspace"));
        cache.observe(&message(scope, "one", &"x".repeat(MAX_ITEM_BYTES + 1)));
        let first = cache.read(&key(scope, "one"), None, 50).unwrap();
        assert_eq!(first.messages.len(), 50);
        assert_eq!(first.messages[0].text, "message 104");
        assert!(!first.history_complete);
        let second = cache
            .read(&key(scope, "one"), first.next_before, 50)
            .unwrap();
        assert_eq!(second.messages.len(), 50);
        assert_eq!(second.messages.last().unwrap().text, "message 5");
        assert!(second.next_before.is_none());
        assert_eq!(
            cache.read(&key(other, "one"), None, 50).unwrap().messages[0].text,
            "other workspace"
        );
        assert!(cache.read(&key(scope, "one"), None, 51).is_err());
        assert!(
            !serde_json::to_string(&first)
                .unwrap()
                .contains("example.invalid")
        );
    }
    #[tokio::test]
    async fn accepted_events_are_cached_once_and_shared_by_database() {
        use crate::application::ports::events::EventStore;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.db");
        let first = SqliteStore::open(&path).await.unwrap();
        let second = SqliteStore::open(&path).await.unwrap();
        let scope = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        first.configure_recent(
            scope,
            Some((
                RecentMessagesConfig {
                    enabled: true,
                    ..Default::default()
                },
                RecentSource::Qq,
            )),
        );
        let event = message(scope, "one", "hello");
        first.append(event.clone()).await.unwrap();
        first.append(event).await.unwrap();
        let page = second
            .read_recent(key(scope, "one"), None, 50)
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].text, "hello");
        first.clear_recent(scope);
        assert!(
            second
                .read_recent(key(scope, "one"), None, 50)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
        first.append(message(scope, "one", "again")).await.unwrap();
        drop(first);
        drop(second);
        let restarted = SqliteStore::open(&path).await.unwrap();
        assert!(
            restarted
                .read_recent(key(scope, "one"), None, 50)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
    }
    #[tokio::test]
    async fn policy_is_opt_in_bounded_and_clears_on_change() {
        use crate::application::ports::events::EventStore;
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("policies.db"))
            .await
            .unwrap();
        let scope = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        store
            .append(message(scope, "one", "disabled"))
            .await
            .unwrap();
        assert!(
            store
                .read_recent(key(scope, "one"), None, 10)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
        store.configure_recent(
            scope,
            Some((
                RecentMessagesConfig {
                    enabled: true,
                    max_items: 2,
                    ttl_seconds: 1,
                },
                RecentSource::Qq,
            )),
        );
        for value in ["first", "second", "third"] {
            store.append(message(scope, "one", value)).await.unwrap();
        }
        let page = store
            .read_recent(key(scope, "one"), None, 10)
            .await
            .unwrap();
        assert_eq!(
            page.messages
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>(),
            vec!["third", "second"]
        );
        {
            let mut cache = store.recent.0.lock().unwrap();
            cache.entries.front_mut().unwrap().inserted = Instant::now() - Duration::from_secs(1);
        }
        assert_eq!(
            store
                .read_recent(key(scope, "one"), None, 10)
                .await
                .unwrap()
                .messages
                .len(),
            1
        );
        store.configure_recent(
            scope,
            Some((
                RecentMessagesConfig {
                    enabled: false,
                    max_items: 2,
                    ttl_seconds: 1,
                },
                RecentSource::Qq,
            )),
        );
        store
            .append(message(scope, "one", "disabled again"))
            .await
            .unwrap();
        assert!(
            store
                .read_recent(key(scope, "one"), None, 10)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
    }
    #[tokio::test]
    async fn mqtt_and_webhook_streams_are_opt_in_and_exclude_state() {
        use crate::application::ports::events::EventStore;
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("streams.db"))
            .await
            .unwrap();
        let workspace = Uuid::new_v4();
        let mqtt = ConnectionScope {
            workspace_id: workspace,
            connection_id: Uuid::new_v4(),
        };
        let webhook = ConnectionScope {
            workspace_id: workspace,
            connection_id: Uuid::new_v4(),
        };
        let config = RecentMessagesConfig {
            enabled: true,
            max_items: 10,
            ttl_seconds: 60,
        };
        store.configure_recent(mqtt, Some((config.clone(), RecentSource::Mqtt)));
        store.configure_recent(webhook, Some((config, RecentSource::Webhook)));
        let mut event = message(mqtt, "ignored", "ignored");
        event.source_event_id = None;
        event.route_context = Some(RouteBinding::new(
            "mqtt",
            json!({"topic":"home/switch/pub"}),
        ));
        event.payload = EventPayload::Json(json!({"value": 42}));
        store.append(event).await.unwrap();
        let stream = |scope| ConversationKey {
            scope,
            account: "connection".into(),
            kind: "stream".into(),
            id: "default".into(),
        };
        let page = store.read_recent(stream(mqtt), None, 10).await.unwrap();
        let data: serde_json::Value = serde_json::from_str(&page.messages[0].text).unwrap();
        assert_eq!(data["topic"], "home/switch/pub");
        assert_eq!(data["payload"]["value"], 42);
        let mut state = message(mqtt, "ignored", "ignored");
        state.route_context = Some(RouteBinding::new(
            "mqtt",
            json!({"topic":"home/switch/pub"}),
        ));
        state.state_report = true;
        store.recent.observe(&state);
        assert_eq!(
            store
                .read_recent(stream(mqtt), None, 10)
                .await
                .unwrap()
                .messages
                .len(),
            1
        );
        let mut incoming = message(webhook, "ignored", "ignored");
        incoming.route_context = None;
        incoming.payload = EventPayload::Json(json!({"hello":"world"}));
        store.append(incoming).await.unwrap();
        let page = store.read_recent(stream(webhook), None, 10).await.unwrap();
        assert!(page.messages[0].text.contains("world"));
        store.configure_recent(webhook, None);
        assert!(
            store
                .read_recent(stream(webhook), None, 10)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
    }
    #[test]
    fn ttl_and_global_capacity_evict_oldest() {
        let cache = RecentCache::default();
        let scope = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        cache.0.lock().unwrap().policies.insert(
            scope,
            (
                RecentMessagesConfig {
                    enabled: true,
                    max_items: 1000,
                    ttl_seconds: 600,
                },
                RecentSource::Qq,
            ),
        );
        cache.observe(&message(scope, "one", "old"));
        {
            let mut inner = cache.0.lock().unwrap();
            inner.entries.front_mut().unwrap().inserted = Instant::now() - MAX_AGE;
        }
        assert!(
            cache
                .read(&key(scope, "one"), None, 1)
                .unwrap()
                .messages
                .is_empty()
        );
        for i in 0..MAX_TOTAL_ITEMS + 10 {
            cache.observe(&message(scope, &format!("group-{i}"), "x"));
        }
        let inner = cache.0.lock().unwrap();
        assert_eq!(inner.entries.len(), 1000);
        assert!(inner.bytes <= MAX_BYTES);
        assert_eq!(
            inner.entries.front().unwrap().key.id,
            format!("group-{}", MAX_TOTAL_ITEMS + 10 - 1000)
        );
    }
}
