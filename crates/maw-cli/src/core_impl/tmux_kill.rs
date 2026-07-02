const DISPATCH_282: &[DispatcherEntry] = &[];

const TMUX_SUB_282: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["kill"],
    handler: run_tmux_kill_command,
}];

const TMUX_KILL_USAGE: &str = "usage: maw tmux kill <target> [--session|-s] [--force]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxKillArgs {
    target: String,
    session: bool,
    force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxKillPane {
    id: String,
    target: String,
    session: String,
    window_index: String,
    pane_index: String,
}

fn run_tmux_kill_command(argv: &[String]) -> CliOutput {
    match tmux_kill_run_with(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) if message == TMUX_KILL_USAGE => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        Err(message) => command_target_error("tmux kill", &message),
    }
}

fn tmux_kill_run_with<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, String> {
    let parsed = tmux_kill_parse_args(argv)?;
    tmux_kill_validate_user_target(&parsed.target)?;
    if parsed.session {
        tmux_kill_session(runner, &parsed)
    } else {
        tmux_kill_pane(runner, &parsed)
    }
}

fn tmux_kill_parse_args(argv: &[String]) -> Result<TmuxKillArgs, String> {
    if argv.is_empty() || argv.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err(TMUX_KILL_USAGE.to_owned());
    }
    let mut target = String::new();
    let mut session = false;
    let mut force = false;
    for arg in argv {
        match arg.as_str() {
            "--session" | "-s" => session = true,
            "--force" => force = true,
            value if value.starts_with('-') => {
                return Err(format!("\"{value}\" looks like a flag, not a tmux kill target"));
            }
            value => {
                if !target.is_empty() {
                    return Err(format!("tmux kill: unexpected argument {value}"));
                }
                value.clone_into(&mut target);
            }
        }
    }
    if target.is_empty() {
        return Err(TMUX_KILL_USAGE.to_owned());
    }
    Ok(TmuxKillArgs {
        target,
        session,
        force,
    })
}

fn tmux_kill_session<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    args: &TmuxKillArgs,
) -> Result<String, String> {
    let sessions = tmux_kill_list_sessions(runner)?;
    let session = tmux_kill_resolve_session(&args.target, &sessions)?;
    tmux_kill_validate_resolved_target(&session, "resolved session")?;
    tmux_kill_refuse_protected_session(&session, args.force)?;
    runner
        .run("kill-session", &tmux_kill_strings(&["-t", &session]))
        .map_err(|error| format!("tmux kill-session failed: {}", error.message))?;
    Ok(format!("  \x1b[32m✓\x1b[0m killed session {session}\n"))
}

fn tmux_kill_pane<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    args: &TmuxKillArgs,
) -> Result<String, String> {
    let raw = runner
        .run(
            "list-panes",
            &tmux_kill_strings(&["-a", "-F", TMUX_KILL_PANE_FORMAT]),
        )
        .map_err(|error| format!("tmux list-panes failed: {}", error.message))?;
    let panes = tmux_kill_parse_panes(&raw);
    let pane = tmux_kill_resolve_pane(&args.target, &panes)?;
    tmux_kill_validate_resolved_target(&pane.id, "resolved pane id")?;
    tmux_kill_validate_resolved_target(&pane.session, "resolved pane session")?;
    tmux_kill_refuse_protected_session(&pane.session, args.force)?;
    runner
        .run("kill-pane", &tmux_kill_strings(&["-t", &pane.id]))
        .map_err(|error| format!("tmux kill-pane failed: {}", error.message))?;
    Ok(format!("  \x1b[32m✓\x1b[0m killed pane {}\n", pane.id))
}

const TMUX_KILL_PANE_FORMAT: &str =
    "#{pane_id}|||#{session_name}:#{window_index}.#{pane_index}|||#{session_name}|||#{window_index}|||#{pane_index}";

fn tmux_kill_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<String>, String> {
    runner
        .run(
            "list-sessions",
            &tmux_kill_strings(&["-F", "#{session_name}"]),
        )
        .map_err(|error| format!("tmux list-sessions failed: {}", error.message))
        .map(|raw| {
            raw.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
}

fn tmux_kill_resolve_session(target: &str, sessions: &[String]) -> Result<String, String> {
    if let Some(exact) = sessions.iter().find(|session| session.as_str() == target) {
        return Ok(exact.clone());
    }
    let matches = sessions
        .iter()
        .filter(|session| session.to_lowercase().contains(&target.to_lowercase()))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [single] => Ok(single.clone()),
        [] => Err(format!("session '{target}' not found")),
        many => Err(format!(
            "session '{target}' is ambiguous — matches: {}",
            many.join(", ")
        )),
    }
}

fn tmux_kill_parse_panes(raw: &str) -> Vec<TmuxKillPane> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split("|||");
            let id = parts.next()?.trim();
            let target = parts.next().unwrap_or_default().trim();
            let session = parts.next().unwrap_or_default().trim();
            let window_index = parts.next().unwrap_or_default().trim();
            let pane_index = parts.next().unwrap_or_default().trim();
            if id.is_empty() || session.is_empty() {
                return None;
            }
            Some(TmuxKillPane {
                id: id.to_owned(),
                target: target.to_owned(),
                session: session.to_owned(),
                window_index: window_index.to_owned(),
                pane_index: pane_index.to_owned(),
            })
        })
        .collect()
}

