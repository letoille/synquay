use super::{QqBotConfig, openid};
use crate::{
    application::ports::PortError,
    domain::event::{EventPayload, IncomingEvent},
    infrastructure::{
        connectors::message_config::invalid,
        pipeline::webhook::{fresh_timestamp, valid_event_id},
    },
};
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde_json::{Value, json};
use std::time::SystemTime;

pub enum Callback {
    Challenge(Value),
    Ignore,
    Event(Box<IncomingEvent>),
}
fn key(secret: &str) -> Result<Ed25519KeyPair, PortError> {
    if secret.is_empty() {
        return Err(invalid("invalid_qq_secret"));
    }
    let seed: Vec<u8> = secret.bytes().cycle().take(32).collect();
    Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| invalid("invalid_qq_secret"))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() != 128 || !s.is_ascii() {
        return None;
    }
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            let a = (b[0] as char).to_digit(16)?;
            let c = (b[1] as char).to_digit(16)?;
            Some((a * 16 + c) as u8)
        })
        .collect()
}
/// Timestamp and exact wire body are signed. Challenge is the sole unsigned opcode
/// and can only sign bounded numeric timestamp + alphanumeric challenge tokens.
pub fn callback(
    c: &QqBotConfig,
    app_id: &str,
    timestamp: &str,
    signature: &str,
    bytes: &[u8],
    now: SystemTime,
) -> Result<Callback, PortError> {
    if app_id != c.app_id {
        return Err(PortError::PermissionDenied);
    }
    if bytes.len() > 64 * 1024 {
        return Err(PortError::PayloadTooLarge);
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| invalid("invalid_qq_callback"))?;
    let key = key(&c.secret()?)?;
    if v["op"].as_u64() == Some(13) {
        let token = v["d"]["plain_token"]
            .as_str()
            .filter(|s| openid(s))
            .ok_or(invalid("invalid_qq_challenge"))?;
        let ts = v["d"]["event_ts"]
            .as_str()
            .filter(|s| fresh_timestamp(s, now))
            .ok_or(PortError::PermissionDenied)?;
        return Ok(Callback::Challenge(
            json!({"plain_token":token,"signature":hex(key.sign(format!("{ts}{token}").as_bytes()).as_ref())}),
        ));
    }
    if !fresh_timestamp(timestamp, now) {
        return Err(PortError::PermissionDenied);
    }
    let sig = unhex(signature).ok_or(PortError::PermissionDenied)?;
    let mut signed = Vec::with_capacity(timestamp.len() + bytes.len());
    signed.extend_from_slice(timestamp.as_bytes());
    signed.extend_from_slice(bytes);
    UnparsedPublicKey::new(&ED25519, key.public_key().as_ref())
        .verify(&signed, &sig)
        .map_err(|_| PortError::PermissionDenied)?;
    if v["op"].as_u64() != Some(0) {
        return Ok(Callback::Ignore);
    }
    let kind = v["t"].as_str().unwrap_or("");
    if ![
        "GROUP_AT_MESSAGE_CREATE",
        "GROUP_MESSAGE_CREATE",
        "C2C_MESSAGE_CREATE",
    ]
    .contains(&kind)
    {
        return Ok(Callback::Ignore);
    }
    let d = &v["d"];
    if d["author"]["bot"].as_bool() == Some(true) {
        return Ok(Callback::Ignore);
    }
    let direct = kind == "C2C_MESSAGE_CREATE";
    let sender = d["author"][if direct {
        "user_openid"
    } else {
        "member_openid"
    }]
    .as_str()
    .filter(|s| openid(s))
    .ok_or(invalid("invalid_qq_sender"))?;
    let conversation = if direct {
        sender
    } else {
        d["group_openid"]
            .as_str()
            .filter(|s| openid(s))
            .ok_or(invalid("invalid_qq_group"))?
    };
    if !c.group_openid.is_empty() && (direct || conversation != c.group_openid) {
        return Ok(Callback::Ignore);
    }
    if !c.allowed_senders.is_empty() && !c.allowed_senders.iter().any(|s| s == sender) {
        return Ok(Callback::Ignore);
    }
    let id = d["id"]
        .as_str()
        .filter(|s| valid_event_id(s, 256))
        .ok_or(invalid("invalid_qq_message_id"))?;
    let content = d["content"].as_str().unwrap_or("");
    // Both full-message and mention events may carry the same message. Deliberately
    // omit the delivery event type/ID from persisted content so retries deduplicate.
    Ok(Callback::Event(Box::new(IncomingEvent {
        route_context: Some(crate::domain::connection_routing::RouteBinding::new(
            "qq",
            json!({"app_id":c.app_id,"kind":if direct {"direct"} else {"group"},"id":conversation,"msg_id":id,"sender_openid":sender}),
        )),
        source_event_id: Some(id.into()),
        event_type: if direct {
            "qq.direct.message"
        } else {
            "qq.group.message"
        }
        .into(),
        schema_version: 1,
        occurred_at: None,
        payload: EventPayload::Json(
            json!({"group_name":null,"content":content.trim(),"msg_id":id,"group_openid":if direct {None}else{Some(conversation)},"user_openid":if direct{Some(conversation)}else{None},"sender_openid":sender,"timestamp":d["timestamp"],"attachments":d["attachments"]}),
        ),
    })))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_signature_vectors() {
        let k = key("DG5g3B4j9X2KOErG").unwrap();
        assert_eq!(
            hex(k.sign(b"1725442341Arq0D5A61EgUu4OxUvOp").as_ref()),
            "87befc99c42c651b3aac0278e71ada338433ae26fcb24307bdc5ad38c1adc2d01bcfcadc0842edac85e85205028a1132afe09280305f13aa6909ffc2d652c706"
        );
        let k = key("naOC0ocQE3shWLAfffVLB1rhYPG7").unwrap();
        assert_eq!(
            k.public_key().as_ref(),
            &[
                215, 195, 98, 254, 120, 174, 248, 31, 242, 50, 135, 180, 147, 98, 139, 93, 176, 42,
                60, 79, 227, 11, 33, 94, 77, 25, 96, 155, 93, 118, 103, 58
            ]
        );
        assert!(unhex(&"文".repeat(128)).is_none());
    }
}
