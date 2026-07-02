const DISPATCH_78: &[DispatcherEntry] = &[DispatcherEntry {
    command: "kill",
    handler: Handler::Sync(kill_run_command),
}];

const KILL_USAGE: &str = "usage: maw kill <target>[:window] [--pane N] [--index N|--all] [--peer <alias>]  (see: maw sleep for graceful stop, maw done for worktrees)";
const KILL_WINDOW_FORMAT: &str =
    "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}";
const KILL_PEER_API_PATH: &str = "/api/kill";
const KILL_PEER_CURL_TIMEOUT_SECONDS: &str = "5";
const KILL_PEER_HTTP_STATUS_MARKER: &str = "__MAW_HTTP_STATUS__:";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct KillOptions {
    target: String,
    pane: Option<u32>,
    index: Option<u32>,
    all: bool,
    peer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeer {
    alias: String,
    url: String,
    node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeerRequest {
    peer: KillPeer,
    target: String,
    pane: Option<u32>,
    index: Option<u32>,
    all: bool,
    from: String,
    peer_key: String,
    timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeerResponse {
    output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillSession {
    name: String,
    windows: Vec<KillWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillWindow {
    index: u32,
    name: String,
}

trait KillTmux {
    fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String>;
    fn kill_list_panes_all(&mut self) -> Result<String, String>;
    fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String>;
    fn kill_kill_session(&mut self, session: &str) -> Result<(), String>;
    fn kill_kill_window(&mut self, target: &str) -> Result<(), String>;
    fn kill_kill_pane(&mut self, target: &str) -> Result<(), String>;
}

trait KillPeerTransport {
    fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String>;
}

struct KillSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

struct KillCurlPeerTransport;

impl KillSystemTmux {
    fn kill_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl KillPeerTransport for KillCurlPeerTransport {
    fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String> {
        kill_validate_peer_request(request)?;
        let body = kill_peer_body(request)?;
        let headers = sign_headers_v3_at(
            &request.peer_key,
            &request.from,
            "POST",
            KILL_PEER_API_PATH,
            Some(body.as_bytes()),
            request.timestamp,
        )?;
        let argv = kill_peer_curl_argv(&request.peer.url, &headers, &body)?;
        let output = kill_spawn_curl(&argv)?;
        let (status, body) = kill_split_peer_http_output(&output)?;
        kill_parse_peer_response(&request.peer.alias, &request.peer.url, status, &body)
    }
}

impl KillTmux for KillSystemTmux {
    fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String> {
        kill_tmux_run(
            &mut self.runner,
            "list-windows",
            &["-a", "-F", KILL_WINDOW_FORMAT],
        )
        .map(|raw| kill_parse_sessions(&raw))
    }

    fn kill_list_panes_all(&mut self) -> Result<String, String> {
        kill_tmux_run(
            &mut self.runner,
            "list-panes",
            &["-a", "-F", maw_tmux::PANE_TARGET_FORMAT],
        )
    }

    fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(
            &mut self.runner,
            "list-panes",
            &["-t", target, "-F", "#{pane_index}"],
        )
        .map(|raw| kill_parse_numbers(&raw))
    }

    fn kill_kill_session(&mut self, session: &str) -> Result<(), String> {
        kill_validate_tmux_target(session)?;
        kill_tmux_run(&mut self.runner, "kill-session", &["-t", session]).map(|_| ())
    }

    fn kill_kill_window(&mut self, target: &str) -> Result<(), String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(&mut self.runner, "kill-window", &["-t", target]).map(|_| ())
    }

    fn kill_kill_pane(&mut self, target: &str) -> Result<(), String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(&mut self.runner, "kill-pane", &["-t", target]).map(|_| ())
    }
}

fn kill_run_command(argv: &[String]) -> CliOutput {
    kill_run_command_with(
        argv,
        &mut KillSystemTmux::kill_new(),
        &mut KillCurlPeerTransport,
        &load_hey_config(),
        load_peer_key,
        kill_now_seconds,
    )
}

fn kill_run_command_with(
    argv: &[String],
    tmux: &mut impl KillTmux,
    peer: &mut impl KillPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> CliOutput {
    match kill_run(argv, tmux, peer, config, peer_key, now) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn kill_run(
    argv: &[String],
    tmux: &mut impl KillTmux,
    peer: &mut impl KillPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> Result<String, String> {
    let options = kill_parse_args(argv)?;
    if options.peer.is_some() {
        return kill_peer_forward(&options, peer, config, peer_key, now);
    }
    kill_validate_user_target(&options.target)?;
    let (raw_session, raw_window) = kill_split_target(&options.target);
    kill_validate_user_target(&raw_session)?;
    let sessions = tmux.kill_list_sessions()?;
    kill_resolve_and_apply(tmux, &sessions, &raw_session, &raw_window, &options)
}

fn kill_parse_args(argv: &[String]) -> Result<KillOptions, String> {
    let mut options = KillOptions::default();
    let mut index = 0;
    while index < argv.len() {
        index += kill_parse_arg(argv, index, &mut options)?;
    }
    if options.target.is_empty() || options.target == "--help" || options.target == "-h" {
        return Err(KILL_USAGE.to_owned());
    }
    Ok(options)
}

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

fn kill_peer_curl_argv(peer_url: &str, headers: &Headers, body: &str) -> Result<Vec<String>, String> {
    kill_validate_peer_url(peer_url)?;
    if body.chars().any(|ch| ch == '\0' || ch.is_control()) { return Err("kill peer body must not contain NUL/control characters".to_owned()); }
    let url = format!("{}{}", peer_url.trim_end_matches('/'), KILL_PEER_API_PATH);
    let mut argv = vec![
        "-sS".to_owned(),
        "--max-time".to_owned(),
        KILL_PEER_CURL_TIMEOUT_SECONDS.to_owned(),
        "-X".to_owned(),
        "POST".to_owned(),
        "-w".to_owned(),
        format!("{KILL_PEER_HTTP_STATUS_MARKER}%{{http_code}}"),
        "-H".to_owned(),
        "Content-Type: application/json".to_owned(),
    ];
    for (name, value) in headers.to_btree_map() {
        argv.push("-H".to_owned());
        argv.push(format!("{name}: {value}"));
    }
    argv.push("--data-binary".to_owned());
    argv.push(body.to_owned());
    argv.push("--".to_owned());
    argv.push(url);
    kill_validate_curl_argv(&argv)?;
    Ok(argv)
}

fn kill_validate_curl_argv(argv: &[String]) -> Result<(), String> {
    if !argv.iter().any(|arg| arg == "--") { return Err("curl argv must include -- URL separator".to_owned()); }
    for arg in argv {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("curl argv must not contain NUL/control characters".to_owned());
        }
    }
    Ok(())
}

fn kill_spawn_curl(argv: &[String]) -> Result<String, String> {
    kill_validate_curl_argv(argv)?;
    let output = std::process::Command::new("curl")
        .args(argv)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("failed to spawn curl: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(format!("curl failed: {}", if stdout.is_empty() { stderr } else { stdout }));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("curl stdout was not utf8: {error}"))
}

fn kill_split_peer_http_output(raw: &str) -> Result<(u16, String), String> {
    let Some((body, status_raw)) = raw.rsplit_once(KILL_PEER_HTTP_STATUS_MARKER) else {
        return Err("curl output missing HTTP status marker".to_owned());
    };
    let status = status_raw.trim().parse::<u16>().map_err(|_| format!("invalid HTTP status from curl: {status_raw}"))?;
    Ok((status, body.trim_end_matches('\n').to_owned()))
}

fn kill_parse_peer_response(alias: &str, peer_url: &str, status: u16, raw: &str) -> Result<KillPeerResponse, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| format!("peer kill failed ({alias} {peer_url}): invalid json: {error}; body={raw}"))?;
    if status == 404 {
        return Err(format!("peer {alias} does not support /api/kill (HTTP 404 at {peer_url})"));
    }
    if status >= 400 {
        let detail = value.get("error").and_then(serde_json::Value::as_str).unwrap_or("request failed");
        return Err(format!("peer kill failed ({alias} {peer_url}): {detail}"));
    }
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(KillPeerResponse { output: value.get("output").and_then(serde_json::Value::as_str).map(ToOwned::to_owned) });
    }
    let detail = value.get("error").and_then(serde_json::Value::as_str).unwrap_or("remote returned ok=false");
    Err(format!("peer kill failed ({alias} {peer_url}): {detail}"))
}

fn kill_now_seconds() -> i64 { i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX) }

fn kill_parse_non_negative(value: &str, flag: &str) -> Result<u32, String> {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!(
            "{flag} must be a non-negative integer (got {value})"
        ));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("{flag} must be a non-negative integer (got {value})"))
}