fn tmux_kill_resolve_pane(target: &str, panes: &[TmuxKillPane]) -> Result<TmuxKillPane, String> {
    let exact = panes
        .iter()
        .filter(|pane| tmux_kill_pane_matches(target, pane))
        .cloned()
        .collect::<Vec<_>>();
    match exact.as_slice() {
        [single] => Ok(single.clone()),
        [] => {
            let raw = panes
                .iter()
                .map(|pane| format!("{}|||{}|||{}|||role|||", pane.id, pane.target, pane.id))
                .collect::<Vec<_>>()
                .join("\n");
            match maw_tmux::resolve_pane_target_from_list_panes_output(target, &raw) {
                maw_tmux::PaneTargetResolution::Match { candidate } => panes
                    .iter()
                    .find(|pane| pane.id == candidate.resolved)
                    .cloned()
                    .ok_or_else(|| format!("pane '{target}' resolved to a missing pane")),
                maw_tmux::PaneTargetResolution::Ambiguous { candidates } => {
                    let labels = candidates
                        .iter()
                        .map(|candidate| candidate.resolved.clone())
                        .collect::<Vec<_>>();
                    Err(format!(
                        "pane '{target}' is ambiguous — matches: {}",
                        labels.join(", ")
                    ))
                }
                maw_tmux::PaneTargetResolution::None => Err(format!("pane '{target}' not found")),
            }
        }
        many => Err(tmux_kill_ambiguous_panes(target, many)),
    }
}

fn tmux_kill_pane_matches(target: &str, pane: &TmuxKillPane) -> bool {
    target == pane.id
        || target == pane.target
        || (!pane.session.is_empty()
            && !pane.window_index.is_empty()
            && !pane.pane_index.is_empty()
            && target == format!("{}:{}.{}", pane.session, pane.window_index, pane.pane_index))
}

