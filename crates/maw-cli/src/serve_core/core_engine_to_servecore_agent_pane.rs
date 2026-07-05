pub mod engine;
pub mod modules;

pub use engine::{ServecoreExecRunner, ServecoreNativeEngine, ServecoreProcessRunner};

use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo,
    },
    http::{Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use maw_hub::WorkspaceConfig;
use maw_tmux::{TmuxClient, TmuxPane};
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

