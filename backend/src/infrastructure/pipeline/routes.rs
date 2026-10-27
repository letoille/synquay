//! Console-managed directional paths over deployment-validated adapters.
use super::Pipeline;
use crate::{application::ports::PortError, domain::managed_route::*};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Serialize)]
pub struct RouteEndpoint {
    pub actions: Vec<crate::application::ports::connection_actions::ConnectionAction>,
    pub owner_connection_id: Option<Uuid>,
    pub routing: Option<crate::domain::connection_routing::RoutingDescriptor>,
    pub protocol: String,
    pub name: String,
    pub connection_id: Uuid,
    pub destination_id: Uuid,
    pub address: String,
}
#[derive(Clone, Serialize)]
pub struct RouteSource {
    pub routing: Option<crate::domain::connection_routing::RoutingDescriptor>,
    pub protocol: String,
    pub connection_id: Uuid,
    pub name: String,
    pub address: String,
}
/// Offset paging over the bounded route snapshot, in saved insertion order.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutesQuery {
    pub source_ids: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}
#[derive(Serialize)]
pub struct RoutePagination {
    pub page: usize,
    pub page_size: usize,
    pub total: usize,
    pub total_pages: usize,
}
#[derive(Serialize)]
pub struct RoutesView {
    pub available: bool,
    pub supports_processors: bool,
    pub supports_commands: bool,
    pub reason: Option<&'static str>,
    pub source_id: Uuid,
    pub source_address: String,
    pub targets: Vec<RouteEndpoint>,
    pub sources: Vec<RouteSource>,
    pub snapshot: RouteSnapshot,
    pub pagination: RoutePagination,
    pub matched: std::collections::BTreeMap<Uuid, u64>,
    pub counts: std::collections::BTreeMap<Uuid, std::collections::BTreeMap<String, u64>>,
}
impl Pipeline {
    pub async fn routes_view(&self) -> Result<RoutesView, PortError> {
        self.routes_page(RoutesQuery::default()).await
    }
    pub async fn routes_page(&self, query: RoutesQuery) -> Result<RoutesView, PortError> {
        let invalid = || PortError::InvalidInput {
            code: "invalid_route_query",
        };
        let page = query.page.unwrap_or(1);
        // Preserve the unfiltered legacy response for existing API clients.
        let page_size = query.page_size.unwrap_or(64);
        if page == 0 || !(1..=64).contains(&page_size) {
            return Err(invalid());
        }
        let offset = (page - 1).checked_mul(page_size).ok_or_else(invalid)?;
        let mut source_ids = Vec::new();
        if let Some(raw) = query.source_ids {
            if raw.is_empty() || raw.len() > 48 * 37 {
                return Err(invalid());
            }
            for value in raw.split(',') {
                let id = Uuid::parse_str(value).map_err(|_| invalid())?;
                if !self.all_sources().iter().any(|s| s.connection_id == id)
                    || source_ids.contains(&id)
                {
                    return Err(invalid());
                }
                source_ids.push(id);
            }
            if source_ids.len() > 48 {
                return Err(invalid());
            }
        }
        let _lane = self.routing_lane.lock().await;
        let snapshot = self
            .bindings
            .routes
            .read()
            .map_err(|_| PortError::Unavailable)?
            .clone();
        let filtered: Vec<_> = snapshot
            .routes
            .into_iter()
            .filter(|r| {
                source_ids.is_empty()
                    || source_ids.contains(&r.source_id.unwrap_or(self.source.connection_id))
            })
            .collect();
        let total = filtered.len();
        let pagination = RoutePagination {
            page,
            page_size,
            total,
            total_pages: total.div_ceil(page_size),
        };
        let snapshot = RouteSnapshot {
            revision: snapshot.revision,
            routes: filtered.into_iter().skip(offset).take(page_size).collect(),
        };
        let counts = self
            .store
            .route_counts(
                self.source.workspace_id,
                snapshot
                    .routes
                    .iter()
                    .flat_map(|r| r.targets.iter().map(|t| t.rule_id))
                    .collect(),
            )
            .await?;
        let matched = self
            .store
            .message_totals(
                self.source.workspace_id,
                "matched",
                snapshot.routes.iter().map(|r| r.id).collect(),
            )
            .await?;
        Ok(RoutesView {
            matched,
            available: self.agent.is_none(),
            supports_processors: true,
            supports_commands: true,
            reason: self.agent.as_ref().map(|_| "agent_pipeline"),
            source_id: self.source.connection_id,
            source_address: format!("/api/webhooks/{}", self.source.connection_id),
            targets: self.all_targets(),
            sources: self.all_sources(),
            snapshot,
            pagination,
            counts,
        })
    }
    pub async fn route_operation(
        &self,
        actor: Uuid,
        id: Uuid,
    ) -> Result<Option<RouteReceipt>, PortError> {
        self.store.route_receipt(self.source, actor, id).await
    }
    pub async fn change_route(
        self: &Arc<Self>,
        actor: Uuid,
        input: RouteInput,
    ) -> Result<RouteReceipt, PortError> {
        if self.agent.is_some() {
            return Err(PortError::Unsupported);
        }
        if actor.is_nil() || input.operation_id.is_nil() || input.route_id.is_nil() {
            return Err(PortError::InvalidInput {
                code: "invalid_route_id",
            });
        }
        let permit = self.try_admit_admin()?;
        let pipeline = self.clone();
        // A lost HTTP response must not interrupt commit + in-memory activation.
        tokio::spawn(async move {
            let _permit = permit;
            let _lane = pipeline.routing_lane.lock().await;
            let _io = pipeline.connection_io.write().await;
            let current = pipeline.store.route_snapshot(pipeline.source).await?;
            if pipeline
                .store
                .route_receipt(pipeline.source, actor, input.operation_id)
                .await?
                .is_some()
            {
                let receipt = pipeline
                    .store
                    .save_routes(pipeline.source, actor, input, current.clone())
                    .await?;
                *pipeline
                    .bindings
                    .routes
                    .write()
                    .map_err(|_| PortError::OutcomeUnknown)? = current;
                return Ok(receipt);
            }
            if current.revision != input.expected_revision {
                return Err(PortError::Conflict);
            }
            let mut next = current;
            next.revision = next.revision.checked_add(1).ok_or(PortError::Conflict)?;
            match &input.change {
                RouteChange::Save {
                    response,
                    name,
                    enabled,
                    source_id,
                    processor,
                    command,
                    routing,
                    event_type,
                    destination_ids,
                } => {
                    if name.trim().is_empty()
                        || name.len() > 120
                        || name.chars().any(char::is_control)
                        || event_type.as_ref().is_some_and(|s| {
                            s.is_empty() || s.len() > 128 || s.chars().any(char::is_control)
                        })
                        || destination_ids.is_empty()
                        || destination_ids.len() > 32
                        || destination_ids
                            .iter()
                            .collect::<std::collections::HashSet<_>>()
                            .len()
                            != destination_ids.len()
                        || destination_ids.iter().any(|id| {
                            !pipeline
                                .all_targets()
                                .iter()
                                .any(|t| t.destination_id == *id)
                        })
                    {
                        return Err(PortError::InvalidInput {
                            code: "invalid_route",
                        });
                    }
                    if let Some(Some(command)) = command {
                        command.validate().map_err(|_| PortError::InvalidInput {
                            code: "invalid_command_mapping",
                        })?;
                    }
                    let previous = next.routes.iter().find(|r| r.id == input.route_id);
                    let source_id = source_id
                        .or_else(|| previous.and_then(|r| r.source_id))
                        .unwrap_or(pipeline.source.connection_id);
                    let processor = processor
                        .clone()
                        .or_else(|| previous.and_then(|r| r.processor.clone()));
                    if !pipeline
                        .all_sources()
                        .iter()
                        .any(|s| s.connection_id == source_id)
                    {
                        return Err(PortError::InvalidInput {
                            code: "unknown_route_source",
                        });
                    }
                    if let Some(RouteProcessor::AgentDraft { name }) = &processor
                        && (*enabled
                            || name.trim().is_empty()
                            || name.len() > 120
                            || name.chars().any(char::is_control))
                    {
                        return Err(PortError::InvalidInput {
                            code: "agent_executor_not_configured",
                        });
                    }
                    if let Some(RouteProcessor::AgentHttp { name }) = &processor
                        && (name.trim().is_empty()
                            || name.len() > 120
                            || name.chars().any(char::is_control)
                            || destination_ids.iter().any(|id| {
                                !pipeline
                                    .all_targets()
                                    .iter()
                                    .any(|t| t.destination_id == *id && t.protocol == "HTTP")
                            }))
                    {
                        return Err(PortError::InvalidInput {
                            code: "invalid_agent_http_target",
                        });
                    }
                    let targets = destination_ids
                        .iter()
                        .map(|destination_id| RouteTarget {
                            destination_id: *destination_id,
                            rule_id: previous
                                .and_then(|r| {
                                    r.targets
                                        .iter()
                                        .find(|t| t.destination_id == *destination_id)
                                })
                                .map(|t| t.rule_id)
                                .unwrap_or_else(Uuid::new_v4),
                        })
                        .collect();
                    let route = ManagedRoute {
                        response: response.or_else(|| previous.and_then(|r| r.response)),
                        routing: routing
                            .as_deref()
                            .cloned()
                            .or_else(|| previous.and_then(|r| r.routing.clone())),
                        id: input.route_id,
                        name: name.trim().into(),
                        enabled: *enabled,
                        source_id: Some(source_id),
                        processor: processor.clone(),
                        command: command
                            .clone()
                            .map(|value| value.map(|boxed| *boxed))
                            .unwrap_or_else(|| previous.and_then(|r| r.command.clone())),
                        event_type: event_type.clone(),
                        revision: next.revision,
                        targets,
                    };
                    super::bindings::validate_routing(
                        &pipeline.connection_snapshot()?,
                        &route,
                        pipeline.source.connection_id,
                    )?;
                    if let Some(index) = next.routes.iter().position(|r| r.id == input.route_id) {
                        next.routes[index] = route;
                    } else {
                        next.routes.push(route);
                    }
                    if next.routes.len() > 64
                        || next
                            .routes
                            .iter()
                            .filter(|r| r.enabled)
                            .map(|r| r.targets.len())
                            .sum::<usize>()
                            > 32
                    {
                        return Err(PortError::InvalidInput {
                            code: "route_limit",
                        });
                    }
                }
                RouteChange::Delete => {
                    if !next.routes.iter().any(|r| r.id == input.route_id) {
                        return Err(PortError::NotFound);
                    }
                    next.routes.retain(|r| r.id != input.route_id);
                }
            }
            let receipt = pipeline
                .store
                .save_routes(pipeline.source, actor, input, next.clone())
                .await?;
            *pipeline
                .bindings
                .routes
                .write()
                .map_err(|_| PortError::OutcomeUnknown)? = next;
            pipeline.wake.notify_one();
            Ok(receipt)
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
}
