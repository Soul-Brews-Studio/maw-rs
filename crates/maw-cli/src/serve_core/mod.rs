pub mod modules;
pub mod process_engine;

pub use process_engine::{ServecoreExecRunner, ServecoreNativeEngine, ServecoreProcessRunner};

use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo,
    },
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use maw_hub::WorkspaceConfig;
use maw_tmux::{TmuxClient, TmuxPane, TmuxSession};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SERVECORE_PIPELINE_ORDER: &[&str] = &[
    "cors-preflight",
    "ws-upgrade",
    "engine-proxy",
    "api-protected-auth",
    "registry",
    "api-public",
    "registry",
    "fallback-views",
];
static SERVECORE_WS_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
const SERVECORE_ORCHESTRATION_BODY_LIMIT: usize = 64 * 1024;

pub trait ServecoreEngine: Send + Sync {
    fn servecore_engine_name(&self) -> &'static str;

    /// Opens a websocket stream for a registered serve-core route.
    ///
    /// # Errors
    ///
    /// Implementations may return an error when the requested stream target is unavailable.
    fn servecore_ws_open(
        &self,
        _kind: ServecoreWsKind,
        _target: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn servecore_ws_text(
        &self,
        _kind: ServecoreWsKind,
        text: &str,
        _target: Option<&str>,
    ) -> Option<String> {
        Some(text.to_owned())
    }

    fn servecore_ws_binary(
        &self,
        _kind: ServecoreWsKind,
        bytes: &[u8],
        _target: Option<&str>,
    ) -> Option<Vec<u8>> {
        Some(bytes.to_vec())
    }

    fn servecore_ws_close(&self, _kind: ServecoreWsKind, _target: Option<&str>) {}
}

#[derive(Debug)]
pub struct ServecoreStubEngine;

impl ServecoreEngine for ServecoreStubEngine {
    fn servecore_engine_name(&self) -> &'static str {
        "stub"
    }
}

#[derive(Clone)]
pub struct ServecoreSharedState {
    pub engine: Arc<dyn ServecoreEngine>,
    pub trigger_bus: ServecoreTriggerBus,
    pub thread_store: ServecoreThreadStore,
    pub orchestrator: Arc<dyn ServecoreOrchestrator>,
    pub lifecycle: ServecoreLifecycle,
    pub hub_workspaces: Arc<Vec<WorkspaceConfig>>,
    pub agents_node: Option<String>,
    pub agents_snapshot: Option<Arc<Vec<ServecoreAgentPane>>>,
    pub tmux_sessions_snapshot: Option<Arc<Vec<TmuxSession>>>,
    pub auth_workspace_key: Option<String>,
    pub auth_cached_pubkey: Option<String>,
    pub auth_ed25519_pins: maw_auth::Ed25519TofuPins,
    pub auth_now_override: Option<i64>,
}

impl Default for ServecoreSharedState {
    fn default() -> Self {
        Self {
            engine: Arc::new(ServecoreStubEngine),
            trigger_bus: ServecoreTriggerBus::default(),
            thread_store: ServecoreThreadStore::servecore_default(),
            orchestrator: Arc::new(ServecoreCommandOrchestrator::servecore_default()),
            lifecycle: ServecoreLifecycle::default(),
            hub_workspaces: Arc::new(Vec::new()),
            agents_node: None,
            agents_snapshot: None,
            tmux_sessions_snapshot: None,
            auth_workspace_key: None,
            auth_cached_pubkey: None,
            auth_ed25519_pins: Arc::new(Mutex::new(maw_auth::Ed25519TofuStore::default())),
            auth_now_override: None,
        }
    }
}

impl ServecoreSharedState {
    #[must_use]
    pub fn servecore_with_engine(mut self, engine: Arc<dyn ServecoreEngine>) -> Self {
        self.engine = engine;
        self
    }

    #[must_use]
    pub fn servecore_with_agents_node(mut self, node: Option<String>) -> Self {
        self.agents_node = node;
        self
    }

    #[must_use]
    pub fn servecore_with_agents_snapshot(mut self, panes: Vec<ServecoreAgentPane>) -> Self {
        self.agents_snapshot = Some(Arc::new(panes));
        self
    }

    #[must_use]
    pub fn servecore_with_tmux_sessions_snapshot(mut self, sessions: Vec<TmuxSession>) -> Self {
        self.tmux_sessions_snapshot = Some(Arc::new(sessions));
        self
    }

    #[must_use]
    pub fn servecore_agents_panes(&self) -> Vec<ServecoreAgentPane> {
        if let Some(snapshot) = &self.agents_snapshot {
            return snapshot.as_ref().clone();
        }
        let mut tmux = TmuxClient::local();
        tmux.list_panes()
            .into_iter()
            .map(ServecoreAgentPane::from)
            .collect()
    }

    #[must_use]
    pub fn servecore_tmux_sessions(&self) -> Vec<TmuxSession> {
        if let Some(snapshot) = &self.tmux_sessions_snapshot {
            return snapshot.as_ref().clone();
        }
        let mut tmux = TmuxClient::local();
        tmux.list_all()
    }

    #[must_use]
    pub fn servecore_with_thread_store(mut self, thread_store: ServecoreThreadStore) -> Self {
        self.thread_store = thread_store;
        self
    }

    #[must_use]
    pub fn servecore_with_orchestrator(
        mut self,
        orchestrator: Arc<dyn ServecoreOrchestrator>,
    ) -> Self {
        self.orchestrator = orchestrator;
        self
    }

    #[must_use]
    pub fn servecore_with_auth(
        mut self,
        workspace_key: Option<String>,
        cached_pubkey: Option<String>,
    ) -> Self {
        self.auth_workspace_key = workspace_key;
        self.auth_cached_pubkey = cached_pubkey;
        self
    }

    #[must_use]
    pub fn servecore_with_auth_pins(mut self, pins: maw_auth::Ed25519TofuPins) -> Self {
        self.auth_ed25519_pins = pins;
        self
    }

    #[must_use]
    pub fn servecore_with_process_auth_pins(self) -> Self {
        let store = maw_auth::Ed25519TofuStore::file_backed(servecore_ed25519_tofu_path());
        self.servecore_with_auth_pins(Arc::new(Mutex::new(store)))
    }

