use crate::application::ports::{PortError, PortFuture, profiles::SchemaValidator};
use serde_json::Value;
use std::{collections::HashMap, sync::RwLock};

/// Compile trusted schemas once at publication, never retrieve network/file refs.
/// Initial profile supports JSON Schema 2020-12 with local refs only.
pub struct Schemas {
    compiled: RwLock<HashMap<String, jsonschema::Validator>>,
}
impl Schemas {
    pub fn compile(values: impl IntoIterator<Item = Value>) -> Result<Self, PortError> {
        fn check(value: &Value) -> bool {
            match value {
                Value::Object(map) => map.iter().all(|(k, v)| match k.as_str() {
                    "$schema" => v.as_str() == Some("https://json-schema.org/draft/2020-12/schema"),
                    "$ref" | "$dynamicRef" => v.as_str().is_some_and(|s| s.starts_with('#')),
                    "$id" => false,
                    _ => check(v),
                }),
                Value::Array(items) => items.iter().all(check),
                _ => true,
            }
        }
        let mut compiled = HashMap::new();
        for schema in values {
            if !schema.is_object() || !check(&schema) {
                return Err(PortError::InvalidInput {
                    code: "unsupported_schema",
                });
            }
            let validator = jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .build(&schema)
                .map_err(|_| PortError::InvalidInput {
                    code: "invalid_schema",
                })?;
            compiled.insert(schema.to_string(), validator);
        }
        Ok(Self {
            compiled: RwLock::new(compiled),
        })
    }
}
impl Schemas {
    pub fn replace(&self, prepared: Schemas) -> Result<(), PortError> {
        *self.compiled.write().map_err(|_| PortError::Unavailable)? = prepared
            .compiled
            .into_inner()
            .map_err(|_| PortError::Unavailable)?;
        Ok(())
    }
    pub fn extend(&self, prepared: Schemas) -> Result<(), PortError> {
        self.compiled
            .write()
            .map_err(|_| PortError::Unavailable)?
            .extend(
                prepared
                    .compiled
                    .into_inner()
                    .map_err(|_| PortError::Unavailable)?,
            );
        Ok(())
    }
}
impl SchemaValidator for Schemas {
    fn validate<'a>(&'a self, schema: &'a Value, value: &'a Value) -> PortFuture<'a, ()> {
        Box::pin(async move {
            let compiled = self.compiled.read().map_err(|_| PortError::Unavailable)?;
            let validator = compiled
                .get(&schema.to_string())
                .ok_or(PortError::InvalidInput {
                    code: "unpublished_schema",
                })?;
            if validator.is_valid(value) {
                Ok(())
            } else {
                Err(PortError::InvalidInput {
                    code: "schema_validation",
                })
            }
        })
    }
}
