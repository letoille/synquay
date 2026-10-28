//! Transport-independent command matching and bounded, data-only payload mapping.
//! A command never grants authority to choose a destination, identity or executable.
use super::event::EventPayload;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_TEXT: usize = 64 * 1024;
const MAX_TEMPLATE: usize = 8 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMapping {
    /// JSON pointer into the adapter's payload. Empty selects a JSON string root.
    pub text_pointer: String,
    pub trigger: String,
    pub mode: CommandMode,
    #[serde(default)]
    pub output: CommandOutput,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandMode {
    Exact,
    /// Requires a whitespace boundary, so /on cannot match /online.
    Prefix,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOutput {
    #[default]
    Original,
    Json {
        event_type: String,
        template: Value,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    InvalidConfiguration,
    MissingValue,
    OutputTooLarge,
}
/// Borrow text while matching; copy only when producing the queued delivery.
pub struct CommandCapture<'a> {
    pub text: &'a str,
    pub arguments: &'a str,
}

fn pointer_valid(pointer: &str) -> bool {
    if pointer.len() > 256 || (!pointer.is_empty() && !pointer.starts_with('/')) {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(c) = chars.next() {
        if c.is_control() || (c == '~' && !matches!(chars.next(), Some('0' | '1'))) {
            return false;
        }
    }
    true
}
impl CommandMapping {
    pub fn validate(&self) -> Result<(), CommandError> {
        if !pointer_valid(&self.text_pointer)
            || self.trigger.is_empty()
            || self.trigger.len() > 128
            || self.trigger.trim() != self.trigger
            || self.trigger.chars().any(char::is_control)
        {
            return Err(CommandError::InvalidConfiguration);
        }
        if let CommandOutput::Json {
            event_type,
            template,
        } = &self.output
        {
            if event_type.is_empty()
                || event_type.len() > 128
                || event_type.chars().any(char::is_control)
                || serde_json::to_vec(template)
                    .map_err(|_| CommandError::InvalidConfiguration)?
                    .len()
                    > MAX_TEMPLATE
            {
                return Err(CommandError::InvalidConfiguration);
            }
            validate_template(template, 0)?;
        }
        Ok(())
    }
    pub fn capture<'a>(&self, payload: &'a EventPayload) -> Option<CommandCapture<'a>> {
        // Fail closed even when a caller did not validate the persisted configuration.
        if self.trigger.is_empty() {
            return None;
        }
        let text = match payload {
            EventPayload::Json(value) => value.pointer(&self.text_pointer)?.as_str()?,
            EventPayload::Message(message) => match message.parts.as_slice() {
                [super::message::ContentPart::Text(text)] => text,
                _ => return None,
            },
            _ => return None,
        };
        if text.len() > MAX_TEXT {
            return None;
        }
        let text = text.trim();
        let arguments = match self.mode {
            CommandMode::Exact if text == self.trigger => "",
            CommandMode::Exact => return None,
            CommandMode::Prefix => {
                let rest = text.strip_prefix(&self.trigger)?;
                if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
                    return None;
                }
                rest.trim()
            }
        };
        Some(CommandCapture { text, arguments })
    }
    pub fn render(
        &self,
        payload: &EventPayload,
        captured: &CommandCapture<'_>,
    ) -> Result<Option<(String, Value)>, CommandError> {
        self.validate()?;
        let CommandOutput::Json {
            event_type,
            template,
        } = &self.output
        else {
            return Ok(None);
        };
        // Evaluation copies values, never interpolates code or recursively evaluates input.
        let mut remaining = MAX_OUTPUT;
        let mapped = render_value(template, payload, captured, &self.trigger, &mut remaining)?;
        if serde_json::to_vec(&mapped)
            .map_err(|_| CommandError::OutputTooLarge)?
            .len()
            > MAX_OUTPUT
        {
            return Err(CommandError::OutputTooLarge);
        }
        Ok(Some((event_type.clone(), mapped)))
    }
}
fn validate_template(value: &Value, depth: usize) -> Result<(), CommandError> {
    if depth > 32 {
        return Err(CommandError::InvalidConfiguration);
    }
    match value {
        Value::Object(fields)
            if fields.contains_key("$command") || fields.contains_key("$payload") =>
        {
            if fields.len() != 1 {
                return Err(CommandError::InvalidConfiguration);
            }
            if let Some(value) = fields.get("$command") {
                if !matches!(value.as_str(), Some("text" | "arguments" | "trigger")) {
                    return Err(CommandError::InvalidConfiguration);
                }
            } else if !fields
                .get("$payload")
                .and_then(Value::as_str)
                .is_some_and(pointer_valid)
            {
                return Err(CommandError::InvalidConfiguration);
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                validate_template(value, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_template(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn render_value(
    template: &Value,
    payload: &EventPayload,
    captured: &CommandCapture<'_>,
    trigger: &str,
    remaining: &mut usize,
) -> Result<Value, CommandError> {
    fn copy(value: &Value, remaining: &mut usize) -> Result<Value, CommandError> {
        let size = serde_json::to_vec(value)
            .map_err(|_| CommandError::OutputTooLarge)?
            .len();
        *remaining = remaining
            .checked_sub(size)
            .ok_or(CommandError::OutputTooLarge)?;
        Ok(value.clone())
    }
    match template {
        Value::Object(fields) if fields.contains_key("$command") => {
            let text = match fields["$command"].as_str() {
                Some("text") => captured.text,
                Some("arguments") => captured.arguments,
                Some("trigger") => trigger,
                _ => return Err(CommandError::InvalidConfiguration),
            };
            *remaining = remaining
                .checked_sub(text.len())
                .ok_or(CommandError::OutputTooLarge)?;
            Ok(Value::String(text.into()))
        }
        Value::Object(fields) if fields.contains_key("$payload") => {
            let EventPayload::Json(value) = payload else {
                return Err(CommandError::MissingValue);
            };
            let pointer = fields["$payload"]
                .as_str()
                .ok_or(CommandError::InvalidConfiguration)?;
            copy(
                value.pointer(pointer).ok_or(CommandError::MissingValue)?,
                remaining,
            )
        }
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.clone(),
                    render_value(value, payload, captured, trigger, remaining)?,
                ))
            })
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(Value::Object),
        Value::Array(values) => values
            .iter()
            .map(|value| render_value(value, payload, captured, trigger, remaining))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        value => copy(value, remaining),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn command(mode: CommandMode, template: Value) -> CommandMapping {
        CommandMapping {
            trigger: "/助手".into(),
            text_pointer: "/content".into(),
            mode,
            output: CommandOutput::Json {
                event_type: "agent.request".into(),
                template,
            },
        }
    }
    #[test]
    fn exact_and_prefix_require_a_complete_command_and_preserve_unicode_arguments() {
        let c = command(CommandMode::Prefix, json!(null));
        for text in ["/助手别名", "请 /助手", "hello"] {
            assert!(
                c.capture(&EventPayload::Json(json!({"content":text})))
                    .is_none()
            );
        }
        let p = EventPayload::Json(json!({"content":"  /助手\t今晚八点开启热水器 🌊  "}));
        assert_eq!(c.capture(&p).unwrap().arguments, "今晚八点开启热水器 🌊");
        assert!(
            command(CommandMode::Exact, json!(null))
                .capture(&p)
                .is_none()
        );
        assert!(
            c.capture(&EventPayload::Json(json!({"content":123})))
                .is_none()
        );
        let mut root = c;
        root.text_pointer.clear();
        assert_eq!(
            root.capture(&EventPayload::Json(json!("/助手")))
                .unwrap()
                .arguments,
            ""
        );
    }
    #[test]
    fn mapping_is_typed_and_incoming_values_are_never_evaluated_as_templates() {
        let c = command(
            CommandMode::Prefix,
            json!({"args":{"$command":"arguments"},"number":{"$payload":"/count"},"untrusted":{"$payload":"/untrusted"},"list":[true,{"$command":"trigger"}]}),
        );
        c.validate().unwrap();
        let p = EventPayload::Json(
            json!({"content":"/助手 do it","count":3,"untrusted":{"$payload":"/secret"},"secret":"must not be substituted"}),
        );
        let (kind, out) = c.render(&p, &c.capture(&p).unwrap()).unwrap().unwrap();
        assert_eq!(kind, "agent.request");
        assert_eq!(
            out,
            json!({"args":"do it","number":3,"untrusted":{"$payload":"/secret"},"list":[true,"/助手"]})
        );
    }
    #[test]
    fn qq_group_name_mapping_keeps_name_or_null() {
        let mapping = command(
            CommandMode::Prefix,
            json!({"group":{"$payload":"/group_name"}}),
        );
        for name in [json!("反馈群"), json!(null)] {
            let payload = EventPayload::Json(json!({"content":"/助手 hi", "group_name":name}));
            let result = mapping
                .render(&payload, &mapping.capture(&payload).unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(result.1["group"], name);
        }
    }
    #[test]
    fn invalid_templates_missing_fields_and_expansion_are_rejected() {
        for template in [
            json!({"$command":"unknown"}),
            json!({"$payload":"not/a/pointer"}),
            json!({"$payload":"/~2"}),
            json!({"$command":"text","extra":true}),
        ] {
            assert!(command(CommandMode::Exact, template).validate().is_err());
        }
        let mut c = command(CommandMode::Prefix, json!({"$payload":"/missing"}));
        let p = EventPayload::Json(json!({"content":"/助手 x"}));
        assert_eq!(
            c.render(&p, &c.capture(&p).unwrap()).unwrap_err(),
            CommandError::MissingValue
        );
        c.output = CommandOutput::Json {
            event_type: "agent.request".into(),
            template: json!([{"$payload":"/large"},{"$payload":"/large"}]),
        };
        let p = EventPayload::Json(json!({"content":"/助手","large":"x".repeat(40000)}));
        assert_eq!(
            c.render(&p, &c.capture(&p).unwrap()).unwrap_err(),
            CommandError::OutputTooLarge
        );
        c.trigger.clear();
        assert!(c.validate().is_err());
        assert!(c.capture(&p).is_none());
    }
}