    #[cfg(test)]
    #[must_use]
    pub fn servecore_with_auth_now(mut self, now: i64) -> Self {
        self.auth_now_override = Some(now);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecoreAgentPane {
    pub id: String,
    pub command: String,
    pub target: String,
    pub title: String,
    pub cwd: Option<String>,
    pub pid: Option<u32>,
    pub last_activity: Option<u64>,
}

impl From<TmuxPane> for ServecoreAgentPane {
    fn from(pane: TmuxPane) -> Self {
        Self {
            id: pane.id,
            command: pane.command,
            target: pane.target,
            title: pane.title,
            cwd: pane.cwd,
            pid: pane.pid,
            last_activity: pane.last_activity,
        }
    }
}

#[derive(Clone, Default)]
pub struct ServecoreTriggerBus {
    events: Arc<Mutex<VecDeque<ServecoreTriggerEvent>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecoreTriggerEvent {
    pub name: String,
    pub payload: String,
}

impl ServecoreTriggerBus {
    pub fn servecore_fire(&self, event: ServecoreTriggerEvent) {
        let mut guard = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.push_back(event);
    }

    pub fn servecore_snapshot(&self) -> Vec<ServecoreTriggerEvent> {
        let guard = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.iter().cloned().collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreWorkonRequest {
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, rename = "with")]
    pub with_oracles: Vec<String>,
    #[serde(default)]
    pub attach: bool,
    #[serde(default)]
    pub split: bool,
    #[serde(default)]
    pub tiled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreWorkonHandle {
    pub ok: bool,
    pub repo: String,
    pub cwd: String,
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub argv: Vec<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_skipped: Option<String>,
}

pub trait ServecoreOrchestrator: Send + Sync {
    /// Spawn a native workon orchestration using argv vectors only.
    ///
    /// # Errors
    ///
    /// Returns an error when request validation fails, the repo escapes the configured
    /// root, or the child process exits unsuccessfully.
    fn spawn_workon(
        &self,
        request: ServecoreWorkonRequest,
        engine: Arc<dyn ServecoreEngine>,
    ) -> Result<ServecoreWorkonHandle, String>;
}

#[derive(Clone)]
pub struct ServecoreCommandOrchestrator {
    root: Arc<PathBuf>,
    runner: Arc<dyn ServecoreExecRunner>,
    pane_runner: Arc<dyn ServecorePaneRunner>,
}

impl ServecoreCommandOrchestrator {
    #[must_use]
    pub fn servecore_default() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::servecore_with_root(root)
    }

    #[must_use]
    pub fn servecore_with_root(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            runner: Arc::new(ServecoreProcessRunner),
            pane_runner: Arc::new(ServecoreTmuxPaneRunner),
        }
    }

    #[cfg(test)]
    pub fn servecore_with_runner(root: PathBuf, runner: Arc<dyn ServecoreExecRunner>) -> Self {
        Self {
            root: Arc::new(root),
            runner,
            pane_runner: Arc::new(TestPaneRunner),
        }
    }

    #[cfg(test)]
    pub fn servecore_with_runners(
        root: PathBuf,
        runner: Arc<dyn ServecoreExecRunner>,
        pane_runner: Arc<dyn ServecorePaneRunner>,
    ) -> Self {
        Self {
            root: Arc::new(root),
            runner,
            pane_runner,
        }
    }
}

impl ServecoreOrchestrator for ServecoreCommandOrchestrator {
    fn spawn_workon(
        &self,
        request: ServecoreWorkonRequest,
        engine: Arc<dyn ServecoreEngine>,
    ) -> Result<ServecoreWorkonHandle, String> {
        let plan = servecore_prepare_workon(&self.root, request, engine.servecore_engine_name())?;
        match plan {
            ServecorePreparedOrchestration::Simple(plan) => {
                self.runner.servecore_run(&plan.argv, &plan.repo_path)?;
                Ok(plan.into_handle("spawned"))
            }
            ServecorePreparedOrchestration::Advanced(plan) => {
                self.runner
                    .servecore_run(&plan.leader_argv, &plan.repo_path)?;
                Ok(self.servecore_finish_advanced(plan))
            }
        }
    }
}

impl ServecoreCommandOrchestrator {
    fn servecore_finish_advanced(&self, plan: ServecoreAdvancedWorkon) -> ServecoreWorkonHandle {
        let Some(swarm_argv) = plan.swarm_argv.clone() else {
            return plan.into_handle("spawned", None, None);
        };
        let Ok(panes) = self.pane_runner.servecore_list_panes() else {
            return plan.into_handle(
                "leader-spawned",
                None,
                Some("pane discovery failed".to_owned()),
            );
        };
        let Ok(pane) = servecore_find_pane_for_task(&panes, &plan.task) else {
            return plan.into_handle(
                "leader-spawned",
                None,
                Some("pane discovery failed".to_owned()),
            );
        };
        let Ok(line) = servecore_shell_line_for_self(&swarm_argv) else {
            return plan.into_handle("leader-spawned", None, Some("pane send failed".to_owned()));
        };
        if self
            .pane_runner
            .servecore_send_literal_enter(&pane, &line)
            .is_err()
        {
            return plan.into_handle("leader-spawned", None, Some("pane send failed".to_owned()));
        }
        plan.into_handle("spawned", Some(pane), None)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecorePaneCandidate {
    pub id: String,
    pub title: String,
}

pub trait ServecorePaneRunner: Send + Sync {
    /// Lists panes that may receive a follow-up swarm command.
    ///
    /// # Errors
    ///
    /// Returns an error when the pane backend cannot enumerate panes.
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String>;

    /// Sends one literal command line to the selected pane and presses Enter.
    ///
    /// # Errors
    ///
    /// Returns an error when the pane id is invalid or the backend rejects the send.
    fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct ServecoreTmuxPaneRunner;

impl ServecorePaneRunner for ServecoreTmuxPaneRunner {
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
        let mut tmux = TmuxClient::local();
        Ok(tmux
            .list_panes()
            .into_iter()
            .map(|pane| ServecorePaneCandidate {
                id: pane.id,
                title: pane.title,
            })
            .collect())
    }

    fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String> {
        servecore_validate_pane_id(pane)?;
        let mut tmux = TmuxClient::local();
        tmux.send_keys(pane, &["C-u".to_owned()])
            .map_err(|_| "serve-orchestration: tmux send failed".to_owned())?;
        tmux.send_text(pane, line)
            .map(|_| ())
            .map_err(|_| "serve-orchestration: tmux send failed".to_owned())
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestPaneRunner;

#[cfg(test)]
impl ServecorePaneRunner for TestPaneRunner {
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
        Ok(Vec::new())
    }

    fn servecore_send_literal_enter(&self, _pane: &str, _line: &str) -> Result<(), String> {
        Ok(())
    }
}

enum ServecorePreparedOrchestration {
    Simple(ServecorePreparedWorkon),
    Advanced(ServecoreAdvancedWorkon),
}

struct ServecorePreparedWorkon {
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
    engine: String,
    argv: Vec<String>,
}

impl ServecorePreparedWorkon {
    fn into_handle(self, status: &str) -> ServecoreWorkonHandle {
        ServecoreWorkonHandle {
            ok: true,
            repo: self.request.repo,
            cwd: self.repo_path.to_string_lossy().into_owned(),
            engine: self.engine,
            target: self.request.target,
            argv: self.argv,
            status: status.to_owned(),
            message: None,
            leader_argv: None,
            swarm_argv: None,
            pane: None,
            swarm_skipped: None,
        }
    }
}

struct ServecoreAdvancedWorkon {
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
    task: String,
    engine: String,
    leader_argv: Vec<String>,
    public_leader_argv: Vec<String>,
    swarm_argv: Option<Vec<String>>,
}

impl ServecoreAdvancedWorkon {
    fn into_handle(
        self,
        status: &str,
        pane: Option<String>,
        swarm_skipped: Option<String>,
    ) -> ServecoreWorkonHandle {
        ServecoreWorkonHandle {
            ok: true,
            repo: self.request.repo,
            cwd: self.repo_path.to_string_lossy().into_owned(),
            engine: self.engine,
            target: self.request.target,
            argv: self.public_leader_argv.clone(),
            status: status.to_owned(),
            message: None,
            leader_argv: Some(self.public_leader_argv),
            swarm_argv: self.swarm_argv,
            pane,
            swarm_skipped,
        }
    }
}

fn servecore_prepare_workon(
    root: &Path,
    request: ServecoreWorkonRequest,
    default_engine: &str,
) -> Result<ServecorePreparedOrchestration, String> {
    servecore_validate_path_text(&request.repo, "repo")?;
    if let Some(task) = &request.task {
        servecore_validate_command_token(task, "task")?;
    }
    if let Some(target) = &request.target {
        servecore_validate_command_token(target, "target")?;
    }
    if let Some(prompt) = &request.prompt {
        servecore_validate_prompt_text(prompt)?;
    }
    for oracle in &request.with_oracles {
        servecore_validate_command_token(oracle, "with")?;
    }
    let repo_path = servecore_resolve_workon_repo(root, &request.repo)?;
    if servecore_has_advanced_fields(&request) {
        return servecore_prepare_advanced_workon(request, repo_path);
    }
    let engine = request
        .engine
        .clone()
        .unwrap_or_else(|| default_engine.to_owned());
    servecore_validate_engine_token(&engine, "engine")?;
    let mut argv = vec!["workon".to_owned(), request.repo.clone()];
    if let Some(task) = &request.task {
        argv.push(task.clone());
    }
    argv.extend(["--layout".to_owned(), "nested".to_owned()]);
    Ok(ServecorePreparedOrchestration::Simple(
        ServecorePreparedWorkon {
            request,
            repo_path,
            engine,
            argv,
        },
    ))
}

fn servecore_prepare_advanced_workon(
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
) -> Result<ServecorePreparedOrchestration, String> {
    if request.attach {
        return Err("serve-orchestration attach is not supported for advanced wake".to_owned());
    }
    let task = request
        .task
        .clone()
        .ok_or_else(|| "serve-orchestration advanced wake requires task".to_owned())?;
    let engine = request
        .engine
        .clone()
        .unwrap_or_else(|| "claude47".to_owned());
    servecore_validate_command_token(&engine, "engine")?;
    let oracle = request
        .target
        .clone()
        .map_or_else(|| servecore_oracle_from_repo(&request.repo), Ok)?;
    servecore_validate_command_token(&oracle, "target")?;
    let mut leader_argv = vec![
        "wake".to_owned(),
        oracle,
        "--task".to_owned(),
        task.clone(),
        "--engine".to_owned(),
        engine.clone(),
        "--split".to_owned(),
        "--no-attach".to_owned(),
    ];
    if servecore_repo_arg_is_safe(&request.repo) {
        leader_argv.extend(["--repo".to_owned(), request.repo.clone()]);
    }
    if let Some(prompt) = &request.prompt {
        leader_argv.extend(["--prompt".to_owned(), prompt.clone()]);
    }
    let public_leader_argv = servecore_redact_prompt_argv(&leader_argv);
    let swarm_argv = if request.with_oracles.is_empty() {
        None
    } else {
        let mut argv = vec!["swarm".to_owned()];
        argv.extend(request.with_oracles.iter().cloned());
        if request.tiled {
            argv.push("--tiled".to_owned());
        }
        Some(argv)
    };
    Ok(ServecorePreparedOrchestration::Advanced(
        ServecoreAdvancedWorkon {
            request,
            repo_path,
            task,
            engine,
            leader_argv,
            public_leader_argv,
            swarm_argv,
        },
    ))
}

fn servecore_has_advanced_fields(request: &ServecoreWorkonRequest) -> bool {
    request.engine.is_some()
        || request.prompt.is_some()
        || request.target.is_some()
        || !request.with_oracles.is_empty()
        || request.attach
        || request.split
        || request.tiled
}

fn servecore_redact_prompt_argv(argv: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(argv.len());
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            redacted.push("<redacted>".to_owned());
            redact_next = false;
            continue;
        }
        redact_next = arg == "--prompt";
        redacted.push(arg.clone());
    }
    redacted
}

fn servecore_oracle_from_repo(repo: &str) -> Result<String, String> {
    let name = Path::new(repo)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "serve-orchestration target must be safe".to_owned())?;
    let oracle = name.strip_suffix("-oracle").unwrap_or(name).to_owned();
    servecore_validate_command_token(&oracle, "target")?;
    Ok(oracle)
}

fn servecore_repo_arg_is_safe(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(name) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && servecore_validate_command_token(owner, "repo").is_ok()
        && servecore_validate_command_token(name, "repo").is_ok()
}

fn servecore_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn servecore_shell_line_for_self(argv: &[String]) -> Result<String, String> {
    let mut parts = vec![servecore_shell_quote(
        &process_engine::serveengine_self_bin()?.to_string_lossy(),
    )];
    parts.extend(argv.iter().map(|arg| servecore_shell_quote(arg)));
    Ok(parts.join(" "))
}

fn servecore_find_pane_for_task(
    panes: &[ServecorePaneCandidate],
    task: &str,
) -> Result<String, String> {
    let needle = task.to_ascii_lowercase();
    let Some(pane) = panes
        .iter()
        .find(|pane| pane.title.to_ascii_lowercase().contains(&needle))
    else {
        return Err("serve-orchestration: pane discovery failed".to_owned());
    };
    servecore_validate_pane_id(&pane.id)?;
    Ok(pane.id.clone())
}

fn servecore_validate_pane_id(value: &str) -> Result<(), String> {
    if value
        .strip_prefix('%')
        .is_none_or(|rest| rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err("serve-orchestration pane must be safe".to_owned());
    }
    Ok(())
}

fn servecore_resolve_workon_repo(root: &Path, repo: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("serve-orchestration: root invalid: {error}"))?;
    let direct = PathBuf::from(repo);
    let first = if direct.is_absolute() {
        direct
    } else {
        root.join(repo)
    };
    if first.exists() {
        return servecore_worktree_inside_root(&root, &first);
    }
    let Some(found) = servecore_find_repo_under_root(&root, repo, 5) else {
        return Err("serve-orchestration: repo not found under root".to_owned());
    };
    servecore_worktree_inside_root(&root, &found)
}

fn servecore_find_repo_under_root(root: &Path, repo: &str, max_depth: usize) -> Option<PathBuf> {
    fn walk(root: &Path, repo: &Path, depth: usize, max_depth: usize) -> Option<PathBuf> {
        if depth > max_depth {
            return None;
        }
        let entries = fs::read_dir(root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.ends_with(repo) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = walk(&path, repo, depth + 1, max_depth) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(root, Path::new(repo), 0, max_depth)
}

fn servecore_worktree_inside_root(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("serve-orchestration: repo invalid: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("serve-orchestration: repo escapes root".to_owned());
    }
    Ok(canonical)
}

fn servecore_validate_path_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if Path::new(value)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

fn servecore_validate_engine_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\0')
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

fn servecore_validate_command_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

fn servecore_validate_prompt_text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(|ch| ch.is_control() || ch == '\0') {
        return Err("serve-orchestration prompt must be safe".to_owned());
    }
    Ok(())
}

#[derive(Clone)]
pub struct ServecoreThreadStore {
    root: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadRecord {
    pub thread: ServecoreThreadInfo,
    pub messages: Vec<ServecoreThreadMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadInfo {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadMessage {
    pub id: u64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ServecoreThreadPostResult {
    pub thread_id: u64,
    pub message_id: u64,
    pub status: String,
}

const SERVECORE_THREAD_MAX_PARTICIPANTS: usize = 32;
const SERVECORE_THREAD_MAX_TEXT_BYTES: usize = 64 * 1024;
const SERVECORE_THREAD_MAX_THREADS: usize = 10_000;
const SERVECORE_THREAD_FILE_BYTES: u64 = 8 * 1024 * 1024;

impl Default for ServecoreThreadStore {
    fn default() -> Self {
        Self::servecore_default()
    }
}

fn servecore_ed25519_tofu_path() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_DATA_DIR",
        "MAW_XDG",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    let env = maw_xdg::MawXdgEnv::with_vars(home, vars);
    maw_xdg::maw_data_path(&env, &["auth", "ed25519-tofu-pins.json"])
}

impl ServecoreThreadStore {
    #[must_use]
    pub fn servecore_default() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let vars = [
            "MAW_HOME",
            "MAW_DATA_DIR",
            "MAW_XDG",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
        let env = maw_xdg::MawXdgEnv::with_vars(home, vars);
        Self::servecore_with_root(maw_xdg::maw_data_path(&env, &["threads"]))
    }

    #[must_use]
    pub fn servecore_with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Create an empty maw-js-compatible thread and return its numeric id.
    ///
    /// # Errors
    /// Returns an error when participants are invalid or the thread file cannot be written.
    pub fn create_thread(&self, participants: &[String]) -> Result<u64, String> {
        let title = servecore_thread_title(participants)?;
        let record = self.servecore_create_record(&title)?;
        Ok(record.thread.id)
    }

    /// Append one maw-js-compatible message to an existing thread.
    ///
    /// # Errors
    /// Returns an error when the id, role, content, or backing file is invalid.
    pub fn append(
        &self,
        id: u64,
        role: &str,
        content: &str,
    ) -> Result<ServecoreThreadPostResult, String> {
        let _guard = self.servecore_lock();
        let mut record = self.servecore_read_record_locked(id)?;
        let message = servecore_thread_message(&record, role, content)?;
        record.messages.push(message.clone());
        self.servecore_write_record_locked(&record)?;
        Ok(servecore_thread_post_result(record.thread.id, message.id))
    }

    /// Read one maw-js-compatible thread record.
    ///
    /// # Errors
    /// Returns an error when the id is missing, invalid, too large, or invalid JSON.
    pub fn read(&self, id: u64) -> Result<ServecoreThreadRecord, String> {
        let _guard = self.servecore_lock();
        self.servecore_read_record_locked(id)
    }

    /// List thread metadata in descending id order.
    ///
    /// # Errors
    /// Returns an error when the backing thread directory cannot be read.
    pub fn list(&self) -> Result<Vec<ServecoreThreadInfo>, String> {
        let _guard = self.servecore_lock();
        self.servecore_list_locked()
    }

    /// Find-or-create an open channel thread and append one message.
    ///
    /// # Errors
    /// Returns an error when the title, role, content, or backing file is invalid.
    pub fn servecore_post_channel(
        &self,
        title: &str,
        role: &str,
        content: &str,
    ) -> Result<(ServecoreThreadPostResult, ServecoreThreadRecord), String> {
        let _guard = self.servecore_lock();
        let mut record = self
            .servecore_find_open_title_locked(title)?
            .unwrap_or_else(|| self.servecore_new_record_locked(title));
        let message = servecore_thread_message(&record, role, content)?;
        record.messages.push(message.clone());
        self.servecore_write_record_locked(&record)?;
        Ok((
            servecore_thread_post_result(record.thread.id, message.id),
            record,
        ))
    }

    fn servecore_lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn servecore_create_record(&self, title: &str) -> Result<ServecoreThreadRecord, String> {
        let _guard = self.servecore_lock();
        let record = self.servecore_new_record_locked(title);
        self.servecore_write_record_locked(&record)?;
        Ok(record)
    }

    fn servecore_new_record_locked(&self, title: &str) -> ServecoreThreadRecord {
        let id = self.servecore_next_id_locked().unwrap_or(1);
        let now = servecore_thread_now();
        ServecoreThreadRecord {
            thread: ServecoreThreadInfo {
                id,
                title: title.to_owned(),
                status: "open".to_owned(),
                created_at: now,
            },
            messages: Vec::new(),
        }
    }

    fn servecore_next_id_locked(&self) -> Result<u64, String> {
        let list = self.servecore_list_locked()?;
        Ok(list.into_iter().map(|thread| thread.id).max().unwrap_or(0) + 1)
    }

    fn servecore_find_open_title_locked(
        &self,
        title: &str,
    ) -> Result<Option<ServecoreThreadRecord>, String> {
        for thread in self.servecore_list_locked()? {
            if thread.title == title && thread.status != "closed" {
                return self.servecore_read_record_locked(thread.id).map(Some);
            }
        }
        Ok(None)
    }

    fn servecore_list_locked(&self) -> Result<Vec<ServecoreThreadInfo>, String> {
        let root = self.servecore_ensure_root()?;
        let mut items = Vec::new();
        let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
        for entry in entries.flatten() {
            if items.len() >= SERVECORE_THREAD_MAX_THREADS {
                break;
            }
            if let Some(record) = self.servecore_load_entry(&entry)? {
                items.push(record.thread);
            }
        }
        items.sort_by_key(|thread| Reverse(thread.id));
        Ok(items)
    }

    fn servecore_load_entry(
        &self,
        entry: &fs::DirEntry,
    ) -> Result<Option<ServecoreThreadRecord>, String> {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            return Ok(None);
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            return Ok(None);
        };
        let id = servecore_thread_id(stem)?;
        self.servecore_read_record_locked(id).map(Some)
    }

    fn servecore_read_record_locked(&self, id: u64) -> Result<ServecoreThreadRecord, String> {
        let path = self.servecore_path_for_id(id)?;
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > SERVECORE_THREAD_FILE_BYTES {
            return Err("thread file too large".to_owned());
        }
        let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str(&raw).map_err(|error| error.to_string())
    }

    fn servecore_write_record_locked(&self, record: &ServecoreThreadRecord) -> Result<(), String> {
        let path = self.servecore_path_for_id(record.thread.id)?;
        let mut data = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
        data.push(b'\n');
        if data.len() as u64 > SERVECORE_THREAD_FILE_BYTES {
            return Err("thread file too large".to_owned());
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, data).map_err(|error| error.to_string())?;
        fs::rename(&tmp, path).map_err(|error| error.to_string())
    }

    fn servecore_path_for_id(&self, id: u64) -> Result<PathBuf, String> {
        let safe_id = servecore_thread_id(&id.to_string())?;
        let root = self.servecore_ensure_root()?;
        let path = root.join(format!("{safe_id}.json"));
        servecore_thread_path_inside(&root, &path)?;
        Ok(path)
    }

    fn servecore_ensure_root(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(self.root.as_path()).map_err(|error| error.to_string())?;
        fs::canonicalize(self.root.as_path()).map_err(|error| error.to_string())
    }
}

fn servecore_thread_title(participants: &[String]) -> Result<String, String> {
    if participants.is_empty() || participants.len() > SERVECORE_THREAD_MAX_PARTICIPANTS {
        return Err("thread participants out of bounds".to_owned());
    }
    for participant in participants {
        servecore_thread_safe_text(participant, "participant")?;
    }
    Ok(participants.join(","))
}

fn servecore_thread_message(
    record: &ServecoreThreadRecord,
    role: &str,
    content: &str,
) -> Result<ServecoreThreadMessage, String> {
    servecore_thread_safe_text(role, "role")?;
    servecore_thread_safe_content(content)?;
    Ok(ServecoreThreadMessage {
        id: record.messages.last().map_or(1, |message| message.id + 1),
        role: role.to_owned(),
        content: content.to_owned(),
        created_at: servecore_thread_now(),
    })
}

fn servecore_thread_safe_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("thread {label} must be safe"));
    }
    if value.contains("..") || value.contains('/') || value.chars().any(char::is_control) {
        return Err(format!("thread {label} must be safe"));
    }
    Ok(())
}

fn servecore_thread_safe_content(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > SERVECORE_THREAD_MAX_TEXT_BYTES {
        return Err("thread content out of bounds".to_owned());
    }
    if value.bytes().any(|byte| byte == 0) {
        return Err("thread content contains nul".to_owned());
    }
    Ok(())
}

fn servecore_thread_id(value: &str) -> Result<u64, String> {
    if value.is_empty() || value == "--" || value.starts_with('-') {
        return Err("thread id must be numeric".to_owned());
    }
    if value.contains("..") || value.chars().any(char::is_control) {
        return Err("thread id must be numeric".to_owned());
    }
    if value.bytes().any(|byte| matches!(byte, b'/' | b'\\')) {
        return Err("thread id must be numeric".to_owned());
    }
    value
        .parse::<u64>()
        .map_err(|_| "thread id must be numeric".to_owned())
}

fn servecore_thread_path_inside(root: &Path, path: &Path) -> Result<(), String> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("thread path escapes root".to_owned());
    }
    if !path.starts_with(root) {
        return Err("thread path escapes root".to_owned());
    }
    Ok(())
}

