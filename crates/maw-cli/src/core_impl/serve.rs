use axum::{
    body::{Body, Bytes},
    extract::{ws::WebSocketUpgrade, ConnectInfo, Path as AxumPath, Query, State},
    http::{header::{CACHE_CONTROL, CONTENT_TYPE}, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Instant,
};
#[cfg(test)]
use std::net::Ipv4Addr;

const DEFAULT_SERVE_PORT: u16 = 3456;
const DEFAULT_SERVE_BIND: &str = "0.0.0.0";
pub(crate) const SERVE_WS_PROTOCOL: &str = "maw.ws.v1";
const SERVE_FEED_MAX: usize = 200;
const SERVE_LOG_TEXT_MAX: usize = 2_000;
const SERVE_LOG_ERROR_MAX: usize = 1_000;
/// How long serve trusts its cached `peers.json`/config pubkeys before a
/// protected request triggers a reload — small so an added peer works within
/// seconds, large enough to avoid re-reading the file on every request (#16).
const PEER_PUBKEY_RELOAD_TTL_SECS: u64 = 3;
const DELIVERY_IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;
#[cfg(test)]
const NON_LOOPBACK_TEST_PEER: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)), 49_152);

struct ServeState {
    cached_pubkey: Option<String>,
    peer_pubkeys: HotReload<Vec<ServePeerPubkey>>,
    workspace_key: Option<String>,
    workspaces: Mutex<WorkspaceStore>,
    requests: Mutex<RequestReplyStore>,
    delivery: Arc<dyn ServeDelivery>,
    receiver_inbox: Arc<dyn ServeReceiverInbox>,
    wake: Arc<dyn ServeWakeExecutor>,
    delivery_idempotency: Mutex<DeliveryIdempotencyStore>,
    feed: Mutex<Vec<Value>>,
    #[cfg(test)]
    peer_addr_override: Option<SocketAddr>,
    #[cfg(test)]
    now_override: Option<i64>,
    #[cfg(test)]
    serve_core_state_override: Option<crate::serve_core::ServecoreSharedState>,
    trust_store_path: std::path::PathBuf,
    plugin_serve_routes: Vec<ServePluginRoute>,
    api_token_auth: ServeApiTokenAuth,
    bound_port: u16,
}




#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ServeApiTokenAuth {
    token: Option<String>,
    loopback_exempt: bool,
    forced_open: bool,
}

#[derive(Clone, Copy)]
struct ServeOperatorAuth;

#[derive(Clone)]
struct ServeApiGateState {
    auth: ServeApiTokenAuth,
    ws_tickets: Arc<maw_auth::WsTicketStore>,
}

tokio::task_local! { static SERVE_OPERATOR_CONTEXT: (); }

fn serve_mark_operator_authenticated(req: &mut Request<Body>) { req.extensions_mut().insert(ServeOperatorAuth); }

pub(crate) fn serve_operator_authenticated(req: &Request<Body>) -> bool { req.extensions().get::<ServeOperatorAuth>().is_some() }

fn serve_operator_context_authenticated() -> bool { SERVE_OPERATOR_CONTEXT.try_with(|()| ()).is_ok() }

impl ServeApiTokenAuth {
    #[cfg(test)]
    fn open() -> Self { Self { token: None, loopback_exempt: true, forced_open: true } }

    /// The banner names what is ENFORCED, not just how the token resolved:
    /// without a configured token there is nothing to mint or validate a
    /// one-use ws-ticket against, so every Origin-bearing browser client is
    /// refused in BOTH open modes. Reporting that as a bare `open` cost a
    /// full debugging session (#955).
    fn mode_label(&self) -> &'static str {
        if self.forced_open {
            "open (configured) (browser clients refused; the ws-ticket flow needs MAW_SERVE_TOKEN, which overrides this open mode)"
        } else if self.token.is_some() {
            "token (browser clients mint a one-use ws-ticket per path)"
        } else {
            "open (no token \u{2014} browser clients refused; set MAW_SERVE_TOKEN to enable the ws-ticket flow)"
        }
    }

    fn token_matches(&self, headers: &HeaderMap) -> bool {
        let Some(token) = self.token.as_deref() else { return true; };
        let bearer = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        bearer == Some(token) || header_to_string(headers, "x-maw-token") == token
    }
}

trait ServeDelivery: Send + Sync {
    fn route_sessions(&self) -> Result<Vec<RouteSession>, String>;
    fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String>;
    fn capture_tail(&self, target: &str, lines: u32) -> Result<String, String>;
}

struct ServeSystemDelivery;

trait ServeWakeExecutor: Send + Sync {
    fn execute_wake(&self, target: &str, task: Option<&str>) -> Result<String, String>;
}

struct ServeSystemWakeExecutor;

impl ServeWakeExecutor for ServeSystemWakeExecutor {
    fn execute_wake(&self, target: &str, task: Option<&str>) -> Result<String, String> {
        // Receiver-side federation wake runs the LOCAL native wake only —
        // never the peer-routing path — so a federated wake cannot re-forward
        // to another node and loop.
        //
        // run_wake_command takes a VERB-STRIPPED argv (the CLI dispatcher
        // strips the `wake` verb before calling it); passing "wake" here made
        // it a second positional, tripping the usage guard on every call (#533).
        let mut argv = vec![target.to_owned(), "--no-attach".to_owned()];
        if let Some(task) = task {
            argv.push("--task".to_owned());
            argv.push(task.to_owned());
        }
        let output = run_wake_command(&argv);
        if output.code == 0 {
            return Ok(output.stdout);
        }
        let stderr = output.stderr.trim();
        let detail = if stderr.is_empty() { output.stdout.trim() } else { stderr };
        let detail = if detail.is_empty() { "wake failed" } else { detail };
        Err(format!("wake exited {}: {detail}", output.code))
    }
}

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
        route_sessions_from_tmux(&mut tmux)
    }

    fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String> {
        let mut tmux = TmuxClient::local();
        tmux.send_text(target, text).map(|_| ()).map_err(|error| error.to_string())
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
    if wants_help(raw_args, &["--host", "--bind", "--port", "--cached-pubkey"]) {
        return help_output(serve_usage_text());
    }
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
    let _pidfile = match ServePidFileGuard::write_current_process(serve_pid_path152()) {
        Ok(pidfile) => pidfile,
        Err(error) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("serve: failed to write pidfile: {error}\n"),
            }
        }
    };
    let api_token_auth = load_serve_api_token_auth();
    let app = serve_router(ServeState {
        cached_pubkey: args.cached_pubkey,
        peer_pubkeys: HotReload::live(load_inbound_peer_pubkeys, PEER_PUBKEY_RELOAD_TTL_SECS),
        workspace_key: load_serve_workspace_key(),
        workspaces: Mutex::new(WorkspaceStore::default()),
        requests: Mutex::new(RequestReplyStore::default()),
        delivery: Arc::new(ServeSystemDelivery),
        receiver_inbox: Arc::new(ServeSystemReceiverInbox::default()),
        wake: Arc::new(ServeSystemWakeExecutor),
        delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
        feed: Mutex::new(Vec::new()),
        #[cfg(test)]
        peer_addr_override: None,
        #[cfg(test)]
        now_override: None,
        #[cfg(test)]
        serve_core_state_override: None,
        trust_store_path: trust_store_path(),
        plugin_serve_routes: serve_discover_plugin_routes(),
        api_token_auth: api_token_auth.clone(),
        bound_port: local_addr.port(),
    });
    println!("maw-rs serve listening http://{local_addr}");
    println!("maw-rs serve auth: {}", api_token_auth.mode_label());
    spawn_serve_peer_refresh();
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

