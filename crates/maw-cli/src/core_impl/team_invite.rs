#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TeamInviteNamedPeer125 {
    name: String,
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TeamInviteConfig125 {
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    named_peers: Vec<TeamInviteNamedPeer125>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TeamInviteEntry125 {
    name: String,
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node: Option<String>,
    scope: String,
    invited_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamInviteOptions125 {
    team: String,
    peer: String,
    scope: String,
    lead: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamInviteConsentRequest125 {
    id: String,
    from: String,
    to: String,
    action: String,
    summary: String,
    pin_hash: String,
    plaintext_pin: String,
    created_at: String,
    expires_at: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamInvitePostResult125 {
    ok: bool,
    status: u16,
    error: Option<String>,
}

trait TeamInviteFs125 {
    fn team_invite_exists(&self, path: &std::path::Path) -> bool;
    fn team_invite_read(&self, path: &std::path::Path) -> Result<String, String>;
    fn team_invite_write_json_atomic_0600(&mut self, path: &std::path::Path, value: &serde_json::Value) -> Result<(), String>;
}

trait TeamInviteTrustStore125 {
    fn team_invite_is_trusted(&self, from: &str, to: &str, action: &str) -> bool;
}

trait TeamInviteConsentStore125 {
    fn team_invite_write_pending(&mut self, request: &TeamInviteConsentRequest125) -> Result<(), String>;
    fn team_invite_next_id(&mut self) -> String;
    fn team_invite_next_pin(&mut self) -> String;
}

trait TeamInviteHttpClient125 {
    fn team_invite_post_consent(&mut self, base_url: &str, request: &TeamInviteConsentRequest125) -> TeamInvitePostResult125;
}

struct TeamInviteSystemFs125;
struct TeamInviteSystemTrust125;
struct TeamInviteSystemConsent125;
struct TeamInviteSystemHttp125;

impl TeamInviteFs125 for TeamInviteSystemFs125 {
    fn team_invite_exists(&self, path: &std::path::Path) -> bool { path.exists() }
    fn team_invite_read(&self, path: &std::path::Path) -> Result<String, String> { std::fs::read_to_string(path).map_err(|error| error.to_string()) }
    fn team_invite_write_json_atomic_0600(&mut self, path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> { team_write_json_atomic_0600(path, value) }
}

impl TeamInviteTrustStore125 for TeamInviteSystemTrust125 {
    fn team_invite_is_trusted(&self, from: &str, to: &str, action: &str) -> bool {
        let path = maw_state_path(&current_xdg_env(), &["trust.json"]);
        let Ok(text) = std::fs::read_to_string(path) else { return false; };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return false; };
        team_invite_trust_json_contains(&value, from, to, action)
    }
}

impl TeamInviteConsentStore125 for TeamInviteSystemConsent125 {
    fn team_invite_write_pending(&mut self, request: &TeamInviteConsentRequest125) -> Result<(), String> {
        let path = maw_state_path(&current_xdg_env(), &["consent-pending", &format!("{}.json", request.id)]);
        team_write_json_atomic_0600(&path, &team_invite_pending_json(request))
    }

    fn team_invite_next_id(&mut self) -> String { team_invite_generate_request_id() }

    fn team_invite_next_pin(&mut self) -> String { team_invite_generate_pin() }
}

impl TeamInviteHttpClient125 for TeamInviteSystemHttp125 {
    fn team_invite_post_consent(&mut self, base_url: &str, request: &TeamInviteConsentRequest125) -> TeamInvitePostResult125 {
        let Ok(url) = team_invite_consent_url(base_url) else { return TeamInvitePostResult125 { ok: false, status: 0, error: Some("invalid peer URL".to_owned()) }; };
        let Ok(body) = serde_json::to_string(&team_invite_pending_json(request)) else { return TeamInvitePostResult125 { ok: false, status: 0, error: Some("encode failed".to_owned()) }; };
        team_invite_http_post(&url, &body)
    }
}

fn team_invite(argv: &[String]) -> Result<String, String> {
    let mut fs = TeamInviteSystemFs125;
    let trust = TeamInviteSystemTrust125;
    let mut consent = TeamInviteSystemConsent125;
    let mut http = TeamInviteSystemHttp125;
    let config = team_invite_load_config();
    team_invite_with(argv, &config, &mut fs, &trust, &mut consent, &mut http)
}

fn team_invite_with(
    argv: &[String],
    config: &TeamInviteConfig125,
    fs: &mut impl TeamInviteFs125,
    trust: &impl TeamInviteTrustStore125,
    consent: &mut impl TeamInviteConsentStore125,
    http: &mut impl TeamInviteHttpClient125,
) -> Result<String, String> {
    let opts = team_invite_parse(argv)?;
    let manifest_path = team_paths(&opts.team).vault_manifest;
    if !fs.team_invite_exists(&manifest_path) {
        return Err(format!("\x1b[31m✗\x1b[0m team '{}' not found — run: maw team create {}", opts.team, opts.team));
    }
    let peer = config.named_peers.iter().find(|peer| peer.name == opts.peer).cloned().ok_or_else(|| {
        format!("\x1b[31m✗\x1b[0m unknown peer '{}' — not in namedPeers.\n  hint: add {} to maw.config.json namedPeers", opts.peer, opts.peer)
    })?;
    team_invite_validate_url(&peer.url)?;
    if std::env::var("MAW_CONSENT").ok().as_deref() != Some("1") {
        team_invite_record(&manifest_path, &peer, &opts.scope, fs)?;
        return Ok(team_invite_success(&opts));
    }
    let my_node = config.node.clone().filter(|node| !node.is_empty()).unwrap_or_else(|| "local".to_owned());
    team_invite_validate_id(&my_node, "node")?;
    let lead = opts.lead.clone().unwrap_or_else(|| my_node.clone());
    team_invite_validate_id(&lead, "lead")?;
    let peer_id = peer.node.clone().unwrap_or_else(|| peer.name.clone());
    team_invite_validate_id(&peer_id, "peer node")?;
    if trust.team_invite_is_trusted(&my_node, &peer_id, "team-invite") {
        team_invite_record(&manifest_path, &peer, &opts.scope, fs)?;
        return Ok(team_invite_success(&opts));
    }
    let request = team_invite_request(&opts, &peer, &my_node, &lead, &peer_id, consent)?;
    consent.team_invite_write_pending(&request)?;
    let post = http.team_invite_post_consent(&peer.url, &request);
    if !post.ok {
        let error = post.error.unwrap_or_else(|| format!("peer rejected request: HTTP {}", post.status));
        return Err(format!("\x1b[31m✗ consent request failed\x1b[0m: {error}\n  request id (local mirror): {}\n  hint: peer may be down, or /api/consent/request not yet deployed", request.id));
    }
    Err(format!("__TEAM_INVITE_EXIT2__{}", team_invite_consent_required(&opts, &peer, &peer_id, &lead, &request)))
}

fn team_invite_parse(argv: &[String]) -> Result<TeamInviteOptions125, String> {
    let mut positional = Vec::new();
    let mut scope = None;
    let mut lead = None;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--scope" => { index += 1; scope = Some(team_invite_next(argv, index, "--scope")?); },
            "--lead" => { index += 1; lead = Some(team_invite_next(argv, index, "--lead")?); },
            value if value.starts_with('-') => return Err(format!("team: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if positional.len() < 2 { return Err("usage: maw team invite <team> <peer> [--scope <scope>] [--lead <lead>]".to_owned()); }
    if positional.len() > 2 { return Err(format!("team invite: unexpected argument {}", positional[2])); }
    team_validate_name(&positional[0])?;
    team_invite_validate_id(&positional[1], "peer")?;
    let scope = scope.unwrap_or_else(|| "member".to_owned());
    team_invite_validate_id(&scope, "scope")?;
    if let Some(value) = &lead { team_invite_validate_id(value, "lead")?; }
    Ok(TeamInviteOptions125 { team: positional[0].clone(), peer: positional[1].clone(), scope, lead })
}

fn team_invite_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index).ok_or_else(|| format!("{flag} requires a value"))?;
    if value.starts_with('-') { return Err(format!("{flag} value must not start with '-'")); }
    Ok(value.clone())
}

fn team_invite_record(path: &std::path::Path, peer: &TeamInviteNamedPeer125, scope: &str, fs: &mut impl TeamInviteFs125) -> Result<(), String> {
    let text = fs.team_invite_read(path)?;
    let mut manifest: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if !manifest.get("invitees").is_some_and(serde_json::Value::is_array) { manifest["invitees"] = serde_json::json!([]); }
    let invitees = manifest["invitees"].as_array_mut().ok_or_else(|| "team invite: invalid invitees".to_owned())?;
    let entry = serde_json::to_value(TeamInviteEntry125 { name: peer.name.clone(), url: peer.url.clone(), node: peer.node.clone(), scope: scope.to_owned(), invited_at: team_timestamp() }).map_err(|error| error.to_string())?;
    if let Some(existing) = invitees.iter().position(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(peer.name.as_str())) { invitees[existing] = entry; } else { invitees.push(entry); }
    fs.team_invite_write_json_atomic_0600(path, &manifest)
}

fn team_invite_request(
    opts: &TeamInviteOptions125,
    peer: &TeamInviteNamedPeer125,
    my_node: &str,
    lead: &str,
    peer_id: &str,
    consent: &mut impl TeamInviteConsentStore125,
) -> Result<TeamInviteConsentRequest125, String> {
    let id = consent.team_invite_next_id();
    team_invite_validate_request_id(&id)?;
    let pin = consent.team_invite_next_pin();
    team_invite_validate_pin(&pin)?;
    let now = team_timestamp();
    Ok(TeamInviteConsentRequest125 {
        id,
        from: my_node.to_owned(),
        to: peer_id.to_owned(),
        action: "team-invite".to_owned(),
        summary: team_invite_summary(opts, peer, lead),
        pin_hash: maw_auth::hash_consent_pin(&pin),
        plaintext_pin: pin,
        created_at: now,
        expires_at: team_invite_expiry(),
        status: "pending".to_owned(),
    })
}

fn team_invite_summary(opts: &TeamInviteOptions125, peer: &TeamInviteNamedPeer125, lead: &str) -> String {
    format!("team-invite: team='{}' lead='{lead}' invitee='{}'{} url='{}' scope='{}'", opts.team, peer.name, peer.node.as_ref().map_or(String::new(), |node| format!(" ({node})")), peer.url, opts.scope)
}

fn team_invite_consent_required(opts: &TeamInviteOptions125, peer: &TeamInviteNamedPeer125, peer_id: &str, lead: &str, request: &TeamInviteConsentRequest125) -> String {
    let pin = &request.plaintext_pin;
    [
        "\x1b[33m⏸  consent required\x1b[0m → team-invite".to_owned(),
        format!("   team:   {}  (lead: {lead})", opts.team),
        format!("   peer:   {}{}  [{}]", peer.name, peer.node.as_ref().map_or(String::new(), |node| format!(" ({node})")), peer.url),
        format!("   scope:  {}", opts.scope),
        format!("   request id: {}", request.id),
        format!("   PIN (relay OOB to {peer_id} operator): \x1b[1m{pin}\x1b[0m"),
        format!("   expires: {}", request.expires_at),
        String::new(),
        format!("   on {peer_id}: \x1b[36mmaw consent approve {} {pin}\x1b[0m", request.id),
        format!("   then re-run: \x1b[36mmaw team invite {} {}\x1b[0m", opts.team, peer.name),
    ].join("\n")
}

fn team_invite_success(opts: &TeamInviteOptions125) -> String { format!("\x1b[32m✓\x1b[0m invited '{}' to team '{}' (scope: {})\n", opts.peer, opts.team, opts.scope) }

fn team_invite_pending_json(request: &TeamInviteConsentRequest125) -> serde_json::Value {
    serde_json::json!({
        "id": request.id,
        "from": request.from,
        "to": request.to,
        "action": request.action,
        "summary": request.summary,
        "pinHash": request.pin_hash,
        "createdAt": request.created_at,
        "expiresAt": request.expires_at,
        "status": request.status,
    })
}

fn team_invite_load_config() -> TeamInviteConfig125 {
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn team_invite_trust_json_contains(value: &serde_json::Value, from: &str, to: &str, action: &str) -> bool {
    let key = format!("{from}→{to}:{action}");
    if value.get(&key).is_some() { return true; }
    value.as_array().is_some_and(|items| items.iter().any(|item| team_invite_trust_entry_matches(item, from, to, action)))
        || value.get("trust").is_some_and(|trust| team_invite_trust_json_contains(trust, from, to, action))
        || value.get("entries").is_some_and(|entries| team_invite_trust_json_contains(entries, from, to, action))
}

fn team_invite_trust_entry_matches(item: &serde_json::Value, from: &str, to: &str, action: &str) -> bool {
    item.get("from").and_then(serde_json::Value::as_str) == Some(from)
        && item.get("to").and_then(serde_json::Value::as_str) == Some(to)
        && item.get("action").and_then(serde_json::Value::as_str) == Some(action)
}

fn team_invite_validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains("..") || value.contains('/') || value.contains('\\') || value.contains('\0') || value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        Err(format!("invalid team invite {label} {value:?}"))
    } else {
        Ok(())
    }
}

fn team_invite_validate_request_id(value: &str) -> Result<(), String> {
    if value.len() != 24 || !value.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("team invite: invalid request id".to_owned()); }
    Ok(())
}

fn team_invite_validate_pin(value: &str) -> Result<(), String> {
    if !maw_auth::is_valid_pair_code_shape(value) { return Err("team invite: invalid generated PIN".to_owned()); }
    Ok(())
}

fn team_invite_validate_url(value: &str) -> Result<(), String> {
    let _ = team_invite_split_url(value)?;
    Ok(())
}

fn team_invite_consent_url(base_url: &str) -> Result<String, String> {
    let (scheme, host, port, _) = team_invite_split_url(base_url)?;
    let port_part = port.map_or(String::new(), |p| format!(":{p}"));
    Ok(format!("{scheme}://{host}{port_part}/api/consent/request"))
}

fn team_invite_split_url(value: &str) -> Result<(&str, &str, Option<u16>, &str), String> {
    if value.contains('\0') || value.chars().any(char::is_control) || value.contains('#') || value.contains('@') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    let (scheme, rest) = value.split_once("://").ok_or_else(|| format!("team invite: invalid peer URL {value:?}"))?;
    if !matches!(scheme, "http" | "https") { return Err(format!("team invite: invalid peer URL {value:?}")); }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || !path.is_empty() || rest.contains('?') || authority.contains('/') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    let (host, port) = if let Some((host, port_text)) = authority.rsplit_once(':') {
        if host.is_empty() || port_text.is_empty() || !port_text.bytes().all(|b| b.is_ascii_digit()) { return Err(format!("team invite: invalid peer URL {value:?}")); }
        let port = port_text.parse::<u16>().map_err(|_| format!("team invite: invalid peer URL {value:?}"))?;
        (host, Some(port))
    } else {
        (authority, None)
    };
    if host.is_empty() || host.starts_with('-') || host.contains("..") || host.chars().any(|ch| ch.is_whitespace() || ch == '\\') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    Ok((scheme, host, port, path))
}

fn team_invite_expiry() -> String { std::env::var("MAW_RS_TEAM_FIXED_EXPIRES").unwrap_or_else(|_| (team_now_millis() + 600_000).to_string()) }

fn team_invite_generate_request_id() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    maw_auth::consent_request_id_from_bytes(&bytes)
}

fn team_invite_generate_pin() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 6];
    rand::thread_rng().fill_bytes(&mut bytes);
    maw_auth::generate_pair_code_from_bytes(&bytes)
}

