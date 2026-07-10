const DISPATCH_111: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "attach",
        handler: Handler::Sync(attach_run_command),
    },
    DispatcherEntry {
        command: "a",
        handler: Handler::Sync(attach_run_command),
    },
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
    attach_validate_target(&opts.target)
        .map_err(|message| command_target_error("attach", &message))?;
    if let Some(alias) = opts.ssh_alias.as_deref() {
        attach_validate_token(alias, "ssh alias")
            .map_err(|message| command_target_error("attach", &message))?;
    }
    for alive in &opts.alive {
        attach_validate_token(alive, "alive session")
            .map_err(|message| command_target_error("attach", &message))?;
    }
    if let Some((node, session_name)) = attach_parse_explicit_remote_target(&opts.target) {
        attach_validate_token(&node, "remote node")
            .map_err(|message| command_target_error("attach", &message))?;
        attach_validate_token(&session_name, "remote session")
            .map_err(|message| command_target_error("attach", &message))?;
        let alias = opts.ssh_alias.clone().unwrap_or_else(|| node.clone());
        attach_validate_token(&alias, "ssh alias")
            .map_err(|message| command_target_error("attach", &message))?;
        let stdout = if attach_has_flag(&opts, ATTACH_FLAG_PLAN_JSON) {
            attach_render_remote_plan_json(
                &opts.target,
                &node,
                &session_name,
                &alias,
                attach_has_flag(&opts, ATTACH_FLAG_YES),
            )
        } else {
            attach_render_remote_plan_text(
                &opts.target,
                &node,
                &session_name,
                &alias,
                attach_has_flag(&opts, ATTACH_FLAG_YES),
            )
        };
        return Ok(CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        });
    }
    if opts.alive.is_empty() {
        opts.alive = attach_list_sessions(runner).into_iter().collect();
    }
    let resolved_target = attach_resolved_target_for_options(&opts)?;
    attach_validate_token(&resolved_target, "resolved session")
        .map_err(|message| command_target_error("attach", &message))?;
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
        attach_render_plan_json(
            &opts.target,
            session,
            &action,
            attach_has_flag(&opts, ATTACH_FLAG_READONLY),
        )
    } else {
        attach_render_plan_text(
            &opts.target,
            session,
            &action,
            attach_has_flag(&opts, ATTACH_FLAG_READONLY),
        )
    };
    let code = i32::from(matches!(action, TmuxAttachAction::Recover { .. }));
    Ok(CliOutput {
        code,
        stdout,
        stderr: String::new(),
    })
}

fn attach_resolved_target_for_options(opts: &AttachOptions) -> Result<String, CliOutput> {
    match attach_resolve_typed_target(&opts.target, &opts.alive) {
        AttachResolvedTarget::Live(session) | AttachResolvedTarget::Missing(session) => Ok(session),
        AttachResolvedTarget::BridgeCandidates(candidates) => attach_picker_output(
            &opts.target,
            "not found as a live session",
            &candidates,
            opts,
        ),
        AttachResolvedTarget::Ambiguous(candidates) => {
            match attach_unique_raw_live_match(&opts.target, &candidates) {
                Some(session) => Ok(session),
                None => attach_picker_output(
                    &opts.target,
                    "matches multiple sessions",
                    &candidates,
                    opts,
                ),
            }
        }
    }
}

enum AttachResolvedTarget {
    Live(String),
    Missing(String),
    BridgeCandidates(Vec<maw_matcher::ResolveMatch>),
    Ambiguous(Vec<maw_matcher::ResolveMatch>),
}

fn attach_resolve_typed_target(target: &str, alive: &BTreeSet<String>) -> AttachResolvedTarget {
    let candidates = attach_typed_candidates(alive);
    match maw_matcher::resolve_typed_target(target, &candidates) {
        maw_matcher::ResolveTypedResult::None => AttachResolvedTarget::Missing(target.to_owned()),
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => {
            AttachResolvedTarget::Ambiguous(candidates)
        }
        maw_matcher::ResolveTypedResult::Match { matched } => match matched.candidate.kind {
            maw_matcher::ResolveCandidateKind::LiveSession
            | maw_matcher::ResolveCandidateKind::Window => {
                AttachResolvedTarget::Live(matched.candidate.name)
            }
            maw_matcher::ResolveCandidateKind::SleepingRegistry
            | maw_matcher::ResolveCandidateKind::Oracle
            | maw_matcher::ResolveCandidateKind::FleetSquad => {
                AttachResolvedTarget::BridgeCandidates(vec![matched])
            }
            _ => AttachResolvedTarget::Missing(target.to_owned()),
        },
    }
}

