use axum::Router;
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{ClientConfig, GetPromptRequestParams, ProtocolVersion, ReadResourceRequestParams},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use synquay_server::{
    AppState,
    application::ports::{PortError, workspaces::WorkspaceRepository},
    domain::{mcp_content::McpContentConfig, workspace::*},
    infrastructure::{
        storage::SqliteStore,
        tool_runtime::{
            ToolRuntime,
            config::ToolsConfig,
            security::{AdminConfig, OAuthConfig, OAuthGrant, SecurityConfig},
        },
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
const TOKEN: &str = "content-test-token-more-than-32-characters";
fn catalog() -> McpContentConfig {
    serde_json::from_value(json!({
    "revision":1,
    "resources":[{"id":"guide","description":"Device guide","mime_type":"text/markdown","text":"# Device guide","exposed":true},{"id":"hidden","description":"Internal guide","mime_type":"text/plain","text":"private-text","exposed":false}],
    "templates":[{"id":"device","description":"Reviewed device information","mime_type":"application/json","parameter":"device_id","entries":{"heater":"{\"kind\":\"heater\"}","lamp":"{\"kind\":\"lamp\"}"},"exposed":true}],
    "prompts":[{"id":"plan","description":"Plan a task","exposed":true,"arguments":[{"name":"device","description":"Target device","required":true,"choices":["heater","lamp"]},{"name":"request","description":"User request","required":false}],"messages":[{"role":"user","text":"Device {{device}}. Request: {{request}}"}]}]
})).unwrap()
}
fn config(path: std::path::PathBuf) -> ToolsConfig {
    let mut c: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    c.database = path;
    c.tools.clear();
    c.mcp_content = catalog();
    c
}
async fn build(c: ToolsConfig) -> Arc<ToolRuntime> {
    ToolRuntime::build_with_secrets(c, TOKEN.into(), vec![])
        .await
        .unwrap()
}
async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let token = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(token.cancelled_owned())
            .await
            .unwrap();
    });
    (url, stop, task)
}
#[tokio::test]
async fn official_clients_read_resources_templates_prompts_and_complete_with_versioned_boundaries()
{
    let dir = tempfile::tempdir().unwrap();
    let rt = build(config(dir.path().join("db"))).await;
    let (endpoint, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    for modern in [false, true] {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).auth_header(TOKEN),
        );
        let client = if modern {
            ClientConfig::default()
                .serve_with_lifecycle(
                    transport,
                    ClientLifecycleMode::Discover {
                        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                    },
                )
                .await
                .unwrap()
        } else {
            ClientConfig::default()
                .with_protocol_version(ProtocolVersion::V_2025_11_25)
                .serve(transport)
                .await
                .unwrap()
        };
        let caps = &client.peer_info().unwrap().capabilities;
        assert!(caps.resources.is_some() && caps.prompts.is_some() && caps.completions.is_some());
        assert_ne!(caps.resources.as_ref().unwrap().subscribe, Some(true));
        let list = client.list_all_resources().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "guide");
        let read = client
            .read_resource(ReadResourceRequestParams::new(list[0].uri.clone()))
            .await
            .unwrap();
        assert!(
            serde_json::to_value(read).unwrap()["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Device guide")
        );
        let templates = client.list_all_resource_templates().await.unwrap();
        assert_eq!(templates.len(), 1);
        let template = &templates[0].uri_template;
        let read = client
            .read_resource(ReadResourceRequestParams::new(
                template.replace("{device_id}", "heater"),
            ))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(read).unwrap()["contents"][0]["mimeType"],
            "application/json"
        );
        for uri in [
            "file:///etc/passwd".to_owned(),
            list[0].uri.replace("/guide", "/hidden"),
            list[0].uri.replace("/c1/", "/c0/"),
            template.replace("{device_id}", "../guide"),
            template.replace("{device_id}", "%68eater"),
        ] {
            assert!(
                client
                    .read_resource(ReadResourceRequestParams::new(uri))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            client
                .complete_resource_simple(template, "device_id", "he")
                .await
                .unwrap(),
            vec!["heater"]
        );
        let prompts = client.list_all_prompts().await.unwrap();
        assert_eq!(prompts[0].name, "plan__c1");
        let request = GetPromptRequestParams::new("plan__c1").with_arguments(
            json!({"device":"heater","request":"{{device}}"})
                .as_object()
                .unwrap()
                .clone(),
        );
        let result = client.get_prompt(request).await.unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap()["messages"][0]["content"]["text"],
            "Device heater. Request: {{device}}"
        );
        assert!(
            client
                .get_prompt(GetPromptRequestParams::new("plan__c1"))
                .await
                .is_err()
        );
        assert!(
            client
                .get_prompt(GetPromptRequestParams::new("plan__c0"))
                .await
                .is_err()
        );
        assert_eq!(
            client
                .complete_prompt_simple("plan__c1", "device", "l")
                .await
                .unwrap(),
            vec!["lamp"]
        );
        client.cancel().await.unwrap();
    }
    stop.cancel();
    task.await.unwrap();
}
#[tokio::test]
async fn content_revision_is_immutable_and_revocation_and_audit_failure_block_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let cfg = config(path.clone());
    let rt = build(cfg.clone()).await;
    let ctx = rt.context(CancellationToken::new());
    let uri = rt.content.resource_uri("guide");
    assert!(rt.content.read(&ctx, &uri).await.is_ok());
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "CREATE TRIGGER audit_fail BEFORE INSERT ON audit BEGIN SELECT RAISE(ABORT,'fail'); END;",
    )
    .unwrap();
    assert!(rt.content.read(&ctx, &uri).await.is_err());
    sql.execute_batch("DROP TRIGGER audit_fail;").unwrap();
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .update(
            cfg.workspace_id,
            cfg.principal_id,
            Uuid::new_v4(),
            WorkspaceUpdate {
                expected_revision: 1,
                change: WorkspaceChange::SetState {
                    state: WorkspaceState::Suspended,
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        rt.content.resources(&ctx).await,
        Err(PortError::PermissionDenied)
    ));
    assert!(
        rt.content
            .complete(&ctx, true, "plan__c1", "device", "")
            .await
            .is_err()
    );
    drop(rt);
    let mut changed = cfg.clone();
    changed.mcp_content.resources[0].text = "changed".into();
    assert!(matches!(
        ToolRuntime::build_with_secrets(changed.clone(), TOKEN.into(), vec![]).await,
        Err(PortError::Conflict)
    ));
    changed.mcp_content.revision = 2;
    let reopened = build(changed).await;
    assert!(reopened.content.resource_uri("guide").contains("/c2/"));
    drop(reopened);
    assert!(matches!(
        ToolRuntime::build_with_secrets(cfg, TOKEN.into(), vec![]).await,
        Err(PortError::Conflict)
    ));
    let plan:String=sql.query_row("EXPLAIN QUERY PLAN SELECT revision,record FROM mcp_catalog_releases WHERE workspace='w' AND profile='p' ORDER BY revision DESC LIMIT 1",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("INDEX"));
}
#[tokio::test]
async fn oauth_content_grants_do_not_inherit_tool_grants_and_viewer_is_still_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"));
    let principal = Uuid::from_u128(20);
    let client = Uuid::from_u128(21);
    let security = SecurityConfig {
        admin: Some(AdminConfig {
            principal_id: Uuid::from_u128(10),
            token_env: "ADMIN".into(),
            allowed_origins: vec![],
        }),
        oauth: Some(OAuthConfig {
            issuer: "https://issuer.example/".into(),
            resource: "https://synquay.example/mcp".into(),
            introspection_endpoint: "https://issuer.example/introspect".into(),
            introspection_client_id: "synquay".into(),
            introspection_secret_env: "CLIENT_SECRET".into(),
            required_scope: "synquay".into(),
            allow_loopback_http: false,
            grants: vec![OAuthGrant {
                agent_jobs: false,
                subject: "user".into(),
                oauth_client_id: "client".into(),
                principal_id: principal,
                client_id: client,
                tools: vec![],
                resources: vec!["guide".into()],
                prompts: vec![],
            }],
        }),
    };
    let rt = ToolRuntime::build_configured(
        cfg.clone(),
        String::new(),
        vec![],
        security,
        Some("secret".into()),
    )
    .await
    .unwrap();
    let mut ctx = rt.context(CancellationToken::new());
    ctx.identity.principal_id = principal;
    ctx.identity.client_id = Some(client);
    ctx.identity.scopes.insert("synquay".into());
    ctx.operation.principal_id = principal;
    let store = SqliteStore::open(&cfg.database).await.unwrap();
    store
        .update(
            cfg.workspace_id,
            Uuid::from_u128(10),
            Uuid::new_v4(),
            WorkspaceUpdate {
                expected_revision: 1,
                change: WorkspaceChange::SetMember {
                    principal_id: principal,
                    role: WorkspaceRole::Viewer,
                    state: MemberState::Active,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(rt.content.resources(&ctx).await.unwrap().len(), 1);
    assert!(rt.content.templates(&ctx).await.unwrap().is_empty());
    assert!(rt.content.prompts(&ctx).await.unwrap().is_empty());
    assert!(
        rt.content
            .prompt(&ctx, "plan__c1", &BTreeMap::new())
            .await
            .is_err()
    );
    assert!(
        rt.content
            .complete(&ctx, true, "plan__c1", "device", "")
            .await
            .is_err()
    );
    store
        .update(
            cfg.workspace_id,
            Uuid::from_u128(10),
            Uuid::new_v4(),
            WorkspaceUpdate {
                expected_revision: 2,
                change: WorkspaceChange::SetMember {
                    principal_id: principal,
                    role: WorkspaceRole::Viewer,
                    state: MemberState::Revoked,
                },
            },
        )
        .await
        .unwrap();
    assert!(
        rt.content
            .read(&ctx, &rt.content.resource_uri("guide"))
            .await
            .is_err()
    );
}
#[test]
fn catalog_validation_rejects_unbound_placeholders_unbounded_and_ambiguous_identifiers() {
    let example = synquay_server::infrastructure::tool_runtime::config::ToolsConfig::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/tools-content.json"),
    )
    .unwrap();
    example.validate().unwrap();
    assert!(example.mcp_content.is_valid());
    let mut c = catalog();
    c.prompts[0].messages[0].text = "{{missing}}".into();
    assert!(!c.is_valid());
    let mut c = catalog();
    c.templates[0]
        .entries
        .insert("../escape".into(), "bad".into());
    assert!(!c.is_valid());
    let mut c = catalog();
    c.resources[0].text = "x".repeat(32769);
    assert!(!c.is_valid());
    let mut c = catalog();
    c.templates[0].id = "guide".into();
    assert!(!c.is_valid());
    let mut c = catalog();
    c.resources[0].mime_type = "application/json".into();
    c.resources[0].text = "invalid json".into();
    assert!(!c.is_valid());
    c.resources[0].text = "{\"valid\":true}".into();
    assert!(c.is_valid());
}

#[tokio::test]
async fn managed_content_scopes_isolate_reads_completion_and_survive_restart() {
    use synquay_server::domain::mcp_server::{ContentScope, ServerAuth, ServerChange};
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"));
    let rt = build(cfg.clone()).await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let empty = Uuid::new_v4();
    let change = |name: &str, resources: Vec<&str>, prompts: Vec<&str>| -> ServerChange {
        serde_json::from_value(
            json!({"operation_id":Uuid::new_v4(),"expected_revision":0,"expected_tool_revision":0,
            "name":name,"enabled":true,"exposed_tools":[],"expected_content_revision":1,
            "content_scope":{"resources":resources,"prompts":prompts}}),
        )
        .unwrap()
    };
    let mut original = change("Devices", vec!["guide", "device"], vec!["plan"]);
    original.auth = Some(ServerAuth::None);
    let saved = rt.save_server(a, original.clone()).await.unwrap();
    assert_eq!(
        saved["content_scope"]["resources"],
        json!(["guide", "device"])
    );
    assert_eq!(rt.save_server(a, original.clone()).await.unwrap(), saved);
    rt.save_server(b, change("Reference", vec!["guide", "hidden"], vec![]))
        .await
        .unwrap();
    // API clients omitting the new fields must not expose default-service content.
    let mut unconfigured = change("Empty", vec![], vec![]);
    unconfigured.content_scope = None;
    unconfigured.expected_content_revision = None;
    rt.save_server(empty, unconfigured).await.unwrap();
    assert_eq!(
        rt.server_view(empty).await.unwrap()["content_scope"],
        json!({"resources":[],"prompts":[]})
    );
    let inventory = rt.control_view().await.unwrap();
    assert_eq!(
        inventory["content"]["resources"].as_array().unwrap().len(),
        3
    );
    assert!(!inventory["content"].to_string().contains("private-text"));
    assert!(
        !inventory["content"]
            .to_string()
            .contains("Request: {{request}}")
    );
    for (resources, prompts) in [
        (vec!["missing"], vec![]),
        (vec!["guide", "guide"], vec![]),
        (vec![], vec!["plan", "plan"]),
    ] {
        assert!(matches!(
            rt.save_server(Uuid::new_v4(), change("Bad", resources, prompts))
                .await,
            Err(PortError::InvalidInput {
                code: "unknown_or_duplicate_content"
            })
        ));
    }
    let mut stale = change("Stale catalog", vec!["guide"], vec![]);
    stale.expected_content_revision = Some(0);
    assert!(matches!(
        rt.save_server(Uuid::new_v4(), stale).await,
        Err(PortError::Conflict)
    ));
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    for modern in [false, true] {
        let connect = |id| {
            let url = format!("{base}/{id}");
            async move {
                let transport = StreamableHttpClientTransport::from_config(
                    StreamableHttpClientTransportConfig::with_uri(url).auth_header(TOKEN),
                );
                if modern {
                    ClientConfig::default()
                        .serve_with_lifecycle(
                            transport,
                            ClientLifecycleMode::Discover {
                                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                            },
                        )
                        .await
                        .unwrap()
                } else {
                    ClientConfig::default()
                        .with_protocol_version(ProtocolVersion::V_2025_11_25)
                        .serve(transport)
                        .await
                        .unwrap()
                }
            }
        };
        let ca = connect(a).await;
        let cb = connect(b).await;
        let ce = connect(empty).await;
        assert!(ca.peer_info().unwrap().capabilities.resources.is_some());
        assert!(ca.peer_info().unwrap().capabilities.prompts.is_some());
        assert!(cb.peer_info().unwrap().capabilities.prompts.is_none());
        assert!(ce.peer_info().unwrap().capabilities.resources.is_none());
        assert!(ce.list_all_resources().await.unwrap().is_empty());
        let ra = ca.list_all_resources().await.unwrap();
        let rb = cb.list_all_resources().await.unwrap();
        assert_eq!(ra.len(), 1);
        assert_eq!(rb.len(), 2);
        assert!(ra[0].uri.contains(&a.to_string()));
        let bguide = rb.iter().find(|r| r.name == "guide").unwrap();
        assert_ne!(ra[0].uri, bguide.uri);
        assert!(
            cb.read_resource(ReadResourceRequestParams::new(ra[0].uri.clone()))
                .await
                .is_err()
        );
        assert!(
            ca.read_resource(ReadResourceRequestParams::new(bguide.uri.clone()))
                .await
                .is_err()
        );
        assert!(
            cb.read_resource(ReadResourceRequestParams::new(
                rb.iter().find(|r| r.name == "hidden").unwrap().uri.clone()
            ))
            .await
            .is_ok()
        );
        let templates = ca.list_all_resource_templates().await.unwrap();
        assert_eq!(templates.len(), 1);
        assert!(cb.list_all_resource_templates().await.unwrap().is_empty());
        assert_eq!(
            ca.complete_resource_simple(&templates[0].uri_template, "device_id", "he")
                .await
                .unwrap(),
            vec!["heater"]
        );
        assert!(
            cb.complete_resource_simple(&templates[0].uri_template, "device_id", "he")
                .await
                .is_err()
        );
        assert_eq!(ca.list_all_prompts().await.unwrap().len(), 1);
        assert!(cb.list_all_prompts().await.unwrap().is_empty());
        assert!(
            cb.get_prompt(GetPromptRequestParams::new("plan__c1"))
                .await
                .is_err()
        );
        assert!(
            cb.complete_prompt_simple("plan__c1", "device", "")
                .await
                .is_err()
        );
        assert_eq!(
            ca.complete_prompt_simple("plan__c1", "device", "he")
                .await
                .unwrap(),
            vec!["heater"]
        );
        ca.cancel().await.unwrap();
        cb.cancel().await.unwrap();
        ce.cancel().await.unwrap();
    }
    let before = rt.endpoint(a).await.unwrap();
    let content = rt.endpoint_content(&before);
    let mut context = rt.context(CancellationToken::new());
    context.identity = rt
        .authenticate_endpoint(&before, Some(&format!("Bearer {TOKEN}")))
        .await
        .unwrap();
    let old_uri = content.resource_uri("guide");
    assert!(content.read(&context, &old_uri).await.is_ok());
    let mut reduced = original;
    reduced.operation_id = Uuid::new_v4();
    reduced.expected_revision = 1;
    reduced.expected_tool_revision = 1;
    reduced.content_scope = Some(ContentScope {
        resources: vec!["device".into()],
        prompts: vec![],
    });
    // Scope and tool publication must roll back together if audit persistence fails.
    let sql = rusqlite::Connection::open(&cfg.database).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_scope_audit BEFORE INSERT ON mcp_server_changes BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(rt.save_server(a, reduced.clone()).await.is_err());
    assert_eq!(rt.server_view(a).await.unwrap(), saved);
    sql.execute_batch("DROP TRIGGER fail_scope_audit").unwrap();
    rt.save_server(a, reduced.clone()).await.unwrap();
    assert!(content.read(&context, &old_uri).await.is_err());
    let after = rt.endpoint(a).await.unwrap();
    context.identity = rt
        .authenticate_endpoint(&after, Some(&format!("Bearer {TOKEN}")))
        .await
        .unwrap();
    let current = rt.endpoint_content(&after);
    assert!(current.resources(&context).await.unwrap().is_empty());
    assert!(current.read(&context, &old_uri).await.is_err());
    assert!(
        current
            .complete(&context, true, "plan__c1", "device", "")
            .await
            .is_err()
    );
    assert_eq!(current.templates(&context).await.unwrap().len(), 1);
    // Legacy enable/name changes preserve the explicit content scope.
    let mut omitted = reduced;
    omitted.operation_id = Uuid::new_v4();
    omitted.expected_revision = 2;
    omitted.expected_tool_revision = 2;
    omitted.content_scope = None;
    omitted.expected_content_revision = None;
    let final_view = rt.save_server(a, omitted).await.unwrap();
    assert_eq!(
        final_view["content_scope"],
        json!({"resources":["device"],"prompts":[]})
    );
    stop.cancel();
    task.await.unwrap();
    drop(rt);
    let restarted = build(cfg.clone()).await;
    assert_eq!(restarted.server_view(a).await.unwrap(), final_view);
    assert_eq!(
        restarted.server_view(cfg.profile_id).await.unwrap()["content_scope"],
        json!({"resources":["guide","device"],"prompts":["plan"]})
    );
}
