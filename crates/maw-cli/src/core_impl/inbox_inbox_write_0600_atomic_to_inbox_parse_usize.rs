fn inbox_write_0600_atomic(path: &std::path::Path, body: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("create parent failed: {error}"))?;
    let tmp = inbox_tmp_path(path);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        }
        let mut file = options.open(&tmp).map_err(|error| format!("tmp create failed: {error}"))?;
        std::io::Write::write_all(&mut file, body.as_bytes())
            .map_err(|error| format!("tmp write failed: {error}"))?;
        file.sync_all().map_err(|error| format!("tmp sync failed: {error}"))?;
    }
    std::fs::read_to_string(&tmp).map_err(|error| format!("tmp validate read failed: {error}"))?;
    std::fs::rename(&tmp, path).map_err(|error| format!("atomic rename failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("chmod 0600 failed: {error}"))?;
    }
    Ok(())
}

fn inbox_tmp_path(path: &std::path::Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("pending.json");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

#[allow(dead_code)]
fn inbox_pending_id(now_ms: u64, random_hex: &str) -> Result<String, String> {
    if random_hex.len() != 6 || !random_hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("inbox: pending id random suffix must be 6 hex chars".to_owned());
    }
    Ok(format!(
        "{}-{}",
        inbox_iso_label(now_ms).replace([':', '.'], "-"),
        random_hex.to_ascii_lowercase()
    ))
}

fn inbox_format_pending_list(rows: &[InboxPendingMessage]) -> String {
    if rows.is_empty() {
        return "no pending messages\n".to_owned();
    }
    let mut out = String::from("id  sender  target  sentAt  preview\n");
    out.push_str("--  ------  ------  ------  -------\n");
    for row in rows {
        let preview = inbox_pending_preview(&row.message);
        let _ = writeln!(
            out,
            "{}  {}  {}  {}  {preview}",
            row.id, row.sender, row.target, row.sent_at
        );
    }
    out
}

fn inbox_pending_preview(message: &str) -> String {
    let flattened = message.replace('\n', " ");
    let lower = flattened.to_ascii_lowercase();
    if lower.contains("token") || lower.contains("secret") || lower.contains("peer-key") {
        return "[redacted sensitive preview]".to_owned();
    }
    inbox_truncate(&flattened, 50)
}

fn inbox_format_pending_detail(message: &InboxPendingMessage) -> String {
    format!(
        "id:      {}\nsender:  {}\ntarget:  {}\nquery:   {}\nsentAt:  {}\nstatus:  {}\nmessage:\n{}\n",
        message.id,
        message.sender,
        message.target,
        message.query.as_deref().unwrap_or("-"),
        message.sent_at,
        message.status,
        message.message
    )
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

fn inbox_json_pretty<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|mut json| {
            json.push('\n');
            json
        })
        .map_err(|error| error.to_string())
}

fn inbox_required_value<'a>(
    argv: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1) else {
        return Err(format!("inbox: missing {flag} value"));
    };
    if value.starts_with('-') {
        return Err(format!("inbox: {flag} value must not start with '-'"));
    }
    Ok(value)
}

fn inbox_single_id_arg<'a>(argv: &'a [String], usage: &str) -> Result<&'a str, String> {
    if argv.len() != 1 {
        return Err(usage.to_owned());
    }
    inbox_validate_lookup_arg(&argv[0], "id")?;
    Ok(&argv[0])
}

fn inbox_validate_lookup_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('/') || value.contains("..") {
        return Err(format!("inbox: invalid {label}"));
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("inbox: invalid {label}"));
    }
    Ok(())
}

fn inbox_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("inbox: invalid {label}"));
    }
    if value.contains('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("inbox: invalid {label}"));
    }
    Ok(())
}

fn inbox_parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err(format!("{flag} must be a non-negative integer"));
    }
    value
        .parse::<usize>()
        .map_err(|_| format!("{flag} must be a non-negative integer"))
}

