// Is this inbox being kept up with?
//
// Unread count and the age of the oldest message are only meaningful against
// last time anyone looked, so a cursor is kept per oracle and the delta since
// that check is part of the answer. The level and its reasons travel together --
// a red light with no reason is not actionable.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxStatus {
    oracle: String,
    unread: usize,
    oldest_age_seconds: Option<u64>,
    last_archive_age_seconds: Option<u64>,
    delta_since_last_check: i64,
    level: String,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct InboxCursorEntry {
    unread: usize,
    #[serde(rename = "latestArchiveMtimeMs")]
    latest_archive_mtime_ms: Option<u64>,
    #[serde(rename = "checkedAt")]
    checked_at: String,
}

type InboxCursorStore = BTreeMap<String, InboxCursorEntry>;

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

fn inbox_render_status(status: &InboxStatus, json: bool) -> Result<String, String> {
    if json {
        return inbox_json_pretty(status);
    }
    let symbol = if status.level == "red" {
        "🔴"
    } else {
        "🟢"
    };
    let oldest = status
        .oldest_age_seconds
        .map_or("none".to_owned(), |age| inbox_format_duration(Some(age)));
    let archive = status
        .last_archive_age_seconds
        .map_or("never".to_owned(), |age| {
            format!("{} ago", inbox_format_duration(Some(age)))
        });
    let mut line = format!(
        "{symbol} UNREAD {} (oldest {oldest}, last archive {archive}, Δ {} last cycle)\n",
        status.unread,
        inbox_format_delta(status.delta_since_last_check)
    );
    if status.level == "red" {
        line.push_str("   → not draining — consider escalation\n");
    }
    Ok(line)
}

fn inbox_render_status_list(statuses: &[InboxStatus], json: bool) -> Result<String, String> {
    if json {
        return inbox_json_pretty(statuses);
    }
    if statuses.is_empty() {
        return Ok("no local fleet inboxes found\n".to_owned());
    }
    let mut out = String::new();
    for status in statuses {
        let symbol = if status.level == "red" {
            "🔴"
        } else {
            "🟢"
        };
        let oldest = status
            .oldest_age_seconds
            .map_or("none".to_owned(), |age| inbox_format_duration(Some(age)));
        let reasons = if status.reasons.is_empty() {
            String::new()
        } else {
            format!(" [{}]", status.reasons.join(","))
        };
        let _ = writeln!(
            out,
            "{symbol} {}: unread {} (oldest {oldest}){reasons}",
            status.oracle, status.unread
        );
    }
    Ok(out)
}

fn inbox_read_cursor(state_dir: &std::path::Path) -> InboxCursorStore {
    let path = state_dir.join("inbox-cursor.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn inbox_write_cursor(state_dir: &std::path::Path, store: &InboxCursorStore) -> Result<(), String> {
    std::fs::create_dir_all(state_dir)
        .map_err(|error| format!("inbox: create {}: {error}", state_dir.display()))?;
    let json = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(state_dir.join("inbox-cursor.json"), format!("{json}\n"))
        .map_err(|error| format!("inbox: write cursor: {error}"))
}

fn inbox_cursor_entry(status: &InboxStatus, now_ms: u64) -> InboxCursorEntry {
    InboxCursorEntry {
        unread: status.unread,
        latest_archive_mtime_ms: None,
        checked_at: inbox_iso_label(now_ms),
    }
}
