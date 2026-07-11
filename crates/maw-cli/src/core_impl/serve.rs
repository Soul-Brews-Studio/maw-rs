use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
#[cfg(test)]
use std::net::Ipv4Addr;

const DEFAULT_SERVE_PORT: u16 = 3456;
const DEFAULT_SERVE_BIND: &str = "0.0.0.0";
const SERVE_FEED_MAX: usize = 200;
const SERVE_LOG_TEXT_MAX: usize = 2_000;
const SERVE_LOG_ERROR_MAX: usize = 1_000;
const DELIVERY_IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;
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
}

#[derive(Debug, Clone)]
struct ServePluginRoute {
    name: String,
    command: Option<String>,
    prefix: String,
    health_path: String,
    events: Vec<String>,
    event_path: Option<String>,
    dir: PathBuf,
    process: Arc<Mutex<Option<ServePluginProcess>>>,
}

#[derive(Debug)]
struct ServePluginProcess { port: u16, child: Child }

impl Drop for ServePluginProcess {
    fn drop(&mut self) { let _ = self.child.kill(); }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ServeApiTokenAuth {
    token: Option<String>,
    loopback_exempt: bool,
    forced_open: bool,
}

impl ServeApiTokenAuth {
    #[cfg(test)]
    fn open() -> Self { Self { token: None, loopback_exempt: true, forced_open: true } }

    fn mode_label(&self) -> &'static str {
        if self.forced_open { "open (configured)" } else if self.token.is_some() { "token" } else { "open" }
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
        peer_pubkeys: load_inbound_peer_pubkeys(),
        workspace_key: load_serve_workspace_key(),
        workspaces: Mutex::new(WorkspaceStore::default()),
        requests: Mutex::new(RequestReplyStore::default()),
        delivery: Arc::new(ServeSystemDelivery),
        receiver_inbox: Arc::new(ServeSystemReceiverInbox::default()),
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
    });
    println!("maw-rs serve listening http://{local_addr}");
    println!("maw-rs serve auth: {}", api_token_auth.mode_label());
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
    let core = crate::serve_core::ServecoreSharedState::default()
        .servecore_with_engine(Arc::new(crate::serve_core::ServecoreNativeEngine))
        .servecore_with_agents_node(load_hey_config().node)
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
    let serve_core_state = serve_core_state(&state);
    let plugin_serve_routes = state.plugin_serve_routes.clone();
    let state = Arc::new(state);
    let router = Router::new();
    let router = crate::serve_core::servecore_mount_core_routes(router);
    let router = crate::serve_core::modules::servecore_mount_modules(router, &[]);
    let router = serve_mount_plugin_routes(router, &plugin_serve_routes);
    let router = router
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
        state.api_token_auth.clone(),
        serve_api_token_gate,
    ));
    let router = crate::serve_core::servecore_with_shared_state(router, serve_core_state);
    router.with_state(state)
}

async fn serve_api_token_gate(
    State(auth): State<ServeApiTokenAuth>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") || path == "/api/health" || auth.forced_open || auth.token.is_none() {
        return next.run(req).await;
    }
    if auth.loopback_exempt {
        if let Some(ConnectInfo(peer)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
            if maw_auth::is_loopback(Some(&peer.ip().to_string())) {
                return next.run(req).await;
            }
        }
    }
    if auth.token_matches(req.headers()) {
        return next.run(req).await;
    }
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized", "auth": "maw-serve-token"}))).into_response()
}

fn serve_discover_plugin_routes() -> Vec<ServePluginRoute> {
    maw_plugin_manifest::discover_packages(&maw_plugin_manifest::DiscoverPackagesOptions::default())
        .plugins
        .into_iter()
        .filter_map(|plugin| {
            let serve = plugin.manifest.engine?.serve?;
            let name = plugin.manifest.name;
            let Some(prefix) = serve.prefix else {
                eprintln!("maw serve: skipping plugin {name}: engine.serve.prefix missing");
                return None;
            };
            let Some(health_path) =
                serve_join_plugin_path(&prefix, serve.health.as_deref().unwrap_or("/health"))
            else {
                eprintln!("maw serve: skipping plugin {name}: invalid engine.serve health path");
                return None;
            };
            let event_path = match serve.event_path.as_deref() {
                Some(path) => {
                    let Some(joined) = serve_join_plugin_path(&prefix, path) else {
                        eprintln!("maw serve: skipping plugin {name}: invalid engine.serve eventPath");
                        return None;
                    };
                    Some(joined)
                },
                None => None,
            };
            Some(ServePluginRoute {
                name,
                command: serve.command,
                prefix,
                health_path,
                events: serve.events.unwrap_or_default(),
                event_path,
                dir: plugin.dir,
                process: Arc::new(Mutex::new(None)),
            })
        })
        .filter(|route| {
            let collides = serve_plugin_route_collides(route);
            if collides {
                eprintln!(
                    "maw serve: skipping plugin {}: engine.serve prefix {} collides with core route",
                    route.name, route.prefix
                );
            }
            !collides
        })
        .collect()
}

fn serve_join_plugin_path(prefix: &str, path: &str) -> Option<String> {
    if !prefix.starts_with("/api/") || !path.starts_with('/') {
        return None;
    }
    Some(format!("{}{}", prefix.trim_end_matches('/'), path))
}

fn serve_plugin_route_collides(route: &ServePluginRoute) -> bool {
    const CORE_PREFIXES: &[&str] = &[
        "/api/agents", "/api/capture", "/api/feed", "/api/health", "/api/message-ledger",
        "/api/orchestration", "/api/plugins", "/api/probe", "/api/requests", "/api/reply",
        "/api/request", "/api/send", "/api/serve-core", "/api/sessions", "/api/transport",
        "/api/triggers", "/api/trust", "/api/wake", "/api/workspace", "/api/worktrees",
    ];
    CORE_PREFIXES
        .iter()
        .any(|core| route.prefix == *core || route.prefix.starts_with(&format!("{core}/")))
}

fn serve_mount_plugin_routes(
    mut router: Router<Arc<ServeState>>,
    plugin_routes: &[ServePluginRoute],
) -> Router<Arc<ServeState>> {
    for route in plugin_routes {
        if route.command.is_some() {
            router = router
                .route(&route.prefix, any(api_plugin_serve_proxy))
                .route(&format!("{}/*path", route.prefix), any(api_plugin_serve_proxy));
        }
        router = router.route(&route.health_path, get(api_plugin_serve_health));
        if let Some(event_path) = &route.event_path {
            router = router.route(event_path, get(api_plugin_serve_events));
        }
    }
    router
}

async fn api_plugin_serve_health(
    State(state): State<Arc<ServeState>>,
    uri: Uri,
) -> Response {
    let Some(route) = serve_plugin_route_for_path(&state, uri.path()) else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "plugin route not found"}))).into_response();
    };
    if route.command.is_some() && serve_plugin_process_running(route) {
        return serve_proxy_to_plugin(route, Method::GET, &uri, HeaderMap::new(), Bytes::new()).await;
    }
    (StatusCode::OK, Json(json!({"ok": true, "plugin": route.name, "prefix": route.prefix, "command": route.command, "health": route.health_path}))).into_response()
}

async fn api_plugin_serve_proxy(
    State(state): State<Arc<ServeState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(route) = serve_plugin_route_for_prefix(&state, uri.path()) else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "plugin route not found"}))).into_response();
    };
    serve_proxy_to_plugin(route, method, &uri, headers, body).await
}

async fn api_plugin_serve_events(
    State(state): State<Arc<ServeState>>,
    uri: Uri,
) -> impl IntoResponse {
    serve_plugin_route_for_path(&state, uri.path()).map_or_else(
        || (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "plugin route not found"}))),
        |route| (StatusCode::OK, Json(json!({"ok": true, "plugin": route.name, "events": route.events}))),
    )
}

fn serve_plugin_route_for_path<'a>(state: &'a ServeState, path: &str) -> Option<&'a ServePluginRoute> {
    state
        .plugin_serve_routes
        .iter()
        .find(|route| route.health_path == path || route.event_path.as_deref() == Some(path))
}

fn serve_plugin_route_for_prefix<'a>(state: &'a ServeState, path: &str) -> Option<&'a ServePluginRoute> {
    state.plugin_serve_routes.iter().filter(|route| route.command.is_some()).find(|route| path == route.prefix || path.starts_with(&format!("{}/", route.prefix)))
}

fn serve_plugin_process_running(route: &ServePluginRoute) -> bool {
    let mut guard = route.process.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(process) = guard.as_mut() else { return false; };
    if let Ok(None) = process.child.try_wait() { return true; }
    *guard = None;
    false
}

fn serve_plugin_process_port(route: &ServePluginRoute) -> Result<u16, String> {
    {
        let mut guard = route.process.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(process) = guard.as_mut() {
            if process.child.try_wait().map_err(|error| error.to_string())?.is_none() { return Ok(process.port); }
            *guard = None;
        }
    }
    let port = serve_allocate_loopback_port()?;
    let command = route.command.as_deref().ok_or_else(|| "missing engine.serve.command".to_owned())?;
    let argv = command.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let (program, command_args) = argv.split_first().ok_or_else(|| "empty engine.serve.command".to_owned())?;
    let child = Command::new(program).args(command_args).current_dir(&route.dir)
        .env("PORT", port.to_string()).env("MAW_ENGINE_SERVE_PORT", port.to_string()).env("MAW_ENGINE_SERVE_PREFIX", &route.prefix)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .map_err(|error| format!("spawn {}: {error}", route.name))?;
    let mut guard = route.process.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(ServePluginProcess { port, child });
    serve_wait_loopback_port(port);
    Ok(port)
}

fn serve_allocate_loopback_port() -> Result<u16, String> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).and_then(|listener| listener.local_addr()).map(|addr| addr.port()).map_err(|error| format!("allocate plugin port: {error}"))
}

