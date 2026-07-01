const DISPATCH_97: &[DispatcherEntry] = &[DispatcherEntry { command: "pair", handler: Handler::Sync(pair_run_command) }];

const PAIR_USAGE: &str = "usage:\n  maw pair generate [--expires <sec>] [--at <local-url>]\n  maw pair <url> <code>\n  maw pair accept <code> --at <url>";
const PAIR_ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const PAIR_DEFAULT_EXPIRES_SEC: u64 = 120;
const PAIR_MIN_EXPIRES_SEC: u64 = 5;
const PAIR_MAX_EXPIRES_SEC: u64 = 3600;
const PAIR_BLOCKED_SUBCOMMANDS: &[&str] = &["approve", "auto-approve", "auto-pair", "pair-approve", "pair-auto", "trust"];
const PAIR_VALUE_FLAGS: &[&str] = &["--at", "--expires", "--token", "--token-ref", "--peer-token", "--federation-token"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum PairAction {
    Help,
    Generate(PairGeneratePlan),
    Accept(PairAcceptPlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairGeneratePlan {
    local_url: String,
    expires_sec: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairAcceptPlan {
    remote_url: String,
    code_normalized: String,
    code_redacted: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairGenerateLive {
    code_pretty: String,
    status_polled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairAcceptLive {
    remote_node: String,
    remote_url: String,
    token_received: bool,
    pubkey_pinned: bool,
    peers_written: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairConfig {
    node: String,
    port: u16,
}

trait PairHost {
    fn pair_config(&mut self) -> PairConfig;
    fn pair_generate_live(&mut self, plan: &PairGeneratePlan) -> Result<PairGenerateLive, String>;
    fn pair_accept_live(&mut self, plan: &PairAcceptPlan, config: &PairConfig) -> Result<PairAcceptLive, String>;
}

struct PairSystemHost;

impl PairHost for PairSystemHost {
    fn pair_config(&mut self) -> PairConfig {
        let config = load_hey_config();
        PairConfig {
            node: config.node.unwrap_or_else(|| "local".to_owned()),
            port: 3456,
        }
    }

    fn pair_generate_live(&mut self, plan: &PairGeneratePlan) -> Result<PairGenerateLive, String> {
        pair_system_generate_live(plan)
    }

    fn pair_accept_live(&mut self, plan: &PairAcceptPlan, config: &PairConfig) -> Result<PairAcceptLive, String> {
        pair_system_accept_live(plan, config)
    }
}

fn pair_run_command(argv: &[String]) -> CliOutput {
    let mut host = PairSystemHost;
    pair_run_command_with(argv, &mut host)
}

fn pair_run_command_with(argv: &[String], host: &mut impl PairHost) -> CliOutput {
    match pair_run(argv, host) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn pair_run(argv: &[String], host: &mut impl PairHost) -> Result<String, String> {
    pair_validate_argv(argv)?;
    match pair_parse(argv, host)? {
        PairAction::Help => Ok(pair_help()),
        PairAction::Generate(plan) => {
            let live = host.pair_generate_live(&plan)?;
            Ok(pair_render_generate(&plan, &live))
        }
        PairAction::Accept(plan) => {
            let config = host.pair_config();
            let live = host.pair_accept_live(&plan, &config)?;
            Ok(pair_render_accept(&plan, &config, &live))
        }
    }
}

fn pair_validate_argv(argv: &[String]) -> Result<(), String> {
    pair_validate_blocked_surface(argv)?;
    pair_validate_separator(argv)?;
    pair_validate_leading_dash_values(argv)?;
    pair_validate_control_free(argv)?;
    Ok(())
}

fn pair_validate_blocked_surface(argv: &[String]) -> Result<(), String> {
    let Some(first) = argv.first().map(String::as_str) else { return Ok(()); };
    if first.starts_with('-') { return Err("pair subcommand must not start with '-'".to_owned()); }
    if PAIR_BLOCKED_SUBCOMMANDS.iter().any(|blocked| blocked == &first) {
        return Err("pair: consent mutation requires explicit human pairing flow; no auto-approve surface is exposed".to_owned());
    }
    Ok(())
}

fn pair_validate_separator(argv: &[String]) -> Result<(), String> {
    if argv.iter().any(|arg| arg == "--") { return Err("pair: -- separator is not allowed".to_owned()); }
    Ok(())
}

fn pair_validate_leading_dash_values(argv: &[String]) -> Result<(), String> {
    let mut index = 0_usize;
    while index < argv.len() {
        let arg = argv[index].as_str();
        if pair_is_value_flag(arg) {
            pair_validate_flag_value(argv, index, arg)?;
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn pair_is_value_flag(arg: &str) -> bool {
    PAIR_VALUE_FLAGS.iter().any(|flag| flag == &arg)
}

fn pair_validate_flag_value(argv: &[String], index: usize, flag: &str) -> Result<(), String> {
    let Some(value) = argv.get(index + 1) else { return Ok(()); };
    if value == "--" || value.starts_with('-') { return Err(format!("pair: {flag} value must not start with '-'")); }
    if value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) { return Err(format!("pair: {flag} value must not contain control characters")); }
    Ok(())
}

fn pair_validate_control_free(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        if arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) { return Err("pair: arguments must not contain control characters".to_owned()); }
    }
    Ok(())
}

fn pair_parse(argv: &[String], host: &mut impl PairHost) -> Result<PairAction, String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| matches!(arg.as_str(), "help" | "--help" | "-h")) { return Ok(PairAction::Help); }
    let first = argv[0].as_str();
    if first == "generate" { return pair_parse_generate(&argv[1..], host).map(PairAction::Generate); }
    if first == "accept" { return pair_parse_accept_command(&argv[1..]).map(PairAction::Accept); }
    if argv.len() >= 2 && pair_is_http_url(first) { return pair_parse_url_code(first, &argv[1]).map(PairAction::Accept); }
    Err(format!("maw pair: unexpected args (got \"{}\") — expected 'generate' or '<url> <code>'\n{PAIR_USAGE}", pair_positional_summary(argv)))
}

fn pair_parse_generate(argv: &[String], host: &mut impl PairHost) -> Result<PairGeneratePlan, String> {
    let mut expires_sec = PAIR_DEFAULT_EXPIRES_SEC;
    let mut local_url = None::<String>;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--expires" => { expires_sec = pair_parse_expires(pair_next(argv, index, "--expires")?)?; index += 1; }
            value if value.starts_with("--expires=") => expires_sec = pair_parse_expires(&value["--expires=".len()..])?,
            "--at" => { local_url = Some(pair_validate_url(pair_next(argv, index, "--at")?, "--at")?); index += 1; }
            value if value.starts_with("--at=") => local_url = Some(pair_validate_url(&value["--at=".len()..], "--at")?),
            value if pair_is_token_value_flag(value) => { let _ = pair_next(argv, index, value)?; index += 1; }
            value if value.starts_with('-') => return Err(format!("pair: unknown argument {value}")),
            value => return Err(format!("pair: unexpected generate argument {value}")),
        }
        index += 1;
    }
    let config = host.pair_config();
    Ok(PairGeneratePlan { local_url: local_url.unwrap_or_else(|| format!("http://localhost:{}", config.port)), expires_sec })
}

fn pair_parse_accept_command(argv: &[String]) -> Result<PairAcceptPlan, String> {
    let Some(code) = argv.first() else { return Err("pair: accept requires <code> --at <url>".to_owned()); };
    let mut remote_url = None::<String>;
    let mut index = 1_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--at" => { remote_url = Some(pair_validate_url(pair_next(argv, index, "--at")?, "--at")?); index += 1; }
            value if value.starts_with("--at=") => remote_url = Some(pair_validate_url(&value["--at=".len()..], "--at")?),
            value if pair_is_token_value_flag(value) => { let _ = pair_next(argv, index, value)?; index += 1; }
            value if value.starts_with('-') => return Err(format!("pair: unknown argument {value}")),
            value => return Err(format!("pair: unexpected accept argument {value}")),
        }
        index += 1;
    }
    let Some(url) = remote_url else { return Err("pair: accept requires --at <url>".to_owned()); };
    pair_parse_url_code(&url, code)
}

fn pair_parse_url_code(url: &str, raw_code: &str) -> Result<PairAcceptPlan, String> {
    let remote_url = pair_validate_url(url, "url")?;
    let code_normalized = pair_normalize_code(raw_code);
    if !pair_is_valid_code(&code_normalized) { return Err(format!("invalid code shape: {}", pair_redact_code(&code_normalized))); }
    Ok(PairAcceptPlan { remote_url, code_redacted: pair_redact_code(&code_normalized), code_normalized })
}

fn pair_next<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1).map(String::as_str) else { return Err(format!("pair: missing value for {flag}")); };
    if value.starts_with('-') { return Err(format!("pair: missing value for {flag}")); }
    Ok(value)
}

