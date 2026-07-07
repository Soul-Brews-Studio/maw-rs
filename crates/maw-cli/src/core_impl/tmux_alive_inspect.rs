const DISPATCH_268: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "alive",
        handler: Handler::Sync(alive_run_command),
    },
    DispatcherEntry {
        command: "inspect",
        handler: Handler::Sync(inspect_run_command),
    },
];

const ALIVE_USAGE: &str = "usage: maw alive [--json] <session|name>";
const INSPECT_USAGE: &str = "usage: maw inspect [--json] <pane>";
const INSPECT_FORMAT: &str =
    "#{pane_current_command}|||#{pane_pid}|||#{pane_current_path}|||#{pane_title}";
type InspectTuple = (String, String, String, String);

fn alive_run_command(argv: &[String]) -> CliOutput {
    if wants_help(argv, &[]) {
        return help_output(ALIVE_USAGE);
    }
    aliveinspect_output(alive_with_runner(
        argv,
        &mut maw_tmux::CommandTmuxRunner::new(),
    ))
}

fn inspect_run_command(argv: &[String]) -> CliOutput {
    if wants_help(argv, &[]) {
        return help_output(INSPECT_USAGE);
    }
    aliveinspect_output(inspect_with_runner(
        argv,
        &mut maw_tmux::CommandTmuxRunner::new(),
    ))
}

fn aliveinspect_output(result: Result<CliOutput, (i32, String)>) -> CliOutput {
    result.unwrap_or_else(|(code, message)| CliOutput {
        code,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    })
}

fn alive_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, (i32, String)> {
    let (json, query) = aliveinspect_parse(argv, ALIVE_USAGE, "alive")?;
    aliveinspect_validate_target(&query, "alive")?;
    if query.starts_with('%') {
        return Err((
            1,
            "alive expects a session or oracle name, not a pane id".to_owned(),
        ));
    }
    let sessions = match route_sessions_from_tmux_runner(runner, "alive") {
        Ok(sessions) => sessions,
        Err(error) if json => return Ok(alive_dead_json(&query, &error)),
        Err(error) => return Err((1, error)),
    };
    let target = match resolve_local_tmux_target_from_sessions(&query, &sessions) {
        Ok(target) => target,
        Err(error) => return Ok(alive_dead(&query, &error, json)),
    };
    capture_validate_tmux_target(&target).map_err(|message| (1, message))?;
    let session = alive_session_from_target(&target).to_owned();
    capture_validate_tmux_target(&session).map_err(|message| (1, message))?;
    let panes = alive_count_panes(runner, &session).map_err(|message| (1, message))?;
    let stdout = if json {
        format!(
            "{{\"alive\":true,\"session\":{},\"panes\":{panes}}}\n",
            json_string(&session)
        )
    } else {
        format!(
            "alive: {session} ({panes} pane{})\n",
            if panes == 1 { "" } else { "s" }
        )
    };
    Ok(CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    })
}

fn inspect_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, (i32, String)> {
    let (json, query) = aliveinspect_parse(argv, INSPECT_USAGE, "inspect")?;
    aliveinspect_validate_target(&query, "inspect")?;
    let target = resolve_local_tmux_runner_target(runner, &query, "inspect")
        .map_err(|message| (1, message))?;
    capture_validate_tmux_target(&target).map_err(|message| (1, message))?;
    let info = inspect_read_pane(runner, &target).map_err(|message| (1, message))?;
    let stdout = if json {
        inspect_json(&info)
    } else {
        format!(
            "command: {}   pid: {}   cwd: {}   title: {}\n",
            info.0, info.1, info.2, info.3
        )
    };
    Ok(CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    })
}

fn aliveinspect_parse(
    argv: &[String],
    usage: &str,
    command: &str,
) -> Result<(bool, String), (i32, String)> {
    let mut json = false;
    let mut target = None;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--" => return Err((2, format!("{command}: -- separator is not supported"))),
            value if value.starts_with('-') => {
                return Err((2, format!("{command}: unknown flag '{value}'\n  {usage}")))
            }
            value if target.is_none() => target = Some(value.to_owned()),
            _ => {
                return Err((
                    2,
                    format!("{command}: expected exactly one target\n  {usage}"),
                ))
            }
        }
    }
    target
        .map(|target| (json, target))
        .ok_or_else(|| (2, usage.to_owned()))
}

fn aliveinspect_validate_target(target: &str, command: &str) -> Result<(), (i32, String)> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') || target == "--" {
        return Err((
            1,
            format!("{command} target must be non-empty, unpadded, and not start with '-'"),
        ));
    }
    if target
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err((
            1,
            format!("{command} target must not contain whitespace or control characters"),
        ));
    }
    Ok(())
}

fn alive_dead(query: &str, error: &str, json: bool) -> CliOutput {
    if json {
        alive_dead_json(query, error)
    } else {
        CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("not alive: {query} ({error})\n"),
        }
    }
}

fn alive_dead_json(query: &str, error: &str) -> CliOutput {
    CliOutput {
        code: 1,
        stdout: format!(
            "{{\"alive\":false,\"query\":{},\"error\":{}}}\n",
            json_string(query),
            json_string(error)
        ),
        stderr: String::new(),
    }
}

fn alive_session_from_target(target: &str) -> &str {
    target
        .split_once(':')
        .map_or(target, |(session, _)| session)
}

fn alive_count_panes<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    session: &str,
) -> Result<usize, String> {
    let raw = runner
        .run(
            "list-panes",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}|||#{pane_id}".to_owned(),
            ],
        )
        .map_err(|error| format!("alive pane count failed: {}", error.message))?;
    Ok(raw
        .lines()
        .filter_map(|line| line.split_once("|||"))
        .filter(|(name, pane_id)| *name == session && !pane_id.trim().is_empty())
        .count())
}

fn inspect_read_pane<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
) -> Result<InspectTuple, String> {
    let raw = runner
        .run(
            "display-message",
            &[
                "-p".to_owned(),
                "-t".to_owned(),
                target.to_owned(),
                INSPECT_FORMAT.to_owned(),
            ],
        )
        .map_err(|error| format!("inspect failed: {}", error.message))?;
    let mut parts = raw.trim_end_matches('\n').splitn(4, "|||");
    Ok((
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or_default().to_owned(),
    ))
}

fn inspect_json(info: &InspectTuple) -> String {
    format!(
        "{{\"command\":{},\"pid\":{},\"cwd\":{},\"title\":{}}}\n",
        json_string(&info.0),
        info.1
            .parse::<u64>()
            .map_or_else(|_| "null".to_owned(), |pid| pid.to_string()),
        json_string(&info.2),
        json_string(&info.3)
    )
}
