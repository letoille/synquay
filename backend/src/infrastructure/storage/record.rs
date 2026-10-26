//! Storage DTO envelope; v1 is the previous unwrapped serde record.
use super::{PortError, capacity};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
#[derive(Serialize)]
struct Record<'a, T> {
    storage_version: u32,
    data: &'a T,
}
pub(super) fn encode<T: Serialize>(data: &T) -> Result<String, PortError> {
    capacity::encode_bounded(&Record {
        storage_version: 2,
        data,
    })
}
pub(super) fn decode<T: DeserializeOwned>(record: &str) -> Result<T, PortError> {
    if record.is_empty() {
        return Err(PortError::NotFound);
    }
    #[derive(Deserialize)]
    struct Header {
        storage_version: Option<u32>,
    }
    let header: Header = super::decode(record)?;
    match header.storage_version {
        None => super::decode(record),
        Some(2) => {
            #[derive(Deserialize)]
            struct Stored<T> {
                data: T,
            }
            Ok(super::decode::<Stored<T>>(record)?.data)
        }
        _ => Err(PortError::Unsupported),
    }
}
