use serde_json::json;
use std::collections::BTreeMap;
use synquay_server::{
    application::ports::{
        OperationContext, PortError,
        capabilities::{CapabilityCatalog, ResourceCatalog},
    },
    domain::{
        capability::{binding::*, resource::*, *},
        connector::ConnectionScope,
        tool::{ToolDefinition, ToolEffect},
    },
    infrastructure::capabilities::StaticCapabilityCatalog,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn scope() -> ConnectionScope {
    ConnectionScope {
        workspace_id: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
    }
}
fn context() -> OperationContext {
    OperationContext {
        scope: scope(),
        principal_id: Uuid::from_u128(3),
        request_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![],
        deadline: None,
        cancellation: CancellationToken::new(),
    }
}
fn action() -> BoundCapability {
    BoundCapability {
        descriptor: CapabilityDescriptor {
            reference: CapabilityRef {
                scope: scope(),
                id: "heater.power".into(),
                revision: 1,
            },
            origin: DefinitionOrigin::Operator,
            resource: None,
            spec: CapabilitySpec::Action {
                tool: ToolDefinition {
                    id: "heater.power".into(),
                    description: "Set heater power".into(),
                    input_schema: json!({"type":"object","properties":{"power":{"type":"boolean"}},"required":["power"]}),
                    output_schema: None,
                    effect: ToolEffect::SideEffecting,
                    documentation: None,
                },
                execution: ExecutionSemantics {
                    mode: ExecutionMode::Deferred,
                    completion: CompletionCondition::TargetConfirmed,
                    idempotency: Idempotency::Idempotent,
                    timeout_ms: 30_000,
                },
            },
        },
        binding_revision: 1,
        binding: ProtocolBinding::MqttPublish {
            topic: "home/heater/set".into(),
            qos: MqttQos::AtLeastOnce,
            retain: false,
            payload: PayloadMapping(BTreeMap::from([
                (
                    "power".into(),
                    ValueBinding::Argument {
                        pointer: "/power".into(),
                    },
                ),
                ("command_id".into(), ValueBinding::InvocationId),
            ])),
            acknowledgement: Some(MqttAcknowledgement {
                topic: "home/heater/ack".into(),
                correlation_pointer: "/command_id".into(),
                result_pointer: "/status".into(),
                success_value: json!("applied"),
            }),
        },
    }
}

#[test]
fn publication_and_subscription_are_different_capabilities() {
    let mut entry = action();
    assert!(entry.is_valid());
    entry.binding = ProtocolBinding::MqttSubscribe {
        topic_filter: "home/+/state".into(),
        qos: MqttQos::AtLeastOnce,
    };
    assert!(!entry.is_valid());
    entry.descriptor.spec = CapabilitySpec::State {
        description: "Heater state".into(),
        schema: json!({"type":"object"}),
        max_age_ms: 30_000,
    };
    assert!(entry.is_valid());
    assert!(entry.descriptor.tool().is_none());
    for filter in ["home/a+/state", "home/#/state", "$share/group/home/+"] {
        entry.binding = ProtocolBinding::MqttSubscribe {
            topic_filter: filter.into(),
            qos: MqttQos::AtLeastOnce,
        };
        assert!(!entry.is_valid(), "{filter}");
    }
}

#[test]
fn confirmation_requires_correlation_and_never_retained_commands() {
    let mut entry = action();
    if let ProtocolBinding::MqttPublish { retain, .. } = &mut entry.binding {
        *retain = true;
    }
    assert!(!entry.is_valid());
    let mut entry = action();
    if let ProtocolBinding::MqttPublish {
        acknowledgement, ..
    } = &mut entry.binding
    {
        *acknowledgement = None;
    }
    assert!(!entry.is_valid());
    let mut entry = action();
    if let ProtocolBinding::MqttPublish { payload, .. } = &mut entry.binding {
        payload.0.remove("command_id");
    }
    assert!(!entry.is_valid());
    let mut entry = action();
    if let ProtocolBinding::MqttPublish { topic, .. } = &mut entry.binding {
        *topic = "home/+/set".into();
    }
    assert!(!entry.is_valid());
}

#[test]
fn transport_acknowledgement_is_not_device_confirmation() {
    let mut entry = action();
    if let CapabilitySpec::Action { execution, .. } = &mut entry.descriptor.spec {
        execution.completion = CompletionCondition::TransportAccepted;
    }
    assert!(!entry.is_valid());
    if let ProtocolBinding::MqttPublish {
        acknowledgement, ..
    } = &mut entry.binding
    {
        *acknowledgement = None;
    }
    assert!(entry.is_valid());
    if let ProtocolBinding::MqttPublish { qos, .. } = &mut entry.binding {
        *qos = MqttQos::AtMostOnce;
    }
    assert!(!entry.is_valid());
}

#[test]
fn mapping_keeps_json_types_and_missing_arguments_fail_closed() {
    let entry = action();
    let ProtocolBinding::MqttPublish { payload, .. } = entry.binding else {
        panic!()
    };
    let id = Uuid::new_v4();
    assert_eq!(
        payload.render(
            &json!({"power":true,"topic":"evil","command_id":"spoof"}),
            id
        ),
        Some(json!({"power":true,"command_id":id.to_string()}))
    );
    assert!(payload.render(&json!({}), id).is_none());
    assert!(
        payload
            .render(&json!({"power":true}), Uuid::nil())
            .is_none()
    );
    let escaped = ValueBinding::Argument {
        pointer: "/a~1b/~0x".into(),
    };
    assert!(escaped.is_valid());
    assert_eq!(
        escaped.resolve(&json!({"a/b":{"~x":false}}), id),
        Some(json!(false))
    );
    assert!(
        !ValueBinding::Argument {
            pointer: "/bad~2escape".into()
        }
        .is_valid()
    );
}

#[tokio::test]
async fn catalog_is_scoped_versioned_and_does_not_leak_bindings_in_descriptions() {
    let entry = action();
    let key = entry.descriptor.reference.clone();
    let catalog = StaticCapabilityCatalog::build(vec![entry], vec![]).unwrap();
    let descriptor = catalog.descriptor(context(), key.clone()).await.unwrap();
    let again = catalog.descriptor(context(), key.clone()).await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&descriptor, &again));
    let public = serde_json::to_string(&*descriptor).unwrap();
    assert!(!public.contains("home/heater/set"));
    assert!(!public.contains("acknowledgement"));
    let mut other = key.clone();
    other.revision = 2;
    assert!(matches!(
        catalog.binding(context(), other).await,
        Err(PortError::NotFound)
    ));
    let mut ctx = context();
    ctx.scope.workspace_id = Uuid::new_v4();
    assert!(matches!(
        catalog.binding(ctx, key.clone()).await,
        Err(PortError::ScopeMismatch)
    ));
    let ctx = context();
    ctx.cancellation.cancel();
    assert!(matches!(
        catalog.descriptor(ctx, key).await,
        Err(PortError::Cancelled)
    ));
}