fn servecore_thread_post_result(thread_id: u64, message_id: u64) -> ServecoreThreadPostResult {
    ServecoreThreadPostResult {
        thread_id,
        message_id,
        status: "ok".to_owned(),
    }
}

fn servecore_thread_now() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("epoch-ms:{ms}")
}

#[derive(Clone, Debug, Default)]
pub struct ServecoreLifecycle {
    modules: Arc<Vec<ServecoreLifecycleModule>>,
    api_routers: Arc<BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecoreLifecycleModule {
    pub name: String,
    pub weight: i32,
}

impl ServecoreLifecycle {
    #[must_use]
    pub fn servecore_from_profile(
        modules: Vec<ServecoreLifecycleModule>,
        api_routers: &[String],
    ) -> Self {
        let mut sorted = modules;
        sorted.sort_by(|left, right| {
            left.weight
                .cmp(&right.weight)
                .then(left.name.cmp(&right.name))
        });
        Self {
            modules: Arc::new(sorted),
            api_routers: Arc::new(api_routers.iter().cloned().collect()),
        }
    }

    #[must_use]
    pub fn servecore_enabled_modules(&self) -> Vec<String> {
        self.modules
            .iter()
            .filter(|module| self.api_routers.is_empty() || self.api_routers.contains(&module.name))
            .map(|module| module.name.clone())
            .collect()
    }
}

#[derive(Default)]
pub struct ServecoreRouteRegistry {
    seen: BTreeSet<ServecoreRouteKey>,
    routes: Vec<ServecoreRouteKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServecoreRouteKey {
    method: Method,
    path: String,
}

impl ServecoreRouteRegistry {
    /// Register one HTTP route.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or the method/path pair is already registered.
    pub fn servecore_register(&mut self, method: Method, path: &str) -> Result<(), String> {
        servecore_validate_path(path)?;
        let key = ServecoreRouteKey {
            method,
            path: path.to_owned(),
        };
        if !self.seen.insert(key.clone()) {
            return Err(format!(
                "serve-core: duplicate route {} {}",
                key.method, key.path
            ));
        }
        self.routes.push(key);
        Ok(())
    }

