#[derive(Debug, Clone, Default)]
struct SendArgs {
    target: String,
    text: String,
    inbox: Option<bool>,
    from: Option<String>,
    approve: bool,
    trust: bool,
    dry_run: bool,
}

#[derive(Debug, Clone, Default)]
struct WakeArgs {
    target: String,
    task: Option<String>,
    from: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct HeyConfig {
    node: Option<String>,
    oracle: Option<String>,
    route: RouteConfig,
}

fn run_hey_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_send_like_async_impl("hey", &args).await })
}

fn run_send_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_send_like_async_impl("send", &args).await })
}

fn run_wake_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_wake_async_impl(&args).await })
}

async fn run_send_like_async_impl(command: &str, raw_args: &[String]) -> CliOutput {
    if wants_help_before_positionals(raw_args, &["--from"]) {
        return help_output(send_usage(command));
    }
    let send_args = match parse_send_args(command, raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return send_usage_error(command, &message),
    };
    run_send_like_async_with_args(command, send_args, false).await
}

async fn run_hey_in_process(query: &str, message: &str, acl_bypass: bool) -> CliOutput {
    let send_args = send_args_for_inbox_hey(query, message);
    run_send_like_async_with_args("hey", send_args, acl_bypass).await
}

fn send_args_for_inbox_hey(query: &str, message: &str) -> SendArgs {
    SendArgs {
        target: query.to_owned(),
        text: message.to_owned(),
        inbox: None,
        from: None,
        approve: false,
        trust: false,
        dry_run: false,
    }
}

async fn run_send_like_async_with_args(
    command: &str,
    send_args: SendArgs,
    acl_bypass: bool,
) -> CliOutput {
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle(&config);
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    let result = resolve_send_route_target(
        &send_args.target,
        &config.route,
        &sessions,
        std::env::var_os("TMUX").is_some(),
        &mut runner,
    );
    if send_args.dry_run {
        return send_dry_run_output(command, &send_args, &result);
    }
    match result {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => send_local_message(
            command,
            &mut tmux,
            &target,
            &send_args.text,
            &config,
            &sender_oracle,
            send_args.from.as_deref(),
        ),
        RouteResult::Peer {
            peer_url,
            target,
            node: _,
        } => {
            gated_send_peer_message(
                command,
                &peer_url,
                &target,
                &send_args,
                &config,
                &sender_oracle,
                acl_bypass,
            )
            .await
        }
        RouteResult::Error { detail, hint, .. } => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: if let Some(hint) = hint {
                format!("{command}: {detail}; {hint}\n")
            } else {
                format!("{command}: {detail}\n")
            },
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SendAclGateResult {
    Proceed { stderr_prefix: String },
    Queued(CliOutput),
    Reject(CliOutput),
}

async fn gated_send_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    acl_bypass: bool,
) -> CliOutput {
    match send_acl_gate_peer(command, target, args, sender_oracle, acl_bypass) {
        SendAclGateResult::Proceed { stderr_prefix } => {
            send_acl_deliver_peer_message(
                command,
                peer_url,
                target,
                args,
                config,
                sender_oracle,
                stderr_prefix,
            )
            .await
        }
        SendAclGateResult::Queued(output) | SendAclGateResult::Reject(output) => output,
    }
}

async fn send_acl_deliver_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    stderr_prefix: String,
) -> CliOutput {
    send_acl_apply_proceed_stderr(
        send_peer_message(command, peer_url, target, args, config, sender_oracle).await,
        &stderr_prefix,
    )
}

fn send_acl_apply_proceed_stderr(mut output: CliOutput, stderr_prefix: &str) -> CliOutput {
    if !stderr_prefix.is_empty() {
        output.stderr = format!("{stderr_prefix}{}", output.stderr);
    }
    output
}

fn send_acl_gate_peer(
    command: &str,
    target: &str,
    args: &SendArgs,
    sender_oracle: &str,
    acl_bypass: bool,
) -> SendAclGateResult {
    if args.trust && !args.approve {
        return SendAclGateResult::Reject(CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{command}: --trust requires --approve\n"),
        });
    }
    let sender = match send_acl_sender(args, sender_oracle) {
        Ok(sender) => sender,
        Err(message) => {
            return SendAclGateResult::Reject(CliOutput {
                code: 2,
                stdout: String::new(),
                stderr: format!("{command}: {message}\n"),
            })
        }
    };
    let target = send_acl_actor_from_target(target);
    if args.approve || acl_bypass {
        let mut stderr_prefix = String::new();
        if args.approve && args.trust {
            if let Err(error) = scope_trust_add_to_path(&scope_trust_path(), &sender, &target, &inbox_iso_label(inbox_now_ms())) {
                let _ = writeln!(
                    stderr_prefix,
                    "warn: ACL trust add failed, allowing send: {error} — fix {}",
                    scope_trust_path().display()
                );
            }
        }
        return SendAclGateResult::Proceed { stderr_prefix };
    }
    let evaluation = match send_acl_evaluate_loaded(&sender, &target) {
        Ok(decision) => decision,
        Err(error) => {
            return SendAclGateResult::Proceed {
                stderr_prefix: format!("warn: ACL check failed, allowing send: {error}\n"),
            }
        }
    };
    match evaluation {
        ScopeAclDecision::Allow => SendAclGateResult::Proceed {
            stderr_prefix: String::new(),
        },
        ScopeAclDecision::Queue => match send_acl_queue_pending(&sender, &target, args) {
            Ok(output) => SendAclGateResult::Queued(output),
            Err(error) => SendAclGateResult::Proceed {
                stderr_prefix: format!("warn: ACL queue failed, allowing send: {error}\n"),
            },
        },
    }
}

