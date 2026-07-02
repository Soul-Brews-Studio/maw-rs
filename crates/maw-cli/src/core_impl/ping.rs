const DISPATCH_138: &[DispatcherEntry] = &[DispatcherEntry { command: "ping", handler: Handler::Sync(ping_run_command) }];

const PING_USAGE: &str = "usage: maw ping [node] — ping all peers or a specific node";
const PING_AUTH_STATUS_PATH: &str = "/api/auth/status";
const PING_CURL_TIMEOUT_SECONDS: &str = "5";
const PING_HTTP_STATUS_MARKER: &str = "__MAW_HTTP_STATUS__:";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PingTarget133 { name: String, url: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct PingProbe133 { target: PingTarget133, status: PingStatus133, ms: u128 }

#[derive(Debug, Clone, PartialEq, Eq)]
enum PingStatus133 { Ok { auth: String, token: String }, Http(u16), Unreachable }

trait PingTransport133 { fn ping_auth_status(&mut self, target: &PingTarget133) -> PingStatus133; }

struct PingCurlTransport133;

impl PingTransport133 for PingCurlTransport133 {
    fn ping_auth_status(&mut self, target: &PingTarget133) -> PingStatus133 {
        let Ok(argv) = ping_curl_argv(&target.url) else { return PingStatus133::Unreachable; };
        let Ok(raw) = ping_spawn_curl(&argv) else { return PingStatus133::Unreachable; };
        ping_parse_curl_output(&raw)
    }
}

fn ping_run_command(argv: &[String]) -> CliOutput {
    ping_run_command_with(argv, &load_hey_config(), &mut PingCurlTransport133, ping_now_millis)
}

fn ping_run_command_with(
    argv: &[String],
    config: &HeyConfig,
    transport: &mut impl PingTransport133,
    now: fn() -> u128,
) -> CliOutput {
    match ping_run(argv, config, transport, now) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("ping: {message}\n") },
    }
}

fn ping_run(argv: &[String], config: &HeyConfig, transport: &mut impl PingTransport133, now: fn() -> u128) -> Result<String, String> {
    let node = ping_parse_args(argv)?;
    let targets = ping_targets(config, node.as_deref())?;
    if targets.is_empty() { return Ok("\x1b[90mno peers configured\x1b[0m\n".to_owned()); }
    let mut probes = Vec::with_capacity(targets.len());
    for target in targets {
        ping_validate_target(&target)?;
        let start = now();
        let status = transport.ping_auth_status(&target);
        let end = now();
        probes.push(PingProbe133 { target, status, ms: end.saturating_sub(start) });
    }
    Ok(ping_render(&probes))
}

fn ping_parse_args(argv: &[String]) -> Result<Option<String>, String> {
    match argv {
        [] => Ok(None),
        [one] if one == "--help" || one == "-h" => Err(PING_USAGE.to_owned()),
        [one] => { ping_validate_node(one)?; Ok(Some(one.clone())) }
        _ => Err(PING_USAGE.to_owned()),
    }
}

fn ping_targets(config: &HeyConfig, node: Option<&str>) -> Result<Vec<PingTarget133>, String> {
    let mut named = config.route.named_peers.iter().map(|peer| PingTarget133 { name: peer.name.clone(), url: peer.url.clone() }).collect::<Vec<_>>();
    named.extend(ping_targets_from_peer_store()?);
    let mut seen = BTreeSet::new();
    named.retain(|target| seen.insert((target.name.clone(), target.url.clone())));
    let legacy = config.route.peers.iter().filter(|url| !named.iter().any(|peer| peer.url == **url)).map(|url| PingTarget133 { name: url.clone(), url: url.clone() }).collect::<Vec<_>>();
    match node {
        Some(value) => {
            ping_validate_node(value)?;
            if let Some(peer) = named.iter().find(|peer| peer.name == value) { return Ok(vec![peer.clone()]); }
            if let Some(peer) = legacy.iter().find(|peer| peer.url.contains(value)) { return Ok(vec![PingTarget133 { name: value.to_owned(), url: peer.url.clone() }]); }
            let known = if named.is_empty() { "(none)".to_owned() } else { named.iter().map(|peer| peer.name.as_str()).collect::<Vec<_>>().join(", ") };
            Err(format!("\x1b[33mknown\x1b[0m: {known}\nunknown node \"{value}\""))
        }
        None => Ok(named.into_iter().chain(legacy).collect()),
    }
}

