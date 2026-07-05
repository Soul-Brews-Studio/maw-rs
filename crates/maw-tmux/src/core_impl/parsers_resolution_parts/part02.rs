fn active_duration_multiplier(unit: char) -> u64 {
    match unit {
        's' => 1,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        _ => 60,
    }
}

/// Return the valid duration argument supplied to a flag such as `--active`.
#[must_use]
pub fn active_duration_arg(argv: &[String], flag: &str) -> Option<String> {
    for (index, arg) in argv.iter().enumerate() {
        if arg == flag {
            let next = argv.get(index + 1)?;
            return (!next.starts_with('-') && parse_active_duration_seconds(Some(next)).is_some())
                .then(|| next.clone());
        }
        if let Some(value) = active_duration_inline_value(arg, flag) {
            return Some(value);
        }
    }
    None
}

fn active_duration_inline_value(arg: &str, flag: &str) -> Option<String> {
    let value = arg.strip_prefix(&format!("{flag}="))?;
    parse_active_duration_seconds(Some(value)).map(|_| value.to_owned())
}

/// Format an epoch second as a deterministic UTC timestamp.
#[must_use]
pub fn format_session_created(epoch_seconds: Option<u64>) -> String {
    let Some(epoch_seconds) = epoch_seconds.filter(|epoch| *epoch > 0) else {
        return "—".to_owned();
    };
    let days = i64::try_from(epoch_seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year, month, day)
}

/// Return unique matching oracle repo slugs, preserving input order.
#[must_use]
pub fn similar_oracle_candidates_from_repos(target: &str, repos: &[String]) -> Vec<String> {
    let query = target.to_lowercase();
    let mut out = Vec::new();
    for repo in repos {
        let name = repo_name_from_path(repo);
        if !name.ends_with("-oracle") || !name.to_lowercase().contains(&query) {
            continue;
        }
        let slug = repo_slug_from_path(repo);
        if !out.iter().any(|existing| existing == &slug) {
            out.push(slug);
        }
    }
    out
}

fn repo_name_from_path(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn repo_slug_from_path(path: &str) -> String {
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() >= 2 {
        parts[parts.len() - 2..].join("/")
    } else {
        repo_name_from_path(path).to_owned()
    }
}

/// Annotate a pane for `maw tmux ls`: team > fleet > view > orphan > empty.
#[must_use]
pub fn annotate_pane(
    pane: &TmuxLsPaneRef,
    fleet_sessions: &BTreeSet<String>,
    team_by_pane: &BTreeMap<String, String>,
) -> String {
    let session = pane
        .target
        .split_once(':')
        .map_or(pane.target.as_str(), |(session, _)| session);
    if let Some(team) = team_by_pane.get(&pane.id) {
        return format!("team: {team}");
    }
    if fleet_sessions.contains(session) {
        return format!("fleet: {}", strip_numeric_prefix(session));
    }
    if session == "maw-view" || session.ends_with("-view") {
        return format!("view: {session}");
    }
    if is_claude_like_pane(pane.command.as_deref()) {
        return "orphan".to_owned();
    }
    String::new()
}

/// Normalize pane metadata keys to tmux `@custom` option names.
#[must_use]
pub fn normalize_pane_tag_key(raw_key: &str) -> String {
    if raw_key.starts_with('@') {
        raw_key.to_owned()
    } else {
        format!("@{raw_key}")
    }
}

/// Parse `show-options -p -t <pane>` output for tmux `@custom` metadata.
#[must_use]
pub fn parse_pane_tag_options(raw: &str) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if !key.starts_with('@') {
            continue;
        }
        let value = parse_tmux_option_value(rest.trim());
        meta.insert(key.to_owned(), value);
    }
    meta
}

fn parse_tmux_option_value(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return unescape_tmux_quoted_value(&value[1..value.len() - 1]);
    }
    value.to_owned()
}

fn unescape_tmux_quoted_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Shell-quote one tmux command argument using the same safe-character policy as maw-js.
#[must_use]
pub fn shell_quote(value: impl fmt::Display) -> String {
    let value = value.to_string();
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-' | b'/')
        })
    {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Build the shell command used by maw-js-style `tmux [-S socket] subcommand args...` execution.
#[must_use]
pub fn tmux_shell_command(socket: Option<&str>, subcommand: &str, args: &[String]) -> String {
    let socket_flag =
        socket.map_or_else(String::new, |socket| format!("-S {} ", shell_quote(socket)));
    let joined_args = args.iter().map(shell_quote).collect::<Vec<_>>().join(" ");
    if joined_args.is_empty() {
        format!("tmux {socket_flag}{subcommand}")
    } else {
        format!("tmux {socket_flag}{subcommand} {joined_args}")
    }
}

/// Parse `tmux list-sessions -F '#{session_name}'` output.
#[must_use]
pub fn parse_session_names(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