fn kill_split_target(target: &str) -> (String, String) {
    target.split_once(':').map_or_else(
        || (target.to_owned(), String::new()),
        |(session, window)| (session.to_owned(), window.to_owned()),
    )
}

fn kill_resolve_and_apply(
    tmux: &mut impl KillTmux,
    sessions: &[KillSession],
    raw_session: &str,
    raw_window: &str,
    options: &KillOptions,
) -> Result<String, String> {
    let names = sessions
        .iter()
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    match resolve_session_target(raw_session, &names) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => {
            let session = kill_find_session(sessions, &matched)?;
            kill_apply_resolved(tmux, session, raw_window, options)
        }
        ResolveResult::Ambiguous { candidates } => Err(kill_ambiguous_session(
            raw_session,
            &kill_sessions_for_names(sessions, &candidates),
        )),
        ResolveResult::None { hints } => {
            let hint_sessions = hints.map(|names| kill_sessions_for_names(sessions, &names));
            kill_apply_orphan_pane_fallback(
                tmux,
                raw_session,
                raw_window,
                options,
                hint_sessions.as_deref(),
            )
        }
    }
}

fn kill_find_session<'a>(
    sessions: &'a [KillSession],
    name: &str,
) -> Result<&'a KillSession, String> {
    sessions
        .iter()
        .find(|session| session.name == name)
        .ok_or_else(|| format!("session '{name}' not found after resolution"))
}