#[derive(Debug, serde::Deserialize, Default)]
struct PingPeersStore133 { #[serde(default)] peers: BTreeMap<String, PingPeerEntry133> }

#[derive(Debug, serde::Deserialize, Default)]
struct PingPeerEntry133 { url: Option<String>, node: Option<String> }

fn ping_targets_from_peer_store() -> Result<Vec<PingTarget133>, String> {
    let Some(raw) = ping_read_peers_json()? else { return Ok(Vec::new()); };
    let store = serde_json::from_str::<PingPeersStore133>(&raw).unwrap_or_default();
    let mut out = Vec::new();
    for (alias, entry) in store.peers {
        ping_validate_node(&alias)?;
        if let Some(url) = entry.url {
            ping_validate_peer_url(&url)?;
            if let Some(node) = entry.node.as_deref() { ping_validate_node(node).map_err(|_| format!("invalid peer node for {alias}"))?; }
            out.push(PingTarget133 { name: alias, url });
        }
    }
    Ok(out)
}

fn ping_read_peers_json() -> Result<Option<String>, String> {
    let path = ping_peers_path();
    if path.exists() {
        return std::fs::read_to_string(&path).map(Some).map_err(|error| format!("peers: read {}: {error}", path.display()));
    }
    Ok(None)
}

fn ping_peers_path() -> std::path::PathBuf {
    std::env::var_os("PEERS_FILE").map_or_else(|| maw_state_path(&current_xdg_env(), &["peers.json"]), std::path::PathBuf::from)
}

fn ping_curl_argv(peer_url: &str) -> Result<Vec<String>, String> {
    ping_validate_peer_url(peer_url)?;
    let url = format!("{}{}", peer_url.trim_end_matches('/'), PING_AUTH_STATUS_PATH);
    let argv = vec![
        "-sS".to_owned(),
        "--max-time".to_owned(),
        PING_CURL_TIMEOUT_SECONDS.to_owned(),
        "-w".to_owned(),
        format!("{PING_HTTP_STATUS_MARKER}%{{http_code}}"),
        "--".to_owned(),
        url,
    ];
    ping_validate_curl_argv(&argv)?;
    Ok(argv)
}

fn ping_spawn_curl(argv: &[String]) -> Result<String, String> {
    ping_validate_curl_argv(argv)?;
    let output = std::process::Command::new("curl")
        .args(argv)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("failed to spawn curl: {error}"))?;
    if !output.status.success() { return Err("curl failed".to_owned()); }
    String::from_utf8(output.stdout).map_err(|error| format!("curl stdout was not utf8: {error}"))
}

fn ping_parse_curl_output(raw: &str) -> PingStatus133 {
    let Some((body, status_raw)) = raw.rsplit_once(PING_HTTP_STATUS_MARKER) else { return PingStatus133::Unreachable; };
    let Ok(status) = status_raw.trim().parse::<u16>() else { return PingStatus133::Unreachable; };
    if !(200..300).contains(&status) { return PingStatus133::Http(status); }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body.trim()) else { return PingStatus133::Unreachable; };
    let auth = if value.get("enabled").and_then(serde_json::Value::as_bool) == Some(true) { "auth: ok" } else { "auth: off" };
    let token = value.get("tokenPreview").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    PingStatus133::Ok { auth: auth.to_owned(), token }
}

fn ping_validate_curl_argv(argv: &[String]) -> Result<(), String> {
    if !argv.iter().any(|arg| arg == "--") { return Err("curl argv must include -- URL separator".to_owned()); }
    for arg in argv {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) { return Err("curl argv must not contain NUL/control characters".to_owned()); }
    }
    Ok(())
}

fn ping_validate_target(target: &PingTarget133) -> Result<(), String> {
    ping_validate_node(&target.name)?;
    ping_validate_peer_url(&target.url)
}

fn ping_validate_node(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.trim() != value || value.len() > 128 {
        return Err("node must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>' | '\'' | '"')) {
        return Err("node must not contain whitespace, control, or shell metacharacters".to_owned());
    }
    Ok(())
}

fn ping_validate_peer_url(value: &str) -> Result<(), String> {
    if !(value.starts_with("http://") || value.starts_with("https://")) { return Err("peer url must start with http:// or https://".to_owned()); }
    if value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) { return Err("peer url must not contain whitespace or control characters".to_owned()); }
    Ok(())
}

fn ping_render(probes: &[PingProbe133]) -> String {
    let mut out = String::new();
    for probe in probes {
        match &probe.status {
            PingStatus133::Ok { auth, token } => {
                let suffix = if token.is_empty() { String::new() } else { format!(" ({token})") };
                let _ = writeln!(out, "\x1b[32m✅\x1b[0m {} \x1b[90m({})\x1b[0m — {}ms, {auth}{suffix}", probe.target.name, probe.target.url, probe.ms);
            }
            PingStatus133::Http(status) => {
                let _ = writeln!(out, "\x1b[31m❌\x1b[0m {} \x1b[90m({})\x1b[0m — {}ms, {status}", probe.target.name, probe.target.url, probe.ms);
            }
            PingStatus133::Unreachable => {
                let _ = writeln!(out, "\x1b[31m❌\x1b[0m {} \x1b[90m({})\x1b[0m — {}ms, unreachable", probe.target.name, probe.target.url, probe.ms);
            }
        }
    }
    out
}

