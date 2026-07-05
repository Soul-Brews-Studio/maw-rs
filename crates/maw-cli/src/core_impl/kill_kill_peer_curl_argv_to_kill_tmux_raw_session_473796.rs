fn kill_peer_curl_argv(peer_url: &str, headers: &Headers, body: &str) -> Result<Vec<String>, String> {
    kill_validate_peer_url(peer_url)?;
    if body.chars().any(|ch| ch == '\0' || ch.is_control()) { return Err("kill peer body must not contain NUL/control characters".to_owned()); }
    let url = format!("{}{}", peer_url.trim_end_matches('/'), KILL_PEER_API_PATH);
    let mut argv = vec![
        "-sS".to_owned(),
        "--max-time".to_owned(),
        KILL_PEER_CURL_TIMEOUT_SECONDS.to_owned(),
        "-X".to_owned(),
        "POST".to_owned(),
        "-w".to_owned(),
        format!("{KILL_PEER_HTTP_STATUS_MARKER}%{{http_code}}"),
        "-H".to_owned(),
        "Content-Type: application/json".to_owned(),
    ];
    for (name, value) in headers.to_btree_map() {
        argv.push("-H".to_owned());
        argv.push(format!("{name}: {value}"));
    }
    argv.push("--data-binary".to_owned());
    argv.push(body.to_owned());
    argv.push("--".to_owned());
    argv.push(url);
    kill_validate_curl_argv(&argv)?;
    Ok(argv)
}

fn kill_validate_curl_argv(argv: &[String]) -> Result<(), String> {
    if !argv.iter().any(|arg| arg == "--") { return Err("curl argv must include -- URL separator".to_owned()); }
    for arg in argv {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("curl argv must not contain NUL/control characters".to_owned());
        }
    }
    Ok(())
}

fn kill_spawn_curl(argv: &[String]) -> Result<String, String> {
    kill_validate_curl_argv(argv)?;
    let output = std::process::Command::new("curl")
        .args(argv)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("failed to spawn curl: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(format!("curl failed: {}", if stdout.is_empty() { stderr } else { stdout }));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("curl stdout was not utf8: {error}"))
}

fn kill_split_peer_http_output(raw: &str) -> Result<(u16, String), String> {
    let Some((body, status_raw)) = raw.rsplit_once(KILL_PEER_HTTP_STATUS_MARKER) else {
        return Err("curl output missing HTTP status marker".to_owned());
    };
    let status = status_raw.trim().parse::<u16>().map_err(|_| format!("invalid HTTP status from curl: {status_raw}"))?;
    Ok((status, body.trim_end_matches('\n').to_owned()))
}

fn kill_parse_peer_response(alias: &str, peer_url: &str, status: u16, raw: &str) -> Result<KillPeerResponse, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| format!("peer kill failed ({alias} {peer_url}): invalid json: {error}; body={raw}"))?;
    if status == 404 {
        return Err(format!("peer {alias} does not support /api/kill (HTTP 404 at {peer_url})"));
    }
    if status >= 400 {
        let detail = value.get("error").and_then(serde_json::Value::as_str).unwrap_or("request failed");
        return Err(format!("peer kill failed ({alias} {peer_url}): {detail}"));
    }
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(KillPeerResponse { output: value.get("output").and_then(serde_json::Value::as_str).map(ToOwned::to_owned) });
    }
    let detail = value.get("error").and_then(serde_json::Value::as_str).unwrap_or("remote returned ok=false");
    Err(format!("peer kill failed ({alias} {peer_url}): {detail}"))
}

fn kill_now_seconds() -> i64 { i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX) }

fn kill_parse_non_negative(value: &str, flag: &str) -> Result<u32, String> {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!(
            "{flag} must be a non-negative integer (got {value})"
        ));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("{flag} must be a non-negative integer (got {value})"))
}

