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
    fs_roots: BTreeMap<String, PathBuf>,
    secret_store: BTreeMap<String, String>,
    fake_responses: BTreeMap<(String, String), FakeHostResponse>,
    tmux_pane_commands: BTreeMap<String, String>,
    tmux_dry_run: bool,
    audit: Arc<Mutex<Vec<AuditEvent>>>,
    http_timeout_ms: u64,
    localserver_url: Option<String>,
    http_resolver_overrides: BTreeMap<String, Vec<IpAddr>>,
}

impl MawWasmHost {
    #[must_use]
    pub fn new(plugin: &LoadedPlugin) -> Self {
        Self {
            plugin_name: plugin.manifest.name.clone(),
            caps: CapabilitySet::from_manifest(&plugin.manifest),
            fs_roots: BTreeMap::new(),
            secret_store: BTreeMap::new(),
            fake_responses: BTreeMap::new(),
            tmux_pane_commands: BTreeMap::new(),
            tmux_dry_run: false,
            audit: Arc::new(Mutex::new(Vec::new())),
            http_timeout_ms: 10_000,
            localserver_url: None,
            http_resolver_overrides: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_fs_root(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.fs_roots.insert(name.into(), path.into());
        self
    }

    #[must_use]
    pub fn with_secret_ref(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.secret_store.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_localserver_url(mut self, url: impl Into<String>) -> Self {
        self.localserver_url = Some(url.into());
        self
    }

    #[must_use]
    pub fn with_http_resolver_override(
        mut self,
        host: impl Into<String>,
        addrs: impl IntoIterator<Item = IpAddr>,
    ) -> Self {
        self.http_resolver_overrides
            .insert(host.into(), addrs.into_iter().collect());
        self
    }

    #[must_use]
    pub fn with_fake_response(
        self,
        name: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        self.with_audited_fake_response(name, input, output, None, None, None)
    }

    #[must_use]
    pub fn with_audited_fake_response(
        mut self,
        name: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
        capability: Option<String>,
        resource: Option<String>,
        status: Option<String>,
    ) -> Self {
        self.fake_responses.insert(
            (name.into(), input.into()),
            FakeHostResponse {
                output: output.into(),
                capability,
                resource,
                status,
            },
        );
        self
    }

    #[must_use]
    pub fn with_tmux_pane_command(
        mut self,
        target: impl Into<String>,
        command: impl Into<String>,
    ) -> Self {
        self.tmux_pane_commands
            .insert(target.into(), command.into());
        self
    }

    #[must_use]
    pub fn with_tmux_dry_run(mut self) -> Self {
        self.tmux_dry_run = true;
        self
    }

    #[must_use]
    pub fn audit_json_lines(&self) -> String {
        self.audit.lock().map_or_else(
            |_| String::new(),
            |events| {
                events
                    .iter()
                    .map(|event| serde_json::to_string(event).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )
    }

    #[must_use]
    pub fn handle_json(&self, name: &str, input: &str) -> String {
        if let Some(fake) = self
            .fake_responses
            .get(&(name.to_owned(), input.to_owned()))
        {
            if let Some(capability) = &fake.capability {
                if !self.caps.caps.contains(capability) {
                    return to_json(&HostResult::<Value>::err(
                        HostErrorCode::CapabilityDenied,
                        format!("capability denied: {capability}"),
                    ));
                }
                self.audit(
                    name,
                    capability,
                    fake.resource.as_deref().unwrap_or("seeded-host"),
                    fake.status.as_deref().unwrap_or("ok"),
                    Instant::now(),
                );
            }
            return fake.output.clone();
        }
        match name {
            "maw.exec.run" => to_json(&self.exec_run(input)),
            "maw.exec.spawn" => to_json(&self.exec_spawn(input)),
            "maw.config.get" => to_json(&self.config_get(input)),
            "maw.config.set" => to_json(&self.config_set(input)),
            "maw.consent.read" => to_json(&self.consent_read(input)),
            "maw.consent.approve" | "maw.consent.reject" | "maw.consent.trust"
            | "maw.consent.untrust" | "maw.state.set" => to_json(&HostResult::<Value>::err(
                HostErrorCode::CapabilityDenied,
                "WASM plugins cannot approve, grant trust, pair, or mutate consent state; use a human-at-terminal command",
            )),
            "maw.fs.read" => to_json(&self.fs_read(input)),
            "maw.fs.write" => to_json(&self.fs_write(input)),
            "maw.fs.remove" => to_json(&self.fs_remove(input)),
            "maw.fs.list" => to_json(&self.fs_list(input)),
            "maw.fs.stat" => to_json(&self.fs_stat(input)),
            "maw.http.request" => to_json(&self.http_request(input)),
            "maw.localserver.request" => to_json(&self.localserver_request(input)),
            "maw.http.peer_send" => to_json(&self.peer_send(input)),
            "maw.http.peer_wake" => to_json(&self.peer_wake(input)),
            "maw.tmux.list_sessions" => to_json(&self.tmux_list_sessions(input)),
            "maw.tmux.capture" => to_json(&self.tmux_capture(input)),
            "maw.tmux.send_keys" => to_json(&self.tmux_send_keys(input)),
            "maw.tmux.run" => to_json(&self.tmux_run(input)),
            "maw.tmux.send_enter" => to_json(&self.tmux_send_enter(input)),
            "maw.tmux.tags_read" => to_json(&self.tmux_tags_read(input)),
            "maw.tmux.tags_write" => to_json(&self.tmux_tags_write(input)),
            "maw.ssh.exec" => to_json(&self.ssh_exec(input)),
            "maw.ssh.tmux_capture" => to_json(&self.ssh_tmux_capture(input)),
            "maw.ssh.tmux_send_keys" => to_json(&self.ssh_tmux_send_keys(input)),
            _ => to_json(&HostResult::<Value>::err(HostErrorCode::Unsupported, format!("unsupported host function: {name}"))),
        }
    }

    fn has_exact_cap(&self, capability: &str) -> bool {
        self.caps.caps.contains(capability)
    }

    fn tmux_current_command(
        &self,
        target: &str,
        client: &mut TmuxClient<CommandTmuxRunner>,
    ) -> Result<String, HostResult<Value>> {
        if let Some(command) = self.tmux_pane_commands.get(target) {
            return Ok(command.clone());
        }
        client
            .display_pane_current_command(target)
            .map_err(|error| HostResult::err(HostErrorCode::IoError, error.message))
    }

    fn audit(&self, name: &str, capability: &str, resource: &str, status: &str, start: Instant) {
        if let Ok(mut events) = self.audit.lock() {
            events.push(AuditEvent {
                plugin: redact(&self.plugin_name),
                host_fn: name.to_owned(),
                capability: capability.to_owned(),
                resource: redact(resource),
                status: status.to_owned(),
                duration_ms: start.elapsed().as_millis(),
            });
        }
    }

    fn exec_run(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ExecRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let base = executable_basename(&args.cmd);
        if is_hard_denied_exec(&args.cmd, &args.args) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "hard-denied executable or interactive/privileged option",
            );
        }
        let cap = match self
            .caps
            .require("proc", "exec", Some(&base))
            .or_else(|_| self.caps.require("shell", "exec", Some(&base)))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        if let Some(cwd) = &args.cwd {
            if let Err(err) = self.check_cwd(cwd) {
                return err;
            }
        }
        let env = match sanitize_env(args.env.as_ref()) {
            Ok(env) => env,
            Err(err) => return err,
        };
        let mut cmd = Command::new(&args.cmd);
        cmd.args(&args.args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &args.cwd {
            cmd.current_dir(cwd);
        }
        let output = run_child(
            cmd,
            args.stdin.as_deref(),
            args.timeout_ms.unwrap_or(10_000).min(MAX_EXEC_TIMEOUT_MS),
        );
        let result = match output {
            Ok(output) => {
                let status = output.status.code().unwrap_or(-1);
                if status != 0 && !args.allow_non_zero {
                    HostResult::err(
                        HostErrorCode::ProcessFailed,
                        format!("process exited with status {status}"),
                    )
                } else {
                    HostResult::ok(
                        json!({"status": status, "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr), "durationMs": start.elapsed().as_millis()}),
                    )
                }
            }
            Err(code) => HostResult::err(code, "process execution failed"),
        };
        self.audit("maw.exec.run", &cap, &args.cmd, status_of(&result), start);
        result
    }

    fn exec_spawn(&self, input: &str) -> HostResult<Value> {
        let mut args = match parse_args::<ExecRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        args.allow_non_zero = true;
        self.exec_run(&serde_json::to_string(&args).unwrap_or_default())
    }

    fn config_get(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConfigGetArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "config", Some("read"))
            .or_else(|_| self.caps.require("sdk", "config:read", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let path = match self.config_file_path() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let config = match read_config_json(&path) {
            Ok(config) => config,
            Err(err) => return err,
        };
        let resource = args
            .key
            .as_deref()
            .map_or_else(|| "config".to_owned(), |key| format!("config:{key}"));
        let value = args
            .key
            .as_deref()
            .and_then(|key| get_json_path(&config, key))
            .cloned()
            .unwrap_or(Value::Null);
        let result = HostResult::ok(json!({"key": args.key, "value": value, "config": config}));
        self.audit("maw.config.get", &cap, &resource, status_of(&result), start);
        result
    }

    fn config_set(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConfigSetArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "config", Some("write"))
            .or_else(|_| self.caps.require("sdk", "config:write", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        if args.key.trim().is_empty() {
            return HostResult::err(HostErrorCode::InvalidArgs, "config key is required");
        }
        if !is_plugin_writable_config_key_path(&args.key)
            || value_contains_secret_config_key_path(&args.key, &args.value)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "config key is not in the host allowlist for WASM writes",
            );
        }
        let path = match self.config_file_path() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let resource = format!("config:{}", args.key);
        self.audit("maw.config.set", &cap, &resource, "attempt", start);
        let mut config = match read_config_json(&path) {
            Ok(config) => config,
            Err(err) => return err,
        };
        if let Err(err) = set_json_path(&mut config, &args.key, args.value.clone()) {
            return err;
        }
        let final_value = get_json_path(&config, &args.key)
            .cloned()
            .unwrap_or(args.value);
        if let Err(err) = write_config_json(&path, &config) {
            return err;
        }
        HostResult::ok(
            json!({"key": args.key, "written": true, "audit": "config-write", "finalValue": final_value}),
        )
    }

    fn consent_read(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConsentReadArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "consent", Some("read"))
            .or_else(|_| self.caps.require("sdk", "consent:read", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let view = args.view.as_deref().unwrap_or("pending");
        let state_root = match self.consent_state_root() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let result = match view {
            "pending" | "list" => {
                let rows = match read_consent_pending(&state_root) {
                    Ok(rows) => rows,
                    Err(err) => return err,
                };
                HostResult::ok(json!({"text": format_consent_pending(&rows), "pending": rows}))
            }
            "trust" | "list-trust" => {
                let rows = match read_consent_trust(&state_root) {
                    Ok(rows) => rows,
                    Err(err) => return err,
                };
                HostResult::ok(json!({"text": format_consent_trust(&rows), "trust": rows}))
            }
            _ => HostResult::err(HostErrorCode::InvalidArgs, "view must be pending or trust"),
        };
        let resource = if matches!(view, "trust" | "list-trust") {
            "consent:trust"
        } else {
            "consent:pending"
        };
        self.audit(
            "maw.consent.read",
            &cap,
            resource,
            status_of(&result),
            start,
        );
        result
    }

    fn fs_read(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsReadArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (cap, real) = match self.secure_path(&args.path, "read") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let max = args.max_bytes.unwrap_or(MAX_READ_BYTES).min(MAX_READ_BYTES);
        let file = match open_nofollow_existing(&real) {
            Ok(file) => file,
            Err(err) => return err,
        };
        if let Err(err) = verify_fd_path(&file, &real) {
            return err;
        }
        let mut bytes = Vec::new();
        if let Err(error) = file.take(max + 1).read_to_end(&mut bytes) {
            return HostResult::err(HostErrorCode::IoError, format!("read failed: {error}"));
        }
        if bytes.len() as u64 > max {
            return HostResult::err(HostErrorCode::IoError, "read exceeds maxBytes");
        }
        let content = if args.encoding.as_deref() == Some("base64") {
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        } else {
            match String::from_utf8(bytes.clone()) {
                Ok(text) => text,
                Err(_) => {
                    return HostResult::err(HostErrorCode::InvalidArgs, "file is not valid utf8")
                }
            }
        };
        let result = HostResult::ok(
            json!({"path": real.display().to_string(), "bytes": bytes.len(), "content": content}),
        );
        self.audit(
            "maw.fs.read",
            &cap,
            &real.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_write(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsWriteArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (cap, path) = match self.secure_write_path(&args.path) {
            Ok(value) => value,
            Err(err) => return err,
        };
        if args.mkdirp.unwrap_or(false) {
            if let Some(parent) = path.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    return HostResult::err(
                        HostErrorCode::IoError,
                        format!("mkdirp failed: {error}"),
                    );
                }
            }
        }
        let bytes = if args.encoding.as_deref() == Some("base64") {
            match base64::engine::general_purpose::STANDARD.decode(&args.content) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return HostResult::err(
                        HostErrorCode::InvalidArgs,
                        format!("base64 decode failed: {error}"),
                    )
                }
            }
        } else {
            args.content.into_bytes()
        };
        let mut opts = OpenOptions::new();
        opts.write(true).custom_flags(O_NOFOLLOW_FLAG);
        match args.mode.as_deref().unwrap_or("create") {
            "create" => {
                opts.create_new(true);
            }
            "overwrite" => {
                opts.create(true).truncate(true);
            }
            "append" => {
                opts.create(true).append(true);
            }
            _ => {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    "mode must be create, overwrite, or append",
                )
            }
        }
        let mut file = match opts.open(&path) {
            Ok(file) => file,
            Err(error) => {
                return HostResult::err(HostErrorCode::IoError, format!("open failed: {error}"))
            }
        };
        if let Err(err) = verify_fd_under_roots(&file, &self.roots_for("write")) {
            return err;
        }
        if let Err(error) = file.write_all(&bytes) {
            return HostResult::err(HostErrorCode::IoError, format!("write failed: {error}"));
        }
        let result =
            HostResult::ok(json!({"path": path.display().to_string(), "bytes": bytes.len()}));
        self.audit(
            "maw.fs.write",
            &cap,
            &path.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_remove(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsRemoveArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (cap, path) = match self.secure_remove_path(&args.path) {
            Ok(value) => value,
            Err(err) => return err,
        };
        let result = match remove_bounded_path(
            &path,
            args.recursive.unwrap_or(false),
            &self.roots_for("write"),
        ) {
            Ok(removed) => {
                HostResult::ok(json!({"path": path.display().to_string(), "removed": removed}))
            }
            Err(err) => err,
        };
        self.audit(
            "maw.fs.remove",
            &cap,
            &path.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_list(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsListArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (_cap, real) = match self.secure_path(&args.path, "read") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let mut entries = Vec::new();
        let max = args.max_entries.unwrap_or(200).min(1000);
        list_dir(
            &real,
            args.recursive.unwrap_or(false),
            args.include_dirs.unwrap_or(true),
            max,
            &mut entries,
        );
        HostResult::ok(json!({"entries": entries}))
    }

    fn fs_stat(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsPathArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let Ok((_cap, real)) = self.secure_path(&args.path, "read") else {
            return HostResult::ok(json!({"exists": false}));
        };
        let Ok(meta) = std::fs::symlink_metadata(&real) else {
            return HostResult::ok(json!({"exists": false}));
        };
        HostResult::ok(
            json!({"exists": true, "kind": file_kind(meta.file_type()), "bytes": meta.len()}),
        )
    }

    fn http_request(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<HttpArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let url = match Url::parse(&args.url) {
            Ok(url) => url,
            Err(error) => {
                return HostResult::err(HostErrorCode::InvalidArgs, format!("invalid url: {error}"))
            }
        };
        if is_discord_gateway(&url) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "Discord gateway is hard-denied from WASM host functions",
            );
        }
        let scheme = url.scheme();
        if !matches!(scheme, "http" | "https") {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "only http/https URLs are supported",
            );
        }
        let host = match url.host_str() {
            Some(host) => host.to_owned(),
            None => return HostResult::err(HostErrorCode::InvalidArgs, "url host is required"),
        };
        let pinned_addr = match self.resolve_http_pinned_addr(&url, &host) {
            Ok(addr) => addr,
            Err(err) => return err,
        };
        if private_ip(pinned_addr.ip()) && !self.caps.contains("net", "private", Some(&host)) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            );
        }
        let cap = match self.caps.require("net", scheme, Some(&host)) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let headers = redact_headers(args.headers.unwrap_or_default());
        let request = TransportHttpRequest {
            method: args.method,
            url: args.url,
            headers,
            body: args.body,
            timeout_ms: Some(
                args.timeout_ms
                    .unwrap_or(self.http_timeout_ms)
                    .min(MAX_HTTP_TIMEOUT_MS),
            ),
            follow_redirects: args.follow_redirects.unwrap_or(false),
            pinned_addr: Some(pinned_addr),
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::NetworkError,
                    format!("tokio runtime failed: {error}"),
                )
            }
        };
        let client =
            match ReqwestHttpTransportIo::new(request.timeout_ms.unwrap_or(self.http_timeout_ms)) {
                Ok(client) => client,
                Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
            };
        let result = match runtime.block_on(client.request(&request)) {
            Ok(resp) => HostResult::ok(
                json!({"status": resp.status, "headers": resp.headers, "body": resp.body, "url": resp.url}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        };
        self.audit("maw.http.request", &cap, &host, status_of(&result), start);
        result
    }


    fn resolve_http_pinned_addr(&self, url: &Url, host: &str) -> Result<SocketAddr, HostResult<Value>> {
        let Some(port) = url.port_or_known_default() else {
            return Err(HostResult::err(
                HostErrorCode::InvalidArgs,
                "url port is required",
            ));
        };
        if private_host_name(host) && !self.caps.contains("net", "private", Some(host)) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            ));
        }
        let addrs = match self.resolve_http_host_once(host, port) {
            Ok(addrs) => addrs,
            Err(error) => return Err(HostResult::err(HostErrorCode::NetworkError, error)),
        };
        if addrs.iter().any(|addr| private_ip(*addr))
            && !self.caps.contains("net", "private", Some(host))
        {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            ));
        }
        addrs
            .first()
            .map(|ip| SocketAddr::new(*ip, port))
            .ok_or_else(|| HostResult::err(HostErrorCode::NetworkError, "host resolved no addresses"))
    }

    fn resolve_http_host_once(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
        if let Some(addrs) = self.http_resolver_overrides.get(host) {
            return Ok(addrs.clone());
        }
        let literal_host = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = literal_host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        (host, port)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|addr| addr.ip()).collect::<Vec<_>>())
            .map_err(|error| format!("failed to resolve {host}: {error}"))
    }


    fn localserver_request(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<LocalserverArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self.caps.require("sdk", "localserver", None) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let base = match self.resolve_localserver_url() {
            Ok(base) => base,
            Err(err) => return err,
        };
        let url = match pinned_localserver_url(&base, args.path.as_deref(), args.url.as_deref()) {
            Ok(url) => url,
            Err(err) => return err,
        };
        let headers = redact_headers(args.headers.unwrap_or_default());
        let request = TransportHttpRequest {
            method: args.method,
            url: url.to_string(),
            headers,
            body: args.body,
            timeout_ms: Some(
                args.timeout_ms
                    .unwrap_or(self.http_timeout_ms)
                    .min(MAX_HTTP_TIMEOUT_MS),
            ),
            follow_redirects: false,
            pinned_addr: None,
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::NetworkError,
                    format!("tokio runtime failed: {error}"),
                )
            }
        };
        let client = match ReqwestHttpTransportIo::new(request.timeout_ms.unwrap_or(self.http_timeout_ms)) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        let result = match runtime.block_on(client.request(&request)) {
            Ok(resp) => HostResult::ok(
                json!({"status": resp.status, "headers": resp.headers, "body": resp.body, "url": resp.url}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        };
        self.audit(
            "maw.localserver.request",
            &cap,
            base.as_str(),
            status_of(&result),
            start,
        );
        result
    }

    fn peer_send(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<PeerSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let key = match self.secret_ref(args.peer_key_ref.as_deref()) {
            Ok(key) => key,
            Err(err) => return err,
        };
        let url = match Url::parse(&args.peer_url) {
            Ok(url) => url,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    format!("invalid peerUrl: {error}"),
                )
            }
        };
        let host = url.host_str().unwrap_or_default();
        if let Err(err) = self
            .caps
            .require("peer", "send", None)
            .and_then(|_| self.caps.require(url.scheme(), "", Some(host)))
        {
            let _ = err;
        }
        if !self.caps.contains("peer", "send", None)
            || !self.caps.contains("net", url.scheme(), Some(host))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peer send capability denied",
            );
        }
        let req = PeerSendRequest {
            peer_url: args.peer_url,
            target: args.target,
            text: args.text,
            inbox: args.inbox,
            from: args.from,
            peer_key: key,
            timestamp: args.timestamp.unwrap_or(0),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())
            .ok();
        let Some(rt) = rt else {
            return HostResult::err(HostErrorCode::NetworkError, "tokio runtime failed");
        };
        let client = match ReqwestHttpTransportIo::new(self.http_timeout_ms) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        match rt.block_on(client.send_peer(&req)) {
            Ok(resp) => HostResult::ok(
                json!({"ok": resp.ok, "status": resp.status, "state": resp.state, "target": resp.target, "lastLine": resp.last_line, "error": resp.error}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        }
    }

    fn peer_wake(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<PeerWakeArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let key = match self.secret_ref(args.peer_key_ref.as_deref()) {
            Ok(key) => key,
            Err(err) => return err,
        };
        let url = Url::parse(&args.peer_url).map_err(|_| ()).ok();
        let Some(url) = url else {
            return HostResult::err(HostErrorCode::InvalidArgs, "invalid peerUrl");
        };
        let host = url.host_str().unwrap_or_default();
        if !self.caps.contains("peer", "wake", None)
            || !self.caps.contains("net", url.scheme(), Some(host))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peer wake capability denied",
            );
        }
        let req = PeerWakeRequest {
            peer_url: args.peer_url,
            target: args.target,
            task: args.task,
            from: args.from,
            peer_key: key,
            timestamp: args.timestamp.unwrap_or(0),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())
            .ok();
        let Some(rt) = rt else {
            return HostResult::err(HostErrorCode::NetworkError, "tokio runtime failed");
        };
        let client = match ReqwestHttpTransportIo::new(self.http_timeout_ms) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        match rt.block_on(client.wake_peer(&req)) {
            Ok(resp) => HostResult::ok(
                json!({"ok": resp.ok, "status": resp.status, "target": resp.target, "error": resp.error}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        }
    }

    fn tmux_list_sessions(&self, _input: &str) -> HostResult<Value> {
        if let Err(err) = self.caps.require("tmux", "read", None) {
            return err;
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        HostResult::ok(json!({"sessions": tmux_sessions_json(client.list_all())}))
    }

    fn tmux_capture(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxCaptureArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "capture", None) && !self.caps.contains("tmux", "read", None)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux capture capability denied",
            );
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.capture(&args.target, args.lines) {
            Ok(mut content) => {
                if args.strip_ansi.unwrap_or(false) {
                    content = maw_tmux::strip_tmux_ansi(&content);
                }
                HostResult::ok(
                    json!({"target": args.target, "content": content, "lines": args.lines.unwrap_or(80)}),
                )
            }
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

    fn tmux_send_keys(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<TmuxSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let text = args.keys.join(" ");
        let destructive = maw_tmux::check_destructive(&text);
        let needs_force = destructive.destructive
            || args.force.unwrap_or(false)
            || args.allow_destructive.unwrap_or(false);
        let has_force_cap =
            self.has_exact_cap("tmux:send:force") || self.has_exact_cap("tmux:send:*");
        let cap = if needs_force {
            if !has_force_cap {
                return HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "tmux send force capability denied",
                );
            }
            "tmux:send:force"
        } else if self.caps.contains("tmux", "send", None) {
            "tmux:send"
        } else {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send capability denied",
            );
        };
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        let pane_command = match self.tmux_current_command(&args.target, &mut client) {
            Ok(command) => command,
            Err(err) => return err,
        };
        if maw_tmux::is_claude_like_pane(Some(&pane_command))
            && !has_force_cap
            && !args.allow_ai_pane.unwrap_or(false)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send into AI-agent pane denied",
            );
        }
        if !self.tmux_dry_run {
            let send = if args.literal.unwrap_or(false) {
                client.send_keys_literal(&args.target, &text)
            } else {
                client.send_keys(&args.target, &args.keys)
            };
            if let Err(error) = send {
                return HostResult::err(HostErrorCode::IoError, error.message);
            }
            if args.enter.unwrap_or(false) {
                if let Err(error) = client.send_enter(&args.target) {
                    return HostResult::err(HostErrorCode::IoError, error.message);
                }
            }
        }
        let result = HostResult::ok(
            json!({"target": args.target, "sent": true, "destructive": destructive.destructive}),
        );
        self.audit(
            "maw.tmux.send_keys",
            cap,
            &args.target,
            status_of(&result),
            start,
        );
        result
    }

    fn tmux_run(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        self.tmux_send_keys(
            &serde_json::to_string(&TmuxSendArgs {
                target: args.target.clone(),
                keys: vec![args.text],
                literal: Some(true),
                enter: Some(true),
                allow_destructive: Some(false),
                force: Some(false),
                allow_ai_pane: Some(false),
            })
            .unwrap_or_default(),
        )
    }

    fn tmux_send_enter(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxEnterArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "send", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send capability denied",
            );
        }
        let count = args.count.unwrap_or(1).min(5);
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        for _ in 0..count {
            if let Err(error) = client.send_enter(&args.target) {
                return HostResult::err(HostErrorCode::IoError, error.message);
            }
        }
        HostResult::ok(json!({"target": args.target, "count": count}))
    }

    fn tmux_tags_read(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsPathArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "read", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux read capability denied",
            );
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.read_pane_tags(&args.path) {
            Ok(tags) => HostResult::ok(json!({"title": tags.title, "meta": tags.meta})),
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

    fn tmux_tags_write(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxTagsWriteArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "write-tags", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux write-tags capability denied",
            );
        }
        let meta = args.meta.unwrap_or_default();
        if meta
            .keys()
            .any(|key| !key.starts_with("@maw-") && !key.starts_with("maw-"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tag keys must use @maw-* namespace",
            );
        }
        let pairs = meta.into_iter().collect::<Vec<_>>();
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.tag_pane(&args.target, args.title.as_deref(), &pairs) {
            Ok(()) => HostResult::ok(json!({"target": args.target})),
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

    fn ssh_exec(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshExecArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_ssh_host_token(&args.host) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "ssh host must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', or '-'",
            );
        }
        if !self.caps.contains("shell", "ssh", Some(&args.host))
            || !self.caps.contains("proc", "exec", Some("ssh"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "ssh exec capability denied",
            );
        }
        if args
            .args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-A" | "-L" | "-R" | "-D" | "-tt" | "-t"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "interactive/forwarding ssh options denied",
            );
        }
        let mut cmd = Command::new("ssh");
        cmd.arg("-T")
            .arg("--")
            .arg(&args.host)
            .arg(&args.cmd)
            .args(&args.args)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());
        let output = run_child(
            cmd,
            args.stdin.as_deref(),
            args.timeout_ms.unwrap_or(10_000).min(MAX_EXEC_TIMEOUT_MS),
        );
        match output {
            Ok(output) => HostResult::ok(
                json!({"transport": "ssh", "host": args.host, "status": output.status.code().unwrap_or(-1), "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}),
            ),
            Err(code) => HostResult::err(code, "ssh execution failed"),
        }
    }

    fn ssh_tmux_capture(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshTmuxCaptureArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_tmux_target_token(&args.target) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "tmux target must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', '%', or '-'",
            );
        }
        self.ssh_exec(
            &serde_json::to_string(&SshExecArgs {
                host: args.host,
                cmd: "tmux".to_owned(),
                args: vec![
                    "capture-pane".to_owned(),
                    "-p".to_owned(),
                    "-t".to_owned(),
                    args.target,
                    "-S".to_owned(),
                    format!("-{}", args.lines.unwrap_or(80)),
                ],
                stdin: None,
                timeout_ms: None,
            })
            .unwrap_or_default(),
        )
    }

    fn ssh_tmux_send_keys(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshTmuxSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_tmux_target_token(&args.target) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "tmux target must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', '%', or '-'",
            );
        }
        self.ssh_exec(
            &serde_json::to_string(&SshExecArgs {
                host: args.host,
                cmd: "tmux".to_owned(),
                args: [
                    vec!["send-keys".to_owned(), "-t".to_owned(), args.target],
                    args.keys,
                ]
                .concat(),
                stdin: None,
                timeout_ms: None,
            })
            .unwrap_or_default(),
        )
    }

    fn secure_path(
        &self,
        requested: &str,
        verb: &str,
    ) -> Result<(String, PathBuf), HostResult<Value>> {
        let path = canonicalize_checked(requested)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        let roots = self.roots_for(verb);
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared roots",
                )
            })?;
        let cap = self.caps.require("fs", verb, Some(scope))?;
        Ok((cap, path))
    }

    fn secure_write_path(&self, requested: &str) -> Result<(String, PathBuf), HostResult<Value>> {
        let raw = Path::new(requested);
        let path = resolve_write_path(raw)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        self.deny_protected_security_path(&path)?;
        let roots = self.roots_for("write");
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared write roots",
                )
            })?;
        let cap = self.caps.require("fs", "write", Some(scope))?;
        Ok((cap, path))
    }

    fn secure_remove_path(&self, requested: &str) -> Result<(String, PathBuf), HostResult<Value>> {
        if contains_glob_pattern(requested) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "glob/wildcard filesystem paths are denied",
            ));
        }
        let raw = Path::new(requested);
        let meta = std::fs::symlink_metadata(raw).map_err(|error| {
            HostResult::err(
                if error.kind() == std::io::ErrorKind::NotFound {
                    HostErrorCode::NotFound
                } else {
                    HostErrorCode::IoError
                },
                format!("stat failed: {error}"),
            )
        })?;
        if meta.file_type().is_symlink() {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "symlink deletion is denied",
            ));
        }
        let path = canonicalize_checked_path(raw)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        self.deny_protected_security_path(&path)?;
        let roots = self.roots_for("write");
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared write roots",
                )
            })?;
        let cap = self.caps.require("fs", "write", Some(scope))?;
        Ok((cap, path))
    }

    fn roots_for(&self, verb: &str) -> BTreeMap<String, PathBuf> {
        self.caps
            .scopes_for("fs", verb)
            .into_iter()
            .filter_map(|scope| {
                self.fs_roots
                    .get(&scope)
                    .and_then(|root| canonicalize_checked_path(root).ok())
                    .map(|root| (scope, root))
            })
            .collect()
    }

    fn check_cwd(&self, cwd: &str) -> Result<(), HostResult<Value>> {
        let cwd = canonicalize_checked(cwd)?;
        let roots = self
            .roots_for("read")
            .into_values()
            .chain(self.roots_for("write").into_values())
            .collect::<Vec<_>>();
        if roots.iter().any(|root| cwd.starts_with(root)) {
            Ok(())
        } else {
            Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "cwd outside declared filesystem roots",
            ))
        }
    }

    fn secret_ref(&self, key: Option<&str>) -> Result<String, HostResult<Value>> {
        let Some(key) = key else {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peerKeyRef is required",
            ));
        };
        self.secret_store.get(key).cloned().ok_or_else(|| {
            HostResult::err(
                HostErrorCode::CapabilityDenied,
                "secret ref not available to plugin",
            )
        })
    }

    fn config_file_path(&self) -> Result<PathBuf, HostResult<Value>> {
        let root = self
            .fs_roots
            .get("config")
            .cloned()
            .unwrap_or_else(default_config_root);
        if deny_special_path(&root) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special config root denied",
            ));
        }
        if let Err(error) = std::fs::create_dir_all(&root) {
            return Err(HostResult::err(
                HostErrorCode::IoError,
                format!("create config root failed: {error}"),
            ));
        }
        let root = canonicalize_checked_path(&root)?;
        Ok(root.join("maw.config.json"))
    }

    fn resolve_localserver_url(&self) -> Result<Url, HostResult<Value>> {
        if let Some(url) = &self.localserver_url {
            return parse_localserver_base_url(url);
        }
        if let Ok(url) = std::env::var("MAW_LOCALSERVER_URL") {
            return parse_localserver_base_url(&url);
        }
        if let Ok(url) = std::env::var("MAW_ENGINE_URL") {
            return parse_localserver_base_url(&url);
        }
        let config_path = self.config_file_path()?;
        let config = read_config_json(&config_path).unwrap_or(Value::Null);
        let port = config
            .get("port")
            .and_then(json_u16)
            .or_else(|| std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()))
            .unwrap_or(31_745);
        parse_localserver_base_url(&format!("http://127.0.0.1:{port}"))
    }

    fn consent_state_root(&self) -> Result<PathBuf, HostResult<Value>> {
        let root = self
            .fs_roots
            .get("state")
            .cloned()
            .unwrap_or_else(default_state_root);
        if deny_special_path(&root) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special consent state root denied",
            ));
        }
        if root.exists() {
            canonicalize_checked_path(&root)
        } else {
            Ok(root)
        }
    }

    fn protected_security_paths(&self) -> Result<Vec<ProtectedPath>, HostResult<Value>> {
        let state_root = self.consent_state_root()?;
        [
            protected_dir(state_root.join("consent-pending")),
            protected_dir(state_root.join("consent")),
            protected_dir(state_root.join("trust")),
            protected_dir(state_root.join("pairing")),
            protected_file(state_root.join("trust.json")),
            protected_file(state_root.join("peer-key")),
            protected_file(state_root.join("peers.json")),
            protected_file(state_root.join("pair-code-store.json")),
            protected_file(state_root.join("recent-hellos.json")),
            protected_file(state_root.join("audit.jsonl")),
            protected_file(state_root.join("audit.log")),
            protected_file(state_root.join("audit.ndjson")),
        ]
        .into_iter()
        .map(resolve_protected_path)
        .collect()
    }

    fn deny_protected_security_path(&self, path: &Path) -> Result<(), HostResult<Value>> {
        if path_is_protected_security_state(path, &self.protected_security_paths()?) {
            Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "protected security-state path denied",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExtismWasmInvokeRuntime {
    host_overrides: BTreeMap<String, MawWasmHost>,
}

impl ExtismWasmInvokeRuntime {
    #[must_use]
    pub fn with_host(mut self, plugin_name: impl Into<String>, host: MawWasmHost) -> Self {
        self.host_overrides.insert(plugin_name.into(), host);
        self
    }
}

impl PluginInvokeRuntime for ExtismWasmInvokeRuntime {
    fn invoke_ts(&mut self, _plugin: &LoadedPlugin, _ctx: &InvokeContext) -> InvokeResult {
        InvokeResult::error("TS plugin runtime is not available in Extism runtime")
    }

    fn invoke_wasm(
        &mut self,
        plugin: &LoadedPlugin,
        ctx: &InvokeContext,
        wasm_bytes: &[u8],
    ) -> InvokeResult {
        let host = self
            .host_overrides
            .remove(&plugin.manifest.name)
            .unwrap_or_else(|| MawWasmHost::new(plugin));
        let manifest = ExtismManifest::new([Wasm::data(wasm_bytes.to_vec())])
            .with_allowed_hosts(host.caps.scopes_for("net", "https").into_iter());
        let mut builder = PluginBuilder::new(manifest).with_wasi(false);
        for name in HOST_FN_NAMES {
            let fn_name = (*name).to_owned();
            let data = UserData::new(host.clone());
            builder = builder.with_function(
                *name,
                [ValType::I64],
                [ValType::I64],
                data,
                move |plugin, inputs, outputs, user_data| {
                    extism_host_call_named(plugin, inputs, outputs, &user_data, &fn_name)
                },
            );
        }
        let mut runtime = match builder.build() {
            Ok(plugin) => plugin,
            Err(error) => {
                return InvokeResult::error(format!("wasm instantiation failed: {error}"))
            }
        };
        let input = invoke_context_json(ctx);
        match runtime.call::<&str, String>(&plugin.wasm_export, &input) {
            Ok(output) => {
                parse_invoke_result_stdout(output.as_bytes()).unwrap_or_else(InvokeResult::error)
            }
            Err(error) => InvokeResult::error(format!("wasm call failed: {error}")),
        }
    }
}

pub const HOST_FN_NAMES: &[&str] = &[
    "maw.exec.run",
    "maw.exec.spawn",
    "maw.config.get",
    "maw.config.set",
    "maw.consent.read",
    "maw.fs.read",
    "maw.fs.write",
    "maw.fs.remove",
    "maw.fs.list",
    "maw.fs.stat",
    "maw.http.request",
    "maw.localserver.request",
    "maw.http.peer_send",
    "maw.http.peer_wake",
    "maw.tmux.list_sessions",
    "maw.tmux.capture",
    "maw.tmux.send_keys",
    "maw.tmux.run",
    "maw.tmux.send_enter",
    "maw.tmux.tags_read",
    "maw.tmux.tags_write",
    "maw.ssh.exec",
    "maw.ssh.tmux_capture",
    "maw.ssh.tmux_send_keys",
];

fn extism_host_call_named(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    host: &UserData<MawWasmHost>,
    name: &str,
) -> Result<(), extism::Error> {
    let input: String = plugin.memory_get_val(&inputs[0])?;
    let host = host.get()?;
    let host = host
        .lock()
        .map_err(|_| extism::Error::msg("host lock failed"))?;
    let output = host.handle_json(name, &input);
    plugin.memory_set_val(&mut outputs[0], output)?;
    Ok(())
}

fn parse_args<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T, HostResult<Value>> {
    serde_json::from_str(input).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("invalid JSON args: {error}"),
        )
    })
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"serialize failed","code":"io_error"}"#.to_owned()
    })
}

