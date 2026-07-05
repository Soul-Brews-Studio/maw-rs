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