fn kill_sessions_for_names(sessions: &[KillSession], names: &[String]) -> Vec<KillSession> {
    names
        .iter()
        .filter_map(|name| sessions.iter().find(|session| session.name == *name))
        .cloned()
        .collect()
}

fn kill_apply_resolved(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    raw_window: &str,
    options: &KillOptions,
) -> Result<String, String> {
    kill_validate_tmux_target(&session.name)?;
    let indexes = kill_matching_window_indexes(session, raw_window, options)?;
    if let Some(pane) = options.pane {
        return kill_kill_resolved_pane(tmux, session, indexes.first().copied(), pane);
    }
    if raw_window.is_empty() && options.index.is_none() && !options.all {
        tmux.kill_kill_session(&session.name)?;
        return Ok(format!(
            "  \x1b[32m✓\x1b[0m killed session {}\n",
            session.name
        ));
    }
    kill_kill_resolved_windows(tmux, session, &indexes, options)
}

fn kill_apply_orphan_pane_fallback(
    tmux: &mut impl KillTmux,
    raw_session: &str,
    raw_window: &str,
    options: &KillOptions,
    hints: Option<&[KillSession]>,
) -> Result<String, String> {
    if raw_window.is_empty() && options.pane.is_none() {
        let pane_raw = tmux.kill_list_panes_all().unwrap_or_default();
        if !pane_raw.trim().is_empty() {
            return kill_resolve_orphan_pane(tmux, raw_session, &pane_raw);
        }
    }
    Err(kill_missing_session(raw_session, hints))
}