fn serve_wait_loopback_port(port: u16) {
    for _ in 0..20 {
        if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() { return; }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

async fn serve_proxy_to_plugin(route: &ServePluginRoute, method: Method, uri: &Uri, headers: HeaderMap, body: Bytes) -> Response {
    let port = match serve_plugin_process_port(route) {
        Ok(port) => port,
        Err(error) => return (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": error, "plugin": route.name}))).into_response(),
    };
    let target = format!("http://127.0.0.1:{port}{}", uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string));
    let req_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(error) => return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": format!("bad method: {error}")}))).into_response(),
    };
    let client = reqwest::Client::new();
    let mut request = client.request(req_method, target).body(body.to_vec());
    if let Some(content_type) = headers.get(axum::http::header::CONTENT_TYPE) {
        request = request.header(reqwest::header::CONTENT_TYPE, content_type.as_bytes());
    }
    match request.send().await {
        Ok(response) => serve_proxy_response(response).await,
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": format!("proxy failed: {error}"), "plugin": route.name}))).into_response(),
    }
}

async fn serve_proxy_response(response: reqwest::Response) -> Response {
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let bytes = response.bytes().await.unwrap_or_default();
    let mut out = (status, Body::from(bytes)).into_response();
    if let Some(value) = content_type.and_then(|value| HeaderValue::from_bytes(value.as_bytes()).ok()) {
        out.headers_mut().insert(axum::http::header::CONTENT_TYPE, value);
    }
    out
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

enum ServeInboxIdempotencyClaim {
    Claimed(Option<DeliveryIdempotencyKey>),
    Duplicate(Box<axum::response::Response>),
}

fn serve_claim_inbox_idempotency(
    state: &ServeState,
    headers: &HeaderMap,
    parsed: &SendBody,
    resolved: &str,
    context: &ServeInboxContext<'_>,
) -> ServeInboxIdempotencyClaim {
    let idempotency_key =
        serve_delivery_idempotency_key(headers, context.log_from, resolved, context.message);
    let Some(key) = idempotency_key.clone() else {
        return ServeInboxIdempotencyClaim::Claimed(None);
    };
    match serve_delivery_idempotency_claim(state, key.clone(), serve_delivery_idempotency_now(state)) {
        DeliveryIdempotencyClaim::Claimed => ServeInboxIdempotencyClaim::Claimed(idempotency_key),
        DeliveryIdempotencyClaim::Duplicate(record) => {
            serve_log_delivery_deduped(
                state,
                &key,
                resolved,
                context.message,
                context.log_from,
                context.log_to,
                "inbox",
            );
            ServeInboxIdempotencyClaim::Duplicate(Box::new(serve_delivery_idempotency_response(
                &record,
                resolved,
                &parsed.text.clone().unwrap_or_default(),
                "inbox",
            )))
        }
    }
}

#[derive(Clone, Copy)]
struct ReceiverInboxInput<'a> {
    query: &'a str,
    target: Option<&'a str>,
    to: Option<&'a str>,
    from: &'a str,
    message: &'a str,
    config: &'a HeyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiverInboxOk {
    oracle: String,
    inbox_dir: std::path::PathBuf,
    path: std::path::PathBuf,
    filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReceiverInboxResult {
    Ok(ReceiverInboxOk),
    Err { oracle: Option<String>, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeliveryIdempotencyKey {
    source: String,
    target: String,
    payload_hash: String,
    logical_ts: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeliveryIdempotencyRecord {
    InFlight { seen_at: i64 },
    Complete {
        target: String,
        state: String,
        seen_at: i64,
    },
}

impl DeliveryIdempotencyRecord {
    fn response_state(&self) -> &str {
        match self {
            Self::InFlight { .. } => "queued",
            Self::Complete { state, .. } => state,
        }
    }

    fn response_target<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::InFlight { .. } => fallback,
            Self::Complete { target, .. } => target,
        }
    }

    const fn seen_at(&self) -> i64 {
        match self {
            Self::InFlight { seen_at } | Self::Complete { seen_at, .. } => *seen_at,
        }
    }
}

#[derive(Default)]
struct DeliveryIdempotencyStore {
    records: HashMap<DeliveryIdempotencyKey, DeliveryIdempotencyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeliveryIdempotencyClaim {
    Claimed,
    Duplicate(DeliveryIdempotencyRecord),
}

fn serve_delivery_idempotency_key(
    headers: &HeaderMap,
    fallback_source: &str,
    target: &str,
    payload: &str,
) -> Option<DeliveryIdempotencyKey> {
    let logical_ts = serve_delivery_logical_ts(headers)?;
    let raw_source = header_to_string(headers, "x-maw-from");
    let source = raw_source.trim();
    let source = if source.is_empty() { fallback_source.trim() } else { source };
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    let payload_hash = maw_auth::hash_body(Some(payload.as_bytes()));
    if payload_hash.is_empty() {
        return None;
    }
    Some(DeliveryIdempotencyKey {
        source: source.to_owned(),
        target: target.to_owned(),
        payload_hash,
        logical_ts,
    })
}

fn serve_delivery_logical_ts(headers: &HeaderMap) -> Option<String> {
    ["x-maw-timestamp", "x-maw-signed-at"]
        .into_iter()
        .map(|name| header_to_string(headers, name))
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn serve_delivery_idempotency_claim(
    state: &ServeState,
    key: DeliveryIdempotencyKey,
    now: i64,
) -> DeliveryIdempotencyClaim {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_delivery_idempotency_prune(&mut store, now);
    if let Some(record) = store.records.get(&key).cloned() {
        return DeliveryIdempotencyClaim::Duplicate(record);
    }
    store
        .records
        .insert(key, DeliveryIdempotencyRecord::InFlight { seen_at: now });
    DeliveryIdempotencyClaim::Claimed
}

fn serve_delivery_idempotency_complete(
    state: &ServeState,
    key: DeliveryIdempotencyKey,
    target: &str,
    state_name: &str,
    now: i64,
) {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.records.insert(
        key,
        DeliveryIdempotencyRecord::Complete {
            target: target.to_owned(),
            state: state_name.to_owned(),
            seen_at: now,
        },
    );
}

fn serve_delivery_idempotency_cancel(state: &ServeState, key: &DeliveryIdempotencyKey) {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if matches!(store.records.get(key), Some(DeliveryIdempotencyRecord::InFlight { .. })) {
        store.records.remove(key);
    }
}

fn serve_delivery_idempotency_prune(store: &mut DeliveryIdempotencyStore, now: i64) {
    store.records.retain(|_, record| {
        let age = now.saturating_sub(record.seen_at());
        age <= DELIVERY_IDEMPOTENCY_TTL_SECONDS
    });
}

fn serve_delivery_idempotency_now(state: &ServeState) -> i64 {
    #[cfg(test)]
    {
        state
            .now_override
            .unwrap_or_else(|| i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX))
    }
    #[cfg(not(test))]
    {
        let _ = state;
        i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX)
    }
}

fn serve_delivery_idempotency_response(
    record: &DeliveryIdempotencyRecord,
    fallback_target: &str,
    text: &str,
    source: &str,
) -> axum::response::Response {
    let target = record.response_target(fallback_target);
    let state_name = record.response_state();
    Json(json!({
        "ok": true,
        "target": target,
        "text": text,
        "source": source,
        "state": state_name,
        "deduped": true,
        "idempotent": true,
        "reason": "duplicate delivery dropped by idempotency key",
        "receipt": ["duplicate_dropped"],
        "lastLine": "duplicate delivery dropped by idempotency key",
    }))
    .into_response()
}

fn serve_log_delivery_deduped(
    state: &ServeState,
    key: &DeliveryIdempotencyKey,
    target: &str,
    message: &str,
    from: &str,
    to: &str,
    route: &str,
) {
    serve_log_lifecycle(
        state,
        json!({
            "kind": "context.message",
            "direction": "inbound",
            "state": "deduped",
            "route": route,
            "from": serve_truncate(from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(to, SERVE_LOG_TEXT_MAX),
            "target": target,
            "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(target),
            "source": "maw-rs-native",
            "idempotency": {
                "source": &key.source,
                "target": &key.target,
                "payloadHash": &key.payload_hash,
                "logicalTs": &key.logical_ts,
            },
        }),
    );
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
    Json(json!({
        "ok": true,
        "target": context.resolved,
        "text": context.message,
        "source": "maw-rs",
        "state": state_name,
        "lastLine": last_line,
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

fn receiver_inbox_explicit_enabled(value: Option<std::ffi::OsString>) -> Option<bool> {
    let value = value?.to_string_lossy().trim().to_ascii_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn receiver_inbox_auto_write_enabled() -> bool {
    if let Some(enabled) = receiver_inbox_explicit_enabled(std::env::var_os("MAW_HEY_INBOX_AUTOWRITE")) {
        return enabled;
    }
    std::env::var("MAW_TEST_MODE").ok().as_deref() != Some("1")
}

fn receiver_inbox_now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn receiver_inbox_iso_from_millis(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1_000).unwrap_or(i64::MAX);
    let ms = u32::try_from(millis % 1_000).unwrap_or(999);
    let (year, month, day, hour, minute, second) = unix_seconds_to_utc(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z")
}

fn receiver_inbox_strip_pane_suffix(value: &str) -> &str {
    let Some((prefix, suffix)) = value.rsplit_once('.') else {
        return value;
    };
    if suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        prefix
    } else {
        value
    }
}

fn receiver_inbox_basename(value: &str) -> &str {
    std::path::Path::new(value)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(value)
}

fn receiver_inbox_normalize_oracle_name(raw: Option<&str>) -> Option<String> {
    let mut value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let colon_value;
    if value.contains(':') {
        let parts = value.split(':').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        colon_value = if parts.len() >= 3 {
            parts[2]
        } else {
            parts.get(1).copied().or_else(|| parts.first().copied()).unwrap_or(value)
        };
        value = colon_value;
    }
    value = receiver_inbox_strip_pane_suffix(value);
    value = receiver_inbox_basename(value);
    if let Some(stripped) = value.strip_suffix("-oracle") {
        value = stripped;
    }
    let trimmed_numeric = value
        .split_once('-')
        .and_then(|(prefix, rest)| prefix.bytes().all(|byte| byte.is_ascii_digit()).then_some(rest))
        .unwrap_or(value);
    (!trimmed_numeric.is_empty()).then(|| trimmed_numeric.to_owned())
}

fn receiver_inbox_resolve_oracle(input: &ReceiverInboxInput<'_>) -> Option<String> {
    receiver_inbox_normalize_oracle_name(input.to)
        .or_else(|| receiver_inbox_normalize_oracle_name(input.target))
        .or_else(|| receiver_inbox_normalize_oracle_name(Some(input.query)))
}

fn receiver_inbox_safe_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-');
        if safe {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-').chars().take(64).collect::<String>();
    if out.is_empty() { "unknown".to_owned() } else { out }
}

fn receiver_inbox_slugify_body(body: &str) -> String {
    receiver_inbox_safe_segment(&body.split_whitespace().take(6).collect::<Vec<_>>().join("-").to_ascii_lowercase())
        .chars()
        .take(48)
        .collect()
}

fn receiver_inbox_body(from: &str, to: &str, timestamp: &str, message: &str) -> String {
    [
        "---".to_owned(),
        format!("from: {from}"),
        format!("to: {to}"),
        format!("timestamp: {timestamp}"),
        "read: false".to_owned(),
        "---".to_owned(),
        String::new(),
        message.to_owned(),
        String::new(),
    ]
    .join("\n")
}

fn receiver_inbox_filename_with_collision_suffix(base: &str, attempt: usize) -> String {
    if attempt <= 1 {
        return base.to_owned();
    }
    base.strip_suffix(".md")
        .map_or_else(|| format!("{base}-{attempt}"), |prefix| format!("{prefix}-{attempt}.md"))
}

fn receiver_inbox_strip_psi_suffix(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.display().to_string();
    let stripped = text.trim_end_matches('/');
    if let Some(prefix) = stripped.strip_suffix("/ψ").or_else(|| stripped.strip_suffix("/psi")) {
        std::path::PathBuf::from(prefix)
    } else {
        std::path::PathBuf::from(stripped)
    }
}

fn receiver_inbox_config_psi_path() -> Option<std::path::PathBuf> {
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    value
        .get("psiPath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

fn receiver_inbox_ghq_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        std::path::PathBuf::from,
    )
}

fn receiver_inbox_target_cwd_parts(target: &str) -> Option<(&str, Option<&str>)> {
    let clean = receiver_inbox_strip_pane_suffix(target.trim());
    if clean.is_empty() {
        return None;
    }
    let parts = clean.split(':').collect::<Vec<_>>();
    let (session, window) = if parts.len() >= 3 {
        (parts.get(1).copied().unwrap_or_default(), parts.get(2).copied())
    } else {
        (parts.first().copied().unwrap_or_default(), parts.get(1).copied())
    };
    let session = session.trim();
    if session.is_empty() {
        return None;
    }
    Some((session, window.map(str::trim).filter(|value| !value.is_empty())))
}

fn receiver_inbox_target_cwd_window<'a>(
    fleet: &'a NativeFleetSession,
    win_ref: Option<&str>,
) -> Option<&'a NativeFleetWindow> {
    let Some(win_ref) = win_ref else {
        return fleet.windows.first();
    };
    if win_ref.bytes().all(|byte| byte.is_ascii_digit()) {
        return win_ref
            .parse::<usize>()
            .ok()
            .and_then(|index| fleet.windows.get(index));
    }
    fleet.windows.iter().find(|window| window.name == win_ref)
}

fn receiver_inbox_resolve_target_cwd(target: &str) -> Result<Option<std::path::PathBuf>, String> {
    let Some((session, win_ref)) = receiver_inbox_target_cwd_parts(target) else {
        return Ok(None);
    };
    let ghq_root = receiver_inbox_ghq_root();
    let mut candidates = Vec::new();
    for fleet in load_native_fleet().into_iter().filter(|fleet| fleet.name == session) {
        let Some(window) = receiver_inbox_target_cwd_window(&fleet, win_ref) else {
            continue;
        };
        let repo = window.repo.trim();
        if repo.is_empty() {
            continue;
        }
        candidates.push(ghq_root.join(repo));
    }
    let candidates = receiver_inbox_existing_candidates(candidates);
    if candidates.len() > 1 {
        return Err(format!("receiver repo ambiguous for {target}"));
    }
    Ok(candidates.into_iter().next())
}

fn receiver_inbox_lookup_key(value: &str) -> Option<String> {
    let value = receiver_inbox_strip_pane_suffix(value.trim()).trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn receiver_inbox_add_target_lookup_keys(keys: &mut BTreeSet<String>, raw: Option<&str>) {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let raw = receiver_inbox_strip_pane_suffix(raw);
    if let Some(key) = receiver_inbox_lookup_key(raw) {
        keys.insert(key);
    }
    let parts = raw
        .split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [session, window] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        [_, session, window, ..] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        _ => {}
    }
}

fn receiver_inbox_target_lookup_keys(input: &ReceiverInboxInput<'_>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    receiver_inbox_add_target_lookup_keys(&mut keys, input.target);
    receiver_inbox_add_target_lookup_keys(&mut keys, input.to);
    receiver_inbox_add_target_lookup_keys(&mut keys, Some(input.query));
    keys
}

fn receiver_inbox_manifest_entry_matches_target(
    entry: &LocateManifestEntry,
    target_keys: &BTreeSet<String>,
) -> bool {
    entry
        .session
        .as_deref()
        .and_then(receiver_inbox_lookup_key)
        .is_some_and(|key| target_keys.contains(&key))
        || entry
            .window
            .as_deref()
            .and_then(receiver_inbox_lookup_key)
            .is_some_and(|key| target_keys.contains(&key))
}

fn receiver_inbox_push_manifest_entry_candidates(
    candidates: &mut Vec<std::path::PathBuf>,
    entry: &LocateManifestEntry,
) {
    if let Some(local_path) = entry.local_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        candidates.push(std::path::PathBuf::from(local_path));
    }
    if let Some(repo) = entry.repo.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        let ghq_root = receiver_inbox_ghq_root();
        candidates.push(ghq_root.join("github.com").join(repo));
        candidates.push(ghq_root.join(repo));
    }
}

fn receiver_inbox_existing_candidates(
    candidates: Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.display().to_string()))
        .filter(|candidate| candidate.exists())
        .collect()
}

fn receiver_inbox_repo_candidates(
    oracle: &str,
    input: &ReceiverInboxInput<'_>,
    psi_root: Option<&std::path::Path>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let mut candidates = Vec::new();
    if let Some(psi_path) = psi_root {
        candidates.push(receiver_inbox_strip_psi_suffix(psi_path));
    } else if let (Some(psi_path), Some(config_oracle)) =
        (receiver_inbox_config_psi_path(), input.config.oracle.as_deref())
    {
        if receiver_inbox_normalize_oracle_name(Some(config_oracle)).as_deref() == Some(oracle) {
            candidates.push(receiver_inbox_strip_psi_suffix(&psi_path));
        }
    }
    if let Some(target) = input.target {
        match receiver_inbox_resolve_target_cwd(target) {
            Ok(Some(path)) => candidates.push(path),
            Ok(None) => {}
            Err(reason) => return Err(reason),
        }
    }
    let manifest = locate_load_manifest();
    if let Some(entry) = manifest.iter().find(|entry| {
        receiver_inbox_normalize_oracle_name(Some(&entry.name)).as_deref() == Some(oracle)
            || entry.window.as_deref().and_then(|window| receiver_inbox_normalize_oracle_name(Some(window))).as_deref()
                == Some(oracle)
    }) {
        receiver_inbox_push_manifest_entry_candidates(&mut candidates, entry);
    }

    let target_keys = receiver_inbox_target_lookup_keys(input);
    if !target_keys.is_empty() {
        let mut phase_b = Vec::new();
        for entry in manifest
            .iter()
            .filter(|entry| receiver_inbox_manifest_entry_matches_target(entry, &target_keys))
        {
            let mut entry_candidates = Vec::new();
            receiver_inbox_push_manifest_entry_candidates(&mut entry_candidates, entry);
            phase_b.extend(receiver_inbox_existing_candidates(entry_candidates));
        }
        let phase_b = receiver_inbox_existing_candidates(phase_b);
        if phase_b.len() > 1 {
            return Err(format!("receiver repo ambiguous for {}", input.query));
        }
        candidates.extend(phase_b);
    }
    Ok(receiver_inbox_existing_candidates(candidates))
}

fn persist_receiver_inbox(
    input: ReceiverInboxInput<'_>,
    now_millis: u128,
    psi_root: Option<&std::path::Path>,
) -> ReceiverInboxResult {
    let Some(oracle) = receiver_inbox_resolve_oracle(&input) else {
        return ReceiverInboxResult::Err { oracle: None, reason: "receiver oracle could not be inferred".to_owned() };
    };
    let repo_candidates = match receiver_inbox_repo_candidates(&oracle, &input, psi_root) {
        Ok(candidates) => candidates,
        Err(reason) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason },
    };
    let Some(repo_path) = repo_candidates.into_iter().next() else {
        return ReceiverInboxResult::Err {
            oracle: Some(oracle.clone()),
            reason: format!("receiver repo not found for {oracle}"),
        };
    };
    let timestamp = receiver_inbox_iso_from_millis(now_millis);
    let date_part = &timestamp[..10];
    let time_part = timestamp[11..16].replace(':', "-");
    let base_filename = format!(
        "{date_part}_{time_part}_{}_{}.md",
        receiver_inbox_safe_segment(input.from),
        receiver_inbox_slugify_body(input.message)
    );
    let inbox_dir = repo_path.join("ψ").join("inbox");
    let body = receiver_inbox_body(input.from, &oracle, &timestamp, input.message);
    if let Err(error) = std::fs::create_dir_all(&inbox_dir) {
        return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
    }
    for attempt in 1..=1000 {
        let filename = receiver_inbox_filename_with_collision_suffix(&base_filename, attempt);
        let path = inbox_dir.join(&filename);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = std::io::Write::write_all(&mut file, body.as_bytes()) {
                    return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
                }
                return ReceiverInboxResult::Ok(ReceiverInboxOk { oracle, inbox_dir, path, filename });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() },
        }
    }
    ReceiverInboxResult::Err {
        oracle: Some(oracle),
        reason: format!("receiver inbox filename collision limit reached for {base_filename}"),
    }
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
        Json(json!({"ok": true})).into_response()
    }
}