fn send_acl_sender(args: &SendArgs, sender_oracle: &str) -> Result<String, String> {
    if let Some(explicit) = args.from.as_deref() {
        let wire = validate_wire_from(explicit)?;
        return send_acl_oracle_component(&wire);
    }
    send_acl_validate_actor(sender_oracle)
}

fn send_acl_oracle_component(wire_from: &str) -> Result<String, String> {
    let oracle = wire_from
        .split_once(':')
        .map_or(wire_from, |(oracle, _node)| oracle);
    send_acl_validate_actor(oracle)
}

fn send_acl_actor_from_target(target: &str) -> String {
    target
        .split_once(':')
        .map_or(target, |(oracle, _rest)| oracle)
        .to_owned()
}

fn send_acl_validate_actor(value: &str) -> Result<String, String> {
    scope_trust_validate_actor("ACL actor", value).map_err(|error| format!("ACL actor rejected: {error}"))
}

fn send_acl_evaluate_loaded(sender: &str, target: &str) -> Result<ScopeAclDecision, String> {
    let scopes = send_acl_load_scopes_strict()?;
    let trust = send_acl_load_trust_pairs_strict()?;
    if scopes.is_empty() {
        return Ok(ScopeAclDecision::Allow);
    }
    Ok(scope_acl_evaluate(sender, target, &scopes, &trust))
}

fn send_acl_load_scopes_strict() -> Result<Vec<ScopeNativeRecord>, String> {
    let dir = scope_native_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut scopes = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("ACL check failed, allowing send: read {}: {error} — fix {}", dir.display(), dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error} — fix {}", path.display(), path.display()))?;
        let scope = serde_json::from_str::<ScopeNativeRecord>(&body)
            .map_err(|error| format!("parse {}: {error} — fix {}", path.display(), path.display()))?;
        scopes.push(scope);
    }
    scopes.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(scopes)
}

fn send_acl_load_trust_pairs_strict() -> Result<Vec<ScopeAclTrustPair>, String> {
    let path = scope_trust_path();
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|error| format!("parse {}: {error} — fix {}", path.display(), path.display()))?;
    let Some(items) = value.as_array() else {
        return Err(format!("parse {}: expected array — fix {}", path.display(), path.display()));
    };
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let entry = scope_trust_entry_from_json(item)
            .ok_or_else(|| format!("parse {}: invalid trust entry — fix {}", path.display(), path.display()))?;
        entries.push(entry);
    }
    Ok(scope_trust_pairs(&entries))
}

fn send_acl_queue_pending(sender: &str, target: &str, args: &SendArgs) -> Result<CliOutput, String> {
    let env = inbox_real_env();
    let id = send_acl_pending_id()?;
    let message = InboxPendingMessage {
        id: id.clone(),
        sender: sender.to_owned(),
        target: target.to_owned(),
        query: Some(args.target.clone()),
        sent_at: inbox_iso_label(inbox_now_ms()),
        status: "pending".to_owned(),
        message: args.text.clone(),
    };
    inbox_write_pending(&inbox_state_pending_dir(&env), &message)?;
    Ok(CliOutput {
        code: 0,
        stdout: send_acl_format_queue_output(&id, sender, target),
        stderr: String::new(),
    })
}

fn send_acl_format_queue_output(id: &str, sender: &str, target: &str) -> String {
    format!(
        "queued pending ACL approval: {id}\n  sender: {sender}\n  target: {target}\n  review: maw inbox show-pending {id}\n  approve: maw inbox approve {id}\n"
    )
}

fn send_acl_pending_id() -> Result<String, String> {
    let suffix = send_acl_random_hex6().unwrap_or_else(|| {
        format!(
            "{:06x}",
            (current_epoch_seconds() ^ u64::from(std::process::id())) & 0x00ff_ffff
        )
    });
    inbox_pending_id(inbox_now_ms(), &suffix)
}

fn send_acl_random_hex6() -> Option<String> {
    let mut bytes = [0_u8; 3];
    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    std::io::Read::read_exact(&mut file, &mut bytes).ok()?;
    Some(hex_bytes(&bytes))
}

fn parse_send_args(command: &str, argv: &[String]) -> Result<SendArgs, String> {
    let mut inbox = None;
    let mut from = None;
    let mut positional = Vec::new();
    let mut approve = false;
    let mut trust = false;
    let mut dry_run = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--inbox" => inbox = Some(true),
            "--no-inbox" => inbox = Some(false),
            "--approve" => approve = true,
            "--trust" => trust = true,
            "--dry-run" => dry_run = true,
            "--from" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(format!("{command}: missing --from value"));
                };
                from = Some(value.clone());
                index += 1;
            }
            value if value.starts_with("--from=") => {
                from = Some(value["--from=".len()..].to_owned());
            }
            value if value.starts_with('-') => return Err(format!("{command}: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if trust && !approve {
        return Err(format!("{command}: --trust requires --approve"));
    }
    if positional.len() < 2 {
        return Err(format!("{command}: target and message are required"));
    }
    Ok(SendArgs {
        target: positional[0].clone(),
        text: positional[1..].join(" "),
        inbox,
        from,
        approve,
        trust,
        dry_run,
    })
}

