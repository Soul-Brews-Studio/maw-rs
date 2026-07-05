use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};
#[cfg(test)]
use std::net::Ipv4Addr;

const DEFAULT_SERVE_PORT: u16 = 3456;
const DEFAULT_SERVE_BIND: &str = "0.0.0.0";
const SERVE_FEED_MAX: usize = 200;
const SERVE_LOG_TEXT_MAX: usize = 2_000;
const SERVE_LOG_ERROR_MAX: usize = 1_000;
#[cfg(test)]
const NON_LOOPBACK_TEST_PEER: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)), 49_152);

struct ServeState {
    cached_pubkey: Option<String>,
    peer_pubkeys: Vec<ServePeerPubkey>,
    workspace_key: Option<String>,
    workspaces: Mutex<WorkspaceStore>,
    requests: Mutex<RequestReplyStore>,
    delivery: Arc<dyn ServeDelivery>,
    receiver_inbox: Arc<dyn ServeReceiverInbox>,
    feed: Mutex<Vec<Value>>,
    #[cfg(test)]
    peer_addr_override: Option<SocketAddr>,
    #[cfg(test)]
    now_override: Option<i64>,
    #[cfg(test)]
    serve_core_state_override: Option<crate::serve_core::ServecoreSharedState>,
    trust_store_path: std::path::PathBuf,
}

trait ServeDelivery: Send + Sync {
    fn route_sessions(&self) -> Result<Vec<RouteSession>, String>;
    fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String>;
    fn capture_tail(&self, target: &str, lines: u32) -> Result<String, String>;
}

struct ServeSystemDelivery;

trait ServeReceiverInbox: Send + Sync {
    fn write_receiver_inbox(&self, input: ReceiverInboxInput<'_>) -> ReceiverInboxResult;
}

#[derive(Default)]
struct ServeSystemReceiverInbox {
    #[cfg(test)]
    enabled: Option<bool>,
    #[cfg(test)]
    fixed_now_millis: Option<u128>,
    #[cfg(test)]
    psi_root: Option<std::path::PathBuf>,
}

impl ServeReceiverInbox for ServeSystemReceiverInbox {
    fn write_receiver_inbox(&self, input: ReceiverInboxInput<'_>) -> ReceiverInboxResult {
        let enabled = {
            #[cfg(test)]
            {
                self.enabled.unwrap_or_else(receiver_inbox_auto_write_enabled)
            }
            #[cfg(not(test))]
            {
                receiver_inbox_auto_write_enabled()
            }
        };
        if !enabled {
            return ReceiverInboxResult::Err {
                oracle: None,
                reason: "receiver inbox auto-write disabled".to_owned(),
            };
        }
        let now_millis = {
            #[cfg(test)]
            {
                self.fixed_now_millis.unwrap_or_else(receiver_inbox_now_millis)
            }
            #[cfg(not(test))]
            {
                receiver_inbox_now_millis()
            }
        };
        let psi_root = {
            #[cfg(test)]
            {
                self.psi_root.as_deref()
            }
            #[cfg(not(test))]
            {
                None
            }
        };
        persist_receiver_inbox(input, now_millis, psi_root)
    }
}

impl ServeDelivery for ServeSystemDelivery {
    fn route_sessions(&self) -> Result<Vec<RouteSession>, String> {
        let mut tmux = TmuxClient::local();
        Ok(route_sessions_from_tmux(&mut tmux))
    }

    fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String> {
        let mut tmux = TmuxClient::local();
        tmux.send_keys_literal(target, text).map_err(|error| error.to_string())?;
        tmux.send_enter(target).map_err(|error| error.to_string())
    }

    fn capture_tail(&self, target: &str, lines: u32) -> Result<String, String> {
        let mut tmux = TmuxClient::local();
        tmux.capture(target, Some(lines)).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServeArgs {
    host: String,
    port: u16,
    cached_pubkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServePeerPubkey {
    from: String,
    node: String,
    pubkey: String,
}

fn run_serve_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_serve_async_impl(&args).await })
}

async fn run_serve_async_impl(raw_args: &[String]) -> CliOutput {
    if let Some(output) = serve_lifecycle_subcommand152(raw_args) { return output; }
    let args = match parse_serve_args(raw_args) {
        Ok(args) => args,
        Err(message) => return serve_usage_error(&message),
    };
    let addr = match resolve_serve_socket_addr(&args) {
        Ok(addr) => addr,
        Err(message) => return serve_usage_error(&message),
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("serve: failed to bind {addr}: {error}\n"),
            }
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(error) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("serve: failed to read bound address: {error}\n"),
            }
        }
    };
    let app = serve_router(ServeState {
        cached_pubkey: args.cached_pubkey,
        peer_pubkeys: load_inbound_peer_pubkeys(),
        workspace_key: load_serve_workspace_key(),
        workspaces: Mutex::new(WorkspaceStore::default()),
        requests: Mutex::new(RequestReplyStore::default()),
        delivery: Arc::new(ServeSystemDelivery),
        receiver_inbox: Arc::new(ServeSystemReceiverInbox::default()),
        feed: Mutex::new(Vec::new()),
        #[cfg(test)]
        peer_addr_override: None,
        #[cfg(test)]
        now_override: None,
        #[cfg(test)]
        serve_core_state_override: None,
        trust_store_path: trust_store_path(),
    });
    println!("maw-rs serve listening http://{local_addr}");
    match axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        Ok(()) => CliOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("serve: server error: {error}\n"),
        },
    }
}

