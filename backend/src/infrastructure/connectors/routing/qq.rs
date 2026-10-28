use super::*;
use crate::infrastructure::connectors::qq::{QqBotConfig, openid};
use serde::Deserialize;
pub struct QqRouting<'a>(pub &'a QqBotConfig);
#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    All,
    Groups { ids: Vec<String> },
    Direct { ids: Vec<String> },
    AllGroups,
    AllDirect,
}
#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Default,
    Group { id: String },
    Direct { id: String },
    Reply,
}
fn ids_valid(ids: &[String]) -> bool {
    !ids.is_empty()
        && ids.len() <= 100
        && ids.iter().all(|id| openid(id))
        && ids.iter().collect::<std::collections::HashSet<_>>().len() == ids.len()
}
impl ConnectionRoutingAdapter for QqRouting<'_> {
    fn supports_reply(&self) -> bool {
        true
    }
    fn validate_command_senders(&self, senders: &[String]) -> Result<(), PortError> {
        if senders.is_empty() || ids_valid(senders) {
            Ok(())
        } else {
            Err(invalid("invalid_command_action"))
        }
    }
    fn authenticated_sender(
        &self,
        context: Option<&RouteBinding>,
    ) -> Result<crate::application::ports::connection_routing::ConnectionSender, PortError> {
        let c = context
            .filter(|c| c.adapter == "qq" && c.version == 1 && c.config["app_id"] == self.0.app_id)
            .ok_or(PortError::PermissionDenied)?;
        let kind = c.config["kind"]
            .as_str()
            .filter(|k| ["group", "direct"].contains(k))
            .ok_or(PortError::PermissionDenied)?;
        let sender = c.config["sender_openid"]
            .as_str()
            .filter(|s| openid(s))
            .ok_or(PortError::PermissionDenied)?;
        let conversation = c.config["id"]
            .as_str()
            .filter(|s| openid(s))
            .ok_or(PortError::PermissionDenied)?;
        if (kind == "direct" && conversation != sender)
            || (!self.0.group_openid.is_empty()
                && (kind != "group" || conversation != self.0.group_openid))
        {
            return Err(PortError::PermissionDenied);
        }
        Ok(
            crate::application::ports::connection_routing::ConnectionSender {
                namespace: format!("qq:{}:{kind}", self.0.app_id),
                id: sender.into(),
                allowed: self.0.allowed_senders.is_empty()
                    || self.0.allowed_senders.iter().any(|s| s == sender),
            },
        )
    }
    fn command_reply(
        &self,
        event: &EventEnvelope,
        text: String,
        consumed_target: Option<&RouteBinding>,
    ) -> Result<crate::domain::message::Message, PortError> {
        let binding = self.resolve_target(
            Some(&RouteBinding::new("qq", json!({"mode":"reply"}))),
            event,
            true,
        )?;
        let seq = if consumed_target
            .zip(binding.as_ref())
            .is_some_and(|(a, b)| a == b)
        {
            2
        } else {
            1
        };
        Ok(crate::domain::message::Message {
            destination_binding: binding,
            parts: vec![crate::domain::message::ContentPart::Json(
                json!({"content":text,"msg_seq":seq}),
            )],
            reply_route_id: None,
        })
    }
    fn discovery_account(&self) -> Option<String> {
        Some(self.0.app_id.clone())
    }
    fn source_descriptor(&self) -> RoutingDescriptor {
        RoutingDescriptor {
            adapter: "qq",
            version: 1,
            modes: vec![
                mode(
                    "groups",
                    "指定群",
                    vec![field("ids", "群", "conversations", Some("group"))],
                ),
                mode(
                    "direct",
                    "指定单聊",
                    vec![field("ids", "单聊", "conversations", Some("direct"))],
                ),
                mode("all_groups", "所有群", vec![]),
                mode("all_direct", "所有单聊", vec![]),
                mode("all", "全部会话", vec![]),
            ],
        }
    }
    fn target_descriptor(&self) -> RoutingDescriptor {
        let mut modes = vec![
            mode("reply", "回复来源会话", vec![]),
            mode(
                "group",
                "指定群",
                vec![field("id", "群", "conversation", Some("group"))],
            ),
            mode(
                "direct",
                "指定单聊",
                vec![field("id", "单聊", "conversation", Some("direct"))],
            ),
        ];
        if !self.0.group_openid.is_empty() {
            modes.push(mode("default", "连接默认群", vec![]));
        }
        RoutingDescriptor {
            adapter: "qq",
            version: 1,
            modes,
        }
    }
    fn validate_source(&self, b: Option<&RouteBinding>) -> Result<(), PortError> {
        let Some(b) = b else {
            return if self.0.group_openid.is_empty() {
                Err(invalid("route_source_selection_required"))
            } else {
                Ok(())
            };
        };
        match decode::<Source>(b, "qq")? {
            Source::Groups { ids }
                if !ids_valid(&ids)
                    || (!self.0.group_openid.is_empty()
                        && ids.iter().any(|i| i != &self.0.group_openid)) =>
            {
                Err(invalid("invalid_qq_route_scope"))
            }
            Source::Direct { ids } if !ids_valid(&ids) || !self.0.group_openid.is_empty() => {
                Err(invalid("invalid_qq_route_scope"))
            }
            Source::AllDirect if !self.0.group_openid.is_empty() => {
                Err(invalid("invalid_qq_route_scope"))
            }
            _ => Ok(()),
        }
    }
    fn validate_target(&self, b: Option<&RouteBinding>, same: bool) -> Result<(), PortError> {
        match b
            .map(|b| decode::<Target>(b, "qq"))
            .transpose()?
            .unwrap_or(Target::Default)
        {
            Target::Default if self.0.group_openid.is_empty() => {
                Err(invalid("route_target_selection_required"))
            }
            Target::Reply if !same => Err(invalid("reply_requires_same_connection")),
            Target::Group { id }
                if !openid(&id)
                    || (!self.0.group_openid.is_empty() && self.0.group_openid != id) =>
            {
                Err(invalid("invalid_qq_route_scope"))
            }
            Target::Direct { id } if !openid(&id) || !self.0.group_openid.is_empty() => {
                Err(invalid("invalid_qq_route_scope"))
            }
            _ => Ok(()),
        }
    }
    fn matches(&self, b: Option<&RouteBinding>, ctx: Option<&RouteBinding>) -> bool {
        if self.validate_source(b).is_err() {
            return false;
        }
        // Legacy fixed-group routes predate trusted context; their adapter already restricts the group.
        if b.is_none() {
            return !self.0.group_openid.is_empty();
        }
        let Some(ctx) = ctx
            .filter(|c| c.adapter == "qq" && c.version == 1 && c.config["app_id"] == self.0.app_id)
        else {
            return false;
        };
        let kind = ctx.config["kind"].as_str().unwrap_or("");
        let id = ctx.config["id"].as_str().unwrap_or("");
        match decode::<Source>(b.unwrap(), "qq") {
            Ok(Source::All) => true,
            Ok(Source::AllGroups) => kind == "group",
            Ok(Source::AllDirect) => kind == "direct",
            Ok(Source::Groups { ids }) => kind == "group" && ids.iter().any(|i| i == id),
            Ok(Source::Direct { ids }) => kind == "direct" && ids.iter().any(|i| i == id),
            _ => false,
        }
    }
    fn resolve_target(
        &self,
        b: Option<&RouteBinding>,
        e: &EventEnvelope,
        same: bool,
    ) -> Result<Option<RouteBinding>, PortError> {
        self.validate_target(b, same)?;
        if b.is_none() && !self.0.group_openid.is_empty() {
            return Ok(None);
        }
        let (kind, id) = match b
            .map(|b| decode::<Target>(b, "qq"))
            .transpose()?
            .unwrap_or(Target::Default)
        {
            Target::Default => ("group", self.0.group_openid.clone()),
            Target::Group { id } => ("group", id),
            Target::Direct { id } => ("direct", id),
            Target::Reply => {
                let c = e
                    .route_context
                    .as_ref()
                    .filter(|c| {
                        c.adapter == "qq" && c.version == 1 && c.config["app_id"] == self.0.app_id
                    })
                    .ok_or(PortError::ScopeMismatch)?;
                let kind = match c.config["kind"].as_str() {
                    Some("group") => "group",
                    Some("direct") => "direct",
                    _ => return Err(PortError::ScopeMismatch),
                };
                (
                    kind,
                    c.config["id"]
                        .as_str()
                        .filter(|id| openid(id))
                        .ok_or(PortError::ScopeMismatch)?
                        .into(),
                )
            }
        };
        // Bind passive reply IDs only to the authenticated original conversation.
        let reply = e
            .route_context
            .as_ref()
            .filter(|c| {
                same && c.adapter == "qq"
                    && c.config["app_id"] == self.0.app_id
                    && c.config["kind"] == kind
                    && c.config["id"] == id
            })
            .and_then(|c| c.config["msg_id"].as_str());
        if reply.is_none() && !self.0.allow_proactive {
            return Err(PortError::PermissionDenied);
        }
        Ok(Some(RouteBinding::new(
            "qq",
            json!({"app_id":self.0.app_id,"kind":kind,"id":id,"msg_id":reply}),
        )))
    }
}
