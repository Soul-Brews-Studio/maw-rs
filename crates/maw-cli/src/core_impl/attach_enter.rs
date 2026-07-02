fn run_attach_plan(argv: &[String]) -> CliOutput {
    let mut print = false;
    let mut readonly = false;
    let mut plan_json = false;
    let mut yes = false;
    let mut ssh_alias: Option<String> = None;
    let mut alive = BTreeSet::new();
    let mut target: Option<String> = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return attach_usage_ok(),
            "--print" => print = true,
            "--readonly" | "--read-only" | "-r" => readonly = true,
            "--plan-json" | "--dry-run" => plan_json = true,
            "--yes" | "-y" => yes = true,
            "--ssh-alias" => {
                let Some(value) = argv.get(index + 1) else {
                    return attach_usage_error("attach: missing --ssh-alias value");
                };
                ssh_alias = Some(value.to_owned());
                index += 1;
            }
            "--alive" => {
                let Some(value) = argv.get(index + 1) else {
                    return attach_usage_error("attach: missing --alive value");
                };
                alive.insert(value.to_owned());
                index += 1;
            }
            arg if arg.starts_with("--alive=") => {
                alive.insert(arg["--alive=".len()..].to_owned());
            }
            arg if arg.starts_with("--ssh-alias=") => {
                ssh_alias = Some(arg["--ssh-alias=".len()..].to_owned());
            }
            arg if arg.starts_with('-') => {
                return attach_usage_error(&format!("attach: unknown argument {arg}"));
            }
            value => {
                if target.is_some() {
                    return attach_usage_error("attach: target already provided");
                }
                target = Some(value.to_owned());
            }
        }
        index += 1;
    }

    let Some(target) = target else {
        return attach_usage_error("attach: target required");
    };
    if let Some((node, session_name)) = parse_explicit_remote_attach_target(&target) {
        let alias = ssh_alias.unwrap_or_else(|| node.clone());
        let stdout = if plan_json {
            render_attach_remote_plan_json(&target, &node, &session_name, &alias, yes)
        } else {
            render_attach_remote_plan_text(&target, &node, &session_name, &alias, yes)
        };
        return CliOutput { code: 0, stdout, stderr: String::new() };
    }
    if alive.is_empty() {
        let mut client = TmuxClient::local();
        alive = client.list_session_names().into_iter().collect();
    }
    let resolved_target = match resolve_tmux_attach_session(&target, &alive) {
        TmuxAttachSessionResolution::Match { session }
        | TmuxAttachSessionResolution::Missing { session } => session,
        TmuxAttachSessionResolution::Ambiguous { candidates, .. } => {
            return attach_ambiguous_error(&target, &candidates);
        }
    };
    let in_tmux = std::env::var_os("TMUX").is_some();
    let action = decide_tmux_attach_action(&resolved_target, &alive, print || plan_json, false, in_tmux);
    let session = attach_action_session(&action);
    let stdout = if plan_json {
        render_attach_plan_json(&target, session, &action, readonly)
    } else {
        render_attach_plan_text(&target, session, &action, readonly)
    };
    let code = i32::from(matches!(action, TmuxAttachAction::Recover { .. }));
    CliOutput {
        code,
        stdout,
        stderr: String::new(),
    }
}

fn attach_ambiguous_error(target: &str, candidates: &[String]) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "attach: '{target}' matches multiple sessions: {}\n  use the full name: maw-rs attach <exact-session>\n",
            candidates.join(", ")
        ),
    }
}

fn attach_usage_ok() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: attach_usage_text(),
        stderr: String::new(),
    }
}

fn attach_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}", attach_usage_text()),
    }
}

fn attach_usage_text() -> String {
    "usage: maw-rs attach <target> [--print] [--readonly|-r]\n       maw-rs a <target> [--print] [--readonly|-r]\n".to_owned()
}


fn parse_explicit_remote_attach_target(target: &str) -> Option<(String, String)> {
    let (node, session_name) = target.split_once(':')?;
    let node = node.trim();
    let session_name = session_name.trim();
    if node.is_empty() || session_name.is_empty() {
        return None;
    }
    if session_name
        .split_once('.')
        .map_or_else(|| session_name.chars().all(|c| c.is_ascii_digit()), |(window, pane)| {
            window.chars().all(|c| c.is_ascii_digit()) && pane.chars().all(|c| c.is_ascii_digit())
        })
    {
        return None;
    }
    Some((node.to_owned(), session_name.to_owned()))
}

fn render_attach_remote_plan_text(
    target: &str,
    node: &str,
    session_name: &str,
    ssh_alias: &str,
    yes: bool,
) -> String {
    let yes_suffix = if yes { " -y" } else { "" };
    format!(
        "  \x1b[36m·\x1b[0m [dry-run] Tier 3 (remote) — would attach to {node}:{session_name} via ssh {ssh_alias}
  command: maw-rs attach-ssh --node {node} --session {session_name} --ssh-alias {ssh_alias}{yes_suffix}
  resolved: {target} → {node}:{session_name}
"
    )
}

fn render_attach_remote_plan_json(
    target: &str,
    node: &str,
    session_name: &str,
    ssh_alias: &str,
    yes: bool,
) -> String {
    let attach_ssh_args = vec![
        "--node".to_owned(),
        node.to_owned(),
        "--session".to_owned(),
        session_name.to_owned(),
        "--ssh-alias".to_owned(),
        ssh_alias.to_owned(),
    ];
    format!(
        "{{\"command\":\"attach\",\"alias\":\"a\",\"target\":{},\"action\":\"remote-attach\",\"tier\":3,\"node\":{},\"sessionName\":{},\"sshAlias\":{},\"yes\":{},\"attachSshArgs\":{}}}\n",
        json_string(target),
        json_string(node),
        json_string(session_name),
        json_string(ssh_alias),
        yes,
        json_string_array(&attach_ssh_args)
    )
}