fn team_invite_http_post(url: &str, body: &str) -> TeamInvitePostResult125 {
    use std::io::{Read as _, Write as _};
    let Ok((scheme, host, port, path)) = team_invite_split_request_url(url) else {
        return TeamInvitePostResult125 { ok: false, status: 0, error: Some("invalid URL".to_owned()) };
    };
    if scheme == "https" {
        return TeamInvitePostResult125 { ok: false, status: 0, error: Some("https consent POST not available in native minimal client yet".to_owned()) };
    }
    let port = port.unwrap_or(80);
    let addr = format!("{host}:{port}");
    let Ok(mut stream) = std::net::TcpStream::connect(addr) else {
        return TeamInvitePostResult125 { ok: false, status: 0, error: Some("network error contacting peer".to_owned()) };
    };
    let request_path = if path.is_empty() { "/" } else { path };
    let request = format!("POST /{request_path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    if let Err(error) = stream.write_all(request.as_bytes()) {
        return TeamInvitePostResult125 { ok: false, status: 0, error: Some(error.to_string()) };
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let status = response.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
    TeamInvitePostResult125 { ok: (200..300).contains(&status), status, error: None }
}

fn team_invite_split_request_url(value: &str) -> Result<(&str, &str, Option<u16>, &str), String> {
    if value.contains('\0') || value.chars().any(char::is_control) || value.contains('#') || value.contains('@') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    let (scheme, rest) = value.split_once("://").ok_or_else(|| format!("team invite: invalid peer URL {value:?}"))?;
    if !matches!(scheme, "http" | "https") {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.contains('/') || authority.contains('?') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    let (host, port) = if let Some((host, port_text)) = authority.rsplit_once(':') {
        if host.is_empty() || port_text.is_empty() || !port_text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("team invite: invalid peer URL {value:?}"));
        }
        let port = port_text.parse::<u16>().map_err(|_| format!("team invite: invalid peer URL {value:?}"))?;
        (host, Some(port))
    } else {
        (authority, None)
    };
    if host.is_empty() || host.starts_with('-') || host.contains("..") || host.chars().any(|ch| ch.is_whitespace() || ch == '\\') {
        return Err(format!("team invite: invalid peer URL {value:?}"));
    }
    Ok((scheme, host, port, path))
}

