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

/// The normalized host-visible result used by parity fixtures and transports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputEnvelope {
    pub result: InvokeResult,
    pub stdout: String,
    pub stderr: String,
}

impl From<InvokeResult> for OutputEnvelope {
    fn from(result: InvokeResult) -> Self {
        Self {
            stdout: result.output.clone().unwrap_or_default(),
            stderr: result.error.clone().unwrap_or_default(),
            result,
        }
    }
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

mod host;
pub use host::{decode_host, HostSuccess};

#[doc(hidden)]
pub mod __private {
    pub use extism_pdk::Memory;
    pub use serde_json::to_vec;
}

#[doc(hidden)]
#[macro_export]
macro_rules! __host_call {
    ($request:expr, $link:literal, $raw:ident) => {{
        (|| {
            let bytes = $crate::__private::to_vec(&$request)
                .map_err(|error| $crate::Error::Codec(error.to_string()))?;
            let input = $crate::__private::Memory::from_bytes(&bytes)
                .map_err(|error| $crate::Error::Codec(error.to_string()))?;
            #[link(wasm_import_module = "extism:host/user")]
            extern "C" {
                #[link_name = $link]
                fn $raw(input: u64) -> u64;
            }
            let output_offset = unsafe { $raw(input.offset()) };
            input.free();
            let output = $crate::__private::Memory::find(output_offset)
                .ok_or_else(|| $crate::Error::Codec("host response memory missing".to_owned()))?;
            let bytes = output.to_vec();
            output.free();
            $crate::decode_host(&bytes)
        })()
    }};
}

/// Call a typed maw host binding without hand-written Extism memory code.
#[rustfmt::skip]
#[macro_export]
macro_rules! host_call {
    (cli_run, $v:expr) => { $crate::__host_call!($v, "maw.cli.run", __maw_cli_run) };
    (exec_run, $v:expr) => { $crate::__host_call!($v, "maw.exec.run", __maw_exec_run) };
    (exec_spawn, $v:expr) => { $crate::__host_call!($v, "maw.exec.spawn", __maw_exec_spawn) };
    (paths_get, $v:expr) => { $crate::__host_call!($v, "maw.paths.get", __maw_paths_get) };
    (time_now, $v:expr) => { $crate::__host_call!($v, "maw.time.now", __maw_time_now) };
    (config_get, $v:expr) => { $crate::__host_call!($v, "maw.config.get", __maw_config_get) };
    (config_set, $v:expr) => { $crate::__host_call!($v, "maw.config.set", __maw_config_set) };
    (consent_read, $v:expr) => { $crate::__host_call!($v, "maw.consent.read", __maw_consent_read) };
    (fs_read, $v:expr) => { $crate::__host_call!($v, "maw.fs.read", __maw_fs_read) };
    (fs_write, $v:expr) => { $crate::__host_call!($v, "maw.fs.write", __maw_fs_write) };
    (fs_mkdir, $v:expr) => { $crate::__host_call!($v, "maw.fs.mkdir", __maw_fs_mkdir) };
    (fs_remove, $v:expr) => { $crate::__host_call!($v, "maw.fs.remove", __maw_fs_remove) };
    (fs_list, $v:expr) => { $crate::__host_call!($v, "maw.fs.list", __maw_fs_list) };
    (fs_stat, $v:expr) => { $crate::__host_call!($v, "maw.fs.stat", __maw_fs_stat) };
    (http_request, $v:expr) => { $crate::__host_call!($v, "maw.http.request", __maw_http_request) };
    (net_fetch, $v:expr) => { $crate::__host_call!($v, "maw.net.fetch", __maw_net_fetch) };
    (localserver_request, $v:expr) => { $crate::__host_call!($v, "maw.localserver.request", __maw_localserver_request) };
    (peer_send, $v:expr) => { $crate::__host_call!($v, "maw.http.peer_send", __maw_peer_send) };
    (peer_wake, $v:expr) => { $crate::__host_call!($v, "maw.http.peer_wake", __maw_peer_wake) };
    (tmux_list_sessions, $v:expr) => { $crate::__host_call!($v, "maw.tmux.list_sessions", __maw_tmux_list_sessions) };
    (tmux_capture, $v:expr) => { $crate::__host_call!($v, "maw.tmux.capture", __maw_tmux_capture) };
    (tmux_send_keys, $v:expr) => { $crate::__host_call!($v, "maw.tmux.send_keys", __maw_tmux_send_keys) };
    (tmux_run, $v:expr) => { $crate::__host_call!($v, "maw.tmux.run", __maw_tmux_run) };
    (tmux_command, $v:expr) => { $crate::__host_call!($v, "maw.tmux.command", __maw_tmux_command) };
    (tmux_send_enter, $v:expr) => { $crate::__host_call!($v, "maw.tmux.send_enter", __maw_tmux_send_enter) };
    (tmux_tags_read, $v:expr) => { $crate::__host_call!($v, "maw.tmux.tags_read", __maw_tmux_tags_read) };
    (tmux_tags_write, $v:expr) => { $crate::__host_call!($v, "maw.tmux.tags_write", __maw_tmux_tags_write) };
    (ssh_exec, $v:expr) => { $crate::__host_call!($v, "maw.ssh.exec", __maw_ssh_exec) };
    (ssh_tmux_capture, $v:expr) => { $crate::__host_call!($v, "maw.ssh.tmux_capture", __maw_ssh_tmux_capture) };
    (ssh_tmux_send_keys, $v:expr) => { $crate::__host_call!($v, "maw.ssh.tmux_send_keys", __maw_ssh_tmux_send_keys) };
}
