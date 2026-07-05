fn inbox_build_status(
    oracle: &str,
    inbox_dir: &std::path::Path,
    env: &InboxEnv,
    now_ms: u64,
) -> Result<InboxStatus, String> {
    let messages = inbox_load_messages(inbox_dir)?;
    let unread_messages = messages
        .iter()
        .filter(|message| !message.read)
        .collect::<Vec<_>>();
    let oldest_age = unread_messages
        .iter()
        .map(|message| inbox_age_seconds(message.timestamp_ms, now_ms))
        .max();
    let archive_age =
        inbox_latest_archive_mtime_ms(inbox_dir)?.map(|mtime| inbox_age_seconds(mtime, now_ms));
    let mut cursor = inbox_read_cursor(&env.state_dir);
    let previous = cursor.get(oracle);
    let delta = previous.map_or(0, |entry| {
        inbox_usize_delta(unread_messages.len(), entry.unread)
    });
    let mut reasons = Vec::<String>::new();
    inbox_push_status_reasons(
        &mut reasons,
        unread_messages.len(),
        oldest_age,
        archive_age,
        delta,
    );
    let status = InboxStatus {
        oracle: oracle.to_owned(),
        unread: unread_messages.len(),
        oldest_age_seconds: oldest_age,
        last_archive_age_seconds: archive_age,
        delta_since_last_check: delta,
        level: if reasons.is_empty() { "green" } else { "red" }.to_owned(),
        reasons,
    };
    cursor.insert(oracle.to_owned(), inbox_cursor_entry(&status, now_ms));
    inbox_write_cursor(&env.state_dir, &cursor)?;
    Ok(status)
}

fn inbox_usize_delta(current: usize, previous: usize) -> i64 {
    let current = i64::try_from(current).unwrap_or(i64::MAX);
    let previous = i64::try_from(previous).unwrap_or(i64::MAX);
    current.saturating_sub(previous)
}

fn inbox_push_status_reasons(
    reasons: &mut Vec<String>,
    unread: usize,
    oldest_age: Option<u64>,
    archive_age: Option<u64>,
    delta: i64,
) {
    if unread > INBOX_UNREAD_RED_THRESHOLD {
        reasons.push("unread>50".to_owned());
    }
    if oldest_age.is_some_and(|age| age > INBOX_OLDEST_RED_SECONDS) {
        reasons.push("oldest>4h".to_owned());
    }
    if archive_age.is_some_and(|age| age > INBOX_ARCHIVE_RED_SECONDS) {
        reasons.push("since_archive>8h".to_owned());
    } else if archive_age.is_none() && unread > 0 {
        reasons.push("no_archive".to_owned());
    }
    if delta > 0 {
        reasons.push("delta>0_no_archive_activity".to_owned());
    }
}

fn inbox_run_drain(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let options = inbox_parse_drain_args(argv)?;
    if options
        .oracle
        .as_ref()
        .is_some_and(|oracle| oracle != &env.oracle)
    {
        return Err("inbox: native drain currently supports local inbox only".to_owned());
    }
    let result = inbox_drain_local(&options, env, now_ms)?;
    if options.json {
        inbox_json_pretty(&result)
    } else {
        Ok(inbox_format_drain_result(&result))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxDrainOptions {
    oracle: Option<String>,
    json: bool,
    dry_run: bool,
    max: usize,
    older_than_seconds: u64,
}

fn inbox_parse_drain_args(argv: &[String]) -> Result<InboxDrainOptions, String> {
    let mut options = InboxDrainOptions {
        oracle: None,
        json: false,
        dry_run: false,
        max: INBOX_SAFE_DRAIN_DEFAULT_MAX,
        older_than_seconds: INBOX_SAFE_DRAIN_DEFAULT_MIN_AGE_SECONDS,
    };
    let mut safe = false;
    let mut index = 0_usize;
    while index < argv.len() {
        inbox_parse_drain_arg(argv, &mut index, &mut options, &mut safe)?;
        index += 1;
    }
    if !safe {
        return Err("usage: maw inbox drain [oracle-name] --safe [--max N] [--older-than-hours H] [--json] [--dry-run]".to_owned());
    }
    Ok(options)
}

fn inbox_parse_drain_arg(
    argv: &[String],
    index: &mut usize,
    options: &mut InboxDrainOptions,
    safe: &mut bool,
) -> Result<(), String> {
    match argv[*index].as_str() {
        "--safe" => *safe = true,
        "--json" => options.json = true,
        "--dry-run" => options.dry_run = true,
        "--max" => {
            options.max = inbox_parse_usize(inbox_required_value(argv, *index, "--max")?, "--max")?;
            *index += 1;
        }
        "--older-than-hours" => {
            options.older_than_seconds = inbox_parse_hours_seconds(inbox_required_value(
                argv,
                *index,
                "--older-than-hours",
            )?)?;
            *index += 1;
        }
        value if value.starts_with("--max=") => {
            options.max = inbox_parse_usize(value.trim_start_matches("--max="), "--max")?;
        }
        value if value.starts_with("--older-than-hours=") => {
            options.older_than_seconds =
                inbox_parse_hours_seconds(value.trim_start_matches("--older-than-hours="))?;
        }
        value if value.starts_with('-') => return Err(format!("inbox: unknown argument {value}")),
        value => inbox_set_drain_oracle(options, value)?,
    }
    Ok(())
}

fn inbox_set_drain_oracle(options: &mut InboxDrainOptions, value: &str) -> Result<(), String> {
    inbox_validate_target_arg(value, "oracle")?;
    if options.oracle.replace(value.to_owned()).is_some() {
        return Err("usage: maw inbox drain [oracle-name] --safe [--max N] [--older-than-hours H] [--json] [--dry-run]".to_owned());
    }
    Ok(())
}

fn inbox_drain_local(
    options: &InboxDrainOptions,
    env: &InboxEnv,
    now_ms: u64,
) -> Result<InboxDrainResult, String> {
    let messages = inbox_load_messages(&env.inbox_dir)?;
    let mut candidates = inbox_drain_candidates(&messages, now_ms, options.older_than_seconds);
    candidates.sort_by_key(|(_, _, age)| *age);
    let selected = candidates.into_iter().take(options.max).collect::<Vec<_>>();
    let processed_dir = env
        .inbox_dir
        .join("processed")
        .join(inbox_archive_day(now_ms));
    let mut items = Vec::<InboxDrainItem>::new();
    for (message, reason, age) in selected {
        let destination = inbox_unique_archive_path(&processed_dir, &message.filename);
        if !options.dry_run {
            inbox_archive_message(&message.path, &destination, now_ms)?;
        }
        items.push(inbox_drain_item(
            &message,
            &reason,
            age,
            &destination,
            options.dry_run,
        ));
    }
    let matched = inbox_drain_candidates(&messages, now_ms, options.older_than_seconds).len();
    Ok(inbox_drain_result(
        env,
        options,
        matched,
        messages.len(),
        &processed_dir,
        items,
    ))
}

fn inbox_drain_candidates(
    messages: &[InboxMessage],
    now_ms: u64,
    min_age: u64,
) -> Vec<(InboxMessage, String, u64)> {
    messages
        .iter()
        .filter_map(|message| {
            let reason = inbox_safe_drain_reason(message)?;
            let age = inbox_age_seconds(message.timestamp_ms, now_ms);
            (age >= min_age).then(|| (message.clone(), reason, age))
        })
        .collect()
}