#[cfg(test)]
mod team_invite_tests125 {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    struct FakeFs125 { files: BTreeMap<std::path::PathBuf, String>, writes: usize }
    impl TeamInviteFs125 for FakeFs125 {
        fn team_invite_exists(&self, path: &std::path::Path) -> bool { self.files.contains_key(path) }
        fn team_invite_read(&self, path: &std::path::Path) -> Result<String, String> { self.files.get(path).cloned().ok_or_else(|| "missing".to_owned()) }
        fn team_invite_write_json_atomic_0600(&mut self, path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> { self.writes += 1; self.files.insert(path.to_path_buf(), serde_json::to_string_pretty(value).unwrap() + "\n"); Ok(()) }
    }

    #[derive(Default)]
    struct FakeTrust125 { trusted: BTreeSet<(String, String, String)> }
    impl TeamInviteTrustStore125 for FakeTrust125 {
        fn team_invite_is_trusted(&self, from: &str, to: &str, action: &str) -> bool { self.trusted.contains(&(from.to_owned(), to.to_owned(), action.to_owned())) }
    }

    #[derive(Default)]
    struct FakeConsent125 { writes: Vec<TeamInviteConsentRequest125>, ids: Vec<String>, pins: Vec<String> }
    impl TeamInviteConsentStore125 for FakeConsent125 {
        fn team_invite_write_pending(&mut self, request: &TeamInviteConsentRequest125) -> Result<(), String> { self.writes.push(request.clone()); Ok(()) }
        fn team_invite_next_id(&mut self) -> String { self.ids.pop().unwrap_or_else(|| "abcdefabcdefabcdefabcdef".to_owned()) }
        fn team_invite_next_pin(&mut self) -> String { self.pins.pop().unwrap_or_else(|| "ABCDEF".to_owned()) }
    }

