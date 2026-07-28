// Messages held back for a human to approve.
//
// When the ACL gate refuses a cross-scope send it parks the message here rather
// than dropping it, so the refusal is recoverable: approve replays it, reject
// discards it, and anything nobody touched ages out on a TTL. Pending files are
// written 0600 and atomically -- they hold message bodies that were not cleared
// to be delivered yet.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct InboxPendingMessage {
    id: String,
    sender: String,
    target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(rename = "sentAt")]
    sent_at: String,
    status: String,
    message: String,
}

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
