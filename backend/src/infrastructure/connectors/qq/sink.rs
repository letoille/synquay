use super::QqBotConfig;
use crate::{
    application::ports::{OperationContext, PortError, PortFuture, messages::MessageSink},
    domain::{connector::ConnectionScope, message::*},
    infrastructure::connectors::message_config::invalid,
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Token {
    value: String,
    refresh_at: Instant,
}
pub struct QqBotSink {
    client: reqwest::Client,
    config: QqBotConfig,
    scope: ConnectionScope,
    id: Uuid,
    // No token or secret appears in config snapshots, Debug, or tracing.
    token: Arc<Mutex<Option<Token>>>,
    base: String,
}
impl QqBotSink {
    pub fn new(scope: ConnectionScope, id: Uuid, config: &QqBotConfig) -> Result<Self, PortError> {
        config.validate()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self {
            client,
            config: config.clone(),
            scope,
            id,
            token: Arc::new(Mutex::new(None)),
            base: "https://api.bot.qq.com".into(),
        })
    }
    async fn access_token(&self) -> Result<String, PortError> {
        // Single flight: concurrent sends share a token acquisition, rather than
        // refreshing once per message. Network timeouts bound the lock lifetime.
        let mut cached = self.token.lock().await;
        if let Some(t) = cached.as_ref().filter(|t| t.refresh_at > Instant::now()) {
            return Ok(t.value.clone());
        }
        let r = self
            .client
            .post(format!("{}/app/getAppAccessToken", self.base))
            .json(&json!({"appId":self.config.app_id,"clientSecret":self.config.secret()?}))
            .send()
            .await
            .map_err(|_| PortError::Unavailable)?;
        let status = r.status();
        let limited = super::super::http_retry::rate_limited(r.headers());
        if status.as_u16() == 429 {
            return Err(limited);
        }
        let v = read_json(r).await.map_err(|_| {
            if status.is_success() {
                PortError::Unavailable
            } else {
                provider_error(status, 0, false)
            }
        })?;
        if !status.is_success() || code(&v) != 0 {
            let error = provider_error(status, code(&v), false);
            return Err(if matches!(error, PortError::RateLimited { .. }) {
                limited
            } else {
                error
            });
        }
        let value = v["access_token"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or(PortError::Unavailable)?
            .to_owned();
        let ttl = v["expires_in"]
            .as_u64()
            .or_else(|| v["expires_in"].as_str()?.parse().ok())
            .filter(|n| *n > 0 && *n <= 7200)
            .ok_or(PortError::Unavailable)?;
        *cached = Some(Token {
            value: value.clone(),
            refresh_at: Instant::now() + Duration::from_secs(ttl.saturating_sub(60)),
        });
        Ok(value)
    }
    pub(super) async fn group_name(&self, group: &str) -> Result<String, PortError> {
        if !super::openid(group) {
            return Err(PortError::InvalidInput {
                code: "invalid_qq_group",
            });
        }
        let token = self.access_token().await?;
        let mut auth = reqwest::header::HeaderValue::from_str(&format!("QQBot {token}"))
            .map_err(|_| PortError::PermissionDenied)?;
        auth.set_sensitive(true);
        let response = self
            .client
            .get(format!("{}/v2/groups/{group}/info", self.base))
            .header(reqwest::header::AUTHORIZATION, auth)
            .send()
            .await
            .map_err(|_| PortError::Unavailable)?;
        let status = response.status();
        if status.as_u16() == 401 {
            *self.token.lock().await = None;
        }
        let limited = super::super::http_retry::rate_limited(response.headers());
        if status.as_u16() == 429 {
            return Err(limited);
        }
        let value = read_json(response).await?;
        if !status.is_success() || code(&value) != 0 {
            return Err(provider_error(status, code(&value), false));
        }
        if value["group_openid"].as_str() != Some(group) {
            return Err(PortError::ScopeMismatch);
        }
        value["group_name"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or(PortError::Unavailable)
    }
    pub(super) async fn bot_info(&self, ctx: &OperationContext) -> Result<Value, PortError> {
        if ctx.scope != self.scope {
            return Err(PortError::ScopeMismatch);
        }
        let request = async {
            let token = self.access_token().await?;
            let mut auth = reqwest::header::HeaderValue::from_str(&format!("QQBot {token}"))
                .map_err(|_| PortError::PermissionDenied)?;
            auth.set_sensitive(true);
            let response = self
                .client
                .get(format!("{}/users/@me", self.base))
                .header(reqwest::header::AUTHORIZATION, auth)
                .send()
                .await
                .map_err(|_| PortError::Unavailable)?;
            let status = response.status();
            if status.as_u16() == 401 {
                *self.token.lock().await = None;
            }
            let value = read_json(response).await?;
            if !status.is_success() || code(&value) != 0 {
                return Err(provider_error(status, code(&value), false));
            }
            let id = value["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(PortError::Unavailable)?;
            Ok(
                json!({"id":id,"username":value["username"],"avatar":value["avatar"],"bot":value["bot"]}),
            )
        };
        tokio::select! {biased;_=ctx.stopped()=>Err(PortError::Cancelled),r=request=>r}
    }
    fn destination<'a>(
        &'a self,
        m: &'a Message,
    ) -> Result<(&'a str, &'a str, Option<&'a str>), PortError> {
        if let Some(b) = &m.destination_binding {
            if b.adapter != "qq" || b.version != 1 || b.config["app_id"] != self.config.app_id {
                return Err(PortError::ScopeMismatch);
            }
            let kind = b.config["kind"]
                .as_str()
                .filter(|k| ["group", "direct"].contains(k))
                .ok_or(PortError::ScopeMismatch)?;
            let id = b.config["id"]
                .as_str()
                .filter(|id| super::openid(id))
                .ok_or(PortError::ScopeMismatch)?;
            if !self.config.group_openid.is_empty()
                && (kind != "group" || id != self.config.group_openid)
            {
                return Err(PortError::ScopeMismatch);
            }
            let reply = b.config["msg_id"].as_str();
            return Ok((kind, id, reply));
        }
        if self.config.group_openid.is_empty() {
            return Err(invalid("route_target_selection_required"));
        }
        Ok(("group", &self.config.group_openid, None))
    }
    fn message(&self, m: &Message) -> Result<Value, PortError> {
        if m.reply_route_id.is_some() {
            return Err(PortError::Unsupported);
        }
        let v = match m.parts.as_slice() {
            [ContentPart::Json(v)] => v.get("payload").unwrap_or(v).clone(),
            [ContentPart::Text(s)] => json!({"content":s}),
            _ => return Err(PortError::Unsupported),
        };
        let (kind, target, reply) = self.destination(m)?;
        if m.destination_binding.is_none()
            && v.get("group_openid")
                .is_some_and(|g| g.as_str() != Some(target))
        {
            return Err(PortError::ScopeMismatch);
        }
        let content = v["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 4000)
            .ok_or(invalid("invalid_qq_content"))?;
        let mut body = json!({"msg_type":0,"content":content});
        let passive = if m.destination_binding.is_some() {
            reply.map(|s| json!(s))
        } else {
            v.get("msg_id").cloned()
        };
        if let Some(id) = passive.as_ref() {
            let id = id
                .as_str()
                .filter(|s| crate::infrastructure::pipeline::webhook::valid_event_id(s, 256))
                .ok_or(invalid("invalid_qq_message_id"))?;
            let seq = match v.get("msg_seq") {
                Some(v) => v
                    .as_u64()
                    .filter(|n| (1..=if kind == "direct" { 4 } else { 5 }).contains(n))
                    .ok_or(invalid("invalid_qq_message_sequence"))?,
                None => 1,
            };
            body["msg_id"] = json!(id);
            body["msg_seq"] = json!(seq);
        } else if !self.config.allow_proactive {
            return Err(PortError::PermissionDenied);
        }
        Ok(body)
    }
}
impl MessageSink for QqBotSink {
    fn validate(&self, m: &Message) -> Result<(), PortError> {
        self.message(m).map(|_| ())
    }
    fn deliver(
        &self,
        ctx: OperationContext,
        r: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope || r.destination_id != self.id {
                return Err(PortError::ScopeMismatch);
            }
            let body = self.message(&r.message)?;
            let (kind, target, _) = self.destination(&r.message)?;
            let token = tokio::select! {biased;_=ctx.stopped()=>return Err(PortError::Cancelled),r=self.access_token()=>r.map_err(|e| match e { PortError::Unavailable => PortError::NotSent { retry_after: None }, PortError::RateLimited { retry_after } => PortError::NotSent { retry_after: Some(retry_after) }, other => other })?};
            let mut auth = reqwest::header::HeaderValue::from_str(&format!("QQBot {token}"))
                .map_err(|_| PortError::PermissionDenied)?;
            auth.set_sensitive(true);
            let send = async {
                let response = self
                    .client
                    .post(format!(
                        "{}/v2/{}/{}/messages",
                        self.base,
                        if kind == "direct" { "users" } else { "groups" },
                        target
                    ))
                    .header(reqwest::header::AUTHORIZATION, auth)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        if e.is_connect() {
                            PortError::NotSent { retry_after: None }
                        } else {
                            PortError::OutcomeUnknown
                        }
                    })?;
                let status = response.status();
                let limited = super::super::http_retry::rate_limited(response.headers());
                if status.as_u16() == 429 {
                    return Err(limited);
                }
                if status.as_u16() == 401 {
                    *self.token.lock().await = None;
                    // QQ rejected authentication, so no message was accepted. Refresh
                    // the token on the next bounded outbox attempt, not in a tight loop.
                    return Err(PortError::NotSent { retry_after: None });
                }
                let v = read_json(response).await.map_err(|e| {
                    if status.is_success() {
                        e
                    } else {
                        provider_error(status, 0, true)
                    }
                })?;
                let code = code(&v);
                if !status.is_success() || code != 0 {
                    // Numeric API errors only; no provider messages, credentials, or group content.
                    tracing::warn!(
                        event = "qq.send_rejected",
                        status = status.as_u16(),
                        provider_code = code
                    );
                    let error = provider_error(status, code, true);
                    return Err(if matches!(error, PortError::RateLimited { .. }) {
                        limited
                    } else {
                        error
                    });
                }
                let id = v["id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512)
                    .ok_or(PortError::OutcomeUnknown)?;
                Ok(DeliveryReceipt {
                    delivery_id: r.delivery_id,
                    external_message_id: Some(id.into()),
                    status: DeliveryStatus::Accepted,
                })
            };
            tokio::select! {biased;_=ctx.stopped()=>Err(PortError::OutcomeUnknown),result=send=>result}
        })
    }
}
async fn read_json(mut response: reqwest::Response) -> Result<Value, PortError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(PortError::OutcomeUnknown);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| PortError::OutcomeUnknown)
}
fn code(v: &Value) -> i64 {
    v.get("code")
        .and_then(Value::as_i64)
        .unwrap_or(if v.get("code").is_some() { -1 } else { 0 })
}
fn provider_error(status: reqwest::StatusCode, c: i64, side_effect: bool) -> PortError {
    if status.as_u16() == 429 || [100001, 40034100].contains(&c) {
        return PortError::RateLimited {
            retry_after: Duration::from_secs(60),
        };
    }
    if status.is_server_error() || status.as_u16() == 408 || c == 40054005 {
        return if side_effect {
            PortError::OutcomeUnknown
        } else {
            PortError::Unavailable
        };
    }
    match c {
        11253 | 100007 | 100016 | 10004 | 40034105 | 40054002 | 40054003 | 40054016 => {
            PortError::PermissionDenied
        }
        304103 | 40034005 | 40034026 | 40034128 => PortError::Failed {
            code: "qq_reply_expired_or_exhausted",
        },
        _ if status.as_u16() == 401 || status.as_u16() == 403 => PortError::PermissionDenied,
        _ => PortError::Failed {
            code: "qq_request_rejected",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[tokio::test]
    async fn qq_delivery_cache_scope_and_errors() {
        scenario().await;
    }
    async fn scenario() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new()
            .route(
                "/app/getAppAccessToken",
                axum::routing::post(move |Json(v): Json<Value>| {
                    let count = count.clone();
                    async move {
                        assert_eq!(v["appId"], "123456");
                        assert_eq!(v["clientSecret"], "test-only-qq-secret");
                        count.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"access_token":"test-token","expires_in":"7200"}))
                    }
                }),
            )
            .route(
                "/v2/groups/group_A/messages",
                axum::routing::post(
                    |h: axum::http::HeaderMap, Json(v): Json<Value>| async move {
                        assert_eq!(h["authorization"], "QQBot test-token");
                        assert_eq!(v["msg_type"], 0);
                        if v["content"] == "proactive" {
                            assert!(v.get("msg_id").is_none());
                        } else {
                            assert_eq!(v["msg_seq"], 1);
                        }
                        Json(if v["content"] == "expired" {
                            json!({"code":40034005})
                        } else if v["content"] == "limited" {
                            json!({"code":40034100})
                        } else if v["content"] == "duplicate" {
                            json!({"code":40054005})
                        } else {
                            json!({"id":"sent-message"})
                        })
                    },
                ),
            );
        let app = app.route(
            "/v2/users/user_A/messages",
            axum::routing::post(|Json(v): Json<Value>| async move {
                assert_eq!(v["msg_id"], "trusted-reply");
                assert_eq!(v["content"], "direct reply");
                Json(json!({"id":"direct-sent"}))
            }),
        );
        let app = app.route(
            "/v2/groups/group_A/info",
            axum::routing::get(|h: axum::http::HeaderMap| async move {
                assert_eq!(h["authorization"], "QQBot test-token");
                Json(json!({"group_openid":"group_A","group_name":"测试群"}))
            }),
        );
        let app = app.route(
            "/users/@me",
            axum::routing::get(|h: axum::http::HeaderMap| async move {
                assert_eq!(h["authorization"], "QQBot test-token");
                Json(
                    json!({"id":"bot-1","username":"Test Bot","bot":true,"internal":"not-exposed"}),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let scope = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        let id = scope.connection_id;
        let config = QqBotConfig {
            resolved_secret: Some(Arc::from("test-only-qq-secret")),
            app_id: "123456".into(),
            group_openid: "group_A".into(),
            allowed_senders: vec![],
            allow_proactive: false,
        };
        let mut sink = QqBotSink::new(scope, id, &config).unwrap();
        sink.base = base;
        let message = |v: Value| Message {
            destination_binding: None,
            parts: vec![ContentPart::Json(v)],
            reply_route_id: None,
        };
        assert!(sink.validate(&message(json!({"content":"test"}))).is_err());
        assert!(
            sink.validate(&message(
                json!({"content":"test","msg_id":"m","group_openid":"group_B"})
            ))
            .is_err()
        );
        assert!(
            sink.validate(&message(json!({"content":"test","msg_id":"m","msg_seq":6})))
                .is_err()
        );
        let ctx = OperationContext {
            scope,
            principal_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            correlation_id: Uuid::new_v4(),
            causation_id: None,
            route_trace: vec![],
            deadline: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        let request = |content: &str| DeliveryRequest {
            delivery_id: Uuid::new_v4(),
            destination_id: id,
            message: message(json!({"payload":{"content":content,"msg_id":"original-message"}})),
        };
        assert_eq!(sink.group_name("group_A").await.unwrap(), "测试群");
        let (a, b) = tokio::join!(
            sink.deliver(ctx.clone(), request("one")),
            sink.deliver(ctx.clone(), request("two"))
        );
        assert_eq!(
            a.unwrap().external_message_id.as_deref(),
            Some("sent-message")
        );
        assert!(b.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            sink.deliver(ctx.clone(), request("expired")).await.err(),
            Some(PortError::Failed {
                code: "qq_reply_expired_or_exhausted"
            })
        );
        assert!(matches!(
            sink.deliver(ctx.clone(), request("limited")).await,
            Err(PortError::RateLimited { .. })
        ));
        assert!(matches!(
            sink.deliver(ctx.clone(), request("duplicate")).await,
            Err(PortError::OutcomeUnknown)
        ));
        sink.config.group_openid.clear();
        let mut direct = request("direct reply");
        direct.message.destination_binding =
            Some(crate::domain::connection_routing::RouteBinding::new(
                "qq",
                json!({"app_id":"123456","kind":"direct","id":"user_A","msg_id":"trusted-reply"}),
            ));
        let result = sink.deliver(ctx.clone(), direct).await.unwrap();
        assert_eq!(result.external_message_id.as_deref(), Some("direct-sent"));
        use crate::application::ports::connection_actions::ConnectionActions;
        use crate::domain::{connection_routing::RouteBinding, tool::ToolCall};
        sink.config.allow_proactive = true;
        let actions = super::super::actions::QqActions {
            sink: Arc::new(sink),
            destination_id: id,
        };
        let executor = actions
            .executor(Some(RouteBinding::new(
                "qq",
                json!({"app_id":"123456","kind":"direct","id":"user_A","msg_id":"trusted-reply"}),
            )))
            .unwrap();
        let info = executor
            .invoke(
                ctx.clone(),
                ToolCall {
                    invocation_id: Uuid::new_v4(),
                    tool_id: "qq.bot_info".into(),
                    arguments: json!({}),
                },
            )
            .await
            .unwrap();
        let info = info.structured_content.unwrap();
        assert_eq!(info["username"], "Test Bot");
        assert!(info.get("internal").is_none());
        let sent = executor
            .invoke(
                ctx.clone(),
                ToolCall {
                    invocation_id: Uuid::new_v4(),
                    tool_id: "qq.send_text".into(),
                    arguments: json!({"content":"direct reply"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            sent.structured_content.unwrap()["message_id"],
            "direct-sent"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let proactive = actions
            .executor(Some(RouteBinding::new(
                "qq",
                json!({"app_id":"123456","kind":"group","id":"group_A"}),
            )))
            .unwrap();
        let sent = proactive
            .invoke(
                ctx.clone(),
                ToolCall {
                    invocation_id: Uuid::new_v4(),
                    tool_id: "qq.send_text".into(),
                    arguments: json!({"content":"proactive"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            sent.structured_content.unwrap()["message_id"],
            "sent-message"
        );
        assert!(
            executor
                .invoke(
                    ctx,
                    ToolCall {
                        invocation_id: Uuid::new_v4(),
                        tool_id: "qq.send_text".into(),
                        arguments: json!({"content":"direct reply","group_openid":"forged"})
                    }
                )
                .await
                .is_err()
        );
        server.abort();
    }
}
