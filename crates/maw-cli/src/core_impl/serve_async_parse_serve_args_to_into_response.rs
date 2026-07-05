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