fn pair_parse_expires(value: &str) -> Result<u64, String> {
    let parsed = value.parse::<u64>().map_err(|_| "--expires must be 5..3600 seconds".to_owned())?;
    if !(PAIR_MIN_EXPIRES_SEC..=PAIR_MAX_EXPIRES_SEC).contains(&parsed) { return Err("--expires must be 5..3600 seconds".to_owned()); }
    Ok(parsed)
}

fn pair_validate_url(raw: &str, label: &str) -> Result<String, String> {
    if raw.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) || raw.starts_with('-') { return Err(format!("pair: invalid {label}")); }
    let Some((scheme, rest)) = raw.split_once("://") else { return Err(format!("invalid URL \"{raw}\"")); };
    if !matches!(scheme, "http" | "https") { return Err(format!("invalid URL \"{raw}\" (must be http:// or https://)")); }
    if rest.is_empty() || rest.starts_with('/') || rest.contains(' ') { return Err(format!("invalid URL \"{raw}\"")); }
    Ok(raw.trim_end_matches('/').to_owned())
}

fn pair_is_token_value_flag(value: &str) -> bool {
    matches!(value, "--token" | "--token-ref" | "--peer-token" | "--federation-token")
}

fn pair_is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn pair_normalize_code(raw: &str) -> String {
    raw.chars().filter(|ch| !matches!(ch, '-' | ' ' | '\t' | '\n' | '\r')).flat_map(char::to_uppercase).collect()
}

