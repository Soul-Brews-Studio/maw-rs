const DISPATCH_328: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "resize",
        handler: Handler::Sync(resize_run_command),
    },
    DispatcherEntry {
        command: "focus",
        handler: Handler::Sync(focus_run_command),
    },
    DispatcherEntry {
        command: "rename-pane",
        handler: Handler::Sync(rename_pane_run_command),
    },
];

const RESIZE_USAGE: &str = "usage: maw resize <left|right|up|down|equal> [amount]";
const FOCUS_USAGE: &str = "usage: maw focus <target|left|right|up|down>";
const RENAME_USAGE: &str = "usage: maw rename-pane <target> <name>";

type CmdResult = Result<String, (i32, String)>;

fn resize_run_command(argv: &[String]) -> CliOutput {
    cli_result(resize_with_runner(
        argv,
        &mut maw_tmux::CommandTmuxRunner::new(),
    ))
}
fn focus_run_command(argv: &[String]) -> CliOutput {
    cli_result(focus_with_runner(
        argv,
        &mut maw_tmux::CommandTmuxRunner::new(),
    ))
}
fn rename_pane_run_command(argv: &[String]) -> CliOutput {
    cli_result(rename_pane_with_runner(
        argv,
        &mut maw_tmux::CommandTmuxRunner::new(),
    ))
}

fn cli_result(result: CmdResult) -> CliOutput {
    match result {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn resize_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> CmdResult {
    let Some(direction) = argv.first().map(String::as_str) else {
        return Err((2, RESIZE_USAGE.to_owned()));
    };
    if matches!(direction, "--help" | "-h") {
        return Err((0, RESIZE_USAGE.to_owned()));
    }
    if direction == "equal" {
        if argv.len() != 1 {
            return Err((2, RESIZE_USAGE.to_owned()));
        }
        tmux_run(runner, "select-layout", &["tiled".to_owned()], "resize")?;
        return Ok("✓ resized panes equally\n".to_owned());
    }
    if argv.len() > 2 {
        return Err((2, RESIZE_USAGE.to_owned()));
    }
    let flag = resize_flag(direction).ok_or_else(|| (2, RESIZE_USAGE.to_owned()))?;
    let amount = argv
        .get(1)
        .map_or(Ok(1_u16), |value| parse_resize_amount(value))?;
    tmux_run(
        runner,
        "resize-pane",
        &[flag.to_owned(), amount.to_string()],
        "resize",
    )?;
    Ok(format!("✓ resized {direction} {amount}\n"))
}

fn focus_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> CmdResult {
    let Some(raw) = argv.first().map(String::as_str) else {
        return Err((2, FOCUS_USAGE.to_owned()));
    };
    if matches!(raw, "--help" | "-h") {
        return Err((0, FOCUS_USAGE.to_owned()));
    }
    if argv.len() != 1 {
        return Err((2, FOCUS_USAGE.to_owned()));
    }
    if let Some(flag) = pane_dir_flag(raw) {
        tmux_run(runner, "select-pane", &[flag.to_owned()], "focus")?;
        return Ok(format!("✓ focused {raw}\n"));
    }
    validate_target(raw)?;
    let target = resolve_focus_target(runner, raw)?;
    validate_target(&target)?;
    tmux_run(
        runner,
        "select-pane",
        &["-t".to_owned(), target.clone()],
        "focus",
    )?;
    Ok(format!("✓ focused {target}\n"))
}

fn rename_pane_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> CmdResult {
    if argv
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Err((0, RENAME_USAGE.to_owned()));
    }
    if argv.len() != 2 {
        return Err((2, RENAME_USAGE.to_owned()));
    }
    let target = argv[0].as_str();
    let name = argv[1].as_str();
    validate_target(target)?;
    if name.is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return Err((
            2,
            "rename-pane name must be non-empty, unpadded, and not contain control characters"
                .to_owned(),
        ));
    }
    let resolved = resolve_focus_target(runner, target)?;
    tmux_run(
        runner,
        "select-pane",
        &[
            "-t".to_owned(),
            resolved.clone(),
            "-T".to_owned(),
            name.to_owned(),
        ],
        "rename-pane",
    )?;
    Ok(format!("✓ renamed {resolved} to {name}\n"))
}

fn tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[String],
    verb: &str,
) -> Result<(), (i32, String)> {
    runner
        .run(subcommand, args)
        .map(|_| ())
        .map_err(|e| (1, format!("{verb} failed: {}", e.message)))
}

fn resize_flag(value: &str) -> Option<&'static str> {
    match value {
        "left" => Some("-L"),
        "right" => Some("-R"),
        "up" => Some("-U"),
        "down" => Some("-D"),
        _ => None,
    }
}

fn pane_dir_flag(value: &str) -> Option<&'static str> {
    resize_flag(value)
}

fn parse_resize_amount(value: &str) -> Result<u16, (i32, String)> {
    if value.is_empty()
        || value.starts_with('-')
        || value == "--"
        || value.chars().any(char::is_control)
    {
        return Err((2, "resize amount must be a positive integer".to_owned()));
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| (2, "resize amount must be a positive integer".to_owned()))
}

fn resolve_focus_target<R: maw_tmux::TmuxRunner>(runner: &mut R, raw: &str) -> CmdResult {
    if raw.starts_with('%') || raw.chars().all(|ch| ch.is_ascii_digit()) {
        Ok(raw.to_owned())
    } else {
        resolve_local_tmux_runner_target(runner, raw, "focus").map_err(|message| (1, message))
    }
}

fn validate_target(value: &str) -> Result<(), (i32, String)> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err((
            2,
            "tmux target must be non-empty, unpadded, and not start with '-'".to_owned(),
        ));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err((
            2,
            "tmux target must not contain whitespace or control characters".to_owned(),
        ));
    }
    Ok(())
}
