const DISPATCH_111: &[DispatcherEntry] = &[
    DispatcherEntry { command: "attach", handler: Handler::Sync(attach_run_command) },
    DispatcherEntry { command: "a", handler: Handler::Sync(attach_run_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttachOptions {
    flags: u8,
    ssh_alias: Option<String>,
    alive: BTreeSet<String>,
    target: String,
}

const ATTACH_FLAG_PRINT: u8 = 1 << 0;
const ATTACH_FLAG_READONLY: u8 = 1 << 1;
const ATTACH_FLAG_PLAN_JSON: u8 = 1 << 2;
const ATTACH_FLAG_YES: u8 = 1 << 3;

fn attach_run_command(argv: &[String]) -> CliOutput {
    match attach_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) | Err(output) => output,
    }
}

fn attach_run_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, CliOutput> {
    let mut opts = attach_parse_args(argv).map_err(|message| {
        if message == attach_port_usage_text() {
            attach_port_usage_ok()
        } else {
            attach_port_usage_error(&message)
        }
    })?;
    attach_validate_target(&opts.target).map_err(|message| command_target_error("attach", &message))?;
    if let Some(alias) = opts.ssh_alias.as_deref() {
        attach_validate_token(alias, "ssh alias").map_err(|message| command_target_error("attach", &message))?;
    }
    for alive in &opts.alive {
        attach_validate_token(alive, "alive session").map_err(|message| command_target_error("attach", &message))?;
    }
    if let Some((node, session_name)) = attach_parse_explicit_remote_target(&opts.target) {
        attach_validate_token(&node, "remote node").map_err(|message| command_target_error("attach", &message))?;
        attach_validate_token(&session_name, "remote session").map_err(|message| command_target_error("attach", &message))?;
        let alias = opts.ssh_alias.clone().unwrap_or_else(|| node.clone());
        attach_validate_token(&alias, "ssh alias").map_err(|message| command_target_error("attach", &message))?;
        let stdout = if attach_has_flag(&opts, ATTACH_FLAG_PLAN_JSON) {
            attach_render_remote_plan_json(&opts.target, &node, &session_name, &alias, attach_has_flag(&opts, ATTACH_FLAG_YES))
        } else {
            attach_render_remote_plan_text(&opts.target, &node, &session_name, &alias, attach_has_flag(&opts, ATTACH_FLAG_YES))
        };
        return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
    }
    if opts.alive.is_empty() {
        opts.alive = attach_list_sessions(runner).into_iter().collect();
    }
    let resolved_target = match resolve_tmux_attach_session(&opts.target, &opts.alive) {
        TmuxAttachSessionResolution::Match { session }
        | TmuxAttachSessionResolution::Missing { session } => session,
        TmuxAttachSessionResolution::Ambiguous { candidates, .. } => {
            return Err(attach_port_ambiguous_error(&opts.target, &candidates));
        }
    };
    attach_validate_token(&resolved_target, "resolved session").map_err(|message| command_target_error("attach", &message))?;
    let in_tmux = std::env::var_os("TMUX").is_some();
    let action = decide_tmux_attach_action(
        &resolved_target,
        &opts.alive,
        attach_has_flag(&opts, ATTACH_FLAG_PRINT) || attach_has_flag(&opts, ATTACH_FLAG_PLAN_JSON),
        false,
        in_tmux,
    );
    let session = attach_port_action_session(&action);
    let stdout = if attach_has_flag(&opts, ATTACH_FLAG_PLAN_JSON) {
        attach_render_plan_json(&opts.target, session, &action, attach_has_flag(&opts, ATTACH_FLAG_READONLY))
    } else {
        attach_render_plan_text(&opts.target, session, &action, attach_has_flag(&opts, ATTACH_FLAG_READONLY))
    };
    let code = i32::from(matches!(action, TmuxAttachAction::Recover { .. }));
    Ok(CliOutput { code, stdout, stderr: String::new() })
}

fn attach_parse_args(argv: &[String]) -> Result<AttachOptions, String> {
    let mut flags = 0u8;
    let mut ssh_alias = None;
    let mut alive = BTreeSet::new();
    let mut target = None;
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(attach_port_usage_text()),
            "--print" => attach_set_flag(&mut flags, ATTACH_FLAG_PRINT),
            "--readonly" | "--read-only" | "-r" => attach_set_flag(&mut flags, ATTACH_FLAG_READONLY),
            "--plan-json" | "--dry-run" => attach_set_flag(&mut flags, ATTACH_FLAG_PLAN_JSON),
            "--yes" | "-y" => attach_set_flag(&mut flags, ATTACH_FLAG_YES),
            "--ssh-alias" => {
                let Some(value) = argv.get(index + 1) else { return Err("attach: missing --ssh-alias value".to_owned()); };
                ssh_alias = Some(value.clone());
                index += 1;
            }
            "--alive" => {
                let Some(value) = argv.get(index + 1) else { return Err("attach: missing --alive value".to_owned()); };
                alive.insert(value.clone());
                index += 1;
            }
            arg if arg.starts_with("--alive=") => { alive.insert(arg["--alive=".len()..].to_owned()); }
            arg if arg.starts_with("--ssh-alias=") => ssh_alias = Some(arg["--ssh-alias=".len()..].to_owned()),
            arg if arg.starts_with('-') => return Err(format!("attach: unknown argument {arg}")),
            value => {
                if target.is_some() { return Err("attach: target already provided".to_owned()); }
                target = Some(value.to_owned());
            }
        }
        index += 1;
    }
    Ok(AttachOptions {
        flags,
        ssh_alias,
        alive,
        target: target.ok_or_else(|| "attach: target required".to_owned())?,
    })
}

