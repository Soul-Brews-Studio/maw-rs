fn parse_date_parts(value: &str) -> Result<DateParts, String> {
    let Some((date, time)) = value.split_once('T') else {
        return Err("calver: --now must use YYYY-M-DTHH:MM".to_owned());
    };
    let mut date_parts = date.split('-');
    let year = parse_i32_part(date_parts.next(), "year")?;
    let month = parse_u32_part(date_parts.next(), "month")?;
    let day = parse_u32_part(date_parts.next(), "day")?;
    if date_parts.next().is_some() {
        return Err("calver: --now date must use YYYY-M-D".to_owned());
    }

    let mut time_parts = time.split(':');
    let hour = parse_u32_part(time_parts.next(), "hour")?;
    let minute = parse_u32_part(time_parts.next(), "minute")?;
    if time_parts.next().is_some() {
        return Err("calver: --now time must use HH:MM".to_owned());
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return Err("calver: --now contains out-of-range date/time parts".to_owned());
    }
    Ok(DateParts {
        year,
        month,
        day,
        hour,
        minute,
    })
}

fn parse_i32_part(value: Option<&str>, name: &str) -> Result<i32, String> {
    let Some(value) = value else {
        return Err(format!("calver: missing {name} in --now"));
    };
    value
        .parse::<i32>()
        .map_err(|_| format!("calver: invalid {name} in --now"))
}

fn parse_u32_part(value: Option<&str>, name: &str) -> Result<u32, String> {
    let Some(value) = value else {
        return Err(format!("calver: missing {name} in --now"));
    };
    value
        .parse::<u32>()
        .map_err(|_| format!("calver: invalid {name} in --now"))
}

fn render_calver_plan_json(
    args: ComputeArgs,
    tags: &[String],
    package_version: &str,
    version: &str,
) -> String {
    let mut arg_fields = vec![
        format!("\"stable\":{}", args.stable),
        format!("\"now\":{}", render_date_parts_json(args.now)),
    ];
    if let Some(channel) = args.channel {
        arg_fields.push(format!("\"channel\":{}", json_string(channel.as_str())));
    }
    format!(
        "{{\"command\":\"calver\",\"args\":{{{}}},\"tags\":{},\"packageVersion\":{},\"version\":{}}}\n",
        arg_fields.join(","),
        json_string_array(tags),
        json_string(package_version),
        json_string(version)
    )
}

fn render_date_parts_json(now: DateParts) -> String {
    format!(
        "{{\"year\":{},\"month\":{},\"day\":{},\"hour\":{},\"minute\":{}}}",
        now.year, now.month, now.day, now.hour, now.minute
    )
}

fn run_normalize_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_normalize_constants_plan(&argv[1..]);
    }

    let plan_json = argv.iter().any(|arg| arg == "--plan-json");
    let target = argv.iter().find(|arg| arg.as_str() != "--plan-json");
    let Some(target) = target else {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr:
                "normalize: expected <target>\nusage: maw-rs normalize <target> [--plan-json]\nusage: maw-rs normalize constants [--plan-json]\n"
                    .to_owned(),
        };
    };
    let normalized = normalize_target(target);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"normalize\",\"input\":{},\"normalized\":{}}}\n",
                json_string(target),
                json_string(&normalized)
            )
        } else {
            format!("{normalized}\n")
        },
        stderr: String::new(),
    }
}

fn run_normalize_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return normalize_constants_usage_error(&format!(
                    "normalize constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_normalize_constants_json()
        } else {
            "normalize constants steps=trim,strip-trailing-slashes,strip-trailing-dot-git-until-stable\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_normalize_constants_json() -> String {
    concat!(
        "{\"command\":\"normalize\",\"kind\":\"constants\",",
        "\"steps\":[\"trim\",\"strip-trailing-slashes\",\"strip-trailing-dot-git-until-stable\"],",
        "\"preserves\":[\"interior characters\",\"case\",\"suffix text named .git without slash-dot\"],",
        "\"emptyBehavior\":{\"empty\":\"empty\",\"whitespaceOnly\":\"empty\"},",
        "\"fixtureCount\":12}\n"
    )
    .to_owned()
}

fn normalize_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs normalize constants [--plan-json]\n"),
    }
}