    #[must_use]
    pub fn servecore_routes(&self) -> &[ServecoreRouteKey] {
        &self.routes
    }
}

#[derive(Default)]
pub struct ServecoreWsRegistry {
    handlers: BTreeMap<String, ServecoreWsKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServecoreWsKind {
    Engine,
    Pty,
    Tmux,
}

impl ServecoreWsRegistry {
    /// Register one websocket upgrade path.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or already registered.
    pub fn servecore_register_ws(&mut self, path: &str) -> Result<(), String> {
        self.servecore_register_ws_kind(path, ServecoreWsKind::Engine)
    }

    /// Register one websocket upgrade path with its stream kind.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or already registered.
    pub fn servecore_register_ws_kind(
        &mut self,
        path: &str,
        kind: ServecoreWsKind,
    ) -> Result<(), String> {
        servecore_validate_path(path)?;
        if self.handlers.insert(path.to_owned(), kind).is_some() {
            return Err(format!("serve-core: duplicate ws route {path}"));
        }
        Ok(())
    }

    #[must_use]
    pub fn servecore_paths(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    #[must_use]
    pub fn servecore_handlers(&self) -> Vec<(String, ServecoreWsKind)> {
        self.handlers
            .iter()
            .map(|(path, kind)| (path.clone(), *kind))
            .collect()
    }
}

pub fn servecore_with_shared_state<S>(router: Router<S>, state: ServecoreSharedState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(Extension(Arc::new(state)))
}

pub fn servecore_mount_core_routes<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/serve-core/pipeline", get(servecore_pipeline_handler))
        .route(
            "/api/orchestration/workon",
            post(servecore_orchestration_workon),
        )
        .route("/api/triggers/fire", post(servecore_protected_stub))
        .route("/api/plugins/*plugin_path", post(servecore_protected_stub))
}

pub fn servecore_mount_ws_routes<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    modules::websocket_routes::ws_mount(router)
}

pub fn servecore_mount_ws_routes_with_config<S>(
    router: Router<S>,
    config: modules::websocket_routes::WsConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    modules::websocket_routes::ws_mount_with_config(router, config)
}

pub fn servecore_mount_ws_registry_with_config<S>(
    router: Router<S>,
    registry: &ServecoreWsRegistry,
    config: modules::websocket_routes::WsConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let ws_routes = servecore_mount_ws_registry(Router::new(), registry).layer(Extension(config));
    router.merge(ws_routes)
}

pub fn servecore_mount_ws_registry<S>(
    router: Router<S>,
    registry: &ServecoreWsRegistry,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    registry
        .servecore_handlers()
        .into_iter()
        .fold(router, |router, (path, kind)| {
            router.route(&path, get(servecore_ws_upgrade).layer(Extension(kind)))
        })
}

pub fn servecore_mount_registry_stub<S>(
    router: Router<S>,
    registry: &ServecoreRouteRegistry,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    registry.routes.iter().fold(router, |router, route| {
        router.route(&route.path, any(servecore_registry_stub))
    })
}

pub fn servecore_apply_pipeline<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    servecore_apply_pipeline_with_views_config(
        router,
        modules::static_views::ViewsConfig::views_from_process_env(),
    )
}

pub fn servecore_apply_pipeline_with_views_config<S>(
    router: Router<S>,
    views_config: modules::static_views::ViewsConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    modules::static_views::views_apply_fallback_with_config(router, views_config)
        .layer(middleware::from_fn(servecore_auth_default_deny))
        .layer(middleware::from_fn(servecore_engine_proxy))
        .layer(middleware::from_fn(servecore_ws_upgrade_gate))
        .layer(middleware::from_fn(servecore_cors_preflight))
}

#[must_use]
pub fn servecore_pipeline_order() -> &'static [&'static str] {
    SERVECORE_PIPELINE_ORDER
}

