const DISPATCH_141: &[DispatcherEntry] = &[DispatcherEntry {
    command: "federation",
    handler: Handler::Sync(federation_run_command),
}];

const FEDERATION_USAGE: &str = "usage: maw federation <status|sync> [--json|--dry-run|--check|--prune|--force|--peers config|both]";
const FEDERATION_CURL_TIMEOUT_SECONDS: &str = "5";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FederationPeer134 {
    name: String,
    url: String,
    node: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct FederationOptions134 {
    json: bool,
    dry_run: bool,
    check: bool,
    prune: bool,
    force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FederationStatusRow134 {
    url: String,
    node: Option<String>,
    reachable: bool,
    latency: Option<u64>,
    agents: Vec<String>,
    error: Option<String>,
}

trait FederationTransport134 {
    fn federation_get(&mut self, url: &str, path: &str) -> Result<String, String>;
}

struct FederationCurlTransport134;

impl FederationTransport134 for FederationCurlTransport134 {
    fn federation_get(&mut self, url: &str, path: &str) -> Result<String, String> {
        let argv = federation_curl_argv(url, path)?;
        let output = std::process::Command::new("curl")
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("failed to spawn curl: {error}"))?;
        if !output.status.success() {
            return Err("peer fetch failed".to_owned());
        }
        String::from_utf8(output.stdout).map_err(|error| format!("curl stdout was not utf8: {error}"))
    }
}

fn federation_run_command(argv: &[String]) -> CliOutput {
    let mut transport = FederationCurlTransport134;
    federation_run_with(argv, &load_hey_config(), &mut transport)
}

fn federation_run_with(
    argv: &[String],
    config: &HeyConfig,
    transport: &mut impl FederationTransport134,
) -> CliOutput {
    match federation_dispatch(argv, config, transport) {
        Ok(output) => output,
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("federation: {message}\n"),
        },
    }
}

fn federation_dispatch(
    argv: &[String],
    config: &HeyConfig,
    transport: &mut impl FederationTransport134,
) -> Result<CliOutput, (i32, String)> {
    federation_validate_argv(argv).map_err(|message| (2, message))?;
    let (subcommand, rest) = federation_subcommand(argv);
    match subcommand {
        "status" | "ls" => federation_status(rest, config, transport),
        "sync" => federation_sync(rest, config, transport),
        "help" | "--help" | "-h" => Ok(federation_ok(&format!("{FEDERATION_USAGE}\n"))),
        other => Err((2, format!("unknown subcommand {other:?}. {FEDERATION_USAGE}"))),
    }
}

fn federation_subcommand(argv: &[String]) -> (&str, &[String]) {
    match argv.first().map(String::as_str) {
        None => ("status", argv),
        Some(value) if value.starts_with('-') => ("status", argv),
        Some(value) => (value, &argv[1..]),
    }
}

fn federation_status(
    argv: &[String],
    config: &HeyConfig,
    transport: &mut impl FederationTransport134,
) -> Result<CliOutput, (i32, String)> {
    let options = federation_parse_options(argv).map_err(|message| (2, message))?;
    let local_node = federation_local_node(config);
    let local_url = federation_local_url();
    let peers = federation_resolve_peers(config)?;
    let rows = peers
        .iter()
        .map(|peer| federation_fetch_status_peer(peer, transport))
        .collect::<Vec<_>>();
    let stdout = if options.json {
        federation_status_json(&local_node, &local_url, &rows)
    } else {
        federation_status_text(&local_node, &local_url, &rows)
    };
    Ok(federation_ok(&stdout))
}

fn federation_sync(
    argv: &[String],
    config: &HeyConfig,
    transport: &mut impl FederationTransport134,
) -> Result<CliOutput, (i32, String)> {
    let options = federation_parse_options(argv).map_err(|message| (2, message))?;
    let local_node = federation_local_node(config);
    let peers = federation_resolve_peers(config)?;
    let identities = peers
        .iter()
        .map(|peer| federation_fetch_identity(peer, transport))
        .collect::<Vec<_>>();
    let diff = compute_sync_diff(&config.route.agents, &identities, &local_node);
    let dirty = federation_sync_dirty(&diff);
    if dirty && !options.json && !options.dry_run && !options.check {
        return Err((
            2,
            "live federation sync write is pending native safety review; use --json, --dry-run, or --check".to_owned(),
        ));
    }
    let apply = federation_apply_preview(options, &config.route.agents, &diff);
    let code = i32::from(options.check && dirty);
    let stdout = if options.json {
        federation_sync_json(&local_node, options, dirty, &diff, &apply)
    } else {
        federation_sync_text(options, &diff, &apply)
    };
    Ok(CliOutput { code, stdout, stderr: String::new() })
}