fn pair_is_valid_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|ch| PAIR_ALPHABET.contains(ch))
}

fn pair_redact_code(code: &str) -> String {
    let normalized = pair_normalize_code(code);
    if normalized.len() >= 3 { format!("{}-***", &normalized[..3]) } else { "***".to_owned() }
}

fn pair_pretty_code(code: &str) -> String {
    let normalized = pair_normalize_code(code);
    if normalized.len() == 6 { format!("{}-{}", &normalized[..3], &normalized[3..]) } else { normalized }
}

fn pair_positional_summary(argv: &[String]) -> String {
    argv.iter().filter(|arg| !arg.starts_with("--")).cloned().collect::<Vec<_>>().join(" ")
}

fn pair_render_generate(plan: &PairGeneratePlan, live: &PairGenerateLive) -> String {
    let ttl_ms = plan.expires_sec * 1000;
    format!(
        "🤝 pair generate live\n   local server: {}\n   code: {}\n   ttlMs: {ttl_ms}\n   status poll: {}\n   waits for explicit remote accept; no auto-approve surface is exposed\n   token: <redacted>\n",
        plan.local_url,
        live.code_pretty,
        if live.status_polled { "ok" } else { "skipped" }
    )
}

fn pair_render_accept(plan: &PairAcceptPlan, config: &PairConfig, live: &PairAcceptLive) -> String {
    let warning = pair_plain_http_warning(&plan.remote_url);
    let local_url = format!("http://localhost:{}", config.port);
    format!(
        "🤝 pair accept live\n   remote: {}/api/pair/{}\n   code: {}\n   body: {{\"node\":{},\"url\":{}}}\n{}   peer: {} {}\n   federation token: {}\n   pubkey: {}\n   peers.json: {}\n   human consent required; no auto-approve surface is exposed\n",
        plan.remote_url,
        plan.code_normalized,
        pair_pretty_code(&plan.code_normalized),
        json_string(&config.node),
        json_string(&local_url),
        warning,
        live.remote_node,
        live.remote_url,
        if live.token_received { "received (redacted)" } else { "missing" },
        if live.pubkey_pinned { "pinned" } else { "missing (v3 signing will fail)" },
        if live.peers_written { "atomic write ok" } else { "not written" }
    )
}

