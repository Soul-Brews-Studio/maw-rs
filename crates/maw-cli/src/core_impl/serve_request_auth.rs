// Deciding whether an inbound request is really from the peer it claims to be.
//
// Loopback is trusted outright; everything else must carry a v3 signature that
// verifies against a pubkey this node already knows for that sender. Resolving
// that pubkey is most of the work -- peers.json and the config both hold peer
// identities in several historical shapes, so they are collected, normalized,
// and cached behind a short TTL. An unsigned or unknown sender is refused with
// the concrete reason, never a bare no.

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
    let cached_pubkey = match resolve_request_cached_pubkey(
        state,
        &auth_headers,
        u64::try_from(now).unwrap_or(0),
    ) {
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
    let refusal = if matches!(&decision, FromVerifyDecision::AcceptLegacy { .. }) {
        Some("refuse-unsigned")
    } else if maw_auth::is_refuse_decision(&decision) {
        Some(decision.kind())
    } else {
        None
    };
    if let Some(kind) = refusal {
        return ProtectedRequestOutcome::Reject {
            decision: kind.to_owned(),
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
        (
            "x-maw-ed25519-signature",
            header_to_string(headers, "x-maw-ed25519-signature"),
        ),
        (
            "x-maw-signature-ed25519",
            header_to_string(headers, "x-maw-signature-ed25519"),
        ),
        (
            "x-maw-from-signature-ed25519",
            header_to_string(headers, "x-maw-from-signature-ed25519"),
        ),
        (
            "x-maw-ed25519-pubkey",
            header_to_string(headers, "x-maw-ed25519-pubkey"),
        ),
        (
            "x-maw-pubkey",
            header_to_string(headers, "x-maw-pubkey"),
        ),
        (
            "x-maw-peer-pubkey",
            header_to_string(headers, "x-maw-peer-pubkey"),
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
    now_secs: u64,
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
    // Reload (subject to TTL) so a peer added after serve booted is trusted
    // without a restart (#16).
    let peer_pubkeys = state.peer_pubkeys.get(now_secs);
    if let Some(entry) = peer_pubkeys.iter().find(|entry| entry.from == from) {
        return Ok(Some(entry.pubkey.clone()));
    }
    let Some(node) = node_from_identity(&from) else {
        return Err("refuse-missing-peer-key");
    };
    let mut node_matches = peer_pubkeys
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
        // Top-level oracle+node: oracle may be absent or empty (see
        // normalize_from_identity comment); only node is required.
        let top_oracle = map.get("oracle").and_then(Value::as_str).unwrap_or("").trim().to_owned();
        if let Some(node) = map.get("node").and_then(Value::as_str).map(str::trim) {
            if let Some(from) = normalize_from_identity(&format!("{top_oracle}:{node}")) {
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
    // oracle may be absent or empty (see normalize_from_identity comment);
    // only node is required here.
    let oracle = map.get("oracle").and_then(Value::as_str).unwrap_or("").trim();
    let node = map.get("node").and_then(Value::as_str)?.trim();
    normalize_from_identity(&format!("{oracle}:{node}"))
}

fn normalize_from_identity(value: &str) -> Option<String> {
    let value = value.trim();
    let (oracle, node) = value.split_once(':')?;
    let oracle = oracle.trim();
    let node = node.trim();
    if node.is_empty()
        || oracle.starts_with('-')
        || node.starts_with('-')
        || oracle.bytes().any(|byte| byte.is_ascii_control())
        || node.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    // An empty oracle is acceptable — the node name + pubkey are sufficient
    // to identify a peer. This was a regression after #677 removed the
    // fabricated "mawjs" default: nodes that never configured an oracle
    // (e.g. maclab, white) became impossible to authenticate because their
    // pubkey entries were silently dropped by the old `oracle.is_empty()`
    // guard here. The node-only fallback in resolve_request_cached_pubkey
    // handles the lookup, but only if the entry is created in the first
    // place — which requires this function to accept an empty oracle.
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