/// Default background peer-refresh cadence for `maw serve`, in seconds.
const SERVE_PEER_REFRESH_DEFAULT_SECS: u64 = 60;

/// How often `maw serve` re-probes its peers and persists the result, in seconds.
/// `0` disables the sweep. Read from `MAW_PEER_REFRESH_SECS`; a missing or
/// unparseable value falls back to [`SERVE_PEER_REFRESH_DEFAULT_SECS`].
fn serve_peer_refresh_interval_secs() -> u64 {
    match std::env::var("MAW_PEER_REFRESH_SECS") {
        Ok(raw) => raw
            .trim()
            .parse::<u64>()
            .unwrap_or(SERVE_PEER_REFRESH_DEFAULT_SECS),
        Err(_) => SERVE_PEER_REFRESH_DEFAULT_SECS,
    }
}

/// Spawn the periodic peer-refresh sweep unless disabled. Detached: it runs for the
/// life of the serve process (torn down when the process exits). Each tick runs the
/// blocking probe-all on a blocking thread so it never stalls the HTTP runtime, and
/// persistence goes through the one existing peer-store writer
/// (`peers_probe_all_and_persist`) — never the read-only federation-map render, so
/// there is a single write path (#677/#684). This is what keeps the map's stored
/// `node` / `oracle` / `lastSeen` fresh instead of frozen at add time.
fn spawn_serve_peer_refresh() {
    let secs = serve_peer_refresh_interval_secs();
    if secs == 0 {
        println!("maw-rs serve peer-refresh: disabled (MAW_PEER_REFRESH_SECS=0)");
        return;
    }
    println!("maw-rs serve peer-refresh: every {secs}s");
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let outcome = tokio::task::spawn_blocking(|| {
                peers_probe_all_and_persist(PEERS_DEFAULT_PROBE_TIMEOUT_MS)
            })
            .await;
            match outcome {
                Ok(Err(error)) => eprintln!("maw-rs serve peer-refresh: {error}"),
                // A panic in the blocking probe surfaces as JoinError. Never swallow it:
                // a sweep that panics every tick would otherwise freeze lastSeen in total
                // silence — the exact #684 disease this task exists to cure.
                Err(join_error) => {
                    eprintln!("maw-rs serve peer-refresh: sweep task panicked: {join_error}");
                }
                Ok(Ok(_)) => {}
            }
        }
    });
}

struct ServePidFileGuard {
    path: std::path::PathBuf,
    pid: u32,
}

impl ServePidFileGuard {
    fn write_current_process(path: std::path::PathBuf) -> Result<Self, String> {
        let pid = std::process::id();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {} failed: {error}", parent.display()))?;
        }
        std::fs::write(&path, format!("{pid}\n"))
            .map_err(|error| format!("write {} failed: {error}", path.display()))?;
        Ok(Self { path, pid })
    }
}

impl Drop for ServePidFileGuard {
    fn drop(&mut self) {
        if messages_read_pid_file152(&self.path) == Some(self.pid) {
            let _ = messages_remove_file152(&self.path);
        }
    }
}

fn parse_serve_args(argv: &[String]) -> Result<ServeArgs, String> {
    let mut host = default_bind_host();
    let mut port = DEFAULT_SERVE_PORT;
    let mut cached_pubkey = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--host" | "--bind" => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| "serve: missing --host value".to_owned())?;
                host.clone_from(value);
                index += 1;
            }
            "--port" => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| "serve: missing --port value".to_owned())?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| "serve: --port must be 0..65535".to_owned())?;
                index += 1;
            }
            "--cached-pubkey" => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| "serve: missing --cached-pubkey value".to_owned())?;
                cached_pubkey = Some(value.clone());
                index += 1;
            }
            "--help" | "-h" => return Err(String::new()),
            value if value.starts_with("--host=") => value["--host=".len()..].clone_into(&mut host),
            value if value.starts_with("--bind=") => value["--bind=".len()..].clone_into(&mut host),
            value if value.starts_with("--port=") => {
                port = value["--port=".len()..]
                    .parse::<u16>()
                    .map_err(|_| "serve: --port must be 0..65535".to_owned())?;
            }
            value if value.starts_with("--cached-pubkey=") => {
                cached_pubkey = Some(value["--cached-pubkey=".len()..].to_owned());
            }
            value if value.starts_with('-') => return Err(format!("serve: unknown argument {value}")),
            value => return Err(format!("serve: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(ServeArgs {
        host,
        port,
        cached_pubkey,
    })
}

fn serve_usage_error(message: &str) -> CliOutput {
    let prefix = if message.is_empty() {
        String::new()
    } else {
        format!("{message}\n")
    };
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{prefix}{}\n", serve_usage_text()),
    }
}

fn serve_usage_text() -> &'static str {
    "usage: maw-rs serve [--host 0.0.0.0] [--port <port>] [--cached-pubkey <key>] | maw-rs serve status|--status|stop"
}

fn default_bind_host() -> String {
    DEFAULT_SERVE_BIND.to_owned()
}

fn resolve_serve_socket_addr(args: &ServeArgs) -> Result<SocketAddr, String> {
    if args.host.is_empty()
        || args.host.starts_with('-')
        || args.host.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("serve: --host must be an IP address".to_owned());
    }
    let host = args
        .host
        .parse::<IpAddr>()
        .map_err(|_| "serve: --host must be an IP address".to_owned())?;
    Ok(SocketAddr::new(host, args.port))
}

