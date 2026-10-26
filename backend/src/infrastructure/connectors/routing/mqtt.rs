use super::*;
use crate::infrastructure::connectors::message_config::{MqttConfig, valid_topic};
use serde::{Deserialize, Serialize};

pub struct MqttRouting<'a>(pub &'a MqttConfig);
#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    Default,
    Topic { filter: String },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Default,
    Topic { topic: String },
}
impl ConnectionRoutingAdapter for MqttRouting<'_> {
    fn source_descriptor(&self) -> RoutingDescriptor {
        RoutingDescriptor {
            adapter: "mqtt",
            version: 1,
            modes: vec![
                mode("default", "全部订阅消息", vec![]),
                mode(
                    "topic",
                    "指定 Topic",
                    vec![field("filter", "Topic 筛选", "text", None)],
                ),
            ],
        }
    }
    fn target_descriptor(&self) -> RoutingDescriptor {
        RoutingDescriptor {
            adapter: "mqtt",
            version: 1,
            modes: vec![
                mode("default", "默认发布 Topic", vec![]),
                mode(
                    "topic",
                    "指定 Topic",
                    vec![field("topic", "发布 Topic", "text", None)],
                ),
            ],
        }
    }
    fn validate_source(&self, b: Option<&RouteBinding>) -> Result<(), PortError> {
        let s = self
            .0
            .subscription
            .as_ref()
            .ok_or(invalid("unsupported_route_direction"))?;
        if let Some(b) = b
            && let Source::Topic { filter } = decode(b, "mqtt")?
            && (!valid_topic(&filter, true) || !filter_within(&filter, &s.topic))
        {
            return Err(invalid("mqtt_filter_outside_subscription"));
        }
        Ok(())
    }
    fn validate_target(&self, b: Option<&RouteBinding>, _: bool) -> Result<(), PortError> {
        self.0
            .publish
            .as_ref()
            .ok_or(invalid("unsupported_route_direction"))?;
        if let Some(b) = b
            && let Target::Topic { topic } = decode(b, "mqtt")?
        {
            if !valid_topic(&topic, false) {
                return Err(invalid("invalid_mqtt_publish"));
            }
            if self
                .0
                .subscription
                .as_ref()
                .is_some_and(|s| rumqttc::matches(&topic, &s.topic))
            {
                return Err(invalid("mqtt_topic_loop"));
            }
        }
        Ok(())
    }
    fn matches(&self, b: Option<&RouteBinding>, ctx: Option<&RouteBinding>) -> bool {
        if self.validate_source(b).is_err() {
            return false;
        }
        match b.map(|b| decode::<Source>(b, "mqtt")).transpose() {
            Ok(None | Some(Source::Default)) => true,
            Ok(Some(Source::Topic { filter })) => ctx
                .filter(|c| c.adapter == "mqtt" && c.version == 1)
                .and_then(|c| c.config["topic"].as_str())
                .is_some_and(|t| rumqttc::matches(t, &filter)),
            _ => false,
        }
    }
    fn resolve_target(
        &self,
        b: Option<&RouteBinding>,
        _: &EventEnvelope,
        _: bool,
    ) -> Result<Option<RouteBinding>, PortError> {
        self.validate_target(b, false)?;
        let topic = match b.map(|b| decode::<Target>(b, "mqtt")).transpose()? {
            Some(Target::Topic { topic }) => topic,
            _ => self
                .0
                .publish
                .as_ref()
                .ok_or(PortError::Unsupported)?
                .topic
                .clone(),
        };
        Ok(Some(RouteBinding::new(
            "mqtt",
            json!({"mode":"topic","topic":topic}),
        )))
    }
}
/// Inclusion of MQTT filters, including empty levels and the special $ namespace.
fn filter_within(child: &str, parent: &str) -> bool {
    if child.starts_with('$') && !parent.starts_with('$') {
        return false;
    }
    let c: Vec<_> = child.split('/').collect();
    let p: Vec<_> = parent.split('/').collect();
    let mut i = 0;
    loop {
        match (c.get(i), p.get(i)) {
            (_, Some(&"#")) => return true,
            (None, None) => return true,
            (Some(&"#"), _) | (None, _) | (_, None) => return false,
            (Some(c), Some(p)) if *p == "+" || c == p => i += 1,
            _ => return false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_inclusion() {
        for (c, p, expected) in [
            ("home/+/temp", "home/#", true),
            ("home/#", "home/+", false),
            ("home/a", "home/+", true),
            ("$SYS/#", "#", false),
            ("a", "a/#", true),
            ("a/+", "a/b", false),
            ("a//b", "a/+/b", true),
        ] {
            assert_eq!(filter_within(c, p), expected, "{c} {p}");
        }
    }
}