fn run_resolve_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_resolve_constants_plan(&argv[1..]);
    }

    let plan_json = argv.iter().any(|arg| arg == "--plan-json");
    let mut mode = "by-name".to_owned();
    let mut positionals = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => {}
            "--mode" => {
                let Some(value) = argv.get(index + 1) else {
                    return resolve_usage_error("resolve: missing --mode value");
                };
                mode.clone_from(value);
                index += 1;
            }
            arg => positionals.push(arg.to_owned()),
        }
        index += 1;
    }

    if positionals.len() < 2 {
        return resolve_usage_error("resolve: expected <target> and at least one item");
    }
    let target = &positionals[0];
    let items = &positionals[1..];
    let result = match mode.as_str() {
        "by-name" | "byName" => resolve_by_name(target, items, ResolveOptions::default()),
        "session" => resolve_session_target(target, items),
        "worktree" => resolve_worktree_target(target, items),
        _ => return resolve_usage_error("resolve: unknown --mode"),
    };
    let stdout = if plan_json {
        render_resolve_plan_json(&mode, target, result)
    } else {
        render_resolve_plan_text(&mode, target, result)
    };
    CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn resolve_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs resolve --mode <by-name|session|worktree> <target> <item...> [--plan-json]\nusage: maw-rs resolve constants [--plan-json]\n"),
    }
}

fn run_resolve_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return resolve_constants_usage_error(&format!(
                    "resolve constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_resolve_constants_json()
        } else {
            "resolve constants modes=by-name,session,worktree results=exact,fuzzy,ambiguous,none\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_resolve_constants_json() -> String {
    concat!(
        "{\"command\":\"resolve\",\"kind\":\"constants\",",
        "\"modes\":[\"by-name\",\"session\",\"worktree\"],\"modeAliases\":{\"byName\":\"by-name\"},",
        "\"resultKinds\":[\"exact\",\"fuzzy\",\"ambiguous\",\"none\"],",
        "\"matchLadder\":[\"trim-lowercase-target\",\"case-insensitive-exact\",\"suffix-segment\",\"prefix-or-middle-segment\",\"substring-hints-only\"],",
        "\"modeRules\":{\"session\":{\"fleetSessions\":true,\"numericPrefixBlocksPrefixMiddle\":true},\"worktree\":{\"fleetSessions\":false,\"numericPrefixesAreSequenceCounters\":true},\"by-name\":{\"fleetSessions\":false}},",
        "\"noneBehavior\":{\"emptyTarget\":\"none-no-hints\",\"substringFallback\":\"none-with-hints-never-fuzzy\"},",
        "\"fixtureCounts\":{\"total\":16,\"byName\":12,\"session\":3,\"worktree\":1,\"exact\":2,\"fuzzy\":7,\"ambiguous\":3,\"none\":4}}\n"
    )
    .to_owned()
}

fn resolve_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs resolve constants [--plan-json]\n"),
    }
}

