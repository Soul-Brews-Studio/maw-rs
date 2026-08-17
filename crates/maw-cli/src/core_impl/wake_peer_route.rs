// Waking a window that lives on another node.
//
// `maw wake host:target` cannot run the local pipeline -- it has to become a
// signed request to that host's serve. Kept apart from the local wake so the two
// paths are not read as one: this one fails closed, because a cross-node wake
// that silently degrades to a local one would start an agent on the wrong box.

#[derive(Debug, Clone, Default)]
struct WakeArgs {
    target: String,
    task: Option<String>,
    from: Option<String>,
}

fn run_wake_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_wake_async_impl(&args).await })
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
    let mut tmux = TmuxClient::local();
    let sessions = match route_sessions_from_tmux(&mut tmux) {
        Ok(sessions) => sessions,
        Err(message) => {
            return CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("wake: {message}\n"),
            }
        }
    };
    match resolve_route_target(&wake_args.target, &config.route, &sessions) {
        RouteResult::Peer {
            peer_url,
            target,
            node: _,
        } => {
            let sender_oracle = resolve_hey_sender_oracle_for_from(&config, wake_args.from.as_deref());
            wake_peer_target(&peer_url, &target, &wake_args, &config, &sender_oracle).await
        }
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
    let federation_token = match load_federation_token() {
        Ok(token) => token,
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
        federation_token,
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
