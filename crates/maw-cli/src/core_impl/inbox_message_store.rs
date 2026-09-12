// Messages as files on disk: reading them, writing them, finding one.
//
// Each note is a markdown file with a frontmatter header, named for its
// timestamp and a slug of its first line. Marking one read rewrites just the
// header field, in place, so the body a human wrote is never reformatted.

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxMessage {
    id: String,
    filename: String,
    path: std::path::PathBuf,
    from: String,
    to: String,
    timestamp_ms: u64,
    read: bool,
    body: String,
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

fn inbox_find_message(
    inbox_dir: &std::path::Path,
    id: &str,
) -> Result<Option<InboxMessage>, String> {
    let messages = inbox_load_messages(inbox_dir)?;
    Ok(inbox_pick_message(&messages, Some(id)).cloned())
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
