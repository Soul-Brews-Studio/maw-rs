async fn send_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
) -> CliOutput {
    let from = match resolve_hey_wire_from(args.from.as_deref(), config) {
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
            stderr: format!(
                "{command}: {message}{}\n",
                hey_pairing_diagnostic(command, peer_url, &request.from, &message)
            ),
        },
    }
}

fn hey_pairing_diagnostic(command: &str, peer_url: &str, from: &str, error: &str) -> String {
    if !error.contains("HTTP 401") {
        return String::new();
    }
    if error.contains("refuse-missing-peer-key") || error.contains("pin-missing") {
        let node = hey_node_from_wire_from(from).unwrap_or("-");
        return format!(
            "\n\n{command}: auth diagnostic: peer pairing is required and still fail-closed\n  missing from: {from}\n  missing node: {node}\n  peer key: not paired (redacted)\n  remote peer: {peer_url}\n  pair this lane:\n    1. On the remote peer, mint a one-time code:\n       maw pair generate --at {peer_url}\n    2. On this node, replace <PAIR-CODE> with that code:\n       maw pair {peer_url} <PAIR-CODE>\n  note: no secret key values are printed."
        );
    }
    if error.contains("refuse-ambiguous-peer-key") {
        let node = hey_node_from_wire_from(from).unwrap_or("-");
        return format!(
            "\n\n{command}: auth diagnostic: peer pairing failed closed because multiple cached peer keys match this node\n  from: {from}\n  node: {node}\n  peer key: ambiguous (redacted)\n  remote peer: {peer_url}\n  action: verify the peer identity, clear stale pins on the receiver, then re-run:\n       maw pair generate --at {peer_url}\n       maw pair {peer_url} <PAIR-CODE>\n  note: no secret key values are printed."
        );
    }
    if error.contains("refuse-mismatch") {
        let node = hey_node_from_wire_from(from).unwrap_or("-");
        return format!(
            "\n\n{command}: auth diagnostic: peer pairing failed closed because the cached peer key did not verify this signature\n  from: {from}\n  node: {node}\n  peer key: mismatch (redacted)\n  remote peer: {peer_url}\n  action: verify you are contacting the intended peer before forgetting/re-pairing.\n  note: no secret key values are printed."
        );
    }
    String::new()
}

fn hey_node_from_wire_from(from: &str) -> Option<&str> {
    let (_, node) = from.trim().split_once(':')?;
    let node = node.trim();
    (!node.is_empty()).then_some(node)
}


async fn run_wake_async_impl(raw_args: &[String]) -> CliOutput {
    let wake_args = match parse_wake_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return wake_usage_error(&message),
    };
    let config = load_hey_config();
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&wake_args.target, &config.route, &sessions) {
        RouteResult::Peer {
            peer_url,
            target,
            node: _,
        } => wake_peer_target(&peer_url, &target, &wake_args, &config).await,
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

