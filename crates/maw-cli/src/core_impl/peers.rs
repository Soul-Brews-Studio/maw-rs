const DISPATCH_104: &[DispatcherEntry] = &[
    DispatcherEntry { command: "peers", handler: Handler::Sync(peers_run_command) },
    DispatcherEntry { command: "peer", handler: Handler::Sync(peers_run_command) },
];

const PEERS_HELP: &str = "usage: maw peers <add|list|info|probe|probe-all|map|accept|remove|forget> [...]\n  add       <alias> <url> [--node <name>] [--ssh <target>] [--user <name>] [--allow-unreachable]\n            — register alias (auto-probes /info). Exits non-zero on handshake failure:\n              2=UNKNOWN/BAD_BODY/TLS  3=DNS  4=REFUSED  5=TIMEOUT  6=HTTP_4XX/5XX  7=UNREACHABLE\n            --ssh sets the SSH config alias/target for cross-node attach; --user overrides SSH user.\n            --allow-unreachable keeps exit 0 even when the probe fails (CI/bootstrap).\n  list      [--discovered] [--all] [--json] [--limit N]\n            — tabular list of all peers. --discovered: LAN candidates from Scout (#1237).\n              --all: include already-paired (default hides). --limit: cap rows (default 50).\n  info      <alias>                         — JSON details for one peer (includes lastError if set)\n  probe     <alias>                         — re-run /info handshake; updates lastSeen / lastError (#565)\n  probe-all [--timeout <ms>] [--allow-unreachable]\n            — probe every peer in parallel; prints liveness table. Exit = worst PROBE_EXIT_CODE (#669).\n  accept    <node|zid-prefix> [--alias X] | --all (#1237)\n            — pair with a Scout-discovered peer. Shortest unambiguous prefix wins.\n              Refuses if pubkey already pins under a different alias (impersonation guard).\n  map       — federation map: node, oracle, up/down, resolved IP, and flags\n              (loopback-self = probe hit our own serve; dup-node = shared node name).\n  remove    <alias>                         — remove (idempotent)\n  forget    <alias>                         — clear cached pubkey so next contact re-TOFUs (#804 Step 2)\n\nstorage: maw state peers.json (v1; reads legacy ~/.maw/peers.json during migration)";
const PEERS_DEFAULT_STALE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const PEERS_DEFAULT_PROBE_TIMEOUT_MS: u64 = 2_000;
const PEERS_FAKE_NOW_ENV: &str = "MAW_RS_PEERS_FAKE_NOW";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
struct PeersStoreNative {
    #[serde(default = "peers_version_one")]
    version: u8,
    #[serde(default)]
    peers: std::collections::BTreeMap<String, PeersPeerNative>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct PeersPeerNative {
    url: String,
    node: Option<String>,
    added_at: String,
    last_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pubkey_first_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ssh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ssh_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_error: Option<String>,
}

fn peers_version_one() -> u8 { 1 }

fn peers_run_command(argv: &[String]) -> CliOutput {
    match peers_dispatch(argv) {
        Ok(output) => output,
        Err(error) => peers_error(&error),
    }
}

fn peers_dispatch(argv: &[String]) -> Result<CliOutput, String> {
    peers_validate_argv(argv)?;
    let positional = argv.iter().filter(|arg| !arg.starts_with("--")).map(String::as_str).collect::<Vec<_>>();
    let Some(sub) = positional.first().copied() else { return Ok(peers_ok(&format!("{PEERS_HELP}\n"))); };
    match sub {
        "help" | "--help" | "-h" => Ok(peers_ok(&format!("{PEERS_HELP}\n"))),
        "add" => peers_cmd_add(argv, &positional),
        "list" | "ls" => peers_cmd_list(argv),
        "info" => peers_cmd_info(&positional),
        "remove" | "rm" => peers_cmd_remove(&positional),
        "forget" => peers_cmd_forget(&positional),
        "probe" => peers_cmd_probe(&positional),
        "probe-all" => peers_cmd_probe_all(argv),
        "map" => Ok(peers_cmd_map()),
        "accept" => peers_cmd_accept(argv, &positional),
        _ => Ok(CliOutput { code: 1, stdout: format!("{PEERS_HELP}\n"), stderr: format!("maw peers: unknown subcommand \"{sub}\" (expected add|list|info|probe|probe-all|accept|remove|forget)\n") }),
    }
}

fn peers_validate_argv(argv: &[String]) -> Result<(), String> {
    for (idx, arg) in argv.iter().enumerate() {
        if arg == "--" { return Err("maw peers: -- separator is not allowed".to_owned()); }
        if arg.starts_with('-') && !peers_known_flag(arg) { return Err(format!("maw peers: unknown flag {arg}")); }
        if peers_flag_needs_value(arg) {
            let value = argv.get(idx + 1).ok_or_else(|| format!("{arg} requires a value"))?;
            peers_validate_value(arg, value)?;
        }
        if peers_flag_with_inline_value(arg) {
            let (flag, value) = arg.split_once('=').unwrap_or((arg, ""));
            peers_validate_value(flag, value)?;
        }
    }
    Ok(())
}

fn peers_known_flag(arg: &str) -> bool {
    matches!(arg, "--node" | "--ssh" | "--user" | "--allow-unreachable" | "--timeout" | "--alias" | "--discovered" | "--all" | "--json" | "--limit" | "--help" | "-h") || arg.starts_with("--node=") || arg.starts_with("--ssh=") || arg.starts_with("--user=") || arg.starts_with("--timeout=") || arg.starts_with("--alias=") || arg.starts_with("--limit=")
}

fn peers_flag_needs_value(arg: &str) -> bool { matches!(arg, "--node" | "--ssh" | "--user" | "--timeout" | "--alias" | "--limit") }
fn peers_flag_with_inline_value(arg: &str) -> bool { ["--node=", "--ssh=", "--user=", "--timeout=", "--alias=", "--limit="].iter().any(|prefix| arg.starts_with(prefix)) }

fn peers_validate_value(flag: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) { return Err(format!("{flag} requires a safe value")); }
    Ok(())
}

fn peers_cmd_add(argv: &[String], positional: &[&str]) -> Result<CliOutput, String> {
    let alias = *positional.get(1).ok_or("usage: maw peers add <alias> <url> [--node <name>] [--ssh <target>] [--user <name>] [--allow-unreachable]")?;
    let url = *positional.get(2).ok_or("usage: maw peers add <alias> <url> [--node <name>] [--ssh <target>] [--user <name>] [--allow-unreachable]")?;
    peers_validate_alias(alias)?;
    peers_validate_url(url)?;
    let node = peers_flag_value(argv, "--node");
    if let Some(node) = &node { peers_validate_node(node)?; }
    let ssh = peers_flag_value(argv, "--ssh").map(|value| peers_clean_optional(&value, "--ssh")).transpose()?;
    let ssh_user = peers_flag_value(argv, "--user").map(|value| peers_clean_optional(&value, "--user")).transpose()?;
    let mut store = peers_load_store();
    let overwrote = store.peers.contains_key(alias);
    // On re-add (upsert) carry over the existing TOFU anchor so repointing a URL
    // does not reset trust provenance: peers_apply_probe_result only bumps
    // pubkey_first_seen when the probed key differs from the cached one (#678).
    let existing = store.peers.get(alias).cloned();
    let (prior_pubkey, prior_first_seen) = match &existing {
        Some(prior) => (prior.pubkey.clone(), prior.pubkey_first_seen.clone()),
        None => (None, None),
    };
    let now = peers_now_iso();
    let mut peer = PeersPeerNative { url: url.to_owned(), node, added_at: now.clone(), last_seen: None, ssh, ssh_user, pubkey: prior_pubkey, pubkey_first_seen: prior_first_seen, ..PeersPeerNative::default() };
    let probe = if argv.iter().any(|arg| arg == "--allow-unreachable") {
        None
    } else {
        let probe = peers_probe_peer(&peer.url, PEERS_DEFAULT_PROBE_TIMEOUT_MS, &now);
        peers_apply_probe_result(&mut peer, &probe, &now)?;
        Some(probe)
    };
    store.peers.insert(alias.to_owned(), peer.clone());
    peers_save_store(&store)?;
    let mut stdout = String::new();
    if overwrote { let _ = writeln!(stdout, "warning: alias \"{alias}\" already existed — overwriting"); }
    let _ = writeln!(stdout, "added {alias} → {url}{}", peer.node.as_ref().map(|node| format!(" ({node})")).unwrap_or_default());
    let Some(probe) = probe else { return Ok(peers_ok(&stdout)); };
    let code = peers_probe_exit_code(&probe);
    if code == 0 {
        let _ = writeln!(stdout, "\x1b[32m✓\x1b[0m peer handshake ok");
        return Ok(peers_ok(&stdout));
    }
    Ok(CliOutput { code, stdout, stderr: peers_probe_stderr(alias, &peer.url, &probe) })
}

