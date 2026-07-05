fn resolve_explicit_local_session_window_target(
    query: &str,
    sessions: &[Session],
    route_type: RouteType,
) -> Option<ResolveResult> {
    let (session_part, raw_window_part) = query.split_once(':')?;
    let session = match_session(sessions, session_part, true)?;

    if raw_window_part.is_empty() {
        return Some(route_target(
            route_type,
            session.windows.first().map_or_else(
                || format!("{}:", session.name),
                |window| format!("{}:{}", session.name, window.index),
            ),
        ));
    }

    Some(resolve_exact_session_window(
        session,
        raw_window_part,
        route_type,
    ))
}

fn resolve_exact_session_window(
    session: &Session,
    raw_window_part: &str,
    route_type: RouteType,
) -> ResolveResult {
    let (window_part, pane_suffix) = split_pane_suffix(raw_window_part);
    if window_part.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(index) = window_part.parse::<u32>() {
            if let Some(window) = session.windows.iter().find(|window| window.index == index) {
                return route_target(route_type, format!("{}:{}{pane_suffix}", session.name, window.index));
            }
        }
        return session_window_not_found_error(session, window_part);
    }

    if let Some(window) = session
        .windows
        .iter()
        .find(|window| window.name.eq_ignore_ascii_case(window_part))
    {
        return route_target(route_type, format!("{}:{}{pane_suffix}", session.name, window.index));
    }

    session_window_not_found_error(session, window_part)
}

fn session_window_not_found_error(session: &Session, window_part: &str) -> ResolveResult {
    error(
        "session_window_not_found",
        format!("no window '{window_part}' in session '{}'", session.name),
        Some(format!("windows: {}", session_window_list(session))),
    )
}

fn fleet_window_candidate_names(query: &str) -> Vec<String> {
    let raw = query.trim();
    let stripped = raw.strip_suffix("-oracle").unwrap_or(raw);
    let unnumbered = strip_numeric_fleet_prefix(raw);
    let stripped_unnumbered = unnumbered.strip_suffix("-oracle").unwrap_or(unnumbered);
    let mut names = Vec::new();
    if !raw.is_empty() {
        names.push(raw.to_owned());
    }
    if stripped != raw {
        names.push(stripped.to_owned());
    }
    if unnumbered != raw {
        names.push(unnumbered.to_owned());
    }
    if stripped_unnumbered != unnumbered {
        names.push(stripped_unnumbered.to_owned());
    }
    if !stripped.is_empty() {
        names.push(format!("{stripped}-oracle"));
    }
    if !raw.to_lowercase().ends_with("-oracle") && !raw.is_empty() {
        names.push(format!("{raw}-oracle"));
    }
    if !stripped_unnumbered.is_empty() {
        names.push(format!("{stripped_unnumbered}-oracle"));
    }
    unique_strings(names)
}

fn alias_query_is_oracle(query: &str, sessions: &[Session]) -> bool {
    if !query.trim().to_lowercase().ends_with("-oracle") {
        return false;
    }
    match declared_alias_kind(query, sessions) {
        Some(RepoKind::Project) => false,
        Some(RepoKind::Oracle) | None => true,
    }
}

fn declared_alias_kind(query: &str, sessions: &[Session]) -> Option<RepoKind> {
    let candidates = fleet_window_candidate_names(query);
    let mut found = None;
    for session in sessions {
        let session_matches = session_alias_names(&session.name)
            .iter()
            .any(|name| candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case(name)));
        for window in &session.windows {
            let window_matches = candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case(&window.name));
            if !(session_matches || window_matches) {
                continue;
            }
            if let Some(kind) = declared_window_kind(window) {
                if found.is_some() && found != Some(kind) {
                    return None;
                }
                found = Some(kind);
            }
        }
    }
    found
}

fn declared_window_kind(window: &Window) -> Option<RepoKind> {
    window.kind
}

fn session_alias_names(name: &str) -> Vec<String> {
    let raw = name.trim();
    let unnumbered = strip_numeric_fleet_prefix(raw);
    unique_strings(
        [
            nonempty(raw).map(str::to_owned),
            raw.strip_suffix("-oracle").map(str::to_owned),
            nonempty(unnumbered).map(str::to_owned),
            unnumbered.strip_suffix("-oracle").map(str::to_owned),
        ]
        .into_iter()
        .flatten(),
    )
}