fn render_resolve_plan_json(mode: &str, target: &str, result: ResolveResult<String>) -> String {
    let mut fields = vec![
        "\"command\":\"resolve\"".to_owned(),
        format!("\"mode\":{}", json_string(mode)),
        format!("\"target\":{}", json_string(target)),
    ];
    match result {
        ResolveResult::Exact { matched } => {
            fields.push("\"kind\":\"exact\"".to_owned());
            fields.push(format!("\"match\":{}", json_string(&matched)));
        }
        ResolveResult::Fuzzy { matched } => {
            fields.push("\"kind\":\"fuzzy\"".to_owned());
            fields.push(format!("\"match\":{}", json_string(&matched)));
        }
        ResolveResult::Ambiguous { candidates } => {
            fields.push("\"kind\":\"ambiguous\"".to_owned());
            fields.push(format!("\"candidates\":{}", json_string_array(&candidates)));
        }
        ResolveResult::None { hints } => {
            fields.push("\"kind\":\"none\"".to_owned());
            if let Some(hints) = hints {
                fields.push(format!("\"hints\":{}", json_string_array(&hints)));
            }
        }
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_resolve_plan_text(mode: &str, target: &str, result: ResolveResult<String>) -> String {
    match result {
        ResolveResult::Exact { matched } => {
            format!("resolve {mode} {target}: exact {matched}\n")
        }
        ResolveResult::Fuzzy { matched } => {
            format!("resolve {mode} {target}: fuzzy {matched}\n")
        }
        ResolveResult::Ambiguous { candidates } => {
            format!(
                "resolve {mode} {target}: ambiguous {}\n",
                candidates.join(", ")
            )
        }
        ResolveResult::None { hints } => hints.map_or_else(
            || format!("resolve {mode} {target}: none\n"),
            |hints| format!("resolve {mode} {target}: none hints={}\n", hints.join(", ")),
        ),
    }
}

fn json_string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_str_array(values: &[&str]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn usage_ok() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: usage_text(),
        stderr: String::new(),
    }
}

fn usage_text() -> String {
    concat!(
        "usage: maw-rs <command> [args]\n",
        "ported commands:\n",
        "  a|attach <target> [--print] [--readonly|-r]   attach to a tmux session\n",
        "  run <target> <cmd...>                         type text and press Enter\n",
        "  send-enter <target> [--N <count>]             send Enter to a tmux target\n",
        "  ls [--compact|-c] [--verbose|-v] [--json]     list live local sessions\n",
        "  plugin ls [-v|--verbose]                      list installed plugins\n",
        "  bring|b <oracle> [--to <session[:window]>]    plan a wake split target\n",
        "  feed active|parse-line|describe                inspect local activity feed data\n",
        "\n",
        "portable parity commands are intentionally hidden from the default menu until their live UX ships.\n",
    )
    .to_owned()
}


#[derive(Debug, Clone, PartialEq, Eq)]
struct LsPanePlan {
    id: String,
    target: String,
    session: String,
    command: String,
    title: String,
    source: Option<String>,
    last_activity: Option<u64>,
    session_created: Option<u64>,
    status: &'static str,
    age_sec: u64,
    agent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LsMode {
    Compact,
    Verbose,
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
struct LsPlanOptions {
    json: bool,
    mode: LsMode,
    all: bool,
    channels: bool,
    active: bool,
    active_threshold_sec: Option<u64>,
    recent: bool,
    recent_limit: Option<usize>,
    filter: Option<String>,
    peer: Option<String>,
    federation: bool,
    node: Option<String>,
    fleet_only: bool,
    teams: bool,
    verify: bool,
    fix: bool,
    now: Option<u64>,
    panes: Vec<TmuxPane>,
    session_created: BTreeMap<String, u64>,
}

fn run_ls_plan(argv: &[String]) -> CliOutput {
    match parse_ls_plan_options(argv) {
        Ok(options) => render_ls_plan(&options),
        Err(output) => output,
    }
}

#[allow(clippy::too_many_lines)]
fn parse_ls_plan_options(argv: &[String]) -> Result<LsPlanOptions, CliOutput> {
    let mut options = LsPlanOptions {
        json: false,
        mode: LsMode::Compact,
        all: false,
        channels: false,
        active: false,
        active_threshold_sec: None,
        recent: false,
        recent_limit: None,
        filter: None,
        peer: None,
        federation: false,
        node: None,
        fleet_only: false,
        teams: true,
        verify: false,
        fix: false,
        now: None,
        panes: Vec::new(),
        session_created: BTreeMap::new(),
    };

    let mut positionals = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(ls_help_ok()),
            "--json" | "--plan-json" => options.json = true,
            "--all" => options.all = true,
            "--compact" | "-c" => options.mode = LsMode::Compact,
            "--verbose" | "-v" => options.mode = LsMode::Verbose,
            "--channels" => options.channels = true,
            "--federation" => options.federation = true,
            "--fleet-only" => options.fleet_only = true,
            "--no-teams" => options.teams = false,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(ls_usage_error("✗ maw ls: --node requires a value"));
                };
                match ls_validate_value(value, "--node") {
                    Ok(()) => options.node = Some(value.trim().to_owned()),
                    Err(message) => return Err(ls_usage_error(&message)),
                }
                index += 1;
            }
            arg if arg.starts_with("--node=") => {
                let value = &arg["--node=".len()..];
                match ls_validate_value(value, "--node") {
                    Ok(()) => options.node = Some(value.trim().to_owned()),
                    Err(message) => return Err(ls_usage_error(&message)),
                }
            }
            "--active" => {
                options.active = true;
                if let Some(next) = argv.get(index + 1) {
                    if !next.starts_with('-') {
                        if let Some(seconds) = parse_ls_duration_seconds(next) {
                            options.active_threshold_sec = Some(seconds);
                            index += 1;
                        }
                    }
                }
            }
            arg if arg.starts_with("--active=") => {
                options.active = true;
                let raw = &arg["--active=".len()..];
                let Some(seconds) = parse_ls_duration_seconds(raw) else {
                    return Err(ls_usage_error("ls: invalid --active duration"));
                };
                options.active_threshold_sec = Some(seconds);
            }
            "--recent" | "-r" => {
                options.recent = true;
                options.all = true;
                if let Some(next) = argv.get(index + 1) {
                    if !next.starts_with('-') {
                        if let Ok(limit) = next.parse::<usize>() {
                            options.recent_limit = Some(limit);
                            index += 1;
                        }
                    }
                }
            }
            "--pane" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(ls_usage_error("ls: missing --pane value"));
                };
                match parse_ls_pane(value) {
                    Ok(pane) => options.panes.push(pane),
                    Err(message) => return Err(ls_usage_error(&message)),
                }
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(ls_usage_error("ls: missing --now value"));
                };
                match value.parse::<u64>() {
                    Ok(value) => options.now = Some(value),
                    Err(_) => return Err(ls_usage_error("ls: --now must be an integer")),
                }
                index += 1;
            }
            "--session-created" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(ls_usage_error("ls: missing --session-created value"));
                };
                let Some((session, created)) = value.split_once('=') else {
                    return Err(ls_usage_error(
                        "ls: --session-created must use <session=epoch_seconds>",
                    ));
                };
                match created.parse::<u64>() {
                    Ok(created) => {
                        options.session_created.insert(session.to_owned(), created);
                    }
                    Err(_) => {
                        return Err(ls_usage_error(
                            "ls: session-created epoch must be an integer",
                        ));
                    }
                }
                index += 1;
            }
            "--verify" => options.verify = true,
            "--fix" => options.fix = true,
            "-a" => {
                options.all = true;
            }
            arg if arg.starts_with('-') => {
                return Err(ls_usage_error(&format!("ls: unknown argument {arg}")));
            }
            arg => {
                if let Err(message) = ls_validate_value(arg, "filter") {
                    return Err(ls_usage_error(&message));
                }
                positionals.push(arg.to_owned());
            }
        }
        index += 1;
    }

    if let Some(first) = positionals.first() {
        if options.federation || (!options.active && options.panes.is_empty()) {
            options.peer = Some(first.clone());
        } else {
            options.filter = Some(first.clone());
        }
    }

    if let Some(node) = &options.node {
        options.filter = Some(node.clone());
    }

    if options.federation {
        options.all = true;
    }

    Ok(options)
}
