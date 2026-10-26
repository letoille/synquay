//! QQ operations share the authenticated client and token cache with message delivery.
use super::QqBotSink;
use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture,
        connection_actions::{ConnectionAction, ConnectionActions},
        messages::MessageSink,
        tools::ToolExecutor,
    },
    domain::{connection_routing::RouteBinding, message::*, tool::*},
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            id: "qq.bot_info".into(),
            description: "查询机器人信息".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            output_schema: None,
            effect: ToolEffect::ReadOnly,
            documentation: None,
        },
        ToolDefinition {
            id: "qq.send_text".into(),
            description: "发送文本消息".into(),
            input_schema: json!({"type":"object","properties":{"content":{"type":"string","minLength":1,"maxLength":4000}},"required":["content"],"additionalProperties":false}),
            output_schema: None,
            effect: ToolEffect::SideEffecting,
            documentation: None,
        },
    ]
}
pub fn catalog() -> Vec<ConnectionAction> {
    definitions()
        .into_iter()
        .map(|definition| ConnectionAction {
            target_required: definition.id == "qq.send_text",
            consumes_reply: definition.id == "qq.send_text",
            revision: 1,
            definition,
        })
        .collect()
}
pub struct QqActions {
    pub sink: Arc<QqBotSink>,
    pub destination_id: Uuid,
}
impl ConnectionActions for QqActions {
    fn definitions(&self) -> Vec<ConnectionAction> {
        catalog()
    }
    fn executor(&self, target: Option<RouteBinding>) -> Result<Arc<dyn ToolExecutor>, PortError> {
        Ok(Arc::new(BoundQqActions {
            sink: self.sink.clone(),
            destination_id: self.destination_id,
            target,
        }))
    }
}
struct BoundQqActions {
    sink: Arc<QqBotSink>,
    destination_id: Uuid,
    target: Option<RouteBinding>,
}
impl ToolExecutor for BoundQqActions {
    fn discover(&self, _ctx: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async { Ok(definitions()) })
    }
    fn invoke(&self, ctx: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            ctx.check_active()?;
            match call.tool_id.as_str() {
                "qq.bot_info" => {
                    if call.arguments != json!({}) {
                        return Err(PortError::InvalidInput {
                            code: "invalid_arguments",
                        });
                    }
                    let value = self.sink.bot_info(&ctx).await?;
                    let text = format!(
                        "{} ({})",
                        value["username"].as_str().unwrap_or("QQ Bot"),
                        value["id"].as_str().unwrap_or("")
                    );
                    Ok(ToolResult {
                        content: vec![ContentPart::Text(text)],
                        structured_content: Some(value),
                        is_error: false,
                    })
                }
                "qq.send_text" => {
                    let content =
                        call.arguments["content"]
                            .as_str()
                            .ok_or(PortError::InvalidInput {
                                code: "invalid_arguments",
                            })?;
                    if call.arguments.as_object().is_none_or(|o| o.len() != 1) {
                        return Err(PortError::InvalidInput {
                            code: "invalid_arguments",
                        });
                    }
                    let message = Message {
                        destination_binding: self.target.clone(),
                        parts: vec![ContentPart::Text(content.into())],
                        reply_route_id: None,
                    };
                    self.sink.validate(&message)?;
                    let receipt = self
                        .sink
                        .deliver(
                            ctx,
                            DeliveryRequest {
                                delivery_id: call.invocation_id,
                                destination_id: self.destination_id,
                                message,
                            },
                        )
                        .await?;
                    Ok(ToolResult {
                        content: vec![ContentPart::Text("消息已提交。".into())],
                        structured_content: Some(
                            json!({"message_id":receipt.external_message_id,"status":"accepted"}),
                        ),
                        is_error: false,
                    })
                }
                _ => Err(PortError::NotFound),
            }
        })
    }
}
