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

fn inbox_load_pending_for_env(env: &InboxEnv, now_ms: u64) -> Result<Vec<InboxPendingMessage>, String> {
    let state_dir = inbox_state_pending_dir(env);
    inbox_reap_expired_pending(&state_dir, now_ms)?;
    let mut by_id = BTreeMap::<String, InboxPendingMessage>::new();
    for message in inbox_load_pending(&env.pending_dir, now_ms, false)? {
        by_id.entry(message.id.clone()).or_insert(message);
    }
    for message in inbox_load_pending(&state_dir, now_ms, true)? {
        by_id.insert(message.id.clone(), message);
    }
    let mut rows = by_id.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.sent_at.cmp(&right.sent_at).then_with(|| left.id.cmp(&right.id)));
    Ok(rows)
}

fn inbox_load_pending(
    pending_dir: &std::path::Path,
    now_ms: u64,
    state_owned: bool,
) -> Result<Vec<InboxPendingMessage>, String> {
    let Ok(entries) = std::fs::read_dir(pending_dir) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::<InboxPendingMessage>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|error| format!("inbox: read pending {}: {error}", path.display()))?;
        if let Ok(message) = serde_json::from_str::<InboxPendingMessage>(&raw) {
            if inbox_pending_is_expired(&message, now_ms) {
                if state_owned {
                    let _ = std::fs::remove_file(&path);
                }
            } else if inbox_validate_pending_message(&message).is_ok() {
                rows.push(message);
            }
        }
    }
    rows.sort_by(|left, right| left.sent_at.cmp(&right.sent_at));
    Ok(rows)
}

fn inbox_resolve_pending_for_env(
    env: &InboxEnv,
    id: &str,
    now_ms: u64,
) -> Result<Option<InboxPendingMessage>, String> {
    inbox_validate_lookup_arg(id, "pending id")?;
    let rows = inbox_load_pending_for_env(env, now_ms)?;
    if let Some(exact) = rows.iter().find(|message| message.id == id) {
        return Ok(Some(exact.clone()));
    }
    let matches = rows
        .into_iter()
        .filter(|message| message.id.starts_with(id))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        _ => Err(format!("pending id prefix is ambiguous: {id}")),
    }
}

fn inbox_write_pending(
    pending_dir: &std::path::Path,
    message: &InboxPendingMessage,
) -> Result<(), String> {
    inbox_validate_pending_message(message)?;
    std::fs::create_dir_all(pending_dir)
        .map_err(|error| format!("inbox: create pending dir: {error}"))?;
    let json = serde_json::to_string_pretty(message).map_err(|error| error.to_string())?;
    let path = pending_dir.join(format!("{}.json", message.id));
    inbox_write_0600_atomic(&path, &(json + "\n"))
        .map_err(|error| format!("inbox: write pending {}: {error}", message.id))?;
    let roundtrip = std::fs::read_to_string(&path)
        .map_err(|error| format!("inbox: validate pending {}: {error}", message.id))?;
    let parsed = serde_json::from_str::<InboxPendingMessage>(&roundtrip)
        .map_err(|error| format!("inbox: validate pending json {}: {error}", message.id))?;
    if parsed != *message {
        return Err(format!("inbox: validate pending mismatch {}", message.id));
    }
    Ok(())
}

fn inbox_delete_pending(pending_dir: &std::path::Path, id: &str) -> Result<(), String> {
    let path = pending_dir.join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|error| format!("inbox: delete pending {}: {error}", path.display()))?;
    }
    Ok(())
}

fn inbox_reap_expired_pending(pending_dir: &std::path::Path, now_ms: u64) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(pending_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<InboxPendingMessage>(&raw) else {
            continue;
        };
        if inbox_pending_is_expired(&message, now_ms) {
            std::fs::remove_file(&path)
                .map_err(|error| format!("inbox: reap expired pending {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn inbox_pending_is_expired(message: &InboxPendingMessage, now_ms: u64) -> bool {
    inbox_parse_iso_ms(&message.sent_at)
        .is_some_and(|sent_ms| inbox_age_seconds(sent_ms, now_ms) > INBOX_PENDING_TTL_SECONDS)
}

fn inbox_validate_pending_message(message: &InboxPendingMessage) -> Result<(), String> {
    inbox_validate_lookup_arg(&message.id, "pending id")?;
    inbox_validate_target_arg(&message.sender, "sender")?;
    inbox_validate_target_arg(&message.target, "target")?;
    if let Some(query) = &message.query {
        inbox_validate_target_arg(query, "query")?;
    }
    if !matches!(message.status.as_str(), "pending" | "approved" | "rejected") {
        return Err("inbox: invalid pending status".to_owned());
    }
    if message.sent_at.is_empty() || message.sent_at.chars().any(char::is_control) {
        return Err("inbox: invalid pending sentAt".to_owned());
    }
    Ok(())
}

