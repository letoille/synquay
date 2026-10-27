use axum::{
    Form, Json, Router,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientConfig},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use synquay_server::{
    AppState,
    application::{
        ports::{
            PortError,
            publications::{DraftInput, PublishInput},
        },
        services::tools::PublishedToolCall,
    },
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
const TOKEN: &str = "static-tool-token-more-than-32-characters";
const ADMIN: &str = "separate-admin-token-more-than-32-characters";
fn config(path: std::path::PathBuf, url: String) -> ToolsConfig {
    let mut c: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    c.database = path;
    c.tools[0].url = url;
    c
}
fn admin() -> SecurityConfig {
    SecurityConfig {
        oauth: None,
        admin: Some(AdminConfig {
            principal_id: Uuid::from_u128(10),
            token_env: "ADMIN_TOKEN".into(),
            allowed_origins: vec![],
        }),
    }
}
async fn runtime(c: ToolsConfig) -> Arc<ToolRuntime> {
    let n = c.tools.len();
    ToolRuntime::build_configured(c, TOKEN.into(), vec![None; n], admin(), None)
        .await
        .unwrap()
}
async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let token = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(token.cancelled_owned())
            .await
            .unwrap();
    });
    (base, stop, task)
}
fn draft(generation: u64, revision: u64, names: &[&str]) -> DraftInput {
    DraftInput {
        expected_generation: generation,
        base_revision: revision,
        exposed_tools: names.iter().map(|v| v.to_string()).collect(),
    }
}
fn publish(generation: u64, revision: u64) -> PublishInput {
    PublishInput {
        operation_id: Uuid::new_v4(),
        draft_generation: generation,
        expected_revision: revision,
    }
}
fn invocation(revision: u64) -> PublishedToolCall {
    PublishedToolCall {
        profile_id: Uuid::from_u128(2),
        revision,
        public_name: "echo".into(),
        invocation_id: Uuid::new_v4(),
        arguments: json!({"value":7}),
    }
}

