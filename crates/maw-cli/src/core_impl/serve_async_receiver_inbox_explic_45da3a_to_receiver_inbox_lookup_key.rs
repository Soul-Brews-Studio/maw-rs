fn receiver_inbox_explicit_enabled(value: Option<std::ffi::OsString>) -> Option<bool> {
    let value = value?.to_string_lossy().trim().to_ascii_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn receiver_inbox_auto_write_enabled() -> bool {
    if let Some(enabled) = receiver_inbox_explicit_enabled(std::env::var_os("MAW_HEY_INBOX_AUTOWRITE")) {
        return enabled;
    }
    std::env::var("MAW_TEST_MODE").ok().as_deref() != Some("1")
}

fn receiver_inbox_now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn receiver_inbox_iso_from_millis(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1_000).unwrap_or(i64::MAX);
    let ms = u32::try_from(millis % 1_000).unwrap_or(999);
    let (year, month, day, hour, minute, second) = unix_seconds_to_utc(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z")
}

fn receiver_inbox_strip_pane_suffix(value: &str) -> &str {
    let Some((prefix, suffix)) = value.rsplit_once('.') else {
        return value;
    };
    if suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        prefix
    } else {
        value
    }
}

fn receiver_inbox_basename(value: &str) -> &str {
    std::path::Path::new(value)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(value)
}

fn receiver_inbox_normalize_oracle_name(raw: Option<&str>) -> Option<String> {
    let mut value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let colon_value;
    if value.contains(':') {
        let parts = value.split(':').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        colon_value = if parts.len() >= 3 {
            parts[2]
        } else {
            parts.get(1).copied().or_else(|| parts.first().copied()).unwrap_or(value)
        };
        value = colon_value;
    }
    value = receiver_inbox_strip_pane_suffix(value);
    value = receiver_inbox_basename(value);
    if let Some(stripped) = value.strip_suffix("-oracle") {
        value = stripped;
    }
    let trimmed_numeric = value
        .split_once('-')
        .and_then(|(prefix, rest)| prefix.bytes().all(|byte| byte.is_ascii_digit()).then_some(rest))
        .unwrap_or(value);
    (!trimmed_numeric.is_empty()).then(|| trimmed_numeric.to_owned())
}

fn receiver_inbox_resolve_oracle(input: &ReceiverInboxInput<'_>) -> Option<String> {
    receiver_inbox_normalize_oracle_name(input.to)
        .or_else(|| receiver_inbox_normalize_oracle_name(input.target))
        .or_else(|| receiver_inbox_normalize_oracle_name(Some(input.query)))
}

fn receiver_inbox_safe_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-');
        if safe {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-').chars().take(64).collect::<String>();
    if out.is_empty() { "unknown".to_owned() } else { out }
}

fn receiver_inbox_slugify_body(body: &str) -> String {
    receiver_inbox_safe_segment(&body.split_whitespace().take(6).collect::<Vec<_>>().join("-").to_ascii_lowercase())
        .chars()
        .take(48)
        .collect()
}

fn receiver_inbox_body(from: &str, to: &str, timestamp: &str, message: &str) -> String {
    [
        "---".to_owned(),
        format!("from: {from}"),
        format!("to: {to}"),
        format!("timestamp: {timestamp}"),
        "read: false".to_owned(),
        "---".to_owned(),
        String::new(),
        message.to_owned(),
        String::new(),
    ]
    .join("\n")
}

fn receiver_inbox_filename_with_collision_suffix(base: &str, attempt: usize) -> String {
    if attempt <= 1 {
        return base.to_owned();
    }
    base.strip_suffix(".md")
        .map_or_else(|| format!("{base}-{attempt}"), |prefix| format!("{prefix}-{attempt}.md"))
}

fn receiver_inbox_strip_psi_suffix(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.display().to_string();
    let stripped = text.trim_end_matches('/');
    if let Some(prefix) = stripped.strip_suffix("/ψ").or_else(|| stripped.strip_suffix("/psi")) {
        std::path::PathBuf::from(prefix)
    } else {
        std::path::PathBuf::from(stripped)
    }
}

fn receiver_inbox_config_psi_path() -> Option<std::path::PathBuf> {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    value
        .get("psiPath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

fn receiver_inbox_ghq_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        std::path::PathBuf::from,
    )
}

fn receiver_inbox_target_cwd_parts(target: &str) -> Option<(&str, Option<&str>)> {
    let clean = receiver_inbox_strip_pane_suffix(target.trim());
    if clean.is_empty() {
        return None;
    }
    let parts = clean.split(':').collect::<Vec<_>>();
    let (session, window) = if parts.len() >= 3 {
        (parts.get(1).copied().unwrap_or_default(), parts.get(2).copied())
    } else {
        (parts.first().copied().unwrap_or_default(), parts.get(1).copied())
    };
    let session = session.trim();
    if session.is_empty() {
        return None;
    }
    Some((session, window.map(str::trim).filter(|value| !value.is_empty())))
}

fn receiver_inbox_target_cwd_window<'a>(
    fleet: &'a NativeFleetSession,
    win_ref: Option<&str>,
) -> Option<&'a NativeFleetWindow> {
    let Some(win_ref) = win_ref else {
        return fleet.windows.first();
    };
    if win_ref.bytes().all(|byte| byte.is_ascii_digit()) {
        return win_ref
            .parse::<usize>()
            .ok()
            .and_then(|index| fleet.windows.get(index));
    }
    fleet.windows.iter().find(|window| window.name == win_ref)
}

fn receiver_inbox_resolve_target_cwd(target: &str) -> Result<Option<std::path::PathBuf>, String> {
    let Some((session, win_ref)) = receiver_inbox_target_cwd_parts(target) else {
        return Ok(None);
    };
    let ghq_root = receiver_inbox_ghq_root();
    let mut candidates = Vec::new();
    for fleet in load_native_fleet().into_iter().filter(|fleet| fleet.name == session) {
        let Some(window) = receiver_inbox_target_cwd_window(&fleet, win_ref) else {
            continue;
        };
        let repo = window.repo.trim();
        if repo.is_empty() {
            continue;
        }
        candidates.push(ghq_root.join(repo));
    }
    let candidates = receiver_inbox_existing_candidates(candidates);
    if candidates.len() > 1 {
        return Err(format!("receiver repo ambiguous for {target}"));
    }
    Ok(candidates.into_iter().next())
}

fn receiver_inbox_lookup_key(value: &str) -> Option<String> {
    let value = receiver_inbox_strip_pane_suffix(value.trim()).trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

