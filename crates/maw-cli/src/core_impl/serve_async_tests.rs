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
        stderr: format!(
            "{prefix}usage: maw-rs serve [--host 0.0.0.0] [--port <port>] [--cached-pubkey <key>] | maw-rs serve status|--status|stop\n"
        ),
    }
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
    let state = Arc::new(state);
    let router = Router::new();
    let router = crate::serve_core::servecore_mount_core_routes(router);
    let router = crate::serve_core::servecore_mount_ws_routes(router);
    let router = crate::serve_core::modules::servecore_mount_modules(router, &[]);
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
        .route("/info", get(api_peers_info))
        .route("/api/peers/info", get(api_peers_info))
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
    let router = crate::serve_core::servecore_apply_pipeline(router);
    let router = crate::serve_core::servecore_with_shared_state(router, serve_core_state);
    router.fallback(api_not_found).with_state(state)
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
    let log_from = from.clone().unwrap_or_else(|| serve_local_identity(&config));
    let log_to = serve_local_identity(&config);

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
                from: from.as_deref(),
                log_from: &log_from,
                log_to: &log_to,
                requested: &target,
                resolved: &resolved,
                message: &message,
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
    let from = serve_display_from(headers, config);
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
            serve_log_delivery_failed(state, target, message, log_from, log_to, &reason, "inbox");
            serve_delivery_error(StatusCode::BAD_GATEWAY, "receiver-inbox-unavailable", target, &reason)
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

struct ServeDeliverContext<'a> {
    config: &'a HeyConfig,
    from: Option<&'a str>,
    log_from: &'a str,
    log_to: &'a str,
    requested: &'a str,
    resolved: &'a str,
    message: &'a str,
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

    let outbound = format_local_hey_message(context.message, context.config, context.from);
    if let Err(error) = state.delivery.send_literal_enter(context.resolved, &outbound) {
        serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "tmux-send");
        return serve_delivery_error(StatusCode::BAD_GATEWAY, "tmux-send-failed", context.resolved, &error);
    }

    let capture = state.delivery.capture_tail(context.resolved, 8).unwrap_or_default();
    let state_name = if capture.contains("Press up to edit queued messages") {
        "queued"
    } else {
        "delivered"
    };
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

fn serve_local_identity(config: &HeyConfig) -> String {
    let node = config.node.as_deref().unwrap_or("local");
    let oracle = config.oracle.as_deref().unwrap_or(DEFAULT_ORACLE);
    format!("{node}:{oracle}")
}

fn serve_oracle_from_target(target: &str) -> String {
    target
        .split([':', '.'])
        .next()
        .unwrap_or(target)
        .to_owned()
}