fn serve_core_state(state: &ServeState) -> crate::serve_core::ServecoreSharedState {
    #[cfg(not(test))]
    let _ = state;
    #[cfg(test)]
    if let Some(state) = &state.serve_core_state_override {
        return state.clone();
    }
    let HeyConfig {
        node,
        oracle,
        route,
    } = load_hey_config();
    let god_config = crate::serve_core::ServecoreGodConfig::servecore_from_parts(
        node.as_deref(),
        route.agents.into_iter().collect(),
        route
            .named_peers
            .into_iter()
            .map(|peer| crate::serve_core::ServecoreGodNamedPeer {
                name: peer.name,
                url: peer.url,
            })
            .collect(),
    );
    let core = crate::serve_core::ServecoreSharedState::default()
        .servecore_with_engine(Arc::new(crate::serve_core::ServecoreNativeEngine))
        .servecore_with_agents_node(node)
        .servecore_with_agents_oracle(oracle)
        .servecore_with_god_config(god_config)
        .servecore_with_auth(state.workspace_key.clone(), None);
    #[cfg(not(test))]
    let core = core.servecore_with_process_auth_pins();
    #[cfg(test)]
    let core = if let Some(now) = state.now_override {
        core.servecore_with_auth_now(now)
    } else {
        core
    };
    core
}

fn serve_router(state: ServeState) -> Router {
    serve_router_with_ws_tickets(state, Arc::new(maw_auth::WsTicketStore::default()))
}

fn serve_router_with_ws_tickets(
    state: ServeState,
    ws_tickets: Arc<maw_auth::WsTicketStore>,
) -> Router {
    let serve_core_state = serve_core_state(&state);
    let plugin_serve_routes = state.plugin_serve_routes.clone();
    let api_gate = ServeApiGateState {
        auth: state.api_token_auth.clone(),
        ws_tickets: ws_tickets.clone(),
    };
    let state = Arc::new(state);
    let router = Router::new();
    let router = crate::serve_core::servecore_mount_core_routes(router);
    let router = crate::serve_core::modules::servecore_mount_modules(router, &[]);
    let router = serve_mount_plugin_routes(router, &plugin_serve_routes);
    let router = router
        .route(
            "/api/auth/ws-ticket",
            post(api_ws_ticket).layer(Extension(ws_tickets)),
        )
        .route("/api/send", post(api_send))
        .route("/api/feed", get(api_feed_get).post(api_feed_post))
        .route("/api/sessions", get(api_sessions))
        .route("/api/capture", get(api_capture))
        .route("/api/probe", post(api_probe))
        .route("/api/wake", post(api_wake))
        .route("/api/pane-keys", post(api_pane_keys))
        .route("/api/transport/status", get(api_transport_status))
        .route("/api/transport/send", post(api_transport_send))
        .route("/api/health", get(api_health))
        .route("/api/message-ledger", get(api_message_ledger))
        .route("/api/requests", get(api_requests))
        .route("/api/trust", get(api_trust_list).post(api_trust_add))
        .route("/api/trust/revoke", post(api_trust_revoke))
        .route("/api/request", post(api_request_create))
        .route("/api/reply/:correlation_id", post(api_reply))
        .route("/api/workspace/create", post(api_workspace_create))
        .route("/api/workspace/join", post(api_workspace_join))
        .route(
            "/api/workspace/:id/agents",
            get(api_workspace_agents_get).post(api_workspace_agents_post),
        )
        .route("/api/workspace/:id/status", get(api_workspace_status))
        .route("/api/workspace/:id/feed", get(api_workspace_feed))
        .route("/api/workspace/:id/message", post(api_workspace_message));
    let router = router.fallback(api_not_found);
    let router = crate::serve_core::servecore_apply_pipeline(router);
    let router = router.layer(middleware::from_fn_with_state(
        api_gate,
        serve_api_token_gate,
    ));
    let router = router.layer(middleware::from_fn(
        crate::serve_core::servecore_cors_preflight,
    ));
    let router = crate::serve_core::servecore_with_shared_state(router, serve_core_state);
    router.with_state(state)
}

/// `/ws` and `/ws/*` carry a `send` frame that reaches `tmux send-keys` (#828),
/// so the websocket surface never gets the `/api/` fall-open: an absent or
/// `authMode:"open"` token refuses the upgrade instead of accepting it.
fn serve_path_is_websocket(path: &str) -> bool {
    path == "/ws" || path.starts_with("/ws/")
}

