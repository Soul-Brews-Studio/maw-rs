fn kill_matching_window_indexes(
    session: &KillSession,
    raw_window: &str,
    options: &KillOptions,
) -> Result<Vec<u32>, String> {
    if options.all && options.index.is_some() {
        return Err("cannot combine --all and --index".to_owned());
    }
    if options.all && options.pane.is_some() {
        return Err("cannot combine --all and --pane".to_owned());
    }
    if let Some(index) = options.index {
        kill_require_window_index(session, index)?;
        return Ok(vec![index]);
    }
    if raw_window.is_empty() {
        return Ok(Vec::new());
    }
    if raw_window.chars().all(|ch| ch.is_ascii_digit()) {
        let index = kill_parse_non_negative(raw_window, "window index")?;
        kill_require_window_index(session, index)?;
        return Ok(vec![index]);
    }
    let matches = session
        .windows
        .iter()
        .filter(|window| window.name.eq_ignore_ascii_case(raw_window))
        .map(|window| window.index)
        .collect::<Vec<_>>();
    kill_validate_window_matches(session, raw_window, &matches, options.all)
}

fn kill_validate_window_matches(
    session: &KillSession,
    raw_window: &str,
    matches: &[u32],
    all: bool,
) -> Result<Vec<u32>, String> {
    if matches.is_empty() {
        return Err(format!(
            "window '{raw_window}' not found in session {} (valid: {})",
            session.name,
            kill_window_labels(session)
        ));
    }
    if matches.len() > 1 && !all {
        return Err(kill_ambiguous_window(session, raw_window, matches));
    }
    Ok(matches.to_vec())
}

fn kill_require_window_index(session: &KillSession, index: u32) -> Result<(), String> {
    if session.windows.iter().any(|window| window.index == index) {
        Ok(())
    } else {
        Err(format!(
            "window index {index} does not exist in session {} (valid: {})",
            session.name,
            kill_window_labels(session)
        ))
    }
}

fn kill_kill_resolved_pane(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    window_index: Option<u32>,
    pane_index: u32,
) -> Result<String, String> {
    let win =
        window_index.unwrap_or_else(|| session.windows.first().map_or(0, |window| window.index));
    kill_require_window_index(session, win)?;
    let win_target = format!("{}:{win}", session.name);
    kill_validate_tmux_target(&win_target)?;
    let valid = tmux.kill_list_pane_indexes(&win_target)?;
    if !valid.contains(&pane_index) {
        let list = kill_number_list(&valid);
        return Err(format!(
            "pane {pane_index} does not exist in window {win_target} (valid: {list})"
        ));
    }
    let pane = format!("{win_target}.{pane_index}");
    kill_validate_tmux_target(&pane)?;
    tmux.kill_kill_pane(&pane)?;
    Ok(format!("  \x1b[32m✓\x1b[0m killed pane {pane}\n"))
}

fn kill_kill_resolved_windows(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    indexes: &[u32],
    options: &KillOptions,
) -> Result<String, String> {
    if indexes.is_empty() {
        return Err(if options.all {
            "--all requires a window name target (session:window)".to_owned()
        } else {
            "window target required".to_owned()
        });
    }
    let mut killed = Vec::new();
    for index in indexes {
        let target = format!("{}:{index}", session.name);
        kill_validate_tmux_target(&target)?;
        tmux.kill_kill_window(&target)?;
        killed.push(target);
    }
    Ok(kill_window_success(&killed))
}

fn kill_window_success(killed: &[String]) -> String {
    if killed.len() == 1 {
        format!("  \x1b[32m✓\x1b[0m killed window {}\n", killed[0])
    } else {
        format!(
            "  \x1b[32m✓\x1b[0m killed {} windows {}\n",
            killed.len(),
            killed.join(", ")
        )
    }
}

fn kill_parse_sessions(raw: &str) -> Vec<KillSession> {
    let mut sessions = Vec::<KillSession>::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        kill_push_window(&mut sessions, line);
    }
    sessions
}

fn kill_push_window(sessions: &mut Vec<KillSession>, line: &str) {
    let parts = line.split("|||").collect::<Vec<_>>();
    let name = parts.first().copied().unwrap_or_default().to_owned();
    let index = parts
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let window = KillWindow {
        index,
        name: parts.get(2).copied().unwrap_or_default().to_owned(),
    };
    if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
        session.windows.push(window);
    } else {
        sessions.push(KillSession {
            name,
            windows: vec![window],
        });
    }
}

fn kill_parse_numbers(raw: &str) -> Vec<u32> {
    raw.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

fn kill_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    runner.run(subcommand, &args).map_err(|error| error.message)
}

fn kill_validate_user_target(target: &str) -> Result<(), String> {
    if target.is_empty()
        || target.trim() != target
        || target.starts_with('-')
        || target.contains('\0')
    {
        Err("kill target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn kill_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty()
        || target.trim() != target
        || target.starts_with('-')
        || target.contains('\0')
    {
        Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn kill_window_labels(session: &KillSession) -> String {
    if session.windows.is_empty() {
        return "(none)".to_owned();
    }
    session
        .windows
        .iter()
        .map(|window| format!("{}:{}", window.index, window.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn kill_number_list(values: &[u32]) -> String {
    if values.is_empty() {
        "(none)".to_owned()
    } else {
        values
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