fn status_of<T>(result: &HostResult<T>) -> &'static str {
    match result {
        HostResult::Ok { .. } => "ok",
        HostResult::Err { .. } => "error",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExecRunArgs {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    env: Option<BTreeMap<String, String>>,
    stdin: Option<String>,
    timeout_ms: Option<u64>,
    #[serde(default)]
    allow_non_zero: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsReadArgs {
    path: String,
    encoding: Option<String>,
    max_bytes: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct FsPathArgs {
    #[serde(alias = "target")]
    path: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsRemoveArgs {
    path: String,
    recursive: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsWriteArgs {
    path: String,
    content: String,
    encoding: Option<String>,
    mode: Option<String>,
    mkdirp: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsListArgs {
    path: String,
    recursive: Option<bool>,
    max_entries: Option<usize>,
    include_dirs: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigGetArgs {
    key: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSetArgs {
    key: String,
    value: Value,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentReadArgs {
    view: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpArgs {
    method: String,
    url: String,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
    follow_redirects: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalserverArgs {
    method: String,
    path: Option<String>,
    url: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerSendArgs {
    peer_url: String,
    target: String,
    text: String,
    inbox: Option<bool>,
    from: String,
    peer_key_ref: Option<String>,
    timestamp: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerWakeArgs {
    peer_url: String,
    target: String,
    task: Option<String>,
    from: String,
    peer_key_ref: Option<String>,
    timestamp: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TmuxCaptureArgs {
    target: String,
    lines: Option<u32>,
    strip_ansi: Option<bool>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TmuxSendArgs {
    target: String,
    keys: Vec<String>,
    literal: Option<bool>,
    enter: Option<bool>,
    allow_destructive: Option<bool>,
    force: Option<bool>,
    allow_ai_pane: Option<bool>,
}
#[derive(Debug, Deserialize)]
struct TmuxRunArgs {
    target: String,
    text: String,
}
#[derive(Debug, Deserialize)]
struct TmuxEnterArgs {
    target: String,
    count: Option<u32>,
}
#[derive(Debug, Deserialize)]
struct TmuxTagsWriteArgs {
    target: String,
    title: Option<String>,
    meta: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshExecArgs {
    host: String,
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    stdin: Option<String>,
    timeout_ms: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct SshTmuxCaptureArgs {
    host: String,
    target: String,
    lines: Option<u32>,
}
#[derive(Debug, Deserialize)]
struct SshTmuxSendArgs {
    host: String,
    target: String,
    keys: Vec<String>,
}

fn is_safe_ssh_host_token(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-'))
}

fn is_safe_tmux_target_token(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-'))
}

fn protected_file(path: PathBuf) -> ProtectedPath {
    ProtectedPath {
        path,
        kind: ProtectedPathKind::File,
    }
}
fn protected_dir(path: PathBuf) -> ProtectedPath {
    ProtectedPath {
        path,
        kind: ProtectedPathKind::Dir,
    }
}

fn resolve_protected_path(protected: ProtectedPath) -> Result<ProtectedPath, HostResult<Value>> {
    if protected.path.exists() {
        Ok(ProtectedPath {
            path: canonicalize_checked_path(&protected.path)?,
            kind: protected.kind,
        })
    } else {
        Ok(protected)
    }
}

fn path_is_protected_security_state(path: &Path, protected: &[ProtectedPath]) -> bool {
    protected
        .iter()
        .any(|protected_path| match protected_path.kind {
            ProtectedPathKind::File => path == protected_path.path,
            ProtectedPathKind::Dir => path.starts_with(&protected_path.path),
        })
}

fn resolve_write_path(raw: &Path) -> Result<PathBuf, HostResult<Value>> {
    if std::fs::symlink_metadata(raw).is_ok() {
        return canonicalize_checked_path(raw);
    }
    let parent = raw
        .parent()
        .ok_or_else(|| HostResult::err(HostErrorCode::InvalidArgs, "write path requires parent"))?;
    let parent = canonicalize_checked_path(parent)?;
    let file_name = raw.file_name().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "write path requires file name")
    })?;
    Ok(parent.join(file_name))
}

fn executable_basename(cmd: &str) -> String {
    Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
        .to_owned()
}
fn is_hard_denied_exec(cmd: &str, args: &[String]) -> bool {
    matches!(
        executable_basename(cmd).as_str(),
        "sudo" | "su" | "doas" | "pkexec"
    ) || args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--pty" | "--ffi"))
}
fn sanitize_env(
    env: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, HostResult<Value>> {
    let mut clean = BTreeMap::new();
    clean.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
    if let Some(env) = env {
        for (key, value) in env {
            let lower = key.to_lowercase();
            if lower.contains("secret") || lower.contains("token") || lower.contains("peerkey") {
                return Err(HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "secret-like env keys are denied",
                ));
            }
            if key.starts_with("MAW_") {
                clean.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(clean)
}

fn run_child(
    mut cmd: Command,
    stdin: Option<&str>,
    timeout_ms: u64,
) -> Result<std::process::Output, HostErrorCode> {
    let mut child = cmd.spawn().map_err(|_| HostErrorCode::ProcessFailed)?;
    if let Some(input) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(input.as_bytes())
                .map_err(|_| HostErrorCode::IoError)?;
        }
    }
    let deadline = Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|_| HostErrorCode::IoError),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HostErrorCode::Timeout);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => return Err(HostErrorCode::ProcessFailed),
        }
    }
}

fn canonicalize_checked(path: &str) -> Result<PathBuf, HostResult<Value>> {
    canonicalize_checked_path(Path::new(path))
}
fn canonicalize_checked_path(path: &Path) -> Result<PathBuf, HostResult<Value>> {
    std::fs::canonicalize(path).map_err(|error| {
        HostResult::err(
            if error.kind() == std::io::ErrorKind::NotFound {
                HostErrorCode::NotFound
            } else {
                HostErrorCode::IoError
            },
            format!("canonicalize failed: {error}"),
        )
    })
}
fn deny_special_path(path: &Path) -> bool {
    path.starts_with("/proc")
        || path.starts_with("/dev")
        || path.starts_with("/sys")
        || path.starts_with("/root")
}
fn default_config_root() -> PathBuf {
    if let Some(path) = std::env::var_os("MAW_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("MAW_HOME") {
        return PathBuf::from(path).join("config");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".config").join("maw"),
        |home| PathBuf::from(home).join(".config").join("maw"),
    )
}
fn default_state_root() -> PathBuf {
    if let Some(path) = std::env::var_os("MAW_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("MAW_HOME") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("maw");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".local").join("state").join("maw"),
        |home| PathBuf::from(home).join(".local").join("state").join("maw"),
    )
}

fn contains_glob_pattern(path: &str) -> bool {
    path.chars()
        .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

fn remove_bounded_path(
    path: &Path,
    recursive: bool,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, HostResult<Value>> {
    if !roots.values().any(|root| path.starts_with(root)) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        HostResult::err(
            if error.kind() == std::io::ErrorKind::NotFound {
                HostErrorCode::NotFound
            } else {
                HostErrorCode::IoError
            },
            format!("stat failed: {error}"),
        )
    })?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "symlink deletion is denied",
        ));
    }
    if file_type.is_file() {
        let file = open_nofollow_existing(path)?;
        verify_fd_path(&file, path)?;
        drop(file);
        std::fs::remove_file(path).map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("remove file failed: {error}"),
            )
        })?;
        return Ok(true);
    }
    if file_type.is_dir() {
        if !recursive {
            std::fs::remove_dir(path).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove dir failed: {error}"),
                )
            })?;
            return Ok(true);
        }
        remove_bounded_dir_recursive(path, roots)?;
        return Ok(true);
    }
    Err(HostResult::err(
        HostErrorCode::CapabilityDenied,
        "device/special file deletion denied",
    ))
}

fn remove_bounded_dir_recursive(
    path: &Path,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<(), HostResult<Value>> {
    if !roots.values().any(|root| path.starts_with(root)) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    for entry in std::fs::read_dir(path).map_err(|error| {
        HostResult::err(HostErrorCode::IoError, format!("read dir failed: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("read dir entry failed: {error}"),
            )
        })?;
        let child = entry.path();
        if !child.starts_with(path) || !roots.values().any(|root| child.starts_with(root)) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "filesystem path outside declared write roots",
            ));
        }
        let meta = std::fs::symlink_metadata(&child).map_err(|error| {
            HostResult::err(HostErrorCode::IoError, format!("stat failed: {error}"))
        })?;
        let file_type = meta.file_type();
        if file_type.is_symlink() {
            std::fs::remove_file(&child).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove symlink failed: {error}"),
                )
            })?;
        } else if file_type.is_dir() {
            remove_bounded_dir_recursive(&child, roots)?;
        } else if file_type.is_file() {
            let file = open_nofollow_existing(&child)?;
            verify_fd_path(&file, &child)?;
            drop(file);
            std::fs::remove_file(&child).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove file failed: {error}"),
                )
            })?;
        } else {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "device/special file deletion denied",
            ));
        }
    }
    std::fs::remove_dir(path).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("remove dir failed: {error}"),
        )
    })
}