async fn serve_api_token_gate(
    State(gate): State<ServeApiGateState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if !crate::serve_core::servecore_request_origin_allowed(req.headers()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"forbidden","reason":"origin-not-allowed"})),
        )
            .into_response();
    }
    let path = req.uri().path();
    let auth = &gate.auth;
    if path == "/api/auth/ws-ticket" {
        if req.headers().contains_key("origin")
            && !auth.forced_open
            && auth.token.is_some()
            && auth.token_matches(req.headers())
        {
            serve_mark_operator_authenticated(&mut req);
            return SERVE_OPERATOR_CONTEXT.scope((), next.run(req)).await;
        }
        return serve_token_unauthorized();
    }
    if req.headers().contains_key("origin") && serve_path_is_websocket(path) {
        if auth.forced_open || auth.token.is_none() {
            return serve_token_unauthorized();
        }
        let Ok(ticket_path) = maw_auth::WsTicketPath::try_from(path) else {
            return serve_token_unauthorized();
        };
        if !serve_apparent_ws_upgrade(&req) {
            return serve_token_unauthorized();
        }
        let Some((origin, ticket)) = serve_browser_ws_ticket(&req) else {
            return serve_token_unauthorized();
        };
        match gate
            .ws_tickets
            .consume(&ticket, Some(&origin), ticket_path, Instant::now)
        {
            maw_auth::WsTicketConsume::Accepted => {
                req.headers_mut().insert(
                    "sec-websocket-protocol",
                    HeaderValue::from_static(SERVE_WS_PROTOCOL),
                );
                crate::serve_core::servecore_mark_ws_ticket_accepted(&mut req);
                return next.run(req).await;
            }
            maw_auth::WsTicketConsume::Rejected => return serve_token_unauthorized(),
            maw_auth::WsTicketConsume::Unavailable => {
                return StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
    let websocket = serve_path_is_websocket(path);
    if !websocket
        && (!path.starts_with("/api/")
            || path == "/api/health"
            || auth.forced_open
            || auth.token.is_none())
    {
        return next.run(req).await;
    }
    if auth.loopback_exempt {
        if let Some(ConnectInfo(peer)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
            if maw_auth::is_loopback(Some(&peer.ip().to_string())) {
                return next.run(req).await;
            }
        }
    }
    // `token_matches` answers true for an unconfigured token; on the websocket
    // path that would reinstate the very fall-open this gate exists to close.
    if auth.token.is_some() && auth.token_matches(req.headers()) {
        if !websocket && req.headers().contains_key("origin") {
            serve_mark_operator_authenticated(&mut req);
            return SERVE_OPERATOR_CONTEXT.scope((), next.run(req)).await;
        }
        return next.run(req).await;
    }
    serve_token_unauthorized()
}

fn serve_apparent_ws_upgrade(req: &Request<Body>) -> bool {
    fn one(headers: &HeaderMap, name: &str, expected: &str, max: usize) -> bool {
        let mut values = headers.get_all(name).iter();
        let Some(value) = values.next() else { return false; };
        values.next().is_none()
            && value.as_bytes().len() <= max
            && value
                .to_str()
                .is_ok_and(|value| value.eq_ignore_ascii_case(expected))
    }
    fn connection(headers: &HeaderMap) -> bool {
        let mut values = headers.get_all("connection").iter();
        let Some(value) = values.next() else { return false; };
        values.next().is_none()
            && value.as_bytes().len() <= 128
            && value.to_str().is_ok_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
    }
    req.method() == Method::GET
        && connection(req.headers())
        && one(req.headers(), "upgrade", "websocket", 32)
        && one(req.headers(), "sec-websocket-version", "13", 8)
        && {
            let mut keys = req.headers().get_all("sec-websocket-key").iter();
            keys.next()
                .is_some_and(|key| !key.is_empty() && key.as_bytes().len() <= 128)
                && keys.next().is_none()
        }
}

fn serve_browser_ws_ticket(req: &Request<Body>) -> Option<(String, String)> {
    let origin = req.headers().get("origin")?.to_str().ok()?.to_owned();
    let mut values = req.headers().get_all("sec-websocket-protocol").iter();
    let raw = values.next()?;
    if values.next().is_some() || raw.as_bytes().len() > 128 {
        return None;
    }
    let raw = raw.to_str().ok()?;
    let (stable, ticket) = raw.split_once(',')?;
    if ticket.contains(',')
        || stable.trim_end_matches([' ', '\t']) != SERVE_WS_PROTOCOL
        || stable.starts_with([' ', '\t'])
        || ticket.trim_end_matches([' ', '\t']) != ticket
    {
        return None;
    }
    Some((origin, ticket.trim_start_matches([' ', '\t']).to_owned()))
}

fn serve_token_unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized", "auth": "maw-serve-token"}))).into_response()
}

/// Same `{"error", …}` envelope the origin gate already answers with, so a
/// ws-ticket 400 says WHICH of the five client-side contract violations it
/// was instead of an empty body (#955). `reason` is a fixed slug from the
/// list below — never request-derived text, so nothing can leak.
fn serve_ws_ticket_bad_request(reason: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": "bad-request", "reason": reason}))).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WsTicketRequest {
    path: String,
}

#[derive(Serialize)]
struct WsTicketResponse {
    ticket: String,
    protocol: &'static str,
}

async fn api_ws_ticket(
    operator: Option<Extension<ServeOperatorAuth>>,
    Extension(store): Extension<Arc<maw_auth::WsTicketStore>>,
    req: Request<Body>,
) -> Response {
    if operator.is_none() {
        return serve_token_unauthorized();
    }
    let origin = req
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let query_present = req.uri().query().is_some();
    let json_content_type = req.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "application/json" || value.starts_with("application/json;"));
    // Order is reporting-only: each guard rejects exactly the requests it
    // rejected before, and a request that trips several still gets one 400.
    if !json_content_type {
        return serve_ws_ticket_bad_request("content-type-not-json");
    }
    if query_present {
        return serve_ws_ticket_bad_request("query-string-not-allowed");
    }
    let Ok(body) = axum::body::to_bytes(req.into_body(), 128).await else {
        return serve_ws_ticket_bad_request("body-too-large");
    };
    let Ok(payload) = serde_json::from_slice::<WsTicketRequest>(&body) else {
        return serve_ws_ticket_bad_request("body-not-a-ws-ticket-request");
    };
    let Ok(path) = maw_auth::WsTicketPath::try_from(payload.path.as_str()) else {
        return serve_ws_ticket_bad_request("path-not-allowed");
    };
    // Unreachable through the gate, which only forwards Origin-bearing
    // requests; kept so a non-UTF-8 Origin header still answers a reason.
    let Some(origin) = origin.as_deref() else {
        return serve_ws_ticket_bad_request("origin-missing");
    };
    let Ok(ticket) = store.issue(origin, path, &mut OsRng, Instant::now) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    (
        [(CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(WsTicketResponse {
            ticket: ticket.expose_secret().to_owned(),
            protocol: SERVE_WS_PROTOCOL,
        }),
    )
        .into_response()
}




















async fn api_send(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    match verify_protected_request_outcome(&state, peer, &method, &uri, &headers, &body) {
        ProtectedRequestOutcome::Accept => serve_deliver_send(&state, &headers, &body),
        ProtectedRequestOutcome::Reject { decision, response } => {
            serve_log_lifecycle(
                &state,
                json!({
                    "kind": "message",
                    "direction": "inbound",
                    "state": "failed",
                    "event": "auth-reject",
                    "decision": serve_truncate(&decision, SERVE_LOG_ERROR_MAX),
                    "route": "auth",
                    "source": "serve",
                }),
            );
            response
        }
    }
}

async fn api_feed_get(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<FeedQuery>,
) -> impl IntoResponse {
    let events = serve_feed_snapshot(&state, query.limit);
    let mut active_oracles = Vec::<String>::new();
    for event in &events {
        if let Some(oracle) = event.get("oracle").and_then(Value::as_str) {
            if !active_oracles.iter().any(|item| item == oracle) {
                active_oracles.push(oracle.to_owned());
            }
        }
    }
    Json(json!({"events": events, "total": events.len(), "active_oracles": active_oracles}))
}


fn serve_deliver_send(
    state: &ServeState,
    headers: &HeaderMap,
    body: &Bytes,
) -> axum::response::Response {
    let parsed = serde_json::from_slice::<SendBody>(body).unwrap_or_default();
    let target = parsed.target.clone().unwrap_or_default();
    let message = serve_send_message(&parsed);
    let raw_from = header_to_string(headers, "x-maw-from");
    let from = (!raw_from.trim().is_empty()).then_some(raw_from);
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle(&config);
    let log_from = from
        .clone()
        .unwrap_or_else(|| serve_local_identity(&config, &sender_oracle));
    let log_to = serve_local_identity(&config, &sender_oracle);

    if target.trim().is_empty() {
        serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, "empty-target", "validate");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "empty-target", "state": "failed"})),
        )
            .into_response();
    }

    if parsed.inbox.unwrap_or(false) {
        let context = ServeInboxContext {
            config: &config,
            sender_oracle: &sender_oracle,
            log_from: &log_from,
            log_to: &log_to,
            target: &target,
            message: &message,
        };
        return serve_deliver_inbox(state, headers, &parsed, &context);
    }

    let sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "route-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", &target, &error);
        }
    };

    match resolve_route_target(&target, &config.route, &sessions) {
        RouteResult::Local { target: resolved } | RouteResult::SelfNode { target: resolved } => {
            let context = ServeDeliverContext {
                config: &config,
                sender_oracle: &sender_oracle,
                from: from.as_deref(),
                log_from: &log_from,
                log_to: &log_to,
                requested: &target,
                resolved: &resolved,
                message: &message,
                idempotency_key: serve_delivery_idempotency_key(
                    headers,
                    &log_from,
                    &resolved,
                    &message,
                ),
            };
            serve_deliver_local(state, &context)
        }
        RouteResult::Peer { node, .. } => {
            let error = format!("peer-forward-unavailable:{node}");
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "peer-forward");
            serve_delivery_error(StatusCode::BAD_GATEWAY, "peer-forward-unavailable", &target, &error)
        }
        RouteResult::Error { reason, detail, .. } => {
            let error = format!("{reason}: {detail}");
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "resolve");
            serve_delivery_error(StatusCode::NOT_FOUND, &reason, &target, &detail)
        }
    }
}