    #[derive(Default)]
    struct FakeHttp125 { posts: Vec<(String, TeamInviteConsentRequest125)>, ok: bool }
    impl TeamInviteHttpClient125 for FakeHttp125 {
        fn team_invite_post_consent(&mut self, base_url: &str, request: &TeamInviteConsentRequest125) -> TeamInvitePostResult125 { self.posts.push((base_url.to_owned(), request.clone())); TeamInvitePostResult125 { ok: self.ok, status: if self.ok { 201 } else { 503 }, error: if self.ok { None } else { Some("peer offline".to_owned()) } } }
    }

    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
    fn config() -> TeamInviteConfig125 { TeamInviteConfig125 { node: Some("lead-node".to_owned()), named_peers: vec![TeamInviteNamedPeer125 { name: "scout".to_owned(), url: "http://scout.example:3456".to_owned(), node: Some("scout-node".to_owned()) }] } }
    fn manifest_path(team: &str) -> std::path::PathBuf { team_paths(team).vault_manifest }
    fn seed_manifest(fs: &mut FakeFs125) { fs.files.insert(manifest_path("alpha"), r#"{"name":"alpha","other":true,"invitees":[{"name":"builder","url":"http://builder","scope":"member","invitedAt":"keep"}]}"#.to_owned()); }

    #[test]
    fn team_invite_missing_team_stops_before_peer_lookup_or_consent() {
        let mut fs = FakeFs125::default(); let trust = FakeTrust125::default(); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125::default();
        let err = team_invite_with(&args(&["invite", "missing", "scout"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap_err();
        assert!(err.contains("team 'missing' not found")); assert_eq!(fs.writes, 0); assert!(consent.writes.is_empty()); assert!(http.posts.is_empty());
    }

    #[test]
    fn team_invite_unknown_peer_does_not_write() {
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let trust = FakeTrust125::default(); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125::default();
        let err = team_invite_with(&args(&["invite", "alpha", "ghost"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap_err();
        assert!(err.contains("unknown peer 'ghost'")); assert_eq!(fs.writes, 0); assert!(http.posts.is_empty());
    }

    #[test]
    fn team_invite_consent_off_records_and_preserves_unrelated_manifest_fields() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner); let _restore = EnvVarRestore::capture("MAW_CONSENT"); std::env::remove_var("MAW_CONSENT");
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let trust = FakeTrust125::default(); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125::default();
        let out = team_invite_with(&args(&["invite", "alpha", "scout", "--scope", "reviewer"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap();
        assert!(out.contains("invited 'scout'")); assert!(consent.writes.is_empty()); assert!(http.posts.is_empty());
        let value: serde_json::Value = serde_json::from_str(fs.files.get(&manifest_path("alpha")).unwrap()).unwrap();
        assert_eq!(value["other"], serde_json::json!(true)); assert_eq!(value["invitees"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn team_invite_trusted_scope_records_without_requesting_consent() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner); let _restore = EnvVarRestore::capture("MAW_CONSENT"); std::env::set_var("MAW_CONSENT", "1");
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let mut trust = FakeTrust125::default(); trust.trusted.insert(("lead-node".to_owned(), "scout-node".to_owned(), "team-invite".to_owned())); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125::default();
        let out = team_invite_with(&args(&["invite", "alpha", "scout"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap();
        assert!(out.contains("scope: member")); assert!(consent.writes.is_empty()); assert!(http.posts.is_empty());
    }

    #[test]
    fn team_invite_not_trusted_exits_two_message_and_never_writes_manifest() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner); let _restore = EnvVarRestore::capture("MAW_CONSENT"); let _time = EnvVarRestore::capture("MAW_RS_TEAM_FIXED_EXPIRES"); std::env::set_var("MAW_CONSENT", "1"); std::env::set_var("MAW_RS_TEAM_FIXED_EXPIRES", "2099-01-01T00:00:00.000Z");
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let trust = FakeTrust125::default(); let mut consent = FakeConsent125 { ids: vec!["111111111111111111111111".to_owned()], pins: vec!["ABCDEF".to_owned()], ..Default::default() }; let mut http = FakeHttp125 { ok: true, ..Default::default() };
        let err = team_invite_with(&args(&["invite", "alpha", "scout"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap_err();
        assert!(err.starts_with("__TEAM_INVITE_EXIT2__")); assert!(err.contains("request id: 111111111111111111111111")); assert!(err.contains("maw consent approve 111111111111111111111111 ABCDEF")); assert!(err.contains("then re-run:"));
        assert_eq!(fs.writes, 0); assert_eq!(consent.writes.len(), 1); assert_eq!(http.posts.len(), 1); assert!(!err.contains("pinHash"));
        let pending = team_invite_pending_json(&consent.writes[0]);
        assert!(pending.get("pinHash").is_some()); assert!(pending.get("plaintextPin").is_none());
    }

    #[test]
    fn team_invite_request_failure_keeps_manifest_unchanged() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner); let _restore = EnvVarRestore::capture("MAW_CONSENT"); std::env::set_var("MAW_CONSENT", "1");
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let trust = FakeTrust125::default(); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125 { ok: false, ..Default::default() };
        let err = team_invite_with(&args(&["invite", "alpha", "scout"]), &config(), &mut fs, &trust, &mut consent, &mut http).unwrap_err();
        assert!(err.contains("consent request failed")); assert!(err.contains("request id (local mirror)")); assert_eq!(fs.writes, 0); assert_eq!(consent.writes.len(), 1);
    }

    #[test]
    fn team_invite_validates_url_before_http_and_args_before_writes() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner); let _restore = EnvVarRestore::capture("MAW_CONSENT"); std::env::set_var("MAW_CONSENT", "1");
        let bad = TeamInviteConfig125 { node: Some("lead-node".to_owned()), named_peers: vec![TeamInviteNamedPeer125 { name: "scout".to_owned(), url: "file:///etc/passwd".to_owned(), node: None }] };
        let mut fs = FakeFs125::default(); seed_manifest(&mut fs); let trust = FakeTrust125::default(); let mut consent = FakeConsent125::default(); let mut http = FakeHttp125::default();
        let err = team_invite_with(&args(&["invite", "alpha", "scout"]), &bad, &mut fs, &trust, &mut consent, &mut http).unwrap_err();
        assert!(err.contains("invalid peer URL")); assert_eq!(fs.writes, 0); assert!(http.posts.is_empty());
        assert!(team_invite_validate_url("https://peer.example/path").is_err());
        assert!(team_invite_validate_url("https://peer.example?x=1").is_err());
        assert!(team_invite_validate_url("https://peer.example:443").is_ok());
        assert!(team_invite_parse(&args(&["invite", "../bad", "scout"])).is_err());
        assert!(team_invite_parse(&args(&["invite", "alpha", "../scout"])).is_err());
    }

    #[test]
    fn team_invite_trust_json_is_action_scoped() {
        let value = serde_json::json!([{ "from":"lead", "to":"peer", "action":"hey" }, { "from":"lead", "to":"peer", "action":"team-invite" }]);
        assert!(team_invite_trust_json_contains(&value, "lead", "peer", "team-invite"));
        assert!(!team_invite_trust_json_contains(&value, "lead", "peer", "plugin-install"));
    }

    #[test]
    fn team_invite_sentinel_maps_to_exit_two() {
        let output = team_output_from_result(Err("__TEAM_INVITE_EXIT2__consent required".to_owned()));
        assert_eq!(output.code, 2);
        assert_eq!(output.stderr, "consent required\n");
    }
}
