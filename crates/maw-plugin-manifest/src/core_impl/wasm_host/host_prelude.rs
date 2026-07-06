use std::fs::{File, OpenOptions};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
#[cfg(target_os = "macos")]
use std::ffi::OsString;
#[cfg(target_os = "macos")]
use std::os::fd::AsFd;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtectedPathKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtectedPath {
    path: PathBuf,
    kind: ProtectedPathKind,
}

use base64::Engine as _;
use extism::{
    CurrentPlugin, Manifest as ExtismManifest, PluginBuilder, UserData, Val, ValType, Wasm,
};
use maw_tmux::{CommandTmuxRunner, TmuxClient};
use maw_transport::{
    HttpRequest as TransportHttpRequest, PeerSendRequest, PeerWakeRequest, ReqwestHttpTransportIo,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

const MAX_HTTP_TIMEOUT_MS: u64 = 30_000;
const MAX_NET_FETCH_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_EXEC_TIMEOUT_MS: u64 = 30_000;
const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;
const O_NOFOLLOW_FLAG: i32 = libc::O_NOFOLLOW;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostResult<T> {
    Ok {
        value: T,
        warnings: Vec<String>,
    },
    Err {
        error: String,
        code: HostErrorCode,
        detail: Option<Value>,
    },
}

impl<T: Serialize> Serialize for HostResult<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::Ok { value, warnings } => {
                let mut map =
                    serializer.serialize_map(Some(if warnings.is_empty() { 2 } else { 3 }))?;
                map.serialize_entry("ok", &true)?;
                map.serialize_entry("value", value)?;
                if !warnings.is_empty() {
                    map.serialize_entry("warnings", warnings)?;
                }
                map.end()
            }
            Self::Err {
                error,
                code,
                detail,
            } => {
                let mut map =
                    serializer.serialize_map(Some(if detail.is_some() { 4 } else { 3 }))?;
                map.serialize_entry("ok", &false)?;
                map.serialize_entry("error", error)?;
                map.serialize_entry("code", code)?;
                if let Some(detail) = detail {
                    map.serialize_entry("detail", detail)?;
                }
                map.end()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl<T> HostResult<T> {
    fn ok(value: T) -> Self {
        Self::Ok {
            value,
            warnings: Vec::new(),
        }
    }
    fn err(code: HostErrorCode, error: impl Into<String>) -> Self {
        Self::Err {
            error: error.into(),
            code,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    caps: BTreeSet<String>,
}

impl CapabilitySet {
    #[must_use]
    pub fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            caps: manifest
                .capabilities
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        }
    }

    #[must_use]
    pub fn contains(&self, namespace: &str, verb: &str, scope: Option<&str>) -> bool {
        let exact = scope.map_or_else(
            || format!("{namespace}:{verb}"),
            |scope| format!("{namespace}:{verb}:{scope}"),
        );
        self.caps.contains(&exact)
            || self.caps.contains(&format!("{namespace}:{verb}:*"))
            || self.caps.contains(&format!("{namespace}:{verb}"))
    }

    fn require(
        &self,
        namespace: &str,
        verb: &str,
        scope: Option<&str>,
    ) -> Result<String, HostResult<Value>> {
        if self.contains(namespace, verb, scope) {
            Ok(scope.map_or_else(
                || format!("{namespace}:{verb}"),
                |scope| format!("{namespace}:{verb}:{scope}"),
            ))
        } else {
            Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                format!(
                    "capability denied: {namespace}:{verb}{}",
                    scope.map_or(String::new(), |s| format!(":{s}"))
                ),
            ))
        }
    }

    fn scopes_for(&self, namespace: &str, verb: &str) -> Vec<String> {
        let prefix = format!("{namespace}:{verb}:");
        self.caps
            .iter()
            .filter_map(|cap| cap.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub plugin: String,
    pub host_fn: String,
    pub capability: String,
    pub resource: String,
    pub status: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
struct FakeHostResponse {
    output: String,
    capability: Option<String>,
    resource: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MawWasmHost {
    plugin_name: String,
    caps: CapabilitySet,
    endpoints: PluginEndpointPolicies,
    fs_roots: BTreeMap<String, PathBuf>,
    secret_store: BTreeMap<String, String>,
    fake_responses: BTreeMap<(String, String), FakeHostResponse>,
    tmux_pane_commands: BTreeMap<String, String>,
    tmux_dry_run: bool,
    audit: Arc<Mutex<Vec<AuditEvent>>>,
    http_timeout_ms: u64,
    localserver_url: Option<String>,
    http_resolver_overrides: BTreeMap<String, Vec<IpAddr>>,
    cwd: Option<String>,
    home: Option<String>,
}