fn attach_set_flag(flags: &mut u8, flag: u8) {
    *flags |= flag;
}

fn attach_has_flag(options: &AttachOptions, flag: u8) -> bool {
    options.flags & flag != 0
}

fn attach_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Vec<String> {
    runner
        .run(
            "list-sessions",
            &["-F".to_owned(), "#{session_name}".to_owned()],
        )
        .map(|raw| raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn attach_parse_explicit_remote_target(target: &str) -> Option<(String, String)> {
    let (node, session_name) = target.split_once(':')?;
    let node = node.trim();
    let session_name = session_name.trim();
    if node.is_empty() || session_name.is_empty() { return None; }
    if session_name.split_once('.').map_or_else(
        || session_name.chars().all(|c| c.is_ascii_digit()),
        |(window, pane)| window.chars().all(|c| c.is_ascii_digit()) && pane.chars().all(|c| c.is_ascii_digit()),
    ) {
        return None;
    }
    Some((node.to_owned(), session_name.to_owned()))
}

fn attach_port_ambiguous_error(target: &str, candidates: &[String]) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "attach: '{target}' matches multiple sessions: {}\n  use the full name: maw-rs attach <exact-session>\n",
            candidates.join(", ")
        ),
    }
}


fn attach_port_usage_ok() -> CliOutput {
    CliOutput { code: 0, stdout: attach_port_usage_text(), stderr: String::new() }
}

fn attach_port_usage_error(message: &str) -> CliOutput {
    let usage = attach_port_usage_text();
    let stderr = if message == usage { format!("{usage}\n") } else { format!("{message}\n{usage}") };
    CliOutput { code: 2, stdout: String::new(), stderr }
}

fn attach_port_usage_text() -> String {
    "usage: maw-rs attach <target> [--print] [--readonly|-r]\n       maw-rs a <target> [--print] [--readonly|-r]\n".to_owned()
}