struct ServeInboxContext<'a> {
    config: &'a HeyConfig,
    sender_oracle: &'a str,
    log_from: &'a str,
    log_to: &'a str,
    target: &'a str,
    message: &'a str,
}

fn serve_deliver_inbox(
    state: &ServeState,
    headers: &HeaderMap,
    parsed: &SendBody,
    context: &ServeInboxContext<'_>,
) -> axum::response::Response {
    let target = context.target;
    let message = context.message;
    let config = context.config;
    let log_from = context.log_from;
    let log_to = context.log_to;
    let sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "route-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", target, &error);
        }
    };
    let resolved = match resolve_route_target(target, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => target,
        RouteResult::Peer { node, .. } => {
            let error = format!("peer-forward-unavailable:{node}");
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "peer-forward");
            return serve_delivery_error(StatusCode::BAD_GATEWAY, "peer-forward-unavailable", target, &error);
        }
        RouteResult::Error { reason, detail, .. } => {
            let error = format!("{reason}: {detail}");
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "resolve");
            return serve_delivery_error(StatusCode::NOT_FOUND, &reason, target, &detail);
        }
    };
    if !serve_resolved_target_exists(&sessions, &resolved) {
        let error = format!("target not live in tmux: {resolved}");
        serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "inbox");
        return serve_delivery_error(StatusCode::NOT_FOUND, "target-not-live", target, &error);
    }
    let idempotency_key = match serve_claim_inbox_idempotency(state, headers, parsed, &resolved, context) {
        ServeInboxIdempotencyClaim::Claimed(key) => key,
        ServeInboxIdempotencyClaim::Duplicate(response) => return *response,
    };
    let from = serve_display_from(headers, config, context.sender_oracle);
    match state.receiver_inbox.write_receiver_inbox(ReceiverInboxInput {
        query: target,
        target: Some(&resolved),
        to: Some(target),
        from: &from,
        message,
        config,
    }) {
        ReceiverInboxResult::Ok(inbox) => {
            let reason = "--inbox requested; pane injection skipped";
            if let Some(key) = idempotency_key.clone() {
                serve_delivery_idempotency_complete(
                    state,
                    key,
                    &resolved,
                    "queued",
                    serve_delivery_idempotency_now(state),
                );
            }
            serve_log_lifecycle(
                state,
                json!({
                    "kind": "context.message",
                    "direction": "inbound",
                    "state": "queued",
                    "route": "inbox",
                    "from": serve_truncate(&from, SERVE_LOG_TEXT_MAX),
                    "to": serve_truncate(log_to, SERVE_LOG_TEXT_MAX),
                    "target": resolved,
                    "requestedTarget": target,
                    "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
                    "oracle": inbox.oracle,
                    "lastLine": reason,
                    "signed": !header_to_string(headers, "x-maw-from").trim().is_empty(),
                    "source": "maw-rs-native",
                }),
            );
            Json(json!({
                "ok": true,
                "target": resolved,
                "text": parsed.text.clone().unwrap_or_default(),
                "source": "inbox",
                "state": "queued",
                "inbox": inbox.path.display().to_string(),
                "reason": reason,
                "receipt": ["fallback_queued"],
            }))
            .into_response()
        }
        ReceiverInboxResult::Err { oracle: _, reason } => {
            if let Some(key) = idempotency_key.as_ref() {
                serve_delivery_idempotency_cancel(state, key);
            }
            serve_log_delivery_failed(state, target, message, log_from, log_to, &reason, "inbox");
            serve_delivery_error(StatusCode::BAD_GATEWAY, "receiver-inbox-unavailable", target, &reason)
        }
    }
}




















struct ServeDeliverContext<'a> {
    config: &'a HeyConfig,
    sender_oracle: &'a str,
    from: Option<&'a str>,
    log_from: &'a str,
    log_to: &'a str,
    requested: &'a str,
    resolved: &'a str,
    message: &'a str,
    idempotency_key: Option<DeliveryIdempotencyKey>,
}








