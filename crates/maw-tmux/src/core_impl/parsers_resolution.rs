fn format_pane_ambiguity_error(target: &str, candidates: &[PaneTargetCandidate]) -> String {
    let lines = candidates
        .iter()
        .map(|candidate| {
            let target_note = if candidate.target.is_empty() {
                String::new()
            } else {
                format!(" ({})", candidate.target)
            };
            format!(
                "    • {} → {}{} [{}]",
                candidate.name, candidate.resolved, target_note, candidate.source
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "'{target}' is ambiguous — matches {} panes:\n{lines}\n  use the pane id or full session:window.pane target",
        candidates.len()
    )
}

fn basename(path: &str) -> &str {
    path.split('/')
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
}

fn nested_agents_worktree(cwd: &str) -> Option<(&str, &str)> {
    let parts = cwd
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let [.., repo, "agents", worktree] = parts.as_slice() else {
        return None;
    };
    Some((repo, worktree))
}

fn worktree_names_from_cwd(cwd: &str) -> Vec<(String, String)> {
    let (repo, base) = nested_agents_worktree(cwd).unwrap_or_else(|| ("", basename(cwd)));
    if base.is_empty() {
        return Vec::new();
    }
    let mut out = vec![(base.to_owned(), "worktree-dir".to_owned())];
    let nested = !repo.is_empty();
    let (repo, rest) = if nested {
        (repo, base)
    } else {
        let Some((repo, rest)) = base.split_once(".wt-") else {
            return out;
        };
        (repo, rest)
    };
    let role = rest
        .split_once('-')
        .map_or(if nested { rest } else { "" }, |(_, role)| role)
        .trim();
    if !role.is_empty() {
        out.push((role.to_owned(), "worktree-role".to_owned()));
        if let Some(repo_stem) = repo.strip_suffix("-oracle") {
            if !repo_stem.is_empty() {
                out.push((format!("{repo_stem}-{role}"), "worktree-alias".to_owned()));
            }
        }
    }
    out
}

/// Parse `PANE_TARGET_FORMAT` rows into pane target resolution candidates.
#[must_use]
pub fn pane_target_candidates_from_list_panes_output(raw: &str) -> Vec<PaneTargetCandidate> {
    let mut candidates = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split("|||").collect::<Vec<_>>();
        let id = fields.first().copied().unwrap_or_default().trim();
        let target = fields.get(1).copied().unwrap_or_default().trim();
        let title = fields.get(2).copied().unwrap_or_default();
        let tile_role = fields.get(3).copied().unwrap_or_default();
        let cwd = fields.get(4).copied().unwrap_or_default();
        let resolved = if id.is_empty() { target } else { id };
        if resolved.is_empty() {
            continue;
        }
        add_pane_target_candidate(&mut candidates, title, resolved, "pane-title", target);
        add_pane_target_candidate(&mut candidates, tile_role, resolved, "tile-role", target);
        for (name, source) in worktree_names_from_cwd(cwd) {
            add_pane_target_candidate(&mut candidates, &name, resolved, &source, target);
        }
    }
    candidates
}

fn add_pane_target_candidate(
    candidates: &mut Vec<PaneTargetCandidate>,
    name: &str,
    resolved: &str,
    source: &str,
    target: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    candidates.push(PaneTargetCandidate {
        name: name.to_owned(),
        resolved: resolved.to_owned(),
        source: source.to_owned(),
        target: target.to_owned(),
    });
}

fn unique_by_resolved(candidates: Vec<PaneTargetCandidate>) -> Vec<PaneTargetCandidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.resolved.clone()) {
            out.push(candidate);
        }
    }
    out
}

/// Resolve a natural pane title, tile role, worktree dir, or worktree alias to a pane id.
#[must_use]
pub fn resolve_pane_target_from_candidates(
    target: &str,
    candidates: &[PaneTargetCandidate],
) -> PaneTargetResolution {
    let trimmed = target.trim().to_lowercase();
    let exact = unique_by_resolved(
        candidates
            .iter()
            .filter(|candidate| candidate.name.to_lowercase() == trimmed)
            .cloned()
            .collect(),
    );
    match exact.len() {
        1 => {
            return PaneTargetResolution::Match {
                candidate: exact[0].clone(),
            }
        }
        2.. => return PaneTargetResolution::Ambiguous { candidates: exact },
        0 => {}
    }

    match resolve_by_name(target, candidates, ResolveOptions::default()) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => {
            PaneTargetResolution::Match { candidate: matched }
        }
        ResolveResult::Ambiguous { candidates } => PaneTargetResolution::Ambiguous {
            candidates: unique_by_resolved(candidates),
        },
        ResolveResult::None { .. } => PaneTargetResolution::None,
    }
}