fn peers_cmd_list(argv: &[String]) -> Result<CliOutput, String> {
    if argv.iter().any(|arg| arg == "--discovered") { return peers_cmd_list_discovered(argv); }
    let store = peers_load_store();
    let rows = store.peers.into_iter().map(|(alias, peer)| peers_list_row(alias, peer)).collect::<Vec<_>>();
    Ok(peers_ok(&format!("{}\n", peers_format_list(&rows))))
}

fn peers_cmd_list_discovered(argv: &[String]) -> Result<CliOutput, String> {
    if let Some(raw) = peers_flag_value(argv, "--limit") { peers_parse_positive_usize(&raw, "usage: maw peers list --discovered [--all] [--json] [--limit N]")?; }
    let json = argv.iter().any(|arg| arg == "--json");
    if json {
        return Ok(peers_ok("{\n  \"ok\": false,\n  \"error\": \"daemon_unreachable\",\n  \"hint\": \"is maw serve running?\"\n}\n"));
    }
    Ok(CliOutput { code: 1, stdout: String::new(), stderr: "\x1b[31m✗\x1b[0m daemon_unreachable — is maw serve running?\n".to_owned() })
}

fn peers_cmd_info(positional: &[&str]) -> Result<CliOutput, String> {
    let alias = *positional.get(1).ok_or("usage: maw peers info <alias>")?;
    peers_validate_alias(alias)?;
    let store = peers_load_store();
    let Some(peer) = store.peers.get(alias) else { return Err(format!("peer \"{alias}\" not found")); };
    let mut value = serde_json::to_value(peer).map_err(|error| format!("peers: render info: {error}"))?;
    if let serde_json::Value::Object(map) = &mut value { map.insert("alias".to_owned(), serde_json::Value::String(alias.to_owned())); }
    let json = serde_json::to_string_pretty(&value).map_err(|error| format!("peers: render info: {error}"))?;
    Ok(peers_ok(&format!("{json}\n")))
}

fn peers_cmd_remove(positional: &[&str]) -> Result<CliOutput, String> {
    let alias = *positional.get(1).ok_or("usage: maw peers remove <alias>")?;
    peers_validate_alias(alias)?;
    let mut store = peers_load_store();
    let removed = store.peers.remove(alias).is_some();
    peers_save_store(&store)?;
    let stdout = if removed { format!("removed {alias}\n") } else { format!("no-op: {alias} not present\n") };
    Ok(peers_ok(&stdout))
}

fn peers_cmd_forget(positional: &[&str]) -> Result<CliOutput, String> {
    let alias = *positional.get(1).ok_or("usage: maw peers forget <alias>")?;
    peers_validate_alias(alias)?;
    let mut store = peers_load_store();
    let Some(peer) = store.peers.get_mut(alias) else { return Err(format!("peer \"{alias}\" not found")); };
    if peer.pubkey.is_some() {
        peer.pubkey = None;
        peer.pubkey_first_seen = None;
        peers_save_store(&store)?;
        Ok(peers_ok(&format!("forgot pubkey for {alias} — next contact will re-TOFU\n")))
    } else {
        Ok(peers_ok(&format!("no-op: {alias} has no cached pubkey (legacy peer)\n")))
    }
}

fn peers_cmd_probe(positional: &[&str]) -> Result<CliOutput, String> {
    let alias = *positional.get(1).ok_or("usage: maw peers probe <alias>")?;
    peers_validate_alias(alias)?;
    let mut store = peers_load_store();
    let Some(peer) = store.peers.get(alias) else { return Err(format!("peer \"{alias}\" not found")); };
    let url = peer.url.clone();
    let now = peers_now_iso();
    let probe = peers_probe_peer(&url, PEERS_DEFAULT_PROBE_TIMEOUT_MS, &now);
    if let Some(peer) = store.peers.get_mut(alias) { peers_apply_probe_result(peer, &probe, &now)?; }
    peers_save_store(&store)?;
    let code = peers_probe_exit_code(&probe);
    let mut stdout = format!("probing {alias} → {url} ...\n");
    if code == 0 { let _ = writeln!(stdout, "\x1b[32m✓\x1b[0m ok"); }
    Ok(CliOutput { code, stdout, stderr: peers_probe_stderr(alias, &url, &probe) })
}

/// One probe-all row: `(alias, url, status_code_string)`.
type PeersProbeRow = (String, String, String);

/// Probe every stored peer and persist the refreshed identity / lastSeen back to
/// `peers.json` through the single peer-store writer (`peers_apply_probe_result` +
/// `peers_save_store`). Shared by `maw peers probe-all` and the serve background
/// refresh so probe results have exactly ONE write path — never the read-only
/// federation-map render (#677/#684). Returns the per-peer rows and the worst probe
/// exit code (`0` when the store is empty).
fn peers_probe_all_and_persist(timeout_ms: u64) -> Result<(Vec<PeersProbeRow>, i32), String> {
    peers_probe_all_and_persist_with(timeout_ms, &peers_probe_peer)
}

/// See [`peers_probe_all_and_persist`]. The `probe` seam is split out so a test can
/// drive the persistence deterministically without real network I/O.
///
/// Lost-update safety: the peer list is snapshotted for probing, but each result is
/// applied onto a **freshly re-read** store just before saving — so a concurrent
/// `maw peers add` / `remove` during the slow, network-bound probe loop is not
/// clobbered by a stale in-memory copy (the sweep's load→save window is otherwise
/// ~timeout×peers long, ~12s on a 10-peer fleet). Each result only touches the
/// probe-owned fields (`lastSeen` / `lastError` / `node` / `identity` / `pubkey` /
/// `authOk`) via `peers_apply_probe_result`, never `url` or `addedAt`, so the sweep
/// and the CLI cannot fight over a peer's non-probe fields. A residual sub-millisecond
/// reload→save window remains; a lock file would close it (follow-up, #689).
fn peers_probe_all_and_persist_with(
    timeout_ms: u64,
    probe: &dyn Fn(&str, u64, &str) -> maw_peer::ProbePeerResult,
) -> Result<(Vec<PeersProbeRow>, i32), String> {
    let store = peers_load_store();
    if store.peers.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let targets = store
        .peers
        .iter()
        .map(|(alias, peer)| (alias.clone(), peer.url.clone()))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut worst = 0;
    let mut results = Vec::new();
    for (alias, url) in &targets {
        let now = peers_now_iso();
        let result = probe(url, timeout_ms, &now);
        worst = worst.max(peers_probe_exit_code(&result));
        let status = result.error.as_ref().map_or("OK", |error| error.code.as_str());
        rows.push((alias.clone(), url.clone(), status.to_owned()));
        results.push((alias.clone(), result, now));
    }
    // Re-read fresh so a concurrent add/remove during the probe loop survives; apply
    // only the probe-owned fields onto whatever peers still exist.
    let mut fresh = peers_load_store();
    for (alias, result, now) in &results {
        if let Some(peer) = fresh.peers.get_mut(alias) {
            peers_apply_probe_result(peer, result, now)?;
        }
    }
    peers_save_store(&fresh)?;
    Ok((rows, worst))
}

