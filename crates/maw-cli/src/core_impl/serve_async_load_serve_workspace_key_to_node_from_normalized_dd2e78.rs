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