fn federation_parse_options(argv: &[String]) -> Result<FederationOptions134, String> {
    let mut options = FederationOptions134::default();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--json" => options.json = true,
            "--dry-run" => options.dry_run = true,
            "--check" => options.check = true,
            "--prune" => options.prune = true,
            "--force" => options.force = true,
            "--peers" => {
                let Some(value) = argv.get(index + 1) else { return Err("--peers requires a value".to_owned()); };
                federation_validate_peer_source(value)?;
                index += 1;
            }
            value if value.starts_with("--peers=") => federation_validate_peer_source(&value[8..])?,
            "--verify" => return Err("--verify pair-symmetric check is not native in ZERO-BUN B2".to_owned()),
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            other => return Err(format!("unexpected argument {other:?}. {FEDERATION_USAGE}")),
        }
        index += 1;
    }
    Ok(options)
}

fn federation_validate_peer_source(value: &str) -> Result<(), String> {
    federation_validate_token(value, "peer source")?;
    if matches!(value, "config" | "both") {
        Ok(())
    } else {
        Err("--peers supports config|both in native ZERO-BUN B2".to_owned())
    }
}

fn federation_resolve_peers(config: &HeyConfig) -> Result<Vec<FederationPeer134>, (i32, String)> {
    let mut peers = Vec::new();
    for peer in &config.route.named_peers {
        federation_push_peer(&mut peers, &peer.name, &peer.url, Some(&peer.name))?;
    }
    for url in &config.route.peers {
        let name = federation_name_from_url(url);
        federation_push_peer(&mut peers, &name, url, None)?;
    }
    let store = peers_load_store();
    for (alias, peer) in store.peers {
        federation_push_peer(&mut peers, &alias, &peer.url, peer.node.as_deref())?;
    }
    Ok(peers)
}

fn federation_push_peer(
    peers: &mut Vec<FederationPeer134>,
    name: &str,
    url: &str,
    node: Option<&str>,
) -> Result<(), (i32, String)> {
    federation_validate_token(name, "peer name").map_err(|message| (2, message))?;
    federation_validate_url(url).map_err(|message| (2, message))?;
    if let Some(node) = node {
        federation_validate_token(node, "peer node").map_err(|message| (2, message))?;
    }
    if peers.iter().any(|existing| existing.url == url || existing.name == name) {
        return Ok(());
    }
    peers.push(FederationPeer134 { name: name.to_owned(), url: url.to_owned(), node: node.map(ToOwned::to_owned) });
    Ok(())
}

fn federation_fetch_status_peer(
    peer: &FederationPeer134,
    transport: &mut impl FederationTransport134,
) -> FederationStatusRow134 {
    match transport.federation_get(&peer.url, "/api/federation/status") {
        Ok(raw) => federation_parse_status_peer(peer, &raw),
        Err(error) => FederationStatusRow134 { url: peer.url.clone(), node: peer.node.clone().or_else(|| Some(peer.name.clone())), reachable: false, latency: None, agents: Vec::new(), error: Some(error) },
    }
}

fn federation_parse_status_peer(peer: &FederationPeer134, raw: &str) -> FederationStatusRow134 {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return FederationStatusRow134 { url: peer.url.clone(), node: peer.node.clone().or_else(|| Some(peer.name.clone())), reachable: false, latency: None, agents: Vec::new(), error: Some("invalid status json".to_owned()) };
    };
    let agents = federation_agents_from_status(&value);
    let node = value.get("node").and_then(serde_json::Value::as_str).map(ToOwned::to_owned).or_else(|| peer.node.clone()).or_else(|| Some(peer.name.clone()));
    FederationStatusRow134 { url: peer.url.clone(), node, reachable: true, latency: Some(0), agents, error: None }
}

