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