fn ping_now_millis() -> u128 { current_epoch_seconds().saturating_mul(1000).into() }

#[cfg(test)]
mod ping_tests133 {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct PingFakeTransport133 { statuses: VecDeque<PingStatus133>, targets: Vec<PingTarget133> }

    impl PingTransport133 for PingFakeTransport133 {
        fn ping_auth_status(&mut self, target: &PingTarget133) -> PingStatus133 {
            ping_validate_target(target).expect("safe target");
            self.targets.push(target.clone());
            self.statuses.pop_front().unwrap_or(PingStatus133::Unreachable)
        }
    }

    fn cfg() -> HeyConfig {
        HeyConfig { node: Some("self".to_owned()), oracle: Some("oracle".to_owned()), route: RouteConfig { node: Some("self".to_owned()), named_peers: vec![RouteNamedPeer { name: "alpha".to_owned(), url: "http://alpha.invalid:31745".to_owned() }], peers: vec!["http://legacy.invalid:31745".to_owned()], agents: HashMap::new() } }
    }

    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn ping_no_peer_store_env() -> (std::sync::MutexGuard<'static, ()>, EnvVarRestore) {
        let lock = env_test_lock().lock().expect("env lock");
        let restore = EnvVarRestore::capture("PEERS_FILE");
        std::env::set_var("PEERS_FILE", "/tmp/maw-rs-ping-native-no-peers.json");
        (lock, restore)
    }

    fn now() -> u128 { 42 }

    #[test]
    fn ping_dispatch_registers_native() { assert_eq!(dispatcher_status("ping"), DispatchKind::Native); }

    #[test]
    fn ping_all_renders_named_and_legacy_without_real_network() {
        let _guard = ping_no_peer_store_env();
        let mut transport = PingFakeTransport133 { statuses: VecDeque::from([PingStatus133::Ok { auth: "auth: ok".to_owned(), token: "tokn****".to_owned() }, PingStatus133::Http(503)]), ..PingFakeTransport133::default() };
        let output = ping_run(&[], &cfg(), &mut transport, now).expect("ping");
        assert!(output.contains("✅\x1b[0m alpha"));
        assert!(output.contains("tokn****"));
        assert!(output.contains("❌\x1b[0m http://legacy.invalid:31745"));
        assert_eq!(transport.targets.len(), 2);
    }

    #[test]
    fn ping_specific_unknown_lists_known_and_errors_before_transport() {
        let _guard = ping_no_peer_store_env();
        let mut transport = PingFakeTransport133::default();
        let error = ping_run(&args(&["missing"]), &cfg(), &mut transport, now).expect_err("unknown");
        assert!(error.contains("known"));
        assert!(error.contains("alpha"));
        assert!(transport.targets.is_empty());
    }

    #[test]
    fn ping_rejects_shell_metachar_node_before_transport() {
        let mut transport = PingFakeTransport133::default();
        let error = ping_run(&args(&["bad;node"]), &cfg(), &mut transport, now).expect_err("bad");
        assert!(error.contains("metacharacters"));
        assert!(transport.targets.is_empty());
    }

    #[test]
    fn ping_no_peers_matches_maw_js_empty_message() {
        let _guard = ping_no_peer_store_env();
        let mut transport = PingFakeTransport133::default();
        let output = ping_run(&[], &HeyConfig::default(), &mut transport, now).expect("empty");
        assert_eq!(output, "\x1b[90mno peers configured\x1b[0m\n");
        assert!(transport.targets.is_empty());
    }

    #[test]
    fn ping_curl_argv_is_no_shell_and_targets_auth_status() {
        let argv = ping_curl_argv("http://peer.invalid:31745").expect("argv");
        assert_eq!(argv.first().map(String::as_str), Some("-sS"));
        assert!(argv.contains(&"--".to_owned()));
        assert!(argv.iter().any(|arg| arg == "http://peer.invalid:31745/api/auth/status"));
        assert!(!argv.windows(2).any(|pair| pair[0] == "sh" && pair[1] == "-c"));
    }

    #[test]
    fn ping_parse_curl_output_maps_status_and_auth() {
        assert_eq!(ping_parse_curl_output(r#"{"enabled":true,"tokenPreview":"abcd****"}__MAW_HTTP_STATUS__:200"#), PingStatus133::Ok { auth: "auth: ok".to_owned(), token: "abcd****".to_owned() });
        assert_eq!(ping_parse_curl_output(r#"{"error":"no"}__MAW_HTTP_STATUS__:404"#), PingStatus133::Http(404));
    }
}