fn attach_typed_candidates(alive: &BTreeSet<String>) -> Vec<maw_matcher::ResolveTypedCandidate> {
    let mut candidates = alive
        .iter()
        .map(|name| maw_matcher::ResolveTypedCandidate {
            kind: maw_matcher::ResolveCandidateKind::LiveSession,
            name: name.clone(),
            aliases: Vec::new(),
        })
        .collect::<Vec<_>>();
    for entry in fleet_load_entries() {
        if let Some(group) = fleet_roster_squad_name(&entry) {
            candidates.push(maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::FleetSquad,
                name: group,
                aliases: attach_group_aliases(&entry),
            });
        } else if !attach_alive_covers_name(alive, &entry.session.name) {
            candidates.push(maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::SleepingRegistry,
                name: entry.session.name.clone(),
                aliases: attach_registry_aliases(&entry),
            });
        }
    }
    candidates
}

fn attach_alive_covers_name(alive: &BTreeSet<String>, name: &str) -> bool {
    let names = maw_matcher::normalized_match_names(name);
    alive.iter().any(|live| {
        maw_matcher::normalized_match_names(live)
            .iter()
            .any(|live_name| names.contains(live_name))
    })
}

fn attach_unique_raw_live_match(
    target: &str,
    candidates: &[maw_matcher::ResolveMatch],
) -> Option<String> {
    let target = target.trim();
    let matches = candidates
        .iter()
        .filter(|matched| {
            matched.candidate.kind == maw_matcher::ResolveCandidateKind::LiveSession
                && matched.candidate.name.eq_ignore_ascii_case(target)
        })
        .map(|matched| matched.candidate.name.clone())
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].clone())
}

fn attach_picker_output(
    target: &str,
    context: &str,
    candidates: &[maw_matcher::ResolveMatch],
    options: &AttachOptions,
) -> Result<String, CliOutput> {
    let rows = attach_picker_rows(candidates);
    if rows.is_empty() {
        return Ok(target.to_owned());
    }
    let json = attach_has_flag(options, ATTACH_FLAG_PLAN_JSON);
    if attach_has_flag(options, ATTACH_FLAG_YES) && rows.len() == 1 {
        return attach_run_picker_row(rows[0].clone());
    }
    if json || attach_has_flag(options, ATTACH_FLAG_PRINT) || !attach_stdin_is_terminal() {
        let stdout = if json {
            picker_render_json("attach", target, context, &rows)
        } else {
            picker_render_text("attach", target, context, &rows)
        };
        return Err(CliOutput {
            code: 1,
            stdout,
            stderr: String::new(),
        });
    }
    attach_prompt_picker(target, context, &rows).map_or_else(
        || {
            Err(CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: "attach: picker cancelled\n".to_owned(),
            })
        },
        attach_run_picker_row,
    )
}

fn attach_picker_rows(candidates: &[maw_matcher::ResolveMatch]) -> Vec<PickerRow> {
    candidates
        .iter()
        .filter_map(|matched| {
            Some(PickerRow {
                matched: matched.clone(),
                detail: attach_picker_detail(matched),
                action: attach_picker_action(matched)?,
            })
        })
        .collect()
}

fn attach_picker_action(matched: &maw_matcher::ResolveMatch) -> Option<String> {
    match matched.candidate.kind {
        maw_matcher::ResolveCandidateKind::FleetSquad => {
            Some(format!("maw fleet wake {}", matched.candidate.name))
        }
        maw_matcher::ResolveCandidateKind::SleepingRegistry => Some(format!(
            "maw wake {} --attach --session {}",
            matched.candidate.name, matched.candidate.name
        )),
        maw_matcher::ResolveCandidateKind::Oracle => {
            Some(format!("maw wake {} --attach", matched.candidate.name))
        }
        maw_matcher::ResolveCandidateKind::LiveSession
        | maw_matcher::ResolveCandidateKind::Window => {
            Some(format!("maw attach {}", matched.candidate.name))
        }
        maw_matcher::ResolveCandidateKind::Repo | maw_matcher::ResolveCandidateKind::Peer => None,
    }
}