fn serve_deliver_local(
    state: &ServeState,
    context: &ServeDeliverContext<'_>,
) -> axum::response::Response {
    let fresh_sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "toctou-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", context.requested, &error);
        }
    };
    if !serve_resolved_target_exists(&fresh_sessions, context.resolved) {
        let error = format!("target disappeared before delivery: {}", context.resolved);
        serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "toctou");
        return serve_delivery_error(StatusCode::NOT_FOUND, "target-disappeared", context.requested, &error);
    }

    let idempotency_key = context.idempotency_key.clone();
    if let Some(key) = idempotency_key.clone() {
        match serve_delivery_idempotency_claim(state, key.clone(), serve_delivery_idempotency_now(state)) {
            DeliveryIdempotencyClaim::Duplicate(record) => {
                serve_log_delivery_deduped(
                    state,
                    &key,
                    context.resolved,
                    context.message,
                    context.log_from,
                    context.log_to,
                    "local",
                );
                return serve_delivery_idempotency_response(
                    &record,
                    context.resolved,
                    context.message,
                    "maw-rs",
                );
            }
            DeliveryIdempotencyClaim::Claimed => {}
        }
    }

    let outbound = format_local_hey_message(
        context.message,
        context.config,
        context.sender_oracle,
        context.from,
    );
    if let Err(error) = state.delivery.send_literal_enter(context.resolved, &outbound) {
        if let Some(key) = idempotency_key.as_ref() {
            serve_delivery_idempotency_cancel(state, key);
        }
        serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "tmux-send");
        return serve_delivery_error(StatusCode::BAD_GATEWAY, "tmux-send-failed", context.resolved, &error);
    }

    let capture = state.delivery.capture_tail(context.resolved, 8).unwrap_or_default();
    let state_name = if capture.contains("Press up to edit queued messages") {
        "queued"
    } else {
        "delivered"
    };
    if let Some(key) = idempotency_key {
        serve_delivery_idempotency_complete(
            state,
            key,
            context.resolved,
            state_name,
            serve_delivery_idempotency_now(state),
        );
    }
    let last_line = serve_last_nonempty_line(&capture);
    serve_log_lifecycle(
        state,
        json!({
            "kind": "context.message",
            "direction": "inbound",
            "state": state_name,
            "route": "local",
            "context.from": serve_truncate(context.log_from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(context.log_to, SERVE_LOG_TEXT_MAX),
            "target": context.resolved,
            "requestedTarget": context.requested,
            "text": serve_truncate(context.message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(context.requested),
            "lastLine": serve_truncate(&last_line, SERVE_LOG_TEXT_MAX),
            "source": "maw-rs-native",
        }),
    );
    let non_agent_warning = serve_log_non_agent_pane_warning(state, context.resolved);
    Json(json!({
        "ok": true,
        "target": context.resolved,
        "text": context.message,
        "source": "maw-rs",
        "state": state_name,
        "lastLine": last_line,
        "warning": non_agent_warning,
    }))
    .into_response()
}

fn serve_delivery_error(
    status: StatusCode,
    error: &str,
    target: &str,
    detail: &str,
) -> axum::response::Response {
    (
        status,
        Json(json!({
            "ok": false,
            "error": error,
            "target": target,
            "detail": serve_truncate(detail, SERVE_LOG_ERROR_MAX),
            "state": "failed"
        })),
    )
        .into_response()
}

fn serve_log_delivery_failed(
    state: &ServeState,
    target: &str,
    message: &str,
    from: &str,
    to: &str,
    error: &str,
    route: &str,
) {
    serve_log_lifecycle(
        state,
        json!({
            "kind": "message",
            "direction": "inbound",
            "state": "failed",
            "route": route,
            "from": serve_truncate(from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(to, SERVE_LOG_TEXT_MAX),
            "target": target,
            "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(target),
            "error": serve_truncate(error, SERVE_LOG_ERROR_MAX),
            "source": "maw-rs-native",
        }),
    );
}

fn serve_log_lifecycle(state: &ServeState, event: Value) {
    match state.feed.lock() {
        Ok(mut feed) => serve_push_feed_event(&mut feed, event),
        Err(poisoned) => {
            let mut feed = poisoned.into_inner();
            serve_push_feed_event(&mut feed, event);
        }
    }
}

fn serve_push_feed_event(feed: &mut Vec<Value>, mut event: Value) {
    if let Value::Object(map) = &mut event {
        map.insert("timestamp".to_owned(), json!(unix_seconds()));
    }
    feed.push(event);
    if feed.len() > SERVE_FEED_MAX {
        let drain = feed.len() - SERVE_FEED_MAX;
        feed.drain(0..drain);
    }
}

fn serve_feed_snapshot(state: &ServeState, limit: Option<usize>) -> Vec<Value> {
    let events = match state.feed.lock() {
        Ok(feed) => feed.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some(limit) = limit {
        let start = events.len().saturating_sub(limit);
        events[start..].to_vec()
    } else {
        events
    }
}

fn serve_send_message(body: &SendBody) -> String {
    let text = body.text.clone().unwrap_or_default();
    match &body.attachments {
        Some(attachments) if !attachments.is_empty() => {
            let mut parts = attachments.clone();
            parts.push(text);
            parts.join("\n")
        }
        _ => text,
    }
}

fn serve_resolved_target_exists(sessions: &[RouteSession], target: &str) -> bool {
    if target.starts_with('%') {
        return false;
    }
    let (session_name, window_part) = target.split_once(':').unwrap_or((target, ""));
    let Some(session) = sessions.iter().find(|session| session.name == session_name) else {
        return false;
    };
    if window_part.is_empty() {
        return true;
    }
    let window_part = window_part.split('.').next().unwrap_or(window_part);
    session.windows.iter().any(|window| {
        window.index.to_string() == window_part || window.name.eq_ignore_ascii_case(window_part)
    })
}

fn serve_last_nonempty_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim_end()
        .to_owned()
}

fn serve_truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut out = value.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

fn serve_local_identity(config: &HeyConfig, sender_oracle: &str) -> String {
    let node = config.node.as_deref().unwrap_or("local");
    format!("{node}:{sender_oracle}")
}

fn serve_oracle_from_target(target: &str) -> String {
    target
        .split([':', '.'])
        .next()
        .unwrap_or(target)
        .to_owned()
}

fn serve_display_from(headers: &HeaderMap, config: &HeyConfig, sender_oracle: &str) -> String {
    let raw = header_to_string(headers, "x-maw-from");
    let raw = raw.trim();
    if raw.is_empty() {
        return serve_local_identity(config, sender_oracle);
    }
    if let Some((oracle, node)) = raw.split_once(':') {
        let oracle = oracle.trim();
        let node = node.trim();
        if !oracle.is_empty() && !node.is_empty() {
            return format!("{node}:{oracle}");
        }
    }
    raw.to_owned()
}



























async fn api_feed_post(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        response
    } else {
        if let Some(oracle) =
            crate::serve_core::modules::agent_status::agentstatus_oracle_from_feed_payload(&body)
        {
            crate::serve_core::modules::agent_status::agentstatus_mark_real_feed_event(&oracle);
        }
        Json(json!({"ok": true})).into_response()
    }
}

async fn api_sessions(Query(query): Query<SessionsQuery>) -> impl IntoResponse {
    let _local = query.local.unwrap_or(false);
    let mut tmux = TmuxClient::local();
    let panes = match tmux.list_panes() {
        Ok(panes) => panes,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": format!("tmux unreachable: {error}")})),
            )
                .into_response();
        }
    };
    let sessions = match tmux.list_all() {
        Ok(sessions) => sessions,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": format!("tmux unreachable: {error}")})),
            )
                .into_response();
        }
    }
    .into_iter()
    .map(|session| serve_tmux_session_json(&session, &panes))
    .collect::<Vec<_>>();
    Json(sessions).into_response()
}

