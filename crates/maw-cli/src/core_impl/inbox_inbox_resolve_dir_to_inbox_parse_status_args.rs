fn inbox_resolve_dir(config: &serde_json::Value) -> std::path::PathBuf {
    if let Some(psi) = config.get("psiPath").and_then(serde_json::Value::as_str) {
        return std::path::Path::new(psi).join("inbox");
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let unicode = cwd.join("ψ").join("inbox");
    if unicode.exists() {
        unicode
    } else {
        cwd.join("psi").join("inbox")
    }
}

fn inbox_run_list(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let options = inbox_parse_list_args(argv)?;
    let mut messages = inbox_load_messages(&env.inbox_dir)?;
    if options.unread {
        messages.retain(|message| !message.read);
    }
    if let Some(from) = &options.from {
        messages.retain(|message| &message.from == from);
    }
    Ok(inbox_render_list(
        &messages,
        options.last.unwrap_or(20),
        now_ms,
    ))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InboxListOptions {
    unread: bool,
    from: Option<String>,
    last: Option<usize>,
}

fn inbox_parse_list_args(argv: &[String]) -> Result<InboxListOptions, String> {
    let mut options = InboxListOptions::default();
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--unread" => options.unread = true,
            "--from" => {
                let value = inbox_required_value(argv, index, "--from")?;
                inbox_validate_target_arg(value, "from")?;
                options.from = Some(value.to_owned());
                index += 1;
            }
            "--last" => {
                let value = inbox_required_value(argv, index, "--last")?;
                options.last = Some(inbox_parse_usize(value, "--last")?);
                index += 1;
            }
            value if value.starts_with("--from=") => {
                let value = value.trim_start_matches("--from=");
                inbox_validate_target_arg(value, "from")?;
                options.from = Some(value.to_owned());
            }
            value if value.starts_with("--last=") => {
                options.last = Some(inbox_parse_usize(
                    value.trim_start_matches("--last="),
                    "--last",
                )?);
            }
            value if value.starts_with('-') => {
                return Err(format!("inbox: unknown argument {value}"))
            }
            value => return Err(format!("inbox: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(options)
}

fn inbox_render_list(messages: &[InboxMessage], limit: usize, now_ms: u64) -> String {
    if messages.is_empty() {
        return "\u{001b}[90mno inbox messages\u{001b}[0m\n".to_owned();
    }
    let mut out = format!(
        "\n\u{001b}[36mINBOX\u{001b}[0m ({} total)\n\n",
        messages.len()
    );
    out.push_str("  R FROM           WHEN       SUBJECT\n");
    out.push_str("  - -------------- ---------- --------------------------------------------\n");
    for message in messages.iter().take(limit) {
        inbox_render_list_row(&mut out, message, now_ms);
    }
    out.push('\n');
    out
}

fn inbox_render_list_row(out: &mut String, message: &InboxMessage, now_ms: u64) {
    let dot = if message.read {
        "\u{001b}[90m○\u{001b}[0m"
    } else {
        "\u{001b}[32m●\u{001b}[0m"
    };
    let from = inbox_pad(&inbox_truncate(&message.from, 14), 14);
    let when = inbox_pad(&inbox_relative_time(message.timestamp_ms, now_ms), 10);
    let subject = inbox_truncate(&message.body.replace('\n', " "), 50);
    let _ = writeln!(out, "  {dot} {from} {when} {subject}");
}

fn inbox_run_mark_read(argv: &[String], env: &InboxEnv) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox read <id>")?;
    let Some(message) = inbox_find_message(&env.inbox_dir, id)? else {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: message not found: {id}\n"
        ));
    };
    if message.read {
        return Ok(format!(
            "\u{001b}[90malready read:\u{001b}[0m {}\n",
            message.filename
        ));
    }
    let content = std::fs::read_to_string(&message.path)
        .map_err(|error| format!("inbox: read {}: {error}", message.path.display()))?;
    let updated = inbox_mark_frontmatter_read(&content, inbox_now_ms());
    if updated == content {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: could not mark read: {}\n",
            message.filename
        ));
    }
    std::fs::write(&message.path, updated)
        .map_err(|error| format!("inbox: write {}: {error}", message.path.display()))?;
    Ok(format!(
        "\u{001b}[32m✓\u{001b}[0m marked read: {}\n",
        message.filename
    ))
}

fn inbox_run_show(argv: &[String], env: &InboxEnv) -> Result<String, String> {
    if argv.len() > 1 {
        return Err("usage: maw inbox show [N|name]".to_owned());
    }
    if let Some(value) = argv.first() {
        inbox_validate_lookup_arg(value, "message")?;
    }
    let messages = inbox_load_messages(&env.inbox_dir)?;
    if messages.is_empty() {
        return Ok("\u{001b}[90mno inbox messages\u{001b}[0m\n".to_owned());
    }
    let target = argv.first().map(String::as_str);
    let Some(message) = inbox_pick_message(&messages, target) else {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: not found: {}\n",
            target.unwrap_or_default()
        ));
    };
    Ok(inbox_render_show(message))
}

fn inbox_run_write(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let note = inbox_parse_write_note(argv)?;
    if !env.inbox_dir.exists() {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: inbox not found: {}\n",
            env.inbox_dir.display()
        ));
    }
    let filename = inbox_write_file(&env.inbox_dir, &env.node, &env.node, &note, now_ms)?;
    Ok(format!(
        "\u{001b}[32m✓\u{001b}[0m wrote \u{001b}[33m{filename}\u{001b}[0m\n"
    ))
}

fn inbox_parse_write_note(argv: &[String]) -> Result<String, String> {
    let mut note_args = argv;
    if note_args.first().is_some_and(|arg| arg == "--") {
        note_args = &note_args[1..];
    } else if note_args.first().is_some_and(|arg| arg.starts_with('-')) {
        return Err("inbox: write message starting with '-' requires -- separator".to_owned());
    }
    if note_args.is_empty() {
        return Err("usage: maw inbox write <msg>".to_owned());
    }
    Ok(note_args.join(" "))
}

fn inbox_run_status(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let (oracle, json, all) = inbox_parse_status_args(argv)?;
    if all {
        let status = inbox_build_status(&env.oracle, &env.inbox_dir, env, now_ms)?;
        let statuses = vec![status];
        return inbox_render_status_list(&statuses, json);
    }
    let oracle = oracle.unwrap_or_else(|| env.oracle.clone());
    let status = inbox_build_status(&oracle, &env.inbox_dir, env, now_ms)?;
    inbox_render_status(&status, json)
}

fn inbox_parse_status_args(argv: &[String]) -> Result<(Option<String>, bool, bool), String> {
    let mut oracle = None::<String>;
    let mut json = false;
    let mut all = false;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            value if value.starts_with('-') => {
                return Err(format!("inbox: unknown argument {value}"))
            }
            value => {
                inbox_validate_target_arg(value, "oracle")?;
                if oracle.replace(value.to_owned()).is_some() {
                    return Err("usage: maw inbox status [oracle-name] [--json] [--all]".to_owned());
                }
            }
        }
    }
    if all && oracle.is_some() {
        return Err("usage: maw inbox status [oracle-name] [--json] [--all]".to_owned());
    }
    Ok((oracle, json, all))
}