fn attach_picker_detail(matched: &maw_matcher::ResolveMatch) -> Option<String> {
    (matched.candidate.kind == maw_matcher::ResolveCandidateKind::FleetSquad)
        .then(|| {
            fleet_load_entries().into_iter().find(|entry| {
                fleet_roster_squad_name(entry).as_deref() == Some(matched.candidate.name.as_str())
            })
        })
        .flatten()
        .map(|entry| {
            format!(
                "{} members",
                entry.session.members.as_ref().map_or(0, Vec::len)
            )
        })
}

fn attach_prompt_picker(target: &str, context: &str, rows: &[PickerRow]) -> Option<PickerRow> {
    use std::io::Write as _;
    eprint!("{}", picker_render_text("attach", target, context, rows));
    let yes_hint = if rows.len() == 1 { ", Enter/y" } else { "" };
    loop {
        eprint!("pick [1-{}]{yes_hint} or q: ", rows.len());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return None;
        }
        match picker_parse_selection(&line, rows.len()) {
            PickerSelection::Pick(index) => return rows.get(index).cloned(),
            PickerSelection::Quit => return None,
            PickerSelection::Invalid => {
                eprintln!("attach: enter a number from 1 to {} or q", rows.len());
            }
        }
    }
}

fn attach_run_picker_row(row: PickerRow) -> Result<String, CliOutput> {
    attach_validate_token(&row.matched.candidate.name, "picker target")
        .map_err(|message| command_target_error("attach", &message))?;
    match row.matched.candidate.kind {
        maw_matcher::ResolveCandidateKind::LiveSession
        | maw_matcher::ResolveCandidateKind::Window => Ok(row.matched.candidate.name),
        maw_matcher::ResolveCandidateKind::SleepingRegistry => Err(run_wake_command(&[
            row.matched.candidate.name.clone(),
            "--attach".to_owned(),
            "--session".to_owned(),
            row.matched.candidate.name,
        ])),
        maw_matcher::ResolveCandidateKind::Oracle => Err(run_wake_command(&[
            row.matched.candidate.name,
            "--attach".to_owned(),
        ])),
        maw_matcher::ResolveCandidateKind::FleetSquad => Err(run_fleet_command(&[
            "wake".to_owned(),
            row.matched.candidate.name,
        ])),
        maw_matcher::ResolveCandidateKind::Repo | maw_matcher::ResolveCandidateKind::Peer => {
            Err(CliOutput {
                code: 1,
                stdout: format!(
                    "attach: no attach action for {}\n",
                    row.matched.candidate.name
                ),
                stderr: String::new(),
            })
        }
    }
}

fn attach_group_aliases(entry: &NativeFleetEntry) -> Vec<String> {
    let mut aliases = vec![
        entry.session.name.clone(),
        entry.file.clone(),
        fleet_roster_unnumbered_stem(entry).to_owned(),
    ];
    if !entry.session.squad_name.is_empty() {
        aliases.push(entry.session.squad_name.clone());
    }
    aliases
}

fn attach_registry_aliases(entry: &NativeFleetEntry) -> Vec<String> {
    let mut aliases = Vec::new();
    for window in &entry.session.windows {
        if !window.name.is_empty() {
            aliases.push(window.name.clone());
        }
        if let Some(name) = native_fleet_window_oracle_name(window) {
            aliases.push(name);
        }
        if let Some(repo) = window
            .repo
            .rsplit('/')
            .next()
            .filter(|repo| !repo.is_empty())
        {
            aliases.push((*repo).to_owned());
        }
    }
    aliases
}