/// Resolve a pane target directly from `PANE_TARGET_FORMAT` list-panes output.
#[must_use]
pub fn resolve_pane_target_from_list_panes_output(target: &str, raw: &str) -> PaneTargetResolution {
    resolve_pane_target_from_candidates(target, &pane_target_candidates_from_list_panes_output(raw))
}

/// Parse `tmux list-sessions -F '#{session_name}\t#{session_created}'` style epoch rows.
#[must_use]
pub fn parse_session_epoch_list(raw: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some((name, epoch_raw)) = line.split_once('\t') else {
            continue;
        };
        let Ok(epoch) = epoch_raw.parse::<u64>() else {
            continue;
        };
        if !name.is_empty() && epoch > 0 {
            out.insert(name.to_owned(), epoch);
        }
    }
    out
}

/// Parse tmux session creation rows.
#[must_use]
pub fn parse_session_created_list(raw: &str) -> BTreeMap<String, u64> {
    parse_session_epoch_list(raw)
}

/// Parse tmux session activity rows.
#[must_use]
pub fn parse_session_activity_list(raw: &str) -> BTreeMap<String, u64> {
    parse_session_epoch_list(raw)
}

/// Parse `maw ls --active` duration values. Bare numbers are minutes.
#[must_use]
pub fn parse_active_duration_seconds(raw: Option<&str>) -> Option<u64> {
    let trimmed = raw?.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let last = trimmed.chars().last()?;
    let (digits, multiplier) = match last {
        's' | 'm' | 'h' | 'd' => (&trimmed[..trimmed.len() - 1], active_duration_multiplier(last)),
        _ => (trimmed.as_str(), 60),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    value.checked_mul(multiplier)
}

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

/// Parse maw-js `list-windows -a` format.
#[must_use]
pub fn parse_list_all_windows(raw: &str) -> Vec<TmuxSession> {
    let mut sessions: Vec<TmuxSession> = Vec::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let fields = line.split("|||").collect::<Vec<_>>();
        if fields.len() < 4 {
            continue;
        }
        let session_name = fields[0];
        let window = TmuxWindow {
            index: fields[1].parse().unwrap_or(0),
            name: fields[2].to_owned(),
            active: fields[3] == "1",
            cwd: fields
                .get(4)
                .and_then(|cwd| (!cwd.is_empty()).then(|| (*cwd).to_owned())),
        };
        if let Some(session) = sessions
            .iter_mut()
            .find(|session| session.name == session_name)
        {
            session.windows.push(window);
        } else {
            sessions.push(TmuxSession {
                name: session_name.to_owned(),
                windows: vec![window],
            });
        }
    }
    sessions
}

/// Parse maw-js `list-windows -t <session> -F '#{window_index}:#{window_name}:#{window_active}'` output.
#[must_use]
pub fn parse_list_windows(raw: &str) -> Vec<TmuxWindow> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, ':');
            let index = parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let name = parts.next().unwrap_or_default().to_owned();
            let active = parts.next() == Some("1");
            TmuxWindow {
                index,
                name,
                active,
                cwd: None,
            }
        })
        .collect()
}

/// Parse `tmux list-panes -a -F '#{pane_id}'` output.
#[must_use]
pub fn parse_pane_ids(raw: &str) -> BTreeSet<String> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parse maw-js structured `list-panes -a` format.
#[must_use]
pub fn parse_list_panes(raw: &str) -> Vec<TmuxPane> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let fields = line.split("|||").collect::<Vec<_>>();
            (fields.len() >= 4).then(|| TmuxPane {
                id: fields[0].to_owned(),
                command: fields[1].to_owned(),
                target: fields[2].to_owned(),
                title: fields[3].to_owned(),
                pid: fields.get(4).and_then(|pid| pid.parse().ok()),
                cwd: fields
                    .get(5)
                    .and_then(|cwd| (!cwd.is_empty()).then(|| (*cwd).to_owned())),
                last_activity: fields.get(6).and_then(|activity| activity.parse().ok()),
            })
        })
        .collect()
}