fn attach_render_remote_plan_text(
    target: &str,
    node: &str,
    session_name: &str,
    ssh_alias: &str,
    yes: bool,
) -> String {
    let yes_suffix = if yes { " -y" } else { "" };
    format!(
        "  \x1b[36m·\x1b[0m [dry-run] Tier 3 (remote) — would attach to {node}:{session_name} via ssh {ssh_alias}\n  command: maw-rs attach-ssh --node {node} --session {session_name} --ssh-alias {ssh_alias}{yes_suffix}\n  resolved: {target} → {node}:{session_name}\n"
    )
}

fn attach_render_remote_plan_json(
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

fn attach_render_plan_text(
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
            let args = attach_port_command_args(action, readonly);
            format!(
                "Run: tmux {}\n  resolved: {target} → {session}\n  detach with: Ctrl-b d\n",
                args.join(" ")
            )
        }
    }
}

fn attach_render_plan_json(
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
    let args = attach_port_command_args(action, readonly);
    format!(
        "{{\"command\":\"attach\",\"alias\":\"a\",\"target\":{},\"session\":{},\"action\":{},\"tmuxArgs\":{}}}\n",
        json_string(target),
        json_string(session),
        json_string(kind),
        json_string_array(&args)
    )
}

fn attach_port_command_args(action: &TmuxAttachAction, readonly: bool) -> Vec<String> {
    if readonly {
        return vec!["attach".to_owned(), "-r".to_owned(), "-t".to_owned(), attach_port_action_session(action).to_owned()];
    }
    tmux_attach_spawn_command(action).map_or_else(
        || vec!["attach".to_owned(), "-t".to_owned(), attach_port_action_session(action).to_owned()],
        |command| command.args,
    )
}

fn attach_port_action_session(action: &TmuxAttachAction) -> &str {
    match action {
        TmuxAttachAction::Print { session }
        | TmuxAttachAction::SwitchClient { session }
        | TmuxAttachAction::Attach { session }
        | TmuxAttachAction::Recover { session } => session,
    }
}

fn attach_validate_target(value: &str) -> Result<(), String> {
    attach_validate_common(value, "target")?;
    if value == "--" { return Err("attach target must not be --".to_owned()); }
    Ok(())
}

fn attach_validate_token(value: &str, label: &str) -> Result<(), String> {
    attach_validate_common(value, label)?;
    if value.chars().any(char::is_whitespace) {
        return Err(format!("attach {label} must not contain whitespace"));
    }
    Ok(())
}

fn attach_validate_common(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("attach {label} must be non-empty, unpadded, not start with '-', and contain no control characters"));
    }
    Ok(())
}

#[cfg(test)]
mod attach_tests {
    use super::*;

    #[derive(Default)]
    struct AttachFakeRunner { calls: Vec<(String, Vec<String>)> }

    impl maw_tmux::TmuxRunner for AttachFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            if subcommand == "list-sessions" { Ok("50-mawjs\n05-volt\n".to_owned()) } else { Ok(String::new()) }
        }
    }

    fn attach_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn attach_dispatch_fragment_owns_attach_aliases() {
        let commands = DISPATCH_111.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, vec!["attach", "a"]);
    }

    #[test]
    fn attach_uses_tmux_runner_for_alive_sessions_and_prints_plan() {
        let mut runner = AttachFakeRunner::default();
        let output = attach_run_with_runner(&attach_strings(&["mawjs", "--print"]), &mut runner).unwrap();
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("Run: tmux attach -t 50-mawjs"));
        assert_eq!(runner.calls[0].0, "list-sessions");
    }

    #[test]
    fn attach_rejects_control_and_leading_dash_targets_before_runner() {
        let mut runner = AttachFakeRunner::default();
        let err = attach_run_with_runner(&attach_strings(&["bad\nname"]), &mut runner).unwrap_err();
        assert!(err.stderr.contains("contain no control"));
        let err = attach_run_with_runner(&attach_strings(&["-t"]), &mut runner).unwrap_err();
        assert!(err.stderr.contains("unknown argument -t"));
        assert!(runner.calls.is_empty());
    }
}