fn send_usage_error(command: &str, message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", send_usage(command)),
    }
}

fn send_usage(command: &str) -> String {
    format!(
        "usage: maw-rs {command} <target> <message> [--inbox|--no-inbox] [--from <oracle:node>] [--approve] [--trust] [--dry-run]"
    )
}

fn resolve_send_route_target<R: maw_tmux::TmuxRunner>(
    query: &str,
    config: &RouteConfig,
    sessions: &[RouteSession],
    inside_tmux: bool,
    runner: &mut R,
) -> RouteResult {
    let current_session = if is_self_target_alias(query) {
        match send_current_session_name(inside_tmux, runner) {
            Ok(current_session) => current_session,
            Err(detail) => {
                return RouteResult::Error {
                    reason: "me_needs_tmux".to_owned(),
                    detail,
                    hint: Some("run inside tmux so maw can resolve the current session".to_owned()),
                }
            }
        }
    } else {
        None
    };
    resolve_route_target_with_current_session(query, config, sessions, current_session.as_deref())
}

fn send_current_session_name<R: maw_tmux::TmuxRunner>(
    inside_tmux: bool,
    runner: &mut R,
) -> Result<Option<String>, String> {
    if !inside_tmux {
        return Ok(None);
    }
    let raw = runner
        .run(
            "display-message",
            &["-p".to_owned(), "#{session_name}".to_owned()],
        )
        .map_err(|error| {
            format!("'me' needs a tmux context: tmux display-message failed: {}", error.message)
        })?;
    let session = raw.trim();
    if session.is_empty() {
        return Err("'me' needs a tmux context: tmux did not report a current session".to_owned());
    }
    Ok(Some(session.to_owned()))
}

fn send_dry_run_output(command: &str, args: &SendArgs, result: &RouteResult) -> CliOutput {
    match result {
        RouteResult::Local { target } => CliOutput {
            code: 0,
            stdout: format!("dry-run: {command} {} -> local {target}\n", args.target),
            stderr: String::new(),
        },
        RouteResult::SelfNode { target } => CliOutput {
            code: 0,
            stdout: format!("dry-run: {command} {} -> self-node {target}\n", args.target),
            stderr: String::new(),
        },
        RouteResult::Peer {
            peer_url,
            target,
            node,
        } => CliOutput {
            code: 0,
            stdout: format!(
                "dry-run: {command} {} -> peer {node} {target} via {peer_url}\n",
                args.target
            ),
            stderr: String::new(),
        },
        RouteResult::Error { detail, hint, .. } => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: if let Some(hint) = hint {
                format!("{command}: {detail}; {hint}\n")
            } else {
                format!("{command}: {detail}\n")
            },
        },
    }
}

fn wake_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", wake_peer_usage()),
    }
}

fn wake_peer_usage() -> &'static str {
    "usage: maw-rs wake <target> [--task <task>] [--from <oracle:node>]"
}

fn send_local_message(
    command: &str,
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
    target: &str,
    text: &str,
    config: &HeyConfig,
    sender_oracle: &str,
    from: Option<&str>,
) -> CliOutput {
    let outbound = format_local_hey_message(text, config, sender_oracle, from);
    if let Err(error) = tmux.send_text(target, &outbound) {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{command}: tmux send-text failed: {error}\n"),
        };
    }
    CliOutput {
        code: 0,
        stdout: format!("delivered {target}\n"),
        stderr: String::new(),
    }
}

async fn send_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
) -> CliOutput {
    let from = match resolve_hey_wire_from(args.from.as_deref(), config, sender_oracle) {
        Ok(from) => from,
        Err(message) => {
            return CliOutput {
                code: 2,
                stdout: String::new(),
                stderr: format!("{command}: {message}\n"),
            }
        }
    };
    let peer_key = match load_peer_key() {
        Ok(key) => key,
        Err(message) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("{command}: {message}\n"),
            }
        }
    };
    let client = match ReqwestHttpTransportIo::new(5_000) {
        Ok(client) => client,
        Err(message) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("{command}: {message}\n"),
            }
        }
    };
    let request = PeerSendRequest {
        peer_url: peer_url.to_owned(),
        target: target.to_owned(),
        text: args.text.clone(),
        inbox: args.inbox,
        from,
        peer_key,
        timestamp: i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX),
    };
    match client.send_peer(&request).await {
        Ok(response) => CliOutput {
            code: 0,
            stdout: format!(
                "{} {}\n",
                response.state.as_deref().unwrap_or("queued"),
                response.target.as_deref().unwrap_or(target)
            ),
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{command}: {message}\n"),
        },
    }
}


async fn run_wake_async_impl(raw_args: &[String]) -> CliOutput {
    if wants_help(raw_args, &["--from", "--task"]) {
        return help_output(wake_peer_usage());
    }
    let wake_args = match parse_wake_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return wake_usage_error(&message),
    };
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle(&config);
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&wake_args.target, &config.route, &sessions) {
        RouteResult::Peer {
            peer_url,
            target,
            node: _,
        } => wake_peer_target(&peer_url, &target, &wake_args, &config, &sender_oracle).await,
        RouteResult::Local { target } | RouteResult::SelfNode { target } => {
            wake_fail_closed_local(&wake_args.target, &target)
        }
        RouteResult::Error { detail, hint, .. } => wake_fail_closed_route_error(&detail, hint.as_deref()),
    }
}