fn kill_resolve_orphan_pane(
    tmux: &mut impl KillTmux,
    raw_session: &str,
    pane_raw: &str,
) -> Result<String, String> {
    match maw_tmux::resolve_pane_target_from_list_panes_output(raw_session, pane_raw) {
        maw_tmux::PaneTargetResolution::Match { candidate } => {
            kill_validate_tmux_target(&candidate.resolved)?;
            tmux.kill_kill_pane(&candidate.resolved)?;
            Ok(format!(
                "  \x1b[32m✓\x1b[0m killed pane {raw_session} → {} \x1b[90m[{} ({})]\x1b[0m\n",
                candidate.resolved, candidate.source, candidate.name
            ))
        }
        maw_tmux::PaneTargetResolution::Ambiguous { candidates } => {
            Err(kill_ambiguous_panes(raw_session, &candidates))
        }
        maw_tmux::PaneTargetResolution::None => Err(kill_missing_session(raw_session, None)),
    }
}

fn kill_matching_window_indexes(
    session: &KillSession,
    raw_window: &str,
    options: &KillOptions,
) -> Result<Vec<u32>, String> {
    if options.all && options.index.is_some() {
        return Err("cannot combine --all and --index".to_owned());
    }
    if options.all && options.pane.is_some() {
        return Err("cannot combine --all and --pane".to_owned());
    }
    if let Some(index) = options.index {
        kill_require_window_index(session, index)?;
        return Ok(vec![index]);
    }
    if raw_window.is_empty() {
        return Ok(Vec::new());
    }
    if raw_window.chars().all(|ch| ch.is_ascii_digit()) {
        let index = kill_parse_non_negative(raw_window, "window index")?;
        kill_require_window_index(session, index)?;
        return Ok(vec![index]);
    }
    let matches = session
        .windows
        .iter()
        .filter(|window| window.name.eq_ignore_ascii_case(raw_window))
        .map(|window| window.index)
        .collect::<Vec<_>>();
    kill_validate_window_matches(session, raw_window, &matches, options.all)
}

fn kill_validate_window_matches(
    session: &KillSession,
    raw_window: &str,
    matches: &[u32],
    all: bool,
) -> Result<Vec<u32>, String> {
    if matches.is_empty() {
        return Err(format!(
            "window '{raw_window}' not found in session {} (valid: {})",
            session.name,
            kill_window_labels(session)
        ));
    }
    if matches.len() > 1 && !all {
        return Err(kill_ambiguous_window(session, raw_window, matches));
    }
    Ok(matches.to_vec())
}

fn kill_require_window_index(session: &KillSession, index: u32) -> Result<(), String> {
    if session.windows.iter().any(|window| window.index == index) {
        Ok(())
    } else {
        Err(format!(
            "window index {index} does not exist in session {} (valid: {})",
            session.name,
            kill_window_labels(session)
        ))
    }
}

fn kill_kill_resolved_pane(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    window_index: Option<u32>,
    pane_index: u32,
) -> Result<String, String> {
    let win =
        window_index.unwrap_or_else(|| session.windows.first().map_or(0, |window| window.index));
    kill_require_window_index(session, win)?;
    let win_target = format!("{}:{win}", session.name);
    kill_validate_tmux_target(&win_target)?;
    let valid = tmux.kill_list_pane_indexes(&win_target)?;
    if !valid.contains(&pane_index) {
        let list = kill_number_list(&valid);
        return Err(format!(
            "pane {pane_index} does not exist in window {win_target} (valid: {list})"
        ));
    }
    let pane = format!("{win_target}.{pane_index}");
    kill_validate_tmux_target(&pane)?;
    tmux.kill_kill_pane(&pane)?;
    Ok(format!("  \x1b[32m✓\x1b[0m killed pane {pane}\n"))
}

fn kill_kill_resolved_windows(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    indexes: &[u32],
    options: &KillOptions,
) -> Result<String, String> {
    if indexes.is_empty() {
        return Err(if options.all {
            "--all requires a window name target (session:window)".to_owned()
        } else {
            "window target required".to_owned()
        });
    }
    let mut killed = Vec::new();
    for index in indexes {
        let target = format!("{}:{index}", session.name);
        kill_validate_tmux_target(&target)?;
        tmux.kill_kill_window(&target)?;
        killed.push(target);
    }
    Ok(kill_window_success(&killed))
}

fn kill_window_success(killed: &[String]) -> String {
    if killed.len() == 1 {
        format!("  \x1b[32m✓\x1b[0m killed window {}\n", killed[0])
    } else {
        format!(
            "  \x1b[32m✓\x1b[0m killed {} windows {}\n",
            killed.len(),
            killed.join(", ")
        )
    }
}