fn serve_display_from(headers: &HeaderMap, config: &HeyConfig) -> String {
    let raw = header_to_string(headers, "x-maw-from");
    let raw = raw.trim();
    if raw.is_empty() {
        return serve_local_identity(config);
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
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
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
    let _ = query.local.unwrap_or(false);
    Json(Vec::<Value>::new())
}

async fn api_capture(Query(query): Query<CaptureQuery>) -> impl IntoResponse {
    Json(json!({"content": "", "target": query.target}))
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

async fn api_peers_info() -> impl IntoResponse {
    Json(serve_peers_info_payload()).into_response()
}

fn serve_peers_info_payload() -> Value {
    let identity = crate::core_impl::serveidentity_http_payload_read_only();
    let mut endpoints = identity
        .get("endpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    endpoints.extend([
        Value::String("/info".to_owned()),
        Value::String("/api/peers/info".to_owned()),
    ]);

    let mut payload = json!({
        "node": identity.get("node").cloned().unwrap_or_else(|| Value::String("local".to_owned())),
        "host": identity.get("host").cloned().unwrap_or_else(|| Value::String("local".to_owned())),
        "version": identity.get("version").cloned().unwrap_or_else(|| Value::String(MAW_RS_BUILD_VERSION.to_owned())),
        "ts": identity.get("clockUtc").cloned().unwrap_or(Value::Null),
        "endpoints": endpoints,
        "reachability": {"reachable": true, "status": "reachable"},
        "maw": {
            "schema": "1",
            "plugins": {"manifestEndpoint": "/api/plugins"},
            "capabilities": ["plugin.listManifest", "peer.handshake", "info", "peer.identity"],
        },
    });
    if let Some(port) = identity.get("port") {
        payload["port"] = port.clone();
    }
    payload
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

fn load_serve_workspace_key() -> Option<String> {
    if let Ok(value) = std::env::var("MAW_FEDERATION_TOKEN") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    value
        .get("federationToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn load_inbound_peer_pubkeys() -> Vec<ServePeerPubkey> {
    let env = real_xdg_env();
    let paths = [
        maw_state_path(&env, &["peers.json"]),
        maw_config_path(&env, &["maw.config.json"]),
    ];
    let mut entries = Vec::new();
    for path in paths {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        collect_peer_pubkeys(&value, None, &mut entries);
    }
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
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path,
        })
    }

    fn signed_trust_request(method: &str, uri: &str, auth_path: &str, body: &'static str) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(
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

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn serve_peers_info_routes_return_public_metadata_for_peer_probe() {
        let _guard = env_test_lock().lock().unwrap_or_else(|error| error.into_inner());
        let _restore_home = EnvVarRestore::capture("HOME");
        let _restore_maw_home = EnvVarRestore::capture("MAW_HOME");
        let _restore_maw_state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _restore_maw_config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _restore_peer = EnvVarRestore::capture("MAW_PEER_KEY");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-peers-info-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let home = root.join("home");
        let state = root.join("state");
        let config = root.join("config");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&state).expect("state");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::write(
            config.join("maw.config.json"),
            r#"{"node":"node-a","oracle":"oracle-a"}"#,
        )
        .expect("config");
        std::env::set_var("HOME", &home);
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_STATE_DIR", &state);
        std::env::set_var("MAW_CONFIG_DIR", &config);
        std::env::set_var("MAW_PEER_KEY", "pub-peers-info-test");

        assert!(!maw_auth::is_protected("/info", "GET"));
        assert!(!maw_auth::is_protected("/api/peers/info", "GET"));

        let app = serve_test_app_with_o6_keys(Vec::new(), 1_782_277_200, Some(NON_LOOPBACK_TEST_PEER));
        for path in ["/info", "/api/peers/info"] {
            let mut request = axum::http::Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("request");
            request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let payload = response_json(response).await;
            assert_eq!(payload["node"], "node-a");
            assert_eq!(payload["host"], "node-a");
            assert!(payload.get("oracle").is_none(), "/info must stay maw-js-safe fields only: {payload}");
            assert!(payload.get("identity").is_none(), "/info must not embed full identity payload: {payload}");
            assert!(payload.get("pubkey").is_none(), "/info must not expose peer_key: {payload}");
            assert!(!payload.to_string().contains("pub-peers-info-test"), "/info leaked peer_key: {payload}");
            assert_eq!(payload["reachability"]["status"], "reachable");
            assert_eq!(payload["maw"]["schema"], "1");
            assert!(payload["endpoints"]
                .as_array()
                .expect("endpoints")
                .iter()
                .any(|value| value == "/api/peers/info"));
        }

        let mut protected = axum::http::Request::builder()
            .method("GET")
            .uri("/api/trust")
            .body(Body::empty())
            .expect("request");
        protected
            .extensions_mut()
            .insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        let response = app.oneshot(protected).await.expect("protected response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let _ = std::fs::remove_dir_all(root);
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
            feed: Mutex::new(Vec::new()),
            peer_addr_override,
            now_override: Some(now),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("o6"),
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
        let headers = sign_headers_v3_at(key, from, method, path, Some(body.as_bytes()), now)
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
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("server"),
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
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: Some(fake_core),
            trust_store_path: serve_test_trust_store_path("agents"),
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
    async fn serve_real_wire_websocket_relay_echoes_text_frame() {
        let addr = spawn_test_server().await;
        let url = format!("ws://{addr}/ws");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            "relay-check".to_owned(),
        ))
        .await
        .expect("send websocket text");

        let received = ws
            .next()
            .await
            .expect("websocket should yield a frame")
            .expect("frame should be ok");
        assert_eq!(
            received,
            tokio_tungstenite::tungstenite::Message::Text("relay-check".to_owned())
        );
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
