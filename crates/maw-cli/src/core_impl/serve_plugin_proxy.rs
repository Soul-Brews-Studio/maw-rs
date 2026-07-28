// Serving a plugin's own HTTP surface through maw serve.
//
// A plugin manifest may declare `engine.serve` with a prefix, a health path and
// an event stream. maw serve discovers those at startup, refuses ones that would
// collide with a native route, then lazily boots the plugin on a loopback port
// and reverse-proxies matching requests to it -- including the WebSocket upgrade,
// which has to be bridged frame-by-frame in both directions.

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
    ws: Option<WebSocketUpgrade>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(route) = serve_plugin_route_for_prefix(&state, uri.path()) else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "plugin route not found"}))).into_response();
    };
    if let Some(ws) = ws {
        let route = route.clone();
        return ws.on_upgrade(move |socket| serve_proxy_websocket(route, uri, headers, socket)).into_response();
    }
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
    let mut request = client.request(req_method, &target).body(body.to_vec());
    if let Some(content_type) = headers.get(axum::http::header::CONTENT_TYPE) {
        request = request.header(reqwest::header::CONTENT_TYPE, content_type.as_bytes());
    }
    match request.send().await {
        Ok(response) => {
            if response.status() == reqwest::StatusCode::NOT_FOUND && uri.path() != route.health_path && serve_spa_fallback_path(&method, uri).is_some() {
                if let Ok(fallback) = client.get(format!("http://127.0.0.1:{port}{}/index.html", route.prefix)).send().await {
                    return serve_proxy_response(fallback).await;
                }
            }
            serve_proxy_response(response).await
        },
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": format!("proxy failed: {error}"), "plugin": route.name}))).into_response(),
    }
}

fn serve_spa_fallback_path(method: &Method, uri: &Uri) -> Option<()> {
    (method == Method::GET && !uri.path().rsplit('/').next().is_some_and(|segment| segment.contains('.'))).then_some(())
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

async fn serve_proxy_websocket(route: ServePluginRoute, uri: Uri, headers: HeaderMap, socket: axum::extract::ws::WebSocket) {
    let Ok(port) = serve_plugin_process_port(&route) else { return; };
    let target = format!("ws://127.0.0.1:{port}{}", uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string));
    let Ok(mut request) = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(target) else { return; };
    if let Some(protocol) = headers.get(axum::http::header::SEC_WEBSOCKET_PROTOCOL).and_then(|value| value.to_str().ok()) {
        if let Ok(value) = protocol.parse() { request.headers_mut().insert("sec-websocket-protocol", value); }
    }
    let Ok((upstream, _)) = tokio_tungstenite::connect_async(request).await else { return; };
    let (mut client_tx, mut client_rx) = socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    loop {
        tokio::select! {
            from_client = client_rx.next() => match from_client {
                Some(Ok(message)) => if let Some(message) = serve_ws_to_upstream(message) { if upstream_tx.send(message).await.is_err() { break; } },
                _ => break,
            },
            from_upstream = upstream_rx.next() => match from_upstream {
                Some(Ok(message)) => if let Some(message) = serve_ws_to_client(message) { if client_tx.send(message).await.is_err() { break; } },
                _ => break,
            },
        }
    }
}

fn serve_ws_to_upstream(message: axum::extract::ws::Message) -> Option<tokio_tungstenite::tungstenite::Message> {
    match message {
        axum::extract::ws::Message::Text(text) => Some(tokio_tungstenite::tungstenite::Message::Text(text)),
        axum::extract::ws::Message::Binary(bytes) => Some(tokio_tungstenite::tungstenite::Message::Binary(bytes)),
        axum::extract::ws::Message::Ping(bytes) => Some(tokio_tungstenite::tungstenite::Message::Ping(bytes)),
        axum::extract::ws::Message::Pong(bytes) => Some(tokio_tungstenite::tungstenite::Message::Pong(bytes)),
        axum::extract::ws::Message::Close(_) => None,
    }
}

fn serve_ws_to_client(message: tokio_tungstenite::tungstenite::Message) -> Option<axum::extract::ws::Message> {
    match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => Some(axum::extract::ws::Message::Text(text)),
        tokio_tungstenite::tungstenite::Message::Binary(bytes) => Some(axum::extract::ws::Message::Binary(bytes)),
        tokio_tungstenite::tungstenite::Message::Ping(bytes) => Some(axum::extract::ws::Message::Ping(bytes)),
        tokio_tungstenite::tungstenite::Message::Pong(bytes) => Some(axum::extract::ws::Message::Pong(bytes)),
        tokio_tungstenite::tungstenite::Message::Close(_) | tokio_tungstenite::tungstenite::Message::Frame(_) => None,
    }
}