#[tokio::test]
async fn admin_api_publishes_and_restores_latest_exposure_without_editing_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{base}/api/tools/config"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        http.get(format!("{base}/api/tools/config"))
            .bearer_auth(ADMIN)
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let view: Value = http
        .get(format!("{base}/api/tools/config"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["revision"], 1);
    let put = |body: Value| {
        http.put(format!("{base}/api/tools/draft"))
            .bearer_auth(ADMIN)
            .json(&body)
    };
    let saved: Value = put(json!({"expected_generation":0,"base_revision":1,"exposed_tools":[]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["generation"], 1);
    assert_eq!(
        put(json!({"expected_generation":0,"base_revision":1,"exposed_tools":[]}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        put(json!({"expected_generation":1,"base_revision":1,"exposed_tools":["unconfigured"]}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(put(json!({"expected_generation":1,"base_revision":1,"exposed_tools":[],"url":"http://127.0.0.1/secrets"})).send().await.unwrap().status(),400);
    let req = publish(1, 1);
    let receipt: Value = http
        .post(format!("{base}/api/tools/publish"))
        .bearer_auth(ADMIN)
        .json(&req)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(receipt["revision"], 2);
    assert_eq!(receipt["replayed"], false);
    let receipt: Value = http
        .post(format!("{base}/api/tools/publish"))
        .bearer_auth(ADMIN)
        .json(&req)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(receipt["replayed"], true);
    assert!(
        rt.service
            .list(&rt.context(CancellationToken::new()), cfg.profile_id)
            .await
            .unwrap()
            .tools
            .is_empty()
    );
    stop.cancel();
    task.await.unwrap();
    drop(rt);
    let rt = runtime(cfg.clone()).await;
    assert_eq!(rt.config.revision, 2);
    assert!(!rt.config.tools[0].exposed);
    rt.save_exposure_draft(draft(1, 2, &["echo"]))
        .await
        .unwrap();
    assert_eq!(
        rt.publish_exposure_draft(publish(2, 2))
            .await
            .unwrap()
            .revision,
        3
    );
    assert_eq!(
        rt.service
            .list(&rt.context(CancellationToken::new()), cfg.profile_id)
            .await
            .unwrap()
            .tools
            .len(),
        1
    );
    assert!(matches!(
        rt.service
            .invoke(&rt.context(CancellationToken::new()), invocation(1))
            .await,
        Err(PortError::Conflict)
    ));
    let c = rusqlite::Connection::open(cfg.database).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM tool_draft_audit", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM tool_publications WHERE principal=?1",
            [Uuid::from_u128(10).to_string()],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        2
    );
}

#[tokio::test]
async fn publication_waits_for_active_call_and_survives_disconnected_admin() {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let wait = release.clone();
    let (up, upstop, uptask) = serve(Router::new().route(
        "/echo",
        post(move || {
            let seen = seen.clone();
            let wait = wait.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                let p = wait.acquire().await.unwrap();
                p.forget();
                Json(json!({"value":7}))
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), format!("{up}/echo"));
    let rt = runtime(cfg.clone()).await;
    let caller = rt.clone();
    let call = tokio::spawn(async move {
        caller
            .service
            .invoke(&caller.context(CancellationToken::new()), invocation(1))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    rt.save_exposure_draft(draft(0, 1, &[])).await.unwrap();
    let request = publish(1, 1);
    let publisher = rt.clone();
    let r = request.clone();
    let pending = tokio::spawn(async move { publisher.publish_exposure_draft(r).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!pending.is_finished());
    // Reconciliation must remain responsive while the writer waits for a call.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(200),
            rt.publication_result(request.clone())
        )
        .await
        .unwrap()
        .unwrap()
        .is_none()
    );
    pending.abort();
    let _ = pending.await;
    release.add_permits(1);
    assert!(call.await.unwrap().unwrap().outcome.is_ok());
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if rt.control_view().await.unwrap()["revision"] == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(rt.publish_exposure_draft(request).await.unwrap().replayed);
    assert!(matches!(
        rt.service
            .invoke(&rt.context(CancellationToken::new()), invocation(2))
            .await,
        Err(PortError::NotFound)
    ));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    upstop.cancel();
    uptask.await.unwrap();
}

#[derive(Clone)]
struct Authority {
    issuer: String,
    resource: String,
    revoked: Arc<AtomicBool>,
    hits: Arc<AtomicUsize>,
}
async fn introspect(
    State(state): State<Authority>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    // client_secret_basic percent-encodes the secret before Basic base64.
    assert_eq!(
        headers["authorization"],
        "Basic aHR0cHMlM0ElMkYlMkZzeW5xdWF5LmV4YW1wbGUlMkZtY3A6c2VjcmV0JTJCcGx1cyUzQWNvbG9u"
    );
    assert_eq!(form["token_type_hint"], "access_token");
    state.hits.fetch_add(1, Ordering::SeqCst);
    let token = form["token"].as_str();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut data = json!({"active":!state.revoked.load(Ordering::SeqCst),"iss":state.issuer,"aud":state.resource,"sub":"alice","client_id":"agent","exp":now+300,"scope":"synquay:tools","token_type":"Bearer"});
    match token {
        "good" => {}
        "array" => data["aud"] = json!([state.resource, "other"]),
        "wrong_aud" => data["aud"] = json!("other"),
        "issuer" => data["iss"] = json!("https://evil.example/"),
        "expired" => data["exp"] = json!(now - 1),
        "future" => data["nbf"] = json!(now + 60),
        "future_iat" => data["iat"] = json!(now + 60),
        "missing_exp" => {
            data.as_object_mut().unwrap().remove("exp");
        }
        "subject" => data["sub"] = json!("mallory"),
        "client" => data["client_id"] = json!("unknown"),
        "scope" => data["scope"] = json!("other"),
        "id_token" => data["token_type"] = json!("id_token"),
        "user_b" => {
            data["sub"] = json!("bob");
            data["client_id"] = json!("agent-b");
        }
        "huge" => data["padding"] = json!("x".repeat(17000)),
        "redirect" => {
            return axum::response::Redirect::temporary("/should-not-follow").into_response();
        }
        "bad_json" => return "not-json".into_response(),
        _ => data["active"] = json!(false),
    }
    Json(data).into_response()
}

#[tokio::test]
async fn oauth_discovery_claim_checks_scope_challenge_grants_and_revocation() {
    let mcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", mcp_listener.local_addr().unwrap());
    let authority_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let authbase = format!("http://{}", authority_listener.local_addr().unwrap());
    let state = Authority {
        issuer: format!("{authbase}/issuer"),
        resource: format!("{base}/mcp"),
        revoked: Arc::new(AtomicBool::new(false)),
        hits: Arc::new(AtomicUsize::new(0)),
    };
    let stop = CancellationToken::new();
    let authstop = stop.clone();
    let auth_router = Router::new()
        .route("/introspect", post(introspect))
        .with_state(state.clone());
    let auth_task = tokio::spawn(async move {
        axum::serve(authority_listener, auth_router)
            .with_graceful_shutdown(authstop.cancelled_owned())
            .await
            .unwrap();
    });
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let seen = upstream_hits.clone();
    let (up, upstop, uptask) = serve(Router::new().route(
        "/echo",
        post(move |headers: HeaderMap| {
            let seen = seen.clone();
            async move {
                assert!(headers.get("authorization").is_none());
                seen.fetch_add(1, Ordering::SeqCst);
                Json(json!({"value":7}))
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), format!("{up}/echo"));
    let mut security = admin();
    security.oauth = Some(OAuthConfig {
        issuer: state.issuer.clone(),
        resource: state.resource.clone(),
        introspection_endpoint: format!("{authbase}/introspect"),
        introspection_client_id: "https://synquay.example/mcp".into(),
        introspection_secret_env: "OAUTH_SECRET".into(),
        required_scope: "synquay:tools".into(),
        allow_loopback_http: true,
        grants: vec![
            OAuthGrant {
                agent_jobs: false,
                resources: vec![],
                prompts: vec![],
                subject: "alice".into(),
                oauth_client_id: "agent".into(),
                principal_id: Uuid::from_u128(11),
                client_id: Uuid::from_u128(12),
                tools: vec!["echo".into()],
            },
            OAuthGrant {
                agent_jobs: false,
                resources: vec![],
                prompts: vec![],
                subject: "bob".into(),
                oauth_client_id: "agent-b".into(),
                principal_id: Uuid::from_u128(13),
                client_id: Uuid::from_u128(14),
                tools: vec![],
            },
        ],
    });
    let rt = ToolRuntime::build_configured(
        cfg.clone(),
        String::new(),
        vec![None],
        security,
        Some("secret+plus:colon".into()),
    )
    .await
    .unwrap();
    let server = stop.clone();
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(rt),
        ..Default::default()
    });
    let server_task = tokio::spawn(async move {
        axum::serve(mcp_listener, app)
            .with_graceful_shutdown(server.cancelled_owned())
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let meta: Value = http
        .get(format!("{base}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(meta["resource"], state.resource);
    assert_eq!(meta["authorization_servers"][0], state.issuer);
    let response = http.post(&state.resource).send().await.unwrap();
    assert_eq!(response.status(), 401);
    assert!(
        response.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("/.well-known/oauth-protected-resource/mcp")
    );
    for token in [
        "wrong_aud",
        "issuer",
        "expired",
        "future",
        "future_iat",
        "missing_exp",
        "subject",
        "client",
        "id_token",
        TOKEN,
        ADMIN,
    ] {
        assert_eq!(
            http.post(&state.resource)
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            401,
            "{token}"
        );
    }
    for token in ["huge", "redirect", "bad_json"] {
        assert_eq!(
            http.post(&state.resource)
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            503,
            "{token}"
        );
    }
    let response = http
        .post(&state.resource)
        .bearer_auth("scope")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert!(
        response.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("insufficient_scope")
    );
    assert_eq!(
        http.get(format!("{base}/api/tools/config"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let client = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(state.resource.clone())
                .auth_header("good"),
        ))
        .await
        .unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 1);
    let result = client
        .call_tool(
            CallToolRequestParams::new("echo__r1")
                .with_arguments(json!({"value":7}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(upstream_hits.load(Ordering::SeqCst), 1);
    let bob = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(state.resource.clone())
                .auth_header("user_b"),
        ))
        .await
        .unwrap();
    assert!(bob.list_all_tools().await.unwrap().is_empty());
    assert_eq!(
        bob.call_tool(
            CallToolRequestParams::new("echo__r1")
                .with_arguments(json!({"value":7}).as_object().unwrap().clone())
        )
        .await
        .unwrap()
        .is_error,
        Some(true)
    );
    bob.cancel().await.unwrap();
    state.revoked.store(true, Ordering::SeqCst);
    assert!(client.list_all_tools().await.is_err());
    let _ = client.cancel().await;
    assert_eq!(
        http.post(&state.resource)
            .bearer_auth("good")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let c = rusqlite::Connection::open(cfg.database).unwrap();
    assert_eq!(
        c.query_row("SELECT principal FROM tool_invocations", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        Uuid::from_u128(11).to_string()
    );
    stop.cancel();
    server_task.await.unwrap();
    auth_task.await.unwrap();
    upstop.cancel();
    uptask.await.unwrap();
}

#[tokio::test]
async fn draft_cas_and_publication_audit_failure_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    let (a, b) = tokio::join!(
        rt.save_exposure_draft(draft(0, 1, &[])),
        rt.save_exposure_draft(draft(0, 1, &["echo"]))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let c = rusqlite::Connection::open(&cfg.database).unwrap();
    c.execute_batch("CREATE TRIGGER block_publication BEFORE INSERT ON tool_publications BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
    let request = publish(1, 1);
    assert!(rt.publish_exposure_draft(request.clone()).await.is_err());
    assert_eq!(rt.control_view().await.unwrap()["revision"], 1);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM tool_publish_operations", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER block_publication").unwrap();
    assert_eq!(
        rt.publish_exposure_draft(request.clone())
            .await
            .unwrap()
            .revision,
        2
    );
    let mut changed = request.clone();
    changed.draft_generation = 2;
    assert!(matches!(
        rt.publish_exposure_draft(changed).await,
        Err(PortError::Conflict)
    ));
    // A new draft must be based on the current head, and a stale draft cannot publish.
    assert!(matches!(
        rt.save_exposure_draft(draft(1, 1, &[])).await,
        Err(PortError::Conflict)
    ));
    rt.save_exposure_draft(draft(1, 2, &[])).await.unwrap();
    assert!(matches!(
        rt.publish_exposure_draft(publish(1, 2)).await,
        Err(PortError::Conflict)
    ));
}

#[tokio::test]
async fn migration_v6_failure_rolls_back_and_preserves_v5() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let c = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/infrastructure/storage/schema.sql"),
        include_str!("../src/infrastructure/storage/migration_v2.sql"),
        include_str!("../src/infrastructure/storage/migration_v3.sql"),
        include_str!("../src/infrastructure/storage/migration_v4.sql"),
        include_str!("../src/infrastructure/storage/migration_v5.sql"),
    ] {
        c.execute_batch(sql).unwrap();
    }
    c.execute_batch("CREATE TABLE tool_publish_operations(collision INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='tool_drafts'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn security_rejects_unsafe_endpoints_and_ambiguous_grants() {
    let cfg = config("unused.db".into(), "http://127.0.0.1:1/echo".into());
    let good: SecurityConfig = serde_json::from_value(json!({
        "admin":{"principal_id":Uuid::from_u128(10),"token_env":"ADMIN_TOKEN"},
        "oauth":{
            "issuer":"https://auth.example/issuer","resource":"https://api.example/mcp",
            "introspection_endpoint":"https://auth.example/introspect",
            "introspection_client_id":"client","introspection_secret_env":"OAUTH_SECRET",
            "required_scope":"synquay:tools","grants":[{
                "subject":"alice","oauth_client_id":"agent","principal_id":Uuid::from_u128(11),
                "client_id":Uuid::from_u128(12),"tools":["echo"]
            }]
        }
    }))
    .unwrap();
    good.validate(&cfg).unwrap();
    let mut url_client = good.clone();
    url_client.oauth.as_mut().unwrap().introspection_client_id =
        "https://synquay.example/mcp".into();
    url_client.validate(&cfg).unwrap();
    for endpoint in [
        "http://auth.example/introspect",
        "https://user:pass@auth.example/introspect",
        "https://auth.example/introspect?token=x",
        "https://auth.example/introspect#fragment",
    ] {
        let mut bad = good.clone();
        bad.oauth.as_mut().unwrap().introspection_endpoint = endpoint.into();
        assert!(bad.validate(&cfg).is_err(), "{endpoint}");
    }
    let mut bad = good.clone();
    bad.admin.as_mut().unwrap().principal_id = Uuid::from_u128(11);
    assert!(bad.validate(&cfg).is_err());
    let mut bad = good.clone();
    bad.admin.as_mut().unwrap().token_env = cfg.token_env.clone();
    // Legacy admin secret names no longer participate in authentication.
    assert!(bad.validate(&cfg).is_ok());
    let mut bad = good.clone();
    let oauth = bad.oauth.as_mut().unwrap();
    oauth.grants.push(oauth.grants[0].clone());
    assert!(bad.validate(&cfg).is_err());
    let mut bad = good.clone();
    bad.oauth.as_mut().unwrap().grants[0]
        .tools
        .push("unconfigured".into());
    assert!(bad.validate(&cfg).is_err());
}

#[tokio::test]
async fn introspection_is_bounded_times_out_and_releases_capacity() {
    use synquay_server::{
        application::ports::identity::{Authenticator, Credentials},
        infrastructure::tool_runtime::oauth::OAuthAuthenticator,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let (base, stop, task) = serve(Router::new().route(
        "/introspect",
        post(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(4)).await;
                Json(json!({"active":false}))
            }
        }),
    ))
    .await;
    let auth = Arc::new(
        OAuthAuthenticator::new(
            OAuthConfig {
                issuer: format!("{base}/issuer"),
                resource: format!("{base}/mcp"),
                introspection_endpoint: format!("{base}/introspect"),
                introspection_client_id: "client".into(),
                introspection_secret_env: "SECRET".into(),
                required_scope: "tools".into(),
                grants: vec![],
                allow_loopback_http: true,
            },
            Uuid::from_u128(1),
            "secret".into(),
        )
        .unwrap(),
    );
    let mut calls = Vec::new();
    for _ in 0..8 {
        let auth = auth.clone();
        calls.push(tokio::spawn(async move {
            auth.authenticate(Credentials {
                bearer_token: "token".into(),
            })
            .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while hits.load(Ordering::SeqCst) < 8 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        auth.authenticate(Credentials {
            bearer_token: "token".into()
        })
        .await,
        Err(PortError::RateLimited { .. })
    ));
    for call in calls {
        assert!(matches!(call.await.unwrap(), Err(PortError::Unavailable)));
    }
    stop.cancel();
    task.await.unwrap();
    assert!(matches!(
        auth.authenticate(Credentials {
            bearer_token: "token".into()
        })
        .await,
        Err(PortError::Unavailable)
    ));
}

#[tokio::test]
async fn console_origin_guards_history_and_result_queries_are_scoped_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let mut security = admin();
    security.admin.as_mut().unwrap().allowed_origins = vec!["http://127.0.0.1:5173".into()];
    let rt = ToolRuntime::build_configured(cfg.clone(), TOKEN.into(), vec![None], security, None)
        .await
        .unwrap();
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    for origin in [
        "null",
        "https://evil.example",
        "http://127.0.0.1:5174",
        "http://127.0.0.1:5173/",
    ] {
        let r = http
            .get(format!("{base}/api/tools/history"))
            .bearer_auth(ADMIN)
            .header("origin", origin)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        assert_eq!(r.headers()["cache-control"], "no-store");
        assert_eq!(r.json::<Value>().await.unwrap()["code"], "origin_denied");
    }
    let r = http
        .get(format!("{base}/api/tools/config"))
        .bearer_auth(ADMIN)
        .header("origin", "http://127.0.0.1:5173")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("access-control-allow-origin").is_none());
    let value = r.json::<Value>().await.unwrap();
    assert_eq!(value["admin_id"], Uuid::from_u128(10).to_string());
    assert_eq!(value["connection"]["authentication"], "bearer");
    assert!(!value.to_string().contains(ADMIN));
    for request in [
        http.get(format!("{base}/api/tools/config"))
            .bearer_auth(ADMIN)
            .header("sec-fetch-site", "cross-site"),
        http.get(format!("{base}/api/tools/config"))
            .bearer_auth(ADMIN)
            .header("origin", "http://127.0.0.1:5173")
            .header("origin", "http://127.0.0.1:5173"),
    ] {
        assert_eq!(request.send().await.unwrap().status(), 403);
    }
    let save = http
        .put(format!("{base}/api/tools/draft"))
        .bearer_auth(ADMIN)
        .header("origin", "http://127.0.0.1:5173")
        .json(&draft(0, 1, &[]))
        .send()
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let operation = publish(1, 1);
    let query = format!(
        "{base}/api/tools/operations/{}?expected_revision=1&draft_generation=1",
        operation.operation_id
    );
    assert_eq!(
        http.get(&query)
            .bearer_auth(ADMIN)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["status"],
        "unconfirmed"
    );
    assert_eq!(
        http.post(format!("{base}/api/tools/publish"))
            .bearer_auth(ADMIN)
            .header("origin", "http://127.0.0.1:5173")
            .json(&operation)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let result = http
        .get(&query)
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(result["status"], "committed");
    assert_eq!(result["receipt"]["revision"], 2);
    assert_eq!(
        http.get(query.replace("draft_generation=1", "draft_generation=2"))
            .bearer_auth(ADMIN)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let first = http
        .get(format!("{base}/api/tools/history?limit=1"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(first["items"][0]["revision"], 2);
    assert_eq!(
        first["items"][0]["actor_id"],
        Uuid::from_u128(10).to_string()
    );
    assert_eq!(first["items"][0]["exposed_tools"], json!([]));
    assert_eq!(first["next_before_revision"], 2);
    let second = http
        .get(format!(
            "{base}/api/tools/history?limit=1&before_revision=2"
        ))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(second["items"][0]["revision"], 1);
    assert_eq!(second["items"][0]["exposed_tools"], json!(["echo"]));
    assert_eq!(second["next_before_revision"], Value::Null);
    assert!(!second.to_string().contains("bearer_env"));
    for query in [
        "limit=0",
        "limit=51",
        "before_revision=0",
        "before_revision=18446744073709551615",
        "workspace=other",
        "limit=no",
    ] {
        assert_eq!(
            http.get(format!("{base}/api/tools/history?{query}"))
                .bearer_auth(ADMIN)
                .send()
                .await
                .unwrap()
                .status(),
            400,
            "{query}"
        );
    }
    assert_eq!(
        http.get(format!("{base}/api/tools/history"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    use synquay_server::application::ports::publications::{
        PublicationScope, ToolPublicationStore,
    };
    let store = SqliteStore::open(&cfg.database).await.unwrap();
    let other = PublicationScope {
        workspace_id: Uuid::new_v4(),
        profile_id: cfg.profile_id,
        actor_id: Uuid::from_u128(10),
    };
    assert!(
        store
            .history(other, None, 10)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let sql = rusqlite::Connection::open(&cfg.database).unwrap();
    let mut plan=sql.prepare("EXPLAIN QUERY PLAN SELECT r.revision,p.principal FROM tool_releases r JOIN tool_publications p ON p.workspace=r.workspace AND p.profile=r.profile AND p.revision=r.revision WHERE r.workspace=?1 AND r.profile=?2 AND r.revision<?3 ORDER BY r.revision DESC LIMIT 11").unwrap();
    let plan = plan
        .query_map(
            rusqlite::params![
                cfg.workspace_id.to_string(),
                cfg.profile_id.to_string(),
                100
            ],
            |r| r.get::<_, String>(3),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("tool_publications_revision"), "{plan}");
    assert!(plan.contains("sqlite_autoindex_tool_releases"), "{plan}");
    assert!(!plan.contains("SCAN "), "{plan}");
    stop.cancel();
    task.await.unwrap();
}

#[test]
fn admin_origins_require_canonical_exact_secure_or_loopback_origins() {
    let cfg = config("unused.db".into(), "http://127.0.0.1:1/echo".into());
    for origin in [
        "https://console.example",
        "http://127.0.0.1:5173",
        "http://[::1]:5173",
        "http://localhost:5174",
    ] {
        let mut s = admin();
        s.admin.as_mut().unwrap().allowed_origins = vec![origin.into()];
        s.validate(&cfg).unwrap();
    }
    for origin in [
        "*",
        "null",
        "http://console.example",
        "http://localhost.example:5174",
        "https://console.example/",
        "https://console.example/path",
        "https://user@console.example",
        "https://console.example?x=1",
        "https://console.example#fragment",
        "https://CONSOLE.example",
        "https://console.example:443",
    ] {
        let mut s = admin();
        s.admin.as_mut().unwrap().allowed_origins = vec![origin.into()];
        assert!(s.validate(&cfg).is_err(), "{origin}");
    }
    let mut s = admin();
    s.admin.as_mut().unwrap().allowed_origins = vec!["https://console.example".into(); 2];
    assert!(s.validate(&cfg).is_err());
}

#[tokio::test]
async fn v7_history_index_failure_preserves_v6_and_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let sql = rusqlite::Connection::open(&path).unwrap();
    for migration in [
        include_str!("../src/infrastructure/storage/schema.sql"),
        include_str!("../src/infrastructure/storage/migration_v2.sql"),
        include_str!("../src/infrastructure/storage/migration_v3.sql"),
        include_str!("../src/infrastructure/storage/migration_v4.sql"),
        include_str!("../src/infrastructure/storage/migration_v5.sql"),
        include_str!("../src/infrastructure/storage/migration_v6.sql"),
    ] {
        sql.execute_batch(migration).unwrap();
    }
    sql.execute_batch("CREATE TABLE tool_publications_revision(collision INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        6
    );
    sql.execute_batch("DROP TABLE tool_publications_revision")
        .unwrap();
    SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        20
    );
}

#[tokio::test]
async fn workspace_admin_api_enforces_membership_suspend_recovery_and_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(
        dir.path().join("tools.db"),
        "http://127.0.0.1:8091/echo".into(),
    );
    cfg.tools[0].definition.effect = synquay_server::domain::tool::ToolEffect::SideEffecting;
    let runtime = runtime(cfg.clone()).await;
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    let endpoint = format!("{base}/api/workspace");
    assert_eq!(http.get(&endpoint).send().await.unwrap().status(), 200);
    let response = http.get(&endpoint).bearer_auth(ADMIN).send().await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let view: Value = response.json().await.unwrap();
    assert_eq!(view["revision"], 1);
    assert_eq!(view["id"], cfg.workspace_id.to_string());
    assert_eq!(
        http.patch(&endpoint)
            .bearer_auth(ADMIN)
            .header("origin", "https://evil.example")
            .json(&json!({"expected_revision":1,"change":{"kind":"set_state","state":"suspended"}}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(http.patch(&endpoint).bearer_auth(ADMIN).json(&json!({"workspace_id":Uuid::new_v4(),"expected_revision":1,"change":{"kind":"rename","name":"spoof"}})).send().await.unwrap().status(),400);
    assert_eq!(http.patch(&endpoint).bearer_auth(ADMIN).json(&json!({"expected_revision":1,"change":{"kind":"set_member","principal_id":Uuid::from_u128(10),"role":"viewer","state":"active"}})).send().await.unwrap().status(),403);
    let response=http.patch(&endpoint).bearer_auth(ADMIN).json(&json!({"expected_revision":1,"change":{"kind":"set_member","principal_id":cfg.principal_id,"role":"viewer","state":"active"}})).send().await.unwrap();
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(response.status(), 200);
    let ctx = runtime.context(CancellationToken::new());
    assert!(
        runtime
            .service
            .list(&ctx, cfg.profile_id)
            .await
            .unwrap()
            .tools
            .is_empty()
    );
    let sql = rusqlite::Connection::open(&cfg.database).unwrap();
    let saved: String = sql
        .query_row(
            "SELECT request_id FROM workspace_changes WHERE revision=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, request_id);
    assert_eq!(
        http.patch(&endpoint)
            .bearer_auth(ADMIN)
            .json(&json!({"expected_revision":1,"change":{"kind":"rename","name":"stale"}}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(http.patch(&endpoint).bearer_auth(ADMIN).json(&json!({"expected_revision":2,"change":{"kind":"set_member","principal_id":cfg.principal_id,"role":"operator","state":"revoked"}})).send().await.unwrap().status(),200);
    assert!(matches!(
        runtime.service.list(&ctx, cfg.profile_id).await,
        Err(PortError::PermissionDenied)
    ));
    assert_eq!(http.patch(&endpoint).bearer_auth(ADMIN).json(&json!({"expected_revision":3,"change":{"kind":"set_member","principal_id":cfg.principal_id,"role":"operator","state":"active"}})).send().await.unwrap().status(),200);
    assert_eq!(
        runtime
            .service
            .list(&ctx, cfg.profile_id)
            .await
            .unwrap()
            .tools
            .len(),
        1
    );
    assert_eq!(
        http.patch(&endpoint)
            .bearer_auth(ADMIN)
            .json(&json!({"expected_revision":4,"change":{"kind":"set_state","state":"suspended"}}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(matches!(
        runtime.service.list(&ctx, cfg.profile_id).await,
        Err(PortError::PermissionDenied)
    ));
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(ADMIN)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        http.put(format!("{base}/api/tools/draft"))
            .bearer_auth(ADMIN)
            .json(&json!({"expected_generation":0,"base_revision":1,"exposed_tools":["echo"]}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        http.patch(&endpoint)
            .bearer_auth(ADMIN)
            .json(&json!({"expected_revision":5,"change":{"kind":"set_state","state":"active"}}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        runtime
            .service
            .list(&ctx, cfg.profile_id)
            .await
            .unwrap()
            .tools
            .len(),
        1
    );
    let page: Value = http
        .get(format!("{endpoint}/members?limit=1"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page.as_array().unwrap().len(), 1);
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn online_connection_is_hidden_idempotent_and_restored_with_its_published_grant() {
    use synquay_server::infrastructure::tool_runtime::connections::{
        AddConnectionInput, VerifyAction, VerifyInput,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (upstream, upstream_stop, upstream_task) = serve(Router::new().route(
        "/echo",
        post(move |Json(v): Json<Value>| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Json(v)
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), format!("{upstream}/echo"));
    let rt = runtime(cfg.clone()).await;
    let id = Uuid::new_v4();
    let mut tool = cfg.tools[0].clone();
    tool.connection_id = id;
    tool.public_name = "online_echo".into();
    tool.exposed = false;
    let input = AddConnectionInput {
        operation_id: id,
        expected_revision: 1,
        tool,
    };
    assert_eq!(rt.add_connection(input.clone()).await.unwrap().revision, 2);
    assert!(rt.add_connection(input.clone()).await.unwrap().replayed);
    let mut changed = input.clone();
    changed.tool.definition.description = "changed".into();
    assert!(matches!(
        rt.add_connection(changed).await,
        Err(PortError::Conflict)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let view = rt.control_view().await.unwrap();
    assert_eq!(view["tools"].as_array().unwrap().len(), 2);
    assert_eq!(view["tools"][1]["exposed"], false);
    let invoke_id = Uuid::new_v4();
    let verify = |token: &str| VerifyInput {
        mcp_token: token.into(),
        invocation_id: invoke_id,
        action: VerifyAction::Execute {
            revision: 2,
            public_name: "online_echo".into(),
            arguments: json!({"value":7}),
            confirmed: true,
        },
    };
    assert!(rt.verify_connection(verify(ADMIN)).await.is_err());
    assert!(rt.verify_connection(verify(TOKEN)).await.is_err());
    rt.save_exposure_draft(draft(0, 2, &["echo", "online_echo"]))
        .await
        .unwrap();
    rt.publish_exposure_draft(publish(1, 2)).await.unwrap();
    let execute = || VerifyInput {
        mcp_token: TOKEN.into(),
        invocation_id: invoke_id,
        action: VerifyAction::Execute {
            revision: 3,
            public_name: "online_echo".into(),
            arguments: json!({"value":7}),
            confirmed: true,
        },
    };
    assert_eq!(
        rt.verify_connection(execute()).await.unwrap()["status"],
        "succeeded"
    );
    assert_eq!(
        rt.verify_connection(execute()).await.unwrap()["status"],
        "succeeded"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        rt.verify_connection(VerifyInput {
            mcp_token: TOKEN.into(),
            invocation_id: invoke_id,
            action: VerifyAction::Inspect
        })
        .await
        .unwrap()["status"],
        "succeeded"
    );
    drop(rt);
    let reopened = runtime(cfg).await;
    let view = reopened.control_view().await.unwrap();
    assert_eq!(view["revision"], 3);
    assert_eq!(view["tools"][1]["exposed"], true);
    assert!(reopened.add_connection(input).await.unwrap().replayed);
    assert_eq!(
        reopened.verify_connection(execute()).await.unwrap()["status"],
        "succeeded"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    upstream_stop.cancel();
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn online_connection_api_enforces_origin_validation_and_atomic_audit() {
    use synquay_server::infrastructure::tool_runtime::connections::AddConnectionInput;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let cfg = config(path.clone(), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let id = Uuid::new_v4();
    let mut tool = cfg.tools[0].clone();
    tool.connection_id = id;
    tool.public_name = "added".into();
    tool.exposed = false;
    let input = AddConnectionInput {
        operation_id: id,
        expected_revision: 1,
        tool,
    };
    let client = reqwest::Client::new();
    let endpoint = format!("{base}/api/tools/connections");
    assert_eq!(client.post(&endpoint).send().await.unwrap().status(), 415);
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(ADMIN)
            .header("origin", "https://evil.example")
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut invalid = input.clone();
    invalid.tool.exposed = true;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(ADMIN)
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    invalid = input.clone();
    invalid.tool.bearer_env = Some("SECRET".into());
    assert!(rt.add_connection(invalid).await.is_err());
    invalid = input.clone();
    invalid.tool.definition.input_schema =
        json!({"type":"object","properties":{"v":{"type":"nonsense"}}});
    assert!(rt.add_connection(invalid).await.is_err());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_connection_audit BEFORE INSERT ON managed_tool_connections BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(rt.add_connection(input.clone()).await.is_err());
    assert_eq!(rt.control_view().await.unwrap()["revision"], 1);
    assert!(rt.added_connection(id).await.unwrap().is_none());
    db.execute_batch("DROP TRIGGER fail_connection_audit;")
        .unwrap();
    assert_eq!(
        client
            .post(&endpoint)
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let receipt: Value = client
        .get(format!("{endpoint}/{id}"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(receipt["status"], "committed");
    let index:String=db.query_row("EXPLAIN QUERY PLAN SELECT tool_record FROM managed_tool_connections WHERE workspace='w' AND profile='p' ORDER BY revision LIMIT 33",[],|r|r.get(3)).unwrap();
    assert!(index.contains("INDEX"));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn console_without_admin_config_can_read_publish_and_restore_but_mcp_still_requires_auth() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = ToolRuntime::build_with_secrets(cfg.clone(), TOKEN.into(), vec![None])
        .await
        .unwrap();
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    let response = http
        .get(format!("{base}/api/tools/config"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<Value>().await.unwrap()["admin_id"],
        cfg.principal_id.to_string()
    );
    assert_eq!(
        http.post(format!("{base}/mcp"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.put(format!("{base}/api/tools/draft"))
            .header("origin", "https://unrelated.example")
            .json(&draft(0, 1, &[]))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = http
        .put(format!("{base}/api/tools/draft"))
        .header("origin", &base)
        .json(&draft(0, 1, &[]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response = http
        .post(format!("{base}/api/tools/publish"))
        .header("origin", &base)
        .json(&publish(1, 1))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["revision"], 2);
    stop.cancel();
    task.await.unwrap();
    drop(rt);
    let restarted = ToolRuntime::build_with_secrets(cfg, TOKEN.into(), vec![None])
        .await
        .unwrap();
    let view = restarted.control_view().await.unwrap();
    assert_eq!(view["revision"], 2);
    assert_eq!(view["tools"][0]["exposed"], false);
}

#[tokio::test]
async fn managed_mcp_servers_have_isolated_catalogs_crud_and_restart_tombstones() {
    use synquay_server::domain::mcp_server::{ServerChange, ServerDeletion};
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let change = |name: &str, names: Vec<String>| ServerChange {
        description: None,
        public_url: None,
        content_scope: None,
        expected_content_revision: None,
        auth: None,
        transport: Default::default(),
        operation_id: Uuid::new_v4(),
        expected_revision: 0,
        expected_tool_revision: 0,
        name: name.into(),
        enabled: true,
        exposed_tools: names,
    };
    let first = change("Research", vec!["echo".into()]);
    let saved = rt.save_server(a, first.clone()).await.unwrap();
    assert_eq!(rt.save_server(a, first.clone()).await.unwrap(), saved);
    rt.save_server(b, change("Empty", vec![])).await.unwrap();
    assert_eq!(
        rt.list_servers().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(matches!(
        rt.save_server(Uuid::new_v4(), change("research", vec![]))
            .await,
        Err(PortError::Conflict)
    ));
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    assert_eq!(
        http.post(format!("{base}/mcp/{a}"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let client_a = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp/{a}"))
                .auth_header(TOKEN),
        ))
        .await
        .unwrap();
    let client_b = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp/{b}"))
                .auth_header(TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(client_a.list_all_tools().await.unwrap().len(), 1);
    assert!(client_b.list_all_tools().await.unwrap().is_empty());
    assert_eq!(
        client_b
            .call_tool(
                CallToolRequestParams::new("echo__r1")
                    .with_arguments(json!({"value":7}).as_object().unwrap().clone())
            )
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    let edit = ServerChange {
        description: None,
        public_url: None,
        content_scope: None,
        expected_content_revision: None,
        auth: None,
        transport: Default::default(),
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        expected_tool_revision: 1,
        name: "Research revised".into(),
        enabled: true,
        exposed_tools: vec![],
    };
    assert_eq!(
        http.put(format!("{base}/api/tools/servers/{a}"))
            .header("origin", "https://evil.example")
            .json(&edit)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        http.put(format!("{base}/api/tools/servers/{a}"))
            .json(&edit)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(client_a.list_all_tools().await.unwrap().is_empty());
    let mut stale = edit.clone();
    stale.operation_id = Uuid::new_v4();
    assert!(matches!(
        rt.save_server(a, stale).await,
        Err(PortError::Conflict)
    ));
    let deletion = ServerDeletion {
        operation_id: Uuid::new_v4(),
        expected_revision: 2,
    };
    assert!(
        rt.delete_server(a, deletion.clone()).await.unwrap()["deleted"]
            .as_bool()
            .unwrap()
    );
    rt.delete_server(a, deletion).await.unwrap();
    assert_eq!(
        http.post(format!("{base}/mcp/{a}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let root = cfg.profile_id;
    rt.delete_server(
        root,
        ServerDeletion {
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        http.post(format!("{base}/mcp"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    client_a.cancel().await.unwrap();
    client_b.cancel().await.unwrap();
    stop.cancel();
    task.await.unwrap();
    drop(rt);
    let restored = runtime(cfg).await;
    assert!(restored.endpoint(a).await.is_err());
    assert!(restored.endpoint(root).await.is_err());
    assert!(restored.endpoint(b).await.is_ok());
    assert_eq!(
        restored.list_servers().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A deleted service ID cannot be reused, even with expected_revision=0.
    assert!(restored.save_server(a, first).await.is_err());
    let disable = ServerChange {
        description: None,
        public_url: None,
        content_scope: None,
        expected_content_revision: None,
        auth: None,
        transport: Default::default(),
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        expected_tool_revision: 1,
        name: "Empty".into(),
        enabled: false,
        exposed_tools: vec![],
    };
    restored.save_server(b, disable).await.unwrap();
    assert!(restored.endpoint(b).await.is_err());
}

#[tokio::test]
async fn default_mcp_edits_restore_without_admin_and_server_migration_is_atomic_and_indexed() {
    use synquay_server::domain::mcp_server::ServerChange;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let cfg = config(path.clone(), "http://127.0.0.1:1/echo".into());
    let build = || ToolRuntime::build_with_secrets(cfg.clone(), TOKEN.into(), vec![None]);
    let rt = build().await.unwrap();
    let input = ServerChange {
        description: None,
        public_url: None,
        content_scope: None,
        expected_content_revision: None,
        auth: None,
        transport: Default::default(),
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        expected_tool_revision: 1,
        name: "Default renamed".into(),
        enabled: true,
        exposed_tools: vec![],
    };
    rt.save_server(cfg.profile_id, input).await.unwrap();
    drop(rt);
    let restored = build().await.unwrap();
    assert_eq!(restored.control_view().await.unwrap()["revision"], 2);
    assert!(
        restored
            .service
            .list(&restored.context(CancellationToken::new()), cfg.profile_id)
            .await
            .unwrap()
            .tools
            .is_empty()
    );
    assert_eq!(
        restored.server_view(cfg.profile_id).await.unwrap()["name"],
        "Default renamed"
    );
    drop(restored);
    let sql = rusqlite::Connection::open(&path).unwrap();
    for query in [
        "SELECT record FROM mcp_servers WHERE workspace='w' AND id='p'",
        "SELECT record FROM mcp_servers WHERE workspace='w' AND deleted=0 ORDER BY name,id LIMIT 64",
        "SELECT 1 FROM mcp_servers WHERE workspace='w' AND lower(name)='n' AND deleted=0",
        "SELECT record FROM mcp_servers WHERE workspace='w' AND route_path='/mcp/a' AND deleted=0",
    ] {
        let plan = sql
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains("INDEX"), "{plan}");
        assert!(!plan.contains("SCAN mcp_servers"), "{plan}");
    }
    let rollback = dir.path().join("rollback");
    drop(SqliteStore::open(&rollback).await.unwrap());
    let c = rusqlite::Connection::open(&rollback).unwrap();
    c.execute_batch("DROP TRIGGER count_received; DROP TRIGGER count_submitted; DROP TABLE message_totals; ALTER TABLE deliveries DROP COLUMN counted_submitted; DROP TABLE connection_secrets; DROP TABLE connection_conversations; DROP INDEX routing_unresolved_source; DROP TABLE message_connection_operations; DROP TABLE message_connections; DROP TABLE managed_routes; DROP TABLE route_operations; DROP INDEX deliveries_by_route_state; ALTER TABLE deliveries DROP COLUMN route_rule_id; DROP TABLE tool_connection_changes; DROP TABLE tool_connection_overrides; DROP TABLE mcp_servers; DROP TABLE mcp_server_changes; CREATE TABLE mcp_server_changes(incompatible INTEGER); PRAGMA user_version=11;").unwrap();
    assert!(SqliteStore::open(&rollback).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        11
    );
    assert!(c.prepare("SELECT * FROM mcp_servers").is_err());
    c.execute_batch("DROP TABLE mcp_server_changes").unwrap();
    drop(SqliteStore::open(&rollback).await.unwrap());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        20
    );
}

#[tokio::test]
async fn connection_changes_cascade_drafts_and_servers_and_survive_restart() {
    use synquay_server::domain::mcp_server::ServerChange;
    use synquay_server::infrastructure::tool_runtime::connections::{
        ChangeConnectionInput, ConnectionChange,
    };
    let (upstream, upstream_stop, upstream_task) =
        serve(Router::new().route("/new", post(|Json(v): Json<Value>| async move { Json(v) })))
            .await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/old".into());
    let id = cfg.tools[0].connection_id;
    let rt = runtime(cfg.clone()).await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    for (server, name, enabled) in [(a, "A", true), (b, "B", false)] {
        let change:ServerChange=serde_json::from_value(json!({"operation_id":Uuid::new_v4(),"expected_revision":0,"expected_tool_revision":0,"name":name,"enabled":enabled,"exposed_tools":["echo"]})).unwrap();
        rt.save_server(server, change).await.unwrap();
    }
    rt.save_exposure_draft(draft(0, 1, &["echo"]))
        .await
        .unwrap();
    let change = ChangeConnectionInput {
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        change: ConnectionChange::Update {
            public_name: "renamed".into(),
            description: "Updated description".into(),
            url: format!("{upstream}/new"),
            effect: cfg.tools[0].definition.effect,
            input_schema: cfg.tools[0].definition.input_schema.clone(),
            allow_loopback_http: true,
        },
    };
    let receipt = rt.change_connection(id, change.clone()).await.unwrap();
    assert_eq!(receipt.revision, 2);
    assert!(
        rt.change_connection(id, change.clone())
            .await
            .unwrap()
            .replayed
    );
    let mut wrong = change.clone();
    wrong.expected_revision = 2;
    assert!(matches!(
        rt.change_connection(id, wrong).await,
        Err(PortError::Conflict)
    ));
    let view = rt.control_view().await.unwrap();
    assert_eq!(view["tools"][0]["public_name"], "renamed");
    assert_eq!(
        view["tools"][0]["definition"]["id"],
        cfg.tools[0].definition.id
    );
    assert_eq!(view["draft"]["exposed_tools"], json!(["renamed"]));
    assert_eq!(view["draft"]["generation"], 2);
    assert!(rt.publish_exposure_draft(publish(1, 1)).await.is_err());
    for server in [cfg.profile_id, a, b] {
        assert_eq!(
            rt.server_view(server).await.unwrap()["exposed_tools"],
            json!(["renamed"])
        );
        assert_eq!(rt.server_view(server).await.unwrap()["tool_revision"], 2);
    }
    let ctx = rt.context(CancellationToken::new());
    let mut call = invocation(2);
    call.public_name = "renamed".into();
    assert!(rt.service.invoke(&ctx, call).await.unwrap().outcome.is_ok());
    assert!(rt.service.invoke(&ctx, invocation(1)).await.is_err());
    drop(rt);
    let rt = runtime(cfg.clone()).await;
    assert_eq!(
        rt.control_view().await.unwrap()["tools"][0]["public_name"],
        "renamed"
    );
    assert!(rt.change_connection(id, change).await.unwrap().replayed);
    let deletion = ChangeConnectionInput {
        operation_id: Uuid::new_v4(),
        expected_revision: 2,
        change: ConnectionChange::Delete,
    };
    // Audit failure must roll back every service, draft and tombstone.
    let db = rusqlite::Connection::open(&cfg.database).unwrap();
    db.execute_batch("CREATE TRIGGER fail_change BEFORE INSERT ON tool_connection_changes BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(rt.change_connection(id, deletion.clone()).await.is_err());
    assert_eq!(
        rt.control_view().await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        rt.server_view(a).await.unwrap()["exposed_tools"],
        json!(["renamed"])
    );
    assert!(
        rt.added_connection(deletion.operation_id)
            .await
            .unwrap()
            .is_none()
    );
    db.execute_batch("DROP TRIGGER fail_change;").unwrap();
    assert_eq!(
        rt.change_connection(id, deletion.clone())
            .await
            .unwrap()
            .revision,
        3
    );
    for server in [cfg.profile_id, a, b] {
        assert_eq!(
            rt.server_view(server).await.unwrap()["exposed_tools"],
            json!([])
        );
    }
    assert_eq!(
        rt.control_view().await.unwrap()["draft"]["exposed_tools"],
        json!([])
    );
    assert_eq!(
        rt.list_servers().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    drop(rt);
    let rt = runtime(cfg).await;
    assert_eq!(rt.control_view().await.unwrap()["tools"], json!([]));
    assert!(
        rt.change_connection(id, deletion.clone())
            .await
            .unwrap()
            .replayed
    );
    assert!(
        rt.added_connection(deletion.operation_id)
            .await
            .unwrap()
            .is_some()
    );
    upstream_stop.cancel();
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn connection_mutation_api_rejects_stale_invalid_and_cross_origin_requests() {
    use synquay_server::infrastructure::tool_runtime::connections::{
        ChangeConnectionInput, ConnectionChange,
    };
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/old".into());
    let id = cfg.tools[0].connection_id;
    let rt = runtime(cfg.clone()).await;
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/tools/connections/{id}");
    let update = ChangeConnectionInput {
        operation_id: Uuid::new_v4(),
        expected_revision: 1,
        change: ConnectionChange::Update {
            public_name: "echo".into(),
            description: "Changed".into(),
            url: "https://example.com/new".into(),
            effect: cfg.tools[0].definition.effect,
            input_schema: cfg.tools[0].definition.input_schema.clone(),
            allow_loopback_http: false,
        },
    };
    assert_eq!(
        client
            .put(&url)
            .header("origin", "https://evil.example")
            .json(&update)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .delete(&url)
            .json(&update)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .put(&url)
            .json(&update)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .put(&url)
            .json(&update)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let mut stale = update.clone();
    stale.operation_id = Uuid::new_v4();
    assert_eq!(
        client.put(&url).json(&stale).send().await.unwrap().status(),
        409
    );
    let deletion = ChangeConnectionInput {
        operation_id: Uuid::new_v4(),
        expected_revision: 2,
        change: ConnectionChange::Delete,
    };
    assert_eq!(
        client
            .delete(&url)
            .json(&deletion)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!(
                "{base}/api/tools/connections/{}",
                deletion.operation_id
            ))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["status"],
        "committed"
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn deleted_online_connections_do_not_exhaust_inventory_on_restart() {
    use synquay_server::infrastructure::tool_runtime::connections::{
        AddConnectionInput, ChangeConnectionInput, ConnectionChange,
    };
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    for n in 0..34 {
        let id = Uuid::new_v4();
        let mut tool = cfg.tools[0].clone();
        tool.connection_id = id;
        tool.public_name = format!("temp_{n}");
        tool.exposed = false;
        rt.add_connection(AddConnectionInput {
            operation_id: id,
            expected_revision: 1 + 2 * n,
            tool,
        })
        .await
        .unwrap();
        rt.change_connection(
            id,
            ChangeConnectionInput {
                operation_id: Uuid::new_v4(),
                expected_revision: 2 + 2 * n,
                change: ConnectionChange::Delete,
            },
        )
        .await
        .unwrap();
    }
    drop(rt);
    let rt = runtime(cfg).await;
    assert_eq!(
        rt.control_view().await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(rt.control_view().await.unwrap()["revision"], 69);
}

#[tokio::test]
async fn deleted_managed_tool_grants_remain_deny_only_after_restart() {
    use synquay_server::infrastructure::tool_runtime::connections::{
        AddConnectionInput, ChangeConnectionInput, ConnectionChange,
    };
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"), "http://127.0.0.1:1/echo".into());
    let rt = runtime(cfg.clone()).await;
    let id = Uuid::new_v4();
    let mut tool = cfg.tools[0].clone();
    tool.connection_id = id;
    tool.public_name = "online".into();
    tool.exposed = false;
    rt.add_connection(AddConnectionInput {
        operation_id: id,
        expected_revision: 1,
        tool,
    })
    .await
    .unwrap();
    drop(rt);
    let security:SecurityConfig=serde_json::from_value(json!({"admin":{"principal_id":Uuid::from_u128(10)},"oauth":{"issuer":"https://auth.example/issuer","resource":"https://api.example/mcp","introspection_endpoint":"https://auth.example/introspect","introspection_client_id":"client","introspection_secret_env":"OAUTH_SECRET","required_scope":"synquay:tools","grants":[{"subject":"alice","oauth_client_id":"agent","principal_id":Uuid::from_u128(11),"client_id":Uuid::from_u128(12),"tools":["online"]}]}})).unwrap();
    let rt = ToolRuntime::build_configured(
        cfg.clone(),
        String::new(),
        vec![None],
        security.clone(),
        Some("introspection-secret".into()),
    )
    .await
    .unwrap();
    rt.change_connection(
        id,
        ChangeConnectionInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 2,
            change: ConnectionChange::Delete,
        },
    )
    .await
    .unwrap();
    drop(rt);
    let rt = ToolRuntime::build_configured(
        cfg,
        String::new(),
        vec![None],
        security,
        Some("introspection-secret".into()),
    )
    .await
    .unwrap();
    assert_eq!(
        rt.control_view().await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        rt.control_view().await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["public_name"] != "online")
    );
}
