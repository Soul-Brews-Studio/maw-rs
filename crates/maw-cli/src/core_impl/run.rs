const DISPATCH_91: &[DispatcherEntry] = &[DispatcherEntry {
    command: "run",
    handler: Handler::Sync(run_native_command),
}];

const RUN_USAGE: &str = "usage: maw-rs run <target> \"<cmd>\"";
const RUN_PANE_KEYS_PATH: &str = "/api/pane-keys";
const RUN_CURL_TIMEOUT_SECONDS: &str = "5";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunArgs {
    target: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunPeerRequest {
    node: String,
    peer_url: String,
    target: String,
    text: String,
    from: String,
    peer_key: String,
    timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunPeerResponse {
    target: Option<String>,
}

struct RunPeerDeps<'a, P: RunPeerTransport> {
    peer: &'a mut P,
    config: &'a HeyConfig,
    from: Option<&'a str>,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
}

trait RunTmux {
    fn run_sessions(&mut self) -> Vec<RouteSession>;
    fn run_send_literal(&mut self, target: &str, text: &str) -> Result<(), String>;
    fn run_send_enter(&mut self, target: &str) -> Result<(), String>;
}

trait RunPeerTransport {
    fn run_peer_keys(&mut self, request: &RunPeerRequest) -> Result<RunPeerResponse, String>;
}

struct RunSystemTmux {
    client: TmuxClient<maw_tmux::CommandTmuxRunner>,
}

struct RunCurlPeerTransport;

impl RunSystemTmux {
    fn run_new() -> Self {
        Self {
            client: TmuxClient::local(),
        }
    }
}

impl RunTmux for RunSystemTmux {
    fn run_sessions(&mut self) -> Vec<RouteSession> {
        self.client
            .list_all()
            .into_iter()
            .map(run_route_session_from_tmux)
            .collect()
    }

    fn run_send_literal(&mut self, target: &str, text: &str) -> Result<(), String> {
        run_validate_tmux_target(target)?;
        run_validate_command_text(text)?;
        self.client.send_keys_literal(target, text).map_err(|error| error.to_string())
    }

    fn run_send_enter(&mut self, target: &str) -> Result<(), String> {
        run_validate_tmux_target(target)?;
        self.client.send_enter(target).map_err(|error| error.to_string())
    }
}

impl RunPeerTransport for RunCurlPeerTransport {
    fn run_peer_keys(&mut self, request: &RunPeerRequest) -> Result<RunPeerResponse, String> {
        run_validate_peer_request(request)?;
        let body = run_peer_body(&request.target, &request.text)?;
        let headers = sign_headers_v3_at(
            &request.peer_key,
            &request.from,
            "POST",
            RUN_PANE_KEYS_PATH,
            Some(body.as_bytes()),
            request.timestamp,
        )?;
        let argv = run_curl_argv(&request.peer_url, &headers, &body)?;
        let output = run_spawn_curl(&argv)?;
        run_parse_peer_response(&request.node, &request.peer_url, &output)
    }
}

fn run_native_command(argv: &[String]) -> CliOutput {
    run_native_command_with(
        argv,
        &mut RunSystemTmux::run_new(),
        &mut RunCurlPeerTransport,
        &load_hey_config(),
        run_load_peer_key,
        run_now_seconds,
    )
}

fn run_native_command_with(
    argv: &[String],
    tmux: &mut impl RunTmux,
    peer: &mut impl RunPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> CliOutput {
    match run_run(argv, tmux, peer, config, peer_key, now) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("run: {message}\n"),
        },
    }
}

