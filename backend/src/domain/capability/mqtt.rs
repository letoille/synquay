//! Operator-defined device capabilities; transport credentials remain on the connection.
use super::binding::PayloadMapping;
use crate::domain::tool::{ToolDefinition, ToolEffect};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttCapabilities {
    #[serde(default)]
    pub tools: Vec<MqttTool>,
    #[serde(default)]
    pub resources: Vec<DeviceResource>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttTool {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub topic: String,
    pub payload: PayloadMapping,
    #[serde(default)]
    pub acknowledgement: Option<DeviceAcknowledgement>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAcknowledgement {
    pub topic: String,
    pub correlation_pointer: String,
    pub result_pointer: String,
    pub expected: Value,
    pub timeout_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceResource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub groups: Vec<StateGroup>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateGroup {
    pub id: String,
    pub name: String,
    pub topic: String,
    #[serde(default)]
    pub identity: Option<FieldMatch>,
    #[serde(default)]
    pub source_time_pointer: Option<String>,
    pub max_age_ms: u64,
    #[serde(default)]
    pub replace: bool,
    pub fields: Vec<StateField>,
    #[serde(default)]
    pub refresh_tool: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMatch {
    pub pointer: String,
    pub value: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateField {
    pub name: String,
    pub pointer: String,
    pub value_type: StateType,
    #[serde(default)]
    pub equals: Option<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateType {
    Boolean,
    Number,
    String,
}
pub fn pointer(s: &str) -> bool {
    if !s.starts_with('/') || s.len() > 512 || s.contains('\0') {
        return false;
    }
    let mut c = s.chars();
    while let Some(v) = c.next() {
        if v == '~' && !matches!(c.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}
fn scalar(v: &Value) -> bool {
    v.is_boolean() || v.is_number() || v.as_str().is_some_and(|s| s.len() <= 1024)
}
impl MqttTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            id: self.id.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            output_schema: None,
            effect: ToolEffect::SideEffecting,
            documentation: None,
        }
    }
}
impl DeviceResource {
    pub fn tool_id(&self) -> String {
        format!("state_{}", self.id)
    }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            id: self.tool_id(),
            description: format!(
                "{}: {} (state, field timestamps and freshness; refresh=true requests the configured device query)",
                self.name, self.description
            ),
            input_schema: json!({"type":"object","properties":{"refresh":{"type":"boolean"},"group":{"type":"string"}},"additionalProperties":false}),
            output_schema: None,
            effect: if self.groups.iter().any(|g| g.refresh_tool.is_some()) {
                ToolEffect::SideEffecting
            } else {
                ToolEffect::ReadOnly
            },
            documentation: None,
        }
    }
}
impl MqttCapabilities {
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(MqttTool::definition)
            .chain(self.resources.iter().map(DeviceResource::definition))
            .collect()
    }
    pub fn validate(&self) -> bool {
        fn valid_topic(s: &str, _filter: bool) -> bool {
            !s.is_empty() && s.len() <= 512 && !s.contains(['\0', '#', '+'])
        }
        use super::binding::ValueBinding;
        let mut ids = std::collections::HashSet::new();
        self.tools.len() + self.resources.len() <= 32
            && self
                .definitions()
                .iter()
                .all(|d| d.is_valid() && d.id.len() <= 48 && ids.insert(d.id.clone()))
            && self.tools.iter().all(|t| {
                valid_topic(&t.topic, false)
                    && t.payload.is_valid()
                    && t.acknowledgement.as_ref().is_none_or(|a| {
                        valid_topic(&a.topic, false)
                            && a.topic != t.topic
                            && pointer(&a.correlation_pointer)
                            && pointer(&a.result_pointer)
                            && scalar(&a.expected)
                            && (100..=15000).contains(&a.timeout_ms)
                            && t.payload
                                .0
                                .values()
                                .any(|v| matches!(v, ValueBinding::InvocationId))
                    })
            })
            && self.resources.iter().all(|r| {
                super::valid_id(&r.id)
                    && r.id.len() <= 40
                    && !r.name.trim().is_empty()
                    && r.name.len() <= 120
                    && r.description.len() <= 1024
                    && !r.groups.is_empty()
                    && r.groups.len() <= 8
                    && {
                        let mut groups = std::collections::HashSet::new();
                        r.groups.iter().all(|g| {
                            super::valid_id(&g.id)
                                && groups.insert(&g.id)
                                && !g.name.is_empty()
                                && g.name.len() <= 120
                                && valid_topic(&g.topic, false)
                                && g.max_age_ms <= 86400000
                                && !g.fields.is_empty()
                                && g.fields.len() <= 32
                                && g.source_time_pointer.as_ref().is_none_or(|s| pointer(s))
                                && g.identity
                                    .as_ref()
                                    .is_none_or(|i| pointer(&i.pointer) && scalar(&i.value))
                                && g.refresh_tool.as_ref().is_none_or(|id| {
                                    self.tools
                                        .iter()
                                        .any(|t| &t.id == id && t.acknowledgement.is_some())
                                })
                                && {
                                    let mut names = std::collections::HashSet::new();
                                    g.fields.iter().all(|f| {
                                        super::valid_id(&f.name)
                                            && names.insert(&f.name)
                                            && pointer(&f.pointer)
                                            && f.equals.as_ref().is_none_or(|v| {
                                                matches!(f.value_type, StateType::Boolean)
                                                    && scalar(v)
                                            })
                                    })
                                }
                        })
                    }
            })
    }
}
