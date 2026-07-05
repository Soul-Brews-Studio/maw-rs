#[derive(Debug, Clone, Default)]
struct SendArgs {
    target: String,
    text: String,
    inbox: Option<bool>,
    from: Option<String>,
    approve: bool,
    trust: bool,
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
    }
}

async fn run_send_like_async_with_args(
    command: &str,
    send_args: SendArgs,
    acl_bypass: bool,
) -> CliOutput {
    let config = load_hey_config();
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&send_args.target, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => send_local_message(
            command,
            &mut tmux,
            &target,
            &send_args.text,
            &config,
            send_args.from.as_deref(),
        ),
        RouteResult::Peer {
            peer_url,
            target,
            node: _,
        } => gated_send_peer_message(command, &peer_url, &target, &send_args, &config, acl_bypass).await,
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
    acl_bypass: bool,
) -> CliOutput {
    match send_acl_gate_peer(command, target, args, config, acl_bypass) {
        SendAclGateResult::Proceed { stderr_prefix } => send_acl_deliver_peer_message(command, peer_url, target, args, config, stderr_prefix).await,
        SendAclGateResult::Queued(output) | SendAclGateResult::Reject(output) => output,
    }
}

async fn send_acl_deliver_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    stderr_prefix: String,
) -> CliOutput {
    send_acl_apply_proceed_stderr(send_peer_message(command, peer_url, target, args, config).await, &stderr_prefix)
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
    config: &HeyConfig,
    acl_bypass: bool,
) -> SendAclGateResult {
    if args.trust && !args.approve {
        return SendAclGateResult::Reject(CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{command}: --trust requires --approve\n"),
        });
    }
    let sender = match send_acl_sender(args, config) {
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

fn send_acl_sender(args: &SendArgs, config: &HeyConfig) -> Result<String, String> {
    if let Some(explicit) = args.from.as_deref() {
        let wire = validate_wire_from(explicit)?;
        return send_acl_oracle_component(&wire);
    }
    send_acl_validate_actor(config.oracle.as_deref().unwrap_or(DEFAULT_ORACLE))
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