fn open_nofollow_existing(path: &Path) -> Result<File, HostResult<Value>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_FLAG)
        .open(path)
        .map_err(|error| {
            HostResult::err(HostErrorCode::IoError, format!("open failed: {error}"))
        })?;
    let meta = file.metadata().map_err(|error| {
        HostResult::err(HostErrorCode::IoError, format!("metadata failed: {error}"))
    })?;
    if !meta.file_type().is_file() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "device/special file denied",
        ));
    }
    Ok(file)
}
fn verify_fd_path(file: &File, expected: &Path) -> Result<(), HostResult<Value>> {
    let actual = fd_real_path(file)?;
    if actual == expected {
        Ok(())
    } else {
        Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem race detected",
        ))
    }
}

fn verify_fd_under_roots(
    file: &File,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<(), HostResult<Value>> {
    let actual = fd_real_path(file)?;
    if roots.values().any(|root| actual.starts_with(root)) {
        Ok(())
    } else {
        Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem race detected",
        ))
    }
}

#[cfg(target_os = "linux")]
fn fd_real_path(file: &File) -> Result<PathBuf, HostResult<Value>> {
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("fd reverify failed: {error}"),
        )
    })
}

#[cfg(target_os = "macos")]
fn fd_real_path(file: &File) -> Result<PathBuf, HostResult<Value>> {
    rustix::fs::getpath(file.as_fd())
        .map(|path| PathBuf::from(OsString::from_vec(path.into_bytes())))
        .map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("fd reverify failed: {error}"),
            )
        })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn fd_real_path(_file: &File) -> Result<PathBuf, HostResult<Value>> {
    Err(HostResult::err(
        HostErrorCode::IoError,
        "fd reverify unsupported on this platform",
    ))
}
fn read_config_json(path: &Path) -> Result<Value, HostResult<Value>> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let file = open_nofollow_existing(path)?;
    verify_fd_path(&file, path)?;
    let mut raw = String::new();
    if let Err(error) = file.take(MAX_READ_BYTES + 1).read_to_string(&mut raw) {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            format!("read config failed: {error}"),
        ));
    }
    if raw.len() as u64 > MAX_READ_BYTES {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            "config exceeds maxBytes",
        ));
    }
    serde_json::from_str(&raw).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("config JSON parse failed: {error}"),
        )
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentPendingRow {
    id: String,
    from: String,
    to: String,
    action: String,
    summary: String,
    #[serde(rename = "pinHash", skip_serializing)]
    _pin_hash: Option<String>,
    created_at: String,
    expires_at: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentTrustRow {
    from: String,
    to: String,
    action: String,
    approved_at: String,
    approved_by: Option<String>,
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConsentTrustFile {
    #[serde(default)]
    trust: BTreeMap<String, ConsentTrustRow>,
}

fn read_consent_pending(state_root: &Path) -> Result<Vec<ConsentPendingRow>, HostResult<Value>> {
    let dir = state_root.join("consent-pending");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let dir = canonicalize_checked_path(&dir)?;
    if deny_special_path(&dir) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "special consent pending path denied",
        ));
    }
    let mut rows = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("read pending dir failed: {error}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("read pending entry failed: {error}"),
            )
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let extension = Path::new(name).extension().and_then(|ext| ext.to_str());
        if extension.is_none_or(|ext| !ext.eq_ignore_ascii_case("json"))
            || extension.is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        {
            continue;
        }
        if let Ok(value) = read_json_file(&path) {
            if let Ok(row) = serde_json::from_value::<ConsentPendingRow>(value) {
                rows.push(row);
            }
        }
    }
    rows.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(rows)
}

