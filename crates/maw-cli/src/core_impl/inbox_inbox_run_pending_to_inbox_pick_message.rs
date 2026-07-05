fn inbox_run_pending(env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let rows = inbox_load_pending_for_env(env, now_ms)?
        .into_iter()
        .filter(|message| message.status == "pending")
        .collect::<Vec<_>>();
    Ok(inbox_format_pending_list(&rows))
}

fn inbox_run_show_pending(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox show-pending <id>")?;
    let Some(message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    Ok(inbox_format_pending_detail(&message))
}

async fn inbox_run_approve(
    argv: &[String],
    env: &InboxEnv,
    sender: &mut impl InboxSender,
    now_ms: u64,
) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox approve <id>")?;
    let Some(mut message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    if message.status != "pending" {
        return Err(format!(
            "message {} is already {}",
            message.id, message.status
        ));
    }
    let original_status = message.status.clone();
    "approved".clone_into(&mut message.status);
    let state_pending_dir = inbox_state_pending_dir(env);
    inbox_write_pending(&state_pending_dir, &message)?;
    let query = message.query.as_deref().unwrap_or(&message.target);
    if let Err(error) = sender.inbox_send(query, &message.message, true).await {
        original_status.clone_into(&mut message.status);
        inbox_write_pending(&state_pending_dir, &message)?;
        return Err(error);
    }
    inbox_delete_pending(&state_pending_dir, &message.id)?;
    Ok(format!(
        "approved: {} ({} → {})\n",
        message.id, message.sender, message.target
    ))
}

fn inbox_run_reject(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox reject <id>")?;
    let Some(mut message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    let state_pending_dir = inbox_state_pending_dir(env);
    if message.status != "rejected" {
        "rejected".clone_into(&mut message.status);
        inbox_write_pending(&state_pending_dir, &message)?;
    }
    inbox_delete_pending(&state_pending_dir, &message.id)?;
    Ok(format!(
        "rejected: {} ({} → {})\n",
        message.id, message.sender, message.target
    ))
}

fn inbox_load_messages(inbox_dir: &std::path::Path) -> Result<Vec<InboxMessage>, String> {
    let Ok(entries) = std::fs::read_dir(inbox_dir) else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::<InboxMessage>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") || !path.is_file() {
            continue;
        }
        if let Some(message) = inbox_load_message(&path)? {
            messages.push(message);
        }
    }
    messages.sort_by_key(|message| std::cmp::Reverse(message.timestamp_ms));
    Ok(messages)
}

fn inbox_load_message(path: &std::path::Path) -> Result<Option<InboxMessage>, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let filename = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_owned();
    let id = filename.strip_suffix(".md").unwrap_or(&filename).to_owned();
    let (fields, body) = inbox_parse_frontmatter(&content);
    let timestamp_ms = inbox_message_timestamp_ms(&filename, path, fields.get("timestamp"))?;
    Ok(Some(InboxMessage {
        id,
        filename,
        path: path.to_path_buf(),
        from: fields
            .get("from")
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        to: fields
            .get("to")
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        timestamp_ms,
        read: fields.get("read").is_some_and(|value| value == "true"),
        body,
    }))
}

fn inbox_parse_frontmatter(content: &str) -> (BTreeMap<String, String>, String) {
    if !content.starts_with("---\n") {
        return (BTreeMap::new(), content.trim().to_owned());
    }
    let Some(end) = content[4..].find("\n---") else {
        return (BTreeMap::new(), content.trim().to_owned());
    };
    let end = end + 4;
    let mut fields = BTreeMap::<String, String>::new();
    for line in content[4..end].lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let body = content[end + "\n---".len()..].trim().to_owned();
    (fields, body)
}

fn inbox_message_timestamp_ms(
    filename: &str,
    path: &std::path::Path,
    frontmatter: Option<&String>,
) -> Result<u64, String> {
    if let Some(ms) = frontmatter.and_then(|value| inbox_parse_iso_ms(value)) {
        return Ok(ms);
    }
    if let Some(ms) = inbox_parse_filename_ms(filename) {
        return Ok(ms);
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("inbox: stat {}: {error}", path.display()))?;
    Ok(inbox_system_time_ms(
        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn inbox_parse_iso_ms(value: &str) -> Option<u64> {
    let prefix = value.get(0..16)?;
    let year = prefix.get(0..4)?.parse::<i32>().ok()?;
    let month = prefix.get(5..7)?.parse::<u32>().ok()?;
    let day = prefix.get(8..10)?.parse::<u32>().ok()?;
    let hour = prefix.get(11..13)?.parse::<u32>().ok()?;
    let minute = prefix.get(14..16)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_parse_filename_ms(filename: &str) -> Option<u64> {
    let head = filename.get(0..16)?;
    let normalized = head.replace('_', "T").replace('-', "");
    let year = normalized.get(0..4)?.parse::<i32>().ok()?;
    let month = normalized.get(4..6)?.parse::<u32>().ok()?;
    let day = normalized.get(6..8)?.parse::<u32>().ok()?;
    let hour = normalized.get(9..11)?.parse::<u32>().ok()?;
    let minute = normalized.get(11..13)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_ymdhm_to_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = inbox_days_from_civil(year, month, day)?;
    let seconds = days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
    u64::try_from(seconds).ok().map(|value| value * 1000)
}

fn inbox_days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_i = i32::try_from(month).ok()?;
    let day_i = i32::try_from(day).ok()?;
    let doy = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

fn inbox_find_message(
    inbox_dir: &std::path::Path,
    id: &str,
) -> Result<Option<InboxMessage>, String> {
    Ok(inbox_load_messages(inbox_dir)?
        .into_iter()
        .find(|message| message.id == id || message.filename.contains(id)))
}

fn inbox_pick_message<'a>(
    messages: &'a [InboxMessage],
    target: Option<&str>,
) -> Option<&'a InboxMessage> {
    let Some(target) = target else {
        return messages.first();
    };
    target
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1).and_then(|idx| messages.get(idx)))
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.id.to_lowercase().contains(&target.to_lowercase()))
        })
}

