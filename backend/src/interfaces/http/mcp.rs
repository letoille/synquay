use crate::{
    application::services::tools::PublishedToolCall, infrastructure::tool_runtime::ToolRuntime,
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
struct VerifiedIdentity(crate::domain::identity::Identity);

#[derive(Clone)]
struct Handler {
    runtime: Arc<ToolRuntime>,
    profile_id: Uuid,
    name: String,
    extras: bool,
    content: crate::application::services::mcp_content::McpContentService,
}
fn rpc_error(error: impl std::fmt::Display) -> ErrorData {
    ErrorData::invalid_request(error.to_string(), None)
}
impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        let (resources, prompts) = self.content.capabilities();
        let mut capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .enable_completions()
            .build();
        if !resources {
            capabilities.resources = None;
        }
        if !prompts {
            capabilities.prompts = None;
        }
        if !resources && !prompts {
            capabilities.completions = None;
        }
        ServerConfig::new(capabilities).with_server_info(Implementation::new(self.name.clone(),env!("CARGO_PKG_VERSION"))).with_instructions("Use listed versioned tool names. Supply _meta[\"synquay/invocation-id\"] (UUID) to safely retry the same logical call. New IDs are new operations. HTTP timeout does not prove no side effect occurred.")
    }
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        self.check_page(&context, request)?;
        let items = self
            .content
            .resources(&self.access(&context))
            .await
            .map_err(rpc_error)?;
        let resources = items
            .into_iter()
            .map(|r| {
                Resource::new(self.content.resource_uri(&r.id), r.id)
                    .with_description(r.description)
                    .with_mime_type(r.mime_type)
                    .with_size(r.text.len() as u64)
            })
            .collect();
        Ok(ListResourcesResult {
            resources,
            ..Default::default()
        })
    }
    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        self.check_page(&context, request)?;
        let items = self
            .content
            .templates(&self.access(&context))
            .await
            .map_err(rpc_error)?;
        let resource_templates = items
            .into_iter()
            .map(|r| {
                ResourceTemplate::new(self.content.template_uri(&r), r.id)
                    .with_description(r.description)
                    .with_mime_type(r.mime_type)
            })
            .collect();
        Ok(ListResourceTemplatesResult {
            resource_templates,
            ..Default::default()
        })
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.check(&context)?;
        let r = self
            .content
            .read(&self.access(&context), &request.uri)
            .await
            .map_err(rpc_error)?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(r.text.as_ref(), r.uri).with_mime_type(r.mime_type),
        ])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
        .into())
    }
    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        self.check_page(&context, request)?;
        let items = self
            .content
            .prompts(&self.access(&context))
            .await
            .map_err(rpc_error)?;
        let prompts = items
            .into_iter()
            .map(|p| {
                Prompt::new(
                    self.content.prompt_name(&p.id),
                    Some(p.description),
                    Some(
                        p.arguments
                            .into_iter()
                            .map(|a| {
                                PromptArgument::new(a.name)
                                    .with_description(a.description)
                                    .with_required(a.required)
                            })
                            .collect(),
                    ),
                )
            })
            .collect();
        Ok(ListPromptsResult {
            prompts,
            ..Default::default()
        })
    }
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        self.check(&context)?;
        let args = serde_json::from_value(serde_json::Value::Object(
            request.arguments.unwrap_or_default(),
        ))
        .map_err(|_| rpc_error("prompt arguments must be strings"))?;
        let (description, messages) = self
            .content
            .prompt(&self.access(&context), &request.name, &args)
            .await
            .map_err(rpc_error)?;
        let mut result = GetPromptResult::new(
            messages
                .into_iter()
                .map(|m| {
                    PromptMessage::new_text(
                        match m.role {
                            crate::domain::mcp_content::PromptRole::User => Role::User,
                            crate::domain::mcp_content::PromptRole::Assistant => Role::Assistant,
                        },
                        m.text,
                    )
                })
                .collect(),
        );
        result.description = Some(description);
        Ok(result.into())
    }
    async fn complete(
        &self,
        request: CompleteRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        self.check(&context)?;
        let (prompt, reference) = match &request.r#ref {
            Reference::Prompt(p) => (true, p.name.as_str()),
            Reference::Resource(r) => (false, r.uri.as_str()),
            _ => return Err(rpc_error("unsupported completion reference")),
        };
        let (values, total) = self
            .content
            .complete(
                &self.access(&context),
                prompt,
                reference,
                &request.argument.name,
                &request.argument.value,
            )
            .await
            .map_err(rpc_error)?;
        let mut info = CompletionInfo::new(values).map_err(rpc_error)?;
        info.total = Some(total as u32);
        info.has_more = Some(total > info.values.len());
        Ok(CompleteResult::new(info))
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.check(&context)?;
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(rpc_error("invalid cursor"));
        }
        let ctx = self.access(&context);
        let catalog = self
            .runtime
            .service
            .list(&ctx, self.profile_id)
            .await
            .map_err(rpc_error)?;
        let mut tools: Vec<Tool> = catalog
            .tools
            .into_iter()
            .map(|d| {
                let mut tool = Tool::default();
                // Version suffix pins the client's observed schema/config across restarts.
                tool.name = format!("{}__r{}", d.id, catalog.revision).into();
                tool.description = Some(d.agent_description().into());
                tool.input_schema =
                    Arc::new(d.input_schema.as_object().cloned().unwrap_or_default());
                tool.output_schema = d
                    .output_schema
                    .and_then(|s| s.as_object().cloned())
                    .map(Arc::new);
                tool.annotations = Some(ToolAnnotations::new().read_only(matches!(
                    d.effect,
                    crate::domain::tool::ToolEffect::ReadOnly
                )));
                tool
            })
            .collect();
        if self.extras
            && let Some(agent) = &self.runtime.agent
        {
            tools.extend(
                super::agent_tools::list(agent, &ctx)
                    .await
                    .map_err(rpc_error)?,
            );
        }
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.check(&context)?;
        if super::agent_tools::is_builtin(&request.name) {
            self.check_extras()?;
            let Some(agent) = &self.runtime.agent else {
                return Err(rpc_error("agent queue is not enabled"));
            };
            let result = super::agent_tools::call(
                agent,
                &self.access(&context),
                &request.name,
                serde_json::Value::Object(request.arguments.unwrap_or_default()),
            )
            .await;
            return Ok(match result {
                Ok(v) => CallToolResult::structured(v),
                Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
            }
            .into());
        }
        let (name, revision) = request
            .name
            .rsplit_once("__r")
            .ok_or_else(|| rpc_error("use a listed versioned tool name"))?;
        let revision = revision
            .parse()
            .map_err(|_| rpc_error("invalid revision"))?;
        let meta = serde_json::to_value(&context.meta).map_err(rpc_error)?;
        let invocation_id = match meta.get("synquay/invocation-id") {
            Some(value) => value
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .filter(|id| !id.is_nil())
                .ok_or_else(|| rpc_error("invalid invocation UUID"))?,
            None => Uuid::new_v4(),
        };
        let ctx = self.access(&context);
        let report = self
            .runtime
            .service
            .invoke(
                &ctx,
                PublishedToolCall {
                    profile_id: self.profile_id,
                    revision,
                    public_name: name.into(),
                    invocation_id,
                    arguments: serde_json::Value::Object(request.arguments.unwrap_or_default()),
                },
            )
            .await;
        let mut result = match report {
            Ok(report) => {
                let mut result = match report.outcome {
                    Ok(value) => {
                        let mut r = CallToolResult::structured(
                            value.structured_content.unwrap_or(serde_json::json!({})),
                        );
                        r.is_error = Some(value.is_error);
                        r
                    }
                    Err(error) => {
                        CallToolResult::error(vec![ContentBlock::text(error.to_string())])
                    }
                };
                result.meta=Some(serde_json::from_value(serde_json::json!({"synquay/invocation-id":invocation_id,"synquay/audit-recorded":report.audit_recorded})).map_err(rpc_error)?);
                result
            }
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        };
        if result.meta.is_none() {
            result.meta = Some(
                serde_json::from_value(serde_json::json!({"synquay/invocation-id":invocation_id}))
                    .map_err(rpc_error)?,
            );
        }
        Ok(result.into())
    }
}
impl Handler {
    fn check_extras(&self) -> Result<(), ErrorData> {
        if self.extras {
            Ok(())
        } else {
            Err(rpc_error("agent jobs are not configured for this endpoint"))
        }
    }
    fn check_page(
        &self,
        ctx: &RequestContext<RoleServer>,
        request: Option<PaginatedRequestParams>,
    ) -> Result<(), ErrorData> {
        self.check(ctx)?;
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(rpc_error("invalid cursor"));
        }
        Ok(())
    }

    fn access(
        &self,
        ctx: &RequestContext<RoleServer>,
    ) -> crate::application::ports::identity::AccessContext {
        let mut access = self.runtime.context(ctx.ct.clone());
        if let Some(identity) = ctx
            .extensions
            .get::<Parts>()
            .and_then(|p| p.extensions.get::<VerifiedIdentity>())
        {
            access.identity = identity.0.clone();
            access.operation.principal_id = identity.0.principal_id;
        }
        if let Some(id) = ctx
            .extensions
            .get::<Parts>()
            .and_then(|p| p.extensions.get::<super::middleware::RequestContext>())
            .and_then(|c| Uuid::parse_str(&c.request_id).ok())
        {
            access.operation.request_id = id;
            access.operation.correlation_id = id;
        }
        access
    }
    fn check(&self, ctx: &RequestContext<RoleServer>) -> Result<(), ErrorData> {
        if ctx
            .extensions
            .get::<Parts>()
            .and_then(|p| p.extensions.get::<VerifiedIdentity>())
            .is_some()
        {
            Ok(())
        } else {
            Err(rpc_error("authentication required"))
        }
    }
}
fn challenge(runtime: &ToolRuntime, server: &crate::domain::mcp_server::McpServer) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [
            ("www-authenticate", runtime.endpoint_challenge(server)),
            ("cache-control", "no-store".into()),
        ],
        "authorization required",
    )
        .into_response()
}
async fn metadata(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    let Ok(server) = runtime.endpoint_by_path(request.uri().path()).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match runtime.endpoint_metadata(&server) {
        Some(data) => Json(data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn guard(
    State(runtime): State<Arc<ToolRuntime>>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.headers().contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(_permit) = runtime.requests.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let server = match runtime.endpoint_by_path(request.uri().path()).await {
        Ok(server) => server,
        Err(crate::application::ports::PortError::NotFound) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if request.headers().get_all("authorization").iter().count() > 1 {
        return challenge(&runtime, &server);
    }
    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    if header.is_some_and(|v| v.len() > 8192) {
        return challenge(&runtime, &server);
    }
    let identity = match runtime.authenticate_endpoint(&server, header).await {
        Ok(identity) => identity,
        Err(crate::application::ports::PortError::PermissionDenied) => {
            return challenge(&runtime, &server);
        }
        Err(crate::application::ports::PortError::RateLimited { .. }) => {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let scope = match &server.auth {
        crate::domain::mcp_server::ServerAuth::Inherited => runtime
            .oauth
            .as_ref()
            .map(|o| o.config.required_scope.as_str()),
        crate::domain::mcp_server::ServerAuth::OAuth { required_scope, .. } => {
            Some(required_scope.as_str())
        }
        _ => None,
    };
    if scope.is_some_and(|scope| !identity.scopes.contains(scope)) {
        return (
            StatusCode::FORBIDDEN,
            [(
                "www-authenticate",
                runtime
                    .endpoint_challenge(&server)
                    .replace("invalid_token", "insufficient_scope"),
            )],
            "insufficient scope",
        )
            .into_response();
    }
    request.extensions_mut().insert(server);
    request.extensions_mut().insert(VerifiedIdentity(identity));
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(Duration::from_secs(5), to_bytes(body, 64 * 1024)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    // JSON response mode keeps the permit until execution completes; no unbounded SSE.
    match tokio::time::timeout(
        Duration::from_secs(30),
        next.run(Request::from_parts(parts, Body::from(body))),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}
async fn dispatch(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    use tower::ServiceExt;
    let Some(endpoint) = request
        .extensions()
        .get::<crate::domain::mcp_server::McpServer>()
        .cloned()
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let id = endpoint.id;
    let handler = Handler {
        content: runtime.endpoint_content(&endpoint),
        runtime: runtime.clone(),
        profile_id: id,
        name: endpoint.name,
        extras: id == runtime.config.profile_id,
    };
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    // Stateless transports can be created per request; authentication and admission are shared.
    Router::new()
        .fallback_service(service)
        .oneshot(request)
        .await
        .unwrap()
}
pub fn router(runtime: Arc<ToolRuntime>) -> Router {
    let mcp = Router::new()
        .route("/mcp", any(dispatch))
        .route("/mcp/", any(dispatch))
        .route("/mcp/{id}", any(dispatch))
        .layer(middleware::from_fn_with_state(runtime.clone(), guard))
        .with_state(runtime.clone());
    let discovery = Router::new()
        .route("/.well-known/oauth-protected-resource/mcp", get(metadata))
        .route("/.well-known/oauth-protected-resource", get(metadata))
        .route(
            "/.well-known/oauth-protected-resource/mcp/{id}",
            get(metadata),
        )
        .with_state(runtime.clone());
    mcp.merge(discovery)
        .merge(super::tool_control::router(runtime))
}
