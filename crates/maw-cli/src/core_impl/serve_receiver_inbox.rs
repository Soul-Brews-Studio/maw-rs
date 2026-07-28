// Where a cross-node message physically lands on the receiving side.
//
// `serve` accepts a delivery and then has to answer one question: which repo on
// THIS box belongs to the addressed oracle, so the note can be written to its
// ψ/inbox? That resolution walks config psiPath, the fleet manifest, the live
// pane cwd, and a ghq scan, then writes an atomically-named markdown file.
// Split out of serve.rs so the routing/auth layers are not read through it.

#[derive(Clone, Copy)]
struct ReceiverInboxInput<'a> {
    query: &'a str,
    target: Option<&'a str>,
    to: Option<&'a str>,
    from: &'a str,
    message: &'a str,
    config: &'a HeyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiverInboxOk {
    oracle: String,
    inbox_dir: std::path::PathBuf,
    path: std::path::PathBuf,
    filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReceiverInboxResult {
    Ok(ReceiverInboxOk),
    Err { oracle: Option<String>, reason: String },
}

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
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = cli_dispatch_civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
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
    let value = merged_config_value_for_env(&env);
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

fn receiver_inbox_add_target_lookup_keys(keys: &mut BTreeSet<String>, raw: Option<&str>) {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let raw = receiver_inbox_strip_pane_suffix(raw);
    if let Some(key) = receiver_inbox_lookup_key(raw) {
        keys.insert(key);
    }
    let parts = raw
        .split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [session, window] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        [_, session, window, ..] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        _ => {}
    }
}

fn receiver_inbox_target_lookup_keys(input: &ReceiverInboxInput<'_>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    receiver_inbox_add_target_lookup_keys(&mut keys, input.target);
    receiver_inbox_add_target_lookup_keys(&mut keys, input.to);
    receiver_inbox_add_target_lookup_keys(&mut keys, Some(input.query));
    keys
}

fn receiver_inbox_manifest_entry_matches_target(
    entry: &LocateManifestEntry,
    target_keys: &BTreeSet<String>,
) -> bool {
    entry
        .session
        .as_deref()
        .and_then(receiver_inbox_lookup_key)
        .is_some_and(|key| target_keys.contains(&key))
        || entry
            .window
            .as_deref()
            .and_then(receiver_inbox_lookup_key)
            .is_some_and(|key| target_keys.contains(&key))
}

fn receiver_inbox_push_manifest_entry_candidates(
    candidates: &mut Vec<std::path::PathBuf>,
    entry: &LocateManifestEntry,
) {
    if let Some(local_path) = entry.local_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        candidates.push(std::path::PathBuf::from(local_path));
    }
    if let Some(repo) = entry.repo.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        let ghq_root = receiver_inbox_ghq_root();
        candidates.push(ghq_root.join("github.com").join(repo));
        candidates.push(ghq_root.join(repo));
    }
}

fn receiver_inbox_existing_candidates(
    candidates: Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.display().to_string()))
        .filter(|candidate| candidate.exists())
        .collect()
}

fn receiver_inbox_repo_candidates(
    oracle: &str,
    input: &ReceiverInboxInput<'_>,
    psi_root: Option<&std::path::Path>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let mut candidates = Vec::new();
    if let Some(psi_path) = psi_root {
        candidates.push(receiver_inbox_strip_psi_suffix(psi_path));
    } else if let (Some(psi_path), Some(config_oracle)) =
        (receiver_inbox_config_psi_path(), input.config.oracle.as_deref())
    {
        if receiver_inbox_normalize_oracle_name(Some(config_oracle)).as_deref() == Some(oracle) {
            candidates.push(receiver_inbox_strip_psi_suffix(&psi_path));
        }
    }
    if let Some(target) = input.target {
        match receiver_inbox_resolve_target_cwd(target) {
            Ok(Some(path)) => candidates.push(path),
            Ok(None) => {}
            Err(reason) => return Err(reason),
        }
    }
    let manifest = locate_load_manifest();
    if let Some(entry) = manifest.iter().find(|entry| {
        receiver_inbox_normalize_oracle_name(Some(&entry.name)).as_deref() == Some(oracle)
            || entry.window.as_deref().and_then(|window| receiver_inbox_normalize_oracle_name(Some(window))).as_deref()
                == Some(oracle)
    }) {
        receiver_inbox_push_manifest_entry_candidates(&mut candidates, entry);
    }

    let target_keys = receiver_inbox_target_lookup_keys(input);
    if !target_keys.is_empty() {
        let mut phase_b = Vec::new();
        for entry in manifest
            .iter()
            .filter(|entry| receiver_inbox_manifest_entry_matches_target(entry, &target_keys))
        {
            let mut entry_candidates = Vec::new();
            receiver_inbox_push_manifest_entry_candidates(&mut entry_candidates, entry);
            phase_b.extend(receiver_inbox_existing_candidates(entry_candidates));
        }
        let phase_b = receiver_inbox_existing_candidates(phase_b);
        if phase_b.len() > 1 {
            return Err(format!("receiver repo ambiguous for {}", input.query));
        }
        candidates.extend(phase_b);
    }
    Ok(receiver_inbox_existing_candidates(candidates))
}

fn persist_receiver_inbox(
    input: ReceiverInboxInput<'_>,
    now_millis: u128,
    psi_root: Option<&std::path::Path>,
) -> ReceiverInboxResult {
    let Some(oracle) = receiver_inbox_resolve_oracle(&input) else {
        return ReceiverInboxResult::Err { oracle: None, reason: "receiver oracle could not be inferred".to_owned() };
    };
    let repo_candidates = match receiver_inbox_repo_candidates(&oracle, &input, psi_root) {
        Ok(candidates) => candidates,
        Err(reason) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason },
    };
    let Some(repo_path) = repo_candidates.into_iter().next() else {
        return ReceiverInboxResult::Err {
            oracle: Some(oracle.clone()),
            reason: format!("receiver repo not found for {oracle}"),
        };
    };
    let timestamp = receiver_inbox_iso_from_millis(now_millis);
    let date_part = &timestamp[..10];
    let time_part = timestamp[11..16].replace(':', "-");
    let base_filename = format!(
        "{date_part}_{time_part}_{}_{}.md",
        receiver_inbox_safe_segment(input.from),
        receiver_inbox_slugify_body(input.message)
    );
    let inbox_dir = repo_path.join("ψ").join("inbox");
    let body = receiver_inbox_body(input.from, &oracle, &timestamp, input.message);
    if let Err(error) = std::fs::create_dir_all(&inbox_dir) {
        return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
    }
    for attempt in 1..=1000 {
        let filename = receiver_inbox_filename_with_collision_suffix(&base_filename, attempt);
        let path = inbox_dir.join(&filename);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = std::io::Write::write_all(&mut file, body.as_bytes()) {
                    return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
                }
                return ReceiverInboxResult::Ok(ReceiverInboxOk { oracle, inbox_dir, path, filename });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() },
        }
    }
    ReceiverInboxResult::Err {
        oracle: Some(oracle),
        reason: format!("receiver inbox filename collision limit reached for {base_filename}"),
    }
}