async fn api_capture(Query(query): Query<CaptureQuery>) -> impl IntoResponse {
    let target = query.target.unwrap_or_default();
    let mut tmux = TmuxClient::local();
    let resolved = serve_resolve_capture_target(&target, &mut tmux);
    match tmux.capture(&resolved, None) {
        Ok(content) => Json(json!({"content": content, "target": target, "resolvedTarget": resolved})).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"content": "", "target": target, "resolvedTarget": resolved, "error": error.to_string()})),
        )
            .into_response(),
    }
}

fn serve_resolve_capture_target(
    target: &str,
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
) -> String {
    if target.trim().is_empty() || target.starts_with('%') {
        return target.to_owned();
    }
    // #860: tmux-unreachable here degrades the same way an unresolved route
    // already did (Peer/Error arms below) — fall through to the raw target
    // rather than erroring, since this helper has no response channel of its
    // own to report through; the caller (`api_capture`) still surfaces the
    // real tmux error from the subsequent `.capture()` call.
    let Ok(sessions) = route_sessions_from_tmux(tmux) else {
        return target.to_owned();
    };
    match resolve_route_target(target, &load_hey_config().route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => target,
        RouteResult::Peer { .. } | RouteResult::Error { .. } => target.to_owned(),
    }
}

fn serve_tmux_session_json(session: &TmuxSession, panes: &[TmuxPane]) -> Value {
    let windows = session
        .windows
        .iter()
        .map(|window| {
            let pane_prefix = format!("{}:{}.", session.name, window.name);
            let window_panes = panes
                .iter()
                .filter(|pane| pane.target.starts_with(&pane_prefix))
                .map(serve_tmux_pane_json)
                .collect::<Vec<_>>();
            json!({
                "index": window.index,
                "name": window.name,
                "active": window.active,
                "cwd": window.cwd,
                "panes": window_panes,
            })
        })
        .collect::<Vec<_>>();
    json!({"name": session.name, "windows": windows})
}

fn serve_tmux_pane_json(pane: &TmuxPane) -> Value {
    json!({
        "id": pane.id,
        "command": pane.command,
        "target": pane.target,
        "title": pane.title,
        "pid": pane.pid,
        "cwd": pane.cwd,
        "lastActivity": pane.last_activity,
    })
}

async fn api_probe(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        response
    } else {
        Json(json!({"ok": true, "transport": "local", "source": "maw-rs", "sessions": []})).into_response()
    }
}

async fn api_wake(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        return response;
    }
    let parsed = serde_json::from_slice::<WakeBody>(&body).unwrap_or_default();
    let target = parsed.target.unwrap_or_default();
    if target.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "empty-target"})),
        )
            .into_response();
    }
    match state.wake.execute_wake(&target, parsed.task.as_deref()) {
        Ok(_) => Json(json!({"ok": true, "target": target})).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "target": target, "error": error})),
        )
            .into_response(),
    }
}

async fn api_pane_keys(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        response
    } else {
        Json(json!({"ok": true})).into_response()
    }
}

async fn api_transport_status() -> impl IntoResponse {
    Json(json!({"transports": [{"name": "http-federation", "connected": true}]}))
}

async fn api_transport_send(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        response
    } else {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"ok": false, "via": "http", "reason": "peer-forward-unavailable", "retryable": true})),
        )
            .into_response()
    }
}

async fn api_health(State(state): State<Arc<ServeState>>) -> impl IntoResponse {
    Json(json!({"ok": true, "source": "maw-rs", "server": "local", "port": state.bound_port}))
}

async fn api_message_ledger(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<MessageLedgerQuery>,
) -> impl IntoResponse {
    let _ = query.json;
    let mut messages = serve_feed_snapshot(&state, None)
        .into_iter()
        .filter(|event| event.get("kind").and_then(Value::as_str) == Some("message"))
        .filter(|event| query.from.as_ref().is_none_or(|from| event.get("from").and_then(Value::as_str) == Some(from.as_str())))
        .filter(|event| query.to.as_ref().is_none_or(|to| event.get("to").and_then(Value::as_str) == Some(to.as_str())))
        .filter(|event| query.direction.as_ref().is_none_or(|direction| event.get("direction").and_then(Value::as_str) == Some(direction.as_str())))
        .filter(|event| query.state.as_ref().is_none_or(|state| event.get("state").and_then(Value::as_str) == Some(state.as_str())))
        .filter(|event| {
            query.q.as_ref().is_none_or(|q| {
                let haystack = event.to_string().to_lowercase();
                haystack.contains(&q.to_lowercase())
            })
        })
        .collect::<Vec<_>>();
    let total = messages.len();
    if let Some(limit) = query.limit {
        let start = messages.len().saturating_sub(limit);
        messages = messages[start..].to_vec();
    }
    Json(json!({"ok": true, "messages": messages, "total": total, "source": "maw-rs-native"}))
}

async fn api_requests(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<RequestListQuery>,
) -> impl IntoResponse {
    let requests = with_request_store(&state, |store| store.list(query.oracle.as_deref(), query.status.as_deref()));
    Json(json!({"requests": requests, "total": requests.len()}))
}

async fn api_request_create(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<RequestCreateBody>,
) -> impl IntoResponse {
    let entry = with_request_store(&state, |store| store.create(body));
    Json(json!({"correlationId": entry.correlation_id, "status": entry.status, "oracle": entry.to}))
}

async fn api_reply(
    State(state): State<Arc<ServeState>>,
    AxumPath(correlation_id): AxumPath<String>,
    Json(body): Json<ReplyBody>,
) -> impl IntoResponse {
    with_request_store(&state, |store| match store.reply(&correlation_id, body.reply, body.data) {
        ReplyResult::Ok => Json(json!({"ok": true, "correlationId": correlation_id})).into_response(),
        ReplyResult::NotFound => (StatusCode::NOT_FOUND, Json(json!({"error": "request not found"}))).into_response(),
        ReplyResult::AlreadyReplied => Json(json!({"error": "already replied", "correlationId": correlation_id})).into_response(),
    })
}


async fn api_trust_list(State(state): State<Arc<ServeState>>) -> impl IntoResponse {
    match trust_read_store(&state.trust_store_path) {
        Ok(entries) => Json(json!({
            "ok": true,
            "entries": trust_entries_json(&entries),
            "total": entries.len()
        }))
        .into_response(),
        Err(message) => trust_http_error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    }
}

async fn api_trust_add(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<TrustAddBody>,
) -> impl IntoResponse {
    match trust_store_add(
        &state.trust_store_path,
        &body.sender,
        &body.target,
        &body.peer_key,
        unix_millis_i64(),
    ) {
        Ok(outcome) => Json(json!({
            "ok": true,
            "state": trust_outcome_state(&outcome),
            "sender": body.sender,
            "target": body.target,
            "peerKey": "received (redacted)"
        }))
        .into_response(),
        Err(message) => trust_http_error(StatusCode::BAD_REQUEST, &message),
    }
}