fn peers_cmd_probe_all(argv: &[String]) -> Result<CliOutput, String> {
    let timeout = if let Some(raw) = peers_flag_value(argv, "--timeout") { peers_parse_positive_u64(&raw, "usage: maw peers probe-all [--timeout <ms>]")? } else { PEERS_DEFAULT_PROBE_TIMEOUT_MS };
    let (rows, worst) = peers_probe_all_and_persist(timeout)?;
    let mut stdout = String::from("alias  url  status\n-----  ---  ------\n");
    for (alias, url, status) in &rows {
        let _ = writeln!(stdout, "{alias}  {url}  {status}");
    }
    let allow = argv.iter().any(|arg| arg == "--allow-unreachable");
    Ok(CliOutput { code: if allow { 0 } else { worst }, stdout, stderr: String::new() })
}

fn peers_cmd_accept(argv: &[String], positional: &[&str]) -> Result<CliOutput, String> {
    if argv.iter().any(|arg| arg == "--all") { return Ok(peers_ok("no unpaired discoveries\n")); }
    let _id = positional.get(1).ok_or("usage: maw peers accept <node|zid-prefix> [--alias X] | --all")?;
    if let Some(alias) = peers_flag_value(argv, "--alias") { peers_validate_alias(&alias)?; }
    Err("daemon_unreachable".to_owned())
}

fn peers_list_row(alias: String, peer: PeersPeerNative) -> (String, PeersPeerNative, bool, Option<u64>) {
    let age = peers_stale_age_ms(&peer);
    let stale = age.is_none_or(|value| value > peers_stale_ttl_ms());
    (alias, peer, stale, age)
}

fn peers_probe_peer(url: &str, timeout_ms: u64, now: &str) -> maw_peer::ProbePeerResult {
    let info = peers_fetch_info(url, timeout_ms);
    // Best-effort /api/identity fetch so TOFU can pin the pubkey (#545); older peers without the endpoint stay unpinned.
    let identity = if matches!(info, maw_peer::ProbeInfoOutcome::Body(_)) { peers_fetch_identity(url, timeout_ms) } else { None };
    // Resolve the URL host to an IP so the map can flag the `m5.local → 127.0.0.1`
    // trap (loopback = the probe hit our OWN serve, not the remote peer).
    let resolved_ip = peers_resolve_ip(url);
    // Read-only signed auth probe (POST /api/probe verifies the v3 from-signature
    // and has no side effect) — only when /info succeeded, so we do not sign
    // requests to an unreachable host. None when we cannot sign (no key/token).
    let auth_probe = if matches!(info, maw_peer::ProbeInfoOutcome::Body(_)) {
        federation_probe_auth(url, timeout_ms)
    } else {
        maw_transport::PeerProbeAuthResult::default()
    };
    maw_peer::probe_peer_from_plan(&maw_peer::ProbePeerPlan {
        url: url.to_owned(),
        now: now.to_owned(),
        dns_error: None,
        info,
        identity,
        resolved_ip,
        auth_ok: auth_probe.ok,
        auth_error: auth_probe.reason,
    })
}

/// Resolve the host in a peer URL to a **routable** IP, so a probe can tell a
/// real remote from our own serve (`m5.local` may resolve to `127.0.0.1`). For a
/// non-loopback host, link-local (`fe80::/10`, `169.254/16`) and loopback
/// addresses are skipped and IPv4 is preferred — otherwise `getaddrinfo` can
/// hand back a zone-less link-local IPv6 first that nothing can connect to.
/// Best effort: `None` when the URL has no host or DNS fails.
fn peers_resolve_ip(url: &str) -> Option<String> {
    use std::net::{IpAddr, ToSocketAddrs};
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default().unwrap_or(3456);
    let host_is_loopback =
        host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let routable = |ip: &IpAddr| -> bool {
        if ip.is_loopback() {
            return false;
        }
        match ip {
            IpAddr::V4(v4) => !v4.is_link_local(),
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
        }
    };
    let mut addrs = (host, port)
        .to_socket_addrs()
        .ok()?
        .filter(|addr| host_is_loopback || routable(&addr.ip()))
        .collect::<Vec<_>>();
    addrs.sort_by_key(|addr| u8::from(addr.is_ipv6()));
    addrs.into_iter().next().map(|addr| addr.ip().to_string())
}

fn peers_fetch_identity(url: &str, timeout_ms: u64) -> Option<maw_peer::ProbeRemoteIdentity> {
    let identity_url = reqwest::Url::parse(url).and_then(|base| base.join("/api/identity")).map(|url| url.to_string()).ok()?;
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        Some(runtime.block_on(peers_fetch_identity_async(&identity_url, std::time::Duration::from_millis(timeout_ms))))
    });
    handle.join().ok().flatten()
}

async fn peers_fetch_identity_async(url: &str, timeout: std::time::Duration) -> maw_peer::ProbeRemoteIdentity {
    let Ok(client) = reqwest::Client::builder().timeout(timeout).redirect(reqwest::redirect::Policy::none()).build() else { return maw_peer::ProbeRemoteIdentity::FetchError; };
    let Ok(response) = client.get(url).send().await else { return maw_peer::ProbeRemoteIdentity::FetchError; };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND { return maw_peer::ProbeRemoteIdentity::Missing; }
    if !status.is_success() { return maw_peer::ProbeRemoteIdentity::HttpError; }
    match response.json::<serde_json::Value>().await {
        Ok(value) => peers_probe_identity_body(&value),
        Err(_) => maw_peer::ProbeRemoteIdentity::MalformedJson,
    }
}

fn peers_probe_identity_body(value: &serde_json::Value) -> maw_peer::ProbeRemoteIdentity {
    maw_peer::ProbeRemoteIdentity::Body { pubkey: peers_json_string(value, "pubkey"), oracle: peers_json_string(value, "oracle"), node: peers_json_string(value, "node") }
}

fn peers_fetch_info(url: &str, timeout_ms: u64) -> maw_peer::ProbeInfoOutcome {
    let info_url = match peers_probe_info_url(url) {
        Ok(value) => value,
        Err(error) => return maw_peer::ProbeInfoOutcome::FetchName { name: "TypeError".to_owned(), message: error },
    };
    let handle = std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(value) => value,
            Err(error) => return maw_peer::ProbeInfoOutcome::FetchName { name: "Error".to_owned(), message: format!("probe runtime failed: {error}") },
        };
        runtime.block_on(peers_fetch_info_async(&info_url, std::time::Duration::from_millis(timeout_ms)))
    });
    handle.join().unwrap_or_else(|_| maw_peer::ProbeInfoOutcome::FetchName { name: "Error".to_owned(), message: "probe runtime panicked".to_owned() })
}

async fn peers_fetch_info_async(url: &str, timeout: std::time::Duration) -> maw_peer::ProbeInfoOutcome {
    let client = match reqwest::Client::builder().timeout(timeout).redirect(reqwest::redirect::Policy::none()).build() {
        Ok(value) => value,
        Err(error) => return peers_reqwest_error(&error),
    };
    let response = match client.get(url).send().await {
        Ok(value) => value,
        Err(error) => return peers_reqwest_error(&error),
    };
    let status = response.status();
    if !status.is_success() { return maw_peer::ProbeInfoOutcome::HttpStatus { status: status.as_u16(), ok: false }; }
    match response.json::<serde_json::Value>().await {
        Ok(value) => peers_probe_info_body(&value),
        Err(_) => maw_peer::ProbeInfoOutcome::InvalidJson,
    }
}

