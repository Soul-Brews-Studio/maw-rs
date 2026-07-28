// Archiving what has been read, without losing anything that has not.
//
// Draining moves old messages into a dated archive folder. `--safe` is the
// interesting part: it only takes messages whose body matches a routine pattern
// and that are older than a floor, so a real note left unread does not get swept
// up with the acknowledgements. Defaults to a dry run and reports what it would
// take.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxDrainResult {
    oracle: String,
    scanned: usize,
    matched: usize,
    archived: usize,
    remaining_matches: usize,
    max: usize,
    dry_run: bool,
    safe: bool,
    older_than_seconds: u64,
    processed_dir: String,
    items: Vec<InboxDrainItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxDrainItem {
    id: String,
    filename: String,
    reason: String,
    age_seconds: u64,
    destination: Option<String>,
    action: String,
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

fn inbox_safe_drain_reason(message: &InboxMessage) -> Option<String> {
    let line = message
        .body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if !line.starts_with('[') || !line.contains(']') || line.contains('?') {
        return None;
    }
    let lower = format!("{}\n{}", message.filename, line).to_lowercase();
    inbox_safe_reason_patterns()
        .into_iter()
        .find(|(_, needle)| lower.contains(needle))
        .map(|(reason, _)| reason.to_owned())
}

fn inbox_safe_reason_patterns() -> Vec<(&'static str, &'static str)> {
    vec![
        ("ci-green", "ci green confirmed"),
        ("local-ship", "local ship commit"),
        ("alpha-pushed", "alpha pushed"),
        ("coverage-pushed", "coverage batch pushed"),
        ("green-batch", "green batch"),
        ("verified", "verified"),
        ("next-slice-shipped", "shipped next slice"),
        ("delivery-confirm", "delivery confirm"),
        ("council", "no response needed"),
    ]
}

fn inbox_archive_message(
    source: &std::path::Path,
    destination: &std::path::Path,
    now_ms: u64,
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("inbox: create {}: {error}", parent.display()))?;
    }
    std::fs::rename(source, destination).map_err(|error| {
        format!(
            "inbox: archive {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    let _ = now_ms;
    Ok(())
}

fn inbox_unique_archive_path(
    processed_dir: &std::path::Path,
    filename: &str,
) -> std::path::PathBuf {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let ext = if std::path::Path::new(filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        ".md"
    } else {
        ""
    };
    let mut candidate = processed_dir.join(filename);
    let mut suffix = 2_usize;
    while candidate.exists() {
        candidate = processed_dir.join(format!("{stem}-{suffix}{ext}"));
        suffix += 1;
    }
    candidate
}

fn inbox_drain_item(
    message: &InboxMessage,
    reason: &str,
    age: u64,
    destination: &std::path::Path,
    dry_run: bool,
) -> InboxDrainItem {
    InboxDrainItem {
        id: message.id.clone(),
        filename: message.filename.clone(),
        reason: reason.to_owned(),
        age_seconds: age,
        destination: Some(destination.display().to_string()),
        action: if dry_run { "would_archive" } else { "archived" }.to_owned(),
    }
}

fn inbox_drain_result(
    env: &InboxEnv,
    options: &InboxDrainOptions,
    matched: usize,
    scanned: usize,
    processed_dir: &std::path::Path,
    items: Vec<InboxDrainItem>,
) -> InboxDrainResult {
    InboxDrainResult {
        oracle: options.oracle.clone().unwrap_or_else(|| env.oracle.clone()),
        scanned,
        matched,
        archived: items.len(),
        remaining_matches: matched.saturating_sub(items.len()),
        max: options.max,
        dry_run: options.dry_run,
        safe: true,
        older_than_seconds: options.older_than_seconds,
        processed_dir: processed_dir.display().to_string(),
        items,
    }
}

fn inbox_format_drain_result(result: &InboxDrainResult) -> String {
    let verb = if result.dry_run {
        "would archive"
    } else {
        "archived"
    };
    let mut lines = vec![format!(
        "{}: {verb} {}/{} safe stale inbox message(s) (scanned {}, max {})",
        result.oracle, result.archived, result.matched, result.scanned, result.max
    )];
    if result.remaining_matches > 0 {
        lines.push(format!(
            "   → {} safe match(es) remain after max cap",
            result.remaining_matches
        ));
    }
    if result.items.is_empty() {
        lines.push("   → no messages matched the safe stale-ack filter".to_owned());
    }
    for item in result.items.iter().take(10) {
        lines.push(format!(
            "   - {} [{}, {}]",
            item.filename,
            item.reason,
            inbox_format_duration(Some(item.age_seconds))
        ));
    }
    lines.push(format!(
        "   → {}: {}",
        if result.dry_run {
            "preview"
        } else {
            "processed"
        },
        result.processed_dir
    ));
    format!("{}\n", lines.join("\n"))
}

fn inbox_scan_archive_day(path: &std::path::Path, latest: &mut Option<u64>) -> Result<(), String> {
    let Ok(files) = std::fs::read_dir(path) else {
        return Ok(());
    };
    for file in files.flatten().filter(|entry| entry.path().is_file()) {
        let metadata = std::fs::metadata(file.path()).map_err(|error| error.to_string())?;
        let ms = inbox_system_time_ms(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        *latest = Some(latest.map_or(ms, |old| old.max(ms)));
    }
    Ok(())
}

fn inbox_latest_archive_mtime_ms(inbox_dir: &std::path::Path) -> Result<Option<u64>, String> {
    let processed = inbox_dir.join("processed");
    let Ok(days) = std::fs::read_dir(processed) else {
        return Ok(None);
    };
    let mut latest = None::<u64>;
    for day in days.flatten().filter(|entry| entry.path().is_dir()) {
        inbox_scan_archive_day(&day.path(), &mut latest)?;
    }
    Ok(latest)
}

fn inbox_archive_day(now_ms: u64) -> String {
    inbox_iso_label(now_ms)
        .get(0..10)
        .unwrap_or("1970-01-01")
        .to_owned()
}
