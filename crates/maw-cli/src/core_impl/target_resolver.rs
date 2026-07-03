fn resolve_local_tmux_runner_target<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    query: &str,
    command: &str,
) -> Result<String, String> {
    if query.starts_with('%') {
        return Ok(query.to_owned());
    }
    let sessions = route_sessions_from_tmux_runner(runner, command)?;
    resolve_local_tmux_target_from_sessions(query, &sessions)
}

fn route_sessions_from_tmux_runner<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    command: &str,
) -> Result<Vec<RouteSession>, String> {
    let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}".to_owned(),
            ],
        )
        .map_err(|error| format!("{command} target resolution failed: {}", error.message))?;
    Ok(tmux_sessions_to_route_sessions(maw_tmux::parse_list_all_windows(&raw)))
}

fn tmux_sessions_to_route_sessions(sessions: Vec<TmuxSession>) -> Vec<RouteSession> {
    sessions
        .into_iter()
        .map(tmux_session_to_route_session)
        .collect()
}

fn tmux_session_to_route_session(session: TmuxSession) -> RouteSession {
    RouteSession {
        name: session.name,
        source: None,
        windows: session
            .windows
            .into_iter()
            .map(|window| RouteWindow {
                index: window.index,
                name: window.name,
                active: window.active,
                kind: None,
            })
            .collect(),
    }
}

fn resolve_local_tmux_target_from_sessions(
    query: &str,
    sessions: &[RouteSession],
) -> Result<String, String> {
    match resolve_route_target(query, &RouteConfig::default(), sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => Ok(target),
        RouteResult::Peer { node, target, .. } => Err(format!(
            "cross-node target '{query}' (node '{node}', target '{target}') is not supported"
        )),
        RouteResult::Error { detail, hint, .. } => {
            if let Some(hint) = hint {
                Err(format!("{detail} — {hint}"))
            } else {
                Err(detail)
            }
        }
    }
}
