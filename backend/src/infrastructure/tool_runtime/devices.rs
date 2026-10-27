use super::*;
use crate::domain::{profile::PublishedTool, tool::ToolEffect};
use crate::infrastructure::connectors::{
    message_config::ConnectionConfig, mqtt::actions::MqttActions,
};
use serde_json::{Value, json};
use uuid::Uuid;
pub fn public_name(id: Uuid, tool: &str) -> String {
    format!("mqtt_{}_{}", id.simple(), tool)
}
impl ToolRuntime {
    pub(crate) async fn device_tools(&self) -> Result<Vec<PublishedTool>, PortError> {
        let mut tools = vec![];
        for c in self
            .store
            .device_connections(self.config.workspace_id)
            .await?
        {
            let ConnectionConfig::Mqtt(config) = &c.config else {
                continue;
            };
            for definition in config.capabilities.definitions() {
                tools.push(PublishedTool {
                    public_name: public_name(c.id, &definition.id),
                    scope: ConnectionScope {
                        workspace_id: self.config.workspace_id,
                        connection_id: c.id,
                    },
                    definition,
                });
            }
        }
        Ok(tools)
    }
    pub async fn refresh_device_tools(&self) -> Result<(), PortError> {
        let _guard = self.publication_gate.write().await;
        let _device_guard = self.store.device_io.read().await;
        let connections = self
            .store
            .device_connections(self.config.workspace_id)
            .await?;
        let inventory = self.inventory_snapshot()?;
        let schemas = inventory
            .tools
            .iter()
            .flat_map(|t| {
                std::iter::once(t.definition.input_schema.clone())
                    .chain(t.definition.output_schema.clone())
            })
            .chain(
                connections
                    .iter()
                    .flat_map(|c| match &c.config {
                        ConnectionConfig::Mqtt(m) => m.capabilities.definitions(),
                        _ => vec![],
                    })
                    .map(|d| d.input_schema),
            );
        self.schemas.replace(schema::Schemas::compile(schemas)?)?;
        let mut scopes = self
            .device_scopes
            .lock()
            .map_err(|_| PortError::Unavailable)?;
        let next = connections
            .iter()
            .map(|c| ConnectionScope {
                workspace_id: self.config.workspace_id,
                connection_id: c.id,
            })
            .collect::<std::collections::HashSet<_>>();
        for scope in scopes.difference(&next) {
            self.registry.replace_shared(*scope, None)?;
            self.read_only_tools
                .write()
                .map_err(|_| PortError::Unavailable)?
                .retain(|(s, _)| s != scope);
        }
        *scopes = next;
        for c in connections {
            let ConnectionConfig::Mqtt(config) = &c.config else {
                continue;
            };
            let scope = ConnectionScope {
                workspace_id: self.config.workspace_id,
                connection_id: c.id,
            };
            let definitions = config.capabilities.definitions();
            let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
                scope,
                kind: ConnectorKind::Mqtt,
                display_name: c.name.clone(),
            });
            registration.tools = Some(Arc::new(MqttActions {
                scope,
                owner: c.id,
                config: config.clone(),
                store: self.store.clone(),
            }));
            if self.registry.get(scope).is_ok() {
                self.registry.replace_shared(scope, Some(registration))?;
            } else {
                self.registry.register_shared(registration)?;
            }
            let mut read_only = self
                .read_only_tools
                .write()
                .map_err(|_| PortError::Unavailable)?;
            read_only.retain(|(s, _)| s != &scope);
            for d in definitions {
                if matches!(d.effect, ToolEffect::ReadOnly) {
                    read_only.insert((scope, d.id));
                }
            }
        }
        Ok(())
    }
    pub async fn device_states(&self, id: Uuid) -> Result<Value, PortError> {
        self.workspace_admin(false).await?;
        self.store
            .resource_states(ConnectionScope {
                workspace_id: self.config.workspace_id,
                connection_id: id,
            })
            .await
    }
    pub(crate) async fn device_tool_views(&self) -> Result<Vec<Value>, PortError> {
        use crate::application::ports::profiles::ProfileRepository;
        let profile = self
            .store
            .published(self.config.workspace_id, self.config.profile_id)
            .await?;
        Ok(self.device_tools().await?.into_iter().map(|t|json!({"connection_id":t.scope.connection_id,"public_name":t.public_name,"definition":t.definition,"exposed":profile.tools.iter().any(|p|p.public_name==t.public_name),"protocol":"mqtt"})).collect())
    }
}

pub(super) fn preserve_devices(
    mut profile: crate::domain::profile::PublishedProfile,
    record: &Value,
) -> Result<crate::domain::profile::PublishedProfile, PortError> {
    let old: crate::domain::profile::PublishedProfile =
        serde_json::from_value(record["profile"].clone()).map_err(|_| PortError::Unavailable)?;
    for t in old.tools {
        if t.public_name.starts_with("mqtt_")
            && !profile.tools.iter().any(|x| x.public_name == t.public_name)
        {
            profile.tools.push(t);
        }
    }
    Ok(profile)
}