fn servecore_validate_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.contains("//") || path.chars().any(char::is_control) {
        return Err("serve-core: route path must be absolute and control-free".to_owned());
    }
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        if segment == "--" || segment.starts_with('-') {
            return Err("serve-core: route segment must not start with '-'".to_owned());
        }
    }
    Ok(())
}

async fn servecore_cors_preflight(req: Request<Body>, next: Next) -> Response {
    let origin = req.headers().get("origin").cloned();
    let allow_headers = req
        .headers()
        .get("access-control-request-headers")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("Content-Type, Authorization"));

    if req.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        servecore_add_cors_headers(response.headers_mut(), origin.as_ref(), &allow_headers);
        return response;
    }

    let mut response = next.run(req).await;
    servecore_add_cors_headers(response.headers_mut(), origin.as_ref(), &allow_headers);
    response
}

fn servecore_add_cors_headers(
    headers: &mut HeaderMap,
    origin: Option<&HeaderValue>,
    allow_headers: &HeaderValue,
) {
    let Some(origin) = origin else {
        return;
    };
    headers.insert("access-control-allow-origin", origin.clone());
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert("access-control-allow-headers", allow_headers.clone());
    servecore_add_vary_origin(headers);
}

fn servecore_add_vary_origin(headers: &mut HeaderMap) {
    let Some(existing) = headers.get("vary").and_then(|value| value.to_str().ok()) else {
        headers.insert("vary", HeaderValue::from_static("Origin"));
        return;
    };
    if existing
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case("origin"))
    {
        return;
    }
    let value = format!("{existing}, Origin");
    let value =
        HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("Origin"));
    headers.insert("vary", value);
}

async fn servecore_ws_upgrade_gate(req: Request<Body>, next: Next) -> Response {
    next.run(req).await
}

async fn servecore_engine_proxy(req: Request<Body>, next: Next) -> Response {
    next.run(req).await
}

async fn servecore_auth_default_deny(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = servecore_api_auth_path(req.uri().path());
    if !maw_auth::is_protected(&path, method.as_str()) {
        return next.run(req).await;
    }

    let peer_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);
    let state = req.extensions().get::<Arc<ServecoreSharedState>>().cloned();
    let (parts, body) = req.into_parts();
    let Ok(body_bytes) = to_bytes(body, 64 * 1024).await else {
        return servecore_forbidden("bad-body");
    };
    let headers = servecore_auth_headers(&parts.headers);
    let uri_path = servecore_api_auth_path(parts.uri.path());
    let request_parts = maw_auth::RequestAuthParts {
        method: method.as_str().to_owned(),
        path: uri_path,
        headers,
        body: Some(body_bytes.to_vec()),
        peer_ip: peer_addr.map(|addr| addr.ip()),
        workspace_key: state
            .as_ref()
            .and_then(|state| state.auth_workspace_key.clone()),
        cached_pubkey: state
            .as_ref()
            .and_then(|state| state.auth_cached_pubkey.clone()),
        ed25519_pins: state.as_ref().map(|state| state.auth_ed25519_pins.clone()),
        now: state
            .as_ref()
            .and_then(|state| state.auth_now_override)
            .unwrap_or_else(servecore_auth_now),
    };
    match maw_auth::verify_request(&request_parts) {
        maw_auth::RequestAuthDecision::Accept { .. } => {
            next.run(Request::from_parts(parts, Body::from(body_bytes)))
                .await
        }
        maw_auth::RequestAuthDecision::Reject { reason } => servecore_forbidden(&reason),
    }
}

fn servecore_auth_headers(headers: &axum::http::HeaderMap) -> maw_auth::Headers {
    maw_auth::Headers::new([
        (
            "x-maw-from",
            servecore_header_to_string(headers, "x-maw-from"),
        ),
        (
            "x-maw-signature",
            servecore_header_to_string(headers, "x-maw-signature"),
        ),
        (
            "x-maw-signature-v3",
            servecore_header_to_string(headers, "x-maw-signature-v3"),
        ),
        (
            "x-maw-signed-at",
            servecore_header_to_string(headers, "x-maw-signed-at"),
        ),
        (
            "x-maw-timestamp",
            servecore_header_to_string(headers, "x-maw-timestamp"),
        ),
        (
            "x-maw-auth-version",
            servecore_header_to_string(headers, "x-maw-auth-version"),
        ),
        (
            "x-maw-ed25519-signature",
            servecore_header_to_string(headers, "x-maw-ed25519-signature"),
        ),
        (
            "x-maw-signature-ed25519",
            servecore_header_to_string(headers, "x-maw-signature-ed25519"),
        ),
        (
            "x-maw-from-signature-ed25519",
            servecore_header_to_string(headers, "x-maw-from-signature-ed25519"),
        ),
        (
            "x-maw-ed25519-pubkey",
            servecore_header_to_string(headers, "x-maw-ed25519-pubkey"),
        ),
        (
            "x-maw-pubkey",
            servecore_header_to_string(headers, "x-maw-pubkey"),
        ),
        (
            "x-maw-peer-pubkey",
            servecore_header_to_string(headers, "x-maw-peer-pubkey"),
        ),
    ])
}

fn servecore_header_to_string(headers: &axum::http::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn servecore_auth_now() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
}

fn servecore_forbidden(reason: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error":"forbidden","reason": reason})),
    )
        .into_response()
}

fn servecore_api_auth_path(path: &str) -> String {
    path.strip_prefix("/api").unwrap_or(path).to_owned()
}

async fn servecore_pipeline_handler() -> impl IntoResponse {
    Json(json!({"pipeline": servecore_pipeline_order()}))
}

async fn servecore_orchestration_workon(req: Request<Body>) -> Response {
    let Some(state) = req.extensions().get::<Arc<ServecoreSharedState>>().cloned() else {
        return servecore_bad_request("missing-state");
    };
    let Ok(body) = to_bytes(req.into_body(), SERVECORE_ORCHESTRATION_BODY_LIMIT).await else {
        return servecore_bad_request("body-too-large");
    };
    let Ok(payload) = serde_json::from_slice::<ServecoreWorkonRequest>(&body) else {
        return servecore_bad_request("body must be valid json");
    };
    match state
        .orchestrator
        .spawn_workon(payload, state.engine.clone())
    {
        Ok(handle) => Json(handle).into_response(),
        Err(error) => servecore_bad_request(&error),
    }
}

fn servecore_bad_request(reason: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": reason}))).into_response()
}

async fn servecore_protected_stub() -> impl IntoResponse {
    Json(json!({"ok": true, "state": "protected-stub"}))
}

async fn servecore_registry_stub() -> impl IntoResponse {
    Json(json!({"ok": true, "source": "serve-core-registry"}))
}

async fn servecore_ws_upgrade(
    ws: WebSocketUpgrade,
    uri: Uri,
    Extension(kind): Extension<ServecoreWsKind>,
    Extension(state): Extension<Arc<ServecoreSharedState>>,
    Extension(config): Extension<modules::websocket_routes::WsConfig>,
) -> impl IntoResponse {
    let target =
        match modules::websocket_routes::ws_validate_target(servecore_ws_target(uri.query())) {
            Ok(target) => target,
            Err(error) => {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response()
            }
        };
    if state
        .engine
        .servecore_ws_open(kind, target.as_deref())
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"ws_engine_unavailable"})),
        )
            .into_response();
    }
    if servecore_ws_connection_limit_reached(config.max_connections) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"ws_connection_limit"})),
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| servecore_ws_stream(socket, state, kind, target, config))
        .into_response()
}

