#[path = "message_connection_validation.rs"]
mod validation;
use super::{
    route_control::access,
    tool_control::{error, guard, rejection},
};
use crate::{
    AppState, application::ports::PortError,
    infrastructure::pipeline::message_connections::ConnectionInput,
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
pub(super) fn router(state: AppState) -> Router {
    let runtime = state
        .tools
        .clone()
        .expect("connection control requires runtime");
    Router::new()
        .route("/api/message-connections", get(view).post(change))
        .route(
            "/api/message-connections/{id}/conversations",
            get(conversations),
        )
        .layer(middleware::from_fn_with_state(runtime, guard))
        .with_state(state)
}
async fn view(State(state): State<AppState>) -> Response {
    if let Err(e) = access(&state, false).await {
        return error(e);
    }
    match state.pipeline {
        Some(p) => match p.connection_snapshot() {
            Ok(snapshot) => {
                let public = public_snapshot(&snapshot);
                Json(serde_json::json!({"available":p.agent.is_none(),"snapshot":public}))
                    .into_response()
            }
            Err(e) => error(e),
        },
        None => Json(serde_json::json!({"available":false})).into_response(),
    }
}
async fn change(State(state): State<AppState>, request: Request) -> Response {
    let actor = match access(&state, true).await {
        Ok(a) => a,
        Err(e) => return error(e),
    };
    let Some(p) = state.pipeline else {
        return error(PortError::Unsupported);
    };
    if request
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_none_or(|v| v.split(';').next().unwrap_or("").trim() != "application/json")
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 32 * 1024).await {
        Ok(b) => b,
        Err(_) => return error(PortError::PayloadTooLarge),
    };
    let input: ConnectionInput = match serde_json::from_slice(&bytes) {
        Ok(i) => i,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    let id = input.operation_id;
    let connection = input.connection.clone();
    match p.change_connection(actor, input).await {
        Ok(revision) => {
            Json(serde_json::json!({"operation_id":id,"revision":revision})).into_response()
        }
        Err(PortError::InvalidInput { code }) => validation::response(code, connection.as_ref()),
        Err(e) => error(e),
    }
}

async fn conversations(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Query(q): Query<crate::infrastructure::storage::conversations::ConversationQuery>,
) -> Response {
    if let Err(e) = access(&state, false).await {
        return error(e);
    }
    let Some(p) = state.pipeline else {
        return error(PortError::Unsupported);
    };
    match p.connection_conversations(id, q).await {
        Ok(page) => Json(page).into_response(),
        Err(e) => error(e),
    }
}

fn public_snapshot(
    snapshot: &crate::infrastructure::pipeline::message_connections::ConnectionSnapshot,
) -> serde_json::Value {
    let mut public = serde_json::to_value(snapshot).expect("serializable connection catalog");
    for (connection, value) in snapshot
        .connections
        .iter()
        .zip(public["connections"].as_array_mut().unwrap())
    {
        if let crate::infrastructure::connectors::message_config::ConnectionConfig::QqBot(qq) =
            &connection.config
        {
            value["secret_configured"] = serde_json::json!(qq.resolved_secret.is_some());
        }
    }
    public
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_read_exposes_status_only_and_rejects_environment_references() {
        use crate::infrastructure::{
            connectors::{message_config::*, qq::QqBotConfig},
            pipeline::message_connections::ConnectionSnapshot,
        };
        let c = MessageConnection {
            id: uuid::Uuid::new_v4(),
            name: "Bot".into(),
            description: String::new(),
            enabled: true,
            config: ConnectionConfig::QqBot(QqBotConfig {
                app_id: "123456".into(),
                resolved_secret: Some(std::sync::Arc::from("test-only-private-secret")),
                group_openid: String::new(),
                allowed_senders: vec![],
                allow_proactive: false,
            }),
        };
        let value = public_snapshot(&ConnectionSnapshot {
            revision: 1,
            connections: vec![c],
        });
        assert_eq!(value["connections"][0]["secret_configured"], true);
        assert!(
            value["connections"][0]["config"]
                .get("resolved_secret")
                .is_none()
        );
        assert!(!value.to_string().contains("test-only-private-secret"));
        assert!(
            serde_json::from_value::<QqBotConfig>(
                serde_json::json!({"app_id":"123456","secret_env":"OLD_SECRET"})
            )
            .is_err()
        );
    }
}
