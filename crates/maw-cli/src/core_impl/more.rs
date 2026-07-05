const DISPATCH_324: &[DispatcherEntry] = &[DispatcherEntry {
    command: "more",
    handler: Handler::Sync(run_more_command),
}];

const MORE_USAGE: &str = "usage: maw more codex [N] [--session <s>] [--dry-run] [-e|--engine <e>]\n       maw more status\n\nPlan additional Codex coder lanes.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreCodexOptions {
    count: usize,
    engine: String,
    session: Option<String>,
}

fn run_more_command(argv: &[String]) -> CliOutput {
    match more_run(argv) {
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

fn more_run(argv: &[String]) -> Result<String, String> {
    let Some((subcommand, rest)) = argv.split_first() else {
        return Ok(format!("{MORE_USAGE}\n"));
    };
    match subcommand.as_str() {
        "--help" | "-h" => Ok(format!("{MORE_USAGE}\n")),
        "codex" => more_parse_codex(rest).map(|options| more_render_codex(&options)),
        "status" => more_parse_status(rest),
        value => Err(format!("more: unknown subcommand '{value}'\n{MORE_USAGE}")),
    }
}

fn more_parse_codex(argv: &[String]) -> Result<MoreCodexOptions, String> {
    let mut count = None::<usize>;
    let mut engine = "codex".to_owned();
    let mut session = None::<String>;
    let mut index = 0usize;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--help" | "-h" => return Err(MORE_USAGE.to_owned()),
            "--dry-run" => {}
            "-e" | "--engine" => {
                index += 1;
                engine = more_take_safe_value(argv, index, arg, "engine")?;
            }
            "--session" => {
                index += 1;
                session = Some(more_take_safe_value(argv, index, arg, "session")?);
            }
            value if value.starts_with('-') && more_count_like(value) => {
                more_set_count(&mut count, value)?;
            }
            value if value.starts_with('-') => {
                return Err(format!(
                    "more codex: unknown argument {value}\n{MORE_USAGE}"
                ));
            }
            value => more_set_count(&mut count, value)?,
        }
        index += 1;
    }
    Ok(MoreCodexOptions {
        count: count.unwrap_or(1),
        engine,
        session,
    })
}

fn more_parse_status(argv: &[String]) -> Result<String, String> {
    if argv
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(format!("{MORE_USAGE}\n"));
    }
    if let Some(extra) = argv.first() {
        return Err(format!(
            "more status: unknown argument {extra}\n{MORE_USAGE}"
        ));
    }
    Ok("more status\nlive coders: 0\nsource: live discovery pending\n".to_owned())
}

fn more_take_safe_value(
    argv: &[String],
    index: usize,
    flag: &str,
    label: &str,
) -> Result<String, String> {
    let Some(value) = argv.get(index) else {
        return Err(format!(
            "more codex: {flag} requires a {label}\n{MORE_USAGE}"
        ));
    };
    if value.trim().is_empty() || value.starts_with('-') || value.contains(['\n', '\r']) {
        return Err(format!("more codex: invalid {label} '{value}'"));
    }
    Ok(value.to_owned())
}

fn more_set_count(count: &mut Option<usize>, value: &str) -> Result<(), String> {
    if count.is_some() {
        return Err(format!(
            "more codex: count specified more than once\n{MORE_USAGE}"
        ));
    }
    *count = Some(more_parse_count(value)?);
    Ok(())
}

fn more_count_like(value: &str) -> bool {
    value
        .strip_prefix('-')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

fn more_parse_count(value: &str) -> Result<usize, String> {
    let Ok(parsed) = value.parse::<isize>() else {
        return Err(format!(
            "more codex: N must be a positive integer, got '{value}'"
        ));
    };
    if parsed <= 0 {
        return Err(format!(
            "more codex: N must be a positive integer, got '{value}'"
        ));
    }
    usize::try_from(parsed).map_err(|_| format!("more codex: N is too large: '{value}'"))
}

fn more_render_codex(options: &MoreCodexOptions) -> String {
    let session = options.session.as_deref().unwrap_or("current");
    format!(
        "would spawn {} coders in session {session} with engine {}\n",
        options.count, options.engine
    )
}
