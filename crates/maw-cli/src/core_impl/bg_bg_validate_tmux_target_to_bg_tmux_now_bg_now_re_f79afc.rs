fn bg_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("bg tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("bg tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_tmux_subcommand(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value == "--" || value.contains(char::is_whitespace) {
        return Err("bg tmux subcommand must be a safe token".to_owned());
    }
    Ok(())
}

fn bg_validate_tmux_args(args: &[String]) -> Result<(), String> {
    for pair in args.windows(2) {
        if pair[0] == "-t" || pair[0] == "-s" {
            bg_validate_tmux_target(&pair[1])?;
        }
    }
    Ok(())
}

fn bg_session_name(slug: &str) -> String {
    format!("{BG_PREFIX}{slug}")
}

fn bg_session_slug(session: &str) -> Option<String> {
    session.strip_prefix(BG_PREFIX).map(str::to_owned)
}

fn bg_new_session_args(session: &str, command: &str) -> Result<Vec<String>, String> {
    bg_validate_session_name(session)?;
    bg_validate_command(command)?;
    Ok(vec![
        "-d".to_owned(),
        "-s".to_owned(),
        session.to_owned(),
        "-n".to_owned(),
        "bg".to_owned(),
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        bg_holds_open(command),
    ])
}

fn bg_holds_open(command: &str) -> String {
    format!("{command}; rc=$?; printf '\\n[done — exit %d]\\n' \"$rc\"; while :; do read -r _ 2>/dev/null || sleep 3600; done")
}

fn bg_session_exists(slug: &str, tmux: &mut impl BgTmux) -> Result<bool, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec!["-t".to_owned(), session];
    Ok(tmux.bg_run("has-session", &args)?.status == 0)
}

fn bg_list_sessions(tmux: &mut impl BgTmux, now: BgNow) -> Result<Vec<BgSession>, String> {
    let args = vec!["-F".to_owned(), BG_LIST_FORMAT.to_owned()];
    let result = tmux.bg_run("list-sessions", &args)?;
    if result.status != 0 && bg_list_error_is_empty(&result) {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for line in result.stdout.lines() {
        if let Some(session) = bg_session_from_line(line, tmux, now)? {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

fn bg_session_from_line(
    line: &str,
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<Option<BgSession>, String> {
    let mut fields = line.split('\t');
    let name = fields.next().unwrap_or_default();
    if !name.starts_with(BG_PREFIX) {
        return Ok(None);
    }
    bg_validate_session_name(name)?;
    let created = fields.next().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or_else(now);
    let command = fields.next().unwrap_or_default();
    let slug = bg_session_slug(name).unwrap_or_default();
    bg_validate_ref(&slug)?;
    Ok(Some(BgSession {
        slug: slug.clone(),
        session: name.to_owned(),
        age_seconds: now().saturating_sub(created),
        status: bg_status_from_pane_command(command),
        last_line: bg_last_line_of(&slug, tmux).unwrap_or_default(),
    }))
}

fn bg_list_error_is_empty(result: &BgTmuxResult) -> bool {
    result.stdout.trim().is_empty()
        || result.stderr.contains("no server running")
        || result.stderr.contains("no current session")
}

fn bg_status_from_pane_command(command: &str) -> BgSessionStatus {
    match command.trim().to_ascii_lowercase().as_str() {
        "" | "read" | "sleep" | "sh" => BgSessionStatus::Done,
        _ => BgSessionStatus::Running,
    }
}

fn bg_last_line_of(slug: &str, tmux: &mut impl BgTmux) -> Result<String, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec![
        "-p".to_owned(),
        "-J".to_owned(),
        "-t".to_owned(),
        session,
        "-S".to_owned(),
        "-1".to_owned(),
        "-E".to_owned(),
        "-1".to_owned(),
    ];
    let result = tmux.bg_run("capture-pane", &args)?;
    if result.status == 0 {
        Ok(result.stdout.trim_end_matches('\n').trim().to_owned())
    } else {
        Ok(String::new())
    }
}

fn bg_list_slugs(tmux: &mut impl BgTmux, now: BgNow) -> Result<Vec<String>, String> {
    Ok(bg_list_sessions(tmux, now)?.into_iter().map(|session| session.slug).collect())
}

fn bg_resolve_slug(reference: &str, live: &[String]) -> Result<String, String> {
    bg_validate_ref(reference)?;
    if live.iter().any(|slug| slug == reference) {
        return Ok(reference.to_owned());
    }
    if bg_is_hash_ref(reference) {
        let hits = live.iter().filter(|slug| slug.ends_with(&format!("-{reference}"))).cloned().collect::<Vec<_>>();
        return bg_resolve_hits(reference, &hits, "hash");
    }
    let hits = live.iter().filter(|slug| slug.starts_with(reference)).cloned().collect::<Vec<_>>();
    bg_resolve_hits(reference, &hits, "ref")
}

fn bg_resolve_hits(reference: &str, hits: &[String], kind: &str) -> Result<String, String> {
    match hits {
        [hit] => Ok(hit.clone()),
        [] => Err(format!("bg: no session matching \"{reference}\"")),
        _ if kind == "hash" => Err(format!("bg: hash \"{reference}\" matches {} sessions: {}", hits.len(), hits.join(", "))),
        _ => Err(format!("bg: ref \"{reference}\" matches {} sessions: {}", hits.len(), hits.join(", "))),
    }
}

fn bg_is_hash_ref(value: &str) -> bool {
    value.len() == 4 && value.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

fn bg_tail_resolved(slug: &str, lines: u32, tmux: &mut impl BgTmux) -> Result<String, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec!["-p".to_owned(), "-J".to_owned(), "-t".to_owned(), session, "-S".to_owned(), format!("-{lines}")];
    let result = tmux.bg_run("capture-pane", &args)?;
    if result.status != 0 {
        return Err(format!("bg: capture-pane failed for {slug}: {}", bg_stderr_or_placeholder(&result.stderr)));
    }
    Ok(result.stdout.trim_end_matches('\n').to_owned())
}

fn bg_tail_output(mut output: String, follow: bool) -> String {
    if follow {
        let _ = writeln!(output, "\n[bg: follow is single-snapshot in maw-rs native]");
    }
    output
}

fn bg_attach_args(slug: &str, inside_tmux: bool) -> Result<Vec<String>, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    if inside_tmux {
        Ok(vec!["switch-client".to_owned(), "-t".to_owned(), session])
    } else {
        Ok(vec!["attach-session".to_owned(), "-t".to_owned(), session])
    }
}

fn bg_kill(
    slug: Option<&String>,
    all: bool,
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<Vec<String>, String> {
    if all {
        let slugs = bg_list_slugs(tmux, now)?;
        for slug in &slugs {
            bg_kill_session(slug, tmux)?;
        }
        return Ok(slugs);
    }
    let slug_ref = slug.ok_or_else(|| "bg kill: missing <slug> (or --all)".to_owned())?;
    bg_validate_ref(slug_ref)?;
    let resolved = bg_resolve_slug(slug_ref, &bg_list_slugs(tmux, now)?)?;
    bg_kill_session(&resolved, tmux)?;
    Ok(vec![resolved])
}