fn peers_probe_info_url(url: &str) -> Result<String, String> {
    reqwest::Url::parse(url).and_then(|base| base.join("/info")).map(|url| url.to_string()).map_err(|error| error.to_string())
}

fn peers_reqwest_error(error: &reqwest::Error) -> maw_peer::ProbeInfoOutcome {
    let message = error.to_string();
    if error.is_timeout() { return maw_peer::ProbeInfoOutcome::FetchName { name: "TimeoutError".to_owned(), message }; }
    if let Some(code) = peers_reqwest_error_code(error) {
        return maw_peer::ProbeInfoOutcome::FetchCode { code: code.to_owned(), message };
    }
    maw_peer::ProbeInfoOutcome::FetchName { name: "Error".to_owned(), message }
}

/// Classify a reqwest probe error into an errno-style code by walking the full
/// cause chain. `HostUnreachable` / `NetworkUnreachable` are matched as typed
/// `io::ErrorKind`s (deterministic), and the substring fallback concatenates
/// every cause's `Display` so it sees the *walked* chain — reqwest's top-level
/// `Display` omits the underlying OS message (e.g. "No route to host"), which
/// is the only clue that identifies a macOS Local Network (TCC) denial (#733).
fn peers_reqwest_error_code(error: &reqwest::Error) -> Option<&'static str> {
    let mut chain = String::new();
    let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = cursor {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => return Some("ECONNREFUSED"),
                std::io::ErrorKind::TimedOut => return Some("ETIMEDOUT"),
                std::io::ErrorKind::HostUnreachable => return Some("EHOSTUNREACH"),
                std::io::ErrorKind::NetworkUnreachable => return Some("ENETUNREACH"),
                _ => {}
            }
        }
        if !chain.is_empty() { chain.push_str(" :: "); }
        // Append this cause's message to the walked chain for substring fallback.
        let _ = std::fmt::Write::write_fmt(&mut chain, std::format_args!("{cause}"));
        cursor = cause.source();
    }

    let lower = chain.to_ascii_lowercase();
    if lower.contains("connection refused") { return Some("ECONNREFUSED"); }
    if lower.contains("timed out") || lower.contains("operation timed out") { return Some("ETIMEDOUT"); }
    if lower.contains("no route to host") || lower.contains("host unreachable") { return Some("EHOSTUNREACH"); }
    if lower.contains("network is unreachable") || lower.contains("network unreachable") { return Some("ENETUNREACH"); }
    if lower.contains("dns") || lower.contains("name or service") || lower.contains("failed to lookup") || lower.contains("nodename nor servname") { return Some("ENOTFOUND"); }
    if lower.contains("certificate") || lower.contains("tls") { return Some("CERT_HAS_EXPIRED"); }
    None
}

fn peers_probe_info_body(value: &serde_json::Value) -> maw_peer::ProbeInfoOutcome {
    maw_peer::ProbeInfoOutcome::Body(maw_peer::ProbeInfoBody { maw: peers_probe_maw_handshake(value.get("maw")), node: peers_json_string(value, "node"), name: peers_json_string(value, "name"), nickname: peers_json_string(value, "nickname") })
}

fn peers_probe_maw_handshake(value: Option<&serde_json::Value>) -> maw_peer::ProbeMawHandshake {
    match value {
        Some(serde_json::Value::Bool(true)) => maw_peer::ProbeMawHandshake::LegacyTrue,
        Some(serde_json::Value::Object(map)) if map.is_empty() => maw_peer::ProbeMawHandshake::EmptyObject,
        Some(serde_json::Value::Object(map)) => maw_peer::ProbeMawHandshake::SchemaObject(map.get("schema").and_then(serde_json::Value::as_str).filter(|schema| !schema.is_empty()).unwrap_or("object").to_owned()),
        None => maw_peer::ProbeMawHandshake::Missing,
        Some(_) => maw_peer::ProbeMawHandshake::OtherTruthy,
    }
}

fn peers_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn peers_apply_probe_result(peer: &mut PeersPeerNative, probe: &maw_peer::ProbePeerResult, now: &str) -> Result<(), String> {
    if let Some(error) = &probe.error {
        peer.last_error = Some(serde_json::to_value(error).map_err(|error| format!("peers: render probe error: {error}"))?);
        return Ok(());
    }
    peer.last_seen = Some(now.to_owned());
    peer.last_error = None;
    if let Some(node) = &probe.node { peer.node = Some(node.clone()); }
    if let Some(nickname) = &probe.nickname { peer.nickname = Some(nickname.clone()); }
    if let Some(pubkey) = &probe.pubkey {
        if peer.pubkey.as_ref() != Some(pubkey) { peer.pubkey_first_seen = Some(now.to_owned()); }
        peer.pubkey = Some(pubkey.clone());
    }
    if let Some(identity) = &probe.identity { peer.identity = Some(serde_json::to_value(identity).map_err(|error| format!("peers: render identity: {error}"))?); }
    peer.auth_ok = probe.auth_ok;
    peer.auth_error.clone_from(&probe.auth_error);
    Ok(())
}

fn peers_probe_exit_code(probe: &maw_peer::ProbePeerResult) -> i32 {
    probe.error.as_ref().map_or(0, |error| probe_exit_code(error.code))
}

fn peers_probe_stderr(alias: &str, url: &str, probe: &maw_peer::ProbePeerResult) -> String {
    probe.error.as_ref().map_or_else(String::new, |error| format!("{}\n", maw_peer::format_probe_error(error, url, alias)))
}

