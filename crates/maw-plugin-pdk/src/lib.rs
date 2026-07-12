#![forbid(unsafe_code)]
#![doc = "Typed contracts for maw Rust WASM plugins."]
#![doc = "Breaking host ABI changes require a new `maw-plugin-pdk` major version."]

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The byte-frozen input contract passed to every plugin export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeContext {
    pub args: Vec<String>,
    pub source: InvokeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvokeSource {
    Cli,
    Api,
    Peer,
}

/// The JSON value returned by a plugin export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvokeResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl InvokeResult {
    #[must_use]
    pub fn output(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            output: Some(output.into()),
            error: None,
        }
    }

    #[must_use]
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            output: None,
            error: Some(error.into()),
        }
    }
}

/// Decode the byte-frozen invoke context.
///
/// # Errors
/// Returns [`Error::Codec`] when the payload is malformed or has extra fields.
pub fn parse_context(input: &str) -> Result<InvokeContext, Error> {
    serde_json::from_str(input).map_err(|error| Error::Codec(error.to_string()))
}

/// Encode the shared plugin result envelope.
///
/// # Errors
/// Returns [`Error::Codec`] if serialization fails.
pub fn encode_result(result: &InvokeResult) -> Result<String, Error> {
    serde_json::to_string(result).map_err(|error| Error::Codec(error.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostErrorCode {
    CapabilityDenied,
    InvalidArgs,
    NotFound,
    Timeout,
    IoError,
    ProcessFailed,
    NetworkError,
    Unsupported,
}

/// A capability-scoped error returned by a maw host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostError {
    pub code: HostErrorCode,
    pub error: String,
    pub detail: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Codec(String),
    Host(HostError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => formatter.write_str(error),
            Self::Host(error) => write!(formatter, "{}: {}", error.code, error.error),
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Display for HostErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = serde_json::to_value(self)
            .map_err(|_| fmt::Error)?
            .as_str()
            .ok_or(fmt::Error)?
            .to_owned();
        formatter.write_str(&code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_context_rejects_contract_drift() {
        let exact = parse_context(r#"{"args":[],"source":"cli"}"#).unwrap();
        assert_eq!(exact.source, InvokeSource::Cli);
        assert!(parse_context(r#"{"args":[],"source":"cli","nowMillis":1}"#).is_err());
    }

    #[test]
    fn result_keeps_wire_shape() {
        assert_eq!(
            encode_result(&InvokeResult::output("ok")).unwrap(),
            r#"{"ok":true,"output":"ok"}"#
        );
    }
}
