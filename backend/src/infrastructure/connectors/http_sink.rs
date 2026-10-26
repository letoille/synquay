use super::http_message::HttpMessage;
use crate::{
    application::ports::{OperationContext, PortError, PortFuture, messages::MessageSink},
    domain::{connector::ConnectionScope, message::*},
};
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub struct HttpSink {
    client: Client,
    url: Url,
    scope: ConnectionScope,
    destination_id: Uuid,
    content: Option<std::sync::Arc<crate::infrastructure::content::FileContentStore>>,
}
impl HttpSink {
    pub fn with_content(
        mut self,
        content: std::sync::Arc<crate::infrastructure::content::FileContentStore>,
    ) -> Self {
        self.content = Some(content);
        self
    }

    /// Target URL is trusted operator configuration, never an event or tool argument.
    pub fn new(
        scope: ConnectionScope,
        destination_id: Uuid,
        url: &str,
        bearer: Option<&str>,
        allow_loopback_http: bool,
    ) -> Result<Self, PortError> {
        let invalid = || PortError::InvalidInput {
            code: "invalid_http_destination",
        };
        let url = Url::parse(url).map_err(|_| invalid())?;
        let loopback = url
            .host_str()
            .and_then(|h| h.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || !(url.scheme() == "https"
                || (allow_loopback_http && url.scheme() == "http" && loopback))
        {
            return Err(invalid());
        }
        let mut headers = HeaderMap::new();
        if let Some(token) = bearer {
            if token.is_empty()
                || token.len() > 4096
                || !token.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(invalid());
            }
            let mut value =
                HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| invalid())?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .default_headers(headers)
            .build()
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self {
            client,
            url,
            scope,
            destination_id,
            content: None,
        })
    }
}
impl MessageSink for HttpSink {
    fn validate(&self, message: &Message) -> Result<(), PortError> {
        if message.reply_route_id.is_some() {
            return Err(PortError::Unsupported);
        }
        if matches!(message.parts.as_slice(), [ContentPart::StoredContent(_)]) {
            return self
                .content
                .as_ref()
                .map(|_| ())
                .ok_or(PortError::Unsupported);
        }
        if message.parts.iter().any(|p| {
            matches!(
                p,
                ContentPart::StoredContent(_) | ContentPart::Attachment { .. }
            )
        }) {
            Err(PortError::Unsupported)
        } else {
            Ok(())
        }
    }

    fn deliver(
        &self,
        context: OperationContext,
        request: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            context.check_active()?;
            if context.scope != self.scope || request.destination_id != self.destination_id {
                return Err(PortError::ScopeMismatch);
            }

            let timeout = context.deadline.map_or(Duration::from_secs(15), |d| {
                d.saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(15))
            });
            if timeout.is_zero() {
                return Err(PortError::DeadlineExceeded);
            }
            // Await the request to a definite transport result; cancellation after dispatch is not rollback.
            let builder = self
                .client
                .post(self.url.clone())
                .timeout(timeout)
                .header("Idempotency-Key", request.delivery_id.to_string());
            let builder =
                if let [ContentPart::StoredContent(reference)] = request.message.parts.as_slice() {
                    use crate::application::ports::content::ContentStore;
                    if request.message.reply_route_id.is_some() {
                        return Err(PortError::Unsupported);
                    }
                    let store = self.content.as_ref().ok_or(PortError::Unsupported)?;
                    let input = store.read(context.clone(), reference.clone()).await?;
                    builder
                        .header("Content-Type", &reference.media_type)
                        .header("Content-Length", reference.size)
                        .header("X-Synquay-Schema-Version", "2")
                        .header("X-Synquay-Content-Sha256", &reference.sha256)
                        .header("X-Synquay-Content-Id", reference.id.to_string())
                        .body(reqwest::Body::wrap_stream(
                            tokio_util::io::ReaderStream::new(input),
                        ))
                } else {
                    builder.json(&HttpMessage::new(&request)?)
                };
            let response = builder.send().await.map_err(|error| {
                if error.is_connect() {
                    PortError::Unavailable
                } else {
                    PortError::OutcomeUnknown
                }
            })?;
            let status = response.status();
            if status.is_success() {
                Ok(DeliveryReceipt {
                    delivery_id: request.delivery_id,
                    external_message_id: None,
                    status: DeliveryStatus::Accepted,
                })
            } else if status.as_u16() == 429 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(60)
                    .min(86400);
                Err(PortError::RateLimited {
                    retry_after: Duration::from_secs(delay),
                })
            } else if status.is_server_error() || status.as_u16() == 408 {
                Err(PortError::OutcomeUnknown)
            } else {
                Err(PortError::Failed {
                    code: "http_rejected",
                })
            }
            // Do not retain/log upstream response bodies; 2xx means acceptance, not recipient delivery.
        })
    }
}
