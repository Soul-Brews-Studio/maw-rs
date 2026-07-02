const DISPATCH_139: &[DispatcherEntry] = &[DispatcherEntry {
    command: "serve-peer-startup-warnings",
    handler: Handler::Sync(servepeerstartupwarnings_run_command),
}];

const SERVEPEERSTARTUPWARNINGS_USAGE: &str = "usage: maw serve-peer-startup-warnings\n  Runs the serve startup peer warning checks without delegating to maw-js.";

#[derive(Debug, Clone, Default)]
struct ServepeerstartupwarningsConfig {
    port: Option<u16>,
    node: Option<String>,
    oracle: Option<String>,
    federation_token: Option<String>,
    peers_len: usize,
    named_peers_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServepeerstartupwarningsResult {
    missing_token_warned: bool,
    duplicate_scan_ran: bool,
    warnings: Vec<String>,
}

fn servepeerstartupwarnings_run_command(argv: &[String]) -> CliOutput {
    match servepeerstartupwarnings_run(argv) {
        Ok(result) => CliOutput {
            code: 0,
            stdout: String::new(),
            stderr: result.warnings.join(""),
        },
        Err((0, message)) => CliOutput {
            code: 0,
            stdout: format!("{message}\n"),
            stderr: String::new(),
        },
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn servepeerstartupwarnings_run(argv: &[String]) -> Result<ServepeerstartupwarningsResult, (i32, String)> {
    servepeerstartupwarnings_parse(argv)?;
    let config = servepeerstartupwarnings_load_config();
    let peers = servepeerstartupwarnings_load_peers();
    Ok(servepeerstartupwarnings_evaluate(&config, &peers, std::env::var("MAW_HOST").ok().as_deref()))
}

fn servepeerstartupwarnings_parse(argv: &[String]) -> Result<(), (i32, String)> {
    if let Some(arg) = argv.first() {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, SERVEPEERSTARTUPWARNINGS_USAGE.to_owned())),
            "--" => return Err((2, "serve-peer-startup-warnings: -- separator is not accepted".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("serve-peer-startup-warnings: unknown flag {value}"))),
            value if value.is_empty() || value.chars().any(char::is_control) => return Err((2, "serve-peer-startup-warnings: arguments must be printable".to_owned())),
            value => return Err((2, format!("serve-peer-startup-warnings: unexpected argument {value}"))),
        }
    }
    Ok(())
}

fn servepeerstartupwarnings_load_config() -> ServepeerstartupwarningsConfig {
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".to_owned());
    servepeerstartupwarnings_parse_config(&raw).unwrap_or_default()
}

fn servepeerstartupwarnings_parse_config(raw: &str) -> Option<ServepeerstartupwarningsConfig> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let object = value.as_object()?;
    Some(ServepeerstartupwarningsConfig {
        port: object.get("port").and_then(serde_json::Value::as_u64).and_then(|port| u16::try_from(port).ok()),
        node: object.get("node").and_then(|node| node.as_str()).map(str::to_owned),
        oracle: object.get("oracle").and_then(|oracle| oracle.as_str()).map(str::to_owned),
        federation_token: object.get("federationToken").and_then(|token| token.as_str()).filter(|token| !token.is_empty()).map(str::to_owned),
        peers_len: object.get("peers").and_then(|peers| peers.as_array()).map_or(0, Vec::len),
        named_peers_len: object.get("namedPeers").and_then(|peers| peers.as_array()).map_or(0, Vec::len),
    })
}

fn servepeerstartupwarnings_load_peers() -> maw_peer::PeerStoreFile {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "PEERS_FILE",
        "MAW_HOME",
        "MAW_STATE_DIR",
        "MAW_XDG",
        "XDG_STATE_HOME",
        "MAW_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "MAW_DATA_DIR",
        "XDG_DATA_HOME",
        "MAW_CACHE_DIR",
        "XDG_CACHE_HOME",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key, value)));
    let env = maw_peer::PeerStoreEnv::with_vars(home, vars);
    maw_peer::load_peer_store(&env)
}

fn servepeerstartupwarnings_evaluate(
    config: &ServepeerstartupwarningsConfig,
    peers: &maw_peer::PeerStoreFile,
    maw_host: Option<&str>,
) -> ServepeerstartupwarningsResult {
    let bind = resolve_bind_host(
        &BindConfig { peers_len: config.peers_len, named_peers_len: config.named_peers_len },
        maw_host,
        Ok(peers.peers.len()),
    );
    let mut warnings = Vec::new();
    let missing_token_warned = if bind.reason.is_some() && config.federation_token.is_none() {
        servepeerstartupwarnings_push_missing_token(config.port.unwrap_or(3456), &mut warnings);
        true
    } else {
        false
    };
    let duplicate_scan_ran = true;
    servepeerstartupwarnings_push_duplicate_warnings(config, peers, &mut warnings);
    ServepeerstartupwarningsResult { missing_token_warned, duplicate_scan_ran, warnings }
}