fn wake_fail_closed_local(query: &str, target: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "wake: native local wake is unavailable for '{query}' ({target}); refusing maw-js fallback\n"
        ),
    }
}

fn wake_fail_closed_route_error(detail: &str, hint: Option<&str>) -> CliOutput {
    let suffix = hint.map_or_else(String::new, |hint| format!("; {hint}"));
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("wake: {detail}{suffix}; refusing maw-js fallback\n"),
    }
}

fn parse_wake_args(argv: &[String]) -> Result<WakeArgs, String> {
    let mut from = None;
    let mut task = None;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--from" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("wake: missing --from value".to_owned());
                };
                from = Some(value.clone());
                index += 1;
            }
            value if value.starts_with("--from=") => {
                from = Some(value["--from=".len()..].to_owned());
            }
            "--task" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("wake: missing --task value".to_owned());
                };
                task = Some(value.clone());
                index += 1;
            }
            value if value.starts_with("--task=") => {
                task = Some(value["--task=".len()..].to_owned());
            }
            value if value.starts_with('-') => return Err(format!("wake: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if positional.len() != 1 {
        return Err("wake: target is required".to_owned());
    }
    Ok(WakeArgs {
        target: positional[0].clone(),
        task,
        from,
    })
}

async fn wake_peer_target(
    peer_url: &str,
    target: &str,
    args: &WakeArgs,
    config: &HeyConfig,
    sender_oracle: &str,
) -> CliOutput {
    let from = match resolve_hey_wire_from(args.from.as_deref(), config, sender_oracle) {
        Ok(from) => from,
        Err(message) => {
            return CliOutput {
                code: 2,
                stdout: String::new(),
                stderr: format!("wake: {message}\n"),
            }
        }
    };
    let peer_key = match load_peer_key() {
        Ok(key) => key,
        Err(message) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("wake: {message}\n"),
            }
        }
    };
    let client = match ReqwestHttpTransportIo::new(5_000) {
        Ok(client) => client,
        Err(message) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("wake: {message}\n"),
            }
        }
    };
    let request = PeerWakeRequest {
        peer_url: peer_url.to_owned(),
        target: target.to_owned(),
        task: args.task.clone(),
        from,
        peer_key,
        timestamp: i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX),
    };
    match client.wake_peer(&request).await {
        Ok(response) => CliOutput {
            code: 0,
            stdout: format!("woke {}\n", response.target.as_deref().unwrap_or(target)),
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("wake: {message}\n"),
        },
    }
}

fn resolve_hey_wire_from(
    explicit: Option<&str>,
    config: &HeyConfig,
    sender_oracle: &str,
) -> Result<String, String> {
    if let Some(value) = explicit {
        return validate_wire_from(value);
    }
    if let Ok(value) = std::env::var("MAW_SENDER") {
        return human_sender_to_wire_from(&value);
    }
    let node = config
        .node
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "cannot resolve sender identity; set MAW_SENDER or config node".to_owned())?;
    Ok(format!("{sender_oracle}:{node}"))
}

fn validate_wire_from(value: &str) -> Result<String, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err("wire sender identity must be oracle:node".to_owned());
    }
    Ok(value.to_owned())
}

fn human_sender_to_wire_from(value: &str) -> Result<String, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err("MAW_SENDER must be node:oracle".to_owned());
    }
    Ok(format!("{}:{}", parts[1], parts[0]))
}

fn format_local_hey_message(
    text: &str,
    config: &HeyConfig,
    sender_oracle: &str,
    from: Option<&str>,
) -> String {
    if text.starts_with('/') || text.starts_with('[') {
        return text.to_owned();
    }
    let display = from.map_or_else(
        || {
            let node = config.node.as_deref().unwrap_or("local");
            format!("{node}:{sender_oracle}")
        },
        ToOwned::to_owned,
    );
    format!("[{display}] {text}")
}

fn resolve_hey_sender_oracle(config: &HeyConfig) -> String {
    let session_window = std::env::var("MAW_SESSION_WINDOW").ok();
    if session_window
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return resolve_sender_oracle(session_window.as_deref(), None, config.oracle.as_deref());
    }
    let tmux_window_name = current_tmux_window_name();
    resolve_sender_oracle(None, tmux_window_name.as_deref(), config.oracle.as_deref())
}

fn current_tmux_window_name() -> Option<String> {
    let mut runner = CommandTmuxRunner::new();
    let raw = maw_tmux::TmuxRunner::run(
        &mut runner,
        "display-message",
        &["-p".to_owned(), "#{window_name}".to_owned()],
    )
    .ok()?;
    let window = raw.trim();
    (!window.is_empty()).then(|| window.to_owned())
}

fn route_sessions_from_tmux(
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
) -> Vec<RouteSession> {
    tmux_sessions_to_route_sessions(tmux.list_all())
}

fn load_hey_config() -> HeyConfig {
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    let node = value
        .get("node")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let oracle = value
        .get("oracle")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let peers = value
        .get("peers")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let named_peers = parse_named_peers(value.get("namedPeers"));
    let agents = value
        .get("agents")
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|node| (key.clone(), node.to_owned())))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    HeyConfig {
        node: node.clone(),
        oracle,
        route: RouteConfig {
            node,
            named_peers,
            peers,
            agents,
        },
    }
}

