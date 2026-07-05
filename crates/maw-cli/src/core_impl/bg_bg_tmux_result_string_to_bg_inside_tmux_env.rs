fn bg_kill_session(slug: &str, tmux: &mut impl BgTmux) -> Result<(), String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let result = tmux.bg_run("kill-session", &["-t".to_owned(), session])?;
    if result.status != 0 {
        return Err(format!("bg: kill-session failed for {slug}: {}", bg_stderr_or_placeholder(&result.stderr)));
    }
    Ok(())
}

fn bg_parse_duration(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    let Some(unit) = trimmed.chars().last() else {
        return Err(bg_bad_duration(value));
    };
    let number = &trimmed[..trimmed.len() - unit.len_utf8()];
    let parsed = number.parse::<u64>().map_err(|_| bg_bad_duration(value))?;
    match unit {
        's' => Ok(parsed),
        'm' => Ok(parsed.saturating_mul(60)),
        'h' => Ok(parsed.saturating_mul(3_600)),
        'd' => Ok(parsed.saturating_mul(86_400)),
        _ => Err(bg_bad_duration(value)),
    }
}

fn bg_bad_duration(value: &str) -> String {
    format!("bg gc: invalid --older-than \"{value}\" (expected NNs/NNm/NNh/NNd)")
}

fn bg_parse_lines(value: &str) -> Result<u32, String> {
    let parsed = value.parse::<u32>().map_err(|_| format!("--lines must be a positive number, got {value}"))?;
    if parsed == 0 {
        return Err(format!("--lines must be a positive number, got {value}"));
    }
    Ok(parsed)
}

fn bg_format_list(sessions: &[BgSession]) -> String {
    if sessions.is_empty() {
        return "(no maw-bg sessions)\n".to_owned();
    }
    let rows = sessions.iter().map(bg_format_row_parts).collect::<Vec<_>>();
    let widths = bg_widths(&rows);
    let mut out = String::new();
    for row in rows {
        let _ = writeln!(
            out,
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
    out
}

fn bg_format_row_parts(session: &BgSession) -> [String; 4] {
    [
        session.slug.clone(),
        bg_status_text(&session.status).to_owned(),
        bg_format_age(session.age_seconds),
        bg_truncate_last_line(&session.last_line),
    ]
}

fn bg_widths(rows: &[[String; 4]]) -> [usize; 3] {
    let mut widths = [0usize; 3];
    for row in rows {
        for index in 0..3 {
            widths[index] = widths[index].max(row[index].len());
        }
    }
    widths
}

fn bg_status_text(status: &BgSessionStatus) -> &'static str {
    match status {
        BgSessionStatus::Running => "running",
        BgSessionStatus::Done => "done",
    }
}

fn bg_format_age(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn bg_truncate_last_line(line: &str) -> String {
    if line.len() > 60 {
        format!("{}...", &line[..57])
    } else {
        line.to_owned()
    }
}

fn bg_list_json(sessions: &[BgSession]) -> Result<String, String> {
    let values = sessions
        .iter()
        .map(|session| {
            serde_json::json!({
                "slug": session.slug,
                "session": session.session,
                "ageSeconds": session.age_seconds,
                "status": bg_status_text(&session.status),
                "lastLine": session.last_line,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&values)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}

fn bg_gc_output(dry_run: bool, reaped: &[String], kept: &[String], threshold: u64) -> String {
    let mut out = String::new();
    let verb = if dry_run { "would reap" } else { "reaped" };
    let _ = writeln!(out, "{verb}: {}", bg_join_or_none(reaped));
    let _ = writeln!(out, "kept:    {}", bg_join_or_none(kept));
    let _ = writeln!(out, "threshold: {threshold}s");
    out
}

fn bg_join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_owned()
    } else {
        values.join(", ")
    }
}

fn bg_tmux_failure(action: &str, status: i32, stderr: &str) -> String {
    format!("bg: tmux {action} failed (status {status}): {}", bg_stderr_or_placeholder(stderr))
}

fn bg_stderr_or_placeholder(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        "(no stderr)".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn bg_now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn bg_inside_tmux_env() -> bool {
    std::env::var_os("TMUX").is_some()
}

