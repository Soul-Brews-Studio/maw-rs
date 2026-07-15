const DISPATCH_266: &[DispatcherEntry] = &[DispatcherEntry {
    command: "swap",
    handler: Handler::Sync(swap_run_command),
}];

const SWAP_USAGE: &str = "usage: maw swap <pane-a> <pane-b>";

fn swap_run_command(argv: &[String]) -> CliOutput {
    if argv
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return CliOutput {
            code: 0,
            stdout: format!("{SWAP_USAGE}\n"),
            stderr: String::new(),
        };
    }
    match swap_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
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

fn swap_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, String> {
    if std::env::var_os("TMUX").is_none() {
        return Err("\x1b[33m⚠\x1b[0m swap requires tmux".to_owned());
    }
    let (left, right) = swap_args(argv)?;
    let mut panes = None::<Vec<PaneRow>>;
    let mut sessions = None::<Vec<RouteSession>>;
    let source = swap_resolve(&left, runner, &mut panes, &mut sessions)?;
    let target = swap_resolve(&right, runner, &mut panes, &mut sessions)?;
    if source == target {
        return Err("swap: source and target are the same pane".to_owned());
    }
    capture_validate_tmux_target(&source)?;
    capture_validate_tmux_target(&target)?;
    runner
        .run(
            "swap-pane",
            &["-s".to_owned(), source, "-t".to_owned(), target],
        )
        .map_err(|error| format!("swap failed: {}", error.message))?;
    Ok(format!("\x1b[32m✓\x1b[0m swapped {left} ↔ {right}\n"))
}

fn swap_args(argv: &[String]) -> Result<(String, String), String> {
    if argv.len() != 2 {
        return Err(format!("{SWAP_USAGE}\ntwo pane targets required"));
    }
    Ok((swap_validate(&argv[0])?, swap_validate(&argv[1])?))
}

fn swap_validate(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!(
            "\"{value}\" looks like a flag, not a pane target.\n  {SWAP_USAGE}"
        ));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("swap target must not contain whitespace or control characters".to_owned());
    }
    if let Some(rest) = value.strip_prefix('%') {
        if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(format!("swap: invalid pane id {value:?}"));
        }
    }
    Ok(value.to_owned())
}

fn swap_resolve<R: maw_tmux::TmuxRunner>(
    spec: &str,
    runner: &mut R,
    panes: &mut Option<Vec<PaneRow>>,
    sessions: &mut Option<Vec<RouteSession>>,
) -> Result<String, String> {
    if spec
        .strip_prefix('%')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Ok(spec.to_owned());
    }
    if spec.chars().all(|ch| ch.is_ascii_digit()) {
        if panes.is_none() {
            *panes = Some(pane_list_rows(&pane_current_anchor()?, runner)?);
        }
        return pane_resolve(spec, panes.as_deref().unwrap_or_default())
            .map(|row| row.pane_id)
            .ok_or_else(|| format!("swap: could not resolve pane index '{spec}'"));
    }
    if sessions.is_none() {
        *sessions = Some(route_sessions_from_tmux_runner(runner, "swap")?);
    }
    let target =
        resolve_local_tmux_target_from_sessions(spec, sessions.as_deref().unwrap_or_default())?;
    capture_validate_tmux_target(&target)?;
    Ok(target)
}