fn attach_stdin_is_terminal() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
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
            "--readonly" | "--read-only" | "-r" => {
                attach_set_flag(&mut flags, ATTACH_FLAG_READONLY);
            }
            "--plan-json" | "--dry-run" => attach_set_flag(&mut flags, ATTACH_FLAG_PLAN_JSON),
            "--yes" | "-y" => attach_set_flag(&mut flags, ATTACH_FLAG_YES),
            "--ssh-alias" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("attach: missing --ssh-alias value".to_owned());
                };
                ssh_alias = Some(value.clone());
                index += 1;
            }
            "--alive" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("attach: missing --alive value".to_owned());
                };
                alive.insert(value.clone());
                index += 1;
            }
            arg if arg.starts_with("--alive=") => {
                alive.insert(arg["--alive=".len()..].to_owned());
            }
            arg if arg.starts_with("--ssh-alias=") => {
                ssh_alias = Some(arg["--ssh-alias=".len()..].to_owned());
            }
            arg if arg.starts_with('-') => return Err(format!("attach: unknown argument {arg}")),
            value => {
                if target.is_some() {
                    return Err("attach: target already provided".to_owned());
                }
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
        .map(|raw| {
            raw.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn attach_parse_explicit_remote_target(target: &str) -> Option<(String, String)> {
    let (node, session_name) = target.split_once(':')?;
    let node = node.trim();
    let session_name = session_name.trim();
    if node.is_empty() || session_name.is_empty() {
        return None;
    }
    if session_name.split_once('.').map_or_else(
        || session_name.chars().all(|c| c.is_ascii_digit()),
        |(window, pane)| {
            window.chars().all(|c| c.is_ascii_digit()) && pane.chars().all(|c| c.is_ascii_digit())
        },
    ) {
        return None;
    }
    Some((node.to_owned(), session_name.to_owned()))
}

fn attach_port_usage_ok() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: attach_port_usage_text(),
        stderr: String::new(),
    }
}

fn attach_port_usage_error(message: &str) -> CliOutput {
    let usage = attach_port_usage_text();
    let stderr = if message == usage {
        format!("{usage}\n")
    } else {
        format!("{message}\n{usage}")
    };
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr,
    }
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
        return vec![
            "attach".to_owned(),
            "-r".to_owned(),
            "-t".to_owned(),
            attach_port_action_session(action).to_owned(),
        ];
    }
    tmux_attach_spawn_command(action).map_or_else(
        || {
            vec![
                "attach".to_owned(),
                "-t".to_owned(),
                attach_port_action_session(action).to_owned(),
            ]
        },
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
    if value == "--" {
        return Err("attach target must not be --".to_owned());
    }
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
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(format!("attach {label} must be non-empty, unpadded, not start with '-', and contain no control characters"));
    }
    Ok(())
}

#[cfg(test)]
mod attach_tests {
    use super::*;