fn pair_system_generate_live(plan: &PairGeneratePlan) -> Result<PairGenerateLive, String> {
    let body = serde_json::json!({ "ttlMs": plan.expires_sec.saturating_mul(1_000) }).to_string();
    let response = pair_http_json("POST", &format!("{}/api/pair/generate", plan.local_url), Some(body))?;
    if !(200..300).contains(&response.status) { return Err(format!("pair generate failed: HTTP {}", response.status)); }
    let value = pair_parse_json(&response.body, "pair generate")?;
    let code = pair_json_string(&value, "code").ok_or_else(|| "pair generate: missing code".to_owned())?;
    let normalized = pair_normalize_code(&code);
    if !pair_is_valid_code(&normalized) { return Err("pair generate: invalid code returned".to_owned()); }
    let status_url = format!("{}/api/pair/status/{}", plan.local_url, normalized);
    let status_polled = pair_http_json("GET", &status_url, None).is_ok();
    Ok(PairGenerateLive { code_pretty: pair_pretty_code(&normalized), status_polled })
}

fn pair_system_accept_live(plan: &PairAcceptPlan, config: &PairConfig) -> Result<PairAcceptLive, String> {
    let local_url = format!("http://localhost:{}", config.port);
    let body = serde_json::json!({ "node": config.node, "url": local_url }).to_string();
    let url = format!("{}/api/pair/{}", plan.remote_url, plan.code_normalized);
    let response = pair_http_json("POST", &url, Some(body))?;
    if !(200..300).contains(&response.status) { return Err(format!("pair accept failed: HTTP {}", response.status)); }
    let value = pair_parse_json(&response.body, "pair accept")?;
    let handshake_node = pair_json_string(&value, "node").ok_or_else(|| "pair accept: missing node".to_owned())?;
    let token_received = pair_json_string(&value, "federationToken").is_some();
    // Pin the peer at the URL we actually reached (the operator-supplied remote
    // URL), never the generator's self-reported `http://localhost:PORT` base URL.
    // Then fetch the remote's published identity (node + peer-key pubkey) so v3
    // request signing can authenticate future cross-node `maw hey` traffic. This
    // mirrors maw-js `cmdAdd → probePeer → /api/identity` TOFU pinning: the
    // handshake alone never carries a pubkey, so a bare accept would pin
    // `pubkey: null` and every signed request would fail with HTTP 401.
    let remote_url = plan.remote_url.clone();
    let identity = pair_fetch_remote_identity(&remote_url);
    let remote_node = identity
        .as_ref()
        .and_then(|value| pair_json_string(value, "host").or_else(|| pair_json_string(value, "node")))
        .unwrap_or_else(|| handshake_node.clone());
    let remote_oracle = identity
        .as_ref()
        .and_then(|value| pair_json_string(value, "oracle"))
        .unwrap_or_else(|| "mawjs".to_owned());
    let pubkey = identity
        .as_ref()
        .and_then(|value| pair_json_string(value, "pubkey"));
    pair_validate_peer_identity(&remote_node, &remote_url)?;
    pair_write_peer(&remote_node, &remote_oracle, &remote_url, pubkey.as_deref())?;
    Ok(PairAcceptLive {
        remote_node,
        remote_url,
        token_received,
        pubkey_pinned: pubkey.is_some(),
        peers_written: true,
    })
}

fn pair_fetch_remote_identity(remote_url: &str) -> Option<serde_json::Value> {
    let response = pair_http_json("GET", &format!("{remote_url}/api/identity"), None).ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    pair_parse_json(&response.body, "pair identity").ok()
}

fn pair_http_json(method: &str, url: &str, body: Option<String>) -> Result<maw_transport::HttpResponse, String> {
    let io = ReqwestHttpTransportIo::new(5_000)?;
    let http_request = TransportHttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        headers: BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]),
        body,
        timeout_ms: Some(5_000),
        follow_redirects: false,
        pinned_addr: None,
    };
    // The CLI executes inside a multi-threaded tokio runtime (see
    // `#[tokio::main(flavor = "multi_thread")]` in maw-cli's binary). Building a
    // nested runtime here and calling `block_on` panics with "Cannot start a
    // runtime from within a runtime". Reuse the current runtime handle via
    // `block_in_place` when one is present, and only build a standalone
    // current-thread runtime when called outside any runtime (e.g. sync tests).
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(io.request(&http_request))),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("pair http runtime failed: {error}"))?
            .block_on(io.request(&http_request)),
    }
}

