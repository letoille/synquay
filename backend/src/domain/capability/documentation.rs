use super::{valid_id, valid_text};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Operator-reviewed guidance. Never interpreted as a grant or executable instruction.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDocumentation {
    #[serde(default)]
    pub when_to_use: Vec<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub examples: Vec<UsageExample>,
    #[serde(default)]
    pub errors: Vec<ErrorExplanation>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageExample {
    pub description: String,
    pub arguments: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorExplanation {
    pub code: String,
    pub description: String,
    pub recovery: String,
}

impl CapabilityDocumentation {
    pub fn is_valid(&self) -> bool {
        self.when_to_use.len() <= 8
            && self.preconditions.len() <= 8
            && self.examples.len() <= 4
            && self.errors.len() <= 16
            && self
                .when_to_use
                .iter()
                .chain(&self.preconditions)
                .all(|s| valid_text(s, 1024))
            && self
                .examples
                .iter()
                .all(|e| valid_text(&e.description, 1024) && e.arguments.is_object())
            && self.errors.iter().all(|e| {
                valid_id(&e.code)
                    && valid_text(&e.description, 1024)
                    && valid_text(&e.recovery, 1024)
            })
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= 16 * 1024)
    }
    pub fn append_to(&self, text: &mut String) {
        for usage in &self.when_to_use {
            text.push_str(&format!("\nUse when: {usage}"));
        }
        for condition in &self.preconditions {
            text.push_str(&format!("\nPrecondition: {condition}"));
        }
        for example in &self.examples {
            text.push_str(&format!(
                "\nExample — {}: {}",
                example.description, example.arguments
            ));
        }
        for error in &self.errors {
            text.push_str(&format!(
                "\nError {}: {} Recovery: {}",
                error.code, error.description, error.recovery
            ));
        }
    }
}
