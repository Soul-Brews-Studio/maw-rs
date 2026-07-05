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

