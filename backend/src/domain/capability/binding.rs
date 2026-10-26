//! Trusted server-side execution configuration. Never included in MCP descriptions.
use super::{CapabilityDescriptor, CapabilitySpec, CompletionCondition, valid_text};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueBinding {
    Literal { value: Value },
    Argument { pointer: String },
    InvocationId,
}
impl ValueBinding {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Literal { value } => serde_json::to_vec(value).is_ok_and(|v| v.len() <= 8192),
            Self::Argument { pointer } => valid_pointer(pointer),
            Self::InvocationId => true,
        }
    }
    pub fn resolve(&self, arguments: &Value, invocation_id: Uuid) -> Option<Value> {
        match self {
            Self::Literal { value } => Some(value.clone()),
            Self::Argument { pointer } => arguments.pointer(pointer).cloned(),
            Self::InvocationId => Some(Value::String(invocation_id.to_string())),
        }
    }
}

/// Fixed field names and JSON values; never string interpolation or executable templates.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PayloadMapping(pub BTreeMap<String, ValueBinding>);
impl PayloadMapping {
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
            && self.0.len() <= 64
            && self
                .0
                .iter()
                .all(|(k, v)| valid_text(k, 128) && v.is_valid())
    }
    /// The caller still validates the input schema, resource policy and final payload size.
    pub fn render(&self, arguments: &Value, invocation_id: Uuid) -> Option<Value> {
        if !self.is_valid() || invocation_id.is_nil() {
            return None;
        }
        let fields: Option<Map<String, Value>> = self
            .0
            .iter()
            .map(|(key, binding)| Some((key.clone(), binding.resolve(arguments, invocation_id)?)))
            .collect();
        fields.map(Value::Object)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MqttQos {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttAcknowledgement {
    pub topic: String,
    pub correlation_pointer: String,
    pub result_pointer: String,
    pub success_value: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProtocolBinding {
    /// Current HTTP adapter: POST a JSON object and validate a JSON response.
    HttpJsonPost {
        url: String,
        allow_loopback_http: bool,
    },
    /// Each binding yields one argv element. No implicit shell or free-form command line.
    Cli {
        program: String,
        argv: Vec<ValueBinding>,
    },
    MqttPublish {
        topic: String,
        qos: MqttQos,
        retain: bool,
        payload: PayloadMapping,
        acknowledgement: Option<MqttAcknowledgement>,
    },
    MqttSubscribe {
        topic_filter: String,
        qos: MqttQos,
    },
    McpTool {
        upstream_name: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundCapability {
    pub descriptor: CapabilityDescriptor,
    pub binding_revision: u64,
    pub binding: ProtocolBinding,
}
impl BoundCapability {
    pub fn is_valid(&self) -> bool {
        if !self.descriptor.is_valid() || self.binding_revision == 0 {
            return false;
        }
        match (&self.descriptor.spec, &self.binding) {
            (
                CapabilitySpec::Action { execution, .. },
                ProtocolBinding::HttpJsonPost {
                    url,
                    allow_loopback_http,
                },
            ) => {
                execution.completion == CompletionCondition::ResponseValidated
                    && valid_http_url(url, *allow_loopback_http)
            }
            (CapabilitySpec::Action { execution, .. }, ProtocolBinding::Cli { program, argv }) => {
                execution.completion == CompletionCondition::ProcessExited
                    && program.starts_with('/')
                    && valid_text(program, 4096)
                    && argv.len() <= 128
                    && argv.iter().all(ValueBinding::is_valid)
            }
            (
                CapabilitySpec::Action { execution, .. },
                ProtocolBinding::MqttPublish {
                    topic,
                    qos,
                    retain,
                    payload,
                    acknowledgement,
                },
            ) => {
                valid_topic(topic, false) && payload.is_valid()
                    // Retained commands can execute long after the original authority expires.
                    && !retain
                    && match (execution.completion, acknowledgement) {
                        (CompletionCondition::TransportAccepted, None) => *qos != MqttQos::AtMostOnce,
                        (CompletionCondition::TargetConfirmed, Some(ack)) => {
                            valid_topic(&ack.topic, false) && ack.topic != *topic
                                && valid_pointer(&ack.correlation_pointer)
                                && valid_pointer(&ack.result_pointer)
                                && ack.correlation_pointer != ack.result_pointer
                                && !ack.success_value.is_null() && !ack.success_value.is_array() && !ack.success_value.is_object()
                                && payload.0.values().any(|v| matches!(v, ValueBinding::InvocationId))
                        }
                        _ => false,
                    }
            }
            (
                CapabilitySpec::State { .. } | CapabilitySpec::Event { .. },
                ProtocolBinding::MqttSubscribe { topic_filter, .. },
            ) => valid_topic(topic_filter, true),
            (
                CapabilitySpec::Action { execution, .. },
                ProtocolBinding::McpTool { upstream_name },
            ) => {
                execution.completion == CompletionCondition::ResponseValidated
                    && valid_text(upstream_name, 128)
            }
            _ => false,
        }
    }
}

fn valid_http_url(value: &str, allow_loopback: bool) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
    value.len() <= 8192
        && url.host_str().is_some()
        && (url.scheme() == "https" || (allow_loopback && loopback && url.scheme() == "http"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn valid_topic(topic: &str, filter: bool) -> bool {
    if !valid_text(topic, 1024) || topic.starts_with("$share/") {
        return false;
    }
    let levels: Vec<_> = topic.split('/').collect();
    levels.iter().enumerate().all(|(index, level)| {
        if level.contains('#') {
            filter && *level == "#" && index + 1 == levels.len()
        } else if level.contains('+') {
            filter && *level == "+"
        } else {
            true
        }
    })
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.starts_with('/') || pointer.len() > 1024 {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(c) = chars.next() {
        if c == '\0' || (c == '~' && !matches!(chars.next(), Some('0' | '1'))) {
            return false;
        }
    }
    true
}
