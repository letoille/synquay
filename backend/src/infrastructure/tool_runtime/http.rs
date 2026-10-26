use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture, capabilities::CapabilityCatalog,
        tools::ToolExecutor,
    },
    domain::{
        capability::{CapabilityRef, binding::ProtocolBinding},
        connector::ConnectionScope,
        message::ContentPart,
        tool::{ToolCall, ToolDefinition, ToolResult},
    },
};
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub struct HttpExecutor {
    pub catalog: Arc<dyn CapabilityCatalog>,
    pub reference: CapabilityRef,
    pub scope: ConnectionScope,
    pub credential: Option<String>,
    pub client: reqwest::Client,
    pub slots: Arc<Semaphore>,
}
impl ToolExecutor for HttpExecutor {
    fn discover(&self, ctx: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope {
                return Err(PortError::ScopeMismatch);
            }
            let descriptor = self.catalog.descriptor(ctx, self.reference.clone()).await?;
            Ok(vec![
                descriptor.tool().ok_or(PortError::Unsupported)?.clone(),
            ])
        })
    }
    fn invoke(&self, ctx: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope || call.tool_id != self.reference.id {
                return Err(PortError::ScopeMismatch);
            }
            let binding = self
                .catalog
                .binding(ctx.clone(), self.reference.clone())
                .await?;
            let ProtocolBinding::HttpJsonPost { url, .. } = &binding.binding else {
                return Err(PortError::Unsupported);
            };
            let _permit =
                self.slots
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| PortError::RateLimited {
                        retry_after: Duration::from_secs(1),
                    })?;
            let body =
                serde_json::to_vec(&call.arguments).map_err(|_| PortError::InvalidInput {
                    code: "invalid_arguments",
                })?;
            if body.len() > 64 * 1024 {
                return Err(PortError::PayloadTooLarge);
            }
            let mut request = self
                .client
                .post(url)
                .header("content-type", "application/json")
                .header("x-synquay-invocation-id", call.invocation_id.to_string())
                .body(body);
            if let Some(secret) = &self.credential {
                request = request.bearer_auth(secret);
            }
            // After dispatch every transport/cancellation failure may hide a side effect.
            tokio::select! {
                _=ctx.stopped()=>Err(PortError::OutcomeUnknown),
                result=async {
                    let response=request.send().await.map_err(|_|PortError::OutcomeUnknown)?;
                    if !response.status().is_success() {return Err(PortError::OutcomeUnknown);}
                    if response.content_length().is_some_and(|n|n>64*1024) {return Err(PortError::OutcomeUnknown);}
                    let mut body=Vec::new();let mut stream=response.bytes_stream();
                    while let Some(chunk)=stream.next().await {
                        let chunk=chunk.map_err(|_|PortError::OutcomeUnknown)?;
                        if body.len()+chunk.len()>64*1024 {return Err(PortError::OutcomeUnknown);}
                        body.extend_from_slice(&chunk);
                    }
                    let value:serde_json::Value=serde_json::from_slice(&body).map_err(|_|PortError::OutcomeUnknown)?;
                    if !value.is_object() {return Err(PortError::OutcomeUnknown);}
                    Ok(ToolResult {content:vec![ContentPart::Json(value.clone())],structured_content:Some(value),is_error:false})
                }=>result
            }
        })
    }
}