async fn api_sessions(Query(query): Query<SessionsQuery>) -> impl IntoResponse {
    let _local = query.local.unwrap_or(false);
    let mut tmux = TmuxClient::local();
    let panes = tmux.list_panes();
    let sessions = tmux
        .list_all()
        .into_iter()
        .map(|session| serve_tmux_session_json(&session, &panes))
        .collect::<Vec<_>>();
    Json(sessions)
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
    let sessions = route_sessions_from_tmux(tmux);
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
        response
    } else {
        Json(json!({"ok": true})).into_response()
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

async fn api_health() -> impl IntoResponse {
    Json(json!({"ok": true, "source": "maw-rs", "server": "local", "port": DEFAULT_SERVE_PORT}))
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

async fn api_workspace_create(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<WorkspaceCreateBody>,
) -> impl IntoResponse {
    let workspace = Workspace::new(body.name, body.node_id);
    let response = json!({
        "id": workspace.id,
        "token": workspace.token,
        "joinCode": workspace.join_code,
        "joinCodeExpiresAt": workspace.join_code_expires_at,
    });
    with_workspace_store(&state, |store| {
        store.join_codes.insert(workspace.join_code.clone(), workspace.id.clone());
        store.workspaces.insert(workspace.id.clone(), workspace);
    });
    Json(response).into_response()
}

async fn api_workspace_join(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<WorkspaceJoinBody>,
) -> impl IntoResponse {
    with_workspace_store(&state, |store| {
        let Some(workspace_id) = store.join_codes.get(&body.code).cloned() else {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response();
        };
        let Some(workspace) = store.workspaces.get_mut(&workspace_id) else {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response();
        };
        workspace.nodes.insert(body.node_id);
        Json(json!({
            "workspaceId": workspace.id,
            "token": workspace.token,
            "name": workspace.name,
        }))
        .into_response()
    })
}

async fn api_workspace_agents_post(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    let agent = serde_json::from_slice::<WorkspaceAgentBody>(&body).unwrap_or_default();
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get_mut(&id) else {
            return workspace_not_found();
        };
        if !agent.node_id.is_empty() {
            workspace.nodes.insert(agent.node_id.clone());
        }
        if !agent.name.is_empty() {
            workspace.agents.insert(
                agent_key(&agent.node_id, &agent.name),
                WorkspaceAgent {
                    name: agent.name,
                    node_id: agent.node_id,
                    status: agent.status,
                    capabilities: agent.capabilities,
                },
            );
        }
        Json(json!({"ok": true, "agents": workspace.agents.len()})).into_response()
    })
}