fn read_consent_trust(state_root: &Path) -> Result<Vec<ConsentTrustRow>, HostResult<Value>> {
    let path = state_root.join("trust.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file: ConsentTrustFile =
        serde_json::from_value(read_json_file(&path)?).map_err(|error| {
            HostResult::err(
                HostErrorCode::InvalidArgs,
                format!("trust JSON parse failed: {error}"),
            )
        })?;
    let mut rows = file.trust.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.approved_at.cmp(&right.approved_at));
    Ok(rows)
}

fn read_json_file(path: &Path) -> Result<Value, HostResult<Value>> {
    let path = canonicalize_checked_path(path)?;
    let file = open_nofollow_existing(&path)?;
    verify_fd_path(&file, &path)?;
    let mut raw = String::new();
    if let Err(error) = file.take(MAX_READ_BYTES + 1).read_to_string(&mut raw) {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            format!("read JSON failed: {error}"),
        ));
    }
    if raw.len() as u64 > MAX_READ_BYTES {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            "JSON exceeds maxBytes",
        ));
    }
    serde_json::from_str(&raw).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("JSON parse failed: {error}"),
        )
    })
}

fn format_consent_pending(rows: &[ConsentPendingRow]) -> String {
    if rows.is_empty() {
        return "no pending consent requests".to_owned();
    }
    let mut lines = vec![
        "id                        from → to             action            status   summary"
            .to_owned(),
    ];
    for row in rows {
        let id = pad(&row.id, 24);
        let from_to = pad(&format!("{} → {}", row.from, row.to), 20);
        let action = pad(&row.action, 16);
        let status = pad(&row.status, 8);
        let summary = truncate_summary(&row.summary);
        lines.push(format!("{id}  {from_to}  {action}  {status}  {summary}"));
    }
    lines.join("\n")
}

