const DISPATCH_103: &[DispatcherEntry] = &[
    DispatcherEntry { command: "scout", handler: Handler::Async(zenohscout_async_native) },
    DispatcherEntry { command: "zenoh-scout", handler: Handler::Async(zenohscout_async_native) },
];

const ZENOHSCOUT_USAGE: &str = "usage: maw scout [--transport zenoh|scout|both] [--force] [--json] [--locator ws://127.0.0.1:10000] [--timeout <ms>] [--limit <n>] [--all] [--advertise|--no-advertise] [--status]";
const ZENOHSCOUT_DEFAULT_LOCATOR: &str = "ws://127.0.0.1:10000";
const ZENOHSCOUT_DEFAULT_TIMEOUT_MS: u64 = 750;
const ZENOHSCOUT_DEFAULT_KEY_PREFIX: &str = "maw/discovery/v1";
const ZENOHSCOUT_FAKE_KEYS_ENV: &str = "MAW_RS_ZENOH_SCOUT_FAKE_KEYS";
const ZENOHSCOUT_FAKE_NOW_ENV: &str = "MAW_RS_ZENOH_SCOUT_FAKE_NOW_MS";

fn zenohscout_async_native(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { zenohscout_run_async(&args).await })
}

async fn zenohscout_run_async(argv: &[String]) -> CliOutput {
    match zenohscout_parse_args(argv) {
        Ok(parsed) => zenohscout_execute(parsed).await,
        Err(message) if message.is_empty() => zenohscout_ok(ZENOHSCOUT_USAGE),
        Err(message) => zenohscout_error(&message),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZenohScoutTransportChoice { Zenoh, Scout, Both }

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ZenohScoutArgs {
    transport: ZenohScoutTransportChoice,
    force: bool,
    json: bool,
    all: bool,
    advertise: bool,
    status: bool,
    locator: Option<String>,
    timeout_ms: Option<u64>,
    limit: Option<usize>,
}

fn zenohscout_parse_args(argv: &[String]) -> Result<ZenohScoutArgs, String> {
    let mut parsed = ZenohScoutArgs {
        transport: ZenohScoutTransportChoice::Zenoh,
        force: false,
        json: false,
        all: false,
        advertise: true,
        status: false,
        locator: None,
        timeout_ms: None,
        limit: None,
    };
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "help" | "--help" | "-h" => return Err(String::new()),
            "--" => return Err("zenoh-scout: -- separator is not allowed".to_owned()),
            "status" | "--status" => parsed.status = true,
            "scout" => parsed.transport = ZenohScoutTransportChoice::Scout,
            "advertise" => { parsed.advertise = true; parsed.force = true; }
            "--force" => parsed.force = true,
            "--json" => parsed.json = true,
            "--all" => parsed.all = true,
            "--advertise" => parsed.advertise = true,
            "--no-advertise" => parsed.advertise = false,
            "--transport" => { parsed.transport = zenohscout_parse_transport(&zenohscout_take_value(argv, index, "--transport")?)?; index += 1; }
            "--locator" => { parsed.locator = Some(zenohscout_validate_locator(&zenohscout_take_value(argv, index, "--locator")?)?); index += 1; }
            "--timeout" => { parsed.timeout_ms = Some(zenohscout_parse_timeout(&zenohscout_take_value(argv, index, "--timeout")?)?); index += 1; }
            "--limit" => { parsed.limit = Some(zenohscout_parse_limit(&zenohscout_take_value(argv, index, "--limit")?)?); index += 1; }
            value if value.starts_with("--transport=") => parsed.transport = zenohscout_parse_transport(&value["--transport=".len()..])?,
            value if value.starts_with("--locator=") => parsed.locator = Some(zenohscout_validate_locator(&value["--locator=".len()..])?),
            value if value.starts_with("--timeout=") => parsed.timeout_ms = Some(zenohscout_parse_timeout(&value["--timeout=".len()..])?),
            value if value.starts_with("--limit=") => parsed.limit = Some(zenohscout_parse_limit(&value["--limit=".len()..])?),
            value if value.starts_with('-') => return Err(format!("zenoh-scout: unknown argument {value}")),
            other => return Err(format!("zenoh-scout: unexpected argument {other}")),
        }
        index += 1;
    }
    Ok(parsed)
}