async fn servecore_ws_stream(
    mut socket: WebSocket,
    state: Arc<ServecoreSharedState>,
    kind: ServecoreWsKind,
    target: Option<String>,
    config: modules::websocket_routes::WsConfig,
) {
    let Some(_guard) = servecore_ws_connection_guard(config.max_connections) else {
        let _ = socket
            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 1013,
                reason: "ws connection limit".into(),
            })))
            .await;
        return;
    };
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    let idle_timer = tokio::time::sleep(config.idle_timeout);
    tokio::pin!(idle_timer);
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if servecore_ws_send(&mut socket, Message::Ping(Vec::new()), config.send_timeout).await.is_err() {
                    break;
                }
            }
            () = &mut idle_timer => {
                let _ = servecore_ws_send(&mut socket, Message::Close(None), config.send_timeout).await;
                break;
            }
            frame = socket.recv() => {
                match frame {
                    Some(Ok(frame)) => {
                        let resets_idle = !matches!(frame, Message::Pong(_));
                        if resets_idle {
                            idle_timer.as_mut().reset(tokio::time::Instant::now() + config.idle_timeout);
                        }
                        if !servecore_ws_handle_frame(&mut socket, &state, kind, target.as_deref(), &config, frame).await {
                            break;
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
        }
    }
    state.engine.servecore_ws_close(kind, target.as_deref());
}

pub(crate) async fn servecore_ws_send_text_frames(
    socket: &mut WebSocket,
    frames: Vec<String>,
    config: &modules::websocket_routes::WsConfig,
) -> bool {
    for frame in frames {
        if servecore_ws_send(socket, Message::Text(frame), config.send_timeout)
            .await
            .is_err()
        {
            return false;
        }
    }
    true
}

pub(crate) async fn servecore_ws_handle_frame(
    socket: &mut WebSocket,
    state: &ServecoreSharedState,
    kind: ServecoreWsKind,
    target: Option<&str>,
    config: &modules::websocket_routes::WsConfig,
    frame: Message,
) -> bool {
    match frame {
        Message::Text(text) => {
            if text.len() > config.max_frame_bytes {
                return servecore_ws_send(socket, Message::Close(None), config.send_timeout)
                    .await
                    .is_ok();
            }
            if let Some(reply) = state.engine.servecore_ws_text(kind, &text, target) {
                return servecore_ws_send(socket, Message::Text(reply), config.send_timeout)
                    .await
                    .is_ok();
            }
            true
        }
        Message::Binary(bytes) => {
            if bytes.len() > config.max_frame_bytes {
                return servecore_ws_send(socket, Message::Close(None), config.send_timeout)
                    .await
                    .is_ok();
            }
            if let Some(reply) = state.engine.servecore_ws_binary(kind, &bytes, target) {
                return servecore_ws_send(socket, Message::Binary(reply), config.send_timeout)
                    .await
                    .is_ok();
            }
            true
        }
        Message::Ping(bytes) => {
            servecore_ws_send(socket, Message::Pong(bytes), config.send_timeout)
                .await
                .is_ok()
        }
        Message::Pong(_) => true,
        Message::Close(frame) => {
            let _ = servecore_ws_send(socket, Message::Close(frame), config.send_timeout).await;
            false
        }
    }
}

pub(crate) async fn servecore_ws_send(
    socket: &mut WebSocket,
    message: Message,
    timeout: Duration,
) -> Result<(), ()> {
    tokio::time::timeout(timeout, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

pub(crate) fn servecore_ws_target(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| (key == "target" || key == "session").then_some(value))
}

pub(crate) fn servecore_ws_connection_limit_reached(max_connections: usize) -> bool {
    SERVECORE_WS_CONNECTIONS.load(Ordering::Relaxed) >= max_connections
}

pub(crate) fn servecore_ws_connection_guard(
    max_connections: usize,
) -> Option<ServecoreWsConnectionGuard> {
    let mut current = SERVECORE_WS_CONNECTIONS.load(Ordering::Relaxed);
    loop {
        if current >= max_connections {
            return None;
        }
        match SERVECORE_WS_CONNECTIONS.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(ServecoreWsConnectionGuard),
            Err(actual) => current = actual,
        }
    }
}

pub(crate) struct ServecoreWsConnectionGuard;

impl Drop for ServecoreWsConnectionGuard {
    fn drop(&mut self) {
        SERVECORE_WS_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod thread_store_tests {
    use super::*;

    fn threadstore_temp(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!(
            "maw-rs-core-threadstore-{name}-{}-{nanos}",
            std::process::id()
        ));
        root
    }

    #[test]
    fn servecore_thread_store_create_append_read_list() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("crud"));
        let id = store
            .create_thread(&["channel:alpha".to_owned()])
            .expect("create");
        let first = store.append(id, "claude", "hello").expect("append");
        let second = store.append(id, "claude", "again").expect("append2");
        assert_eq!(first.thread_id, id);
        assert_eq!(first.message_id, 1);
        assert_eq!(second.message_id, 2);
        let record = store.read(id).expect("read");
        assert_eq!(record.thread.title, "channel:alpha");
        assert_eq!(record.messages.len(), 2);
        let list = store.list().expect("list");
        assert_eq!(list[0].id, id);
    }

    #[test]
    fn servecore_thread_store_rejects_traversal_and_injection() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("guard"));
        assert!(store.create_thread(&["../bad".to_owned()]).is_err());
        assert!(store.create_thread(&["-bad".to_owned()]).is_err());
        assert!(servecore_thread_id("../../1").is_err());
        assert!(servecore_thread_id("-1").is_err());
        assert!(servecore_thread_id("1\n").is_err());
    }

    #[test]
    fn servecore_thread_store_concurrent_append_no_corrupt() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("concurrent"));
        let id = store
            .create_thread(&["channel:alpha".to_owned()])
            .expect("create");
        let handles = (0..8)
            .map(|index| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let text = format!("message-{index}");
                    store.append(id, "claude", &text).expect("append");
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().expect("join");
        }
        let record = store.read(id).expect("read");
        assert_eq!(record.messages.len(), 8);
        let ids = record
            .messages
            .iter()
            .map(|message| message.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    #[derive(Default)]
    struct FakeOrchestrator {
        calls: Mutex<Vec<ServecoreWorkonRequest>>,
    }

    impl ServecoreOrchestrator for FakeOrchestrator {
        fn spawn_workon(
            &self,
            request: ServecoreWorkonRequest,
            engine: Arc<dyn ServecoreEngine>,
        ) -> Result<ServecoreWorkonHandle, String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            Ok(ServecoreWorkonHandle {
                ok: true,
                repo: request.repo,
                cwd: "/tmp/fake-worktree".to_owned(),
                engine: request
                    .engine
                    .unwrap_or_else(|| engine.servecore_engine_name().to_owned()),
                target: request.target,
                argv: vec!["workon".to_owned(), "demo".to_owned()],
                status: "fake-spawned".to_owned(),
                message: None,
                leader_argv: None,
                swarm_argv: None,
                pane: None,
                swarm_skipped: None,
            })
        }
    }

    #[derive(Default)]
    struct FakeExecRunner {
        calls: Mutex<Vec<(Vec<String>, PathBuf)>>,
    }

    impl ServecoreExecRunner for FakeExecRunner {
        fn servecore_run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((argv.to_vec(), cwd.to_path_buf()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakePaneRunner {
        panes: Mutex<Vec<ServecorePaneCandidate>>,
        sends: Mutex<Vec<(String, String)>>,
        fail_send: Mutex<Option<String>>,
    }

    impl FakePaneRunner {
        fn with_panes(panes: Vec<ServecorePaneCandidate>) -> Self {
            Self {
                panes: Mutex::new(panes),
                sends: Mutex::new(Vec::new()),
                fail_send: Mutex::new(None),
            }
        }

        fn with_send_failure(panes: Vec<ServecorePaneCandidate>) -> Self {
            Self {
                panes: Mutex::new(panes),
                sends: Mutex::new(Vec::new()),
                fail_send: Mutex::new(Some("send failed".to_owned())),
            }
        }
    }

    impl ServecorePaneRunner for FakePaneRunner {
        fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
            Ok(self
                .panes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone())
        }

        fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String> {
            if let Some(error) = self
                .fail_send
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                return Err(error);
            }
            self.sends
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((pane.to_owned(), line.to_owned()));
            Ok(())
        }
    }

    fn servecore_test_root(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!(
            "maw-rs-core-orchestrator-{name}-{}-{nanos}",
            std::process::id()
        ));
        root
    }

    fn servecore_expected_public_leader() -> Vec<String> {
        [
            "wake",
            "nova",
            "--task",
            "feat-295",
            "--engine",
            "claude47",
            "--split",
            "--no-attach",
            "--repo",
            "acme/demo",
            "--prompt",
            "<redacted>",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    }

    fn servecore_expected_private_leader() -> Vec<String> {
        [
            "wake",
            "nova",
            "--task",
            "feat-295",
            "--engine",
            "claude47",
            "--split",
            "--no-attach",
            "--repo",
            "acme/demo",
            "--prompt",
            "SECRET prompt $(touch pwn)",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    }

    #[test]
    fn servecore_orchestrator_validates_engine_and_repo_bounds() {
        let root = servecore_test_root("bounds");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let valid = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("feat-219".to_owned()),
            engine: Some("codex-anything".to_owned()),
            target: Some("nova:1".to_owned()),
            prompt: Some("ship it".to_owned()),
            with_oracles: vec!["wish".to_owned()],
            attach: false,
            split: true,
            tiled: false,
        };
        let plan = servecore_prepare_workon(&root, valid, "stub").expect("plan");
        let ServecorePreparedOrchestration::Advanced(plan) = plan else {
            panic!("advanced plan");
        };
        assert_eq!(plan.engine, "codex-anything");
        assert_eq!(
            plan.leader_argv,
            vec![
                "wake",
                "nova:1",
                "--task",
                "feat-219",
                "--engine",
                "codex-anything",
                "--split",
                "--no-attach",
                "--repo",
                "acme/demo",
                "--prompt",
                "ship it",
            ]
        );
        assert_eq!(plan.repo_path, repo.canonicalize().expect("canon"));

        let bad_engine = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            engine: Some("-shell".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_engine, "stub").is_err());

        let escaped = ServecoreWorkonRequest {
            repo: "../demo".to_owned(),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, escaped, "stub").is_err());
    }

    #[test]
    fn servecore_simple_workon_executes_self_runner_and_matches_golden() {
        let root = servecore_test_root("simple-exec");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runner(root.clone(), runner.clone());
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("spawn");
        assert_eq!(handle.engine, "maw-rs");
        assert_eq!(handle.status, "spawned");
        assert_eq!(handle.message, None);
        assert_eq!(handle.leader_argv, None);
        assert_eq!(handle.swarm_argv, None);
        assert_eq!(handle.swarm_skipped, None);
        let calls = runner
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            vec!["workon", "acme/demo", "feat-295", "--layout", "nested"]
        );
        assert_eq!(calls[0].1, repo.canonicalize().expect("canon"));
        let golden = serde_json::json!({"argv": handle.argv, "engine": handle.engine, "status": handle.status}).to_string();
        assert_eq!(
            format!("{golden}\n"),
            include_str!("../../tests/fixtures/native-serve-engine/simple-workon.stdout")
        );
    }

    #[test]
    fn servecore_advanced_wake_swarm_executes_and_matches_golden() {
        let root = servecore_test_root("advanced-live");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_panes(vec![ServecorePaneCandidate {
            id: "%42".to_owned(),
            title: "nova feat-295 leader".to_owned(),
        }]));
        let orchestrator = ServecoreCommandOrchestrator::servecore_with_runners(
            root,
            runner.clone(),
            pane_runner.clone(),
        );
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("nova".to_owned()),
                    prompt: Some("SECRET prompt $(touch pwn)".to_owned()),
                    with_oracles: vec!["wish".to_owned(), "codex".to_owned()],
                    split: true,
                    tiled: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("advanced");
        assert_eq!(handle.engine, "claude47");
        assert_eq!(handle.status, "spawned");
        assert_eq!(handle.pane.as_deref(), Some("%42"));
        let expected_public_leader = servecore_expected_public_leader();
        assert_eq!(handle.leader_argv, Some(expected_public_leader.clone()));
        assert_eq!(handle.argv, expected_public_leader);
        assert_eq!(
            handle.swarm_argv,
            Some(vec![
                "swarm".to_owned(),
                "wish".to_owned(),
                "codex".to_owned(),
                "--tiled".to_owned()
            ])
        );
        let handle_json = serde_json::to_string(&handle).expect("handle json");
        assert!(!handle_json.contains("SECRET"));
        assert!(!handle_json.contains("touch pwn"));
        assert!(!handle_json.contains("workon"));
        let calls = runner
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, servecore_expected_private_leader());
        let sends = pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expected_line = format!(
            "{} 'swarm' 'wish' 'codex' '--tiled'",
            servecore_shell_quote(
                &process_engine::serveengine_self_bin()
                    .expect("self")
                    .to_string_lossy()
            )
        );
        assert_eq!(sends.as_slice(), [("%42".to_owned(), expected_line)]);
        let golden = serde_json::json!({
            "argv": handle.argv,
            "engine": handle.engine,
            "leader_argv": handle.leader_argv,
            "pane": handle.pane,
            "status": handle.status,
            "swarm_argv": handle.swarm_argv,
        })
        .to_string();
        assert_eq!(
            format!("{golden}\n"),
            include_str!("../../tests/fixtures/native-serve-engine/advanced-wake-swarm.stdout")
        );
    }

    #[test]
    fn servecore_advanced_shell_quote_and_metachar_guards_block_injection() {
        assert_eq!(servecore_shell_quote("builder'one"), "'builder'\\''one'");
        assert_eq!(servecore_shell_quote("$(touch pwn)"), "'$(touch pwn)'");
        assert_eq!(servecore_shell_quote("`touch pwn`;"), "'`touch pwn`;'");

        let root = servecore_test_root("advanced-quote");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_panes(vec![ServecorePaneCandidate {
            id: "%7".to_owned(),
            title: "feat-295".to_owned(),
        }]));
        let orchestrator = ServecoreCommandOrchestrator::servecore_with_runners(
            root.clone(),
            runner.clone(),
            pane_runner.clone(),
        );
        orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("nova".to_owned()),
                    prompt: Some("data $(touch pwn) `whoami`;".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("spawn");
        let sends = pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expected_line = format!(
            "{} 'swarm' 'wish'",
            servecore_shell_quote(
                &process_engine::serveengine_self_bin()
                    .expect("self")
                    .to_string_lossy()
            )
        );
        assert_eq!(
            sends[0].1, expected_line,
            "send-keys receives one quoted literal line, not shell-expanded fragments"
        );

        for (label, mut request) in [
            (
                "target",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("bad;name".to_owned()),
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
            (
                "with",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["$(touch-pwn)".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
            (
                "with-quote",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["bad'name".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
        ] {
            request.engine = Some("claude47".to_owned());
            assert!(
                servecore_prepare_workon(&root, request, "maw-rs").is_err(),
                "{label} metachar must reject before runner"
            );
        }
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1,
            "bad metachar requests never reach child runner"
        );
    }

    #[test]
    fn servecore_advanced_pane_discovery_fail_is_soft_loud() {
        let root = servecore_test_root("advanced-no-pane");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::default());
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runners(root, runner, pane_runner.clone());
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("soft");
        assert_eq!(handle.status, "leader-spawned");
        assert_eq!(
            handle.swarm_skipped.as_deref(),
            Some("pane discovery failed")
        );
        assert!(pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty());
    }

    #[test]
    fn servecore_advanced_pane_send_fail_is_soft_loud() {
        let root = servecore_test_root("advanced-send-fail");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_send_failure(vec![
            ServecorePaneCandidate {
                id: "%9".to_owned(),
                title: "feat-295".to_owned(),
            },
        ]));
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runners(root, runner, pane_runner);
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("soft");
        assert_eq!(handle.status, "leader-spawned");
        assert_eq!(handle.swarm_skipped.as_deref(), Some("pane send failed"));
    }

    #[test]
    fn servecore_advanced_refuses_attach_and_requires_task() {
        let root = servecore_test_root("advanced-guards");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let attach = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("feat-295".to_owned()),
            attach: true,
            split: true,
            ..ServecoreWorkonRequest::default()
        };
        let Err(attach_err) = servecore_prepare_workon(&root, attach, "maw-rs") else {
            panic!("attach must fail");
        };
        assert!(attach_err.contains("attach is not supported"));

        let no_task = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            split: true,
            ..ServecoreWorkonRequest::default()
        };
        let Err(task_err) = servecore_prepare_workon(&root, no_task, "maw-rs") else {
            panic!("task must fail");
        };
        assert!(task_err.contains("advanced wake requires task"));
    }

    #[test]
    fn servecore_rejects_task_engine_and_repo_guards() {
        let root = servecore_test_root("guards");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let bad_task = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("-bad".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_task, "maw-rs").is_err());
        let bad_engine = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            engine: Some("bad\nengine".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_engine, "maw-rs").is_err());
        let bad_repo = ServecoreWorkonRequest {
            repo: "../demo".to_owned(),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_repo, "maw-rs").is_err());
    }

    async fn servecore_spawn_test_server() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = servecore_apply_pipeline(servecore_mount_core_routes(Router::new()));
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    async fn servecore_spawn_ws_test_server(
        state: ServecoreSharedState,
        config: modules::websocket_routes::WsConfig,
    ) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_mount_core_routes(Router::new());
        let router = modules::websocket_routes::ws_mount_with_config(router, config);
        let router = servecore_with_shared_state(router, state);
        let app = servecore_apply_pipeline_with_views_config(
            router,
            modules::static_views::ViewsConfig::views_with_paths(
                std::env::temp_dir().join("maw-rs-ws-no-ui"),
                std::env::temp_dir().join("maw-rs-ws-no-door.html"),
                std::env::temp_dir().join("maw-rs-ws-no-topology.html"),
            ),
        );
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    #[derive(Debug, Default)]
    struct TestEngine {
        opened: Mutex<Vec<(ServecoreWsKind, Option<String>)>>,
    }

    impl ServecoreEngine for TestEngine {
        fn servecore_engine_name(&self) -> &'static str {
            "test"
        }

        fn servecore_ws_open(
            &self,
            kind: ServecoreWsKind,
            target: Option<&str>,
        ) -> Result<(), String> {
            let mut guard = self
                .opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.push((kind, target.map(ToOwned::to_owned)));
            Ok(())
        }

        fn servecore_ws_text(
            &self,
            kind: ServecoreWsKind,
            text: &str,
            target: Option<&str>,
        ) -> Option<String> {
            Some(format!("{kind:?}:{}:{text}", target.unwrap_or("none")))
        }
    }

    #[test]
    fn servecore_route_registry_rejects_duplicates_and_accepts_params() {
        let mut registry = ServecoreRouteRegistry::default();
        registry
            .servecore_register(Method::GET, "/api/agent/:id")
            .expect("first");
        let duplicate = registry.servecore_register(Method::GET, "/api/agent/:id");
        assert!(duplicate
            .expect_err("duplicate")
            .contains("duplicate route"));
        registry
            .servecore_register(Method::POST, "/api/agent/:id")
            .expect("method distinct");
        assert_eq!(registry.servecore_routes().len(), 2);
    }

    #[test]
    fn servecore_ws_registry_rejects_duplicates() {
        let mut registry = ServecoreWsRegistry::default();
        registry.servecore_register_ws("/ws").expect("ws");
        registry
            .servecore_register_ws_kind("/ws/pty", ServecoreWsKind::Pty)
            .expect("pty");
        registry
            .servecore_register_ws_kind("/ws/tmux", ServecoreWsKind::Tmux)
            .expect("tmux");
        assert!(registry
            .servecore_register_ws("/ws")
            .expect_err("dup")
            .contains("duplicate ws"));
        assert_eq!(
            registry.servecore_paths(),
            vec!["/ws", "/ws/pty", "/ws/tmux"]
        );
        assert_eq!(
            registry.servecore_handlers(),
            vec![
                ("/ws".to_owned(), ServecoreWsKind::Engine),
                ("/ws/pty".to_owned(), ServecoreWsKind::Pty),
                ("/ws/tmux".to_owned(), ServecoreWsKind::Tmux),
            ]
        );
    }

    #[test]
    fn servecore_lifecycle_sorts_by_weight_then_name_and_whitelists() {
        let modules = vec![
            ServecoreLifecycleModule {
                name: "triggers".to_owned(),
                weight: 20,
            },
            ServecoreLifecycleModule {
                name: "agents".to_owned(),
                weight: 10,
            },
            ServecoreLifecycleModule {
                name: "debug".to_owned(),
                weight: 10,
            },
        ];
        let enabled = ServecoreLifecycle::servecore_from_profile(
            modules,
            &["debug".to_owned(), "triggers".to_owned()],
        );
        assert_eq!(
            enabled.servecore_enabled_modules(),
            vec!["debug", "triggers"]
        );
    }

    #[test]
    fn servecore_pipeline_order_matches_maw_js_surface() {
        assert_eq!(
            servecore_pipeline_order(),
            [
                "cors-preflight",
                "ws-upgrade",
                "engine-proxy",
                "api-protected-auth",
                "registry",
                "api-public",
                "registry",
                "fallback-views",
            ]
        );
    }

    #[tokio::test]
    async fn servecore_cors_reflects_origin_on_preflight_and_404() {
        let app = servecore_apply_pipeline(servecore_mount_core_routes(Router::new()));
        let preflight = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/costs")
                    .header("origin", "https://god.buildwithoracle.com")
                    .header("access-control-request-headers", "x-maw-from,content-type")
                    .body(Body::empty())
                    .expect("preflight"),
            )
            .await
            .expect("preflight response");
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-methods")
                .and_then(|value| value.to_str().ok()),
            Some("GET, POST, OPTIONS")
        );
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-headers")
                .and_then(|value| value.to_str().ok()),
            Some("x-maw-from,content-type")
        );
        assert_eq!(
            preflight
                .headers()
                .get("vary")
                .and_then(|value| value.to_str().ok()),
            Some("Origin")
        );

        let missing = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/missing-god-ui-route")
                    .header("origin", "https://god.buildwithoracle.com")
                    .body(Body::empty())
                    .expect("missing"),
            )
            .await
            .expect("missing response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
        assert_eq!(
            missing
                .headers()
                .get("access-control-allow-headers")
                .and_then(|value| value.to_str().ok()),
            Some("Content-Type, Authorization")
        );
    }

    #[tokio::test]
    async fn servecore_loopback_allows_protected_paths_and_public_still_passes() {
        let addr = servecore_spawn_test_server().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::OK);
        let plugins = client
            .post(format!("http://{addr}/api/plugins/reload"))
            .send()
            .await
            .expect("plugins");
        assert_eq!(plugins.status(), StatusCode::OK);
        let public = client
            .get(format!("http://{addr}/api/serve-core/pipeline"))
            .send()
            .await
            .expect("public");
        assert_eq!(public.status(), StatusCode::OK);
    }

    fn servecore_auth_test_app(state: ServecoreSharedState) -> Router {
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_with_shared_state(router, state);
        servecore_apply_pipeline(router)
    }

    async fn servecore_auth_request(
        state: ServecoreSharedState,
        mut request: Request<Body>,
        peer: SocketAddr,
    ) -> Response {
        request.extensions_mut().insert(ConnectInfo(peer));
        request.extensions_mut().insert(Arc::new(state.clone()));
        servecore_auth_test_app(state)
            .oneshot(request)
            .await
            .expect("auth request")
    }

    #[tokio::test]
    async fn servecore_nonloopback_no_credentials_and_xff_spoof_fail_closed() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .body(Body::empty())
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty())
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn servecore_accepts_real_maw_js_stacked_fleet_hmac_v3_headers() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default()
            .servecore_with_auth(Some("fake-federation-token-393".to_owned()), None)
            .servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "nova:codex4")
            .header(
                "x-maw-signature",
                "536c867f3d9aa1f97c6c00c6b7e0337fe3d6d9c47ce1e38efe9d58d726d2c821",
            )
            .header(
                "x-maw-signature-v3",
                "19603ec4c4b9c6ad630809f50bc346066bb553b557b07d9809dfb62d4fb714a2",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "v3")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state, request, peer).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn servecore_rejects_wrong_fleet_token_even_with_valid_from_sign_header() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default()
            .servecore_with_auth(Some("wrong-federation-token".to_owned()), None)
            .servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "nova:codex4")
            .header(
                "x-maw-signature",
                "536c867f3d9aa1f97c6c00c6b7e0337fe3d6d9c47ce1e38efe9d58d726d2c821",
            )
            .header(
                "x-maw-signature-v3",
                "19603ec4c4b9c6ad630809f50bc346066bb553b557b07d9809dfb62d4fb714a2",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "v3")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state, request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn servecore_ed25519_from_sign_allows_nonloopback_and_pins_first_contact() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default().servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "mawjs:m5")
            .header(
                "x-maw-ed25519-signature",
                concat!(
                    "d232e00767facc77aca0eaaf2ebc18dc3c608639430f93167679805c7e3ccf69",
                    "f15a856c7d8f4eddf64730cc61d4ccc0c28ca91b9a9df1a5016c628d737b3a0f"
                ),
            )
            .header(
                "x-maw-ed25519-pubkey",
                "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "ed25519")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state.clone(), request, peer).await;
        assert_eq!(response.status(), StatusCode::OK);
        let pins = state
            .auth_ed25519_pins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            pins.pinned("mawjs:m5"),
            Some("79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664")
        );
    }

    #[tokio::test]
    async fn servecore_orchestration_workon_is_auth_gated_and_loopback_can_spawn_fake() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let payload = r#"{"repo":"demo","engine":"any-engine","target":"nova:1"}"#;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/orchestration/workon")
            .body(Body::from(payload))
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let orchestrator = Arc::new(FakeOrchestrator::default());
        let state =
            ServecoreSharedState::default().servecore_with_orchestrator(orchestrator.clone());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/orchestration/workon")
            .body(Body::from(payload))
            .expect("request");
        let response =
            servecore_auth_request(state, request, SocketAddr::from(([127, 0, 0, 1], 49_152)))
                .await;
        assert_eq!(response.status(), StatusCode::OK);
        let calls = orchestrator
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].engine.as_deref(), Some("any-engine"));
    }

    #[tokio::test]
    async fn servecore_ws_uses_engine_hook_and_loopback_auth() {
        let engine = Arc::new(TestEngine::default());
        let state = ServecoreSharedState::default().servecore_with_engine(engine.clone());
        let addr =
            servecore_spawn_ws_test_server(state, modules::websocket_routes::WsConfig::default())
                .await;
        let url = format!("ws://{addr}/ws/tmux?target=nova:1.0");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            "hello".to_owned(),
        ))
        .await
        .expect("send");
        loop {
            let received = ws.next().await.expect("frame").expect("frame ok");
            if let tokio_tungstenite::tungstenite::Message::Text(text) = received {
                assert_eq!(text, "Tmux:nova:1.0:hello");
                break;
            }
        }
        assert_eq!(
            engine
                .opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_slice(),
            &[(ServecoreWsKind::Tmux, Some("nova:1.0".to_owned()))]
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn servecore_ws_rejects_bad_tunnel_target_before_upgrade() {
        let addr = servecore_spawn_ws_test_server(
            ServecoreSharedState::default(),
            modules::websocket_routes::WsConfig::default(),
        )
        .await;
        let err = tokio_tungstenite::connect_async(format!("ws://{addr}/ws/tmux?target=-danger"))
            .await
            .expect_err("bad target must be rejected before upgrade");
        assert!(err.to_string().contains("400"));
    }

    #[tokio::test]
    async fn servecore_ws_idle_timeout_closes_dead_connection() {
        let config = modules::websocket_routes::WsConfig {
            idle_timeout: Duration::from_millis(80),
            heartbeat_interval: Duration::from_millis(20),
            send_timeout: Duration::from_millis(50),
            max_frame_bytes: 1024,
            max_connections: 8,
        };
        let addr = servecore_spawn_ws_test_server(ServecoreSharedState::default(), config).await;
        let (mut ws, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws/tmux"))
            .await
            .expect("connect websocket");
        let close = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) = ws.next().await
                {
                    break;
                }
            }
        })
        .await;
        assert!(close.is_ok());
    }
}
