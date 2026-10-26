use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture, mcp_content::McpContentProvider,
    },
    domain::mcp_content::*,
};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

pub struct StaticMcpContent {
    workspace: Uuid,
    config: Arc<McpContentConfig>,
    bodies: BTreeMap<(String, Option<String>), Arc<str>>,
}
impl StaticMcpContent {
    pub fn new(workspace: Uuid, config: McpContentConfig) -> Result<Self, PortError> {
        if !config.is_valid() {
            return Err(PortError::InvalidInput {
                code: "invalid_mcp_catalog",
            });
        }
        let mut bodies = BTreeMap::new();
        for r in &config.resources {
            bodies.insert((r.id.clone(), None), Arc::from(r.text.as_str()));
        }
        for r in &config.templates {
            for (key, body) in &r.entries {
                bodies.insert((r.id.clone(), Some(key.clone())), Arc::from(body.as_str()));
            }
        }
        Ok(Self {
            workspace,
            config: Arc::new(config),
            bodies,
        })
    }
    fn check(&self, ctx: &OperationContext) -> Result<(), PortError> {
        ctx.check_active()?;
        if ctx.scope.workspace_id != self.workspace {
            return Err(PortError::ScopeMismatch);
        }
        Ok(())
    }
}
impl McpContentProvider for StaticMcpContent {
    fn catalog(&self) -> Arc<McpContentConfig> {
        self.config.clone()
    }
    fn read<'a>(
        &'a self,
        context: OperationContext,
        id: &'a str,
        key: Option<&'a str>,
    ) -> PortFuture<'a, Arc<str>> {
        Box::pin(async move {
            self.check(&context)?;
            self.bodies
                .get(&(id.into(), key.map(str::to_owned)))
                .cloned()
                .ok_or(PortError::NotFound)
        })
    }
    fn render<'a>(
        &'a self,
        context: OperationContext,
        id: &'a str,
        arguments: &'a BTreeMap<String, String>,
    ) -> PortFuture<'a, Vec<PromptText>> {
        Box::pin(async move {
            self.check(&context)?;
            let p = self
                .config
                .prompts
                .iter()
                .find(|p| p.id == id)
                .ok_or(PortError::NotFound)?;
            if arguments.len() > 8
                || arguments.iter().any(|(k, v)| {
                    v.len() > 4096 || v.contains('\0') || !p.arguments.iter().any(|a| a.name == *k)
                })
                || p.arguments.iter().any(|a| match arguments.get(&a.name) {
                    None => a.required,
                    Some(v) => !a.choices.is_empty() && !a.choices.contains(v),
                })
            {
                return Err(PortError::InvalidInput {
                    code: "invalid_prompt_arguments",
                });
            }
            let mut total = 0;
            let mut messages = Vec::with_capacity(p.messages.len());
            for message in &p.messages {
                let mut input = message.text.as_str();
                let mut output = String::new();
                while let Some(start) = input.find("{{") {
                    output.push_str(&input[..start]);
                    input = &input[start + 2..];
                    let end = input.find("}}").ok_or(PortError::Unavailable)?;
                    if let Some(value) = arguments.get(&input[..end]) {
                        output.push_str(value)
                    }
                    input = &input[end + 2..];
                    // Check each expansion, before potentially large repeated substitutions.
                    if output.len() > 64 * 1024 {
                        return Err(PortError::PayloadTooLarge);
                    }
                }
                output.push_str(input);
                total += output.len();
                if total > 64 * 1024 {
                    return Err(PortError::PayloadTooLarge);
                }
                messages.push(PromptText {
                    role: message.role,
                    text: output,
                });
            }
            Ok(messages)
        })
    }
}