fn servepeerstartupwarnings_push_missing_token(port: u16, warnings: &mut Vec<String>) {
    warnings.push("\u{1b}[31m⚠ WARNING: peers configured but no federationToken set!\u{1b}[0m\n".to_owned());
    warnings.push(format!("\u{1b}[31m  Port {port} is exposed to network WITHOUT authentication.\u{1b}[0m\n"));
    warnings.push("\u{1b}[31m  Add \"federationToken\" (min 16 chars) to maw.config.json\u{1b}[0m\n".to_owned());
}

fn servepeerstartupwarnings_push_duplicate_warnings(
    config: &ServepeerstartupwarningsConfig,
    peers: &maw_peer::PeerStoreFile,
    warnings: &mut Vec<String>,
) {
    let local = config.node.as_ref().map(|node| (config.oracle.as_deref().unwrap_or("mawjs"), node.as_str()));
    let mut groups: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();
    if let Some((oracle, node)) = local {
        groups.insert(format!("{oracle}:{node}"), vec![("<local>".to_owned(), None)]);
    }
    for (alias, peer) in &peers.peers {
        let Some(identity) = &peer.identity else { continue; };
        if identity.oracle.is_empty() || identity.node.is_empty() { continue; }
        groups
            .entry(format!("{}:{}", identity.oracle, identity.node))
            .or_default()
            .push((alias.clone(), Some(peer.url.clone())));
    }
    for (key, claimants) in groups.into_iter().filter(|(_, claimants)| claimants.len() >= 2) {
        let tail = claimants
            .into_iter()
            .map(|(alias, url)| url.map_or(alias.clone(), |url| format!("{alias} ({url})")))
            .collect::<Vec<_>>()
            .join(", ");
        warnings.push(format!("\u{1b}[33m⚠ duplicate <oracle>:<node> claim \"{key}\" — {tail}\u{1b}[0m\n"));
        warnings.push("\u{1b}[33m  investigate with `maw peers list` and `maw peers remove <alias>` if stale.\u{1b}[0m\n".to_owned());
    }
}

#[cfg(test)]
mod servepeerstartupwarnings_tests {
    use super::*;

    fn peer(alias_url: &str, oracle: &str, node: &str) -> maw_peer::PeerRecord {
        maw_peer::PeerRecord {
            url: alias_url.to_owned(),
            node: None,
            added_at: "2026-06-24T09:00:00.000Z".to_owned(),
            last_seen: None,
            last_error: None,
            nickname: None,
            pubkey: None,
            pubkey_first_seen: None,
            identity: Some(maw_peer::PeerIdentity { oracle: oracle.to_owned(), node: node.to_owned() }),
            one_way: None,
            last_symmetric_check: None,
        }
    }

    #[test]
    fn servepeerstartupwarnings_dispatch_registers_native() {
        assert_eq!(dispatcher_status("serve-peer-startup-warnings"), DispatchKind::Native);
    }

    #[test]
    fn servepeerstartupwarnings_missing_token_and_duplicate_warnings_match_js_shape() {
        let config = ServepeerstartupwarningsConfig {
            port: Some(3099),
            node: Some("m5".to_owned()),
            oracle: Some("sender".to_owned()),
            named_peers_len: 1,
            ..ServepeerstartupwarningsConfig::default()
        };
        let mut peers = maw_peer::PeerStoreFile::default();
        peers.peers.insert("one".to_owned(), peer("https://one.example.test", "sender", "m5"));
        let result = servepeerstartupwarnings_evaluate(&config, &peers, None);
        assert!(result.missing_token_warned);
        assert!(result.duplicate_scan_ran);
        assert!(result.warnings.join("").contains("peers configured but no federationToken"));
        assert!(result.warnings.join("").contains("duplicate <oracle>:<node> claim \"sender:m5\""));
    }

    #[test]
    fn servepeerstartupwarnings_rejects_arguments_before_io() {
        assert!(servepeerstartupwarnings_parse(&["--bad".to_owned()]).is_err());
        assert!(servepeerstartupwarnings_parse(&["target".to_owned()]).is_err());
        assert!(servepeerstartupwarnings_parse(&["line\nbreak".to_owned()]).is_err());
    }
}