fn format_consent_trust(rows: &[ConsentTrustRow]) -> String {
    if rows.is_empty() {
        return "no trust entries".to_owned();
    }
    let mut lines = vec!["from → to                action            approvedAt".to_owned()];
    for row in rows {
        let from_to = pad(&format!("{} → {}", row.from, row.to), 22);
        let action = pad(&row.action, 16);
        lines.push(format!("{from_to}  {action}  {}", row.approved_at));
    }
    lines.join("\n")
}

fn pad(value: &str, width: usize) -> String {
    if value.chars().count() >= width {
        value.to_owned()
    } else {
        format!("{value}{}", " ".repeat(width - value.chars().count()))
    }
}

fn truncate_summary(value: &str) -> String {
    if value.chars().count() <= 50 {
        return value.to_owned();
    }
    format!("{}…", value.chars().take(47).collect::<String>())
}
fn write_config_json(path: &Path, config: &Value) -> Result<(), HostResult<Value>> {
    let parent = path.parent().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "config path requires parent")
    })?;
    let parent = canonicalize_checked_path(parent)?;
    if deny_special_path(&parent) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "special config root denied",
        ));
    }
    let path = parent.join("maw.config.json");
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create(true)
        .truncate(true)
        .custom_flags(O_NOFOLLOW_FLAG);
    let mut file = opts.open(&path).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("open config failed: {error}"),
        )
    })?;
    verify_fd_path(&file, &path)?;
    let content = serde_json::to_string_pretty(config).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("serialize config failed: {error}"),
        )
    })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("write config failed: {error}"),
        )
    })?;
    file.write_all(b"\n").map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("write config failed: {error}"),
        )
    })?;
    Ok(())
}
fn get_json_path<'a>(value: &'a Value, key_path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in key_path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    Some(current)
}
fn set_json_path(
    target: &mut Value,
    key_path: &str,
    value: Value,
) -> Result<(), HostResult<Value>> {
    let parts = key_path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some((last, parents)) = parts.split_last() else {
        return Err(HostResult::err(
            HostErrorCode::InvalidArgs,
            "config key is required",
        ));
    };
    if !target.is_object() {
        *target = json!({});
    }
    let mut current = target;
    for part in parents {
        let object = current.as_object_mut().ok_or_else(|| {
            HostResult::err(
                HostErrorCode::InvalidArgs,
                "config path conflicts with non-object value",
            )
        })?;
        current = object
            .entry((*part).to_owned())
            .or_insert_with(|| json!({}));
    }
    let object = current.as_object_mut().ok_or_else(|| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            "config path conflicts with non-object value",
        )
    })?;
    object.insert((*last).to_owned(), value);
    Ok(())
}
const PLUGIN_WRITABLE_CONFIG_KEYS: &[&str] = &[
    // Keep this host-side allowlist intentionally small: sandboxed plugins may
    // only write config keys that are already parity-backed as safe user-facing
    // `maw config set` targets. Everything else is denied by default.
    "node",
    "port",
];