async fn api_workspace_agents_get(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        let agents = workspace
            .agents
            .values()
            .map(|agent| {
                json!({
                    "name": agent.name,
                    "nodeId": agent.node_id,
                    "status": agent.status,
                    "capabilities": agent.capabilities,
                })
            })
            .collect::<Vec<_>>();
        Json(json!({"agents": agents, "total": workspace.agents.len()})).into_response()
    })
}

async fn api_workspace_status(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        Json(json!({
            "id": workspace.id,
            "name": workspace.name,
            "createdAt": workspace.created_at,
            "nodes": workspace.nodes.iter().cloned().collect::<Vec<_>>(),
            "nodeCount": workspace.nodes.len(),
            "healthyNodes": workspace.nodes.len(),
            "agents": workspace.agents.values().map(|agent| json!({"name": agent.name, "nodeId": agent.node_id, "status": agent.status, "capabilities": agent.capabilities})).collect::<Vec<_>>(),
            "agentCount": workspace.agents.len(),
            "feedCount": workspace.feed.len(),
        }))
        .into_response()
    })
}

async fn api_workspace_feed(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorkspaceFeedQuery>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        let limit = query.limit.unwrap_or(workspace.feed.len());
        let start = workspace.feed.len().saturating_sub(limit);
        Json(json!({"events": workspace.feed[start..].to_vec(), "total": workspace.feed.len()}))
            .into_response()
    })
}

async fn api_workspace_message(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    let message = serde_json::from_slice::<WorkspaceMessageBody>(&body).unwrap_or_default();
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get_mut(&id) else {
            return workspace_not_found();
        };
        workspace.feed.push(json!({
            "from": message.from,
            "text": message.text,
            "to": message.to,
            "timestamp": unix_seconds(),
        }));
        Json(json!({"ok": true})).into_response()
    })
}

async fn api_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"})))
}

fn verify_protected_request(
    state: &ServeState,
    peer: SocketAddr,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> Option<axum::response::Response> {
    match verify_protected_request_outcome(state, peer, method, uri, headers, body) {
        ProtectedRequestOutcome::Accept => None,
        ProtectedRequestOutcome::Reject { response, .. } => Some(response),
    }
}

enum ProtectedRequestOutcome {
    Accept,
    Reject {
        decision: String,
        response: axum::response::Response,
    },
}

fn verify_protected_request_outcome(
    state: &ServeState,
    peer: SocketAddr,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> ProtectedRequestOutcome {
    let effective_peer = effective_peer_addr(state, peer);
    if maw_auth::is_loopback(Some(&effective_peer.ip().to_string())) {
        return ProtectedRequestOutcome::Accept;
    }
    let now = verify_now(state);
    let auth_headers = extract_auth_headers(headers);
    let cached_pubkey = match resolve_request_cached_pubkey(state, &auth_headers) {
        Ok(pubkey) => pubkey,
        Err(decision) => {
            return ProtectedRequestOutcome::Reject {
                decision: decision.to_string(),
                response: (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "unauthorized", "decision": decision})),
                )
                    .into_response(),
            };
        }
    };
    let decision = verify_request(&VerifyRequestArgs {
        method: method.as_str().to_owned(),
        path: path_and_query(uri),
        headers: auth_headers,
        body: Some(body.to_vec()),
        cached_pubkey,
        now,
    });
    if maw_auth::is_refuse_decision(&decision) {
        let kind = decision.kind().to_owned();
        return ProtectedRequestOutcome::Reject {
            decision: kind.clone(),
            response: (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "unauthorized", "decision": kind})),
            )
                .into_response(),
        };
    }
    ProtectedRequestOutcome::Accept
}

#[cfg(test)]
fn effective_peer_addr(state: &ServeState, peer: SocketAddr) -> SocketAddr {
    state.peer_addr_override.unwrap_or(peer)
}

#[cfg(not(test))]
fn effective_peer_addr(_state: &ServeState, peer: SocketAddr) -> SocketAddr {
    peer
}

#[cfg(test)]
fn verify_now(state: &ServeState) -> i64 {
    state
        .now_override
        .unwrap_or_else(|| i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX))
}

#[cfg(not(test))]
fn verify_now(_state: &ServeState) -> i64 {
    i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX)
}

fn extract_auth_headers(headers: &HeaderMap) -> Headers {
    Headers::new([
        ("x-maw-from", header_to_string(headers, "x-maw-from")),
        (
            "x-maw-signature-v3",
            header_to_string(headers, "x-maw-signature-v3"),
        ),
        (
            "x-maw-timestamp",
            header_to_string(headers, "x-maw-timestamp"),
        ),
        (
            "x-maw-signed-at",
            header_to_string(headers, "x-maw-signed-at"),
        ),
        (
            "x-maw-signature",
            header_to_string(headers, "x-maw-signature"),
        ),
        (
            "x-maw-auth-version",
            header_to_string(headers, "x-maw-auth-version"),
        ),
    ])
}

fn header_to_string(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn path_and_query(uri: &Uri) -> String {
    uri.path_and_query()
        .map_or_else(|| uri.path().to_owned(), ToString::to_string)
}

fn verify_workspace_request(
    state: &ServeState,
    id: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Option<axum::response::Response> {
    with_workspace_store(state, |store| {
        let Some(workspace) = store.workspaces.get(id) else {
            return Some(workspace_not_found());
        };
        let timestamp = header_to_string(headers, "x-maw-timestamp");
        let signature = header_to_string(headers, "x-maw-signature");
        let Some(signed_at) = parse_workspace_timestamp(&timestamp) else {
            return Some(workspace_auth_failed());
        };
        let now = verify_now(state);
        if (now - signed_at).abs() > 300 {
            return Some(workspace_auth_failed());
        }
        let payload = format!("{}:{}:{}", method.as_str(), uri.path(), timestamp);
        if maw_auth::verify_hmac_sig(&workspace.token, &payload, &signature) {
            None
        } else {
            Some(workspace_auth_failed())
        }
    })
}

fn parse_workspace_timestamp(timestamp: &str) -> Option<i64> {
    if timestamp.chars().all(|ch| ch.is_ascii_digit()) {
        timestamp.parse().ok()
    } else {
        None
    }
}

fn workspace_auth_failed() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

fn workspace_not_found() -> axum::response::Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
}

fn with_workspace_store<T>(state: &ServeState, op: impl FnOnce(&mut WorkspaceStore) -> T) -> T {
    let mut guard = state
        .workspaces
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    op(&mut guard)
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

fn agent_key(node_id: &str, name: &str) -> String {
    format!("{node_id}:{name}")
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

fn load_inbound_peer_pubkeys() -> Vec<ServePeerPubkey> {
    let env = real_xdg_env();
    let mut entries = Vec::new();
    let peer_path = maw_state_path(&env, &["peers.json"]);
    if let Ok(raw) = std::fs::read_to_string(peer_path) {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            collect_peer_pubkeys(&value, None, &mut entries);
        }
    }
    let config = merged_config_value_for_env(&env);
    collect_peer_pubkeys(&config, None, &mut entries);
    entries
}

fn resolve_request_cached_pubkey(
    state: &ServeState,
    headers: &Headers,
) -> Result<Option<String>, &'static str> {
    if let Some(pubkey) = state
        .cached_pubkey
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(pubkey.to_owned()));
    }
    let Some(from) = request_from_sign_sender(headers) else {
        return Ok(None);
    };
    if let Some(entry) = state.peer_pubkeys.iter().find(|entry| entry.from == from) {
        return Ok(Some(entry.pubkey.clone()));
    }
    let Some(node) = node_from_identity(&from) else {
        return Err("refuse-missing-peer-key");
    };
    let mut node_matches = state
        .peer_pubkeys
        .iter()
        .filter(|entry| entry.node == node)
        .filter(|entry| !entry.pubkey.trim().is_empty());
    let Some(first) = node_matches.next() else {
        return Err("refuse-missing-peer-key");
    };
    if node_matches.any(|entry| entry.pubkey != first.pubkey) {
        return Err("refuse-ambiguous-peer-key");
    }
    Ok(Some(first.pubkey.clone()))
}

fn request_from_sign_sender(headers: &Headers) -> Option<String> {
    let from = headers.get("x-maw-from").unwrap_or_default().trim();
    if from.is_empty() {
        return None;
    }
    let has_v3 = !headers
        .get("x-maw-signature-v3")
        .unwrap_or_default()
        .trim()
        .is_empty()
        && !headers
            .get("x-maw-timestamp")
            .unwrap_or_default()
            .trim()
            .is_empty();
    let has_legacy = !headers
        .get("x-maw-signature")
        .unwrap_or_default()
        .trim()
        .is_empty()
        && !headers
            .get("x-maw-signed-at")
            .unwrap_or_default()
            .trim()
            .is_empty();
    (has_v3 || has_legacy).then(|| from.to_owned())
}