fn pair_parse_json(raw: &str, label: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(raw).map_err(|error| format!("{label}: invalid json: {error}"))
}

fn pair_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

fn pair_validate_peer_identity(node: &str, url: &str) -> Result<(), String> {
    if let Some(message) = maw_peer::validate_peer_alias(node) { return Err(message); }
    if let Some(message) = maw_peer::validate_peer_url(url) { return Err(message); }
    Ok(())
}

fn pair_write_peer(node: &str, oracle: &str, url: &str, pubkey: Option<&str>) -> Result<(), String> {
    let env = pair_peer_store_env();
    pair_write_peer_to_env(&env, node, oracle, url, pubkey)
}

fn pair_write_peer_to_env(env: &maw_peer::PeerStoreEnv, node: &str, oracle: &str, url: &str, pubkey: Option<&str>) -> Result<(), String> {
    let now = now_iso_utc();
    let pubkey = pubkey.map(ToOwned::to_owned);
    let pubkey_first_seen = pubkey.as_ref().map(|_| now.clone());
    maw_peer::mutate_peer_store(env, |store| {
        store.peers.insert(node.to_owned(), maw_peer::PeerRecord {
            url: url.to_owned(),
            node: Some(node.to_owned()),
            added_at: now.clone(),
            last_seen: Some(now.clone()),
            last_error: None,
            nickname: None,
            pubkey: pubkey.clone(),
            pubkey_first_seen: pubkey_first_seen.clone(),
            identity: Some(maw_peer::PeerIdentity { oracle: oracle.to_owned(), node: node.to_owned() }),
            one_way: Some(false),
            last_symmetric_check: Some(now.clone()),
        });
    }).map_err(|error| format!("pair peers.json write failed: {error}"))?;
    Ok(())
}

fn pair_peer_store_env() -> maw_peer::PeerStoreEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = ["PEERS_FILE", "MAW_HOME", "MAW_XDG", "XDG_STATE_HOME"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.to_owned(), value)))
        .collect::<Vec<_>>();
    maw_peer::PeerStoreEnv::with_vars(home, vars)
}

fn pair_plain_http_warning(url: &str) -> String {
    if !url.starts_with("http://") { return String::new(); }
    let host = url.trim_start_matches("http://").split(['/', ':']).next().unwrap_or_default();
    if matches!(host, "localhost" | "127.0.0.1" | "::1") { String::new() } else { "   ⚠ pairing over plain HTTP — TLS recommended for cross-network pairing\n".to_owned() }
}

fn pair_help() -> String {
    [
        PAIR_USAGE,
        "",
        "example: B: `maw pair generate` → prints W4K-7F3; A: `maw pair http://b:5002 W4K-7F3`",
        "human consent is required; no auto approval or token-writing surface is exposed.",
        "live native pair: generate mints via serve, accept handshakes and atomically updates peers.json.",
    ].join("\n") + "\n"
}

#[cfg(test)]
mod pair_tests {
    use super::*;

    struct PairFakeHost;

    impl PairHost for PairFakeHost {
        fn pair_config(&mut self) -> PairConfig {
            PairConfig { node: "fake-node".to_owned(), port: 5002 }
        }

        fn pair_generate_live(&mut self, _plan: &PairGeneratePlan) -> Result<PairGenerateLive, String> {
            Ok(PairGenerateLive { code_pretty: "W4K-7F3".to_owned(), status_polled: true })
        }

        fn pair_accept_live(&mut self, _plan: &PairAcceptPlan, _config: &PairConfig) -> Result<PairAcceptLive, String> {
            Ok(PairAcceptLive {
                remote_node: "peer-node".to_owned(),
                remote_url: "https://peer.example".to_owned(),
                token_received: true,
                pubkey_pinned: true,
                peers_written: true,
            })
        }
    }

    fn pair_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn pair_output(values: &[&str]) -> CliOutput {
        let mut host = PairFakeHost;
        pair_run_command_with(&pair_args(values), &mut host)
    }

