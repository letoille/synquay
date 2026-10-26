//! Borrowed v1 wire view: serialize existing content directly, without a second JSON tree.
use crate::{
    application::ports::PortError,
    domain::message::{ContentPart, DeliveryRequest},
};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use uuid::Uuid;

#[derive(Serialize)]
pub(super) struct HttpMessage<'a> {
    schema_version: u8,
    delivery_id: Uuid,
    parts: HttpParts<'a>,
}

impl<'a> HttpMessage<'a> {
    pub(super) fn new(request: &'a DeliveryRequest) -> Result<Self, PortError> {
        if request.message.reply_route_id.is_some()
            || request.message.parts.iter().any(|p| {
                matches!(
                    p,
                    ContentPart::Attachment { .. } | ContentPart::StoredContent(_)
                )
            })
        {
            return Err(PortError::Unsupported);
        }
        Ok(Self {
            schema_version: 1,
            delivery_id: request.delivery_id,
            parts: HttpParts(&request.message.parts),
        })
    }
}

struct HttpParts<'a>(&'a [ContentPart]);

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HttpPart<'a> {
    Text { text: &'a str },
    Json { value: &'a serde_json::Value },
}

impl Serialize for HttpParts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for part in self.0 {
            match part {
                ContentPart::Text(text) => sequence.serialize_element(&HttpPart::Text { text })?,
                ContentPart::Json(value) => {
                    sequence.serialize_element(&HttpPart::Json { value })?
                }
                ContentPart::Attachment { .. } | ContentPart::StoredContent(_) => {
                    return Err(serde::ser::Error::custom("unsupported attachment"));
                }
            }
        }
        sequence.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::message::Message;
    use serde_json::{Value, json};

    // The previous HTTP encoding path, retained only as a comparison fixture.
    fn previous(request: DeliveryRequest) -> Value {
        let parts: Vec<_> = request
            .message
            .parts
            .into_iter()
            .map(|p| match p {
                ContentPart::Text(text) => json!({"type":"text","text":text}),
                ContentPart::Json(value) => json!({"type":"json","value":value}),
                _ => unreachable!(),
            })
            .collect();
        json!({"schema_version":1,"delivery_id":request.delivery_id,"parts":parts})
    }

    fn fixture() -> DeliveryRequest {
        DeliveryRequest {
            delivery_id: Uuid::from_u128(1),
            destination_id: Uuid::from_u128(2),
            message: Message {
                destination_binding: None,
                parts: vec![
                    ContentPart::Text("引号 \" 与换行\n".into()),
                    ContentPart::Json(
                        json!({"text":"x".repeat(60 * 1024), "nested":[null,true,42,{"k":"v"}]}),
                    ),
                ],
                reply_route_id: None,
            },
        }
    }

    #[test]
    fn borrowed_encoding_preserves_v1_wire_contract() {
        let request = fixture();
        let bytes = serde_json::to_vec(&HttpMessage::new(&request).unwrap()).unwrap();
        let actual: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(actual, previous(request));
    }

    #[test]
    fn reply_route_is_rejected_before_serialization() {
        let mut request = fixture();
        request.message.reply_route_id = Some(Uuid::new_v4());
        assert!(matches!(
            HttpMessage::new(&request),
            Err(PortError::Unsupported)
        ));
    }

    #[test]
    #[ignore = "manual release-mode serialization measurement; no timing assertions"]
    fn measure_http_encoding() {
        use std::{hint::black_box, time::Instant};
        const N: usize = 2000;
        let request = fixture();
        // Both paths pay the same fixture clone outside the measured interval.
        // Production takes an owned request; the optimization is the wire conversion.
        for round in 0..4 {
            for borrowed in if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let mut elapsed = std::time::Duration::ZERO;
                let mut bytes = 0;
                for _ in 0..N {
                    let input = black_box(DeliveryRequest {
                        delivery_id: request.delivery_id,
                        destination_id: request.destination_id,
                        message: request.message.clone(),
                    });
                    let start = Instant::now();
                    let output = if borrowed {
                        let output =
                            serde_json::to_vec(&HttpMessage::new(&input).unwrap()).unwrap();
                        drop(input);
                        output
                    } else {
                        serde_json::to_vec(&previous(input)).unwrap()
                    };
                    elapsed += start.elapsed();
                    bytes += black_box(output).len();
                }
                eprintln!(
                    "round={round} borrowed={borrowed} messages={N} output_bytes={bytes} elapsed_us={}",
                    elapsed.as_micros()
                );
            }
        }
    }
}