fn kill_parse_sessions(raw: &str) -> Vec<KillSession> {
    let mut sessions = Vec::<KillSession>::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        kill_push_window(&mut sessions, line);
    }
    sessions
}

fn kill_push_window(sessions: &mut Vec<KillSession>, line: &str) {
    let parts = line.split("|||").collect::<Vec<_>>();
    let name = parts.first().copied().unwrap_or_default().to_owned();
    let index = parts
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let window = KillWindow {
        index,
        name: parts.get(2).copied().unwrap_or_default().to_owned(),
    };
    if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
        session.windows.push(window);
    } else {
        sessions.push(KillSession {
            name,
            windows: vec![window],
        });
    }
}

fn kill_parse_numbers(raw: &str) -> Vec<u32> {
    raw.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

fn kill_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    runner.run(subcommand, &args).map_err(|error| error.message)
}

fn kill_validate_user_target(target: &str) -> Result<(), String> {
    if target.is_empty()
        || target.trim() != target
        || target.starts_with('-')
        || target.contains('\0')
    {
        Err("kill target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn kill_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty()
        || target.trim() != target
        || target.starts_with('-')
        || target.contains('\0')
    {
        Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn kill_window_labels(session: &KillSession) -> String {
    if session.windows.is_empty() {
        return "(none)".to_owned();
    }
    session
        .windows
        .iter()
        .map(|window| format!("{}:{}", window.index, window.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn kill_number_list(values: &[u32]) -> String {
    if values.is_empty() {
        "(none)".to_owned()
    } else {
        values
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn kill_ambiguous_session(target: &str, candidates: &[KillSession]) -> String {
    let mut out = format!(
        "  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} sessions:",
        candidates.len()
    );
    for session in candidates {
        let _ = write!(out, "\n  \x1b[90m    • {}\x1b[0m", session.name);
    }
    out.push_str("\n  \x1b[90m  use the full name: maw kill <exact-session>\x1b[0m");
    out
}

fn kill_missing_session(target: &str, hints: Option<&[KillSession]>) -> String {
    let mut out = format!("  \x1b[31m✗\x1b[0m session '{target}' not found");
    if let Some(hints) = hints.filter(|hints| !hints.is_empty()) {
        out.push_str("\n  \x1b[90m  did you mean:\x1b[0m");
        for session in hints {
            let _ = write!(out, "\n  \x1b[90m    • {}\x1b[0m", session.name);
        }
    } else {
        out.push_str("\n  \x1b[90m  try: maw ls\x1b[0m");
    }
    out
}

fn kill_ambiguous_window(session: &KillSession, raw_window: &str, matches: &[u32]) -> String {
    let mut out = format!(
        "window '{raw_window}' is ambiguous in session {} — matches {} windows:",
        session.name,
        matches.len()
    );
    for index in matches {
        if let Some(window) = session.windows.iter().find(|window| window.index == *index) {
            let _ = write!(out, "\n    • {}:{}", window.index, window.name);
        }
    }
    out.push_str("\n  use --index N to kill one, or --all to kill all matching windows");
    out
}

fn kill_ambiguous_panes(target: &str, candidates: &[maw_tmux::PaneTargetCandidate]) -> String {
    let mut out = format!(
        "  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} panes:",
        candidates.len()
    );
    for candidate in candidates {
        let _ = write!(
            out,
            "\n  \x1b[90m    • {} → {} ({}) [{}]\x1b[0m",
            candidate.name, candidate.resolved, candidate.target, candidate.source
        );
    }
    out.push_str("\n  \x1b[90m  use the pane id or full session:window.pane target\x1b[0m");
    out
}

#[cfg(test)]
mod kill_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct KillFakeTmux {
        sessions_raw: String,
        panes_all_raw: String,
        pane_indexes_raw: String,
        calls: Vec<(String, Vec<String>)>,
        fail_kill: Option<String>,
    }

    impl KillTmux for KillFakeTmux {
        fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String> {
            self.calls.push((
                "list-windows".to_owned(),
                kill_strings(&["-a", "-F", KILL_WINDOW_FORMAT]),
            ));
            Ok(kill_parse_sessions(&self.sessions_raw))
        }

        fn kill_list_panes_all(&mut self) -> Result<String, String> {
            self.calls.push((
                "list-panes".to_owned(),
                kill_strings(&["-a", "-F", maw_tmux::PANE_TARGET_FORMAT]),
            ));
            Ok(self.panes_all_raw.clone())
        }

        fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String> {
            kill_validate_tmux_target(target)?;
            self.calls.push((
                "list-panes".to_owned(),
                kill_strings(&["-t", target, "-F", "#{pane_index}"]),
            ));
            Ok(kill_parse_numbers(&self.pane_indexes_raw))
        }

        fn kill_kill_session(&mut self, session: &str) -> Result<(), String> {
            kill_validate_tmux_target(session)?;
            self.calls
                .push(("kill-session".to_owned(), kill_strings(&["-t", session])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }

        fn kill_kill_window(&mut self, target: &str) -> Result<(), String> {
            kill_validate_tmux_target(target)?;
            self.calls
                .push(("kill-window".to_owned(), kill_strings(&["-t", target])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }

        fn kill_kill_pane(&mut self, target: &str) -> Result<(), String> {
            kill_validate_tmux_target(target)?;
            self.calls
                .push(("kill-pane".to_owned(), kill_strings(&["-t", target])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }
    }

    #[derive(Debug, Default)]
    struct KillFakePeer {
        requests: Vec<KillPeerRequest>,
        response: Option<KillPeerResponse>,
        fail: Option<String>,
    }

    impl KillPeerTransport for KillFakePeer {
        fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String> {
            kill_validate_peer_request(request)?;
            self.requests.push(request.clone());
            if let Some(message) = &self.fail {
                return Err(message.clone());
            }
            Ok(self.response.clone().unwrap_or(KillPeerResponse { output: None }))
        }
    }

    struct KillEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: std::path::PathBuf,
    }

    impl KillEnvGuard {
        fn new(label: &str) -> Self {
            let lock = env_test_lock().lock().expect("kill env lock");
            let keys = ["PEERS_FILE", "MAW_SENDER", "MAW_PEER_KEY", "HOME", "MAW_HOME", "MAW_STATE_DIR", "XDG_STATE_HOME"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let dir = std::env::temp_dir().join(format!("maw-rs-kill-peer-{label}-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            for key in ["MAW_HOME", "MAW_STATE_DIR", "XDG_STATE_HOME"] { std::env::remove_var(key); }
            std::env::set_var("HOME", &dir);
            std::env::set_var("PEERS_FILE", dir.join("peers.json"));
            std::env::set_var("MAW_SENDER", "local:test-oracle");
            std::env::set_var("MAW_PEER_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
            Self { saved, _lock: lock, dir }
        }

        fn write_peers(&self, body: &str) {
            std::fs::write(self.dir.join("peers.json"), body).expect("write peers");
        }
    }

    impl Drop for KillEnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn kill_run_fake(argv: &[String], tmux: &mut impl KillTmux) -> CliOutput {
        let mut peer = KillFakePeer::default();
        kill_run_command_with(
            argv,
            tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned()),
            || 1_700_000_000,
        )
    }

    fn kill_maybe_fail(error: Option<&String>) -> Result<(), String> {
        error.cloned().map_or(Ok(()), Err)
    }

    fn kill_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn kill_fake(sessions_raw: &str) -> KillFakeTmux {
        KillFakeTmux {
            sessions_raw: sessions_raw.to_owned(),
            ..KillFakeTmux::default()
        }
    }

    #[test]
    fn kill_dispatch_registers_native_kill() {
        assert_eq!(DISPATCH_78.len(), 1);
        assert_eq!(DISPATCH_78[0].command, "kill");
    }

    #[test]
    fn kill_session_resolves_and_validates_before_destructive_call() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["demo"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m killed session 07-demo\n");
        assert_eq!(tmux.calls[0].0, "list-windows");
        assert_eq!(
            tmux.calls[1],
            ("kill-session".to_owned(), kill_strings(&["-t", "07-demo"]))
        );
    }

    #[test]
    fn kill_rejects_leading_dash_target_before_listing_or_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["-Sbad"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn kill_refuses_invalid_resolved_session_before_destructive_call() {
        let mut tmux = kill_fake("-Sbad-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["demo"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("target/session"));
        assert_eq!(
            tmux.calls.len(),
            1,
            "listed before refusing resolved kill target"
        );
    }

    #[test]
    fn kill_window_index_and_all_are_validated_against_listing() {
        let mut tmux = kill_fake("07-demo|||0|||work|||1|||/tmp\n07-demo|||2|||work|||0|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["07-demo:work", "--all"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("killed 2 windows"));
        assert_eq!(
            tmux.calls[1],
            ("kill-window".to_owned(), kill_strings(&["-t", "07-demo:0"]))
        );
        assert_eq!(
            tmux.calls[2],
            ("kill-window".to_owned(), kill_strings(&["-t", "07-demo:2"]))
        );
    }

    #[test]
    fn kill_ambiguous_window_requires_index_or_all_and_does_not_kill() {
        let mut tmux = kill_fake("07-demo|||0|||work|||1|||/tmp\n07-demo|||2|||work|||0|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["07-demo:work"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("ambiguous"));
        assert_eq!(tmux.calls.len(), 1);
    }

    #[test]
    fn kill_pane_lists_valid_indexes_before_kill_pane() {
        let mut tmux = kill_fake("07-demo|||1|||main|||1|||/tmp\n");
        tmux.pane_indexes_raw = "0\n2\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["demo:1", "--pane", "2"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert_eq!(
            output.stdout,
            "  \x1b[32m✓\x1b[0m killed pane 07-demo:1.2\n"
        );
        assert_eq!(
            tmux.calls[1],
            (
                "list-panes".to_owned(),
                kill_strings(&["-t", "07-demo:1", "-F", "#{pane_index}"])
            )
        );
        assert_eq!(
            tmux.calls[2],
            ("kill-pane".to_owned(), kill_strings(&["-t", "07-demo:1.2"]))
        );
    }

    #[test]
    fn kill_pane_rejects_missing_pane_without_kill() {
        let mut tmux = kill_fake("07-demo|||1|||main|||1|||/tmp\n");
        tmux.pane_indexes_raw = "0\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["demo:1", "--pane=2"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("pane 2 does not exist"));
        assert!(!tmux.calls.iter().any(|call| call.0 == "kill-pane"));
    }

    #[test]
    fn kill_orphan_pane_fallback_uses_pane_resolver_before_kill() {
        let mut tmux = kill_fake("");
        tmux.panes_all_raw = "%42|||07-demo:1.0|||agent|||role|||/repo/demo\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["agent"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("killed pane agent → %42"));
        assert_eq!(tmux.calls[0].0, "list-windows");
        assert_eq!(tmux.calls[1].0, "list-panes");
        assert_eq!(
            tmux.calls[2],
            ("kill-pane".to_owned(), kill_strings(&["-t", "%42"]))
        );
    }

    #[test]
    fn kill_missing_session_prints_hints_and_does_not_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["dem"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("did you mean"));
        assert!(!tmux.calls.iter().any(|call| call.0.starts_with("kill-")));
    }



    #[test]
    fn kill_peer_forward_posts_signed_body_and_skips_local_tmux() {
        let env = KillEnvGuard::new("forward");
        env.write_peers(r#"{"version":1,"peers":{"neo":{"url":"http://peer.example:3456","node":"neo-node","addedAt":"1"}}}"#);
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let mut peer = KillFakePeer { response: Some(KillPeerResponse { output: Some("remote log\n".to_owned()) }), ..KillFakePeer::default() };
        let output = kill_run_command_with(
            &kill_strings(&["target", "--pane", "3", "--peer", "neo"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "\x1b[32m✓\x1b[0m forwarded kill → neo (http://peer.example:3456) — target\nremote log\n");
        assert!(tmux.calls.is_empty(), "peer kill must not touch local tmux");
        assert_eq!(peer.requests.len(), 1);
        let request = &peer.requests[0];
        assert_eq!(request.peer.alias, "neo");
        assert_eq!(request.peer.url, "http://peer.example:3456");
        assert_eq!(request.target, "target");
        assert_eq!(request.pane, Some(3));
        assert_eq!(request.from, "test-oracle:local");
    }

    #[test]
    fn kill_peer_unknown_alias_is_clean_error_without_transport() {
        let env = KillEnvGuard::new("missing");
        env.write_peers(r#"{"version":1,"peers":{}}"#);
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let mut peer = KillFakePeer::default();
        let output = kill_run_command_with(
            &kill_strings(&["target", "--peer", "missing"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("key".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("unknown peer alias: missing"));
        assert!(tmux.calls.is_empty());
        assert!(peer.requests.is_empty());
    }

    #[test]
    fn kill_peer_validates_alias_and_target_before_transport() {
        let env = KillEnvGuard::new("invalid");
        env.write_peers(r#"{"version":1,"peers":{"neo":{"url":"http://peer.example"}}}"#);
        let mut tmux = kill_fake("");
        let mut peer = KillFakePeer::default();
        let output = kill_run_command_with(
            &kill_strings(&["target", "--peer", "bad;alias"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("key".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("invalid peer alias"));
        assert!(peer.requests.is_empty());
    }

    #[test]
    fn kill_peer_body_and_curl_argv_are_argv_no_shell() {
        let request = KillPeerRequest {
            peer: KillPeer { alias: "neo".to_owned(), url: "http://peer".to_owned(), node: None },
            target: "target".to_owned(),
            pane: Some(1),
            index: Some(2),
            all: true,
            from: "oracle:node".to_owned(),
            peer_key: "key".to_owned(),
            timestamp: 1,
        };
        let body = kill_peer_body(&request).expect("body");
        let value = serde_json::from_str::<serde_json::Value>(&body).expect("json body");
        assert_eq!(value["target"], "target");
        assert_eq!(value["pane"], 1);
        assert_eq!(value["index"], 2);
        assert_eq!(value["all"], true);
        let headers = sign_headers_v3_at("key", "oracle:node", "POST", KILL_PEER_API_PATH, Some(body.as_bytes()), 1).expect("headers");
        let argv = kill_peer_curl_argv("http://peer/", &headers, &body).expect("argv");
        assert!(argv.iter().any(|arg| arg == "--"));
        assert!(argv.iter().any(|arg| arg == "http://peer/api/kill"));
        assert!(argv.windows(2).any(|pair| pair == ["--data-binary", body.as_str()]));
        assert!(!argv.iter().any(|arg| arg == "sh" || arg == "-c"));
    }

    #[test]
    fn kill_peer_response_maps_404_and_remote_errors_like_maw_js() {
        let unsupported = kill_parse_peer_response("neo", "http://peer", 404, r"{}").unwrap_err();
        assert_eq!(unsupported, "peer neo does not support /api/kill (HTTP 404 at http://peer)");
        let maintenance = kill_parse_peer_response("neo", "http://peer", 503, r#"{"error":"maintenance"}"#).unwrap_err();
        assert_eq!(maintenance, "peer kill failed (neo http://peer): maintenance");
        let ok = kill_parse_peer_response("neo", "http://peer", 200, r#"{"ok":true,"output":"remote log"}"#).expect("ok");
        assert_eq!(ok.output.as_deref(), Some("remote log"));
    }

    #[test]
    fn kill_peer_split_http_output_reads_marker() {
        let (status, body) = kill_split_peer_http_output("{\"ok\":true}\n__MAW_HTTP_STATUS__:200").expect("split");
        assert_eq!(status, 200);
        assert_eq!(body, "{\"ok\":true}");
    }

    #[test]
    fn kill_rejects_bad_flag_combinations_before_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(
            &kill_strings(&["demo:main", "--all", "--pane", "0"]),
            &mut tmux,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("cannot combine --all and --pane"));
        assert_eq!(tmux.calls.len(), 1);
    }
}