fn tmux_kill_ambiguous_panes(target: &str, panes: &[TmuxKillPane]) -> String {
    format!(
        "pane '{target}' is ambiguous — matches: {}",
        panes
            .iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn tmux_kill_refuse_protected_session(session: &str, force: bool) -> Result<(), String> {
    if force || !tmux_kill_is_protected_session(session) {
        return Ok(());
    }
    Err(format!(
        "refusing to kill protected fleet/view session '{session}' without --force"
    ))
}

fn tmux_kill_is_protected_session(session: &str) -> bool {
    let lower = session.to_ascii_lowercase();
    lower.ends_with("-view")
        || lower.contains("-view-")
        || lower.starts_with("view-")
        || lower.starts_with("view:")
        || tmux_kill_looks_like_fleet_session(session)
}

fn tmux_kill_looks_like_fleet_session(session: &str) -> bool {
    let Some((prefix, rest)) = session.split_once('-') else {
        return false;
    };
    (1..=3).contains(&prefix.len())
        && prefix.bytes().all(|byte| byte.is_ascii_digit())
        && !rest.is_empty()
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn tmux_kill_validate_user_target(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("target must be non-empty".to_owned());
    }
    if value.trim() != value {
        return Err("target must not have surrounding whitespace".to_owned());
    }
    if value == "--" || value.starts_with('-') {
        return Err(format!("\"{value}\" looks like a flag, not a tmux kill target"));
    }
    if value.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("target must not contain NUL/control characters".to_owned());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%'))
    {
        return Err("target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_kill_validate_resolved_target(value: &str, label: &str) -> Result<(), String> {
    tmux_kill_validate_user_target(value).map_err(|message| format!("{label}: {message}"))
}

fn tmux_kill_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tmux_kill_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TmuxKillCall {
        subcommand: String,
        args: Vec<String>,
    }

    #[derive(Debug, Default)]
    struct TmuxKillFakeRunner {
        sessions: String,
        panes: String,
        calls: Vec<TmuxKillCall>,
    }

    impl maw_tmux::TmuxRunner for TmuxKillFakeRunner {
        fn run(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push(TmuxKillCall {
                subcommand: subcommand.to_owned(),
                args: args.to_vec(),
            });
            Ok(match subcommand {
                "list-sessions" => self.sessions.clone(),
                "list-panes" => self.panes.clone(),
                "kill-session" | "kill-pane" => String::new(),
                other => panic!("unexpected tmux subcommand: {other}"),
            })
        }
    }

    fn fake_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn fake_runner() -> TmuxKillFakeRunner {
        TmuxKillFakeRunner {
            sessions: "scratch\n".to_owned(),
            panes: "%42|||scratch:1.2|||scratch|||1|||2\n".to_owned(),
            calls: Vec::new(),
        }
    }

    #[test]
    fn tmux_kill_fragment_is_part282_only() {
        assert!(DISPATCH_282.is_empty());
        assert_eq!(TMUX_SUB_282.len(), 1);
        assert_eq!(TMUX_SUB_282[0].names, &["kill"]);
    }

    #[test]
    fn tmux_kill_pane_preflights_before_kill_pane() {
        let mut runner = fake_runner();
        let output = tmux_kill_run_with(&fake_args(&["%42"]), &mut runner).expect("kill pane");
        assert_eq!(output, "  \x1b[32m✓\x1b[0m killed pane %42\n");
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(runner.calls[0].subcommand, "list-panes");
        assert_eq!(runner.calls[1].subcommand, "kill-pane");
        assert_eq!(runner.calls[1].args, fake_args(&["-t", "%42"]));
    }

    #[test]
    fn tmux_kill_session_flag_preflights_before_kill_session() {
        let mut runner = fake_runner();
        let output = tmux_kill_run_with(&fake_args(&["scratch", "--session"]), &mut runner)
            .expect("kill session");
        assert_eq!(output, "  \x1b[32m✓\x1b[0m killed session scratch\n");
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(runner.calls[0].subcommand, "list-sessions");
        assert_eq!(runner.calls[1].subcommand, "kill-session");
        assert_eq!(runner.calls[1].args, fake_args(&["-t", "scratch"]));
    }

    #[test]
    fn tmux_kill_refuses_protected_session_without_force_before_kill() {
        let mut runner = TmuxKillFakeRunner {
            sessions: "07-demo\n".to_owned(),
            ..TmuxKillFakeRunner::default()
        };
        let error = tmux_kill_run_with(&fake_args(&["demo", "--session"]), &mut runner)
            .expect_err("protected session refused");
        assert!(error.contains("protected fleet/view session"));
        assert_eq!(runner.calls.len(), 1);
        assert!(!runner
            .calls
            .iter()
            .any(|call| call.subcommand == "kill-session"));
    }

    #[test]
    fn tmux_kill_force_allows_protected_session_after_preflight() {
        let mut runner = TmuxKillFakeRunner {
            sessions: "07-demo\n".to_owned(),
            ..TmuxKillFakeRunner::default()
        };
        let output = tmux_kill_run_with(&fake_args(&["demo", "--session", "--force"]), &mut runner)
            .expect("forced kill session");
        assert!(output.contains("killed session 07-demo"));
        assert_eq!(runner.calls[1].subcommand, "kill-session");
    }

    #[test]
    fn tmux_kill_refuses_protected_pane_session_without_force_before_kill() {
        let mut runner = TmuxKillFakeRunner {
            panes: "%7|||foo-view:0.1|||foo-view|||0|||1\n".to_owned(),
            ..TmuxKillFakeRunner::default()
        };
        let error = tmux_kill_run_with(&fake_args(&["%7"]), &mut runner)
            .expect_err("protected pane session refused");
        assert!(error.contains("protected fleet/view session"));
        assert_eq!(runner.calls.len(), 1);
        assert!(!runner.calls.iter().any(|call| call.subcommand == "kill-pane"));
    }

    #[test]
    fn tmux_kill_rejects_leading_dash_target_before_runner() {
        let mut runner = fake_runner();
        let error = tmux_kill_run_with(&fake_args(&["-Sbad"]), &mut runner)
            .expect_err("leading dash rejected");
        assert!(error.contains("looks like a flag"));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn tmux_kill_rejects_control_target_before_runner() {
        let mut runner = fake_runner();
        let error = tmux_kill_run_with(&["bad\ntarget".to_owned()], &mut runner)
            .expect_err("control target rejected");
        assert!(error.contains("control"));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn tmux_kill_rejects_ambiguous_session_before_kill() {
        let mut runner = TmuxKillFakeRunner {
            sessions: "one-bar\ntwo-bar\n".to_owned(),
            ..TmuxKillFakeRunner::default()
        };
        let error = tmux_kill_run_with(&fake_args(&["bar", "--session"]), &mut runner)
            .expect_err("ambiguous session rejected");
        assert!(error.contains("ambiguous"));
        assert_eq!(runner.calls.len(), 1);
    }

    #[test]
    fn tmux_kill_fake_maw_no_delegate_and_no_bun_runtime() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runner = fake_runner();
        let output = tmux_kill_run_with(&fake_args(&["scratch:1.2"]), &mut runner)
            .expect("kill pane by canonical target");
        assert!(output.contains("killed pane %42"));
        assert!(!runner.calls.iter().any(|call| call.subcommand == "bun"));
    }
}