fn is_plugin_writable_config_key_path(key: &str) -> bool {
    normalized_config_key_path(key).is_some_and(|key| {
        PLUGIN_WRITABLE_CONFIG_KEYS
            .iter()
            .any(|allowed| key == *allowed)
    })
}

fn normalized_config_key_path(key: &str) -> Option<String> {
    let parts = key
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn is_secret_like_config_key_path(key: &str) -> bool {
    let lower = key.to_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "credential",
        "private",
        "privatekey",
        "private_key",
        "passphrase",
        "cert",
        "pem",
        "secret",
        "token",
        "apikey",
        "api_key",
        "peerkey",
        "peer_key",
        "oauth",
        "auth_token",
        "auth-token",
        "authtoken",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || Path::new(&lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("key"))
        || Path::new(&lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("env"))
        || lower == "key"
}
fn value_contains_secret_config_key_path(prefix: &str, value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            is_secret_like_config_key_path(&path) || value_contains_secret_config_key_path(&path, value)
        }),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_secret_config_key_path(prefix, value)),
        _ => false,
    }
}
fn file_kind(file_type: std::fs::FileType) -> &'static str {
    if file_type.is_dir() {
        "dir"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "file"
    }
}

fn list_dir(path: &Path, recursive: bool, include_dirs: bool, max: usize, out: &mut Vec<Value>) {
    if out.len() >= max {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= max {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let kind = file_kind(meta.file_type());
        if include_dirs || kind != "dir" {
            out.push(json!({
                "path": entry.path().display().to_string(),
                "kind": kind,
                "bytes": meta.len()
            }));
        }
        if recursive && kind == "dir" {
            list_dir(&entry.path(), true, include_dirs, max, out);
        }
    }
}
fn redact_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .map(|(key, value)| {
            let lower = key.to_lowercase();
            if [
                "authorization",
                "token",
                "secret",
                "peerkey",
                "cookie",
                "api-key",
                "x-api-key",
                "bearer",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
            {
                (key, "[REDACTED]".to_owned())
            } else {
                (key, value)
            }
        })
        .collect()
}
fn redact(value: &str) -> String {
    let mut out = value.to_owned();
    for marker in ["peerKey", "token", "secret", "authorization"] {
        if out.to_lowercase().contains(&marker.to_lowercase()) {
            "[REDACTED]".clone_into(&mut out);
        }
    }
    out
}

fn json_u16(value: &Value) -> Option<u16> {
    if let Some(port) = value.as_u64() {
        return u16::try_from(port).ok();
    }
    value.as_str()?.parse::<u16>().ok()
}

fn parse_localserver_base_url(raw: &str) -> Result<Url, HostResult<Value>> {
    let mut url = Url::parse(raw.trim_end_matches('/')).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("invalid localserver url: {error}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver URL must be http/https",
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "localserver URL host is required")
    })?;
    if !is_localserver_host(host) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver URL must resolve to loopback",
        ));
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn pinned_localserver_url(
    base: &Url,
    path: Option<&str>,
    requested_url: Option<&str>,
) -> Result<Url, HostResult<Value>> {
    let url = if let Some(raw) = requested_url {
        Url::parse(raw).map_err(|error| {
            HostResult::err(HostErrorCode::InvalidArgs, format!("invalid url: {error}"))
        })?
    } else {
        let path = path.unwrap_or("/");
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "localserver path must be absolute and hostless",
            ));
        }
        base.join(path.trim_start_matches('/')).map_err(|error| {
            HostResult::err(HostErrorCode::InvalidArgs, format!("invalid path: {error}"))
        })?
    };
    if !same_origin(base, &url) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver request denied: URL is not the host-pinned maw server endpoint",
        ));
    }
    Ok(url)
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn is_localserver_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| ip.to_canonical().is_loopback())
}

