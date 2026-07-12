use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, HostError, HostErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSuccess<T> {
    pub value: T,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
struct HostWire<T> {
    ok: bool,
    value: Option<T>,
    #[serde(default)]
    warnings: Vec<String>,
    error: Option<String>,
    code: Option<HostErrorCode>,
    detail: Option<Value>,
}

/// Decode the shared host response envelope.
///
/// # Errors
/// Returns a codec error for malformed envelopes and a host error for a
/// capability or runtime failure reported by maw-rs.
pub fn decode_host<T: DeserializeOwned>(bytes: &[u8]) -> Result<HostSuccess<T>, Error> {
    let wire: HostWire<T> =
        serde_json::from_slice(bytes).map_err(|error| Error::Codec(error.to_string()))?;
    if wire.ok {
        let value = wire
            .value
            .ok_or_else(|| Error::Codec("host response omitted value".to_owned()))?;
        Ok(HostSuccess {
            value,
            warnings: wire.warnings,
        })
    } else {
        Err(Error::Host(HostError {
            code: wire.code.unwrap_or(HostErrorCode::Unsupported),
            error: wire.error.unwrap_or_else(|| "host call failed".to_owned()),
            detail: wire.detail,
        }))
    }
}
