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

