const DISPATCH_46: &[DispatcherEntry] = &[DispatcherEntry {
    command: "signals",
    handler: Handler::Sync(run_signals_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignalsOptions {
    days: i64,
    root: std::path::PathBuf,
    json: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
struct SignalsRecord {
    timestamp: String,
    bud: String,
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
    #[serde(skip_deserializing)]
    file: String,
}

fn run_signals_command(argv: &[String]) -> CliOutput {
    match signals_run(argv) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{error}\n"),
        },
    }
}

fn signals_run(argv: &[String]) -> Result<String, String> {
    let options = signals_parse_args(argv)?;
    let records = signals_scan(&options.root, options.days)?;
    if options.json {
        return serde_json::to_string_pretty(&records)
            .map(|json| format!("{json}\n"))
            .map_err(|error| format!("signals: render json: {error}"));
    }
    Ok(signals_render_text(&records, options.days))
}

fn signals_parse_args(argv: &[String]) -> Result<SignalsOptions, String> {
    let mut days = 7_i64;
    let mut root = std::env::current_dir().map_err(|error| format!("signals: cwd: {error}"))?;
    let mut json = false;
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if arg == "--json" {
            json = true;
            index += 1;
        } else if arg == "--days" {
            let value = signals_take_value(argv, index, "--days")?;
            days = signals_parse_days(value)?;
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--days=") {
            days = signals_parse_days(value)?;
            index += 1;
        } else if arg == "--root" {
            let value = signals_take_value(argv, index, "--root")?;
            signals_validate_path_arg(value, "--root")?;
            root = std::path::PathBuf::from(value);
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--root=") {
            signals_validate_path_arg(value, "--root")?;
            root = std::path::PathBuf::from(value);
            index += 1;
        } else {
            index += 1;
        }
    }
    Ok(SignalsOptions { days, root, json })
}

fn signals_take_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    argv.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("signals: {flag} requires a value"))
}

fn signals_parse_days(value: &str) -> Result<i64, String> {
    if value.starts_with('-') {
        return Err("signals: --days must not start with '-'".to_owned());
    }
    value
        .parse::<i64>()
        .map_err(|_| format!("signals: --days must be an integer, got '{value}'"))
}

fn signals_validate_path_arg(value: &str, flag: &str) -> Result<(), String> {
    if value.trim() != value || value.is_empty() || value.starts_with('-') {
        Err(format!(
            "signals: {flag} must be non-empty, unpadded, and not start with '-'"
        ))
    } else {
        Ok(())
    }
}

fn signals_scan(root: &std::path::Path, days: i64) -> Result<Vec<SignalsRecord>, String> {
    let dir = root.join("ψ").join("memory").join("signals");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let cutoff_day = signals_today_epoch_day()? - days;
    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();

    let mut records = Vec::new();
    for path in files {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut record) = serde_json::from_str::<SignalsRecord>(&raw) else {
            continue;
        };
        let Some(record_day) = signals_epoch_day_from_timestamp(&record.timestamp) else {
            continue;
        };
        if record_day >= cutoff_day {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .clone_into(&mut record.file);
            records.push(record);
        }
    }
    records.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    Ok(records)
}

fn signals_render_text(records: &[SignalsRecord], days: i64) -> String {
    const SIGNALS_DIM: &str = "\x1b[90m";
    const SIGNALS_RESET: &str = "\x1b[0m";
    if records.is_empty() {
        return format!("  {SIGNALS_DIM}no signals in the last {days} days{SIGNALS_RESET}\n");
    }
    let mut out = format!(
        "\n  \x1b[36mBud signals\x1b[0m (last {days}d — {} total)\n\n",
        records.len()
    );
    for record in records {
        let _ = writeln!(out, "{}", signals_format_record(record));
    }
    out.push('\n');
    out
}

fn signals_format_record(record: &SignalsRecord) -> String {
    const SIGNALS_DIM: &str = "\x1b[90m";
    const SIGNALS_RESET: &str = "\x1b[0m";
    let color = signals_kind_color(&record.kind);
    let date = record.timestamp.get(0..10).unwrap_or(&record.timestamp);
    format!(
        "  {color}[{}]{SIGNALS_RESET} {SIGNALS_DIM}{date}{SIGNALS_RESET} {}: {}",
        record.kind, record.bud, record.message
    )
}

fn signals_kind_color(kind: &str) -> &'static str {
    match kind {
        "alert" => "\x1b[31m",
        "pattern" => "\x1b[33m",
        "info" => "\x1b[36m",
        _ => "\x1b[37m",
    }
}

fn signals_today_epoch_day() -> Result<i64, String> {
    if let Ok(today) = std::env::var("MAW_SIGNALS_TODAY") {
        return signals_epoch_day_from_date(&today)
            .ok_or_else(|| format!("signals: invalid MAW_SIGNALS_TODAY '{today}'"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("signals: system clock before unix epoch: {error}"))?;
    i64::try_from(now.as_secs() / 86_400)
        .map_err(|error| format!("signals: system time overflow: {error}"))
}

fn signals_epoch_day_from_timestamp(timestamp: &str) -> Option<i64> {
    signals_epoch_day_from_date(timestamp.get(0..10)?)
}

fn signals_epoch_day_from_date(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(signals_days_from_civil(year, month, day))
}

fn signals_days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month = i64::from(month);
    let day = i64::from(day);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