fn federation_fetch_identity(
    peer: &FederationPeer134,
    transport: &mut impl FederationTransport134,
) -> SyncPeerIdentity {
    match transport.federation_get(&peer.url, "/api/identity") {
        Ok(raw) => federation_parse_identity(peer, &raw),
        Err(error) => SyncPeerIdentity { peer_name: peer.name.clone(), url: peer.url.clone(), node: peer.node.clone().unwrap_or_else(|| peer.name.clone()), agents: Vec::new(), reachable: false, error: Some(error) },
    }
}

fn federation_parse_identity(peer: &FederationPeer134, raw: &str) -> SyncPeerIdentity {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => SyncPeerIdentity { peer_name: peer.name.clone(), url: peer.url.clone(), node: federation_identity_node(peer, &value), agents: federation_identity_agents(&value), reachable: true, error: None },
        Err(error) => SyncPeerIdentity { peer_name: peer.name.clone(), url: peer.url.clone(), node: peer.node.clone().unwrap_or_else(|| peer.name.clone()), agents: Vec::new(), reachable: false, error: Some(format!("invalid identity json: {error}")) },
    }
}

fn federation_identity_node(peer: &FederationPeer134, value: &serde_json::Value) -> String {
    value
        .get("node")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| peer.node.clone())
        .unwrap_or_else(|| peer.name.clone())
}

fn federation_identity_agents(value: &serde_json::Value) -> Vec<String> {
    value.get("agents").map_or_else(Vec::new, federation_string_array)
}

fn federation_agents_from_status(value: &serde_json::Value) -> Vec<String> {
    if let Some(agents) = value.get("agents") {
        return federation_string_array(agents);
    }
    value
        .get("peers")
        .and_then(serde_json::Value::as_array)
        .and_then(|peers| peers.iter().find_map(|peer| peer.get("agents")))
        .map_or_else(Vec::new, federation_string_array)
}

fn federation_string_array(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(ToOwned::to_owned).filter(|item| federation_validate_token(item, "agent").is_ok()).collect())
        .unwrap_or_default()
}

fn federation_sync_dirty(diff: &SyncDiff) -> bool {
    !(diff.add.is_empty() && diff.stale.is_empty() && diff.conflict.is_empty())
}

fn federation_apply_preview(
    options: FederationOptions134,
    agents: &HashMap<String, String>,
    diff: &SyncDiff,
) -> SyncApplyResult {
    if options.dry_run || options.check || options.json {
        return SyncApplyResult { agents: agents.clone(), applied: Vec::new() };
    }
    apply_sync_diff(agents, diff, SyncApplyOptions { force: options.force, prune: options.prune })
}

fn federation_status_text(local_node: &str, local_url: &str, rows: &[FederationStatusRow134]) -> String {
    let mut out = format!("\nFederation Status\n{} nodes (1 local + {} peers)\n\n", rows.len() + 1, rows.len());
    let _ = writeln!(out, "  ●  {local_node} (local)  online  0ms · 0 agents");
    let _ = writeln!(out, "     {local_url}");
    for row in rows {
        let status = if row.reachable { "online" } else { "offline" };
        let agents = row.agents.len();
        let node = row.node.as_deref().unwrap_or("unknown");
        let _ = writeln!(out, "  {}  {node}  {status}  {}ms · {agents} agents", if row.reachable { "●" } else { "○" }, row.latency.unwrap_or(0));
        let _ = writeln!(out, "     {}", row.url);
    }
    let reachable = rows.iter().filter(|row| row.reachable).count();
    let _ = writeln!(out, "\n{reachable}/{} reachable (one-way; use --verify for pair-symmetric check)", rows.len());
    out
}

fn federation_status_json(local_node: &str, local_url: &str, rows: &[FederationStatusRow134]) -> String {
    let peers = rows.iter().map(federation_status_peer_json).collect::<Vec<_>>().join(",");
    format!("{{\"command\":\"federation\",\"action\":\"status\",\"node\":{},\"localUrl\":{},\"peers\":[{}]}}\n", json_string(local_node), json_string(local_url), peers)
}