fn parse_named_peers(value: Option<&serde_json::Value>) -> Vec<RouteNamedPeer> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                Some(RouteNamedPeer {
                    name: item.get("name")?.as_str()?.to_owned(),
                    url: item.get("url")?.as_str()?.to_owned(),
                })
            })
            .collect(),
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .filter_map(|(name, value)| {
                value.as_str().map(|url| RouteNamedPeer {
                    name: name.clone(),
                    url: url.to_owned(),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn load_peer_key() -> Result<String, String> {
    if let Ok(value) = std::env::var("MAW_PEER_KEY") {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    let env = real_xdg_env();
    let path = maw_state_path(&env, &["peer-key"]);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let key = raw.trim().to_owned();
        if !key.is_empty() {
            return Ok(key);
        }
    }
    let key = generate_peer_key()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create peer-key directory: {error}"))?;
    }
    write_peer_key_file(&path, &key)?;
    Ok(key)
}

fn generate_peer_key() -> Result<String, String> {
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|error| format!("failed to open random peer key source: {error}"))?;
    let mut bytes = [0_u8; 32];
    std::io::Read::read_exact(&mut file, &mut bytes)
        .map_err(|error| format!("failed to read random peer key bytes: {error}"))?;
    Ok(hex_bytes(&bytes))
}

fn write_peer_key_file(path: &std::path::Path, key: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, key.as_bytes())
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, b"\n")
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, format!("{key}\n"))
            .map_err(|error| format!("failed to write peer-key: {error}"))
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn real_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "MAW_TEST_MODE",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok().map(|value| (name.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

#[derive(Debug, Clone, Default)]
struct LocalserverCliRequest {
    method: String,
    path: String,
    body: Option<String>,
}

fn run_health_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_health_async_impl(&args).await })
}

fn run_messages_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_messages_async_impl(&args).await })
}

fn run_reply_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_reply_async_impl(&args).await })
}

async fn run_health_async_impl(raw_args: &[String]) -> CliOutput {
    if !raw_args.is_empty() {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "usage: maw-rs health\n".to_owned(),
        };
    }
    let mut lines = vec!["\nmaw health\n".to_owned()];
    let sessions = TmuxClient::local().list_all();
    lines.push(format!(
        "  \u{1b}[32m●\u{1b}[0m tmux server        running ({} sessions)",
        sessions.len()
    ));
    match localserver_request(LocalserverCliRequest {
        method: "POST".to_owned(),
        path: "/api/probe".to_owned(),
        body: Some("{}".to_owned()),
    })
    .await
    {
        Ok(resp) if resp.status < 400 => lines.push(format!(
            "  \u{1b}[32m●\u{1b}[0m maw server         online (:{}, probe ok)",
            localserver_port_label()
        )),
        Ok(resp) => lines.push(format!(
            "  \u{1b}[33m●\u{1b}[0m maw server         HTTP {} (probe)",
            resp.status
        )),
        Err(_) => lines.push("  \u{1b}[31m●\u{1b}[0m maw server         offline".to_owned()),
    }
    lines.push(String::new());
    CliOutput {
        code: 0,
        stdout: format!("{}\n", lines.join("\n")),
        stderr: String::new(),
    }
}

async fn run_messages_async_impl(raw_args: &[String]) -> CliOutput {
    if let Some(output) = messages_lifecycle_subcommand152(raw_args) { return output; }
    let mut path = "/api/message-ledger".to_owned();
    let mut passthrough = Vec::<String>::new();
    let mut index = 0;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--limit" | "--from" | "--to" | "--direction" | "--state" | "--q" => {
                let Some(value) = raw_args.get(index + 1) else {
                    return messages_usage_error(&format!("messages: missing {} value", raw_args[index]));
                };
                passthrough.push(format!("{}={}", raw_args[index].trim_start_matches("--"), percent_encode_query(value)));
                index += 1;
            }
            "--json" => passthrough.push("json=1".to_owned()),
            value if value.starts_with('-') => return messages_usage_error(&format!("messages: unknown argument {value}")),
            value => return messages_usage_error(&format!("messages: unexpected argument {value}")),
        }
        index += 1;
    }
    if !passthrough.is_empty() {
        path.push('?');
        path.push_str(&passthrough.join("&"));
    }
    match localserver_request(LocalserverCliRequest {
        method: "GET".to_owned(),
        path,
        body: None,
    })
    .await
    {
        Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: ensure_trailing_newline(resp.body), stderr: String::new() },
        Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("messages: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("messages: {message}\n") },
    }
}

fn messages_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs messages [serve [--detach] [--engine URL] [--port N] | status [--engine URL] | stop [--engine URL] | --limit N --from ID --to ID --direction outbound|inbound|forwarded --state queued|delivered|failed --q text --json]\n"),
    }
}