    #[derive(Default)]
    struct AttachFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        sessions: String,
    }

    impl maw_tmux::TmuxRunner for AttachFakeRunner {
        fn run(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            if subcommand == "list-sessions" {
                Ok(if self.sessions.is_empty() {
                    "50-mawjs\n05-volt\n".to_owned()
                } else {
                    self.sessions.clone()
                })
            } else {
                Ok(String::new())
            }
        }
    }

    fn attach_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn attach_fleet_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("maw-rs-attach-{name}-{}", std::process::id()))
    }

    fn attach_with_fleet_env(root: &std::path::Path, test: impl FnOnce()) {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = [
            "HOME",
            "MAW_HOME",
            "MAW_XDG",
            "MAW_STATE_DIR",
            "MAW_CONFIG_DIR",
            "XDG_STATE_HOME",
            "XDG_CONFIG_HOME",
            "GHQ_ROOT",
        ]
        .map(EnvVarRestore::capture);
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq"));
        for key in ["MAW_HOME", "MAW_XDG", "XDG_STATE_HOME", "XDG_CONFIG_HOME"] {
            std::env::remove_var(key);
        }
        test();
    }

    #[test]
    fn attach_dispatch_fragment_owns_attach_aliases() {
        let commands = DISPATCH_111
            .iter()
            .map(|entry| entry.command)
            .collect::<Vec<_>>();
        assert_eq!(commands, vec!["attach", "a"]);
    }

    #[test]
    fn attach_uses_tmux_runner_for_alive_sessions_and_prints_plan() {
        let root = attach_fleet_root("print-plan");
        let _ = std::fs::remove_dir_all(&root);
        attach_with_fleet_env(&root, || {
            let mut runner = AttachFakeRunner::default();
            let output =
                attach_run_with_runner(&attach_strings(&["mawjs", "--print"]), &mut runner)
                    .unwrap();
            assert_eq!(output.code, 0);
            assert!(output.stdout.contains("Run: tmux attach -t 50-mawjs"));
            assert_eq!(runner.calls[0].0, "list-sessions");
        });
    }

    #[test]
    fn attach_prefers_live_tmux_session_over_stale_legacy_fleet_file() {
        let root = attach_fleet_root("live-over-fleet");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home/.maw/fleet")).expect("legacy fleet dir");
        std::fs::write(
            root.join("home/.maw/fleet/ghost.json"),
            r#"{"name":"ghost","windows":[{"name":"ghost","repo":"acme/dead"}]}"#,
        )
        .expect("legacy fleet file");
        attach_with_fleet_env(&root, || {
            let mut runner = AttachFakeRunner {
                sessions: "05-ghost\n".to_owned(),
                ..AttachFakeRunner::default()
            };
            let output =
                attach_run_with_runner(&attach_strings(&["ghost", "--plan-json"]), &mut runner)
                    .unwrap();
            assert_eq!(output.code, 0, "{}{}", output.stdout, output.stderr);
            assert!(output.stdout.contains("\"session\":\"05-ghost\""));
            assert!(output.stdout.contains("\"action\":\"print\""));
            assert!(runner.calls.iter().any(|call| call.0 == "list-sessions"));
        });
    }

    #[test]
    fn attach_resolver_bridges_sleeping_registry_and_suggests_group() {
        let root = attach_fleet_root("bridge-group");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("state/fleet")).expect("state fleet");
        std::fs::write(
            root.join("state/fleet/ghost.json"),
            r#"{"name":"ghost","windows":[{"name":"ghost","repo":"acme/ghost"}]}"#,
        )
        .expect("ghost fleet file");
        std::fs::write(root.join("state/fleet/01-3e.json"), r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"},{"handle":"drift"}]}"#).expect("group fleet file");
        attach_with_fleet_env(&root, || {
            let mut runner = AttachFakeRunner {
                sessions: "05-volt\n".to_owned(),
                ..AttachFakeRunner::default()
            };
            let sleeping =
                attach_run_with_runner(&attach_strings(&["ghost"]), &mut runner).unwrap_err();
            assert_eq!(sleeping.code, 1, "{}{}", sleeping.stdout, sleeping.stderr);
            assert!(sleeping.stdout.contains("maw wake ghost --attach"));
            let group = attach_run_with_runner(&attach_strings(&["3e"]), &mut runner).unwrap_err();
            assert_eq!(group.code, 1, "{}{}", group.stdout, group.stderr);
            assert!(group.stdout.contains("maw fleet wake 3e"));
            assert!(group.stdout.contains("2 members"));
        });
    }

    #[test]
    fn attach_resolver_prints_candidates_for_non_tty_ambiguity() {
        let root = attach_fleet_root("homekeeper-ambiguous");
        let _ = std::fs::remove_dir_all(&root);
        attach_with_fleet_env(&root, || {
            let mut runner = AttachFakeRunner::default();
            let err = attach_run_with_runner(
                &attach_strings(&[
                    "homekeeper",
                    "--alive",
                    "158-homekeeper",
                    "--alive",
                    "159-homekeeper",
                ]),
                &mut runner,
            )
            .unwrap_err();
            assert_eq!(err.code, 1, "{}{}", err.stdout, err.stderr);
            assert!(
                err.stdout
                    .contains("attach: 'homekeeper' matches multiple sessions. Found nearby:"),
                "{}",
                err.stdout
            );
            assert!(
                err.stdout.contains("1. session 158-homekeeper (Exact)"),
                "{}",
                err.stdout
            );
            assert!(
                err.stdout.contains("→ maw attach 158-homekeeper"),
                "{}",
                err.stdout
            );
            assert!(err.stderr.is_empty(), "{}", err.stderr);
            assert!(runner.calls.is_empty());
        });
    }

    #[test]
    fn attach_picker_actions_map_candidates_to_bridge_commands() {
        let group = maw_matcher::ResolveMatch {
            rank: maw_matcher::ResolveMatchRank::Exact,
            candidate: maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::FleetSquad,
                name: "3e".to_owned(),
                aliases: Vec::new(),
            },
        };
        let sleeping = maw_matcher::ResolveMatch {
            rank: maw_matcher::ResolveMatchRank::Exact,
            candidate: maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::SleepingRegistry,
                name: "47-3e-infra".to_owned(),
                aliases: Vec::new(),
            },
        };
        let live = maw_matcher::ResolveMatch {
            rank: maw_matcher::ResolveMatchRank::Live,
            candidate: maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::LiveSession,
                name: "99-live".to_owned(),
                aliases: Vec::new(),
            },
        };
        assert_eq!(picker_parse_selection("", 1), PickerSelection::Pick(0));
        assert_eq!(picker_parse_selection("y", 1), PickerSelection::Pick(0));
        assert_eq!(picker_parse_selection("2", 3), PickerSelection::Pick(1));
        assert_eq!(
            attach_picker_action(&group).as_deref(),
            Some("maw fleet wake 3e")
        );
        assert_eq!(
            attach_picker_action(&sleeping).as_deref(),
            Some("maw wake 47-3e-infra --attach --session 47-3e-infra")
        );
        assert_eq!(
            attach_picker_action(&live).as_deref(),
            Some("maw attach 99-live")
        );
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
