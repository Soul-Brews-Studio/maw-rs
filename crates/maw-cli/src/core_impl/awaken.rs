const DISPATCH_54: &[DispatcherEntry] = &[DispatcherEntry {
    command: "awaken",
    handler: Handler::Sync(run_awaken_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct AwakenOptions {
    name: String,
    from: Option<String>,
    from_repo: Option<String>,
    stem: Option<String>,
    org: Option<String>,
    repo: Option<String>,
    issue: Option<u64>,
    note: Option<String>,
    nickname: Option<String>,
    trigger: Option<String>,
    no_trigger: bool,
    fast: bool,
    root: bool,
    blank: bool,
    pr: bool,
    split: bool,
    seed: bool,
    dry_run: bool,
    signal_on_birth: bool,
    force: bool,
    track_vault: bool,
    sync_peers: bool,
    parent: Option<String>,
    parent_session_id: Option<String>,
    session_id: Option<String>,
    yes: bool,
}

trait AwakenRunner {
    fn awaken_stdin_is_tty(&mut self) -> bool;
    fn awaken_ask_yes_no(&mut self, question: &str) -> bool;
    fn awaken_run(&mut self, program: &str, args: &[String])
        -> Result<AwakenProcessOutput, String>;
    fn awaken_sleep(&mut self, duration: std::time::Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AwakenProcessOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

struct AwakenSystemRunner;

impl AwakenRunner for AwakenSystemRunner {
    fn awaken_stdin_is_tty(&mut self) -> bool {
        use std::io::IsTerminal as _;
        std::io::stdin().is_terminal()
    }

    fn awaken_ask_yes_no(&mut self, question: &str) -> bool {
        use std::io::{Read as _, Write as _};
        let Ok(mut tty) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
        else {
            return false;
        };
        if tty.write_all(question.as_bytes()).is_err() || tty.flush().is_err() {
            return false;
        }
        let mut buf = [0_u8; 8];
        let Ok(bytes_read) = tty.read(&mut buf) else {
            return false;
        };
        let answer = String::from_utf8_lossy(&buf[..bytes_read])
            .trim()
            .to_ascii_lowercase();
        answer == "y" || answer == "yes"
    }

    fn awaken_run(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<AwakenProcessOutput, String> {
        awaken_validate_exec_name(program)?;
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| {
                let mut message = String::from("awaken: failed to execute ");
                message.push_str(program);
                message.push_str(": ");
                message.push_str(&error.to_string());
                message
            })?;
        Ok(AwakenProcessOutput {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn awaken_sleep(&mut self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }
}

fn run_awaken_command(argv: &[String]) -> CliOutput {
    match awaken_run_with_runner(argv, &mut AwakenSystemRunner) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: awaken_error_line(&error),
        },
    }
}

fn awaken_run_with_runner(
    argv: &[String],
    runner: &mut impl AwakenRunner,
) -> Result<String, String> {
    let options = awaken_parse_args(argv)?;
    let mut stdout = String::new();

    if awaken_prompting_needed(&options, runner) {
        stdout.push_str(&awaken_summarize_plan(&options));
        stdout.push('\n');
        if !runner.awaken_ask_yes_no("  Proceed? [y/N] ") {
            stdout.push_str("  aborted — no changes made.\n");
            return Ok(stdout);
        }
    }

    let trigger = awaken_trigger(&options);
    let bud_args = awaken_bud_args(&options)?;
    let bud = runner.awaken_run("maw", &bud_args)?;
    stdout.push_str(&bud.stdout);
    if bud.code != 0 {
        return Err(awaken_child_error("bud", &bud));
    }

    if options.dry_run {
        if let Some(trigger) = trigger {
            stdout.push_str("  \u{001b}[36m⬡\u{001b}[0m [dry-run] would send \u{001b}[33m");
            stdout.push_str(trigger);
            stdout.push_str("\u{001b}[0m to ");
            stdout.push_str(&options.name);
            stdout.push('\n');
        } else {
            stdout.push_str(
                "  \u{001b}[36m⬡\u{001b}[0m [dry-run] --no-trigger: would NOT fire /awaken\n",
            );
        }
        return Ok(stdout);
    }

    let Some(trigger) = trigger else {
        stdout.push_str(
            "  \u{001b}[90m○\u{001b}[0m --no-trigger: bud + wake done, skipping /awaken\n",
        );
        return Ok(stdout);
    };

    let Some(target) = awaken_resolve_target(&options.name, runner, &mut stdout)? else {
        return Ok(stdout);
    };
    if !awaken_wait_for_agent(&target, runner)? {
        stdout.push_str("  \u{001b}[33m⚠\u{001b}[0m timeout waiting for agent in ");
        stdout.push_str(&target);
        stdout.push_str(" after 10000ms\n");
        stdout.push_str("  \u{001b}[90m  pane may still be in zsh — try manually: maw send-text ");
        stdout.push_str(&options.name);
        stdout.push(' ');
        stdout.push_str(trigger);
        stdout.push_str("\u{001b}[0m\n");
        return Ok(stdout);
    }

    stdout.push_str("  \u{001b}[36m🔔\u{001b}[0m firing \u{001b}[33m");
    stdout.push_str(trigger);
    stdout.push_str("\u{001b}[0m → ");
    stdout.push_str(&options.name);
    stdout.push('\n');

    let send_args = vec![
        "send-text".to_owned(),
        options.name.clone(),
        trigger.to_owned(),
    ];
    let sent = runner.awaken_run("maw", &send_args)?;
    stdout.push_str(&sent.stdout);
    if sent.code == 0 {
        stdout.push_str("  \u{001b}[32m✓\u{001b}[0m awakened\n");
    } else {
        stdout.push_str("  \u{001b}[33m⚠\u{001b}[0m send-text failed: ");
        stdout.push_str(&awaken_child_stderr(&sent));
        stdout.push('\n');
        stdout.push_str("  \u{001b}[90m  try manually: maw send-text ");
        stdout.push_str(&options.name);
        stdout.push(' ');
        stdout.push_str(trigger);
        stdout.push_str("\u{001b}[0m\n");
    }
    Ok(stdout)
}

fn awaken_parse_args(argv: &[String]) -> Result<AwakenOptions, String> {
    let mut options = awaken_default_options();
    let mut positionals = Vec::<String>::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if let Some(consumed) = awaken_parse_option_arg(argv, index, &mut options)? {
            index += consumed;
        } else {
            positionals.push(arg.to_owned());
            index += 1;
        }
    }
    awaken_finalize_parse(options, positionals)
}

fn awaken_default_options() -> AwakenOptions {
    AwakenOptions {
        name: String::new(),
        from: None,
        from_repo: None,
        stem: None,
        org: None,
        repo: None,
        issue: None,
        note: None,
        nickname: None,
        trigger: None,
        no_trigger: false,
        fast: false,
        root: false,
        blank: false,
        pr: false,
        split: false,
        seed: false,
        dry_run: false,
        signal_on_birth: false,
        force: false,
        track_vault: false,
        sync_peers: false,
        parent: None,
        parent_session_id: None,
        session_id: None,
        yes: false,
    }
}

fn awaken_parse_option_arg(
    argv: &[String],
    index: usize,
    options: &mut AwakenOptions,
) -> Result<Option<usize>, String> {
    let Some(arg) = argv.get(index).map(String::as_str) else {
        return Ok(None);
    };
    if matches!(arg, "--help" | "-h") {
        return Err(awaken_usage().to_owned());
    }
    if let Some(consumed) = awaken_parse_value_option(argv, index, options)? {
        return Ok(Some(consumed));
    }
    if awaken_parse_bool_option(arg, options) {
        return Ok(Some(1));
    }
    if arg.starts_with('-') {
        return Err(format!("awaken: unknown argument {arg}"));
    }
    Ok(None)
}

fn awaken_parse_value_option(
    argv: &[String],
    index: usize,
    options: &mut AwakenOptions,
) -> Result<Option<usize>, String> {
    let Some(arg) = argv.get(index).map(String::as_str) else {
        return Ok(None);
    };
    match arg {
        "--from" => options.from = Some(awaken_take_target(argv, index, "--from")?),
        "--from-repo" => options.from_repo = Some(awaken_take_repo(argv, index, "--from-repo")?),
        "--stem" => options.stem = Some(awaken_take_target(argv, index, "--stem")?),
        "--org" => options.org = Some(awaken_take_repo_part(argv, index, "--org")?),
        "--repo" => options.repo = Some(awaken_take_repo(argv, index, "--repo")?),
        "--issue" => options.issue = Some(awaken_take_issue(argv, index, "--issue")?),
        "--note" => options.note = Some(awaken_take_text(argv, index, "--note")?),
        "--nickname" => options.nickname = Some(awaken_take_text(argv, index, "--nickname")?),
        "--trigger" => options.trigger = Some(awaken_take_trigger(argv, index, "--trigger")?),
        "--parent" => options.parent = Some(awaken_take_target(argv, index, "--parent")?),
        "--parent-session-id" => {
            options.parent_session_id =
                Some(awaken_take_target(argv, index, "--parent-session-id")?);
        }
        "--session-id" => {
            options.session_id = Some(awaken_take_target(argv, index, "--session-id")?);
        }
        _ => return awaken_parse_equals_option(arg, options),
    }
    Ok(Some(2))
}

fn awaken_parse_equals_option(
    arg: &str,
    options: &mut AwakenOptions,
) -> Result<Option<usize>, String> {
    if arg.starts_with("--from=") {
        options.from = Some(awaken_value_target(arg, "--from")?);
    } else if arg.starts_with("--from-repo=") {
        options.from_repo = Some(awaken_value_repo(arg, "--from-repo")?);
    } else if arg.starts_with("--stem=") {
        options.stem = Some(awaken_value_target(arg, "--stem")?);
    } else if arg.starts_with("--org=") {
        options.org = Some(awaken_value_repo_part(arg, "--org")?);
    } else if arg.starts_with("--repo=") {
        options.repo = Some(awaken_value_repo(arg, "--repo")?);
    } else if arg.starts_with("--issue=") {
        options.issue = Some(awaken_value_issue(arg, "--issue")?);
    } else if arg.starts_with("--note=") {
        options.note = Some(awaken_value_text(arg, "--note")?);
    } else if arg.starts_with("--nickname=") {
        options.nickname = Some(awaken_value_text(arg, "--nickname")?);
    } else if arg.starts_with("--trigger=") {
        options.trigger = Some(awaken_value_trigger(arg, "--trigger")?);
    } else if arg.starts_with("--parent=") {
        options.parent = Some(awaken_value_target(arg, "--parent")?);
    } else if arg.starts_with("--parent-session-id=") {
        options.parent_session_id = Some(awaken_value_target(arg, "--parent-session-id")?);
    } else if arg.starts_with("--session-id=") {
        options.session_id = Some(awaken_value_target(arg, "--session-id")?);
    } else {
        return Ok(None);
    }
    Ok(Some(1))
}

fn awaken_parse_bool_option(arg: &str, options: &mut AwakenOptions) -> bool {
    match arg {
        "--no-trigger" => options.no_trigger = true,
        "--fast" => options.fast = true,
        "--root" => options.root = true,
        "--blank" => options.blank = true,
        "--pr" => options.pr = true,
        "--split" => options.split = true,
        "--seed" => options.seed = true,
        "--dry-run" => options.dry_run = true,
        "--signal-on-birth" => options.signal_on_birth = true,
        "--force" => options.force = true,
        "--track-vault" => options.track_vault = true,
        "--sync-peers" => options.sync_peers = true,
        "--yes" | "-y" => options.yes = true,
        _ => return false,
    }
    true
}

fn awaken_finalize_parse(
    mut options: AwakenOptions,
    mut positionals: Vec<String>,
) -> Result<AwakenOptions, String> {
    if positionals.len() != 1 {
        return Err(awaken_usage().to_owned());
    }
    options.name = positionals.remove(0);
    awaken_validate_target_arg(&options.name, "oracle name")?;
    Ok(options)
}

fn awaken_usage() -> &'static str {
    "usage: maw awaken <name> [--from <oracle>] [--root] [--seed] [--org <org>] [--repo org/repo] [--issue N] [--note <text>] [--nickname <pretty>] [--fast] [--split] [--dry-run] [--trigger <text>] [--no-trigger] [-y|--yes]"
}

fn awaken_prompting_needed(options: &AwakenOptions, runner: &mut impl AwakenRunner) -> bool {
    !options.yes && !options.dry_run && runner.awaken_stdin_is_tty()
}

fn awaken_summarize_plan(options: &AwakenOptions) -> String {
    let mut lines = Vec::new();
    lines.push("  Will create:".to_owned());
    let mut oracle = String::from("    oracle:  ");
    oracle.push_str(&options.name);
    lines.push(oracle);
    if let Some(repo) = &options.repo {
        let mut line = String::from("    repo:    ");
        line.push_str(repo);
        lines.push(line);
    } else if let Some(org) = &options.org {
        let mut line = String::from("    org:     ");
        line.push_str(org);
        lines.push(line);
    }
    if let Some(from) = &options.from {
        let mut line = String::from("    from:    ");
        line.push_str(from);
        lines.push(line);
    } else if options.root {
        lines.push("    parent:  root (no lineage)".to_owned());
    }
    let mut trigger = String::from("    trigger: ");
    trigger.push_str(awaken_trigger(options).unwrap_or("(none — --no-trigger)"));
    lines.push(trigger);
    if options.fast {
        lines.push("    mode:    fast (skip soul sync)".to_owned());
    }
    if options.seed {
        lines.push("    mode:    seed (new mind)".to_owned());
    }
    if options.blank {
        lines.push("    mode:    blank (no soul)".to_owned());
    }
    if options.split {
        lines.push("    layout:  split pane".to_owned());
    }
    lines.join("\n")
}

fn awaken_trigger(options: &AwakenOptions) -> Option<&str> {
    if options.no_trigger {
        None
    } else {
        Some(options.trigger.as_deref().unwrap_or("/awaken"))
    }
}

fn awaken_bud_args(options: &AwakenOptions) -> Result<Vec<String>, String> {
    let mut args = vec!["bud".to_owned(), options.name.clone()];
    awaken_push_value_arg(&mut args, "--from", options.from.as_deref())?;
    awaken_push_value_arg(&mut args, "--from-repo", options.from_repo.as_deref())?;
    awaken_push_value_arg(&mut args, "--stem", options.stem.as_deref())?;
    awaken_push_value_arg(&mut args, "--org", options.org.as_deref())?;
    awaken_push_value_arg(&mut args, "--repo", options.repo.as_deref())?;
    if let Some(issue) = options.issue {
        args.push("--issue".to_owned());
        args.push(issue.to_string());
    }
    awaken_push_value_arg(&mut args, "--note", options.note.as_deref())?;
    awaken_push_value_arg(&mut args, "--nickname", options.nickname.as_deref())?;
    awaken_push_flag(&mut args, "--fast", options.fast);
    awaken_push_flag(&mut args, "--root", options.root);
    awaken_push_flag(&mut args, "--blank", options.blank);
    awaken_push_flag(&mut args, "--pr", options.pr);
    awaken_push_flag(&mut args, "--split", options.split);
    awaken_push_flag(&mut args, "--seed", options.seed);
    awaken_push_flag(&mut args, "--dry-run", options.dry_run);
    awaken_push_flag(&mut args, "--signal-on-birth", options.signal_on_birth);
    awaken_push_flag(&mut args, "--force", options.force);
    awaken_push_flag(&mut args, "--track-vault", options.track_vault);
    awaken_push_flag(&mut args, "--sync-peers", options.sync_peers);
    awaken_push_value_arg(&mut args, "--parent", options.parent.as_deref())?;
    awaken_push_value_arg(
        &mut args,
        "--parent-session-id",
        options.parent_session_id.as_deref(),
    )?;
    awaken_push_value_arg(&mut args, "--session-id", options.session_id.as_deref())?;
    Ok(args)
}

fn awaken_resolve_target(
    name: &str,
    runner: &mut impl AwakenRunner,
    stdout: &mut String,
) -> Result<Option<String>, String> {
    let args = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        name.to_owned(),
        "#{pane_id}".to_owned(),
    ];
    let resolved = runner.awaken_run("tmux", &args)?;
    if resolved.code == 0 {
        let target = resolved.stdout.trim();
        if awaken_validate_tmux_target(target).is_ok() {
            return Ok(Some(target.to_owned()));
        }
    }
    stdout.push_str("  \u{001b}[33m⚠\u{001b}[0m could not resolve ");
    stdout.push_str(name);
    stdout.push_str(" after wake — skipping /awaken\n");
    stdout.push_str("  \u{001b}[90m  try manually: maw send-text ");
    stdout.push_str(name);
    stdout.push_str(" /awaken\u{001b}[0m\n");
    Ok(None)
}

fn awaken_wait_for_agent(target: &str, runner: &mut impl AwakenRunner) -> Result<bool, String> {
    let args = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        target.to_owned(),
        "#{pane_current_command}".to_owned(),
    ];
    for _ in 0..20 {
        let output = runner.awaken_run("tmux", &args)?;
        if output.code == 0 && awaken_is_agent_command(output.stdout.trim()) {
            return Ok(true);
        }
        runner.awaken_sleep(std::time::Duration::from_millis(500));
    }
    Ok(false)
}

fn awaken_is_agent_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    matches!(lower.as_str(), "claude" | "codex" | "gemini" | "node")
        || lower.contains("claude")
        || lower.contains("codex")
        || lower.contains("gemini")
}

fn awaken_push_flag(args: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        args.push(flag.to_owned());
    }
}