fn peers_format_list(rows: &[(String, PeersPeerNative, bool, Option<u64>)]) -> String {
    if rows.is_empty() { return "no peers".to_owned(); }
    let header = ["alias", "url", "node", "nickname", "lastSeen"];
    let data = rows.iter().map(|(alias, peer, _, _)| [alias.clone(), peer.url.clone(), peer.node.clone().unwrap_or_else(|| "-".to_owned()), peer.nickname.clone().unwrap_or_else(|| "-".to_owned()), peer.last_seen.clone().unwrap_or_else(|| "-".to_owned())]).collect::<Vec<_>>();
    let widths = (0..header.len()).map(|idx| data.iter().map(|cols| cols[idx].len()).chain([header[idx].len()]).max().unwrap_or(0)).collect::<Vec<_>>();
    let format_row = |cols: &[String]| cols.iter().enumerate().map(|(idx, col)| format!("{col:<width$}", width = widths[idx])).collect::<Vec<_>>().join("  ");
    let mut lines = Vec::new();
    lines.push(format_row(&header.map(str::to_owned)));
    lines.push(format_row(&widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>()));
    for (idx, (_alias, _peer, stale, age)) in rows.iter().enumerate() {
        let mut line = format_row(&data[idx]);
        if *stale {
            let suffix = age.map_or_else(
                || "never seen".to_owned(),
                |value| format!("last seen {}d ago", value / PEERS_DEFAULT_STALE_TTL_MS),
            );
            let _ = write!(line, "  \x1b[2m(stale, {suffix})\x1b[0m");
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// One terminal-map row for `maw peers map` — the federation as this node sees
/// it: node + oracle identity, whether the last handshake succeeded, the IP the
/// URL resolves to (loopback = we reached our own serve), and whether the node
/// name is unique (a duplicate makes "us vs them" ambiguous).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeersMapRow {
    alias: String,
    node: String,
    oracle: String,
    reachable: bool,
    resolved_ip: Option<String>,
    loopback_self: bool,
    node_unique: bool,
    auth_ok: Option<bool>,
    auth_error: Option<String>,
}

/// Deterministic core of `maw peers map`: map the peer store to rows, with the
/// IP resolver injected so it is testable without DNS.
fn peers_map_rows(
    store: &PeersStoreNative,
    resolve: impl Fn(&str) -> Option<String>,
) -> Vec<PeersMapRow> {
    let mut node_counts = std::collections::BTreeMap::<String, usize>::new();
    for peer in store.peers.values() {
        if let Some(node) = peer.node.as_deref().filter(|node| !node.is_empty()) {
            *node_counts.entry(node.to_owned()).or_insert(0) += 1;
        }
    }
    store
        .peers
        .iter()
        .map(|(alias, peer)| {
            let node = peer.node.clone().unwrap_or_default();
            let resolved_ip = resolve(&peer.url);
            PeersMapRow {
                alias: alias.clone(),
                oracle: peer
                    .identity
                    .as_ref()
                    .and_then(|identity| identity.get("oracle"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|oracle| !oracle.is_empty())
                    .unwrap_or("-")
                    .to_owned(),
                reachable: peer.last_error.is_none(),
                loopback_self: maw_peer::is_loopback_ip(resolved_ip.as_deref()),
                node_unique: node_counts.get(&node).copied().unwrap_or(0) <= 1 && !node.is_empty(),
                node: if node.is_empty() { "-".to_owned() } else { node },
                resolved_ip,
                auth_ok: peer.auth_ok,
                auth_error: peer.auth_error.clone(),
            }
        })
        .collect()
}

fn peers_format_map(rows: &[PeersMapRow]) -> String {
    if rows.is_empty() {
        return "no peers — the federation is empty (maw peers add <alias> <url>)".to_owned();
    }
    let header = ["alias", "node", "oracle", "reach", "ip", "flags"];
    let data = rows
        .iter()
        .map(|row| {
            let reach = if row.reachable { "up" } else { "down" };
            let ip = row.resolved_ip.clone().unwrap_or_else(|| "-".to_owned());
            let mut flags = Vec::new();
            if row.loopback_self {
                flags.push("loopback-self".to_owned());
            }
            if !row.node_unique {
                flags.push("dup-node".to_owned());
            }
            if row.auth_ok == Some(false) {
                // The reason this flags: `auth_ok` reflects the signed-request
                // handshake used by /api/send, /api/probe, /api/wake -- the
                // action-capable surface, which the fleet DOES always enforce
                // (loopback-exempt only, no config opt-out). It is unrelated
                // to whether read-only endpoints like /api/sessions answer --
                // those are gated separately, by an opt-in bearer token, and
                // are open by design when no token is configured (#685).
                // Bare "auth-fail" with no reason is how #685 happened: a
                // flag nobody could act on. Show the reason whenever it's
                // known; "never negotiated" itself is real, useful information
                // (distinct from "credential rejected"), not a placeholder.
                flags.push(row.auth_error.as_deref().map_or_else(
                    || "auth-fail".to_owned(),
                    |reason| format!("auth-fail:{reason}"),
                ));
            }
            let flags = if flags.is_empty() {
                "-".to_owned()
            } else {
                flags.join(",")
            };
            [
                row.alias.clone(),
                row.node.clone(),
                row.oracle.clone(),
                reach.to_owned(),
                ip,
                flags,
            ]
        })
        .collect::<Vec<_>>();
    let widths = (0..header.len())
        .map(|idx| {
            data.iter()
                .map(|cols| cols[idx].len())
                .chain([header[idx].len()])
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let format_row = |cols: &[String]| {
        cols.iter()
            .enumerate()
            .map(|(idx, col)| format!("{col:<width$}", width = widths[idx]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    let mut lines = vec![
        format_row(&header.map(str::to_owned)),
        format_row(&widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>()),
    ];
    lines.extend(data.iter().map(|cols| format_row(cols)));
    lines.join("\n")
}

fn peers_cmd_map() -> CliOutput {
    let store = peers_load_store();
    let rows = peers_map_rows(&store, peers_resolve_ip);
    peers_ok(&format!("{}\n", peers_format_map(&rows)))
}

fn peers_load_store() -> PeersStoreNative {
    let path = peers_path();
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(tmp);
    let Ok(raw) = std::fs::read_to_string(&path) else { return PeersStoreNative { version: 1, peers: std::collections::BTreeMap::new() }; };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn peers_save_store(store: &PeersStoreNative) -> Result<(), String> {
    let path = peers_path();
    let parent = path.parent().ok_or_else(|| format!("peers path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|error| format!("peers: create {}: {error}", parent.display()))?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(store).map_err(|error| format!("peers: render store: {error}"))? + "\n";
    std::fs::write(&tmp, body).map_err(|error| format!("peers: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|error| format!("peers: rename {}: {error}", path.display()))
}

fn peers_path() -> std::path::PathBuf {
    std::env::var_os("PEERS_FILE").map_or_else(|| maw_state_path(&current_xdg_env(), &["peers.json"]), std::path::PathBuf::from)
}

fn peers_flag_value(argv: &[String], flag: &str) -> Option<String> {
    argv.iter().enumerate().find_map(|(idx, arg)| {
        if arg == flag { return argv.get(idx + 1).cloned(); }
        arg.strip_prefix(&format!("{flag}=")).map(ToOwned::to_owned)
    })
}

fn peers_validate_alias(alias: &str) -> Result<(), String> {
    let mut chars = alias.chars();
    let Some(first) = chars.next() else { return Err("invalid alias \"\" (must match ^[a-z0-9][a-z0-9_-]{0,31}$)".to_owned()); };
    let valid = alias.len() <= 32 && (first.is_ascii_lowercase() || first.is_ascii_digit());
    if !valid || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') { return Err(format!("invalid alias \"{alias}\" (must match ^[a-z0-9][a-z0-9_-]{{0,31}}$)")); }
    Ok(())
}

fn peers_validate_node(node: &str) -> Result<(), String> { peers_validate_alias(node).map_err(|_| format!("invalid --node \"{node}\"")) }

fn peers_validate_url(raw: &str) -> Result<(), String> {
    if raw.starts_with('-') || raw.chars().any(char::is_control) { return Err(format!("invalid URL \"{raw}\"")); }
    if !(raw.starts_with("http://") || raw.starts_with("https://")) { return Err(format!("invalid URL \"{raw}\" (must be http:// or https://)")); }
    let rest = raw.split_once("://").map_or("", |(_, tail)| tail);
    if rest.is_empty() || rest.starts_with('/') { return Err(format!("invalid URL \"{raw}\"")); }
    Ok(())
}

fn peers_clean_optional(raw: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return Err(format!("invalid {label} (must be non-empty)")); }
    if trimmed.chars().any(char::is_whitespace) || trimmed.starts_with('-') { return Err(format!("invalid {label} \"{raw}\" (must not contain whitespace)")); }
    Ok(trimmed.to_owned())
}

fn peers_parse_positive_usize(raw: &str, usage: &str) -> Result<usize, String> {
    raw.parse::<usize>().ok().filter(|value| *value > 0).ok_or_else(|| format!("{usage} (got --limit {raw})"))
}

fn peers_parse_positive_u64(raw: &str, usage: &str) -> Result<u64, String> {
    raw.parse::<u64>().ok().filter(|value| *value > 0).ok_or_else(|| format!("{usage} (got --timeout {raw})"))
}

fn peers_stale_ttl_ms() -> u64 {
    std::env::var("MAW_PEER_STALE_TTL_MS").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|value| *value > 0).unwrap_or(PEERS_DEFAULT_STALE_TTL_MS)
}

fn peers_stale_age_ms(peer: &PeersPeerNative) -> Option<u64> {
    let stamp = peer.last_seen.as_ref().unwrap_or(&peer.added_at);
    // #678: peers.json holds BOTH epoch-ms (current writers) and ISO-8601
    // (legacy records). A bare parse::<u64>() only reads epoch-ms and silently
    // drops ISO timestamps (→ "permanently stale"); use the shared dual-format
    // parser that `stale_age_ms` / `peers map` already use.
    let then = maw_peer::parse_timestamp_ms(stamp)?;
    Some(peers_now_ms().saturating_sub(then))
}

/// Canonical "now" timestamp for the peer store, as a string.
///
/// The store's canonical representation is **epoch-milliseconds** (e.g.
/// `"1784987579845"`), which is what every current writer emits. Legacy records
/// may still hold ISO-8601 (`"2026-06-12T00:09:39.105Z"`); readers normalize via
/// `maw_peer::stale_age_ms` / `parse_timestamp_ms`, which accept both forms, so
/// the mixed-format history is migrated on read rather than rewritten in place
/// (#678). New code must write epoch-ms through this function — do not emit ISO.
fn peers_now_iso() -> String { peers_now_ms().to_string() }
fn peers_now_ms() -> u64 { std::env::var(PEERS_FAKE_NOW_ENV).ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))) }
fn peers_ok(stdout: &str) -> CliOutput { CliOutput { code: 0, stdout: stdout.to_owned(), stderr: String::new() } }
fn peers_error(message: &str) -> CliOutput { CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") } }

#[cfg(test)]
mod peers_tests {
    use super::*;

    fn peers_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn peers_map_rows_flag_loopback_self_duplicate_nodes_and_reachability() {
        let record = |url: &str, node: &str, oracle: Option<&str>, errored: bool| PeersPeerNative {
            url: url.to_owned(),
            node: Some(node.to_owned()),
            last_error: errored.then(|| serde_json::json!({"code": "DNS"})),
            identity: oracle.map(|oracle| serde_json::json!({ "oracle": oracle })),
            ..PeersPeerNative::default()
        };
        let mut store = PeersStoreNative {
            version: 1,
            peers: std::collections::BTreeMap::new(),
        };
        store.peers.insert(
            "m5".to_owned(),
            record("http://m5.local:3456", "m5", Some("atlas"), false),
        );
        store
            .peers
            .insert("d1".to_owned(), record("http://a:3456", "dup", None, true));
        store
            .peers
            .insert("d2".to_owned(), record("http://b:3456", "dup", None, false));

        // Stub resolver: m5.local resolves to loopback (the trap), others to LAN.
        let rows = peers_map_rows(&store, |url| {
            Some(if url.contains("m5.local") {
                "127.0.0.1".to_owned()
            } else {
                "192.168.1.9".to_owned()
            })
        });
        let row = |alias: &str| {
            rows.iter()
                .find(|row| row.alias == alias)
                .cloned()
                .expect("row present")
        };
        assert!(row("m5").loopback_self, "m5.local → 127.0.0.1 is loopback-self");
        assert_eq!(row("m5").oracle, "atlas");
        assert!(row("m5").node_unique);
        assert!(!row("d1").node_unique, "two peers share node 'dup'");
        assert!(!row("d1").reachable, "lastError set → down");
        assert!(row("d2").reachable);
        assert!(!row("d2").loopback_self);
    }

    #[test]
    fn peers_format_map_carries_the_auth_error_reason_not_a_bare_flag() {
        // #685 half 2: `auth_ok: false` with no reason is a flag the user
        // can't act on. auth_ok reflects the signed-request handshake used
        // by send/probe/wake (always enforced, loopback-exempt only) --
        // separate from read-only endpoints like /api/sessions, which are
        // gated by their own opt-in bearer token and answer regardless.
        // Surfacing the reason here, rather than newly gating those
        // read-only endpoints, is the fix: it doesn't change what the
        // fleet enforces (already correct), it fixes what the map explains.
        let row = PeersMapRow {
            alias: "m5".to_owned(),
            node: "m5".to_owned(),
            oracle: "atlas".to_owned(),
            reachable: true,
            resolved_ip: Some("192.168.1.9".to_owned()),
            loopback_self: false,
            node_unique: true,
            auth_ok: Some(false),
            auth_error: Some("pubkey-mismatch".to_owned()),
        };
        let text = peers_format_map(&[row]);
        assert!(text.contains("auth-fail:pubkey-mismatch"), "{text}");
    }

    #[test]
    fn peers_map_dispatch_is_native() {
        assert!(peers_format_map(&[]).contains("federation is empty"));
    }

    fn peers_probe_all_temp_store(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("maw-rs-probeall-{}-{}.json", std::process::id(), tag))
    }

    #[test]
    fn probe_all_persist_advances_last_seen_off_a_frozen_value() {
        // #684: the sweep exists to move lastSeen; a test must prove it does. Seed a peer
        // frozen at lastSeen="1000", run the persist with a successful probe, and assert the
        // stored value advanced. Deleting `peers_save_store` (or freezing the write) turns
        // this RED — the silence #684 is about is caught here, not just the env knob.
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("advances");
        std::fs::write(
            &path,
            r#"{"version":1,"peers":{"m5":{"url":"http://127.0.0.1:1/","addedAt":"1000","lastSeen":"1000"}}}"#,
        )
        .expect("seed store");
        std::env::set_var("PEERS_FILE", &path);

        let fake = |_url: &str, _timeout: u64, _now: &str| maw_peer::ProbePeerResult {
            node: Some("m5".to_owned()),
            identity: Some(maw_peer::PeerIdentity { oracle: "arra".to_owned(), node: "m5".to_owned() }),
            pubkey: Some("pk".to_owned()),
            error: None,
            ..maw_peer::ProbePeerResult::default()
        };
        let (rows, worst) = peers_probe_all_and_persist_with(2_000, &fake).expect("persist");
        assert_eq!(worst, 0);
        assert_eq!(rows.len(), 1);

        let reloaded = peers_load_store();
        let peer = reloaded.peers.get("m5").expect("m5 present");
        assert_ne!(peer.last_seen.as_deref(), Some("1000"), "lastSeen must advance, not freeze (#684)");
        assert_eq!(
            peer.identity.as_ref().and_then(|id| id.get("oracle")).and_then(|v| v.as_str()),
            Some("arra"),
            "probe-owned oracle is persisted through the one writer",
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn probe_all_persist_preserves_a_concurrent_add() {
        // #689 lost-update race: a `maw peers add` that lands DURING the slow probe loop must
        // not be clobbered by the sweep's stale in-memory copy. The probe fn writes a new peer
        // mid-loop; the reload-before-save must preserve it. Reverting to saving the initially
        // loaded store turns this RED (the new peer vanishes).
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("race");
        std::fs::write(
            &path,
            r#"{"version":1,"peers":{"a":{"url":"http://127.0.0.1:1/","addedAt":"1000"}}}"#,
        )
        .expect("seed store");
        std::env::set_var("PEERS_FILE", &path);

        let race_path = path.clone();
        let fake = move |_url: &str, _timeout: u64, _now: &str| {
            // Simulate a concurrent `maw peers add b` landing while we probe peer a.
            std::fs::write(
                &race_path,
                r#"{"version":1,"peers":{"a":{"url":"http://127.0.0.1:1/","addedAt":"1000"},"b":{"url":"http://127.0.0.1:2/","addedAt":"2000"}}}"#,
            )
            .expect("concurrent add");
            maw_peer::ProbePeerResult { node: Some("a".to_owned()), error: None, ..maw_peer::ProbePeerResult::default() }
        };
        peers_probe_all_and_persist_with(2_000, &fake).expect("persist");

        let reloaded = peers_load_store();
        assert!(reloaded.peers.contains_key("b"), "concurrent add must survive the sweep (#689 lost-update)");
        assert!(reloaded.peers.contains_key("a"), "probed peer still present");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn peers_dispatch_registers_aliases_and_guards() {
        assert_eq!(dispatcher_status("peers"), DispatchKind::Native);
        assert_eq!(dispatcher_status("peer"), DispatchKind::Native);
        assert_eq!(DISPATCH_104.len(), 2);
        let out = peers_run_command(&peers_args(&["list", "--limit", "-1"]));
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("--limit requires a safe value"));
        let out = peers_run_command(&peers_args(&["--"]));
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("separator"));
    }

    fn peers_probe_plan_with_identity(identity: Option<maw_peer::ProbeRemoteIdentity>) -> maw_peer::ProbePeerResult {
        maw_peer::probe_peer_from_plan(&maw_peer::ProbePeerPlan {
            url: "http://peer.test:3456".to_owned(),
            now: "1700000000000".to_owned(),
            dns_error: None,
            info: maw_peer::ProbeInfoOutcome::Body(maw_peer::ProbeInfoBody { maw: maw_peer::ProbeMawHandshake::SchemaObject("1".to_owned()), node: Some("peer-node".to_owned()), name: None, nickname: None }),
            identity,
            resolved_ip: None,
            auth_ok: None,
            auth_error: None,
        })
    }

    #[test]
    fn peers_probe_with_identity_body_pins_pubkey_on_first_contact() {
        let probe = peers_probe_plan_with_identity(Some(maw_peer::ProbeRemoteIdentity::Body { pubkey: Some("pub-545".to_owned()), oracle: Some("oracle-x".to_owned()), node: Some("peer-node".to_owned()) }));
        assert!(probe.error.is_none());
        assert_eq!(probe.pubkey.as_deref(), Some("pub-545"));
        let mut peer = PeersPeerNative { url: "http://peer.test:3456".to_owned(), ..PeersPeerNative::default() };
        peers_apply_probe_result(&mut peer, &probe, "1700000000000").unwrap();
        assert_eq!(peer.pubkey.as_deref(), Some("pub-545"));
        assert_eq!(peer.pubkey_first_seen.as_deref(), Some("1700000000000"));
    }

    #[test]
    fn peers_apply_probe_result_keeps_first_seen_when_pubkey_is_unchanged() {
        // #678: the field means "when we FIRST trusted this key". A probe that
        // returns the same key must NOT bump it — only a key change does.
        let mut peer = PeersPeerNative {
            url: "http://peer.test:3456".to_owned(),
            pubkey: Some("KEY-A".to_owned()),
            pubkey_first_seen: Some("1111111111111".to_owned()),
            ..PeersPeerNative::default()
        };
        let same = maw_peer::ProbePeerResult { pubkey: Some("KEY-A".to_owned()), node: Some("peer-node".to_owned()), ..maw_peer::ProbePeerResult::default() };
        peers_apply_probe_result(&mut peer, &same, "9999999999999").unwrap();
        assert_eq!(peer.pubkey.as_deref(), Some("KEY-A"));
        assert_eq!(peer.pubkey_first_seen.as_deref(), Some("1111111111111"), "unchanged key must keep its original first-seen anchor");

        let rotated = maw_peer::ProbePeerResult { pubkey: Some("KEY-B".to_owned()), node: Some("peer-node".to_owned()), ..maw_peer::ProbePeerResult::default() };
        peers_apply_probe_result(&mut peer, &rotated, "9999999999999").unwrap();
        assert_eq!(peer.pubkey.as_deref(), Some("KEY-B"));
        assert_eq!(peer.pubkey_first_seen.as_deref(), Some("9999999999999"), "a rotated key re-anchors first-seen at the new contact");
    }

    #[test]
    fn peers_cmd_add_upsert_preserves_pubkey_first_seen_when_url_is_repointed() {
        // #678 measured bug: `maw peers add <alias> <newurl>` on an existing alias
        // built a fresh record (pubkey=None) and reset pubkeyFirstSeen to now,
        // erasing the TOFU anchor even though the key was byte-identical. Before
        // the fix this drops first-seen to None on an --allow-unreachable re-add.
        let _guard = env_test_lock();
        let _restore_now = EnvVarRestore::capture(PEERS_FAKE_NOW_ENV);
        let _restore_file = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("firstseen-preserve");
        std::fs::write(
            &path,
            r#"{"version":1,"peers":{"m5":{"url":"http://old.tunnel:3456/","addedAt":"1000","pubkey":"KEY-A","pubkeyFirstSeen":"1111111111111"}}}"#,
        )
        .expect("seed store");
        std::env::set_var("PEERS_FILE", &path);
        std::env::set_var(PEERS_FAKE_NOW_ENV, "9999999999999");

        let out = peers_run_command(&peers_args(&["add", "m5", "http://m5.wg:3456/", "--allow-unreachable"]));
        assert_eq!(out.code, 0, "{}", out.stderr);

        let reloaded = peers_load_store();
        let peer = reloaded.peers.get("m5").expect("m5 present");
        assert_eq!(peer.url, "http://m5.wg:3456/");
        assert_eq!(peer.pubkey.as_deref(), Some("KEY-A"), "cached pubkey is carried across the URL repoint");
        assert_eq!(peer.pubkey_first_seen.as_deref(), Some("1111111111111"), "repointing the URL must not reset the TOFU anchor (#678)");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn merge_peer_store_named_peers_makes_store_aliases_routable_and_lets_config_win() {
        // #681 read-side: config namedPeers and peers.json are two registries. A
        // peer added via `maw peers add` lives in the store, so `maw hey` could
        // not route to it. The store is merged in as a fallback (config wins on a
        // name clash); an unreadable/missing store must not break routing.
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("named-fallback");
        std::fs::write(
            &path,
            r#"{"version":1,"peers":{"storepeer":{"url":"http://store.test:3456/","addedAt":"1000"},"shared":{"url":"http://store-loses.test:3456/","addedAt":"1000"},"blank":{"url":"","addedAt":"1000"}}}"#,
        )
        .expect("seed store");
        std::env::set_var("PEERS_FILE", &path);

        let merged = merge_peer_store_named_peers(vec![RouteNamedPeer {
            name: "shared".to_owned(),
            url: "http://config-wins.test:3456/".to_owned(),
        }]);

        // store-only alias becomes routable
        assert!(
            merged.iter().any(|peer| peer.name == "storepeer" && peer.url == "http://store.test:3456/"),
            "a store-only peer must become addressable by hey",
        );
        // config wins on conflict: exactly one "shared", with the config URL
        let shareds: Vec<&RouteNamedPeer> = merged.iter().filter(|peer| peer.name == "shared").collect();
        assert_eq!(shareds.len(), 1, "config wins; no duplicate store entry for a conflicting name");
        assert_eq!(shareds[0].url, "http://config-wins.test:3456/");
        // blank URL is skipped
        assert!(merged.iter().all(|peer| peer.name != "blank"), "a peer with an empty URL is not routable");
        std::fs::remove_file(&path).ok();

        // a missing/unreadable store must not break the config list
        let _restore_missing = EnvVarRestore::capture("PEERS_FILE");
        std::env::set_var("PEERS_FILE", "/dev/null/does-not-exist");
        let only_config = merge_peer_store_named_peers(vec![RouteNamedPeer { name: "c".to_owned(), url: "http://c.test:3456/".to_owned() }]);
        assert_eq!(only_config.len(), 1, "missing store leaves config namedPeers untouched");
    }

    #[test]
    fn peer_probe_constants_surface_knows_unreachable() {
        // #733 review: the new UNREACHABLE code must reach EVERY surface a
        // consumer enumerates — the classify parser AND the constants JSON — not
        // just the enum (the tmux_close-class "fixed the root, missed the
        // surface" trap).
        assert_eq!(parse_probe_error_code("UNREACHABLE"), Some(maw_peer::ProbeErrorCode::Unreachable));
        let json = render_peer_probe_constants_json();
        assert!(json.contains("\"UNREACHABLE\""), "constants codes list must include UNREACHABLE: {json}");
        assert!(json.contains("\"UNREACHABLE\":7"), "constants exitCodes must map UNREACHABLE→7: {json}");
    }

    #[test]
    fn merge_peer_store_gate_blocks_only_explicit_failure_not_absent_auth() {
        // #681 review (semantic lock): the route gate blocks only EXPLICIT
        // failure. auth_ok == None is "no federation auth configured" (N/A), not
        // "verified untrusted" — blocking it would make every peer in an
        // unfederated fleet unroutable (regressing #681 wholesale). Only a failed
        // probe (last_error) or an explicit auth refusal (Some(false)) blocks.
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("named-health");
        std::fs::write(
            &path,
            r#"{"version":1,"peers":{
                "verified":{"url":"http://ok.test:3456/","addedAt":"1000","authOk":true},
                "unverified":{"url":"http://noverify.test:3456/","addedAt":"1000"},
                "dead":{"url":"http://dead.test:3456/","addedAt":"1000","lastError":{"code":"DNS"}},
                "refused":{"url":"http://authfail.test:3456/","addedAt":"1000","authOk":false}
            }}"#,
        )
        .expect("seed store");
        std::env::set_var("PEERS_FILE", &path);

        let merged = merge_peer_store_named_peers(Vec::new());
        let names: Vec<&str> = merged.iter().map(|peer| peer.name.as_str()).collect();
        assert!(names.contains(&"verified"), "auth-verified peer is routable");
        assert!(names.contains(&"unverified"), "auth_ok == None (no federation auth) stays routable");
        assert!(!names.contains(&"dead"), "a peer whose last probe failed is not routable");
        assert!(!names.contains(&"refused"), "a peer whose auth was refused is not routable");

        // reasons are named for the two real blocks; None/Some(true) return None
        let reason = |alias: &str| peers_load_store().peers.get(alias).and_then(peer_route_block_reason);
        assert_eq!(reason("dead"), Some("last probe failed"));
        assert_eq!(reason("refused"), Some("federation auth refused"));
        assert_eq!(reason("unverified"), None, "absent auth is not a block reason");
        assert_eq!(reason("verified"), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn bare_name_local_vs_peer_warning_only_when_a_same_named_peer_exists() {
        // #681/#695: a bare name resolving local while a same-named peer exists is
        // a silent self-address; the guard must fire only for a genuine bare name
        // shadowed by a peer, never for explicit local:/node: forms or `me`.
        let with_peer = HeyConfig {
            route: RouteConfig {
                named_peers: vec![RouteNamedPeer { name: "nova".to_owned(), url: "http://nova.test:3456".to_owned() }],
                ..RouteConfig::default()
            },
            ..HeyConfig::default()
        };
        assert!(bare_name_local_vs_peer_warning("nova", &with_peer).is_some(), "bare name shadowed by a peer warns");

        let no_peer = HeyConfig::default();
        assert!(bare_name_local_vs_peer_warning("nova", &no_peer).is_none(), "no same-named peer => no warning");

        // explicit forms must not warn even when the peer exists
        assert!(bare_name_local_vs_peer_warning("nova:01-sess", &with_peer).is_none());
        assert!(bare_name_local_vs_peer_warning("local:nova", &with_peer).is_none());
        assert!(bare_name_local_vs_peer_warning("me", &with_peer).is_none());
    }

    #[test]
    fn hey_peer_source_hint_names_the_peer_store_when_the_url_is_registered() {
        // #681 dry-run provenance: the hint must say the peer store satisfied the
        // route when the URL is in peers.json, else attribute it to config.
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("PEERS_FILE");
        let path = peers_probe_all_temp_store("source-hint");
        std::fs::write(&path, r#"{"version":1,"peers":{"x":{"url":"http://store.test:3456/","addedAt":"1000"}}}"#).expect("seed");
        std::env::set_var("PEERS_FILE", &path);

        assert_eq!(hey_peer_source_hint("http://store.test:3456/"), "from peer store (peers.json)");
        assert_eq!(hey_peer_source_hint("http://store.test:3456"), "from peer store (peers.json)", "trailing slash is normalized");
        assert_eq!(hey_peer_source_hint("http://config-only.test:3456"), "from config namedPeers");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn pair_system_config_reads_port_and_node_from_config_not_defaults() {
        // #734: PairSystemHost.pair_config must advertise the real config
        // port/node, not the 3456/"local" defaults, so a pair handshake doesn't
        // echo defaults that point each side at localhost:<default> (itself).
        let _guard = env_test_lock();
        let _c1 = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _c2 = EnvVarRestore::capture("MAW_HOME");
        let _c3 = EnvVarRestore::capture("MAW_NODE");
        let _c4 = EnvVarRestore::capture("MAW_PORT");
        let dir = std::env::temp_dir().join(format!("maw-pair-cfg-{}-port", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("maw.config.json"), r#"{"port": 3457, "node": "fleet", "oracle": "arra"}"#).expect("config");
        std::env::set_var("MAW_CONFIG_DIR", &dir);
        std::env::remove_var("MAW_NODE");
        std::env::remove_var("MAW_PORT");

        let cfg = PairSystemHost.pair_config();
        assert_eq!(cfg.port, 3457, "pair must advertise config.port, not the 3456 default (#734)");
        assert_eq!(cfg.node, "fleet", "pair must advertise config.node, not \"local\"");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn peers_apply_probe_result_persists_the_auth_error_reason() {
        // #685: `auth_ok: false` with no reason is the same disease as the six
        // bugs behind #680 -- the peer record must persist WHY, so `peers info`
        // can show it, not just a bare boolean the map already renders as
        // "auth-fail".
        let probe = maw_peer::ProbePeerResult {
            node: Some("peer-node".to_owned()),
            auth_ok: Some(false),
            auth_error: Some("pubkey-mismatch".to_owned()),
            ..maw_peer::ProbePeerResult::default()
        };
        let mut peer = PeersPeerNative { url: "http://peer.test:3456".to_owned(), ..PeersPeerNative::default() };
        peers_apply_probe_result(&mut peer, &probe, "1700000000000").unwrap();
        assert_eq!(peer.auth_ok, Some(false));
        assert_eq!(peer.auth_error.as_deref(), Some("pubkey-mismatch"));

        let value = serde_json::to_value(&peer).expect("serialize");
        assert_eq!(value["authError"], "pubkey-mismatch", "{value}");
    }

    #[test]
    fn peers_probe_identity_failure_degrades_to_unpinned_probe() {
        for identity in [None, Some(maw_peer::ProbeRemoteIdentity::Missing), Some(maw_peer::ProbeRemoteIdentity::HttpError), Some(maw_peer::ProbeRemoteIdentity::FetchError), Some(maw_peer::ProbeRemoteIdentity::MalformedJson)] {
            let probe = peers_probe_plan_with_identity(identity);
            assert!(probe.error.is_none(), "identity failure must not fail the probe");
            assert_eq!(probe.pubkey, None);
            assert_eq!(probe.node.as_deref(), Some("peer-node"));
            let mut peer = PeersPeerNative { url: "http://peer.test:3456".to_owned(), ..PeersPeerNative::default() };
            peers_apply_probe_result(&mut peer, &probe, "1700000000000").unwrap();
            assert_eq!(peer.pubkey, None);
            assert_eq!(peer.pubkey_first_seen, None);
            assert_eq!(peer.last_seen.as_deref(), Some("1700000000000"));
        }
    }

    #[test]
    fn peers_probe_identity_body_parses_api_identity_payload() {
        let value = serde_json::json!({ "node": "m5", "oracle": "arra", "pubkey": "78ebf563", "version": "v26.7.16", "uptime": 1 });
        assert_eq!(peers_probe_identity_body(&value), maw_peer::ProbeRemoteIdentity::Body { pubkey: Some("78ebf563".to_owned()), oracle: Some("arra".to_owned()), node: Some("m5".to_owned()) });
        assert_eq!(peers_probe_identity_body(&serde_json::json!({})), maw_peer::ProbeRemoteIdentity::Body { pubkey: None, oracle: None, node: None });
    }
}
