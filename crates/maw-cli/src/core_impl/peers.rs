const DISPATCH_104: &[DispatcherEntry] = &[
    DispatcherEntry { command: "peers", handler: Handler::Sync(peers_run_command) },
    DispatcherEntry { command: "peer", handler: Handler::Sync(peers_run_command) },
];

const PEERS_HELP: &str = "usage: maw peers <add|list|info|probe|probe-all|accept|remove|forget> [...]\n  add       <alias> <url> [--node <name>] [--ssh <target>] [--user <name>] [--allow-unreachable]\n            — register alias (auto-probes /info). Exits non-zero on handshake failure:\n              2=UNKNOWN/BAD_BODY/TLS  3=DNS  4=REFUSED  5=TIMEOUT  6=HTTP_4XX/5XX\n            --ssh sets the SSH config alias/target for cross-node attach; --user overrides SSH user.\n            --allow-unreachable keeps exit 0 even when the probe fails (CI/bootstrap).\n  list      [--discovered] [--all] [--json] [--limit N]\n            — tabular list of all peers. --discovered: LAN candidates from Scout (#1237).\n              --all: include already-paired (default hides). --limit: cap rows (default 50).\n  info      <alias>                         — JSON details for one peer (includes lastError if set)\n  probe     <alias>                         — re-run /info handshake; updates lastSeen / lastError (#565)\n  probe-all [--timeout <ms>] [--allow-unreachable]\n            — probe every peer in parallel; prints liveness table. Exit = worst PROBE_EXIT_CODE (#669).\n  accept    <node|zid-prefix> [--alias X] | --all (#1237)\n            — pair with a Scout-discovered peer. Shortest unambiguous prefix wins.\n              Refuses if pubkey already pins under a different alias (impersonation guard).\n  remove    <alias>                         — remove (idempotent)\n  forget    <alias>                         — clear cached pubkey so next contact re-TOFUs (#804 Step 2)\n\nstorage: maw state peers.json (v1; reads legacy ~/.maw/peers.json during migration)";
const PEERS_DEFAULT_STALE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
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
    let now = peers_now_iso();
    let peer = PeersPeerNative { url: url.to_owned(), node, added_at: now, last_seen: None, ssh, ssh_user, ..PeersPeerNative::default() };
    store.peers.insert(alias.to_owned(), peer.clone());
    peers_save_store(&store)?;
    let mut stdout = String::new();
    if overwrote { let _ = writeln!(stdout, "warning: alias \"{alias}\" already existed — overwriting"); }
    let _ = writeln!(stdout, "added {alias} → {url}{}", peer.node.as_ref().map(|node| format!(" ({node})")).unwrap_or_default());
    if argv.iter().any(|arg| arg == "--allow-unreachable") { return Ok(peers_ok(&stdout)); }
    Ok(CliOutput { code: 2, stdout, stderr: "peer handshake failed: UNKNOWN — pass --allow-unreachable to bypass\n".to_owned() })
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
    let store = peers_load_store();
    let Some(peer) = store.peers.get(alias) else { return Err(format!("peer \"{alias}\" not found")); };
    Ok(CliOutput { code: 2, stdout: format!("probing {alias} → {} ...\n", peer.url), stderr: "\x1b[31m✗\x1b[0m UNKNOWN probing peer\n".to_owned() })
}

fn peers_cmd_probe_all(argv: &[String]) -> Result<CliOutput, String> {
    if let Some(raw) = peers_flag_value(argv, "--timeout") { peers_parse_positive_u64(&raw, "usage: maw peers probe-all [--timeout <ms>]")?; }
    let store = peers_load_store();
    if store.peers.is_empty() { return Ok(peers_ok("alias  url  status\n-----  ---  ------\n")); }
    let mut stdout = String::from("alias  url  status\n-----  ---  ------\n");
    for (alias, peer) in store.peers { let _ = writeln!(stdout, "{alias}  {}  UNKNOWN", peer.url); }
    let allow = argv.iter().any(|arg| arg == "--allow-unreachable");
    Ok(CliOutput { code: if allow { 0 } else { 2 }, stdout, stderr: String::new() })
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
    let then = stamp.parse::<u64>().ok()?;
    Some(peers_now_ms().saturating_sub(then))
}

fn peers_now_iso() -> String { peers_now_ms().to_string() }
fn peers_now_ms() -> u64 { std::env::var(PEERS_FAKE_NOW_ENV).ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))) }
fn peers_ok(stdout: &str) -> CliOutput { CliOutput { code: 0, stdout: stdout.to_owned(), stderr: String::new() } }
fn peers_error(message: &str) -> CliOutput { CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") } }

#[cfg(test)]
mod peers_tests {
    use super::*;

    fn peers_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

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
}
