//! Test-only RSA key and process-local secret references; no production credentials.
use axum::{Form, Json, Router, routing::post};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};
use synquay_server::{
    AppState,
    domain::mcp_server::{ServerAuth, ServerChange},
    infrastructure::tool_runtime::{ToolRuntime, config::ToolsConfig},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[test]
fn per_endpoint_auth_matrix() {
    // Rust 2024 forbids mutating the environment of a multithreaded test runner.
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "endpoint_auth_child", "--nocapture"])
        .env("SYNQUAY_AUTH_CHILD", "1")
        .env("SYNQUAY_TEST_BASIC", "fixture-basic-password")
        .env(
            "SYNQUAY_TEST_OAUTH",
            "fixture-oauth-secret-at-least-32-characters",
        )
        .status()
        .unwrap();
    assert!(status.success());
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
fn config(path: std::path::PathBuf) -> ToolsConfig {
    let mut c: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    c.database = path;
    c
}
fn change(name: &str, auth: ServerAuth) -> ServerChange {
    ServerChange {
        operation_id: Uuid::new_v4(),
        expected_revision: 0,
        expected_tool_revision: 0,
        name: name.into(),
        public_url: None,
        content_scope: None,
        expected_content_revision: None,
        description: Some("Test endpoint".into()),
        enabled: true,
        exposed_tools: vec!["echo".into()],
        auth: Some(auth),
        transport: Default::default(),
    }
}
fn jwt(claims: Value) -> String {
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
            "support/endpoint-test-private.pem"
        ))
        .unwrap(),
    )
    .unwrap()
}
async fn rpc(
    http: &reqwest::Client,
    url: &str,
    header: Option<&str>,
    method: &str,
) -> reqwest::Response {
    let mut r = http
        .post(url)
        .header("accept", "application/json, text/event-stream");
    if let Some(h) = header {
        r = r.header("authorization", h);
    }
    r.json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":if method=="initialize" {json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"auth-test","version":"1"}})} else {json!({})}})).send().await.unwrap()
}
#[tokio::test]
async fn endpoint_auth_child() {
    if std::env::var("SYNQUAY_AUTH_CHILD").as_deref() != Ok("1") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("db"));
    let rt = ToolRuntime::build_with_secrets(
        cfg.clone(),
        "legacy-token-at-least-32-characters".into(),
        vec![None],
    )
    .await
    .unwrap();
    let public = Uuid::new_v4();
    let basic = Uuid::new_v4();
    let signed = Uuid::new_v4();
    let oauth = Uuid::new_v4();
    let public_change = change("Public", ServerAuth::None);
    rt.save_server(public, public_change.clone()).await.unwrap();
    let basic_change = change(
        "Basic",
        ServerAuth::Basic {
            username: "device".into(),
            password_env: "SYNQUAY_TEST_BASIC".into(),
        },
    );
    let basic_view = rt.save_server(basic, basic_change.clone()).await.unwrap();
    assert!(!basic_view.to_string().contains("fixture-basic-password"));
    let jwt_auth = ServerAuth::Jwt {
        issuer: "https://issuer.test".into(),
        audience: "device-api".into(),
        subject: "agent".into(),
        public_key_pem: include_str!("support/endpoint-test-public.pem").into(),
    };
    rt.save_server(signed, change("Signed", jwt_auth))
        .await
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let resource = "https://synquay.test/mcp/oauth-fixture".to_string();
    let response_resource = resource.clone();
    let (authority,astop,atask)=serve(Router::new().route("/introspect",post(move |Form(form):Form<HashMap<String,String>>| {
        let resource=response_resource.clone();async move {
            Json(json!({"active":form.get("token").map(String::as_str)!=Some("revoked"),"iss":"https://issuer.test","aud":resource,"sub":"agent","client_id":"client-a","scope":if form.get("token").map(String::as_str)==Some("no-scope") {""}else{"mcp"},"exp":now+3600,"token_type":"Bearer"}))
        }
    }))).await;
    let oauth_auth = ServerAuth::OAuth {
        issuer: "https://issuer.test".into(),
        resource: resource.clone(),
        introspection_endpoint: format!("{authority}/introspect"),
        introspection_client_id: "resource-server".into(),
        introspection_secret_env: "SYNQUAY_TEST_OAUTH".into(),
        required_scope: "mcp".into(),
        subject: "agent".into(),
        oauth_client_id: "client-a".into(),
        allow_loopback_http: true,
    };
    let mut oauth_change = change("OAuth", oauth_auth);
    oauth_change.public_url = Some(resource.clone());
    rt.save_server(oauth, oauth_change).await.unwrap();
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(rt.clone()),
        ..Default::default()
    }))
    .await;
    let http = reqwest::Client::new();
    let public_url = format!("{base}/mcp/{public}");
    let basic_url = format!("{base}/mcp/{basic}");
    let jwt_url = format!("{base}/mcp/{signed}");
    let oauth_url = format!("{base}/mcp/oauth-fixture");
    assert_eq!(
        rpc(&http, &format!("{base}/mcp"), None, "initialize")
            .await
            .status(),
        401
    );
    let response = rpc(&http, &public_url, None, "tools/list").await;
    assert_eq!(response.status(), 200);
    assert!(
        response.json::<Value>().await.unwrap()["result"]["tools"]
            .as_array()
            .is_some_and(|v| v.len() == 1)
    );
    assert_eq!(
        rpc(&http, &basic_url, None, "initialize").await.status(),
        401
    );
    let basic_header = format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            "device:fixture-basic-password"
        )
    );
    assert_eq!(
        rpc(&http, &basic_url, Some(&basic_header), "tools/list")
            .await
            .status(),
        200
    );
    assert_eq!(
        rpc(
            &http,
            &basic_url,
            Some("Basic ZGV2aWNlOndyb25n"),
            "initialize"
        )
        .await
        .status(),
        401
    );
    assert_eq!(
        rpc(
            &http,
            &basic_url,
            Some("Bearer legacy-token-at-least-32-characters"),
            "initialize"
        )
        .await
        .status(),
        401
    );
    let claims =
        json!({"iss":"https://issuer.test","aud":"device-api","sub":"agent","exp":now+3600});
    let valid = format!("Bearer {}", jwt(claims.clone()));
    assert_eq!(
        rpc(&http, &jwt_url, Some(&valid), "tools/list")
            .await
            .status(),
        200
    );
    for (key, value) in [
        ("iss", json!("wrong")),
        ("aud", json!("wrong")),
        ("sub", json!("wrong")),
        ("exp", json!(now - 10)),
        ("nbf", json!(now + 3600)),
        ("iat", json!(now + 3600)),
    ] {
        let mut c = claims.clone();
        c[key] = value;
        assert_eq!(
            rpc(
                &http,
                &jwt_url,
                Some(&format!("Bearer {}", jwt(c))),
                "initialize"
            )
            .await
            .status(),
            401,
            "{key}"
        );
    }
    let mut missing = claims.clone();
    missing.as_object_mut().unwrap().remove("exp");
    assert_eq!(
        rpc(
            &http,
            &jwt_url,
            Some(&format!("Bearer {}", jwt(missing))),
            "initialize"
        )
        .await
        .status(),
        401
    );
    let hs = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(include_bytes!("support/endpoint-test-public.pem")),
    )
    .unwrap();
    assert_eq!(
        rpc(&http, &jwt_url, Some(&format!("Bearer {hs}")), "initialize")
            .await
            .status(),
        401
    );
    assert_eq!(
        rpc(&http, &oauth_url, Some("Bearer accepted"), "tools/list")
            .await
            .status(),
        200
    );
    assert_eq!(
        rpc(&http, &oauth_url, Some("Bearer revoked"), "initialize")
            .await
            .status(),
        401
    );
    assert_eq!(
        rpc(&http, &oauth_url, Some("Bearer no-scope"), "initialize")
            .await
            .status(),
        403
    );
    let metadata: Value = http
        .get(format!(
            "{base}/.well-known/oauth-protected-resource/mcp/oauth-fixture"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(metadata["resource"], resource);
    assert_eq!(
        http.get(format!(
            "{base}/.well-known/oauth-protected-resource/mcp/{basic}"
        ))
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    let endpoint = rt.endpoint(public).await.unwrap();
    let identity = rt.authenticate_endpoint(&endpoint, None).await.unwrap();
    let mut ctx = rt.context(CancellationToken::new());
    ctx.identity = identity;
    assert!(rt.service.list(&ctx, basic).await.is_err());
    let update = ServerChange {
        expected_revision: 1,
        expected_tool_revision: 1,
        auth: Some(ServerAuth::Basic {
            username: "device".into(),
            password_env: "SYNQUAY_TEST_BASIC".into(),
        }),
        operation_id: Uuid::new_v4(),
        ..public_change
    };
    rt.save_server(public, update.clone()).await.unwrap();
    assert!(rt.service.list(&ctx, public).await.is_err());
    assert_eq!(
        rpc(&http, &public_url, None, "initialize").await.status(),
        401
    );
    assert_eq!(rt.save_server(public, update).await.unwrap()["revision"], 2);
    let invalid = change(
        "Missing secret",
        ServerAuth::Basic {
            username: "device".into(),
            password_env: "SYNQUAY_MISSING_TEST_SECRET".into(),
        },
    );
    assert!(rt.save_server(Uuid::new_v4(), invalid).await.is_err());
    // Renaming routes changes actual dispatch and keeps the same auth and tool scope.
    let mut rename = change("Basic", ServerAuth::None);
    rename.expected_revision = 1;
    rename.expected_tool_revision = 1;
    rename.auth = None;
    rename.description = None;
    rename.public_url = Some(format!("{base}/mcp/home-water"));
    let saved = rt.save_server(basic, rename.clone()).await.unwrap();
    assert_eq!(saved["path"], "/mcp/home-water");
    assert_eq!(saved["authentication"], "basic");
    assert_eq!(rt.save_server(basic, rename).await.unwrap(), saved);
    assert_eq!(
        rpc(&http, &basic_url, Some(&basic_header), "initialize")
            .await
            .status(),
        404
    );
    let renamed_url = format!("{base}/mcp/home-water");
    assert_eq!(
        rpc(&http, &renamed_url, None, "initialize").await.status(),
        401
    );
    let response = rpc(&http, &renamed_url, Some(&basic_header), "tools/list").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut collision = change("Collision", ServerAuth::None);
    // Paths are unique even if the advertised host differs.
    collision.public_url = Some("https://another.test/mcp/home-water".into());
    assert!(matches!(
        rt.save_server(Uuid::new_v4(), collision).await,
        Err(
            synquay_server::application::ports::PortError::InvalidInput {
                code: "mcp_url_taken"
            }
        )
    ));
    for url in [
        "https://user:pass@example.test/mcp/demo",
        "https://example.test/mcp/demo?token=x",
        "https://example.test/mcp/demo#x",
        "https://example.test/api/tools",
        "https://example.test/mcp/a/b",
        "https://example.test/mcp/a/../b",
        "http://example.test/mcp/demo",
    ] {
        let mut invalid = change("Invalid URL", ServerAuth::None);
        invalid.public_url = Some(url.into());
        assert!(
            rt.save_server(Uuid::new_v4(), invalid).await.is_err(),
            "{url}"
        );
    }
    let mut default_edit = change("Default custom path", ServerAuth::None);
    default_edit.expected_revision = 1;
    default_edit.expected_tool_revision = 1;
    default_edit.auth = None;
    default_edit.public_url = Some(format!("{base}/mcp/default-custom"));
    rt.save_server(cfg.profile_id, default_edit).await.unwrap();
    assert_eq!(
        rpc(&http, &format!("{base}/mcp"), None, "initialize")
            .await
            .status(),
        404
    );
    assert_eq!(
        rpc(
            &http,
            &format!("{base}/mcp/default-custom"),
            None,
            "initialize"
        )
        .await
        .status(),
        401
    );
    stop.cancel();
    task.await.unwrap();
    drop(rt);
    let restarted = ToolRuntime::build_with_secrets(
        cfg,
        "legacy-token-at-least-32-characters".into(),
        vec![None],
    )
    .await
    .unwrap();
    let endpoint = restarted.endpoint(public).await.unwrap();
    assert!(matches!(endpoint.auth, ServerAuth::Basic { .. }));
    assert!(
        restarted
            .authenticate_endpoint(&endpoint, Some(&basic_header))
            .await
            .is_ok()
    );
    assert_eq!(
        restarted
            .endpoint_by_path("/mcp/home-water")
            .await
            .unwrap()
            .id,
        basic
    );
    assert_eq!(
        restarted
            .endpoint_by_path("/mcp/default-custom")
            .await
            .unwrap()
            .id,
        restarted.config.profile_id
    );
    assert!(restarted.endpoint_by_path("/mcp").await.is_err());
    assert!(
        restarted
            .endpoint_by_path(&format!("/mcp/{basic}"))
            .await
            .is_err()
    );
    astop.cancel();
    atask.await.unwrap();
}