fn awaken_push_value_arg(
    args: &mut Vec<String>,
    flag: &str,
    value: Option<&str>,
) -> Result<(), String> {
    if let Some(value) = value {
        awaken_validate_flag_name(flag)?;
        args.push(flag.to_owned());
        args.push(value.to_owned());
    }
    Ok(())
}

fn awaken_take_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    argv.get(index + 1).map(String::as_str).ok_or_else(|| {
        let mut message = String::from("awaken: ");
        message.push_str(flag);
        message.push_str(" requires a value");
        message
    })
}

fn awaken_value_after_prefix<'a>(value: &'a str, flag: &str) -> Result<&'a str, String> {
    let mut prefix = String::new();
    prefix.push_str(flag);
    prefix.push('=');
    value
        .strip_prefix(&prefix)
        .ok_or_else(|| "awaken: internal parser error".to_owned())
}

fn awaken_take_target(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = awaken_take_value(argv, index, flag)?;
    awaken_validate_target_arg(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_value_target(value: &str, flag: &str) -> Result<String, String> {
    let value = awaken_value_after_prefix(value, flag)?;
    awaken_validate_target_arg(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_take_repo(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = awaken_take_value(argv, index, flag)?;
    awaken_validate_repo_slug(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_value_repo(value: &str, flag: &str) -> Result<String, String> {
    let value = awaken_value_after_prefix(value, flag)?;
    awaken_validate_repo_slug(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_take_repo_part(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = awaken_take_value(argv, index, flag)?;
    awaken_validate_repo_part(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_value_repo_part(value: &str, flag: &str) -> Result<String, String> {
    let value = awaken_value_after_prefix(value, flag)?;
    awaken_validate_repo_part(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_take_issue(argv: &[String], index: usize, flag: &str) -> Result<u64, String> {
    awaken_parse_issue(awaken_take_value(argv, index, flag)?, flag)
}

fn awaken_value_issue(value: &str, flag: &str) -> Result<u64, String> {
    awaken_parse_issue(awaken_value_after_prefix(value, flag)?, flag)
}

fn awaken_take_text(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = awaken_take_value(argv, index, flag)?;
    awaken_validate_text_arg(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_value_text(value: &str, flag: &str) -> Result<String, String> {
    let value = awaken_value_after_prefix(value, flag)?;
    awaken_validate_text_arg(value, flag)?;
    Ok(value.to_owned())
}

fn awaken_take_trigger(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = awaken_take_value(argv, index, flag)?;
    awaken_validate_trigger_arg(value)?;
    Ok(value.to_owned())
}

fn awaken_value_trigger(value: &str, flag: &str) -> Result<String, String> {
    let value = awaken_value_after_prefix(value, flag)?;
    awaken_validate_trigger_arg(value)?;
    Ok(value.to_owned())
}

fn awaken_parse_issue(value: &str, flag: &str) -> Result<u64, String> {
    if value.starts_with('-') {
        return Err("awaken: --issue must not start with '-'".to_owned());
    }
    let issue = value.parse::<u64>().map_err(|_| {
        let mut message = String::from("awaken: ");
        message.push_str(flag);
        message.push_str(" must be a positive integer");
        message
    })?;
    if issue == 0 {
        Err("awaken: --issue must be a positive integer".to_owned())
    } else {
        Ok(issue)
    }
}

fn awaken_validate_exec_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.contains('/')
        || value.chars().any(char::is_control)
    {
        Err("awaken: executable name is not allowed".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_flag_name(value: &str) -> Result<(), String> {
    if !value.starts_with("--") || value.contains('=') || value.chars().any(char::is_control) {
        Err("awaken: invalid internal flag name".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| part == "..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'))
    {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be non-empty, unpadded, not start with '-', contain no '..' segments, and contain only safe target characters");
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        Err("awaken: tmux target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_repo_slug(value: &str, label: &str) -> Result<(), String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be org/repo");
        return Err(message);
    }
    awaken_validate_repo_part(parts[0], label)?;
    awaken_validate_repo_part(parts[1], label)
}

fn awaken_validate_repo_part(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value == "."
        || value == ".."
        || value.chars().any(char::is_control)
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        let mut message = String::from("awaken: invalid ");
        message.push_str(label);
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_text_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be non-empty single-line text");
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_trigger_arg(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        Err("awaken: --trigger must be non-empty single-line text".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_child_error(action: &str, output: &AwakenProcessOutput) -> String {
    let mut message = String::from("awaken: maw ");
    message.push_str(action);
    message.push_str(" failed: ");
    message.push_str(&awaken_child_stderr(output));
    message
}

fn awaken_child_stderr(output: &AwakenProcessOutput) -> String {
    let stderr = output.stderr.trim();
    if stderr.is_empty() {
        let mut message = String::from("exit code ");
        message.push_str(&output.code.to_string());
        message
    } else {
        stderr.to_owned()
    }
}

fn awaken_error_line(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        let mut line = error.to_owned();
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod awaken_tests {
    use super::*;

    #[derive(Default)]
    struct AwakenFakeRunner {
        tty: bool,
        answer: bool,
        calls: Vec<(String, Vec<String>)>,
        outputs: Vec<AwakenProcessOutput>,
        sleeps: usize,
    }

    impl AwakenRunner for AwakenFakeRunner {
        fn awaken_stdin_is_tty(&mut self) -> bool {
            self.tty
        }
        fn awaken_ask_yes_no(&mut self, _question: &str) -> bool {
            self.answer
        }
        fn awaken_run(
            &mut self,
            program: &str,
            args: &[String],
        ) -> Result<AwakenProcessOutput, String> {
            self.calls.push((program.to_owned(), args.to_vec()));
            if self.outputs.is_empty() {
                return Ok(AwakenProcessOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            Ok(self.outputs.remove(0))
        }
        fn awaken_sleep(&mut self, _duration: std::time::Duration) {
            self.sleeps += 1;
        }
    }

    fn awaken_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn awaken_ok(stdout: &str) -> AwakenProcessOutput {
        AwakenProcessOutput {
            code: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    #[test]
    fn awaken_parse_real_flags_and_builds_bud_args_without_trigger() {
        let options = awaken_parse_args(&awaken_strings(&[
            "nova",
            "--from",
            "wish",
            "--repo",
            "tonkmac/maw-rs",
            "--issue",
            "132",
            "--trigger",
            "/awaken --fast",
            "--fast",
            "--split",
            "--dry-run",
            "--sync-peers",
            "--track-vault",
            "--yes",
        ]))
        .expect("parse");
        assert_eq!(options.name, "nova");
        assert_eq!(options.trigger.as_deref(), Some("/awaken --fast"));
        let bud_args = awaken_bud_args(&options).expect("bud args");
        assert_eq!(
            bud_args,
            awaken_strings(&[
                "bud",
                "nova",
                "--from",
                "wish",
                "--repo",
                "tonkmac/maw-rs",
                "--issue",
                "132",
                "--fast",
                "--split",
                "--dry-run",
                "--track-vault",
                "--sync-peers",
            ])
        );
    }

    #[test]
    fn awaken_dry_run_is_hermetic_and_matches_golden_without_real_env() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![awaken_ok("bud plan\n")],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(
            &awaken_strings(&["nova", "--dry-run", "--trigger", "/awaken", "--yes"]),
            &mut runner,
        )
        .expect("run");
        assert_eq!(
            output,
            include_str!("../../tests/fixtures/native-awaken/awaken-dry-run.stdout")
        );
        assert_eq!(
            runner.calls,
            vec![(
                "maw".to_owned(),
                awaken_strings(&["bud", "nova", "--dry-run"])
            )]
        );
    }

    #[test]
    fn awaken_non_dry_run_waits_for_agent_then_sends_trigger() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                awaken_ok("%12\n"),
                awaken_ok("zsh\n"),
                awaken_ok("claude\n"),
                awaken_ok(""),
            ],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(
            &awaken_strings(&["nova", "--yes", "--no-trigger"]),
            &mut runner,
        )
        .expect("run");
        assert!(output.contains("--no-trigger"));
        assert_eq!(runner.calls.len(), 1);

        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                awaken_ok("%12\n"),
                awaken_ok("zsh\n"),
                awaken_ok("claude\n"),
                awaken_ok("sent\n"),
            ],
            ..AwakenFakeRunner::default()
        };
        let output =
            awaken_run_with_runner(&awaken_strings(&["nova", "--yes"]), &mut runner).expect("run");
        assert!(output.contains("awakened"));
        assert_eq!(runner.sleeps, 1);
        assert_eq!(
            runner.calls[4],
            (
                "maw".to_owned(),
                awaken_strings(&["send-text", "nova", "/awaken"])
            )
        );
    }

    #[test]
    fn awaken_unresolved_target_returns_warning_success_like_js() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                AwakenProcessOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "no target".to_owned(),
                },
            ],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(&awaken_strings(&["nova", "--yes"]), &mut runner)
            .expect("warning success");
        assert!(output.contains("could not resolve nova"));
        assert_eq!(runner.calls.len(), 2);
    }

    #[test]
    fn awaken_option_injection_guard_blocks_exec_path_and_target_values() {
        assert!(awaken_validate_exec_name("/bin/maw").is_err());
        assert!(awaken_validate_exec_name("-maw").is_err());
        assert!(awaken_validate_target_arg("-nova", "oracle name").is_err());
        assert!(awaken_validate_target_arg("../nova", "oracle name").is_err());
        assert!(awaken_parse_args(&awaken_strings(&["--repo", "-bad/repo", "nova"])).is_err());
        assert!(awaken_parse_args(&awaken_strings(&["-bad"])).is_err());
    }

    #[test]
    fn awaken_dispatcher_is_native() {
        assert_eq!(dispatcher_status("awaken"), DispatchKind::Native);
    }
}
