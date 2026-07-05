fn inbox_render_show(message: &InboxMessage) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n\u{001b}[36m{}\u{001b}[0m", message.filename);
    let _ = writeln!(
        out,
        "\u{001b}[90mfrom: {}  {}\u{001b}[0m\n",
        message.from,
        inbox_iso_label(message.timestamp_ms)
    );
    out.push_str(&message.body);
    out.push('\n');
    out
}

fn inbox_mark_frontmatter_read(content: &str, now_ms: u64) -> String {
    if !content.starts_with("---\n") {
        return content.to_owned();
    }
    let Some(end) = content[4..].find("\n---") else {
        return content.to_owned();
    };
    let end = end + 4;
    let mut frontmatter = content[..end + "\n---".len()].to_owned();
    if frontmatter.lines().any(|line| line.trim() == "read: false") {
        frontmatter = frontmatter.replace("read: false", "read: true");
    } else if !frontmatter.lines().any(|line| line.starts_with("read:")) {
        frontmatter = frontmatter.replace("\n---", "\nread: true\n---");
    }
    if !frontmatter.lines().any(|line| line.starts_with("readAt:")) {
        let replacement = format!("\nreadAt: {}\n---", inbox_iso_label(now_ms));
        frontmatter = frontmatter.replace("\n---", &replacement);
    }
    frontmatter + &content[end + "\n---".len()..]
}

fn inbox_write_file(
    inbox_dir: &std::path::Path,
    from: &str,
    to: &str,
    body: &str,
    now_ms: u64,
) -> Result<String, String> {
    inbox_validate_target_arg(from, "from")?;
    inbox_validate_target_arg(to, "to")?;
    std::fs::create_dir_all(inbox_dir)
        .map_err(|error| format!("inbox: create {}: {error}", inbox_dir.display()))?;
    let filename = inbox_filename(from, body, now_ms);
    let frontmatter = format!(
        "---\nfrom: {from}\nto: {to}\ntimestamp: {}\nread: false\n---\n\n{body}\n",
        inbox_iso_label(now_ms)
    );
    std::fs::write(inbox_dir.join(&filename), frontmatter)
        .map_err(|error| format!("inbox: write {filename}: {error}"))?;
    Ok(filename)
}

fn inbox_filename(from: &str, body: &str, now_ms: u64) -> String {
    let label = inbox_file_time_label(now_ms);
    let slug = inbox_slugify(body);
    format!("{label}_{from}_{slug}.md")
}

fn inbox_slugify(body: &str) -> String {
    let mut slug = String::new();
    for word in body.split_whitespace().take(5) {
        if !slug.is_empty() {
            slug.push('-');
        }
        for ch in word.to_lowercase().chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                slug.push(ch);
            }
            if slug.len() >= 40 {
                break;
            }
        }
        if slug.len() >= 40 {
            break;
        }
    }
    if slug.is_empty() {
        "note".to_owned()
    } else {
        slug
    }
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