fn is_discord_gateway(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.contains("discord") && url.path().contains("gateway"))
}
fn private_host_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.to_lowercase().ends_with(".local")
}

fn private_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    }
}

fn tmux_sessions_json(sessions: Vec<maw_tmux::TmuxSession>) -> Vec<Value> {
    sessions
        .into_iter()
        .map(|session| {
            json!({
                "name": session.name,
                "windows": session.windows.into_iter().map(|window| json!({
                    "index": window.index,
                    "name": window.name,
                    "active": window.active,
                    "cwd": window.cwd,
                })).collect::<Vec<_>>()
            })
        })
        .collect()
}

#[cfg(test)]
mod fd_reverify_tests {
    use super::*;
    use std::fs::{create_dir_all, rename, write, OpenOptions};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "maw-fd-reverify-{label}-{}-{nonce}",
            std::process::id()
        ));
        create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn fd_reverify_detects_open_file_path_mismatch() {
        let dir = temp_dir("path-mismatch");
        let expected = dir.join("target.txt");
        let moved = dir.join("moved.txt");
        write(&expected, "original").expect("target");

        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_FLAG)
            .open(&expected)
            .expect("open target");
        rename(&expected, &moved).expect("rename opened file");
        write(&expected, "replacement").expect("replacement");

        let err = verify_fd_path(&file, &expected).expect_err("mismatch denied");
        assert!(matches!(
            err,
            HostResult::Err {
                code: HostErrorCode::CapabilityDenied,
                ..
            }
        ));
    }
}
