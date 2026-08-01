// This node's federation identity: who it is, who it knows, what it signs with.
//
// Node name, named peers, the Ed25519 signing key and the shared federation
// token, each env-or-config so a node can be provisioned without a secret store.
// The signing key is generated on first use and written with tight permissions;
// a peer's auth state is probed rather than assumed.

fn load_hey_config() -> HeyConfig {
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    let node = value
        .get("node")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let oracle = value
        .get("oracle")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let peers = value
        .get("peers")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let named_peers = parse_named_peers(value.get("namedPeers"));
    // #681 (read-side): config `namedPeers` and the peer store (`peers.json`) are
    // two separate registries — `maw peers map` / `/fed.json` read the store while
    // the `hey` router read only config — so a peer could be registered, probed
    // green, and still unroutable. Fall back to the store so "green in the map"
    // implies "addressable by hey". Config wins on a name conflict (operator
    // intent); store aliases already named in config are skipped.
    let named_peers = merge_peer_store_named_peers(named_peers);
    let agents = value
        .get("agents")
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|node| (key.clone(), node.to_owned())))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    HeyConfig {
        node: node.clone(),
        oracle,
        route: RouteConfig {
            node,
            named_peers,
            peers,
            agents,
        },
    }
}

fn parse_named_peers(value: Option<&serde_json::Value>) -> Vec<RouteNamedPeer> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                Some(RouteNamedPeer {
                    name: item.get("name")?.as_str()?.to_owned(),
                    url: item.get("url")?.as_str()?.to_owned(),
                })
            })
            .collect(),
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .filter_map(|(name, value)| {
                value.as_str().map(|url| RouteNamedPeer {
                    name: name.clone(),
                    url: url.to_owned(),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Append peer-store (`peers.json`) aliases that config `namedPeers` does not
/// already cover, so `maw hey <alias>:...` can route to a peer added via
/// `maw peers add` / `pair` (#681 read-side unification). Config wins on a name
/// conflict; empty URLs and an unreadable store are skipped (best-effort).
fn merge_peer_store_named_peers(mut config: Vec<RouteNamedPeer>) -> Vec<RouteNamedPeer> {
    let known: std::collections::HashSet<String> = config.iter().map(|peer| peer.name.clone()).collect();
    for (alias, peer) in peers_load_store().peers {
        if known.contains(&alias) || peer.url.trim().is_empty() {
            continue;
        }
        config.push(RouteNamedPeer { name: alias, url: peer.url });
    }
    config
}

fn load_peer_key() -> Result<String, String> {
    if let Ok(value) = std::env::var("MAW_PEER_KEY") {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    let env = real_xdg_env();
    let path = maw_state_path(&env, &["peer-key"]);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let key = raw.trim().to_owned();
        if !key.is_empty() {
            return Ok(key);
        }
    }
    let key = generate_peer_key()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create peer-key directory: {error}"))?;
    }
    write_peer_key_file(&path, &key)?;
    Ok(key)
}

fn load_federation_token() -> Result<String, String> {
    load_serve_workspace_key()
        .ok_or_else(|| "federationToken is required for peer federation auth".to_owned())
}

/// Read-only auth probe used by `maw peers` to learn whether OUR signed
/// requests are trusted by a peer, without delivering anything: sign a
/// `POST /api/probe` (the peer verifies the v3 from-signature and returns
/// `{ok:true, sessions:[]}` — no side effect). Reuses the exact send-path
/// credential assembly (`from`, peer key, federation token, signing), so a
/// green result here means a real `maw hey` to this peer would also
/// authenticate. `Some(true)` trusted, `Some(false)` refused (401/403),
/// `None` when we cannot even sign (no key/token/identity) or on error.
pub(crate) fn federation_probe_auth(peer_url: &str, timeout_ms: u64) -> PeerProbeAuthResult {
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle_for_from(&config, None);
    let Ok(from) = resolve_hey_wire_from(None, &config, &sender_oracle) else {
        return PeerProbeAuthResult::default();
    };
    let Ok(peer_key) = load_peer_key() else {
        return PeerProbeAuthResult::default();
    };
    let Ok(federation_token) = load_federation_token() else {
        return PeerProbeAuthResult::default();
    };
    let request = PeerWakeRequest {
        peer_url: peer_url.to_owned(),
        target: String::new(),
        task: None,
        from,
        federation_token,
        peer_key,
        timestamp: i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX),
    };
    // Own tokio runtime on a scratch thread so this stays callable from the
    // synchronous probe path (mirrors peers_fetch_info).
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        let client = ReqwestHttpTransportIo::new(timeout_ms).ok()?;
        runtime.block_on(client.probe_peer_auth(&request)).ok()
    })
    .join()
    .ok()
    .flatten()
    .unwrap_or_default()
}

fn generate_peer_key() -> Result<String, String> {
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|error| format!("failed to open random peer key source: {error}"))?;
    let mut bytes = [0_u8; 32];
    std::io::Read::read_exact(&mut file, &mut bytes)
        .map_err(|error| format!("failed to read random peer key bytes: {error}"))?;
    Ok(hex_bytes(&bytes))
}

fn write_peer_key_file(path: &std::path::Path, key: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, key.as_bytes())
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, b"\n")
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, format!("{key}\n"))
            .map_err(|error| format!("failed to write peer-key: {error}"))
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn real_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "MAW_TEST_MODE",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok().map(|value| (name.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

fn route_sessions_from_tmux(
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
) -> Vec<RouteSession> {
    tmux_sessions_to_route_sessions(tmux.list_all())
}