#[tokio::test]
async fn resources_must_exist_at_exact_revision_and_in_same_connection() {
    let mut entry = action();
    let resource = ResourceDefinition {
        reference: ResourceRef {
            scope: scope(),
            id: "heater".into(),
            revision: 1,
        },
        resource_type: "appliance".into(),
        display_name: "Heater".into(),
        description: "Main water heater".into(),
    };
    entry.descriptor.resource = Some(resource.reference.clone());
    assert!(StaticCapabilityCatalog::build(vec![entry.clone()], vec![]).is_err());
    let catalog =
        StaticCapabilityCatalog::build(vec![entry.clone()], vec![resource.clone()]).unwrap();
    assert_eq!(
        catalog
            .resource(context(), resource.reference.clone())
            .await
            .unwrap()
            .display_name,
        "Heater"
    );
    let mut ctx = context();
    ctx.scope.connection_id = Uuid::new_v4();
    assert!(matches!(
        catalog.resource(ctx, resource.reference.clone()).await,
        Err(PortError::ScopeMismatch)
    ));
    entry.descriptor.resource.as_mut().unwrap().revision = 2;
    assert!(StaticCapabilityCatalog::build(vec![entry.clone()], vec![resource]).is_err());
    entry
        .descriptor
        .resource
        .as_mut()
        .unwrap()
        .scope
        .workspace_id = Uuid::new_v4();
    assert!(!entry.is_valid());
}

#[test]
fn duplicate_revisions_and_unknown_fields_are_rejected() {
    let entry = action();
    assert!(matches!(
        StaticCapabilityCatalog::build(vec![entry.clone(), entry.clone()], vec![]),
        Err(PortError::AlreadyRegistered)
    ));
    let mut encoded = serde_json::to_value(entry).unwrap();
    encoded["binding"]["arbitrary_script"] = json!("run anything");
    assert!(serde_json::from_value::<BoundCapability>(encoded).is_err());
}

#[test]
fn old_tool_json_round_trips_without_changing_snapshots() {
    let old = json!({"id":"echo","description":"Echo input","input_schema":{"type":"object"},"output_schema":null,"effect":"ReadOnly"});
    let tool: ToolDefinition = serde_json::from_value(old.clone()).unwrap();
    assert!(tool.is_valid());
    assert!(tool.documentation.is_none());
    assert_eq!(serde_json::to_value(tool).unwrap(), old);
}

#[test]
fn protocol_binding_cannot_claim_an_incompatible_completion() {
    let mut entry = action();
    entry.binding = ProtocolBinding::Cli {
        program: "/usr/bin/example".into(),
        argv: vec![],
    };
    assert!(!entry.is_valid());
    if let CapabilitySpec::Action { execution, .. } = &mut entry.descriptor.spec {
        execution.completion = CompletionCondition::ProcessExited;
    }
    assert!(entry.is_valid());
    if let ProtocolBinding::Cli { program, .. } = &mut entry.binding {
        *program = "user-selected-command".into();
    }
    assert!(!entry.is_valid());
}

#[test]
fn checked_in_mqtt_example_builds_and_preserves_state_action_separation() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Example {
        resources: Vec<ResourceDefinition>,
        capabilities: Vec<BoundCapability>,
    }
    let example: Example =
        serde_json::from_str(include_str!("../../examples/capabilities/mqtt-heater.json")).unwrap();
    assert_eq!(example.capabilities.len(), 2);
    assert!(example.capabilities[0].descriptor.tool().is_some());
    assert!(example.capabilities[1].descriptor.tool().is_none());
    StaticCapabilityCatalog::build(example.capabilities, example.resources).unwrap();
}

#[test]
fn documentation_is_bounded_and_does_not_grant_permissions() {
    let mut entry = action();
    let CapabilitySpec::Action { tool, .. } = &mut entry.descriptor.spec else {
        panic!()
    };
    tool.documentation = Some(
        serde_json::from_value(
            json!({"preconditions":["Requires permission to operate the target"]}),
        )
        .unwrap(),
    );
    assert!(tool.agent_description().contains("Requires permission"));
    tool.documentation.as_mut().unwrap().when_to_use = vec!["condition".into(); 9];
    assert!(!entry.is_valid());
}
