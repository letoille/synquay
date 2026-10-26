use super::message_config::*;
use crate::{
    application::ports::{OperationContext, PortError, PortFuture, messages::MessageSink},
    domain::{connector::ConnectionScope, message::*},
};
use std::time::Duration;
use uuid::Uuid;

/// Bounded message serialization, shared by HTTP and MQTT. Payload mode unwraps
/// the routing envelope, not arbitrary nested application fields.
pub fn body(message: &Message, mode: BodyMode) -> Result<Vec<u8>, PortError> {
    if message.reply_route_id.is_some() {
        return Err(PortError::Unsupported);
    }
    let bytes = match message.parts.as_slice() {
        [ContentPart::Json(v)] => serde_json::to_vec(match mode {
            BodyMode::Payload => v.get("payload").unwrap_or(v),
            BodyMode::Event => v,
        })
        .map_err(|_| PortError::Unsupported)?,
        [ContentPart::Text(t)] => t.as_bytes().to_vec(),
        _ => return Err(PortError::Unsupported),
    };
    if bytes.len() > 64 * 1024 {
        return Err(PortError::PayloadTooLarge);
    }
    Ok(bytes)
}
pub struct HttpClientSink {
    client: reqwest::Client,
    url: reqwest::Url,
    method: reqwest::Method,
    mode: BodyMode,
    scope: ConnectionScope,
    id: Uuid,
}
impl HttpClientSink {
    pub fn new(scope: ConnectionScope, id: Uuid, c: &HttpClientConfig) -> Result<Self, PortError> {
        let url = reqwest::Url::parse(&c.url).map_err(|_| invalid("invalid_http_url"))?;
        if c.url.len() > 2048
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !((url.scheme() == "https") || (c.allow_http && url.scheme() == "http"))
        {
            return Err(invalid("invalid_http_url"));
        }
        if !["POST", "PUT", "PATCH"].contains(&c.method.as_str()) {
            return Err(invalid("invalid_http_method"));
        }
        let mut headers = reqwest::header::HeaderMap::new();
        let value = match &c.auth {
            HttpAuth::None => None,
            HttpAuth::Bearer { token_env } => Some(format!("Bearer {}", secret(token_env)?)),
            HttpAuth::Basic {
                username,
                password_env,
            } => {
                use base64::Engine;
                if username.contains(':') || username.len() > 256 {
                    return Err(invalid("invalid_http_username"));
                }
                Some(format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD
                        .encode(format!("{username}:{}", secret(password_env)?))
                ))
            }
        };
        if let Some(v) = value {
            if v.len() > 8192 {
                return Err(invalid("invalid_http_auth"));
            }
            let mut h = reqwest::header::HeaderValue::from_str(&v)
                .map_err(|_| invalid("invalid_http_auth"))?;
            h.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, h);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .default_headers(headers)
            .build()
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self {
            client,
            url,
            method: reqwest::Method::from_bytes(c.method.as_bytes())
                .map_err(|_| invalid("invalid_http_method"))?,
            mode: c.body,
            scope,
            id,
        })
    }
}
impl MessageSink for HttpClientSink {
    fn validate(&self, m: &Message) -> Result<(), PortError> {
        body(m, self.mode).map(|_| ())
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
            let bytes = body(&r.message, self.mode)?;
            let request = self
                .client
                .request(self.method.clone(), self.url.clone())
                .header(
                    "Content-Type",
                    if matches!(r.message.parts.as_slice(), [ContentPart::Text(_)]) {
                        "text/plain; charset=utf-8"
                    } else {
                        "application/json"
                    },
                )
                .header("Idempotency-Key", r.delivery_id.to_string())
                .body(bytes)
                .send();
            let response = tokio::select! {biased;_=ctx.stopped()=>return Err(PortError::OutcomeUnknown),r=request=>r.map_err(|e|if e.is_connect(){PortError::Unavailable}else{PortError::OutcomeUnknown})?};
            let status = response.status();
            if status.is_success() {
                Ok(DeliveryReceipt {
                    delivery_id: r.delivery_id,
                    external_message_id: None,
                    status: DeliveryStatus::Accepted,
                })
            } else if status.as_u16() == 429 {
                Err(PortError::RateLimited {
                    retry_after: Duration::from_secs(
                        response
                            .headers()
                            .get("retry-after")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(60)
                            .clamp(1, 86400),
                    ),
                })
            } else if status.is_server_error() || status.as_u16() == 408 {
                Err(PortError::OutcomeUnknown)
            } else {
                Err(PortError::Failed {
                    code: "http_rejected",
                })
            }
        })
    }
}