fn collect_peer_pubkeys(value: &Value, key_hint: Option<&str>, entries: &mut Vec<ServePeerPubkey>) {
    match value {
        Value::Object(map) => {
            if let Some(pubkey) = object_pubkey(value) {
                for from in object_from_identities(value, key_hint) {
                    if let Some(node) = node_from_normalized_identity(&from) {
                        entries.push(ServePeerPubkey {
                            from,
                            node,
                            pubkey: pubkey.clone(),
                        });
                    }
                }
            }
            for (key, child) in map {
                collect_peer_pubkeys(child, Some(key), entries);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_peer_pubkeys(item, key_hint, entries);
            }
        }
        Value::String(pubkey) => {
            if let Some(from) = key_hint.and_then(normalize_from_identity) {
                let pubkey = pubkey.trim();
                if !pubkey.is_empty() {
                    if let Some(node) = node_from_normalized_identity(&from) {
                        entries.push(ServePeerPubkey {
                            from,
                            node,
                            pubkey: pubkey.to_owned(),
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

fn object_pubkey(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    ["pubkey", "pubKey", "peerKey", "publicKey"]
        .into_iter()
        .find_map(|key| map.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn object_from_identities(value: &Value, key_hint: Option<&str>) -> Vec<String> {
    let mut identities = Vec::new();
    if let Some(from) = key_hint.and_then(normalize_from_identity) {
        identities.push(from);
    }
    if let Some(map) = value.as_object() {
        for key in ["from", "fromAddress", "sender", "identity"] {
            if let Some(from) = map
                .get(key)
                .and_then(Value::as_str)
                .and_then(normalize_from_identity)
            {
                identities.push(from);
            }
        }
        if let Some(from) = map.get("identity").and_then(identity_from_object) {
            identities.push(from);
        }
        if let (Some(oracle), Some(node)) = (
            map.get("oracle").and_then(Value::as_str),
            map.get("node").and_then(Value::as_str),
        ) {
            if let Some(from) = normalize_from_identity(&format!("{}:{}", oracle.trim(), node.trim())) {
                identities.push(from);
            }
        }
    }
    identities.sort();
    identities.dedup();
    identities
}

fn identity_from_object(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    let oracle = map.get("oracle").and_then(Value::as_str)?.trim();
    let node = map.get("node").and_then(Value::as_str)?.trim();
    normalize_from_identity(&format!("{oracle}:{node}"))
}

fn normalize_from_identity(value: &str) -> Option<String> {
    let value = value.trim();
    let (oracle, node) = value.split_once(':')?;
    let oracle = oracle.trim();
    let node = node.trim();
    if oracle.is_empty()
        || node.is_empty()
        || oracle.starts_with('-')
        || node.starts_with('-')
        || oracle.bytes().any(|byte| byte.is_ascii_control())
        || node.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    Some(format!("{oracle}:{node}"))
}

fn node_from_normalized_identity(value: &str) -> Option<String> {
    value
        .split_once(':')
        .map(|(_, node)| node)
        .filter(|node| !node.is_empty())
        .map(ToOwned::to_owned)
}

fn node_from_identity(value: &str) -> Option<String> {
    let normalized = normalize_from_identity(value)?;
    node_from_normalized_identity(&normalized)
}

#[derive(Default, Deserialize)]
struct SendBody {
    target: Option<String>,
    text: Option<String>,
    inbox: Option<bool>,
    attachments: Option<Vec<String>>,
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

#[derive(Default)]
struct WorkspaceStore {
    workspaces: HashMap<String, Workspace>,
    join_codes: HashMap<String, String>,
}

struct Workspace {
    id: String,
    name: String,
    token: String,
    join_code: String,
    join_code_expires_at: u64,
    created_at: u64,
    nodes: HashSet<String>,
    agents: HashMap<String, WorkspaceAgent>,
    feed: Vec<Value>,
}

impl Workspace {
    fn new(name: String, node_id: String) -> Self {
        let created_at = unix_millis();
        let mut nodes = HashSet::new();
        nodes.insert(node_id);
        Self {
            id: format!("ws-{}", random_hex(8)),
            name,
            token: random_hex(32),
            join_code: random_hex(3),
            join_code_expires_at: created_at.saturating_add(15 * 60 * 1_000),
            created_at,
            nodes,
            agents: HashMap::new(),
            feed: Vec::new(),
        }
    }
}

struct WorkspaceAgent {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCreateBody {
    name: String,
    node_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceJoinBody {
    code: String,
    node_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceAgentBody {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Default, Deserialize)]
struct WorkspaceMessageBody {
    from: String,
    text: String,
    to: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceFeedQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SessionsQuery {
    local: Option<bool>,
}

#[derive(Deserialize)]
struct CaptureQuery {
    target: Option<String>,
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod serve_tests {
    use super::*;
    use axum::body::Body;
    use futures_util::{SinkExt, StreamExt};
    use maw_auth::{build_legacy_from_sign_payload, hash_body, sign_headers_v3_at, sign_hmac_sig};
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    const KEY: &str = "test-peer-key-0123456789";
    const FROM: &str = "sender-oracle:sender-node";

    #[derive(Default)]
    struct FakeServeDelivery {
        sessions: Mutex<Vec<Vec<RouteSession>>>,
        sends: Mutex<Vec<(String, String)>>,
        captures: Mutex<HashMap<String, String>>,
        send_error: Mutex<Option<String>>,
        list_error: Mutex<Option<String>>,
    }

    impl FakeServeDelivery {
        fn with_capture_agent() -> Self {
            let fake = Self::default();
            fake.set_sessions(vec![vec![
                serve_test_session("capture-agent", 0, "capture-agent"),
                serve_test_session("remote-oracle", 0, "remote-oracle"),
            ]]);
            fake.set_capture("capture-agent:0", "[capture] delivered\n");
            fake.set_capture("remote-oracle:0", "[capture] delivered\n");
            fake
        }

        fn set_sessions(&self, sessions: Vec<Vec<RouteSession>>) {
            *self.sessions.lock().expect("sessions") = sessions;
        }

        fn set_capture(&self, target: &str, capture: &str) {
            self.captures
                .lock()
                .expect("captures")
                .insert(target.to_owned(), capture.to_owned());
        }

        fn sends(&self) -> Vec<(String, String)> {
            self.sends.lock().expect("sends").clone()
        }
    }

    impl ServeDelivery for FakeServeDelivery {
        fn route_sessions(&self) -> Result<Vec<RouteSession>, String> {
            if let Some(error) = self.list_error.lock().expect("list error").clone() {
                return Err(error);
            }
            let mut sessions = self.sessions.lock().expect("sessions");
            if sessions.len() > 1 {
                return Ok(sessions.remove(0));
            }
            Ok(sessions.first().cloned().unwrap_or_default())
        }

        fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String> {
            if let Some(error) = self.send_error.lock().expect("send error").clone() {
                return Err(error);
            }
            self.sends
                .lock()
                .expect("sends")
                .push((target.to_owned(), text.to_owned()));
            Ok(())
        }

        fn capture_tail(&self, target: &str, _lines: u32) -> Result<String, String> {
            Ok(self
                .captures
                .lock()
                .expect("captures")
                .get(target)
                .cloned()
                .unwrap_or_else(|| "[capture] delivered\n".to_owned()))
        }
    }

    fn serve_test_session(name: &str, index: u32, window: &str) -> RouteSession {
        RouteSession {
            name: name.to_owned(),
            source: None,
            windows: vec![RouteWindow {
                index,
                name: window.to_owned(),
                active: true,
                kind: None,
            }],
        }
    }

    fn serve_test_delivery() -> Arc<dyn ServeDelivery> {
        Arc::new(FakeServeDelivery::with_capture_agent())
    }

    fn serve_test_receiver_inbox() -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(false),
            fixed_now_millis: Some(1_782_277_200_000),
            psi_root: None,
        })
    }

    fn serve_test_receiver_inbox_at(repo: &std::path::Path, now_millis: u128) -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(true),
            fixed_now_millis: Some(now_millis),
            psi_root: Some(repo.join("ψ")),
        })
    }

    fn serve_test_receiver_inbox_from_manifest(now_millis: u128) -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(true),
            fixed_now_millis: Some(now_millis),
            psi_root: None,
        })
    }

    fn serve_test_peer_pubkey(from: &str, pubkey: &str) -> ServePeerPubkey {
        ServePeerPubkey {
            from: from.to_owned(),
            node: node_from_identity(from).expect("peer identity node"),
            pubkey: pubkey.to_owned(),
        }
    }

    fn serve_test_trust_store_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "maw-rs-trust-live-{label}-{}-{}.json",
            std::process::id(),
            random_hex(4)
        ))
    }

    fn serve_test_app(trust_store_path: std::path::PathBuf) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path,
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
        })
    }

    fn serve_test_app_with_plugin_routes(plugin_serve_routes: Vec<ServePluginRoute>) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("plugins"),
            plugin_serve_routes,
            api_token_auth: ServeApiTokenAuth::open(),
        })
    }

    fn serve_test_app_with_api_auth(api_token_auth: ServeApiTokenAuth) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("api-token"),
            plugin_serve_routes: vec![ServePluginRoute {
                name: "testext".to_owned(),
                command: None,
                prefix: "/api/testext".to_owned(),
                health_path: "/api/testext/health".to_owned(),
                events: Vec::new(),
                event_path: None,
                dir: std::env::temp_dir(),
                process: Arc::new(Mutex::new(None)),
            }],
            api_token_auth,
        })
    }

    fn serve_test_proxy_route(port: u16, child: Child) -> ServePluginRoute {
        ServePluginRoute {
            name: "testext".to_owned(),
            command: Some("sleep 60".to_owned()),
            prefix: "/api/testext".to_owned(),
            health_path: "/api/testext/health".to_owned(),
            events: Vec::new(),
            event_path: None,
            dir: std::env::temp_dir(),
            process: Arc::new(Mutex::new(Some(ServePluginProcess { port, child }))),
        }
    }

    fn signed_trust_request(method: &str, uri: &str, auth_path: &str, body: &'static str) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(
            KEY,
            KEY,
            FROM,
            method,
            auth_path,
            Some(body.as_bytes()),
            1_782_277_200,
        )
        .expect("sign trust");
        let fleet_signature = sign_hmac_sig(KEY, &format!("{method}:{uri}:1782277200"));
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-maw-signature", fleet_signature);
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::from(body)).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    fn unsigned_trust_request(method: &str, uri: &str, body: &'static str) -> axum::http::Request<Body> {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json")
    }

    fn serve_test_app_with_o6_keys(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
    ) -> Router {
        serve_test_app_with_o6_keys_and_delivery(keys, now, peer_addr_override, serve_test_delivery())
    }

    fn serve_test_app_with_o6_keys_and_delivery(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
        delivery: Arc<dyn ServeDelivery>,
    ) -> Router {
        serve_test_app_with_o6_keys_delivery_and_inbox(
            keys,
            now,
            peer_addr_override,
            delivery,
            serve_test_receiver_inbox(),
        )
    }

    fn serve_test_app_with_o6_keys_delivery_and_inbox(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
        delivery: Arc<dyn ServeDelivery>,
        receiver_inbox: Arc<dyn ServeReceiverInbox>,
    ) -> Router {
        serve_router(ServeState {
            cached_pubkey: None,
            peer_pubkeys: keys,
            workspace_key: Some("capture-test-token-393av2".to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery,
            receiver_inbox,
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override,
            now_override: Some(now),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("o6"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
        })
    }

    fn captured_send_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/serve-auth/maw-js-hey-captured-api-send.json"
        ))
        .expect("captured maw-js fixture")
    }

    fn captured_send_key() -> ServePeerPubkey {
        let fixture = captured_send_fixture();
        let from = fixture["headers"]["X-Maw-From"]
            .as_str()
            .expect("from");
        serve_test_peer_pubkey(from, fixture["testPeerKey"].as_str().expect("peer key"))
    }

    fn captured_send_request() -> axum::http::Request<Body> {
        let fixture = captured_send_fixture();
        let method = fixture["method"].as_str().expect("method");
        let path = fixture["path"].as_str().expect("path");
        let body = fixture["body"].as_str().expect("body");
        let mut builder = axum::http::Request::builder().method(method).uri(path);
        for (name, value) in fixture["headers"].as_object().expect("headers") {
            builder = builder.header(name.as_str(), value.as_str().expect("header value"));
        }
        let mut request = builder.body(Body::from(body.to_owned())).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    fn signed_json_request(
        method: &str,
        path: &str,
        body: &'static str,
        key: &str,
        from: &str,
        now: i64,
    ) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(key, key, from, method, path, Some(body.as_bytes()), now)
            .expect("sign v3");
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::from(body)).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    #[test]
    fn serve_peer_pubkey_collection_sets_node_for_identity_shapes() {
        let value = json!({
            "peers": {
                "nova:bigboy-vps": "node-key-a",
                "alias": {"pubkey": "node-key-b", "oracle": "seed", "node": "bigboy-vps"},
                "direct": {"pubkey": "node-key-c", "from": "gm-bo:bigboy-vps"}
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "nova:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-a"));
        assert!(entries.iter().any(|entry| entry.from == "seed:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-b"));
        assert!(entries.iter().any(|entry| entry.from == "gm-bo:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-c"));
    }

    #[test]
    fn serve_peer_pubkey_collection_reads_maw_js_nested_identity_shape() {
        let value = json!({
            "version": 1,
            "peers": {
                "bigboy-vps": {
                    "url": "http://100.64.0.1:3456",
                    "node": "bigboy-vps",
                    "addedAt": "2026-06-28T00:00:00.000Z",
                    "lastSeen": "2026-06-28T00:01:00.000Z",
                    "pubkeyFirstSeen": "2026-06-24T00:00:00.000Z",
                    "pubkey": "node-key-bigboy-vps-401",
                    "identity": {"oracle": "mawjs", "node": "bigboy-vps"}
                }
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "mawjs:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-bigboy-vps-401"));
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_accepts_unseeded_oracle_on_known_node() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello node fallback"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("node fallback response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
        let sends = delivery.sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, "capture-agent:0");
        assert_eq!(sends[0].1, "[alloy:bigboy-vps] hello node fallback");
    }

    #[tokio::test]
    async fn serve_api_send_dedups_cross_turn_duplicate_by_delivery_key() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let first_body = r#"{"target":"capture-agent","text":"codex-2 DONE #87 full suite green"}"#;
        let intervening_body = r#"{"target":"capture-agent","text":"another turn between duplicate emissions"}"#;

        let first = app
            .clone()
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                first_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("first response");
        let first_payload = response_json(first).await;
        assert_eq!(first_payload["state"], "delivered");

        let intervening = app
            .clone()
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                intervening_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("intervening response");
        let intervening_payload = response_json(intervening).await;
        assert_eq!(intervening_payload["state"], "delivered");

        let duplicate = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                first_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("duplicate response");
        let duplicate_status = duplicate.status();
        let duplicate_payload = response_json(duplicate).await;
        assert_eq!(duplicate_status, StatusCode::OK, "{duplicate_payload}");
        assert_eq!(duplicate_payload["state"], "delivered");
        assert_eq!(duplicate_payload["deduped"], true);
        assert_eq!(duplicate_payload["receipt"], json!(["duplicate_dropped"]));

        let sends = delivery.sends();
        assert_eq!(sends.len(), 2, "delayed replay must not reinject");
        assert_eq!(
            sends[0],
            (
                "capture-agent:0".to_owned(),
                "[alloy:bigboy-vps] codex-2 DONE #87 full suite green".to_owned()
            )
        );
        assert_eq!(
            sends[1],
            (
                "capture-agent:0".to_owned(),
                "[alloy:bigboy-vps] another turn between duplicate emissions".to_owned()
            )
        );
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_accepts_collected_maw_js_nested_identity_shape() {
        let node_key = "node-key-bigboy-vps-401";
        let value = json!({
            "version": 1,
            "peers": {
                "bigboy-vps": {
                    "url": "http://100.64.0.1:3456",
                    "node": "bigboy-vps",
                    "addedAt": "2026-06-28T00:00:00.000Z",
                    "lastSeen": "2026-06-28T00:01:00.000Z",
                    "pubkeyFirstSeen": "2026-06-24T00:00:00.000Z",
                    "pubkey": node_key,
                    "identity": {"oracle": "mawjs", "node": "bigboy-vps"}
                }
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "mawjs:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == node_key));

        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            entries,
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested identity"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("nested identity fallback response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
        let sends = delivery.sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, "capture-agent:0");
        assert_eq!(sends[0].1, "[alloy:bigboy-vps] hello nested identity");
    }

    #[tokio::test]
    async fn serve_o6_exact_mismatch_does_not_fallback_to_node_key() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("alloy:bigboy-vps", "wrong-exact-key-399"),
                serve_test_peer_pubkey("nova:bigboy-vps", node_key),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"exact must win"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("exact mismatch response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-mismatch");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_rejects_unknown_node() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"unknown node"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:other-node",
                1_782_277_200,
            ))
            .await
            .expect("unknown node response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-missing-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_rejects_ambiguous_node_keys() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("nova:bigboy-vps", "node-key-a-399"),
                serve_test_peer_pubkey("seed:bigboy-vps", "node-key-b-399"),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"ambiguous node"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                "node-key-a-399",
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("ambiguous node response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-ambiguous-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_live_router_accepts_captured_maw_js_send_for_exact_from_key() {
        let app = serve_test_app_with_o6_keys(
            vec![
                serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key"),
                captured_send_key(),
            ],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_when_exact_from_key_missing() {
        let app = serve_test_app_with_o6_keys(
            vec![serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key")],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-missing-peer-key");
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_with_wrong_from_key() {
        let mut key = captured_send_key();
        key.pubkey = "wrong-peer-key-393av2".to_owned();
        let app = serve_test_app_with_o6_keys(vec![key], 1_782_553_858, Some(NON_LOOPBACK_TEST_PEER));
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-mismatch");
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_with_expired_timestamp() {
        let app = serve_test_app_with_o6_keys(
            vec![captured_send_key()],
            1_782_554_500,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-skew");
    }

    #[tokio::test]
    async fn serve_o6_live_router_loopback_bypasses_from_key_resolution_separately() {
        let app = serve_test_app_with_o6_keys(
            Vec::new(),
            1_782_553_858,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152)),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
    }

    fn serve_test_inbox_repo(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "maw-rs-receiver-inbox-{label}-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("ψ")).expect("repo psi");
        repo
    }

    struct ServeInboxManifestEnv {
        _guard: std::sync::MutexGuard<'static, ()>,
        root: std::path::PathBuf,
        config: std::path::PathBuf,
        cache: std::path::PathBuf,
        ghq: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl ServeInboxManifestEnv {
        fn new(label: &str) -> Self {
            let guard = env_test_lock().lock().unwrap_or_else(|error| error.into_inner());
            let keys = [
                "HOME",
                "MAW_HOME",
                "MAW_CONFIG_DIR",
                "MAW_CACHE_DIR",
                "MAW_XDG",
                "XDG_CONFIG_HOME",
                "GHQ_ROOT",
            ];
            let saved = keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!(
                "maw-rs-receiver-inbox-manifest-{label}-{}-{}",
                std::process::id(),
                random_hex(4)
            ));
            let home = root.join("home");
            let config = root.join("config");
            let cache = root.join("cache");
            let ghq = root.join("ghq");
            std::fs::create_dir_all(config.join("fleet")).expect("fleet dir");
            std::fs::create_dir_all(&cache).expect("cache dir");
            std::fs::create_dir_all(ghq.join("github.com")).expect("ghq dir");
            std::env::set_var("HOME", &home);
            std::env::remove_var("MAW_HOME");
            std::env::remove_var("MAW_XDG");
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("MAW_CONFIG_DIR", &config);
            std::env::set_var("MAW_CACHE_DIR", &cache);
            std::env::set_var("GHQ_ROOT", ghq.join("github.com"));
            Self {
                _guard: guard,
                root,
                config,
                cache,
                ghq,
                saved,
            }
        }

        fn add_fleet_repo(
            &self,
            file: &str,
            session: &str,
            window: &str,
            repo: &str,
        ) -> std::path::PathBuf {
            let repo_path = self.ghq.join("github.com").join(repo);
            std::fs::create_dir_all(repo_path.join("ψ")).expect("repo psi");
            let fleet = json!({
                "name": session,
                "windows": [{"name": window, "repo": repo}],
            });
            std::fs::write(
                self.config.join("fleet").join(file),
                serde_json::to_string_pretty(&fleet).expect("fleet json"),
            )
            .expect("write fleet");
            repo_path
        }

        fn write_local_scanned_oracles_json(&self, name: &str, repo: &str, local_path: &std::path::Path) {
            let value = json!({
                "schema": 1,
                "oracles": [{
                    "org": "tonkmac",
                    "repo": repo,
                    "name": name,
                    "local_path": local_path.display().to_string(),
                    "has_psi": true,
                    "has_fleet_config": true,
                    "federation_node": "bigboy-vps"
                }]
            });
            std::fs::write(
                self.cache.join("oracles.json"),
                serde_json::to_string_pretty(&value).expect("oracles json"),
            )
            .expect("write oracles");
        }
    }

    impl Drop for ServeInboxManifestEnv {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_writes_receiver_inbox_without_tmux_send() {
        let repo = serve_test_inbox_repo("success");
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_at(&repo, 1_782_623_880_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["source"], "inbox");
        assert_eq!(payload["state"], "queued");
        assert_eq!(payload["target"], "capture-agent:0");
        assert_eq!(payload["receipt"], json!(["fallback_queued"]));
        assert_eq!(payload["reason"], "--inbox requested; pane injection skipped");
        assert!(delivery.sends().is_empty(), "inbox-only must not inject tmux");

        let expected = repo
            .join("ψ")
            .join("inbox")
            .join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox.md");
        assert_eq!(payload["inbox"], expected.display().to_string());
        let written = std::fs::read_to_string(&expected).expect("inbox body");
        assert_eq!(
            written,
            "---\nfrom: bigboy-vps:alloy\nto: capture-agent\ntimestamp: 2026-06-28T05:18:00.000Z\nread: false\n---\n\nhello nested inbox\n"
        );
    }

    #[test]
    fn receiver_inbox_manifest_phase_a_keeps_numbered_oracle_name_match() {
        let env = ServeInboxManifestEnv::new("phase-a");
        let repo = env.add_fleet_repo(
            "01-wish.json",
            "01-wish",
            "wish-oracle",
            "tonkmac/wish-oracle",
        );
        let config = HeyConfig {
            node: None,
            oracle: None,
            route: RouteConfig::default(),
        };
        let result = persist_receiver_inbox(
            ReceiverInboxInput {
                query: "wish",
                target: Some("wish"),
                to: Some("wish"),
                from: "bigboy-vps:alloy",
                message: "hello wish inbox",
                config: &config,
            },
            1_782_623_880_000,
            None,
        );
        let ReceiverInboxResult::Ok(ok) = result else {
            panic!("phase-a inbox write failed: {result:?}");
        };
        assert_eq!(ok.oracle, "wish");
        assert_eq!(ok.inbox_dir, repo.join("ψ").join("inbox"));
        let written = std::fs::read_to_string(ok.path).expect("inbox body");
        assert!(written.contains("to: wish\n"));
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_resolves_fleet_target_cwd_without_relabeling_oracle() {
        let env = ServeInboxManifestEnv::new("bigboylocal");
        let repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        env.write_local_scanned_oracles_json("bigboylocal", "bigboylocal-oracle", &repo);
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![vec![serve_test_session(
            "02-bigboy",
            0,
            "bigboylocal-oracle",
        )]]);
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_from_manifest(1_782_623_880_000),
        );
        let body = r#"{"target":"02-bigboy","text":"hello bigboy inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["target"], "02-bigboy:0");
        assert_eq!(payload["source"], "inbox");
        assert!(delivery.sends().is_empty(), "inbox-only must not inject tmux");

        let expected = repo
            .join("ψ")
            .join("inbox")
            .join("2026-06-28_05-18_bigboy-vps-alloy_hello-bigboy-inbox.md");
        assert_eq!(payload["inbox"], expected.display().to_string());
        let written = std::fs::read_to_string(&expected).expect("inbox body");
        assert_eq!(
            written,
            concat!(
                "---\n",
                "from: bigboy-vps:alloy\n",
                "to: bigboy\n",
                "timestamp: 2026-06-28T05:18:00.000Z\n",
                "read: false\n",
                "---\n\n",
                "hello bigboy inbox\n"
            )
        );
    }

    #[test]
    fn receiver_inbox_target_cwd_matches_maw_js_window_selection_rules() {
        let env = ServeInboxManifestEnv::new("target-cwd");
        let repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy").expect("session"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy:0").expect("index"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy:bigboylocal-oracle").expect("window"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("node:02-bigboy:bigboylocal-oracle")
                .expect("node window"),
            Some(repo)
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("bigboy").expect("wrong owner"),
            None
        );
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_refuses_ambiguous_fleet_session_owner() {
        let env = ServeInboxManifestEnv::new("ambiguous");
        let repo_one = env.add_fleet_repo(
            "02-bigboy-a.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        let repo_two = env.add_fleet_repo(
            "02-bigboy-b.json",
            "02-bigboy",
            "bigboylocal-alt-oracle",
            "tonkmac/bigboylocal-alt-oracle",
        );
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![vec![serve_test_session(
            "02-bigboy",
            0,
            "bigboylocal-oracle",
        )]]);
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_from_manifest(1_782_623_880_000),
        );
        let body = r#"{"target":"02-bigboy","text":"hello ambiguous inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(payload["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("receiver repo ambiguous"));
        assert!(delivery.sends().is_empty());
        assert!(!repo_one.join("ψ").join("inbox").exists());
        assert!(!repo_two.join("ψ").join("inbox").exists());
    }

    #[test]
    fn receiver_inbox_target_lookup_refuses_numeric_strip_wrong_owner() {
        let env = ServeInboxManifestEnv::new("wrong-owner");
        let _repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        let config = HeyConfig {
            node: None,
            oracle: None,
            route: RouteConfig::default(),
        };
        let result = persist_receiver_inbox(
            ReceiverInboxInput {
                query: "bigboy",
                target: Some("bigboy"),
                to: Some("bigboy"),
                from: "bigboy-vps:alloy",
                message: "hello wrong owner",
                config: &config,
            },
            1_782_623_880_000,
            None,
        );
        match result {
            ReceiverInboxResult::Err { oracle, reason } => {
                assert_eq!(oracle.as_deref(), Some("bigboy"));
                assert_eq!(reason, "receiver repo not found for bigboy");
            }
            ReceiverInboxResult::Ok(ok) => panic!("unexpected inbox write: {ok:?}"),
        }
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_disabled_fails_closed_without_fake_queue() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/send", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["state"], "failed");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(payload["detail"].as_str().unwrap_or_default().contains("disabled"));
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_write_error_fails_closed_without_tmux_send() {
        let repo = serve_test_inbox_repo("write-error");
        std::fs::write(repo.join("ψ").join("inbox"), "not a dir").expect("block inbox dir");
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_at(&repo, 1_782_277_200_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/send", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["state"], "failed");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_uses_exclusive_collision_suffix() {
        let repo = serve_test_inbox_repo("collision");
        let inbox_dir = repo.join("ψ").join("inbox");
        std::fs::create_dir_all(&inbox_dir).expect("inbox dir");
        let base = inbox_dir.join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox.md");
        std::fs::write(&base, "existing").expect("existing base");
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            Arc::new(FakeServeDelivery::with_capture_agent()),
            serve_test_receiver_inbox_at(&repo, 1_782_623_880_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let payload = response_json(response).await;
        let suffixed = inbox_dir.join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox-2.md");
        assert_eq!(payload["inbox"], suffixed.display().to_string());
        assert_eq!(std::fs::read_to_string(&base).expect("base"), "existing");
        assert!(suffixed.is_file());
    }

    #[tokio::test]
    async fn serve_api_send_toctou_refuses_disappeared_target_before_send() {
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![
            vec![serve_test_session("capture-agent", 0, "capture-agent")],
            Vec::new(),
        ]);
        let app = serve_test_app_with_o6_keys_and_delivery(
            Vec::new(),
            1_782_553_858,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152)),
            delivery.clone(),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{payload}");
        assert_eq!(payload["error"], "target-disappeared");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_auth_reject_is_logged_without_delivery() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key")],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let rejected = app
            .clone()
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
        let feed = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/feed")
                    .body(Body::empty())
                    .expect("feed request"),
            )
            .await
            .expect("feed");
        let payload = response_json(feed).await;
        assert_eq!(payload["events"][0]["state"], "failed");
        assert_eq!(payload["events"][0]["decision"], "refuse-missing-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_from_aware_key_resolution_also_unblocks_api_feed() {
        let app = serve_test_app_with_o6_keys(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/feed",
                r#"{"event":"hello"}"#,
                KEY,
                FROM,
                1_782_277_200,
            ))
            .await
            .expect("feed response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
    }

    async fn spawn_test_server() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let app = serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("server"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
        });
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("serve test server");
        });
        std::mem::forget(tx);
        addr
    }

    #[tokio::test]
    async fn serve_real_wire_accepts_v3_rejects_unsigned_and_accepts_legacy() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let url = format!("http://{addr}/api/send");
        let body = r#"{"target":"remote-oracle","text":"hello"}"#;
        let timestamp = 1_782_277_200_i64;
        let headers = sign_headers_v3_at(
            KEY,
            KEY,
            FROM,
            "POST",
            "/api/send",
            Some(body.as_bytes()),
            timestamp,
        )
        .expect("sign v3");
        let mut request = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned());
        for (name, value) in headers.to_btree_map() {
            request = request.header(name, value);
        }
        let response = request.send().await.expect("send signed");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["state"], "delivered");

        let response = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-forwarded-for", "127.0.0.1")
            .body(body.to_owned())
            .send()
            .await
            .expect("send unsigned");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let signed_at = "2026-06-24T05:00:00.000Z";
        let now = 1_782_277_200_i64;
        let body_hash = hash_body(Some(body.as_bytes()));
        let payload = build_legacy_from_sign_payload(FROM, signed_at, "POST", "/api/send", &body_hash);
        let legacy_sig = sign_hmac_sig(KEY, &payload);
        let response = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-maw-from", FROM)
            .header("x-maw-signature", legacy_sig)
            .header("x-maw-signed-at", signed_at)
            .header("x-maw-auth-version", "v3")
            .header("x-maw-timestamp", now.to_string())
            .body(body.to_owned())
            .send()
            .await
            .expect("send legacy");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_plugin_engine_command_prefix_http_proxies_when_process_is_up() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind upstream");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept upstream");
            let mut buf = [0_u8; 1024];
            let n = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.starts_with("GET /api/testext/assets/app.js?x=1 "));
            stream.write_all(b"HTTP/1.1 202 Accepted\r\ncontent-type: text/plain\r\ncontent-length: 7\r\n\r\nproxied").await.expect("write response");
        });
        let child = Command::new("sleep").arg("60").spawn().expect("sleep child");
        let app = serve_test_app_with_plugin_routes(vec![serve_test_proxy_route(port, child)]);
        let response = app.oneshot(axum::http::Request::get("/api/testext/assets/app.js?x=1").body(Body::empty()).unwrap()).await.expect("proxy response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024).await.expect("body");
        assert_eq!(&body[..], b"proxied");
    }

    #[tokio::test]
    async fn serve_plugin_health_falls_back_when_command_process_is_down() {
        let route = ServePluginRoute {
            name: "testext".to_owned(),
            command: Some("sleep 60".to_owned()),
            prefix: "/api/testext".to_owned(),
            health_path: "/api/testext/health".to_owned(),
            events: Vec::new(),
            event_path: None,
            dir: std::env::temp_dir(),
            process: Arc::new(Mutex::new(None)),
        };
        let app = serve_test_app_with_plugin_routes(vec![route]);
        let response = app.oneshot(axum::http::Request::get("/api/testext/health").body(Body::empty()).unwrap()).await.expect("health");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["plugin"], "testext");
        assert_eq!(payload["command"], "sleep 60");
    }

    #[tokio::test]
    async fn serve_api_token_auth_gates_api_but_leaves_health_open() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth {
            token: Some("secret-token".to_owned()),
            loopback_exempt: false,
            forced_open: false,
        });
        let denied = app.clone().oneshot(axum::http::Request::get("/api/feed").body(Body::empty()).unwrap()).await.expect("denied");
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let health = app.clone().oneshot(axum::http::Request::get("/api/health").body(Body::empty()).unwrap()).await.expect("health");
        assert_eq!(health.status(), StatusCode::OK);

        let bearer = app.clone().oneshot(
            axum::http::Request::get("/api/feed")
                .header("authorization", "Bearer secret-token")
                .body(Body::empty())
                .unwrap(),
        ).await.expect("bearer");
        assert_eq!(bearer.status(), StatusCode::OK);

        let plugin = app.oneshot(
            axum::http::Request::get("/api/testext/health")
                .header("x-maw-token", "secret-token")
                .body(Body::empty())
                .unwrap(),
        ).await.expect("plugin x token");
        assert_eq!(plugin.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_api_token_auth_open_mode_is_backward_compatible() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth::open());
        let response = app.oneshot(axum::http::Request::get("/api/feed").body(Body::empty()).unwrap()).await.expect("open mode");
        assert_eq!(response.status(), StatusCode::OK);
    }


    #[tokio::test]
    async fn serve_mounts_discovered_plugin_engine_serve_health_and_skips_bad_manifest() {
        let (root, plugin_routes) = {
            let _guard = env_test_lock().lock().unwrap_or_else(|error| error.into_inner());
            let _plugins_restore = EnvVarRestore::capture("MAW_PLUGINS_DIR");
            let root = std::env::temp_dir().join(format!(
                "maw-serve-plugin-{}-{}",
                std::process::id(),
                random_hex(4)
            ));
            let plugins = root.join("plugins");
            serve_write_plugin(
                &plugins,
                "testext",
                &json!({"prefix": "/api/testext", "health": "/health", "events": ["ready"], "eventPath": "/events"}),
            );
            serve_write_plugin(&plugins, "badext", &json!({"prefix": "/not-api/bad"}));
            std::env::set_var("MAW_PLUGINS_DIR", &plugins);
            (root, serve_discover_plugin_routes())
        };

        let app = serve_test_app_with_plugin_routes(plugin_routes);
        let health = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/testext/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("plugin health");
        assert_eq!(health.status(), StatusCode::OK);
        let payload = response_json(health).await;
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["plugin"], "testext");
        assert_eq!(payload["prefix"], "/api/testext");

        let missing = app
            .clone()
            .oneshot(
                axum::http::Request::get("/not-api/bad/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("bad plugin skipped");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let core = app
            .oneshot(axum::http::Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .expect("core health");
        assert_eq!(core.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(root);
    }

    fn serve_write_plugin(root: &std::path::Path, name: &str, serve: &Value) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("plugin dir");
        std::fs::write(dir.join("index.ts"), "export default async function run() {}\n").expect("entry");
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::to_vec_pretty(&json!({
                "name": name,
                "version": "1.0.0",
                "sdk": "*",
                "target": "js",
                "entry": "index.ts",
                "engine": {"serve": serve}
            }))
            .expect("manifest json"),
        )
        .expect("manifest");
    }

    #[tokio::test]
    async fn serve_trust_live_is_auth_gated_atomic_redacted_and_tofu_safe() {
        let path = serve_test_trust_store_path("route");
        let app = serve_test_app(path.clone());
        assert!(maw_auth::is_protected("/api/trust", "POST"));
        assert!(maw_auth::is_protected("/api/trust/revoke", "POST"));
        assert!(maw_auth::is_protected("/api/trust", "GET"));

        let secret_key = "ed25519:alpha-peer-key-secret";
        let body = r#"{"sender":"alpha","target":"beta","peerKey":"ed25519:alpha-peer-key-secret"}"#;
        let denied = app
            .clone()
            .oneshot(unsigned_trust_request("POST", "/api/trust", body))
            .await
            .expect("denied");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let trusted = app
            .clone()
            .oneshot(signed_trust_request("POST", "/api/trust", "/trust", body))
            .await
            .expect("trust");
        let trusted_status = trusted.status();
        let payload = response_json(trusted).await;
        assert_eq!(trusted_status, StatusCode::OK, "{payload}");
        let rendered = payload.to_string();
        assert_eq!(payload["peerKey"], "received (redacted)");
        assert!(!rendered.contains(secret_key), "{rendered}");
        let stored = std::fs::read_to_string(&path).expect("stored");
        assert!(stored.contains(secret_key));
        assert!(!path.with_extension("json.tmp").exists());

        let mismatch = r#"{"sender":"beta","target":"alpha","peerKey":"ed25519:different-peer-key"}"#;
        let rejected = app
            .clone()
            .oneshot(signed_trust_request("POST", "/api/trust", "/trust", mismatch))
            .await
            .expect("mismatch");
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let rejected_payload = response_json(rejected).await.to_string();
        assert!(rejected_payload.contains("peer-key mismatch"));
        assert!(!rejected_payload.contains("different-peer-key"));

        let listed = app
            .clone()
            .oneshot(signed_trust_request("GET", "/api/trust", "/trust", ""))
            .await
            .expect("list");
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_payload = response_json(listed).await.to_string();
        assert!(listed_payload.contains("received (redacted)"));
        assert!(!listed_payload.contains(secret_key));

        let missing_yes = r#"{"sender":"alpha","target":"beta"}"#;
        let refused = app
            .clone()
            .oneshot(signed_trust_request(
                "POST",
                "/api/trust/revoke",
                "/trust/revoke",
                missing_yes,
            ))
            .await
            .expect("missing yes");
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

        let revoke = r#"{"sender":"alpha","target":"beta","yes":true}"#;
        let revoked = app
            .oneshot(signed_trust_request(
                "POST",
                "/api/trust/revoke",
                "/trust/revoke",
                revoke,
            ))
            .await
            .expect("revoke");
        assert_eq!(revoked.status(), StatusCode::OK);
        let entries = trust_read_store(&path).expect("read after revoke");
        assert!(entries.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn serve_default_bind_matches_maw_js_parity_and_ignores_maw_host() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("MAW_HOST");
        std::env::set_var("MAW_HOST", "127.0.0.1");
        let args = parse_serve_args(&[]).expect("default serve args");
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(args.port, 3456);
        assert_eq!(
            resolve_serve_socket_addr(&args).expect("default bind"),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3456)
        );
    }

    #[tokio::test]
    async fn serve_host_port_override_resolves_and_binds_throwaway_loopback() {
        let args = parse_serve_args(&[
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            "0".to_owned(),
        ])
        .expect("override serve args");
        let addr = resolve_serve_socket_addr(&args).expect("override bind");
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(addr.port(), 0);
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("throwaway loopback bind");
        assert_eq!(
            listener.local_addr().expect("local addr").ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn serve_host_validation_rejects_injection_before_bind() {
        for host in ["", "-0.0.0.0", "127.0.0.1\nx", "localhost"] {
            let args = ServeArgs {
                host: host.to_owned(),
                port: 3456,
                cached_pubkey: None,
            };
            assert_eq!(
                resolve_serve_socket_addr(&args),
                Err("serve: --host must be an IP address".to_owned()),
                "host={host:?}"
            );
        }
    }

    #[tokio::test]
    async fn serve_core_real_router_allows_loopback_protected_paths() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let trigger = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event":"agent-idle","context":{"repo":"maw-rs"}}))
            .send()
            .await
            .expect("protected request");
        assert_eq!(trigger.status(), StatusCode::OK, "/api/triggers/fire");
        let plugins = client
            .post(format!("http://{addr}/api/plugins/reload"))
            .send()
            .await
            .expect("protected request");
        assert_eq!(plugins.status(), StatusCode::OK, "/api/plugins/reload");
        let cleanup = client
            .post(format!("http://{addr}/api/worktrees/cleanup"))
            .send()
            .await
            .expect("protected request");
        assert_eq!(
            cleanup.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "/api/worktrees/cleanup is live JSON route, not core stub"
        );
        let public = client
            .get(format!("http://{addr}/api/agents"))
            .send()
            .await
            .expect("public request");
        assert_eq!(public.status(), StatusCode::OK);
        let costs = client
            .get(format!("http://{addr}/api/costs"))
            .header("origin", "https://god.buildwithoracle.com")
            .send()
            .await
            .expect("costs request");
        assert_eq!(costs.status(), StatusCode::OK, "/api/costs");
        assert_eq!(
            costs
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
        let missing = client
            .get(format!("http://{addr}/api/missing-god-ui-route"))
            .header("origin", "https://god.buildwithoracle.com")
            .send()
            .await
            .expect("missing request");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
    }

    #[tokio::test]
    async fn serve_agents_real_router_is_public_and_uses_fake_state() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let fake_core = crate::serve_core::ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_agents_snapshot(vec![crate::serve_core::ServecoreAgentPane {
                id: "%86".to_owned(),
                command: "codex".to_owned(),
                target: "nova:1.0".to_owned(),
                title: "nova-agent".to_owned(),
                cwd: Some("/tmp/maw-rs".to_owned()),
                pid: Some(8600),
                last_activity: Some(86),
            }]);
        let app = serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: Some(fake_core),
            trust_store_path: serve_test_trust_store_path("agents"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
        });
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("serve test server");
        });
        std::mem::forget(tx);

        let client = reqwest::Client::builder().build().expect("client");
        let response = client
            .get(format!("http://{addr}/api/agents"))
            .send()
            .await
            .expect("agents");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["node"], "node-a");
        assert_eq!(payload["agents"][0]["target"], "nova:1.0");

        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event":"agent-idle","context":{"repo":"maw-rs"}}))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_real_wire_websocket_subscribe_returns_native_ack_not_echo() {
        let addr = spawn_test_server().await;
        let url = format!("ws://{addr}/ws");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"type":"subscribe","target":"demo:1"}"#.to_owned(),
        ))
        .await
        .expect("send websocket text");

        let ack = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let received = ws
                    .next()
                    .await
                    .expect("websocket should yield a frame")
                    .expect("frame should be ok");
                if let tokio_tungstenite::tungstenite::Message::Text(text) = received {
                    let value = serde_json::from_str::<Value>(&text).expect("json");
                    if value["type"] == "subscribed" {
                        assert_eq!(value["target"], "demo:1");
                        break;
                    }
                }
            }
        })
        .await;
        assert!(ack.is_ok(), "websocket should ack subscribe after stream frames");
    }

    #[tokio::test]
    async fn workspace_hub_signed_routes_accept_and_unsigned_rejects() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let create_url = format!("http://{addr}/api/workspace/create");
        let create_response = client
            .post(create_url)
            .json(&json!({"name": "nova", "nodeId": "node-a"}))
            .send()
            .await
            .expect("create workspace");
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_payload = create_response.json::<Value>().await.expect("create json");
        let workspace_id = create_payload["id"].as_str().expect("workspace id");
        let token = create_payload["token"].as_str().expect("workspace token");
        assert_eq!(token.len(), 64);

        let agents_path = format!("/api/workspace/{workspace_id}/agents");
        let agents_url = format!("http://{addr}{agents_path}");
        let unsigned = client
            .post(&agents_url)
            .json(&json!({"name": "nova-codex-1", "nodeId": "node-a"}))
            .send()
            .await
            .expect("unsigned agents request");
        assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);

        let timestamp = "1782277200";
        let signature = sign_hmac_sig(token, &format!("POST:{agents_path}:{timestamp}"));
        let signed = client
            .post(&agents_url)
            .header("x-maw-timestamp", timestamp)
            .header("x-maw-signature", signature)
            .json(&json!({
                "name": "nova-codex-1",
                "nodeId": "node-a",
                "status": "online",
                "capabilities": ["relay"]
            }))
            .send()
            .await
            .expect("signed agents request");
        assert_eq!(signed.status(), StatusCode::OK);
        let signed_payload = signed.json::<Value>().await.expect("signed json");
        assert_eq!(signed_payload["ok"], true);
        assert_eq!(signed_payload["agents"], 1);
    }
}
