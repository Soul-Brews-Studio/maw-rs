fn bg_run_gc(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let threshold = match flags.older_than.as_deref() {
        Some(value) => bg_parse_duration(value).map_err(|message| (1, message))?,
        None => BG_DEFAULT_GC_SECONDS,
    };
    let sessions = bg_list_sessions(tmux, now).map_err(|message| (1, message))?;
    let mut reaped = Vec::new();
    let mut kept = Vec::new();
    for session in sessions {
        if session.status == BgSessionStatus::Done && session.age_seconds >= threshold {
            if !bg_flags_has(&flags, BG_FLAG_DRY_RUN) {
                bg_kill_session(&session.slug, tmux).map_err(|message| (1, message))?;
            }
            reaped.push(session.slug);
        } else {
            kept.push(session.slug);
        }
    }
    Ok((0, bg_gc_output(bg_flags_has(&flags, BG_FLAG_DRY_RUN), &reaped, &kept, threshold)))
}

fn bg_parse_flags(argv: &[String]) -> Result<BgFlags, String> {
    let mut flags = BgFlags::default();
    let mut index = 0usize;
    while index < argv.len() {
        let token = &argv[index];
        if token == "--" {
            flags.positionals.extend(argv[index + 1..].iter().cloned());
            break;
        }
        if !token.starts_with('-') {
            flags.positionals.push(token.clone());
            index += 1;
            continue;
        }
        index = bg_parse_flag_token(argv, index, &mut flags)?;
    }
    Ok(flags)
}

fn bg_parse_flag_token(argv: &[String], index: usize, flags: &mut BgFlags) -> Result<usize, String> {
    let token = &argv[index];
    let (key, inline) = bg_split_flag(token);
    match key.as_str() {
        "--follow" | "--dry-run" | "--all" | "--json" | "--help" | "-h" => {
            bg_assign_bool(flags, &key);
            Ok(index + 1)
        }
        "--name" | "--lines" | "--older-than" => {
            let (value, next) = bg_flag_value(argv, index, inline.as_deref(), &key)?;
            bg_assign_string(flags, &key, &value)?;
            Ok(next)
        }
        _ => {
            flags.positionals.push(token.clone());
            Ok(index + 1)
        }
    }
}

fn bg_split_flag(token: &str) -> (String, Option<String>) {
    if let Some((key, value)) = token.split_once('=') {
        (key.to_owned(), Some(value.to_owned()))
    } else {
        (token.to_owned(), None)
    }
}

fn bg_flag_value(
    argv: &[String],
    index: usize,
    inline: Option<&str>,
    key: &str,
) -> Result<(String, usize), String> {
    if let Some(value) = inline {
        return Ok((value.to_owned(), index + 1));
    }
    let Some(next) = argv.get(index + 1) else {
        return Err(format!("flag {key} requires a value"));
    };
    if next.starts_with('-') {
        return Err(format!("flag {key} requires a value"));
    }
    Ok((next.clone(), index + 2))
}

fn bg_assign_bool(flags: &mut BgFlags, key: &str) {
    match key {
        "--follow" => bg_flags_set(flags, BG_FLAG_FOLLOW),
        "--dry-run" => bg_flags_set(flags, BG_FLAG_DRY_RUN),
        "--all" => bg_flags_set(flags, BG_FLAG_ALL),
        "--json" => bg_flags_set(flags, BG_FLAG_JSON),
        "--help" | "-h" => bg_flags_set(flags, BG_FLAG_HELP),
        _ => {}
    }
}

fn bg_flags_set(flags: &mut BgFlags, bit: u8) {
    flags.bits |= bit;
}

fn bg_flags_has(flags: &BgFlags, bit: u8) -> bool {
    flags.bits & bit != 0
}

fn bg_assign_string(flags: &mut BgFlags, key: &str, value: &str) -> Result<(), String> {
    match key {
        "--name" => flags.name = Some(value.to_owned()),
        "--lines" => flags.lines = Some(bg_parse_lines(value)?),
        "--older-than" => flags.older_than = Some(value.to_owned()),
        _ => {}
    }
    Ok(())
}

fn bg_command_from_positionals(positionals: &[String]) -> Result<String, String> {
    if positionals.is_empty() {
        return Err("bg: missing command (usage: maw bg \"<cmd>\")".to_owned());
    }
    Ok(positionals.join(" ").trim().to_owned())
}

fn bg_spawn_slug(command: &str, name: Option<&str>) -> Result<String, String> {
    if let Some(name) = name {
        bg_validate_name(name)?;
        Ok(name.to_owned())
    } else {
        bg_derive_slug(command)
    }
}

fn bg_derive_slug(command: &str) -> Result<String, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("bg: command cannot be empty".to_owned());
    }
    let first = trimmed.split_whitespace().next().unwrap_or_default();
    let mut stem = bg_slug_stem(first);
    if stem.is_empty() {
        stem.clear();
        stem.push_str("cmd");
    }
    let hash = hash_body(Some(command.as_bytes()));
    Ok(format!("{}-{}", stem, &hash[..4]))
}

fn bg_slug_stem(first: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in first.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_dash = false;
        } else if ch == '-' && !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 16 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn bg_validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 || name.starts_with('-') || name == "--" {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    let mut chars = name.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit()) {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    Ok(())
}

fn bg_validate_command(command: &str) -> Result<(), String> {
    if command.is_empty() || command.starts_with('-') || command == "--" {
        return Err("bg: command must be non-empty and not start with '-'".to_owned());
    }
    if command.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("bg: command must not contain NUL/control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_ref(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value == "--" || value.trim() != value {
        return Err("bg ref must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("bg ref must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_session_name(value: &str) -> Result<(), String> {
    if !value.starts_with(BG_PREFIX) {
        return Err(format!("bg: refusing non-bg session {value}"));
    }
    bg_validate_tmux_target(value)
}