fn kill_split_target(target: &str) -> (String, String) {
    target.split_once(':').map_or_else(
        || (target.to_owned(), String::new()),
        |(session, window)| (session.to_owned(), window.to_owned()),
    )
}

fn kill_resolve_and_apply(
    tmux: &mut impl KillTmux,
    sessions: &[KillSession],
    raw_session: &str,
    raw_window: &str,
    options: &KillOptions,
) -> Result<String, String> {
    let names = sessions
        .iter()
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    match resolve_session_target(raw_session, &names) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => {
            let session = kill_find_session(sessions, &matched)?;
            kill_apply_resolved(tmux, session, raw_window, options)
        }
        ResolveResult::Ambiguous { candidates } => Err(kill_ambiguous_session(
            raw_session,
            &kill_sessions_for_names(sessions, &candidates),
        )),
        ResolveResult::None { hints } => {
            let hint_sessions = hints.map(|names| kill_sessions_for_names(sessions, &names));
            kill_apply_orphan_pane_fallback(
                tmux,
                raw_session,
                raw_window,
                options,
                hint_sessions.as_deref(),
            )
        }
    }
}

fn kill_find_session<'a>(
    sessions: &'a [KillSession],
    name: &str,
) -> Result<&'a KillSession, String> {
    sessions
        .iter()
        .find(|session| session.name == name)
        .ok_or_else(|| format!("session '{name}' not found after resolution"))
}

fn kill_sessions_for_names(sessions: &[KillSession], names: &[String]) -> Vec<KillSession> {
    names
        .iter()
        .filter_map(|name| sessions.iter().find(|session| session.name == *name))
        .cloned()
        .collect()
}

fn kill_apply_resolved(
    tmux: &mut impl KillTmux,
    session: &KillSession,
    raw_window: &str,
    options: &KillOptions,
) -> Result<String, String> {
    kill_validate_tmux_target(&session.name)?;
    let indexes = kill_matching_window_indexes(session, raw_window, options)?;
    if let Some(pane) = options.pane {
        return kill_kill_resolved_pane(tmux, session, indexes.first().copied(), pane);
    }
    if raw_window.is_empty() && options.index.is_none() && !options.all {
        tmux.kill_kill_session(&session.name)?;
        return Ok(format!(
            "  \x1b[32m✓\x1b[0m killed session {}\n",
            session.name
        ));
    }
    kill_kill_resolved_windows(tmux, session, &indexes, options)
}

fn kill_apply_orphan_pane_fallback(
    tmux: &mut impl KillTmux,
    raw_session: &str,
    raw_window: &str,
    options: &KillOptions,
    hints: Option<&[KillSession]>,
) -> Result<String, String> {
    if raw_window.is_empty() && options.pane.is_none() {
        let pane_raw = tmux.kill_list_panes_all().unwrap_or_default();
        if !pane_raw.trim().is_empty() {
            return kill_resolve_orphan_pane(tmux, raw_session, &pane_raw);
        }
    }
    Err(kill_missing_session(raw_session, hints))
}

fn kill_resolve_orphan_pane(
    tmux: &mut impl KillTmux,
    raw_session: &str,
    pane_raw: &str,
) -> Result<String, String> {
    match maw_tmux::resolve_pane_target_from_list_panes_output(raw_session, pane_raw) {
        maw_tmux::PaneTargetResolution::Match { candidate } => {
            kill_validate_tmux_target(&candidate.resolved)?;
            tmux.kill_kill_pane(&candidate.resolved)?;
            Ok(format!(
                "  \x1b[32m✓\x1b[0m killed pane {raw_session} → {} \x1b[90m[{} ({})]\x1b[0m\n",
                candidate.resolved, candidate.source, candidate.name
            ))
        }
        maw_tmux::PaneTargetResolution::Ambiguous { candidates } => {
            Err(kill_ambiguous_panes(raw_session, &candidates))
        }
        maw_tmux::PaneTargetResolution::None => Err(kill_missing_session(raw_session, None)),
    }
}