async fn run_reply_async_impl(raw_args: &[String]) -> CliOutput {
    if raw_args.first().is_some_and(|arg| arg == "--list" || arg == "-l") {
        let mut path = "/api/requests?status=delivered".to_owned();
        if let Some(oracle) = raw_args.get(1) {
            path.push_str("&oracle=");
            path.push_str(&percent_encode_query(oracle));
        }
        return match localserver_request(LocalserverCliRequest { method: "GET".to_owned(), path, body: None }).await {
            Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: format_reply_list(&resp.body), stderr: String::new() },
            Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
            Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: {message}\n") },
        };
    }
    if raw_args.len() < 2 {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "usage: maw-rs reply <correlationId> <message>\n       maw-rs reply --list [oracle]\n".to_owned(),
        };
    }
    let correlation_id = &raw_args[0];
    let reply = raw_args[1..].join(" ");
    let body = serde_json::json!({ "reply": reply }).to_string();
    let path = format!("/api/reply/{}", percent_encode_path(correlation_id));
    match localserver_request(LocalserverCliRequest { method: "POST".to_owned(), path, body: Some(body) }).await {
        Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: format!("\u{1b}[32mreplied\u{1b}[0m → {correlation_id}\n"), stderr: String::new() },
        Ok(resp) if resp.body.contains("already replied") => CliOutput { code: 0, stdout: String::new(), stderr: format!("\u{1b}[33mwarn\u{1b}[0m: request '{correlation_id}' already replied\n") },
        Ok(resp) if resp.body.contains("request not found") => CliOutput { code: 1, stdout: String::new(), stderr: format!("\u{1b}[31merror\u{1b}[0m: request '{correlation_id}' not found\n") },
        Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: {message}\n") },
    }
}

async fn localserver_request(request: LocalserverCliRequest) -> Result<maw_transport::HttpResponse, String> {
    let base = resolve_localserver_base_url();
    let url = format!("{}{}", base.trim_end_matches('/'), request.path);
    let client = ReqwestHttpTransportIo::new(5_000)?;
    client.request(&TransportHttpRequest {
        method: request.method,
        url,
        headers: BTreeMap::new(),
        body: request.body,
        timeout_ms: Some(5_000),
        follow_redirects: false,
        pinned_addr: None,
    }).await
}

fn resolve_localserver_base_url() -> String {
    if let Ok(url) = std::env::var("MAW_LOCALSERVER_URL").or_else(|_| std::env::var("MAW_ENGINE_URL")) {
        return url.trim_end_matches('/').to_owned();
    }
    let port = load_hey_config_port().unwrap_or_else(|| std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(31_745));
    format!("http://127.0.0.1:{port}")
}

fn localserver_port_label() -> String {
    resolve_localserver_base_url().rsplit(':').next().unwrap_or("?").to_owned()
}

