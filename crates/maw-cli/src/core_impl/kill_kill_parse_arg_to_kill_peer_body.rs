fn kill_parse_arg(
    argv: &[String],
    index: usize,
    options: &mut KillOptions,
) -> Result<usize, String> {
    let arg = argv[index].as_str();
    match arg {
        "--all" => {
            options.all = true;
            Ok(1)
        }
        "--pane" => kill_parse_value_flag(argv, index, "--pane", |value| {
            options.pane = Some(kill_parse_non_negative(value, "--pane")?);
            Ok(())
        }),
        "--index" => kill_parse_value_flag(argv, index, "--index", |value| {
            options.index = Some(kill_parse_non_negative(value, "--index")?);
            Ok(())
        }),
        "--peer" => kill_parse_value_flag(argv, index, "--peer", |value| {
            kill_validate_user_target(value)?;
            options.peer = Some(value.to_owned());
            Ok(())
        }),
        value if value.starts_with("--pane=") => {
            options.pane = Some(kill_parse_non_negative(&value[7..], "--pane")?);
            Ok(1)
        }
        value if value.starts_with("--index=") => {
            options.index = Some(kill_parse_non_negative(&value[8..], "--index")?);
            Ok(1)
        }
        value if value.starts_with("--peer=") => {
            kill_validate_user_target(&value[7..])?;
            options.peer = Some(value[7..].to_owned());
            Ok(1)
        }
        value if value.starts_with('-') => Err(format!(
            "\"{value}\" looks like a flag, not a target.\n  usage: maw kill <target>  (see: maw sleep for graceful stop, maw done for worktrees)"
        )),
        value => {
            if !options.target.is_empty() {
                return Err(format!("kill: unexpected argument {value}"));
            }
            value.clone_into(&mut options.target);
            Ok(1)
        }
    }
}

fn kill_parse_value_flag<F>(
    argv: &[String],
    index: usize,
    flag: &str,
    mut assign: F,
) -> Result<usize, String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let value = argv
        .get(index + 1)
        .ok_or_else(|| format!("kill: missing {flag} value"))?;
    if value.starts_with('-') {
        return Err(format!("kill: {flag} value must not start with '-'"));
    }
    assign(value)?;
    Ok(2)
}


fn kill_peer_forward(
    options: &KillOptions,
    transport: &mut impl KillPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> Result<String, String> {
    kill_validate_user_target(&options.target)?;
    let alias = options.peer.as_deref().ok_or_else(|| "kill: missing --peer value".to_owned())?;
    kill_validate_peer_alias(alias)?;
    let peer = kill_resolve_peer(alias)?;
    let from = resolve_hey_wire_from(None, config)?;
    let request = KillPeerRequest {
        peer,
        target: options.target.clone(),
        pane: options.pane,
        index: options.index,
        all: options.all,
        from,
        peer_key: peer_key()?,
        timestamp: now(),
    };
    let response = transport.kill_peer(&request)?;
    let summary = format!(
        "\x1b[32m✓\x1b[0m forwarded kill → {} ({}) — {}",
        request.peer.alias, request.peer.url, request.target
    );
    Ok(response.output.filter(|out| !out.is_empty()).map_or_else(
        || format!("{summary}\n"),
        |out| format!("{summary}\n{out}"),
    ))
}

#[derive(Debug, serde::Deserialize, Default)]
struct KillPeersStore {
    #[serde(default)]
    peers: BTreeMap<String, KillPeerStoreEntry>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct KillPeerStoreEntry {
    url: Option<String>,
    node: Option<String>,
}

fn kill_resolve_peer(alias: &str) -> Result<KillPeer, String> {
    kill_validate_peer_alias(alias)?;
    let Some(raw) = kill_read_peers_json()? else {
        return Err(format!("unknown peer alias: {alias} (see: maw peers list)"));
    };
    let store = serde_json::from_str::<KillPeersStore>(&raw).unwrap_or_default();
    let Some(entry) = store.peers.get(alias) else {
        return Err(format!("unknown peer alias: {alias} (see: maw peers list)"));
    };
    let Some(url) = entry.url.as_deref() else {
        return Err(format!("unknown peer alias: {alias} (see: maw peers list)"));
    };
    kill_validate_peer_url(url)?;
    if let Some(node) = entry.node.as_deref() {
        kill_validate_peer_alias(node).map_err(|_| format!("invalid peer node for {alias}"))?;
    }
    Ok(KillPeer { alias: alias.to_owned(), url: url.to_owned(), node: entry.node.clone() })
}

fn kill_read_peers_json() -> Result<Option<String>, String> {
    let primary = kill_peers_path();
    if primary.exists() {
        return std::fs::read_to_string(&primary)
            .map(Some)
            .map_err(|error| format!("peers: read {}: {error}", primary.display()));
    }
    if std::env::var_os("PEERS_FILE").is_none() && std::env::var_os("MAW_HOME").is_none() {
        let legacy = kill_legacy_peers_path();
        if legacy != primary && legacy.exists() {
            return std::fs::read_to_string(&legacy)
                .map(Some)
                .map_err(|error| format!("peers: read {}: {error}", legacy.display()));
        }
    }
    Ok(None)
}

fn kill_peers_path() -> std::path::PathBuf {
    std::env::var_os("PEERS_FILE").map_or_else(
        || maw_state_path(&current_xdg_env(), &["peers.json"]),
        std::path::PathBuf::from,
    )
}

fn kill_legacy_peers_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from(".maw/peers.json"), |home| std::path::PathBuf::from(home).join(".maw/peers.json"))
}

fn kill_validate_peer_alias(alias: &str) -> Result<(), String> {
    let mut chars = alias.chars();
    let Some(first) = chars.next() else { return Err("peer alias must be non-empty".to_owned()); };
    let valid_first = first.is_ascii_lowercase() || first.is_ascii_digit();
    let valid_rest = chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-');
    if alias.len() <= 32 && valid_first && valid_rest {
        Ok(())
    } else {
        Err(format!("invalid peer alias \"{alias}\" (must match ^[a-z0-9][a-z0-9_-]{{0,31}}$)"))
    }
}

fn kill_validate_peer_url(value: &str) -> Result<(), String> {
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err("peer url must start with http:// or https://".to_owned());
    }
    if value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) {
        return Err("peer url must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn kill_validate_peer_request(request: &KillPeerRequest) -> Result<(), String> {
    kill_validate_peer_alias(&request.peer.alias)?;
    kill_validate_peer_url(&request.peer.url)?;
    kill_validate_user_target(&request.target)?;
    if request.from.is_empty() || request.peer_key.is_empty() || request.timestamp <= 0 {
        return Err("peer kill request auth fields are incomplete".to_owned());
    }
    Ok(())
}

fn kill_peer_body(request: &KillPeerRequest) -> Result<String, String> {
    kill_validate_peer_request(request)?;
    let mut body = serde_json::Map::new();
    body.insert("target".to_owned(), serde_json::Value::String(request.target.clone()));
    if let Some(pane) = request.pane { body.insert("pane".to_owned(), serde_json::Value::from(pane)); }
    if let Some(index) = request.index { body.insert("index".to_owned(), serde_json::Value::from(index)); }
    if request.all { body.insert("all".to_owned(), serde_json::Value::Bool(true)); }
    serde_json::to_string(&serde_json::Value::Object(body)).map_err(|error| error.to_string())
}

