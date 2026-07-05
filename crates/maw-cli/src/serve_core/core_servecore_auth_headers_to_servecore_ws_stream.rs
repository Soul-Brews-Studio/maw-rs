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
    Extension(config): Extension<modules::ws::WsConfig>,
) -> impl IntoResponse {
    let target = match modules::ws::ws_validate_target(servecore_ws_target(uri.query())) {
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
    if SERVECORE_WS_CONNECTIONS.load(Ordering::Relaxed) >= config.max_connections {
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
    config: modules::ws::WsConfig,
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