fn load_hey_config_port() -> Option<u16> {
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    value.get("port").and_then(|port| port.as_u64().and_then(|n| u16::try_from(n).ok()).or_else(|| port.as_str()?.parse::<u16>().ok()))
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

fn percent_encode_query(value: &str) -> String {
    percent_encode(value, false)
}

fn percent_encode_path(value: &str) -> String {
    percent_encode(value, true)
}

fn percent_encode(value: &str, slash: bool) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let ok = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') || (slash && byte == b'/');
        if ok {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn format_reply_list(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return ensure_trailing_newline(body.to_owned());
    };
    let Some(requests) = value.get("requests").and_then(serde_json::Value::as_array) else {
        return ensure_trailing_newline(body.to_owned());
    };
    if requests.is_empty() {
        return "no pending requests\n".to_owned();
    }
    let mut lines = Vec::new();
    for request in requests {
        let id = request.get("correlationId").and_then(serde_json::Value::as_str).unwrap_or("?");
        let from = request.get("from").and_then(serde_json::Value::as_str).unwrap_or("?");
        let message = request.get("message").and_then(serde_json::Value::as_str).unwrap_or("");
        lines.push(format!("  \u{1b}[36m{id}\u{1b}[0m from \u{1b}[33m{from}\u{1b}[0m → {message}"));
    }
    let total = value.get("total").and_then(serde_json::Value::as_u64).unwrap_or(requests.len() as u64);
    lines.push(String::new());
    lines.push(format!("{total} pending request(s)"));
    ensure_trailing_newline(lines.join("\n"))
}

#[cfg(test)]
mod send_acl_hotpath_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct SendFakeTmuxRunner {
        current_session: Option<Result<String, String>>,
        calls: Vec<(String, Vec<String>)>,
    }

    impl maw_tmux::TmuxRunner for SendFakeTmuxRunner {
        fn run(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" => self
                    .current_session
                    .clone()
                    .unwrap_or_else(|| Ok(String::new()))
                    .map_err(maw_tmux::TmuxError::new),
                other => Err(maw_tmux::TmuxError::new(format!(
                    "unexpected tmux command {other}"
                ))),
            }
        }
    }

    struct SendAclEnvGuard {
        _home: EnvVarRestore,
        _maw_home: EnvVarRestore,
        _config: EnvVarRestore,
        _state: EnvVarRestore,
        _bypass: EnvVarRestore,
        root: std::path::PathBuf,
    }

    impl SendAclEnvGuard {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let root = std::env::temp_dir().join(format!("maw-send-acl-{name}-{}-{nanos}", std::process::id()));
            let _ = std::fs::create_dir_all(root.join("home"));
            let _ = std::fs::create_dir_all(root.join("config"));
            let _ = std::fs::create_dir_all(root.join("state"));
            let guard = Self {
                _home: EnvVarRestore::capture("HOME"),
                _maw_home: EnvVarRestore::capture("MAW_HOME"),
                _config: EnvVarRestore::capture("MAW_CONFIG_DIR"),
                _state: EnvVarRestore::capture("MAW_STATE_DIR"),
                _bypass: EnvVarRestore::capture("MAW_ACL_BYPASS"),
                root: root.clone(),
            };
            std::env::set_var("HOME", root.join("home"));
            std::env::remove_var("MAW_HOME");
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            std::env::set_var("MAW_STATE_DIR", root.join("state"));
            std::env::remove_var("MAW_ACL_BYPASS");
            guard
        }
    }

    fn send_acl_config(oracle: &str) -> HeyConfig {
        HeyConfig { node: Some("node-a".to_owned()), oracle: Some(oracle.to_owned()), route: RouteConfig::default() }
    }

    fn send_acl_args(target: &str, text: &str) -> SendArgs {
        SendArgs { target: target.to_owned(), text: text.to_owned(), inbox: None, from: None, approve: false, trust: false, dry_run: false }
    }

    fn send_route_window(index: u32, name: &str) -> RouteWindow {
        RouteWindow {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: None,
        }
    }

    fn send_route_session(name: &str, windows: Vec<RouteWindow>) -> RouteSession {
        RouteSession {
            name: name.to_owned(),
            windows,
            source: None,
        }
    }

    #[test]
    fn send_self_alias_uses_current_tmux_session_from_runner() {
        let sessions = vec![send_route_session(
            "188-maw-rs",
            vec![
                send_route_window(0, "work"),
                send_route_window(1, "maw-rs-oracle"),
            ],
        )];
        let mut runner = SendFakeTmuxRunner {
            current_session: Some(Ok("188-maw-rs\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };

        assert_eq!(
            resolve_send_route_target(
                "me",
                &RouteConfig::default(),
                &sessions,
                true,
                &mut runner
            ),
            RouteResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
        assert_eq!(
            runner.calls,
            vec![(
                "display-message".to_owned(),
                vec!["-p".to_owned(), "#{session_name}".to_owned()]
            )]
        );
    }

    #[test]
    fn send_self_alias_outside_tmux_does_not_match_literal_me_window() {
        let sessions = vec![send_route_session(
            "scratch",
            vec![send_route_window(0, "me"), send_route_window(1, "shell")],
        )];
        let mut runner = SendFakeTmuxRunner::default();

        let result = resolve_send_route_target(
            "me",
            &RouteConfig::default(),
            &sessions,
            false,
            &mut runner,
        );

        assert!(matches!(
            result,
            RouteResult::Error { reason, .. } if reason == "me_needs_tmux"
        ));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn send_dry_run_parser_and_output_include_resolved_target() {
        let args = parse_send_args(
            "hey",
            &send_acl_vec(&["me", "--dry-run", "test"]),
        )
        .expect("parse");
        assert!(args.dry_run);
        assert_eq!(args.target, "me");
        assert_eq!(args.text, "test");

        let output = send_dry_run_output(
            "hey",
            &args,
            &RouteResult::Local {
                target: "188-maw-rs:1".to_owned(),
            },
        );
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "dry-run: hey me -> local 188-maw-rs:1\n");
    }

    #[test]
    fn hey_help_prints_usage_to_stdout_zero() {
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_send_like_async_impl("hey", &send_acl_vec(&["--help"])));

        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("usage: maw-rs hey <target> <message>"));
        assert!(output.stderr.is_empty());
        assert!(!wants_help_before_positionals(&send_acl_vec(&["bob", "hello", "--help"]), &["--from"]));
    }

    fn send_acl_write_scope(name: &str, members: &[&str]) {
        let dir = scope_native_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ScopeNativeRecord { name: name.to_owned(), members: members.iter().map(|member| (*member).to_owned()).collect(), lead: None, created: "2026-06-26T00:00:00.000Z".to_owned(), ttl: None };
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(&scope).unwrap()).unwrap();
    }

    fn send_acl_assert_proceed(result: SendAclGateResult) -> String {
        match result {
            SendAclGateResult::Proceed { stderr_prefix } => stderr_prefix,
            other => panic!("expected proceed, got {other:?}"),
        }
    }

    #[test]
    fn send_identity_uses_invocation_oracle_for_wire_and_local_tags() {
        let _lock = env_test_lock().lock().unwrap();
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("MAW_SENDER");
        let config = HeyConfig {
            node: Some("m5".to_owned()),
            oracle: Some("configured".to_owned()),
            route: RouteConfig::default(),
        };

        assert_eq!(
            resolve_hey_wire_from(None, &config, "maw-rs").expect("wire from"),
            "maw-rs:m5"
        );
        assert_eq!(
            format_local_hey_message("hello", &config, "maw-rs", None),
            "[m5:maw-rs] hello"
        );
        assert_eq!(
            format_local_hey_message("[pretagged] hello", &config, "maw-rs", None),
            "[pretagged] hello"
        );
    }

    #[test]
    fn send_acl_no_scope_same_scope_and_trusted_allow_peer_send() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("allow");
        let config = send_acl_config("alice");
        let sender = config.oracle.as_deref().expect("test oracle");
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );

        send_acl_write_scope("team", &["alice", "bob"]);
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );

        std::fs::remove_file(scope_native_path("team")).unwrap();
        scope_trust_add_to_path(&scope_trust_path(), "alice", "bob", "2026-06-26T00:00:00.000Z").unwrap();
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );
    }

    #[test]
    fn send_acl_cross_scope_queues_without_body_or_peer_key() {
        let _lock = env_test_lock().lock().unwrap();
        let env = SendAclEnvGuard::new("queue");
        send_acl_write_scope("team", &["alice", "carol"]);
        let args = send_acl_args("remote-bob", "SECRET_BODY token=abc123");
        let result = send_acl_gate_peer("hey", "bob", &args, "alice", false);
        let output = match result { SendAclGateResult::Queued(output) => output, other => panic!("expected queue, got {other:?}") };
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(output.stdout.contains("sender: alice"));
        assert!(output.stdout.contains("target: bob"));
        assert!(output.stdout.contains("maw inbox approve"));
        assert!(!output.stdout.contains("SECRET_BODY"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!env.root.join("state").join("peer-key").exists());
        let pending_dir = env.root.join("state").join("pending");
        let files = std::fs::read_dir(pending_dir).unwrap().collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn send_acl_approve_bypass_and_human_only_trust_rules() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("approve");
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");

        let sender = config.oracle.as_deref().expect("test oracle");
        let mut approve = send_acl_args("remote-bob", "hello");
        approve.approve = true;
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, sender, false)),
            ""
        );
        assert!(!scope_trust_path().exists());

        approve.trust = true;
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, sender, false)),
            ""
        );
        let trusted = scope_trust_load_from_path(&scope_trust_path());
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].sender, "alice");
        assert_eq!(trusted[0].target, "bob");

        let err = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--trust"])).unwrap_err();
        assert!(err.contains("--trust requires --approve"));
    }

    #[test]
    fn send_acl_env_bypass_is_ignored_and_explicit_param_writes_no_trust() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("bypass");
        send_acl_write_scope("team", &["alice", "carol"]);
        std::env::set_var("MAW_ACL_BYPASS", "1");
        let queued = send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false);
        assert!(
            matches!(queued, SendAclGateResult::Queued(_)),
            "env must not bypass ACL"
        );
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", true)), "");
        assert!(!scope_trust_path().exists());
        assert_eq!(std::env::var("MAW_ACL_BYPASS").as_deref(), Ok("1"));
    }

    #[test]
    fn send_acl_corrupt_acl_fails_open_with_loud_warning() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("corrupt");
        let dir = scope_native_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.json"), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("broken.json"));
        assert!(stderr.contains("fix"));

        std::fs::remove_file(dir.join("broken.json")).unwrap();
        std::fs::write(scope_trust_path(), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("scope-trust.json"));
    }

    #[test]
    fn send_acl_parser_accepts_approve_and_rejects_trust_alone() {
        let parsed = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--approve", "--trust"])).unwrap();
        assert!(parsed.approve);
        assert!(parsed.trust);
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.code, 2);
        assert!(output.stderr.contains("[--approve] [--trust]"));
    }

    #[test]
    fn inbox_hey_send_args_keep_message_flags_opaque() {
        let args = send_args_for_inbox_hey(
            "bob",
            "hello --approve --from=mallory:edge --trust -leading",
        );

        assert_eq!(args.target, "bob");
        assert_eq!(
            args.text,
            "hello --approve --from=mallory:edge --trust -leading"
        );
        assert_eq!(args.inbox, None);
        assert_eq!(args.from, None);
        assert!(!args.approve);
        assert!(!args.trust);
    }


    #[test]
    fn send_acl_notify_cross_scope_queues_before_peer_transport() {
        let _lock = env_test_lock().lock().unwrap();
        let env = SendAclEnvGuard::new("notify-callsite");
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");
        let args = NotifyArgs {
            target: "remote-bob".to_owned(),
            text: "SECRET_NOTIFY token=abc123".to_owned(),
            from: None,
            approve: false,
            trust: false,
            force: false,
        };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(notify_peer(
                "http://127.0.0.1:1",
                "bob",
                &args,
                &config,
                "alice",
            ));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(!output.stdout.contains("SECRET_NOTIFY"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!env.root.join("state").join("peer-key").exists());
        assert_eq!(std::fs::read_dir(env.root.join("state").join("pending")).unwrap().count(), 1);
    }

    #[test]
    fn send_acl_talkto_cross_scope_queues_before_fake_or_real_transport() {
        let _lock = env_test_lock().lock().unwrap();
        let env = SendAclEnvGuard::new("talkto-callsite");
        let _fake = EnvVarRestore::capture("MAW_RS_TALKTO_FAKE_PEER_LOG");
        let fake_log = env.root.join("talkto-peer.jsonl");
        std::env::set_var("MAW_RS_TALKTO_FAKE_PEER_LOG", &fake_log);
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");
        let args = TalktoArgs { recipient: "remote-bob".to_owned(), message: "SECRET_TALK token=abc123".to_owned(), force: false };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(talkto_peer("http://127.0.0.1:1", "bob", Some("remote"), &args, "SECRET_TALK token=abc123", &config, None));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(!output.stdout.contains("SECRET_TALK"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!fake_log.exists(), "ACL queue must happen before fake/real peer transport");
        assert!(!env.root.join("state").join("peer-key").exists());
        assert_eq!(std::fs::read_dir(env.root.join("state").join("pending")).unwrap().count(), 1);
    }

    #[test]
    fn send_acl_queue_and_usage_match_committed_goldens() {
        assert_eq!(
            send_acl_format_queue_output("2026-06-26T00-00-00-000Z-a1b2c3", "alice", "bob"),
            include_str!("../../tests/fixtures/native-scope-acl/acl-queue.stdout")
        );
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.stderr, include_str!("../../tests/fixtures/native-scope-acl/send-usage.stderr"));
    }

    fn send_acl_vec(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
}