fn run_run(
    argv: &[String],
    tmux: &mut impl RunTmux,
    peer: &mut impl RunPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> Result<String, (i32, String)> {
    run_run_with_from(argv, tmux, peer, config, None, peer_key, now)
}

fn run_run_with_from(
    argv: &[String],
    tmux: &mut impl RunTmux,
    peer: &mut impl RunPeerTransport,
    config: &HeyConfig,
    from: Option<&str>,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> Result<String, (i32, String)> {
    let parsed = run_parse_args(argv).map_err(|message| (2, message))?;
    run_validate_target_query(&parsed.target).map_err(|message| (2, message))?;
    run_validate_command_text(&parsed.text).map_err(|message| (2, message))?;
    match resolve_route_target(&parsed.target, &config.route, &tmux.run_sessions()) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => run_local(&target, &parsed.text, tmux),
        RouteResult::Peer { peer_url, target, node } => {
            let mut deps = RunPeerDeps { peer, config, from, peer_key, now };
            run_peer(&node, &peer_url, &target, &parsed.text, &mut deps)
        }
        RouteResult::Error { detail, hint, .. } => Err((2, run_route_error(&detail, hint))),
    }
}

fn run_local(target: &str, text: &str, tmux: &mut impl RunTmux) -> Result<String, (i32, String)> {
    run_validate_tmux_target(target).map_err(|message| (2, message))?;
    if !text.is_empty() {
        tmux.run_send_literal(target, text)
            .map_err(|error| (1, format!("tmux send-keys failed: {error}")))?;
    }
    tmux.run_send_enter(target)
        .map_err(|error| (1, format!("tmux send-keys failed: {error}")))?;
    Ok(format!("\x1b[32mran\x1b[0m → {target}: {}\n", run_truncate(text, 200)))
}

fn run_peer(
    node: &str,
    peer_url: &str,
    target: &str,
    text: &str,
    deps: &mut RunPeerDeps<'_, impl RunPeerTransport>,
) -> Result<String, (i32, String)> {
    run_validate_node(node).map_err(|message| (2, message))?;
    run_validate_peer_url(peer_url).map_err(|message| (2, message))?;
    run_validate_tmux_target(target).map_err(|message| (2, message))?;
    let from = resolve_hey_wire_from(deps.from, deps.config).map_err(|message| (2, message))?;
    let request = RunPeerRequest {
        node: node.to_owned(),
        peer_url: peer_url.to_owned(),
        target: target.to_owned(),
        text: text.to_owned(),
        from,
        peer_key: (deps.peer_key)().map_err(|message| (1, message))?,
        timestamp: (deps.now)(),
    };
    let response = deps.peer.run_peer_keys(&request).map_err(|message| (1, message))?;
    let delivered = response.target.as_deref().unwrap_or(target);
    Ok(format!("\x1b[32mran\x1b[0m ⚡ {node} → {delivered}: {}\n", run_truncate(text, 200)))
}

fn run_parse_args(argv: &[String]) -> Result<RunArgs, String> {
    if argv.is_empty() {
        return Err(RUN_USAGE.to_owned());
    }
    let start = run_arg_start(argv)?;
    let Some(target) = argv.get(start) else {
        return Err(RUN_USAGE.to_owned());
    };
    let text = argv[start + 1..].join(" ");
    Ok(RunArgs { target: target.clone(), text })
}

fn run_arg_start(argv: &[String]) -> Result<usize, String> {
    match argv.first().map(String::as_str) {
        Some("--") => Ok(1),
        Some(value) if value.starts_with('-') => Err(run_flag_like_target(value)),
        Some(_) => Ok(0),
        None => Err(RUN_USAGE.to_owned()),
    }
}

fn run_flag_like_target(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a target. Use `--` before the target if needed.\n  {RUN_USAGE}")
}

fn run_validate_target_query(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.trim() != value || value.starts_with('-') {
        return Err("target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn run_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.trim() != value || value.starts_with('-') {
        return Err("tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn run_validate_command_text(value: &str) -> Result<(), String> {
    if value.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("command text must not contain NUL/control characters".to_owned());
    }
    Ok(())
}

fn run_validate_node(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.trim() != value {
        return Err("peer node must be a safe token".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("peer node must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn run_validate_peer_url(value: &str) -> Result<(), String> {
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err("peer url must start with http:// or https://".to_owned());
    }
    if value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) {
        return Err("peer url must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn run_validate_peer_request(request: &RunPeerRequest) -> Result<(), String> {
    run_validate_node(&request.node)?;
    run_validate_peer_url(&request.peer_url)?;
    run_validate_tmux_target(&request.target)?;
    run_validate_command_text(&request.text)?;
    if request.from.is_empty() || request.peer_key.is_empty() || request.timestamp <= 0 {
        return Err("peer request auth fields are incomplete".to_owned());
    }
    Ok(())
}

fn run_peer_body(target: &str, text: &str) -> Result<String, String> {
    run_validate_tmux_target(target)?;
    run_validate_command_text(text)?;
    serde_json::to_string(&serde_json::json!({ "target": target, "text": text, "enter": true }))
        .map_err(|error| error.to_string())
}

fn run_curl_argv(peer_url: &str, headers: &Headers, body: &str) -> Result<Vec<String>, String> {
    run_validate_peer_url(peer_url)?;
    run_validate_command_text(body)?;
    let url = format!("{}{}", peer_url.trim_end_matches('/'), RUN_PANE_KEYS_PATH);
    let mut argv = vec![
        "-sS".to_owned(),
        "--fail-with-body".to_owned(),
        "--max-time".to_owned(),
        RUN_CURL_TIMEOUT_SECONDS.to_owned(),
        "-X".to_owned(),
        "POST".to_owned(),
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
    run_validate_curl_argv(&argv)?;
    Ok(argv)
}

fn run_validate_curl_argv(argv: &[String]) -> Result<(), String> {
    if !argv.iter().any(|arg| arg == "--") {
        return Err("curl argv must include -- URL separator".to_owned());
    }
    for arg in argv {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("curl argv must not contain NUL/control characters".to_owned());
        }
    }
    Ok(())
}

fn run_spawn_curl(argv: &[String]) -> Result<String, String> {
    run_validate_curl_argv(argv)?;
    let output = std::process::Command::new("curl")
        .args(argv)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("failed to spawn curl: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(format!("curl failed: {}", run_nonempty_or(stdout, stderr)));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("curl stdout was not utf8: {error}"))
}

fn run_parse_peer_response(node: &str, peer_url: &str, raw: &str) -> Result<RunPeerResponse, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| format!("peer run failed ({node} {peer_url}): invalid json: {error}; body={raw}"))?;
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(RunPeerResponse {
            target: value.get("target").and_then(serde_json::Value::as_str).map(ToOwned::to_owned),
        });
    }
    let underlying = value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("remote returned ok=false");
    Err(format!("peer run failed ({node} {peer_url}): {underlying}"))
}

fn run_route_error(detail: &str, hint: Option<String>) -> String {
    hint.map_or_else(|| detail.to_owned(), |hint| format!("{detail} — {hint}"))
}

fn run_route_session_from_tmux(session: TmuxSession) -> RouteSession {
    RouteSession {
        name: session.name,
        source: None,
        windows: session
            .windows
            .into_iter()
            .map(|window| RouteWindow {
                index: window.index,
                name: window.name,
                active: window.active,
            })
            .collect(),
    }
}

fn run_load_peer_key() -> Result<String, String> {
    load_peer_key()
}

fn run_now_seconds() -> i64 {
    i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX)
}

fn run_nonempty_or(first: String, second: String) -> String {
    if first.is_empty() {
        second
    } else {
        first
    }
}

fn run_truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Debug, Default)]
    struct RunFakeTmux {
        sessions: Vec<RouteSession>,
        sends: Vec<(String, String)>,
        enters: Vec<String>,
    }

    #[derive(Debug, Default)]
    struct RunFakePeer {
        requests: Vec<RunPeerRequest>,
        fail: Option<String>,
        response_target: Option<String>,
    }

    impl RunTmux for RunFakeTmux {
        fn run_sessions(&mut self) -> Vec<RouteSession> {
            self.sessions.clone()
        }

        fn run_send_literal(&mut self, target: &str, text: &str) -> Result<(), String> {
            run_validate_tmux_target(target)?;
            run_validate_command_text(text)?;
            self.sends.push((target.to_owned(), text.to_owned()));
            Ok(())
        }

        fn run_send_enter(&mut self, target: &str) -> Result<(), String> {
            run_validate_tmux_target(target)?;
            self.enters.push(target.to_owned());
            Ok(())
        }
    }

    impl RunPeerTransport for RunFakePeer {
        fn run_peer_keys(&mut self, request: &RunPeerRequest) -> Result<RunPeerResponse, String> {
            run_validate_peer_request(request)?;
            self.requests.push(request.clone());
            if let Some(error) = &self.fail {
                Err(error.clone())
            } else {
                Ok(RunPeerResponse { target: self.response_target.clone() })
            }
        }
    }

    fn run_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn run_window(index: u32, name: &str) -> RouteWindow {
        RouteWindow { index, name: name.to_owned(), active: index == 0 }
    }

    fn run_session(name: &str, windows: Vec<RouteWindow>) -> RouteSession {
        RouteSession { name: name.to_owned(), windows, source: None }
    }

    fn run_config() -> HeyConfig {
        HeyConfig {
            node: Some("test-node".to_owned()),
            oracle: Some("test-oracle".to_owned()),
            route: RouteConfig::default(),
        }
    }

    fn run_peer_config() -> HeyConfig {
        run_peer_config_with_identity("test-oracle", "test-node")
    }

    fn run_peer_config_with_identity(oracle: &str, node: &str) -> HeyConfig {
        let mut agents = HashMap::new();
        agents.insert("remote".to_owned(), "peer1".to_owned());
        HeyConfig {
            node: Some(node.to_owned()),
            oracle: Some(oracle.to_owned()),
            route: RouteConfig {
                node: Some(node.to_owned()),
                named_peers: vec![RouteNamedPeer { name: "peer1".to_owned(), url: "http://peer.example".to_owned() }],
                peers: Vec::new(),
                agents,
            },
        }
    }

    fn run_key() -> Result<String, String> {
        std::str::from_utf8(b"test-peer-key")
            .map(str::to_owned)
            .map_err(|error| error.to_string())
    }

    fn run_now() -> i64 {
        1_700_000_000
    }

    #[test]
    fn run_dispatch_registers_run_only_in_part91() {
        assert_eq!(DISPATCH_91[0].command, "run");
    }

    #[test]
    fn run_local_sends_literal_then_enter() {
        let mut tmux = RunFakeTmux {
            sessions: vec![run_session("work", vec![run_window(0, "shell")])],
            ..RunFakeTmux::default()
        };
        let mut peer = RunFakePeer::default();
        let output = run_run(&run_strings(&["work:shell", "ls", "-la"]), &mut tmux, &mut peer, &run_config(), run_key, run_now)
            .expect("run");
        assert!(output.contains("ran"));
        assert_eq!(tmux.sends, vec![("work:0".to_owned(), "ls -la".to_owned())]);
        assert_eq!(tmux.enters, vec!["work:0".to_owned()]);
        assert!(peer.requests.is_empty());
    }

    #[test]
    fn run_empty_text_sends_enter_only() {
        let mut tmux = RunFakeTmux { sessions: vec![run_session("work", vec![run_window(0, "shell")])], ..RunFakeTmux::default() };
        let mut peer = RunFakePeer::default();
        run_run(&run_strings(&["work:shell"]), &mut tmux, &mut peer, &run_config(), run_key, run_now).expect("enter");
        assert!(tmux.sends.is_empty());
        assert_eq!(tmux.enters, vec!["work:0".to_owned()]);
    }

    #[test]
    fn run_rejects_leading_dash_target_before_tmux() {
        let mut tmux = RunFakeTmux::default();
        let mut peer = RunFakePeer::default();
        let error = run_run(&run_strings(&["-bad", "echo"]), &mut tmux, &mut peer, &run_config(), run_key, run_now).expect_err("bad");
        assert_eq!(error.0, 2);
        assert!(error.1.contains("looks like a flag"));
        assert!(tmux.sends.is_empty());
    }

    #[test]
    fn run_separator_allows_explicit_target_position() {
        let parsed = run_parse_args(&run_strings(&["--", "work:shell", "echo", "ok"])).expect("parse");
        assert_eq!(parsed.target, "work:shell");
        assert_eq!(parsed.text, "echo ok");
    }

    #[test]
    fn run_rejects_control_text_before_tmux() {
        let mut tmux = RunFakeTmux::default();
        let mut peer = RunFakePeer::default();
        let error = run_run(&["work".to_owned(), "bad\ncmd".to_owned()], &mut tmux, &mut peer, &run_config(), run_key, run_now)
            .expect_err("control");
        assert!(error.1.contains("control"));
        assert!(tmux.enters.is_empty());
    }

    #[test]
    fn run_peer_posts_pane_keys_with_enter_true() {
        let mut tmux = RunFakeTmux::default();
        let mut peer = RunFakePeer { response_target: Some("remote:0.0".to_owned()), ..RunFakePeer::default() };
        let config = run_peer_config_with_identity("test-oracle", "test-node");
        let output = run_run_with_from(
            &run_strings(&["remote", "echo", "hi"]),
            &mut tmux,
            &mut peer,
            &config,
            Some("test-oracle:test-node"),
            run_key,
            run_now,
        )
        .expect("peer");
        assert!(output.contains("⚡ peer1 → remote:0.0"));
        assert_eq!(peer.requests.len(), 1);
        assert_eq!(peer.requests[0].target, "remote");
        assert_eq!(peer.requests[0].text, "echo hi");
        assert_eq!(peer.requests[0].from, "test-oracle:test-node");
        assert!(tmux.enters.is_empty());
    }

    #[test]
    fn run_peer_failure_is_reported() {
        let mut tmux = RunFakeTmux::default();
        let mut peer = RunFakePeer { fail: Some("peer run failed (peer1 http://peer.example): nope".to_owned()), ..RunFakePeer::default() };
        let error = run_run(&run_strings(&["remote", "echo"]), &mut tmux, &mut peer, &run_peer_config(), run_key, run_now).expect_err("peer fail");
        assert_eq!(error.0, 1);
        assert!(error.1.contains("nope"));
    }

    #[test]
    fn run_peer_body_matches_pane_keys_contract() {
        let body = run_peer_body("pane:0.0", "ls -la").expect("body");
        assert_eq!(body, r#"{"enter":true,"target":"pane:0.0","text":"ls -la"}"#);
    }

    #[test]
    fn run_curl_argv_has_separator_before_url() {
        let headers = sign_headers_v3_at("key", "test-oracle:test-node", "POST", RUN_PANE_KEYS_PATH, Some(b"{}"), run_now()).expect("headers");
        let argv = run_curl_argv("http://peer.example/", &headers, "{}").expect("argv");
        let sep = argv.iter().position(|arg| arg == "--").expect("separator");
        assert_eq!(argv[sep + 1], "http://peer.example/api/pane-keys");
        assert!(argv.iter().any(|arg| arg == "--data-binary"));
    }

    #[test]
    fn run_parse_peer_response_rejects_remote_error() {
        let error = run_parse_peer_response("n", "http://p", r#"{"ok":false,"error":"bad target"}"#).expect_err("bad");
        assert!(error.contains("bad target"));
    }
}
