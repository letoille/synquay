//! QQ's protocol is deliberately separate from generic Webhook authentication.
use super::tool_control::rejection;
use crate::{
    AppState,
    application::ports::PortError,
    infrastructure::connectors::qq::{Callback, callback},
};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

pub async fn receive(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Response {
    let Some(p) = state.pipeline else {
        return rejection(StatusCode::NOT_FOUND, "not_found");
    };
    let configuration = p.connection_io.clone().read_owned().await;
    let c = match p.qq_settings(id) {
        Ok(c) => c,
        Err(_) => return rejection(StatusCode::NOT_FOUND, "not_found"),
    };
    let headers = request.headers();
    let one = |key: &str| -> Option<String> {
        if headers.get_all(key).iter().count() != 1 {
            return None;
        }
        headers.get(key)?.to_str().ok().map(str::to_owned)
    };
    let app = one("x-bot-appid").unwrap_or_default();
    let ts = one("x-signature-timestamp").unwrap_or_default();
    let sig = one("x-signature-ed25519").unwrap_or_default();
    if headers.contains_key("origin") || app != c.app_id {
        return rejection(StatusCode::UNAUTHORIZED, "invalid_qq_identity");
    }
    if headers.contains_key("content-encoding")
        || one("content-type").is_none_or(|s| {
            !s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let _permit = match p.try_admit_webhook() {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::SERVICE_UNAVAILABLE, "ingress_busy"),
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 64 * 1024),
    )
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => return rejection(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"),
        Err(_) => return rejection(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
    };
    let mut event = match callback(&c, &app, &ts, &sig, &bytes, SystemTime::now()) {
        Ok(Callback::Challenge(v)) => return Json(v).into_response(),
        Ok(Callback::Ignore) => return Json(json!({"op":12})).into_response(),
        Ok(Callback::Event(e)) => *e,
        Err(PortError::PermissionDenied) => {
            return rejection(StatusCode::UNAUTHORIZED, "invalid_qq_signature");
        }
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_qq_callback"),
    };
    drop(bytes);
    // Detach the entire accepted operation, including enrichment, so a disconnected
    // client cannot cancel a verified message before durable persistence.
    let result = tokio::spawn(async move {
        let _configuration = configuration;
        let _permit = _permit;
        if let Some(group) = event
            .route_context
            .as_ref()
            .filter(|b| b.adapter == "qq" && b.config["kind"] == "group")
            .and_then(|b| b.config["id"].as_str())
            && let Some(name) = p
                .qq_group_names
                .get(p.source.workspace_id, id, &c, group)
                .await
            && let crate::domain::event::EventPayload::Json(payload) = &mut event.payload
            && let Some(object) = payload.as_object_mut()
        {
            object.insert("group_name".into(), json!(name));
        }
        p.accept_from(id, Uuid::new_v4(), event).await
    })
    .await
    .unwrap_or(Err(PortError::OutcomeUnknown));
    match result {
        Ok(_) => Json(json!({"op":12})).into_response(),
        Err(PortError::Conflict) => rejection(StatusCode::CONFLICT, "event_id_conflict"),
        Err(PortError::InvalidInput { .. } | PortError::ScopeMismatch) => {
            rejection(StatusCode::BAD_REQUEST, "invalid_event")
        }
        Err(_) => rejection(StatusCode::SERVICE_UNAVAILABLE, "persistence_unconfirmed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::builtin_tools::{BuiltinScope, BuiltinTool, ConversationGrant},
        infrastructure::{
            connectors::{
                message_config::{ConnectionConfig, MessageConnection, RecentMessagesConfig},
                qq::QqBotConfig,
            },
            pipeline::{Pipeline, config::PipelineConfig, message_connections::ConnectionInput},
            storage::conversations::ConversationQuery,
        },
    };
    use axum::{body::Body, http::Request};
    use ring::signature::Ed25519KeyPair;
    use std::{sync::Arc, time::UNIX_EPOCH};
    use tower::ServiceExt;
    #[tokio::test]
    async fn signed_callback_exposes_group_name_to_directory_and_mcp_query() {
        let dir = tempfile::tempdir().unwrap();
        let config = PipelineConfig {
            bootstrap_routes: false,
            webhook: Default::default(),
            content: None,
            retention: None,
            capacity: Default::default(),
            scheduler: Default::default(),
            additional_targets: vec![],
            additional_sources: vec![],
            source_name: None,
            admin_token_env: None,
            operator_principal_id: None,
            database: dir.path().join("db"),
            workspace_id: Uuid::from_u128(1),
            source_connection_id: Uuid::from_u128(2),
            target_connection_id: Uuid::from_u128(3),
            destination_id: Uuid::from_u128(4),
            principal_id: Uuid::from_u128(5),
            rule_id: Uuid::from_u128(6),
            webhook_token_env: "TEST_WEBHOOK".into(),
            destination_url: "http://127.0.0.1:1".into(),
            destination_token_env: None,
            allow_loopback_http: true,
        };
        let secret = "test-only-webhook-token-0123456789abcdef";
        let pipeline = Arc::new(Pipeline::build(config, secret.into(), None).await.unwrap());
        let connection = MessageConnection {
            id: Uuid::new_v4(),
            name: "QQ".into(),
            description: String::new(),
            enabled: true,
            recent_messages: RecentMessagesConfig {
                enabled: true,
                ..Default::default()
            },
            config: ConnectionConfig::QqBot(QqBotConfig {
                app_id: "123456".into(),
                resolved_secret: None,
                group_openid: String::new(),
                allowed_senders: vec![],
                allow_proactive: false,
            }),
        };
        pipeline
            .change_connection(
                Uuid::from_u128(10),
                ConnectionInput {
                    operation_id: Uuid::new_v4(),
                    expected_revision: 0,
                    connection_id: connection.id,
                    connection: Some(connection.clone()),
                    secret: Some(secret.into()),
                },
            )
            .await
            .unwrap();
        pipeline
            .qq_group_names
            .seed_for_test(connection.id, "group_A", "反馈群")
            .await;
        let body=json!({"op":0,"t":"GROUP_AT_MESSAGE_CREATE","d":{"id":"message_A","group_openid":"group_A","author":{"member_openid":"member_A"},"content":"hello"}}).to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let seed: Vec<u8> = secret.bytes().cycle().take(32).collect();
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let signature = key
            .sign(format!("{timestamp}{body}").as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let request = Request::post(format!("/api/qq/{}/webhook", connection.id))
            .header("content-type", "application/json")
            .header("x-bot-appid", "123456")
            .header("x-signature-timestamp", timestamp)
            .header("x-signature-ed25519", signature)
            .body(Body::from(body))
            .unwrap();
        let app = crate::interfaces::http::router(AppState {
            pipeline: Some(pipeline.clone()),
            ..Default::default()
        });
        assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::OK);
        let directory = pipeline
            .connection_conversations(
                connection.id,
                ConversationQuery {
                    kind: Some("group".into()),
                    search: Some("反馈".into()),
                    page: None,
                    page_size: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(directory.items[0].display_name.as_deref(), Some("反馈群"));
        let grant = ConversationGrant {
            connection_id: connection.id,
            account: "123456".into(),
            kind: "group".into(),
            conversation_id: "group_A".into(),
        };
        let scope = BuiltinScope {
            tools: vec![BuiltinTool::RecentMessages],
            connections: vec![connection.id],
            conversations: vec![grant.clone()],
            qq_send_conversations: vec![],
        };
        let result = pipeline
            .builtin_recent_messages(&scope, grant, None, 10)
            .await
            .unwrap();
        assert_eq!(result["messages"][0]["group_name"], "反馈群");
    }
}