async fn api_trust_revoke(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<TrustRevokeBody>,
) -> impl IntoResponse {
    if !body.yes.unwrap_or(false) {
        return trust_http_error(StatusCode::BAD_REQUEST, "trust revoke: missing explicit yes");
    }
    match trust_store_remove(&state.trust_store_path, &body.sender, &body.target) {
        Ok(true) => Json(json!({"ok": true, "state": "revoked"})).into_response(),
        Ok(false) => trust_http_error(StatusCode::NOT_FOUND, "trust revoke: entry not found"),
        Err(message) => trust_http_error(StatusCode::BAD_REQUEST, &message),
    }
}

fn trust_entries_json(entries: &[TrustEntryPlan]) -> Vec<Value> {
    let mut rows = entries.to_vec();
    rows.sort_by(|left, right| left.added_at.cmp(&right.added_at));
    rows.into_iter()
        .map(|entry| {
            json!({
                "sender": entry.sender,
                "target": entry.target,
                "addedAt": entry.added_at,
                "peerKey": if entry.peer_key.is_some() { "received (redacted)" } else { "missing" }
            })
        })
        .collect()
}

fn trust_outcome_state(outcome: &TrustWriteOutcome) -> &'static str {
    match outcome {
        TrustWriteOutcome::Added => "trusted",
        TrustWriteOutcome::AlreadyTrusted => "already-trusted",
        TrustWriteOutcome::UpdatedPin => "pin-updated",
    }
}

fn trust_http_error(status: StatusCode, message: &str) -> axum::response::Response {
    (status, Json(json!({"ok": false, "error": message}))).into_response()
}

fn unix_millis_i64() -> i64 {
    i64::try_from(unix_millis()).unwrap_or(i64::MAX)
}








async fn api_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"})))
}
















fn random_hex(bytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut data = vec![0_u8; bytes];
    rand::thread_rng().fill_bytes(&mut data);
    let mut output = String::with_capacity(bytes * 2);
    for byte in data {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn unix_seconds() -> i64 {
    i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX)
}

fn unix_millis() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}


fn load_serve_api_token_auth() -> ServeApiTokenAuth {
    let config = merged_config_value_for_env(&real_xdg_env());
    let serve = config.get("serve");
    let forced_open = serve
        .and_then(|v| v.get("authMode").or_else(|| v.get("auth")))
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("open"))
        || serve.and_then(|v| v.get("open")).and_then(Value::as_bool) == Some(true);
    let loopback_exempt = serve
        .and_then(|v| v.get("loopbackExempt").or_else(|| v.get("authLoopbackExempt")))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let config_token = serve
        .and_then(|v| v.get("token").or_else(|| v.get("apiToken")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let env_token = std::env::var("MAW_SERVE_TOKEN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let env_overrides = env_token.is_some();
    let token = env_token.or(config_token);
    ServeApiTokenAuth {
        token: if forced_open && !env_overrides { None } else { token },
        loopback_exempt,
        forced_open: forced_open && !env_overrides,
    }
}

fn load_serve_workspace_key() -> Option<String> {
    if let Ok(value) = std::env::var("MAW_FEDERATION_TOKEN") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    value
        .get("federationToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}















#[derive(Default, Deserialize)]
struct SendBody {
    target: Option<String>,
    text: Option<String>,
    inbox: Option<bool>,
    attachments: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
struct WakeBody {
    target: Option<String>,
    task: Option<String>,
}

#[derive(Default, Deserialize)]
struct FeedQuery {
    limit: Option<usize>,
}

#[derive(Default)]
struct RequestReplyStore {
    entries: HashMap<String, RequestEntry>,
    next_id: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestEntry {
    correlation_id: String,
    from: String,
    to: String,
    target: String,
    message: String,
    status: String,
    reply: Option<String>,
    data: Option<Value>,
}

enum ReplyResult {
    Ok,
    NotFound,
    AlreadyReplied,
}

impl RequestReplyStore {
    fn create(&mut self, body: RequestCreateBody) -> RequestEntry {
        self.next_id = self.next_id.saturating_add(1);
        let correlation_id = format!("req-{}", self.next_id);
        let to = body.to.split(':').next().unwrap_or(&body.to).to_owned();
        let entry = RequestEntry {
            correlation_id: correlation_id.clone(),
            from: body.from.unwrap_or_else(|| "external".to_owned()),
            to,
            target: body.to,
            message: body.message,
            status: "delivered".to_owned(),
            reply: None,
            data: None,
        };
        self.entries.insert(correlation_id, entry.clone());
        entry
    }

    fn list(&self, oracle: Option<&str>, status: Option<&str>) -> Vec<RequestEntry> {
        let mut entries = self.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.correlation_id.cmp(&b.correlation_id));
        entries
            .into_iter()
            .filter(|entry| oracle.is_none_or(|oracle| entry.to == oracle))
            .filter(|entry| status.is_none_or(|status| entry.status == status))
            .collect()
    }

    fn reply(&mut self, correlation_id: &str, reply: String, data: Option<Value>) -> ReplyResult {
        let Some(entry) = self.entries.get_mut(correlation_id) else {
            return ReplyResult::NotFound;
        };
        if entry.status == "replied" {
            return ReplyResult::AlreadyReplied;
        }
        "replied".clone_into(&mut entry.status);
        entry.reply = Some(reply);
        entry.data = data;
        ReplyResult::Ok
    }
}

fn with_request_store<T>(state: &ServeState, f: impl FnOnce(&mut RequestReplyStore) -> T) -> T {
    match state.requests.lock() {
        Ok(mut store) => f(&mut store),
        Err(poisoned) => {
            let mut store = poisoned.into_inner();
            f(&mut store)
        }
    }
}

#[derive(Deserialize)]
struct MessageLedgerQuery {
    limit: Option<usize>,
    from: Option<String>,
    to: Option<String>,
    direction: Option<String>,
    state: Option<String>,
    q: Option<String>,
    json: Option<String>,
}

#[derive(Deserialize)]
struct RequestListQuery {
    oracle: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustAddBody {
    sender: String,
    target: String,
    peer_key: String,
}

#[derive(Deserialize)]
struct TrustRevokeBody {
    sender: String,
    target: String,
    yes: Option<bool>,
}

#[derive(Default, Deserialize)]
struct RequestCreateBody {
    to: String,
    message: String,
    from: Option<String>,
}

#[derive(Deserialize)]
struct ReplyBody {
    reply: String,
    data: Option<Value>,
}










#[derive(Deserialize)]
struct SessionsQuery {
    local: Option<bool>,
}

#[derive(Deserialize)]
struct CaptureQuery {
    target: Option<String>,
}