fn zenohscout_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(index + 1) else { return Err(format!("zenoh-scout: missing {flag} value")); };
    if value == "--" || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("zenoh-scout: {flag} value is not allowed"));
    }
    Ok(value.clone())
}

fn zenohscout_parse_transport(value: &str) -> Result<ZenohScoutTransportChoice, String> {
    if value.starts_with('-') || value.chars().any(char::is_control) { return Err("zenoh-scout: invalid transport".to_owned()); }
    match value {
        "zenoh" => Ok(ZenohScoutTransportChoice::Zenoh),
        "scout" => Ok(ZenohScoutTransportChoice::Scout),
        "both" => Ok(ZenohScoutTransportChoice::Both),
        _ => Err("usage: maw scout --transport zenoh|scout|both".to_owned()),
    }
}

fn zenohscout_validate_locator(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { return Err("zenoh-scout: locator is required".to_owned()); }
    if trimmed != value || trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err("zenoh-scout: locator is not allowed".to_owned());
    }
    if trimmed.len() > 2048 { return Err("zenoh-scout: locator is too long".to_owned()); }
    if !trimmed.contains("://") { return Err("zenoh-scout: locator must include a scheme".to_owned()); }
    Ok(trimmed.to_owned())
}

fn zenohscout_parse_timeout(value: &str) -> Result<u64, String> {
    let timeout = value.parse::<u64>().map_err(|_| "zenoh-scout: timeout must be a positive number".to_owned())?;
    if timeout == 0 { return Err("zenoh-scout: timeout must be a positive number".to_owned()); }
    Ok(timeout.min(60_000))
}

