//! One capability description per connection. Discovery describes, but does not authorize.
use crate::{
    application::ports::connection_actions::{
        ConnectionAction, ConnectionCapabilityCatalog, ConnectionCapabilitySet,
        MessageCapabilities, StateFieldInfo, StateGroupInfo, StateResourceInfo, StateValueType,
    },
    infrastructure::connectors::message_config::{ConnectionConfig, MessageConnection},
};

pub fn operations(config: &ConnectionConfig) -> Vec<ConnectionAction> {
    match config {
        ConnectionConfig::Mqtt(config) => super::mqtt::actions::catalog(config),
        ConnectionConfig::QqBot(_) => super::qq::actions::catalog(),
        _ => vec![],
    }
}
impl ConnectionCapabilityCatalog for MessageConnection {
    fn capabilities(&self) -> ConnectionCapabilitySet {
        let all = operations(&self.config);
        let resources = match &self.config {
            ConnectionConfig::Mqtt(config) => config
                .capabilities
                .resources
                .iter()
                .map(|resource| StateResourceInfo {
                    id: resource.id.clone(),
                    name: resource.name.clone(),
                    description: resource.description.clone(),
                    query_operation: all
                        .iter()
                        .find(|action| action.definition.id == resource.tool_id())
                        .cloned(),
                    groups: resource
                        .groups
                        .iter()
                        .map(|group| StateGroupInfo {
                            id: group.id.clone(),
                            name: group.name.clone(),
                            max_age_ms: group.max_age_ms,
                            fields: group
                                .fields
                                .iter()
                                .map(|field| StateFieldInfo {
                                    name: field.name.clone(),
                                    value_type: match field.value_type {
                                        crate::domain::capability::mqtt::StateType::Boolean => {
                                            StateValueType::Boolean
                                        }
                                        crate::domain::capability::mqtt::StateType::Number => {
                                            StateValueType::Number
                                        }
                                        crate::domain::capability::mqtt::StateType::String => {
                                            StateValueType::String
                                        }
                                    },
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            _ => vec![],
        };
        let resource_queries = resources
            .iter()
            .filter_map(|resource: &StateResourceInfo| resource.query_operation.as_ref())
            .map(|action| action.definition.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let actions = all
            .iter()
            .filter(|action| !resource_queries.contains(action.definition.id.as_str()))
            .cloned()
            .collect();
        ConnectionCapabilitySet {
            operations: actions,
            resources,
            messages: MessageCapabilities {
                receive: self.receives(),
                send: self.sends(),
                recent_text_cache: matches!(self.config, ConnectionConfig::QqBot(_)),
            },
        }
    }
}