fn federation_status_peer_json(peer: &FederationStatusRow134) -> String {
    let mut fields = vec![format!("\"url\":{}", json_string(&peer.url)), format!("\"reachable\":{}", peer.reachable)];
    push_json_opt(&mut fields, "node", peer.node.as_deref());
    if let Some(latency) = peer.latency { fields.push(format!("\"latency\":{latency}")); }
    fields.push(format!("\"agents\":{}", json_string_array(&peer.agents)));
    push_json_opt(&mut fields, "error", peer.error.as_deref());
    format!("{{{}}}", fields.join(","))
}

fn federation_sync_json(
    node: &str,
    options: FederationOptions134,
    dirty: bool,
    diff: &SyncDiff,
    result: &SyncApplyResult,
) -> String {
    format!("{{\"command\":\"federation\",\"action\":\"sync\",\"node\":{},\"dryRun\":{},\"check\":{},\"force\":{},\"prune\":{},\"dirty\":{dirty},\"diff\":{},\"applied\":{},\"agents\":{}}}\n", json_string(node), options.dry_run, options.check, options.force, options.prune, render_sync_diff_json(diff), json_string_array(&result.applied), render_agents_json(&result.agents))
}

fn federation_sync_text(options: FederationOptions134, diff: &SyncDiff, result: &SyncApplyResult) -> String {
    format!("federation sync add={} conflict={} stale={} unreachable={} applied={} dryRun={} check={} force={} prune={}\n", diff.add.len(), diff.conflict.len(), diff.stale.len(), diff.unreachable.len(), result.applied.len(), options.dry_run, options.check, options.force, options.prune)
}

fn federation_curl_argv(peer_url: &str, path: &str) -> Result<Vec<String>, String> {
    federation_validate_url(peer_url)?;
    federation_validate_path(path)?;
    let argv = vec!["-sS".to_owned(), "--fail-with-body".to_owned(), "--max-time".to_owned(), FEDERATION_CURL_TIMEOUT_SECONDS.to_owned(), "--".to_owned(), format!("{}{}", peer_url.trim_end_matches('/'), path)];
    federation_validate_curl_argv(&argv)?;
    Ok(argv)
}

fn federation_validate_curl_argv(argv: &[String]) -> Result<(), String> {
    if !argv.iter().any(|arg| arg == "--") { return Err("curl argv must include -- URL separator".to_owned()); }
    if argv.iter().any(|arg| arg.chars().any(|ch| ch == '\0' || ch.is_control())) { return Err("curl argv must not contain NUL/control characters".to_owned()); }
    Ok(())
}

fn federation_validate_argv(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        if arg == "--" || arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("arguments must not contain -- separator or control characters".to_owned());
        }
    }
    Ok(())
}

fn federation_validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.len() > 64 {
        return Err(format!("{label} must be a safe token"));
    }
    if value.chars().any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')) {
        return Err(format!("{label} must contain only ascii alnum, dot, underscore, or hyphen"));
    }
    Ok(())
}

fn federation_validate_url(value: &str) -> Result<(), String> {
    if value.starts_with('-') || value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) { return Err("peer URL must be a safe http(s) URL".to_owned()); }
    if !(value.starts_with("http://") || value.starts_with("https://")) { return Err("peer URL must start with http:// or https://".to_owned()); }
    let rest = value.split_once("://").map_or("", |(_, rest)| rest);
    if rest.is_empty() || rest.starts_with('/') { return Err("peer URL must include a host".to_owned()); }
    Ok(())
}

fn federation_validate_path(path: &str) -> Result<(), String> {
    if !path.starts_with("/api/") || path.contains("..") || path.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) { return Err("unsafe federation path".to_owned()); }
    Ok(())
}

fn federation_name_from_url(url: &str) -> String {
    let host = url.split_once("://").map_or(url, |(_, rest)| rest).split(['/', ':']).next().unwrap_or("peer");
    host.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.').collect::<String>().trim_matches('.').to_owned()
}

fn federation_local_node(config: &HeyConfig) -> String {
    config.node.clone().unwrap_or_else(|| "local".to_owned())
}

fn federation_local_url() -> String {
    format!("http://127.0.0.1:{}", load_hey_config_port().unwrap_or(31_745))
}

