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
    servecore_mount_ws_routes_with_config(router, modules::ws::WsConfig::ws_from_process_env())
}

pub fn servecore_mount_ws_routes_with_config<S>(
    router: Router<S>,
    config: modules::ws::WsConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let registry = servecore_default_ws_registry();
    servecore_mount_ws_registry(router, &registry).layer(Extension(config))
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

fn servecore_default_ws_registry() -> ServecoreWsRegistry {
    let mut registry = ServecoreWsRegistry::default();
    registry
        .servecore_register_ws_kind("/ws", ServecoreWsKind::Engine)
        .expect("default ws route");
    registry
        .servecore_register_ws_kind("/ws/pty", ServecoreWsKind::Pty)
        .expect("default pty ws route");
    registry
        .servecore_register_ws_kind("/ws/tmux", ServecoreWsKind::Tmux)
        .expect("default tmux ws route");
    registry
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
        modules::views::ViewsConfig::views_from_process_env(),
    )
}

pub fn servecore_apply_pipeline_with_views_config<S>(
    router: Router<S>,
    views_config: modules::views::ViewsConfig,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    modules::views::views_apply_fallback_with_config(router, views_config)
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
    if req.method() == Method::OPTIONS {
        return StatusCode::NO_CONTENT.into_response();
    }
    next.run(req).await
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