fn render_attach_plan_text(
    target: &str,
    session: &str,
    action: &TmuxAttachAction,
    readonly: bool,
) -> String {
    match action {
        TmuxAttachAction::Recover { .. } => format!(
            "attach: '{target}' resolved to missing session {session}\n  → maw wake {target} --attach\n"
        ),
        TmuxAttachAction::Print { .. }
        | TmuxAttachAction::SwitchClient { .. }
        | TmuxAttachAction::Attach { .. } => {
            let args = attach_command_args(action, readonly);
            format!(
                "Run: tmux {}\n  resolved: {target} → {session}\n  detach with: Ctrl-b d\n",
                args.join(" ")
            )
        }
    }
}

fn render_attach_plan_json(
    target: &str,
    session: &str,
    action: &TmuxAttachAction,
    readonly: bool,
) -> String {
    let kind = match action {
        TmuxAttachAction::Print { .. } => "print",
        TmuxAttachAction::SwitchClient { .. } => "switch-client",
        TmuxAttachAction::Attach { .. } => "attach",
        TmuxAttachAction::Recover { .. } => "recover",
    };
    let args = attach_command_args(action, readonly);
    format!(
        "{{\"command\":\"attach\",\"alias\":\"a\",\"target\":{},\"session\":{},\"action\":{},\"tmuxArgs\":{}}}\n",
        json_string(target),
        json_string(session),
        json_string(kind),
        json_string_array(&args)
    )
}

fn attach_command_args(action: &TmuxAttachAction, readonly: bool) -> Vec<String> {
    if readonly {
        return vec![
            "attach".to_owned(),
            "-r".to_owned(),
            "-t".to_owned(),
            attach_action_session(action).to_owned(),
        ];
    }
    tmux_attach_spawn_command(action).map_or_else(
        || vec!["attach".to_owned(), "-t".to_owned(), attach_action_session(action).to_owned()],
        |command| command.args,
    )
}

fn attach_action_session(action: &TmuxAttachAction) -> &str {
    match action {
        TmuxAttachAction::Print { session }
        | TmuxAttachAction::SwitchClient { session }
        | TmuxAttachAction::Attach { session }
        | TmuxAttachAction::Recover { session } => session,
    }
}


fn run_send_enter_command(argv: &[String]) -> CliOutput {
    let (target, count) = match parse_send_enter_command_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return send_enter_usage_error(&message),
    };
    let mut client = TmuxClient::local();
    let resolved = match resolve_local_tmux_command_target(&mut client, &target) {
        Ok(target) => target,
        Err(message) => return command_target_error("send-enter", &message),
    };
    for _ in 0..count {
        if let Err(error) = client.send_enter(&resolved) {
            return command_target_error(
                "send-enter",
                &format!("tmux send-keys failed: {error}"),
            );
        }
    }
    let plural = if count == 1 {
        "Enter".to_owned()
    } else {
        format!("{count} Enters")
    };
    CliOutput {
        code: 0,
        stdout: format!("\x1b[32mdelivered\x1b[0m → {resolved}: {plural}\n"),
        stderr: String::new(),
    }
}

fn parse_send_enter_command_args(argv: &[String]) -> Result<(String, usize), String> {
    let mut target = None;
    let mut count = 1usize;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if matches!(arg.as_str(), "--N" | "-N" | "--n") {
            let Some(next) = argv.get(index + 1) else {
                return Err("--N requires a positive integer (got: nothing)".to_owned());
            };
            count = parse_send_enter_count(next, next)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg
            .strip_prefix("--N=")
            .or_else(|| arg.strip_prefix("--n="))
        {
            count = parse_send_enter_count(value, arg)?;
            index += 1;
            continue;
        }
        if target.is_none() && !arg.starts_with('-') {
            target = Some(arg.clone());
        }
        index += 1;
    }
    let Some(target) = target else {
        return Err("usage: maw-rs send-enter <target> [--N <count>]".to_owned());
    };
    Ok((target, count))
}

fn parse_send_enter_count(raw: &str, label: &str) -> Result<usize, String> {
    match raw.parse::<usize>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(format!("--N requires a positive integer (got: {label})")),
    }
}

fn resolve_local_tmux_command_target(
    client: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
    query: &str,
) -> Result<String, String> {
    if query.starts_with('%') {
        return Ok(query.to_owned());
    }
    let sessions = client
        .list_all()
        .into_iter()
        .map(|session| RouteSession {
            name: session.name,
            windows: session
                .windows
                .into_iter()
                .map(|window| RouteWindow {
                    index: window.index,
                    name: window.name,
                    active: window.active,
                })
                .collect(),
            source: None,
        })
        .collect::<Vec<_>>();
    match resolve_route_target(query, &RouteConfig::default(), &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => Ok(target),
        RouteResult::Peer { node, target, .. } => Err(format!(
            "cross-node target '{query}' (node '{node}', target '{target}') is not supported"
        )),
        RouteResult::Error { detail, hint, .. } => {
            if let Some(hint) = hint {
                Err(format!("{detail} — {hint}"))
            } else {
                Err(detail)
            }
        }
    }
}

fn command_target_error(command: &str, message: &str) -> CliOutput {
    CliOutput {
        code: 1,
        stdout: String::new(),
        stderr: format!("{command}: {message}\n"),
    }
}

fn send_enter_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs send-enter <target> [--N <count>]\n"),
    }
}