    #[test]
    fn pair_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_97.len(), 1);
        assert_eq!(DISPATCH_97[0].command, "pair");
        assert_eq!(dispatcher_status("pair"), DispatchKind::Native);
    }

    #[test]
    fn pair_generate_is_live_and_does_not_echo_fake_token() {
        let output = pair_output(&["generate", "--expires", "60", "--at", "http://localhost:5002", "--token", "fake-test-token"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("pair generate live"));
        assert!(output.stdout.contains("ttlMs: 60000"));
        assert!(output.stdout.contains("status poll: ok"));
        assert!(output.stdout.contains("token: <redacted>"));
        assert!(!output.stdout.contains("fake-test-token"));
        assert!(!output.stderr.contains("fake-test-token"));
    }

    #[test]
    fn pair_accept_url_code_is_live_and_redacts_flow() {
        let output = pair_output(&["http://peer.example:5002", "W4K-7F3"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("pair accept live"));
        assert!(output.stdout.contains("W4K7F3"));
        assert!(output.stdout.contains("fake-node"));
        assert!(output.stdout.contains("plain HTTP"));
        assert!(output.stdout.contains("federation token: received (redacted)"));
    }

    #[test]
    fn pair_accept_subcommand_supports_at_url() {
        let output = pair_output(&["accept", "W4K-7F3", "--at", "https://peer.example"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("https://peer.example/api/pair/W4K7F3"));
    }

    #[test]
    fn pair_refuses_auto_approve_surface_before_secret_values() {
        for blocked in PAIR_BLOCKED_SUBCOMMANDS {
            let output = pair_output(&[blocked, "--token", "fake-test-token"]);
            assert_eq!(output.code, 2, "blocked {blocked}");
            assert!(output.stderr.contains("no auto-approve"));
            assert!(!output.stderr.contains("fake-test-token"));
        }
    }

    #[test]
    fn pair_guards_separator_and_leading_dash_values_without_secret_echo() {
        let sep = pair_output(&["generate", "--"]);
        assert_eq!(sep.code, 2);
        assert!(sep.stderr.contains("separator"));
        let bad = pair_output(&["generate", "--token", "-secret-token"]);
        assert_eq!(bad.code, 2);
        assert!(bad.stderr.contains("--token value must not start"));
        assert!(!bad.stderr.contains("secret-token"));
    }

    #[test]
    fn pair_validates_expires_url_and_code_shape() {
        assert!(pair_output(&["generate", "--expires", "4"]).stderr.contains("5..3600"));
        assert!(pair_output(&["generate", "--at", "ftp://peer"]).stderr.contains("must be http"));
        assert!(pair_output(&["https://peer", "BAD000"]).stderr.contains("invalid code shape"));
    }


    #[test]
    fn pair_write_peer_uses_atomic_peer_store_path() {
        let root = std::env::temp_dir().join(format!(
            "maw-rs-pair-live-{}",
            std::process::id()
        ));
        let peers = root.join("state").join("peers.json");
        let env = maw_peer::PeerStoreEnv::with_vars(
            root.clone(),
            [("PEERS_FILE", peers.to_string_lossy().to_string())],
        );
        let pinned_pubkey = "a".repeat(64);
        pair_write_peer_to_env(&env, "peer-node", "mawjs", "https://peer.example", Some(&pinned_pubkey)).expect("write peer");
        let raw = std::fs::read_to_string(&peers).expect("read peers");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(value["peers"]["peer-node"]["url"], "https://peer.example");
        assert_eq!(value["peers"]["peer-node"]["pubkey"], pinned_pubkey);
        assert_eq!(value["peers"]["peer-node"]["identity"]["oracle"], "mawjs");
        assert_eq!(value["peers"]["peer-node"]["identity"]["node"], "peer-node");
        assert!(value["peers"]["peer-node"]["pubkeyFirstSeen"].is_string());
        assert!(!peers.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pair_help_has_no_auto_approve_surface() {
        let output = pair_output(&[]);
        assert_eq!(output.code, 0);
        assert!(!output.stdout.contains("auto-approve"));
        assert!(output.stdout.contains("human"));
    }
}
