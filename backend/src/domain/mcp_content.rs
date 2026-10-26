//! MCP-facing content contracts, independent of the protocol SDK and physical device resources.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpContentConfig {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub resources: Vec<ResourceDefinition>,
    #[serde(default)]
    pub templates: Vec<ResourceTemplateDefinition>,
    #[serde(default)]
    pub prompts: Vec<PromptDefinition>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDefinition {
    pub id: String,
    pub description: String,
    pub mime_type: String,
    pub text: String,
    #[serde(default)]
    pub exposed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceTemplateDefinition {
    pub id: String,
    pub description: String,
    pub mime_type: String,
    pub parameter: String,
    /// Finite operator-reviewed values. Reading never resolves a URL or filesystem path.
    pub entries: BTreeMap<String, String>,
    #[serde(default)]
    pub exposed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptDefinition {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub exposed: bool,
    #[serde(default)]
    pub arguments: Vec<PromptArgument>,
    pub messages: Vec<PromptText>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptRole {
    User,
    Assistant,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptText {
    pub role: PromptRole,
    pub text: String,
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn description(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 2048 && !s.contains('\0')
}
fn mime(s: &str) -> bool {
    matches!(s, "text/plain" | "text/markdown" | "application/json")
}
fn resource_text(mime: &str, s: &str) -> bool {
    text(s) && (mime != "application/json" || serde_json::from_str::<serde_json::Value>(s).is_ok())
}
fn text(s: &str) -> bool {
    s.len() <= 32 * 1024 && !s.contains('\0')
}
impl McpContentConfig {
    pub fn is_valid(&self) -> bool {
        if self.resources.len() + self.templates.len() > 32
            || self.prompts.len() > 32
            || self.revision > i64::MAX as u64
            || (self.revision == 0
                && (!self.resources.is_empty()
                    || !self.templates.is_empty()
                    || !self.prompts.is_empty()))
        {
            return false;
        }
        let mut resource_ids = HashSet::new();
        let mut prompt_ids = HashSet::new();
        self.resources.iter().all(|r| {
            valid_name(&r.id)
                && resource_ids.insert(&r.id)
                && description(&r.description)
                && mime(&r.mime_type)
                && resource_text(&r.mime_type, &r.text)
        }) && self.templates.iter().all(|r| {
            valid_name(&r.id)
                && resource_ids.insert(&r.id)
                && description(&r.description)
                && mime(&r.mime_type)
                && valid_name(&r.parameter)
                && !r.entries.is_empty()
                && r.entries.len() <= 100
                && r.entries
                    .iter()
                    .all(|(k, v)| valid_name(k) && resource_text(&r.mime_type, v))
        }) && self.prompts.iter().all(|p| {
            let mut args = HashSet::new();
            valid_name(&p.id)
                && prompt_ids.insert(&p.id)
                && description(&p.description)
                && p.arguments.len() <= 8
                && p.arguments.iter().all(|a| {
                    valid_name(&a.name)
                        && args.insert(a.name.as_str())
                        && description(&a.description)
                        && a.choices.len() <= 100
                        && a.choices
                            .iter()
                            .all(|v| !v.is_empty() && v.len() <= 256 && !v.contains('\0'))
                })
                && !p.messages.is_empty()
                && p.messages.len() <= 8
                && p.messages
                    .iter()
                    .all(|m| text(&m.text) && placeholders_valid(&m.text, &args))
        }) && serde_json::to_vec(self).is_ok_and(|v| v.len() <= 64 * 1024)
    }
}
fn placeholders_valid(mut text: &str, args: &HashSet<&str>) -> bool {
    while let Some(start) = text.find("{{") {
        text = &text[start + 2..];
        let Some(end) = text.find("}}") else {
            return false;
        };
        if !args.contains(&text[..end]) {
            return false;
        }
        text = &text[end + 2..];
    }
    true
}