fn zenohscout_parse_limit(value: &str) -> Result<usize, String> {
    let limit = value.parse::<usize>().map_err(|_| "zenoh-scout: limit must be a positive number".to_owned())?;
    if limit == 0 { return Err("zenoh-scout: limit must be a positive number".to_owned()); }
    Ok(limit.min(500))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZenohScoutConfigNative {
    enabled: bool,
    locator: String,
    timeout_ms: u64,
    key_prefix: String,
    node: String,
    oracle: String,
    api_url: String,
    capabilities: Vec<String>,
}

async fn zenohscout_execute(args: ZenohScoutArgs) -> CliOutput {
    let base = zenohscout_read_config();
    let locator = args.locator.clone().unwrap_or_else(|| base.locator.clone());
    let timeout_ms = args.timeout_ms.unwrap_or(base.timeout_ms);
    if args.transport == ZenohScoutTransportChoice::Scout {
        return zenohscout_execute_scout(&args, timeout_ms).await;
    }
    if args.status {
        return zenohscout_render_result(&zenohscout_status_result(&base, &locator), args.json);
    }
    let zenoh = if base.enabled || args.force {
        zenohscout_run_zenoh(&base, &locator, timeout_ms, args.advertise)
    } else {
        zenohscout_disabled_result(&base, &locator)
    };
    if args.transport == ZenohScoutTransportChoice::Both {
        let scout = zenohscout_fetch_discoveries(args.all, args.limit, timeout_ms).await;
        return zenohscout_render_both(&zenoh, &scout, args.json);
    }
    zenohscout_render_result(&zenoh, args.json)
}

fn zenohscout_read_config() -> ZenohScoutConfigNative {
    let value = zenohscout_read_config_json().unwrap_or(serde_json::Value::Null);
    let zenoh = value.get("zenoh").and_then(serde_json::Value::as_object);
    let scout = zenoh.and_then(|map| map.get("scout")).and_then(serde_json::Value::as_object);
    let node = value.get("node").and_then(serde_json::Value::as_str).unwrap_or("local");
    let oracle = value.get("oracle").and_then(serde_json::Value::as_str).unwrap_or("mawrs");
    let port = value.get("port").and_then(serde_json::Value::as_u64).unwrap_or(3456);
    ZenohScoutConfigNative {
        enabled: scout.and_then(|map| map.get("enabled")).and_then(serde_json::Value::as_bool) == Some(true),
        locator: zenohscout_config_string(scout, "locator").or_else(|| zenohscout_config_string(zenoh, "locator")).unwrap_or_else(|| ZENOHSCOUT_DEFAULT_LOCATOR.to_owned()),
        timeout_ms: zenohscout_config_u64(scout, "timeoutMs").unwrap_or(ZENOHSCOUT_DEFAULT_TIMEOUT_MS).max(1),
        key_prefix: zenohscout_config_string(scout, "keyPrefix").map(|value| value.trim_end_matches('/').to_owned()).filter(|value| !value.is_empty()).unwrap_or_else(|| ZENOHSCOUT_DEFAULT_KEY_PREFIX.to_owned()),
        node: node.to_owned(),
        oracle: oracle.to_owned(),
        api_url: format!("http://{node}:{port}"),
        capabilities: vec!["pair".to_owned(), "feed".to_owned(), "send".to_owned()],
    }
}

fn zenohscout_read_config_json() -> Option<serde_json::Value> {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn zenohscout_config_string(map: Option<&serde_json::Map<String, serde_json::Value>>, key: &str) -> Option<String> {
    map?.get(key)?.as_str().map(ToOwned::to_owned)
}

fn zenohscout_config_u64(map: Option<&serde_json::Map<String, serde_json::Value>>, key: &str) -> Option<u64> {
    map?.get(key)?.as_u64()
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ZenohScoutResultNative {
    ok: bool,
    enabled: bool,
    locator: String,
    #[serde(rename = "keyPrefix")]
    key_prefix: String,
    total: usize,
    peers: Vec<ZenohScoutPeerNative>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ZenohScoutPeerNative {
    zid: String,
    node: String,
    oracle: String,
    host: String,
    locators: Vec<String>,
    capabilities: Vec<String>,
    oracles: Vec<String>,
    #[serde(rename = "firstSeen")]
    first_seen: String,
    #[serde(rename = "lastSeen")]
    last_seen: String,
    #[serde(rename = "seenRel")]
    seen_rel: String,
    paired: bool,
    transport: String,
}

fn zenohscout_status_result(base: &ZenohScoutConfigNative, locator: &str) -> ZenohScoutResultNative {
    let hint = if base.enabled { "zenoh-scout enabled; run `maw scout --force` to query now" } else { "zenoh-scout disabled; set zenoh.scout.enabled=true or pass --force for a one-shot query" };
    ZenohScoutResultNative { ok: true, enabled: base.enabled, locator: locator.to_owned(), key_prefix: base.key_prefix.clone(), total: 0, peers: Vec::new(), error: None, hint: Some(hint.to_owned()) }
}

fn zenohscout_disabled_result(base: &ZenohScoutConfigNative, locator: &str) -> ZenohScoutResultNative {
    ZenohScoutResultNative { ok: true, enabled: false, locator: locator.to_owned(), key_prefix: base.key_prefix.clone(), total: 0, peers: Vec::new(), error: None, hint: Some("zenoh-scout is opt-in; set zenoh.scout.enabled=true or pass --force for a one-shot query".to_owned()) }
}

fn zenohscout_run_zenoh(base: &ZenohScoutConfigNative, locator: &str, _timeout_ms: u64, advertise: bool) -> ZenohScoutResultNative {
    let mut config = base.clone();
    locator.clone_into(&mut config.locator);
    if advertise { let _key = zenohscout_discovery_key(&config); }
    if let Some(raw) = std::env::var_os(ZENOHSCOUT_FAKE_KEYS_ENV) {
        return zenohscout_fake_result(&config, &raw.to_string_lossy());
    }
    ZenohScoutResultNative {
        ok: false,
        enabled: true,
        locator: locator.to_owned(),
        key_prefix: base.key_prefix.clone(),
        total: 0,
        peers: Vec::new(),
        error: Some("zenoh_unavailable".to_owned()),
        hint: Some("native zenoh backend is not linked; use --transport scout|both or request supply-chain review for the zenoh Rust crate".to_owned()),
    }
}

fn zenohscout_fake_result(config: &ZenohScoutConfigNative, raw: &str) -> ZenohScoutResultNative {
    let now = zenohscout_now_millis();
    let mut peers = raw.lines().filter_map(|line| zenohscout_parse_discovery_key(line.trim(), &config.key_prefix, now)).filter(|peer| peer.node != config.node || peer.oracle != config.oracle).collect::<Vec<_>>();
    peers.sort_by(|left, right| left.node.cmp(&right.node).then(left.oracle.cmp(&right.oracle)));
    ZenohScoutResultNative { ok: true, enabled: true, locator: config.locator.clone(), key_prefix: config.key_prefix.clone(), total: peers.len(), peers, error: None, hint: None }
}

async fn zenohscout_execute_scout(args: &ZenohScoutArgs, timeout_ms: u64) -> CliOutput {
    let scout = zenohscout_fetch_discoveries(args.all, args.limit, timeout_ms).await;
    if args.json { return zenohscout_ok(&zenohscout_json(&scout)); }
    match scout {
        ZenohScoutDiscoveryResult::Ok(response) => zenohscout_ok(&zenohscout_format_discoveries(&response)),
        ZenohScoutDiscoveryResult::Err(error) => zenohscout_ok(&zenohscout_format_discovery_error(&error)),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum ZenohScoutDiscoveryResult { Ok(ZenohScoutDiscoveryResponse), Err(ZenohScoutDiscoveryError) }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ZenohScoutDiscoveryResponse {
    ok: bool,
    total: usize,
    shown: usize,
    filtered: bool,
    peers: Vec<ZenohScoutDiscoveryRow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ZenohScoutDiscoveryRow {
    zid: String,
    node: String,
    oracle: String,
    host: String,
    locators: Vec<String>,
    capabilities: Vec<String>,
    oracles: Vec<String>,
    #[serde(rename = "firstSeen")]
    first_seen: String,
    #[serde(rename = "lastSeen")]
    last_seen: String,
    #[serde(rename = "seenRel")]
    seen_rel: String,
    paired: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ZenohScoutDiscoveryError {
    ok: bool,
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
}

async fn zenohscout_fetch_discoveries(all: bool, limit: Option<usize>, timeout_ms: u64) -> ZenohScoutDiscoveryResult {
    let request = zenohscout_discoveries_request(all, limit);
    tokio::task::spawn_blocking(move || zenohscout_fetch_discoveries_blocking(&request, timeout_ms))
        .await
        .unwrap_or_else(|error| zenohscout_discovery_err("daemon_unreachable", Some(format!("{error} — is `maw serve` running?")), None))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZenohScoutHttpRequest { port: u16, path: String }

fn zenohscout_discoveries_request(all: bool, limit: Option<usize>) -> ZenohScoutHttpRequest {
    let port = zenohscout_read_config_json().and_then(|value| value.get("port").and_then(serde_json::Value::as_u64)).and_then(|port| u16::try_from(port).ok()).unwrap_or(3456);
    let mut query = Vec::new();
    if all { query.push("all=1".to_owned()); }
    if let Some(limit) = limit { query.push(format!("limit={limit}")); }
    let suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
    ZenohScoutHttpRequest { port, path: format!("/api/peers/discoveries{suffix}") }
}

fn zenohscout_fetch_discoveries_blocking(request: &ZenohScoutHttpRequest, timeout_ms: u64) -> ZenohScoutDiscoveryResult {
    let timeout = std::time::Duration::from_millis(timeout_ms.min(5_000));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], request.port));
    let mut stream = match std::net::TcpStream::connect_timeout(&addr, timeout) {
        Ok(stream) => stream,
        Err(error) => return zenohscout_discovery_err("daemon_unreachable", Some(format!("{error} — is `maw serve` running?")), None),
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request_text = format!("GET {} HTTP/1.1\r\nHost: localhost:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n", request.path, request.port);
    if let Err(error) = std::io::Write::write_all(&mut stream, request_text.as_bytes()) {
        return zenohscout_discovery_err("daemon_unreachable", Some(format!("{error} — is `maw serve` running?")), None);
    }
    let mut response = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut stream, &mut response) {
        return zenohscout_discovery_err("daemon_unreachable", Some(format!("{error} — is `maw serve` running?")), None);
    }
    zenohscout_decode_discovery_response(&response)
}

fn zenohscout_decode_discovery_response(response: &str) -> ZenohScoutDiscoveryResult {
    let Some((head, body)) = response.split_once("

") else { return zenohscout_discovery_err("parse_error", Some("invalid http response".to_owned()), None); };
    let status_code = head.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    if (200..300).contains(&status_code) {
        return match serde_json::from_str::<ZenohScoutDiscoveryResponse>(body) {
            Ok(body) => ZenohScoutDiscoveryResult::Ok(body),
            Err(error) => zenohscout_discovery_err("parse_error", Some(error.to_string()), Some(status_code)),
        };
    }
    let body = serde_json::from_str::<serde_json::Value>(body).ok();
    if status_code == 404 { return zenohscout_discovery_err("discovery_endpoint_missing", Some("daemon doesn't expose /api/peers/discoveries — restart `maw serve` after upgrading".to_owned()), Some(404)); }
    let error = body.as_ref().and_then(|value| value.get("error")).and_then(serde_json::Value::as_str).map_or_else(|| format!("http_{status_code}"), ToOwned::to_owned);
    let hint = body.as_ref().and_then(|value| value.get("hint")).and_then(serde_json::Value::as_str).map(ToOwned::to_owned);
    zenohscout_discovery_err(&error, hint, Some(status_code))
}

fn zenohscout_discovery_err(error: &str, hint: Option<String>, status: Option<u16>) -> ZenohScoutDiscoveryResult {
    ZenohScoutDiscoveryResult::Err(ZenohScoutDiscoveryError { ok: false, error: error.to_owned(), hint, status })
}

fn zenohscout_render_both(zenoh: &ZenohScoutResultNative, scout: &ZenohScoutDiscoveryResult, json: bool) -> CliOutput {
    let scout_ok = matches!(scout, ZenohScoutDiscoveryResult::Ok(response) if response.ok);
    let zenoh_usable = zenoh.enabled && zenoh.ok;
    let ok = zenoh_usable || scout_ok;
    if json {
        return zenohscout_ok(&serde_json::to_string_pretty(&json!({"ok": ok, "zenoh": zenoh, "scout": scout})).expect("json"));
    }
    zenohscout_ok(&format!("zenoh:\n{}\n\nscout:\n{}", zenohscout_indent(&zenohscout_format_result(zenoh)), zenohscout_indent(&zenohscout_format_discovery_any(scout))))
}

fn zenohscout_render_result(result: &ZenohScoutResultNative, json: bool) -> CliOutput {
    if json { zenohscout_ok(&zenohscout_json(result)) } else { zenohscout_ok(&zenohscout_format_result(result)) }
}

fn zenohscout_format_result(result: &ZenohScoutResultNative) -> String {
    if !result.enabled { return format!("zenoh-scout disabled\n  locator: {}\n  hint: {}", result.locator, result.hint.as_deref().unwrap_or("set zenoh.scout.enabled=true")); }
    if !result.ok { return format!("zenoh-scout unavailable\n  locator: {}\n  error: {}\n  hint: {}", result.locator, result.error.as_deref().unwrap_or("unknown"), result.hint.as_deref().unwrap_or("check zenohd remote-api")); }
    if result.peers.is_empty() { return format!("no zenoh discoveries\n  locator: {}\n  key: {}/**", result.locator, result.key_prefix); }
    zenohscout_format_zenoh_table(&result.peers)
}

fn zenohscout_format_zenoh_table(peers: &[ZenohScoutPeerNative]) -> String {
    let header = ["zid", "node", "oracle", "host", "caps"];
    let rows = peers.iter().map(|peer| vec![format!("{}…", peer.zid.trim_start_matches("zenoh:").chars().take(8).collect::<String>()), peer.node.clone(), peer.oracle.clone(), peer.host.clone(), if peer.capabilities.is_empty() { "-".to_owned() } else { peer.capabilities.join(",") }]).collect::<Vec<_>>();
    zenohscout_table(&header, &rows)
}

fn zenohscout_format_discovery_any(result: &ZenohScoutDiscoveryResult) -> String {
    match result {
        ZenohScoutDiscoveryResult::Ok(response) => zenohscout_format_discoveries(response),
        ZenohScoutDiscoveryResult::Err(error) => zenohscout_format_discovery_error(error),
    }
}

fn zenohscout_format_discoveries(response: &ZenohScoutDiscoveryResponse) -> String {
    if response.peers.is_empty() { return if response.filtered { "no unpaired discoveries (pass --all to include already-paired)".to_owned() } else { "no discoveries".to_owned() }; }
    let header = ["zid", "node", "oracle", "host", "seen", "paired", "caps"];
    let rows = response.peers.iter().map(|peer| vec![format!("{}…", peer.zid.chars().take(8).collect::<String>()), peer.node.clone(), peer.oracle.clone(), peer.host.clone(), peer.seen_rel.clone(), if peer.paired { "✓".to_owned() } else { "-".to_owned() }, if peer.capabilities.is_empty() { "-".to_owned() } else { peer.capabilities.join(",") }]).collect::<Vec<_>>();
    let mut text = zenohscout_table(&header, &rows);
    if response.total > response.shown {
        let _ = write!(
            text,
            "\n\n({}/{} shown — pass --limit N to widen)",
            response.shown, response.total
        );
    }
    text
}

fn zenohscout_format_discovery_error(error: &ZenohScoutDiscoveryError) -> String {
    match &error.hint { Some(hint) => format!("{} — {hint}", error.error), None => error.error.clone() }
}

fn zenohscout_table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths = header.iter().enumerate().map(|(index, value)| rows.iter().map(|row| row[index].len()).max().unwrap_or(0).max(value.len())).collect::<Vec<_>>();
    let mut lines = Vec::with_capacity(rows.len() + 2);
    lines.push(zenohscout_table_row(&header.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>(), &widths));
    lines.push(zenohscout_table_row(&widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>(), &widths));
    lines.extend(rows.iter().map(|row| zenohscout_table_row(row, &widths)));
    lines.join("\n")
}

fn zenohscout_table_row(cols: &[String], widths: &[usize]) -> String {
    cols.iter().enumerate().map(|(index, col)| format!("{col:<width$}", width = widths[index])).collect::<Vec<_>>().join("  ")
}

fn zenohscout_discovery_key(config: &ZenohScoutConfigNative) -> String {
    [config.key_prefix.clone(), zenohscout_base64url(config.node.as_bytes()), zenohscout_base64url(config.oracle.as_bytes()), zenohscout_base64url(config.api_url.as_bytes()), zenohscout_base64url(config.capabilities.join(",").as_bytes()), "alive".to_owned()].join("/")
}

fn zenohscout_parse_discovery_key(key: &str, prefix: &str, now_ms: u64) -> Option<ZenohScoutPeerNative> {
    let prefix = prefix.trim_end_matches('/');
    let rest = key.strip_prefix(&format!("{prefix}/"))?.split('/').collect::<Vec<_>>();
    if rest.len() != 5 || rest[4] != "alive" { return None; }
    let node = String::from_utf8(zenohscout_base64url_decode(rest[0])?).ok()?;
    let oracle = String::from_utf8(zenohscout_base64url_decode(rest[1])?).ok()?;
    let url = String::from_utf8(zenohscout_base64url_decode(rest[2])?).ok()?;
    let caps = String::from_utf8(zenohscout_base64url_decode(rest[3])?).ok()?;
    let iso = zenohscout_iso_millis(now_ms);
    Some(ZenohScoutPeerNative { zid: format!("zenoh:{:016x}", zenohscout_stable_hash(key)), node, oracle: oracle.clone(), host: zenohscout_host_from_url(&url), locators: vec![url], capabilities: caps.split(',').filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect(), oracles: vec![oracle], first_seen: iso.clone(), last_seen: iso, seen_rel: "now".to_owned(), paired: false, transport: "zenoh".to_owned() })
}

fn zenohscout_base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let b0 = bytes[index];
        let b1 = bytes.get(index + 1).copied().unwrap_or(0);
        let b2 = bytes.get(index + 2).copied().unwrap_or(0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if index + 1 < bytes.len() { out.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char); }
        if index + 2 < bytes.len() { out.push(TABLE[(b2 & 0b11_1111) as usize] as char); }
        index += 3;
    }
    out
}

fn zenohscout_base64url_decode(value: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut bit_len = 0u8;
    let mut out = Vec::new();
    for byte in value.bytes() {
        let val = zenohscout_base64url_value(byte)?;
        bits = (bits << 6) | u32::from(val);
        bit_len += 6;
        if bit_len >= 8 {
            bit_len -= 8;
            out.push(((bits >> bit_len) & 0xff) as u8);
        }
    }
    Some(out)
}

fn zenohscout_base64url_value(byte: u8) -> Option<u8> {
    match byte { b'A'..=b'Z' => Some(byte - b'A'), b'a'..=b'z' => Some(byte - b'a' + 26), b'0'..=b'9' => Some(byte - b'0' + 52), b'-' => Some(62), b'_' => Some(63), _ => None }
}

fn zenohscout_host_from_url(url: &str) -> String {
    url.split_once("://").map_or_else(
        || url.to_owned(),
        |(_, rest)| rest.split('/').next().unwrap_or(rest).to_owned(),
    )
}

fn zenohscout_stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.bytes() { hash = hash.wrapping_mul(0x100_0000_01b3) ^ u64::from(byte); }
    hash
}

fn zenohscout_now_millis() -> u64 {
    std::env::var(ZENOHSCOUT_FAKE_NOW_ENV).ok().and_then(|value| value.parse::<u64>().ok()).unwrap_or_else(|| u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX))
}

fn zenohscout_iso_millis(millis: u64) -> String {
    let seconds = millis / 1000;
    let millis_part = millis % 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = zenohscout_civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z")
}

fn zenohscout_civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch.saturating_add(719_468);
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

fn zenohscout_indent(value: &str) -> String { value.split('\n').map(|line| format!("  {line}")).collect::<Vec<_>>().join("\n") }

fn zenohscout_json<T: serde::Serialize>(value: &T) -> String { format!("{}\n", serde_json::to_string_pretty(value).expect("json")) }

fn zenohscout_ok(text: &str) -> CliOutput { CliOutput { code: 0, stdout: format!("{}{}", text, if text.ends_with('\n') { "" } else { "\n" }), stderr: String::new() } }

fn zenohscout_error(message: &str) -> CliOutput { CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n{ZENOHSCOUT_USAGE}\n") } }

#[cfg(test)]
mod zenohscout_tests {
    use super::*;

    fn zenohscout_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn zenohscout_dispatch_registers_aliases_and_parser_guards() {
        assert_eq!(DISPATCH_103.len(), 2);
        assert_eq!(dispatcher_status("scout"), DispatchKind::Native);
        assert_eq!(dispatcher_status("zenoh-scout"), DispatchKind::Native);
        assert!(zenohscout_parse_args(&zenohscout_args(&["--transport", "both", "--force", "--all", "--limit", "5", "--locator", "ws://127.0.0.1:10000"])).is_ok());
        assert!(zenohscout_parse_args(&zenohscout_args(&["--transport", "bad"])).unwrap_err().contains("zenoh|scout|both"));
        assert!(zenohscout_parse_args(&zenohscout_args(&["--locator", "-secret"])).unwrap_err().contains("not allowed"));
        assert!(zenohscout_parse_args(&zenohscout_args(&["--timeout", "0"])).unwrap_err().contains("positive"));
    }

    #[test]
    fn zenohscout_key_roundtrip_and_fake_result_are_hermetic() {
        let config = ZenohScoutConfigNative { enabled: true, locator: "ws://127.0.0.1:10000".to_owned(), timeout_ms: 750, key_prefix: ZENOHSCOUT_DEFAULT_KEY_PREFIX.to_owned(), node: "alpha".to_owned(), oracle: "bo".to_owned(), api_url: "http://alpha:3456".to_owned(), capabilities: vec!["pair".to_owned(), "feed".to_owned(), "send".to_owned()] };
        let peer = ZenohScoutConfigNative { node: "beta".to_owned(), oracle: "nova".to_owned(), api_url: "http://beta:3456".to_owned(), ..config.clone() };
        let key = zenohscout_discovery_key(&peer);
        let parsed = zenohscout_parse_discovery_key(&key, &config.key_prefix, 1_782_277_200_000).expect("peer");
        assert_eq!(parsed.node, "beta");
        assert_eq!(parsed.oracle, "nova");
        assert_eq!(parsed.host, "beta:3456");
        let result = zenohscout_fake_result(&config, &format!("{}\n{}", zenohscout_discovery_key(&config), key));
        assert_eq!(result.total, 1);
        assert!(zenohscout_format_result(&result).contains("beta"));
    }

    #[tokio::test]
    async fn zenohscout_status_disabled_and_unavailable_do_not_require_zenoh_crate() {
        let status = zenohscout_run_async(&zenohscout_args(&["--status", "--json"])).await;
        assert_eq!(status.code, 0, "{}", status.stderr);
        assert!(status.stdout.contains("zenoh-scout"));
        let disabled = zenohscout_run_async(&zenohscout_args(&[])).await;
        assert_eq!(disabled.code, 0, "{}", disabled.stderr);
        assert!(disabled.stdout.contains("opt-in") || disabled.stdout.contains("disabled"));
        let unavailable = zenohscout_run_async(&zenohscout_args(&["--force"])).await;
        assert_eq!(unavailable.code, 0, "{}", unavailable.stderr);
        assert!(unavailable.stdout.contains("zenoh-scout unavailable"));
        assert!(unavailable.stdout.contains("supply-chain review"));
    }
}