fn federation_ok(stdout: &str) -> CliOutput {
    CliOutput { code: 0, stdout: stdout.to_owned(), stderr: String::new() }
}

#[cfg(test)]
mod federation_tests {
    use super::*;

    #[derive(Default)]
    struct FederationFakeTransport134 { calls: Vec<(String, String)> }

    impl FederationTransport134 for FederationFakeTransport134 {
        fn federation_get(&mut self, url: &str, path: &str) -> Result<String, String> {
            self.calls.push((url.to_owned(), path.to_owned()));
            match path {
                "/api/federation/status" | "/api/identity" => {
                    Ok(r#"{"node":"peer-node","agents":["remote"]}"#.to_owned())
                }
                _ => Err("unexpected path".to_owned()),
            }
        }
    }

    #[allow(dead_code)]
    struct FederationTestEnv134 {
        restore_peers: EnvVarRestore,
        restore_config: EnvVarRestore,
        root: std::path::PathBuf,
    }

    impl FederationTestEnv134 {
        fn new(label: &str) -> Self {
            let restore_peers = EnvVarRestore::capture("PEERS_FILE");
            let restore_config = EnvVarRestore::capture("MAW_CONFIG_DIR");
            let root = std::env::temp_dir().join(format!("maw-rs-federation-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("tmp");
            std::env::set_var("PEERS_FILE", root.join("peers.json"));
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            Self { restore_peers, restore_config, root }
        }
    }

    impl Drop for FederationTestEnv134 {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); }
    }

    fn federation_test_config() -> HeyConfig {
        let mut agents = HashMap::new();
        agents.insert("local-agent".to_owned(), "local".to_owned());
        HeyConfig { node: Some("local-node".to_owned()), oracle: None, route: RouteConfig { node: Some("local-node".to_owned()), named_peers: vec![RouteNamedPeer { name: "peer1".to_owned(), url: "http://peer.example:3456".to_owned() }], peers: Vec::new(), agents } }
    }

    fn federation_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn federation_dispatch_registers_native_and_guards() {
        assert_eq!(dispatcher_status("federation"), DispatchKind::Native);
        let _guard = env_test_lock().lock().expect("env lock");
        let _env = FederationTestEnv134::new("guard");
        let mut fake = FederationFakeTransport134::default();
        let out = federation_run_with(&federation_args(&["status", "--peers", "scout"]), &federation_test_config(), &mut fake);
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("config|both"));
    }

    #[test]
    fn federation_status_uses_native_curl_transport_shape() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _env = FederationTestEnv134::new("status");
        let mut fake = FederationFakeTransport134::default();
        let out = federation_run_with(&federation_args(&["status", "--json"]), &federation_test_config(), &mut fake);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("\"action\":\"status\""));
        assert!(out.stdout.contains("peer-node"));
        assert_eq!(fake.calls, vec![("http://peer.example:3456".to_owned(), "/api/federation/status".to_owned())]);
    }

    #[test]
    fn federation_sync_json_is_read_only_preview() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _env = FederationTestEnv134::new("sync");
        let mut fake = FederationFakeTransport134::default();
        let out = federation_run_with(&federation_args(&["sync", "--json"]), &federation_test_config(), &mut fake);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("\"action\":\"sync\""));
        assert!(out.stdout.contains("\"remote\""));
        assert!(out.stdout.contains("\"applied\":[]"));
    }

    #[test]
    fn federation_sync_dirty_default_refuses_live_write() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _env = FederationTestEnv134::new("sync-refuse");
        let mut fake = FederationFakeTransport134::default();
        let out = federation_run_with(&federation_args(&["sync"]), &federation_test_config(), &mut fake);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("pending native safety review"));
    }

    #[test]
    fn federation_curl_argv_is_no_shell_and_separator_guarded() {
        let argv = federation_curl_argv("http://peer.example:3456", "/api/identity").expect("argv");
        assert_eq!(argv.last().map(String::as_str), Some("http://peer.example:3456/api/identity"));
        assert!(argv.iter().any(|arg| arg == "--"));
        assert!(federation_curl_argv("http://peer.example", "/api/../secret").is_err());
    }
}
