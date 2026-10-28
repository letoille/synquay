//! Optional single-route runtime. Real storage/HTTP adapters, explicitly configured scope.
mod bindings;
pub mod builtin_queries;
mod command_actions;
pub mod config;
mod connection_data;
mod management;
pub mod message_connections;
pub mod routes;
pub mod scheduler;
pub mod webhook;
use self::{bindings::Bindings, config::PipelineConfig};
use super::{connectors::http_sink::HttpSink, storage::SqliteStore};
use crate::{
    application::{
        connectors::{ConnectorRegistration, ConnectorRegistry, IngressService},
        ports::{
            OperationContext, PortError, events::EventIngress, identity::AccessContext,
            routing::EventReader,
        },
        services::{delivery::DeliveryService, governance::Governance, routing::RoutingService},
    },
    domain::{
        connector::{ConnectionScope, ConnectorDescriptor, ConnectorKind},
        delivery::{Destination, ReplaySafety},
        event::{IncomingEvent, IngestReceipt},
        identity::Identity,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Pipeline {
    connection_status: std::sync::RwLock<
        std::collections::HashMap<Uuid, (String, super::connectors::runtime_status::RuntimeStatus)>,
    >,
    pub connection_io: Arc<tokio::sync::RwLock<()>>,
    pub(crate) qq_group_names: super::connectors::qq::group_names::GroupNames,
    connections: Arc<std::sync::RwLock<message_connections::ConnectionSnapshot>>,
    registry: Arc<ConnectorRegistry>,
    commands: Arc<command_actions::CommandRuntime>,
    pub webhook: webhook::WebhookConfig,
    pub source: ConnectionScope,
    pub content: Option<Arc<super::content::FileContentStore>>,
    pub content_downloads: Arc<Semaphore>,
    store: Arc<SqliteStore>,
    retention: Option<super::storage::maintenance::RetentionPolicy>,
    ingresses: std::collections::HashMap<Uuid, IngressService>,
    pub sources: Vec<routes::RouteSource>,
    routing: RoutingService,
    bindings: Arc<Bindings>,
    route_endpoints: Vec<routes::RouteEndpoint>,
    delivery: DeliveryService,
    principal: Uuid,
    token: String,
    ingress_slots: Arc<Semaphore>,
    routing_lane: Mutex<()>,
    delivery_lanes: Vec<scheduler::TargetLane>,
    delivery_slots: Semaphore,
    scheduler: scheduler::SchedulerConfig,
    wake: tokio::sync::Notify,
    admin: Option<(Uuid, String)>,
    admin_slots: Arc<Semaphore>,
}
impl Pipeline {
    pub fn database_path(&self) -> &std::path::Path {
        self.store.database_path()
    }
    pub async fn build(
        config: PipelineConfig,
        webhook_token: String,
        destination_token: Option<String>,
    ) -> Result<Self, PortError> {
        config.validate()?;
        if webhook_token.len() < 32
            || webhook_token.len() > 4096
            || !webhook_token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(PortError::InvalidInput {
                code: "invalid_webhook_secret",
            });
        }
        let source = ConnectionScope {
            workspace_id: config.workspace_id,
            connection_id: config.source_connection_id,
        };
        let target = ConnectionScope {
            workspace_id: config.workspace_id,
            connection_id: config.target_connection_id,
        };
        let sink = HttpSink::new(
            target,
            config.destination_id,
            &config.destination_url,
            destination_token.as_deref(),
            config.allow_loopback_http,
        )?;
        let admin = match (&config.admin_token_env, config.operator_principal_id) {
            (Some(env), Some(principal)) => {
                let token = std::env::var(env).map_err(|_| PortError::InvalidInput {
                    code: "missing_admin_secret",
                })?;
                if token.len() < 32
                    || token.len() > 4096
                    || !token.bytes().all(|b| b.is_ascii_graphic())
                    || token == webhook_token
                {
                    return Err(PortError::InvalidInput {
                        code: "invalid_admin_secret",
                    });
                }
                Some((principal, token))
            }
            _ => None,
        };
        let mut sinks = vec![(target, config.destination_id, sink)];
        for t in &config.additional_targets {
            let scope = ConnectionScope {
                workspace_id: config.workspace_id,
                connection_id: t.connection_id,
            };
            let secret = t
                .token_env
                .as_ref()
                .map(|e| {
                    std::env::var(e).map_err(|_| PortError::InvalidInput {
                        code: "missing_target_secret",
                    })
                })
                .transpose()?;
            sinks.push((
                scope,
                t.destination_id,
                HttpSink::new(
                    scope,
                    t.destination_id,
                    &t.url,
                    secret.as_deref(),
                    t.allow_loopback_http,
                )?,
            ));
        }
        let store = Arc::new(SqliteStore::open(&config.database).await?);
        let mut binding = serde_json::json!({
                "workspace":config.workspace_id,"source":config.source_connection_id,
                "target":config.target_connection_id,"destination":config.destination_id,
                "principal":config.principal_id,"rule":config.rule_id,"url":config.destination_url,
        });
        // Preserve the exact v1/v2 binding for configurations with a single target.
        if !config.additional_targets.is_empty() {
            binding["additional_targets"]=serde_json::Value::Array(config.additional_targets.iter().map(|t|serde_json::json!({"connection":t.connection_id,"destination":t.destination_id,"rule":t.rule_id,"url":t.url})).collect());
        }
        store.bind_pipeline(binding).await?;

        let sources: Vec<_> = std::iter::once(routes::RouteSource {
            routing: None,
            connection_id: source.connection_id,
            name: config
                .source_name
                .clone()
                .unwrap_or_else(|| "Webhook".into()),
            address: format!("/api/webhooks/{}", source.connection_id),
            protocol: "Webhook".into(),
        })
        .chain(
            config
                .additional_sources
                .iter()
                .map(|s| routes::RouteSource {
                    routing: None,
                    connection_id: s.connection_id,
                    name: s.name.clone(),
                    address: format!("/api/webhooks/{}", s.connection_id),
                    protocol: "Webhook".into(),
                }),
        )
        .collect();
        store.activate_release(serde_json::json!({"sources":sources,"storage_encoding":2,"binary_wire_version":config.content.as_ref().map(|_|2),"webhook":config.webhook,"scheduler":config.scheduler,"capacity":config.capacity,"retention":config.retention})).await?;
        let content = match config.content {
            Some(content_config) => Some(Arc::new(
                super::content::FileContentStore::open(
                    store.clone(),
                    content_config,
                    source,
                    config.principal_id,
                    sinks
                        .iter()
                        .map(|(scope, _, _)| scope.connection_id)
                        .collect(),
                )
                .await?,
            )),
            None => None,
        };
        store.configure_capacity(config.capacity).await?;
        let mut initial = crate::domain::managed_route::RouteSnapshot {
            revision: 1,
            routes: vec![crate::domain::managed_route::ManagedRoute {
                message_types: crate::domain::managed_route::default_message_types(),
                response: None,
                routing: None,
                id: config.rule_id,
                name: "Webhook → HTTP".into(),
                enabled: true,
                source_id: Some(source.connection_id),
                processor: None,
                command: None,
                event_type: None,
                revision: 1,
                targets: std::iter::once((config.rule_id, config.destination_id))
                    .chain(
                        config
                            .additional_targets
                            .iter()
                            .map(|t| (t.rule_id, t.destination_id)),
                    )
                    .map(
                        |(rule_id, destination_id)| crate::domain::managed_route::RouteTarget {
                            rule_id,
                            destination_id,
                            branch: Default::default(),
                        },
                    )
                    .collect(),
            }],
        };
        if !config.bootstrap_routes {
            initial.routes.clear();
        }
        let connections = Arc::new(std::sync::RwLock::new(
            store.message_connections(source).await?,
        ));
        for connection in &connections
            .read()
            .map_err(|_| PortError::Unavailable)?
            .connections
        {
            store.configure_recent(
                ConnectionScope {
                    workspace_id: source.workspace_id,
                    connection_id: connection.id,
                },
                connection
                    .enabled
                    .then(|| {
                        connection
                            .recent_source()
                            .map(|source| (connection.recent_messages.clone(), source))
                    })
                    .flatten(),
            );
        }
        let mut snapshot = store.initialize_routes(source, initial).await?;
        for route in &mut snapshot.routes {
            let id = route.source_id.unwrap_or(source.connection_id);
            if !sources.iter().any(|s| s.connection_id == id)
                && !connections
                    .read()
                    .map_err(|_| PortError::Unavailable)?
                    .connections
                    .iter()
                    .any(|c| c.id == id && c.receives())
            {
                return Err(PortError::InvalidInput {
                    code: "route_source_missing",
                });
            }
            if let Some(command) = &route.command {
                command.validate().map_err(|_| PortError::InvalidInput {
                    code: "invalid_command_mapping",
                })?;
            }
            route.source_id = Some(id);
        }
        let route_endpoints = std::iter::once(routes::RouteEndpoint {
            actions: vec![],
            owner_connection_id: None,
            routing: None,
            connection_id: config.target_connection_id,
            destination_id: config.destination_id,
            address: config.destination_url.clone(),
            name: String::new(),
            protocol: "HTTP".into(),
        })
        .chain(
            config
                .additional_targets
                .iter()
                .map(|t| routes::RouteEndpoint {
                    actions: vec![],
                    owner_connection_id: None,
                    routing: None,
                    connection_id: t.connection_id,
                    destination_id: t.destination_id,
                    address: t.url.clone(),
                    name: String::new(),
                    protocol: "HTTP".into(),
                }),
        )
        .collect();
        let bindings = Arc::new(Bindings {
            connections: connections.clone(),
            source,
            sources: sources
                .iter()
                .map(|s| ConnectionScope {
                    workspace_id: source.workspace_id,
                    connection_id: s.connection_id,
                })
                .collect(),
            destinations: sinks
                .iter()
                .map(|(scope, id, _)| Destination {
                    id: *id,
                    scope: *scope,
                    replay_safety: ReplaySafety::NonIdempotent,
                })
                .collect(),
            principal: config.principal_id,
            routes: std::sync::RwLock::new(snapshot),
        });
        let governance = Arc::new(Governance::new(bindings.clone(), store.clone()));
        let commands = Arc::new(command_actions::CommandRuntime {
            bindings: bindings.clone(),
            store: store.clone(),
        });
        let mut registry = ConnectorRegistry::default();
        let lanes = sinks
            .iter()
            .map(|(_, id, _)| scheduler::TargetLane::new(*id))
            .collect();
        for (scope, _, sink) in sinks {
            let sink = if let Some(content) = &content {
                sink.with_content(content.clone())
            } else {
                sink
            };
            let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
                scope,
                kind: ConnectorKind::Http,
                display_name: "Configured HTTP destination".into(),
            });
            registration.sink = Some(Arc::new(sink));
            registry.register(registration)?;
        }
        for c in &connections
            .read()
            .map_err(|_| PortError::Unavailable)?
            .connections
        {
            if let Some(r) =
                message_connections::registration(source.workspace_id, c, commands.clone())?
            {
                registry.register(r)?;
            }
        }
        let registry = Arc::new(registry);
        Ok(Self {
            connection_status: Default::default(),
            qq_group_names: Default::default(),
            connection_io: Arc::new(tokio::sync::RwLock::new(())),
            commands,
            connections,
            registry: registry.clone(),
            webhook: config.webhook,
            source,
            content,
            retention: config.retention,
            content_downloads: Arc::new(Semaphore::new(16)),
            principal: config.principal_id,
            token: webhook_token,
            ingress_slots: Arc::new(Semaphore::new(64)),
            routing_lane: Mutex::new(()),
            delivery_lanes: lanes,
            delivery_slots: Semaphore::new(config.scheduler.max_in_flight),
            scheduler: config.scheduler,
            wake: tokio::sync::Notify::new(),
            admin,
            admin_slots: Arc::new(Semaphore::new(8)),
            ingresses: sources
                .iter()
                .map(|s| {
                    (
                        s.connection_id,
                        IngressService::new(
                            ConnectionScope {
                                workspace_id: source.workspace_id,
                                connection_id: s.connection_id,
                            },
                            store.clone(),
                        ),
                    )
                })
                .collect(),
            sources,
            routing: RoutingService::new(
                store.clone(),
                bindings.clone(),
                bindings.clone(),
                bindings.clone(),
                store.clone(),
                governance.clone(),
            ),
            delivery: DeliveryService::new(store.clone(), bindings.clone(), registry, governance),
            bindings,
            route_endpoints,
            store,
        })
    }
    pub fn authenticates(&self, header: Option<&str>) -> bool {
        header
            .and_then(|s| s.strip_prefix("Bearer "))
            .is_some_and(|s| bool::from(s.as_bytes().ct_eq(self.token.as_bytes())))
    }
    pub fn verifies_webhook(
        &self,
        timestamp: &str,
        delivery_id: &str,
        signature: &str,
        body: &[u8],
    ) -> bool {
        self.verifies_webhook_from(
            self.source.connection_id,
            timestamp,
            delivery_id,
            signature,
            body,
        )
    }
    pub fn verifies_webhook_from(
        &self,
        connection: Uuid,
        timestamp: &str,
        delivery_id: &str,
        signature: &str,
        body: &[u8],
    ) -> bool {
        self.ingresses.contains_key(&connection)
            && webhook::verify_signature(
                self.token.as_bytes(),
                connection,
                timestamp,
                delivery_id,
                signature,
                body,
                std::time::SystemTime::now(),
            )
    }
    /// HTTP admission before body buffering. The permit covers parsing and persistence.
    pub fn try_admit_webhook(&self) -> Result<OwnedSemaphorePermit, PortError> {
        self.ingress_slots.clone().try_acquire_owned().map_err(|_| {
            self.store
                .metrics
                .ingress_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            PortError::RateLimited {
                retry_after: Duration::from_secs(1),
            }
        })
    }
    pub(crate) fn context(
        &self,
        request_id: Uuid,
        cancellation: CancellationToken,
    ) -> AccessContext {
        AccessContext {
            identity: Identity {
                mcp_endpoint: None,
                workspace_id: self.source.workspace_id,
                principal_id: self.principal,
                client_id: None,
                scopes: Default::default(),
                expires_at: None,
            },
            operation: OperationContext {
                scope: self.source,
                principal_id: self.principal,
                request_id,
                correlation_id: request_id,
                causation_id: None,
                route_trace: vec![],
                deadline: Some(Instant::now() + Duration::from_secs(30)),
                cancellation,
            },
        }
    }
    /// HTTP boundary must authenticate before accepting a body.
    pub async fn accept(
        &self,
        request_id: Uuid,
        event: IncomingEvent,
    ) -> Result<IngestReceipt, PortError> {
        self.accept_from(self.source.connection_id, request_id, event)
            .await
    }
    pub async fn accept_from(
        &self,
        connection: Uuid,
        request_id: Uuid,
        event: IncomingEvent,
    ) -> Result<IngestReceipt, PortError> {
        if !self.has_source(connection) {
            return Err(PortError::NotFound);
        }
        let ingress = IngressService::new(
            ConnectionScope {
                workspace_id: self.source.workspace_id,
                connection_id: connection,
            },
            self.store.clone(),
        );
        if connection != self.source.connection_id
            && matches!(
                &event.payload,
                crate::domain::event::EventPayload::Content(_)
            )
        {
            return Err(PortError::Unsupported);
        }
        let started = Instant::now();
        let mut operation = self.context(request_id, CancellationToken::new()).operation;
        operation.scope.connection_id = connection;
        let event_type = event.event_type.clone();
        tracing::debug!(event="message.receive_started",connection_id=%connection,request_id=%request_id,event_type=%event_type,"Receiving message");
        let result = ingress.accept(operation, event).await;
        use crate::domain::event::IngestDisposition;
        use std::sync::atomic::Ordering;
        self.store
            .metrics
            .ingest_latency
            .observe(started.elapsed().as_micros() as u64);
        match &result {
            Ok(r) => {
                let disposition = match r.disposition {
                    IngestDisposition::Stored => "stored",
                    IngestDisposition::Duplicate => "duplicate",
                };
                tracing::info!(event="message.received",workspace_id=%self.source.workspace_id,connection_id=%connection,request_id=%request_id,event_id=%r.event_id,event_type=%event_type,disposition,elapsed_ms=started.elapsed().as_millis() as u64,"Message accepted");
                match r.disposition {
                    IngestDisposition::Stored => &self.store.metrics.stored,
                    IngestDisposition::Duplicate => &self.store.metrics.duplicates,
                }
                .fetch_add(1, Ordering::Relaxed);
                self.wake.notify_waiters();
            }
            Err(PortError::CapacityExceeded { .. }) => {
                self.store
                    .metrics
                    .capacity_rejected
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        if let Err(error) = &result {
            tracing::warn!(event="message.receive_failed",connection_id=%connection,request_id=%request_id,error=%error,elapsed_ms=started.elapsed().as_millis() as u64,"Message not accepted");
        }
        result
    }
    /// Bounded catch-up batch; errors leave events unrouted for later inspection/recovery.
    pub async fn tick(&self, stop: &CancellationToken) -> Result<(), PortError> {
        self.route_batch(stop).await?;
        self.deliver_batch(stop).await?;
        for c in self.connection_snapshot()?.connections {
            if c.enabled && c.sends() {
                self.deliver_target(&scheduler::TargetLane::new(c.send_id()), stop)
                    .await?;
            }
        }
        Ok(())
    }
    async fn route_batch(&self, stop: &CancellationToken) -> Result<(), PortError> {
        let _lane = tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            lane = self.routing_lane.lock() => lane,
        };
        for (scope, id) in self.store.unrouted(self.source.workspace_id, 100).await? {
            if stop.is_cancelled() {
                return Ok(());
            }
            if scope.workspace_id != self.source.workspace_id
                || !self.has_source(scope.connection_id)
            {
                continue;
            }
            let mut ctx = self.context(Uuid::new_v4(), stop.clone());
            ctx.operation.scope = scope;
            match self.routing.route(&ctx, id).await {
                Ok(report) if !report.audit_recorded => {
                    tracing::error!(event="pipeline.route_audit_failed",event_id=%id)
                }
                Err(error) => {
                    tracing::warn!(event="pipeline.route_failed",event_id=%id,error=%error);
                    self.store.record_routing_failure(scope, id, error).await?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    async fn deliver_batch(&self, stop: &CancellationToken) -> Result<(), PortError> {
        // Bounded by configured targets, never one spawned future per message.
        let mut tasks: Vec<_> = self
            .delivery_lanes
            .iter()
            .map(|lane| Some(Box::pin(self.deliver_target(lane, stop))))
            .collect();
        let mut error = None;
        std::future::poll_fn(|cx| {
            let mut pending = false;
            for slot in &mut tasks {
                if let Some(f) = slot {
                    use std::future::Future;
                    match f.as_mut().poll(cx) {
                        std::task::Poll::Ready(r) => {
                            if let Err(e) = r {
                                error = Some(e);
                            }
                            *slot = None;
                        }
                        std::task::Poll::Pending => pending = true,
                    }
                }
            }
            if pending {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        })
        .await;
        error.map_or(Ok(()), Err)
    }
    async fn deliver_target(
        &self,
        lane: &scheduler::TargetLane,
        stop: &CancellationToken,
    ) -> Result<(), PortError> {
        use crate::domain::delivery::DeliveryOutcome;
        use std::sync::atomic::Ordering;
        let _lane = tokio::select! {biased;_=stop.cancelled()=>return Ok(()),g=lane.gate.lock()=>g};
        for _ in 0..100 {
            let delay = {
                let s = lane.state.lock().await;
                if s.open_until.is_some_and(|t| t > Instant::now()) {
                    return Ok(());
                }
                s.next_start.saturating_duration_since(Instant::now())
            };
            // Long backoff yields to the supervisor; do not keep a tick or lane
            // occupied until a provider's potentially hours-long Retry-After expires.
            if delay > Duration::from_millis(self.scheduler.min_interval_ms) {
                return Ok(());
            }
            tokio::select! {biased;_=stop.cancelled()=>return Ok(()),_=tokio::time::sleep(delay)=>{}}
            let _slot = tokio::select! {biased;_=stop.cancelled()=>return Ok(()),p=self.delivery_slots.acquire()=>p.map_err(|_|PortError::Unavailable)?};
            let started = Instant::now();
            let _io = self.connection_io.read().await;
            let result = self
                .delivery
                .run_target(
                    &self.context(Uuid::new_v4(), stop.clone()),
                    lane.id,
                    self.scheduler.strict_order,
                )
                .await;
            // Never retain a global permit during rate-limit or circuit waiting.
            drop(_slot);
            drop(_io);
            let report = match result {
                Ok(Some(r)) => r,
                Ok(None) => return Ok(()),
                Err(e) => {
                    lane.state.lock().await.complete(false, &self.scheduler);
                    return Err(e);
                }
            };
            self.store
                .metrics
                .delivery_latency
                .observe(started.elapsed().as_micros() as u64);
            let success =
                matches!(report.outcome, DeliveryOutcome::Submitted(_)) && report.state_recorded;
            let counter = match report.outcome {
                DeliveryOutcome::Submitted(_) => &self.store.metrics.submitted,
                DeliveryOutcome::RetryAfter(_) | DeliveryOutcome::Deferred(_) => {
                    &self.store.metrics.retried
                }
                DeliveryOutcome::Failed { .. } => &self.store.metrics.failed,
                DeliveryOutcome::Reconcile => &self.store.metrics.reconcile,
            };
            counter.fetch_add(1, Ordering::Relaxed);
            let outcome = match report.outcome {
                DeliveryOutcome::Submitted(_) => "accepted",
                DeliveryOutcome::Deferred(_) | DeliveryOutcome::RetryAfter(_) => "retry_pending",
                DeliveryOutcome::Failed { .. } => "failed",
                DeliveryOutcome::Reconcile => "reconcile",
            };
            tracing::info!(event="pipeline.delivery_processed",target_id=%lane.id,delivery_id=%report.delivery_id,outcome,state_recorded=report.state_recorded,audit_recorded=report.audit_recorded);
            let mut s = lane.state.lock().await;
            s.starts += 1;
            // Respect a rejected attempt's delay across the whole destination, so the
            // remaining queue cannot hammer an unavailable or rate-limited service.
            let wait = match &report.outcome {
                DeliveryOutcome::Deferred(d) | DeliveryOutcome::RetryAfter(d) => *d,
                _ => Duration::ZERO,
            };
            s.next_start =
                Instant::now() + wait.max(Duration::from_millis(self.scheduler.min_interval_ms));
            s.complete(success, &self.scheduler);
            drop(s);
            if !report.state_recorded || !report.audit_recorded {
                tracing::error!(event="pipeline.delivery_record_failed",delivery_id=%report.delivery_id);
            }
        }
        Ok(())
    }
    pub async fn run(self: Arc<Self>, stop: CancellationToken) {
        let mut workers = tokio::task::JoinSet::new();
        let p = self.clone();
        let token = stop.clone();
        workers.spawn(async move { p.run_lane(&token, None).await });
        for i in 0..self.delivery_lanes.len() {
            let p = self.clone();
            let token = stop.clone();
            workers.spawn(async move { p.run_lane(&token, Some(i)).await });
        }
        {
            let p = self.clone();
            let token = stop.clone();
            workers.spawn(async move { p.run_connections(token).await });
        }
        if self.retention.is_some() || self.content.is_some() {
            let p = self.clone();
            let token = stop.clone();
            workers.spawn(async move {
                loop {
                    if token.is_cancelled() {break;}
                    if let Some(policy)=&p.retention {match p.store.maintain(p.source.workspace_id,policy.clone()).await {Ok(report)=>tracing::info!(event="storage.maintenance",payload_rows=report.payload_rows,audit_rows=report.audit_rows,expired_rows=report.expired_rows),Err(e)=>tracing::warn!(event="storage.maintenance_failed",error=%e)}}
                    if let Some(content)=&p.content && let Err(e)=content.collect().await {tracing::warn!(event="content.cleanup_failed",error=%e);}
                    tokio::select! {biased;_=token.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(60))=>{}}
                }
            });
        }
        // Unexpected exit cancels siblings; drain their in-flight side effects before failing.
        let mut failed = false;
        while let Some(result) = workers.join_next().await {
            if result.is_err() || !stop.is_cancelled() {
                failed = true;
                stop.cancel();
            }
        }
        assert!(!failed, "pipeline lane terminated unexpectedly");
    }
    async fn run_lane(&self, stop: &CancellationToken, target: Option<usize>) {
        loop {
            let wake = self.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            if stop.is_cancelled() {
                break;
            }
            let result = match target {
                None => self.route_batch(stop).await,
                Some(i) => self.deliver_target(&self.delivery_lanes[i], stop).await,
            };
            if let Err(error) = result {
                self.store
                    .metrics
                    .worker_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::error!(event="pipeline.worker_failed",error=%error);
            }
            tokio::select! {biased;_=stop.cancelled()=>break,_=wake=>{},_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        }
    }
}
