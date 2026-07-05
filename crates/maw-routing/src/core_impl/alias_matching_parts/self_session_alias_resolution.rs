fn find_peer_url(node_name: &str, config: &MawConfig) -> Option<String> {
    config
        .named_peers
        .iter()
        .find(|peer| peer.name == node_name)
        .map(|peer| peer.url.clone())
        .or_else(|| {
            config
                .peers
                .iter()
                .find(|peer| peer.contains(node_name))
                .cloned()
        })
}

#[derive(Debug, Clone, Copy)]
enum RouteType {
    Local,
    SelfNode,
}

fn route_target(route_type: RouteType, target: String) -> ResolveResult {
    match route_type {
        RouteType::Local => ResolveResult::Local { target },
        RouteType::SelfNode => ResolveResult::SelfNode { target },
    }
}

fn resolve_self_target_alias_window(
    current_session: &str,
    writable: &[Session],
    route_type: RouteType,
) -> ResolveResult {
    let Some(session) = writable
        .iter()
        .find(|session| session.name.eq_ignore_ascii_case(current_session))
    else {
        let sessions = writable
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return error(
            "me_session_not_found",
            format!("'me' resolved current tmux session '{current_session}', but it is not in local sessions"),
            Some(if sessions.is_empty() {
                "sessions: (none)".to_owned()
            } else {
                format!("sessions: {sessions}")
            }),
        );
    };

    let stem = strip_numeric_fleet_prefix(&session.name);
    let exact_oracle_window = format!("{stem}-oracle");
    if let Some(window) = session
        .windows
        .iter()
        .filter(|window| self_target_oracle_candidate(window))
        .find(|window| window.name.eq_ignore_ascii_case(&exact_oracle_window))
    {
        return route_target(route_type, format!("{}:{}", session.name, window.index));
    }

    let oracle_windows = session
        .windows
        .iter()
        .filter(|window| self_target_oracle_candidate(window))
        .collect::<Vec<_>>();
    match oracle_windows.as_slice() {
        [window] => route_target(route_type, format!("{}:{}", session.name, window.index)),
        [] => error(
            "me_oracle_window_not_found",
            format!(
                "'me' resolved current tmux session '{}', but no *-oracle window was found",
                session.name
            ),
            Some(format!("windows: {}", session_window_list(session))),
        ),
        _ => error(
            "me_oracle_window_ambiguous",
            format!(
                "'me' resolved current tmux session '{}', but multiple *-oracle windows were found and none matched '{}'",
                session.name, exact_oracle_window
            ),
            Some(format!("windows: {}", session_window_list(session))),
        ),
    }
}

fn self_target_oracle_candidate(window: &Window) -> bool {
    match declared_window_kind(window) {
        Some(RepoKind::Oracle) => true,
        Some(RepoKind::Project) => false,
        None => window.name.to_lowercase().ends_with("-oracle"),
    }
}

fn resolve_session_alias_window_target(
    query: &str,
    writable: &[Session],
    route_type: RouteType,
) -> Option<ResolveResult> {
    if alias_query_is_oracle(query, writable) {
        return None;
    }

    let wanted = session_alias_names(query);
    if wanted.is_empty() {
        return None;
    }
    let wanted_lower: Vec<String> = wanted.iter().map(|name| name.to_lowercase()).collect();
    let mut matches: Vec<Session> = writable
        .iter()
        .filter(|session| {
            session_alias_names(&session.name)
                .iter()
                .any(|name| wanted_lower.contains(&name.to_lowercase()))
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return None;
    }

    if matches.len() > 1 {
        let normalized_query = query.trim().to_lowercase();
        let exact_unnumbered: Vec<Session> = matches
            .iter()
            .filter(|session| {
                strip_numeric_fleet_prefix(&session.name).to_lowercase() == normalized_query
            })
            .cloned()
            .collect();
        if exact_unnumbered.len() == 1 {
            matches = exact_unnumbered;
        }
    }

    if matches.len() > 1 {
        return Some(error(
            "session_alias_ambiguous",
            format!("'{query}' matches multiple local sessions; refusing to guess a window"),
            Some(format!(
                "candidates: {}",
                matches
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        ));
    }

    let session = &matches[0];
    if let Some(named_target) = find_named_fleet_window(session, query) {
        return Some(route_target(route_type, named_target));
    }

    if session.windows.len() == 1 {
        return Some(route_target(
            route_type,
            format!("{}:{}", session.name, session.windows[0].index),
        ));
    }

    let candidate_names = fleet_window_candidate_names(query);
    let candidates = session
        .windows
        .iter()
        .map(|window| format!("{}:{} ({})", session.name, window.index, window.name))
        .collect::<Vec<_>>()
        .join(", ");
    Some(error(
        "session_window_not_found",
        format!(
            "'{query}' matched local session '{}', but no window named {} was found; refusing to default to the first window",
            session.name,
            quoted_or(&candidate_names)
        ),
        Some(format!("candidates: {candidates}")),
    ))
}

fn find_named_fleet_window(session: &Session, query: &str) -> Option<String> {
    for name in fleet_window_candidate_names(query) {
        if let Some(window) = session
            .windows
            .iter()
            .find(|window| window.name.eq_ignore_ascii_case(&name))
        {
            return Some(format!("{}:{}", session.name, window.index));
        }
    }
    None
}

