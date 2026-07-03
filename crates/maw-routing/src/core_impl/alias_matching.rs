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

fn find_window(sessions: &[Session], query: &str) -> Option<String> {
    let q = query.to_lowercase();

    if query.contains(':') {
        let (sess_part, raw_win_part) = q.split_once(':').unwrap_or(("", ""));
        let (win_part, pane_suffix) = split_pane_suffix(raw_win_part);
        if let Some(session) = match_session(sessions, sess_part, true) {
            if win_part.is_empty() {
                if let Some(window) = session.windows.first() {
                    return Some(format!("{}:{}", session.name, window.index));
                }
                return Some(format!("{}:", session.name));
            }

            let numeric_window = numeric_window_or_pane(raw_win_part);
            if numeric_window {
                if let Ok(window_index) = win_part.parse::<u32>() {
                    if let Some(window) = session
                        .windows
                        .iter()
                        .find(|window| window.index == window_index)
                    {
                        return Some(format!("{}:{}{pane_suffix}", session.name, window.index));
                    }
                }
            }

            if let Some(window) = session
                .windows
                .iter()
                .find(|window| window.name.eq_ignore_ascii_case(win_part))
            {
                return Some(format!("{}:{}{pane_suffix}", session.name, window.index));
            }

            if numeric_window {
                return Some(format!("{}:{}{}", session.name, win_part, pane_suffix));
            }

            if let Some(window) = session
                .windows
                .iter()
                .find(|window| window.name.to_lowercase().contains(win_part))
            {
                return Some(format!("{}:{}{pane_suffix}", session.name, window.index));
            }
        }
    }

    let exact_sessions: Vec<String> = sessions
        .iter()
        .filter_map(|session| {
            let window = session.windows.first()?;
            let name = session.name.to_lowercase();
            (name == q || strip_numeric_fleet_prefix(&name) == q)
                .then(|| format!("{}:{}", session.name, window.index))
        })
        .collect();
    if exact_sessions.len() == 1 {
        return exact_sessions.first().cloned();
    }
    if exact_sessions.len() > 1 {
        return None;
    }

    let exact_windows = unique_strings(sessions.iter().flat_map(|session| {
        let q = q.clone();
        session
            .windows
            .iter()
            .filter(move |window| window.name.eq_ignore_ascii_case(&q))
            .map(|window| format!("{}:{}", session.name, window.index))
    }));
    if exact_windows.len() == 1 {
        return exact_windows.first().cloned();
    }
    if exact_windows.len() > 1 {
        return None;
    }

    let substring_matches = unique_strings(sessions.iter().flat_map(|session| {
        let mut matches = Vec::new();
        for window in &session.windows {
            if window.name.to_lowercase().contains(&q) {
                matches.push(format!("{}:{}", session.name, window.index));
            }
        }
        if session.name.to_lowercase().contains(&q) {
            if let Some(window) = session.windows.first() {
                matches.push(format!("{}:{}", session.name, window.index));
            }
        }
        matches
    }));
    if substring_matches.len() == 1 {
        return substring_matches.first().cloned();
    }
    if substring_matches.len() > 1 {
        return None;
    }
    None
}

fn match_session<'a>(sessions: &'a [Session], part: &str, strict: bool) -> Option<&'a Session> {
    let p = part.to_lowercase();
    if p.is_empty() {
        return None;
    }
    sessions
        .iter()
        .find(|session| session.name.to_lowercase() == p)
        .or_else(|| {
            sessions
                .iter()
                .find(|session| strip_numeric_fleet_prefix(&session.name.to_lowercase()) == p)
        })
        .or_else(|| {
            (!strict)
                .then(|| {
                    sessions
                        .iter()
                        .find(|session| session.name.to_lowercase().contains(&p))
                })
                .flatten()
        })
}

fn split_pane_suffix(raw_win_part: &str) -> (&str, String) {
    if let Some((win, pane)) = raw_win_part.rsplit_once('.') {
        if !win.is_empty() && !pane.is_empty() && pane.bytes().all(|byte| byte.is_ascii_digit()) {
            return (win, format!(".{pane}"));
        }
    }
    (raw_win_part, String::new())
}

fn numeric_window_or_pane(value: &str) -> bool {
    let Some((window, pane)) = value.split_once('.') else {
        return !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    };
    !window.is_empty()
        && !pane.is_empty()
        && window.bytes().all(|byte| byte.is_ascii_digit())
        && pane.bytes().all(|byte| byte.is_ascii_digit())
}

fn strip_numeric_fleet_prefix(name: &str) -> &str {
    let Some((prefix, rest)) = name.split_once('-') else {
        return name;
    };
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        rest
    } else {
        name
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn unique_strings<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut out = Vec::new();
    for value in values {
        let value = value.into();
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn quoted_or(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn session_window_list(session: &Session) -> String {
    let windows = session
        .windows
        .iter()
        .map(|window| format!("{}:{} ({})", session.name, window.index, window.name))
        .collect::<Vec<_>>();
    if windows.is_empty() {
        "(none)".to_owned()
    } else {
        windows.join(", ")
    }
}

fn error(
    reason: impl Into<String>,
    detail: impl Into<String>,
    hint: Option<impl Into<String>>,
) -> ResolveResult {
    ResolveResult::Error {
        reason: reason.into(),
        detail: detail.into(),
        hint: hint.map(Into::into),
    }
}

#[cfg(test)]
mod coverage_gap_tests {
    use super::*;

    fn window(index: u32, name: &str) -> Window {
        Window {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: None,
        }
    }

    fn window_with_kind(index: u32, name: &str, kind: RepoKind) -> Window {
        Window {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: Some(kind),
        }
    }

    fn session(name: &str, windows: Vec<Window>) -> Session {
        Session {
            name: name.to_owned(),
            windows,
            source: None,
        }
    }

    fn config_with_node(node: &str) -> MawConfig {
        MawConfig {
            node: Some(node.to_owned()),
            ..MawConfig::default()
        }
    }

    #[test]
    fn sync_apply_skips_conflicts_and_stale_without_force_or_prune() {
        let diff = SyncDiff {
            add: Vec::new(),
            conflict: vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            stale: vec![StaleRoute {
                oracle: "old".to_owned(),
                peer_node: "white".to_owned(),
            }],
            unreachable: Vec::new(),
        };
        let current = HashMap::from([
            ("mawjs".to_owned(), "mba".to_owned()),
            ("old".to_owned(), "white".to_owned()),
        ]);

        let result = apply_sync_diff(&current, &diff, SyncApplyOptions::default());

        assert_eq!(result.agents, current);
        assert!(result.applied.is_empty());
    }

    #[test]
    fn invalid_node_agent_query_reports_empty_side() {
        assert_eq!(
            resolve_target(":ghost", &config_with_node("white"), &[]),
            ResolveResult::Error {
                reason: "empty_node_or_agent".to_owned(),
                detail: "invalid format: ':ghost'".to_owned(),
                hint: Some("use node:agent format (e.g. mba:homekeeper)".to_owned()),
            }
        );
    }

    #[test]
    fn self_node_alias_returns_self_node_target() {
        let sessions = vec![session("pulse", vec![window(3, "pulse")])];

        assert_eq!(
            resolve_target("white:pulse", &config_with_node("white"), &sessions),
            ResolveResult::SelfNode {
                target: "pulse:3".to_owned(),
            }
        );
    }

    #[test]
    fn self_target_alias_resolves_numbered_current_session_oracle_window() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window(1, "maw-rs-oracle"),
                window(2, "maw-rs-codex-6"),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "ME",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_resolves_unnumbered_current_session_oracle_window() {
        let sessions = vec![session(
            "mawjs",
            vec![window(0, "shell"), window(4, "mawjs-oracle")],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("mawjs")
            ),
            ResolveResult::Local {
                target: "mawjs:4".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_skips_declared_project_oracle_suffix_window() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(1, "maw-rs-oracle", RepoKind::Project),
                window_with_kind(4, "maw-rs-codex-6", RepoKind::Oracle),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Local {
                target: "188-maw-rs:4".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_reports_no_oracle_when_suffix_window_is_declared_project() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(1, "maw-rs-oracle", RepoKind::Project),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Error {
                reason: "me_oracle_window_not_found".to_owned(),
                detail: "'me' resolved current tmux session '188-maw-rs', but no *-oracle window was found".to_owned(),
                hint: Some(
                    "windows: 188-maw-rs:0 (work), 188-maw-rs:1 (maw-rs-oracle)"
                        .to_owned()
                ),
            }
        );
    }

    #[test]
    fn self_target_alias_matches_declared_oracle_window_without_suffix() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(3, "maw-rs-codex-6", RepoKind::Oracle),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Local {
                target: "188-maw-rs:3".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_reports_no_oracle_window_with_session_windows() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![window(0, "work"), window(2, "maw-rs-codex-6")],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Error {
                reason: "me_oracle_window_not_found".to_owned(),
                detail: "'me' resolved current tmux session '188-maw-rs', but no *-oracle window was found".to_owned(),
                hint: Some(
                    "windows: 188-maw-rs:0 (work), 188-maw-rs:2 (maw-rs-codex-6)"
                        .to_owned()
                ),
            }
        );
    }

    #[test]
    fn self_target_alias_reports_outside_tmux_context() {
        assert_eq!(
            resolve_target_with_current_session("me", &MawConfig::default(), &[], None),
            ResolveResult::Error {
                reason: "me_needs_tmux".to_owned(),
                detail: "'me' needs a tmux context".to_owned(),
                hint: Some(
                    "run inside tmux so maw can resolve the current session".to_owned()
                ),
            }
        );
    }

    #[test]
    fn literal_me_window_is_reachable_with_full_session_form() {
        let sessions = vec![session(
            "scratch",
            vec![window(0, "shell"), window(3, "me")],
        )];

        assert_eq!(
            resolve_target("scratch:me", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "scratch:3".to_owned()
            }
        );
    }

    #[test]
    fn exact_unnumbered_session_breaks_alias_tie() {
        let sessions = vec![
            session("47-mawjs", vec![window(0, "mawjs")]),
            session("mawjs-oracle", vec![window(2, "mawjs")]),
        ];

        assert_eq!(
            resolve_target("mawjs", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "47-mawjs:0".to_owned(),
            }
        );
    }

    #[test]
    fn blank_alias_and_numeric_prefixed_candidates_are_defensive() {
        assert!(resolve_session_alias_window_target("   ", &[], RouteType::Local).is_none());
        assert_eq!(fleet_window_candidate_names(""), Vec::<String>::new());
        assert_eq!(
            fleet_window_candidate_names("mawjs"),
            vec!["mawjs", "mawjs-oracle"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fleet_window_candidate_names("47-mawjs-oracle"),
            vec!["47-mawjs-oracle", "47-mawjs", "mawjs-oracle", "mawjs"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn find_window_covers_colon_fallthrough_edges() {
        let sessions = vec![session("dev", Vec::new())];
        assert_eq!(find_window(&sessions, "dev:"), Some("dev:".to_owned()));
        assert_eq!(find_window(&sessions, "dev:nope"), None);
    }

    #[test]
    fn find_window_supports_colon_first_window_and_numeric_fallbacks() {
        let sessions = vec![session("dev", vec![window(5, "main")])];

        assert_eq!(find_window(&sessions, "dev:"), Some("dev:5".to_owned()));
        assert_eq!(find_window(&sessions, "dev:4"), Some("dev:4".to_owned()));
        assert_eq!(
            find_window(&sessions, "dev:4.2"),
            Some("dev:4.2".to_owned())
        );
    }

    #[test]
    fn find_window_prefers_colon_index_over_numeric_window_name_suffix() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(1, "maw-rs-oracle"),
                window(2, "maw-rs-codex-1"),
            ],
        )];

        assert_eq!(
            resolve_target("188-maw-rs:1", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
        assert_eq!(
            resolve_target("maw-rs:1", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn find_window_prefers_exact_full_window_name_over_substring() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(2, "maw-rs-codex-10"),
                window(1, "maw-rs-codex-1"),
            ],
        )];

        assert_eq!(
            resolve_target(
                "188-maw-rs:maw-rs-codex-1",
                &MawConfig::default(),
                &sessions
            ),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn find_window_refuses_ambiguous_exact_session_or_window_matches() {
        let duplicate_sessions = vec![
            session("47-mawjs", vec![window(0, "left")]),
            session("99-mawjs", vec![window(1, "right")]),
        ];
        assert_eq!(find_window(&duplicate_sessions, "mawjs"), None);

        let duplicate_windows = vec![
            session("alpha", vec![window(0, "oracle")]),
            session("bravo", vec![window(0, "oracle")]),
        ];
        assert_eq!(find_window(&duplicate_windows, "oracle"), None);
    }

    #[test]
    fn find_window_uses_unique_substring_window_or_session_match() {
        let window_match = vec![session("alpha", vec![window(9, "mawjs-codex")])];
        assert_eq!(
            find_window(&window_match, "codex"),
            Some("alpha:9".to_owned())
        );

        let session_match = vec![session("mawjs-session", vec![window(4, "main")])];
        assert_eq!(
            find_window(&session_match, "session"),
            Some("mawjs-session:4".to_owned())
        );
        assert_eq!(
            find_window(&[session("empty-session", Vec::new())], "empty"),
            None
        );

        let ambiguous = vec![
            session("alpha", vec![window(0, "mawjs-left")]),
            session("bravo-mawjs", vec![window(1, "main")]),
        ];
        assert_eq!(find_window(&ambiguous, "mawjs"), None);
    }

    #[test]
    fn find_window_direct_paths_cover_unique_exact_and_strict_fallbacks() {
        let sessions = vec![session("alpha", vec![window(7, "main")])];
        assert_eq!(find_window(&sessions, "alpha"), Some("alpha:7".to_owned()));
        assert_eq!(
            find_window(&sessions, "alpha:9"),
            Some("alpha:9".to_owned())
        );
        assert_eq!(
            match_session(&sessions, "alp", false).map(|session| session.name.as_str()),
            Some("alpha")
        );
    }

    #[test]
    fn helper_functions_cover_non_matching_edges() {
        assert_eq!(match_session(&[], "", true), None);
        assert_eq!(split_pane_suffix("main."), ("main.", String::new()));
        assert_eq!(split_pane_suffix("main.x"), ("main.x", String::new()));
        assert!(!numeric_window_or_pane(""));
        assert!(!numeric_window_or_pane("1."));
        assert!(!numeric_window_or_pane("x.1"));
        assert_eq!(strip_numeric_fleet_prefix("mawjs"), "mawjs");
        assert_eq!(strip_numeric_fleet_prefix("dev-mawjs"), "dev-mawjs");
    }

    #[test]
    fn peer_url_lookup_prefers_named_peer_then_peer_substring() {
        let config = MawConfig {
            named_peers: vec![NamedPeer {
                name: "white".to_owned(),
                url: "http://white".to_owned(),
            }],
            peers: vec!["http://mba:3456".to_owned()],
            ..MawConfig::default()
        };

        assert_eq!(
            find_peer_url("white", &config),
            Some("http://white".to_owned())
        );
        assert_eq!(
            find_peer_url("mba", &config),
            Some("http://mba:3456".to_owned())
        );
        assert_eq!(find_peer_url("ghost", &config), None);
    }

    #[test]
    fn oracle_suffixed_alias_is_left_for_agent_routing() {
        let sessions = vec![session("mawjs", vec![window(0, "mawjs")])];
        assert!(
            resolve_session_alias_window_target("mawjs-oracle", &sessions, RouteType::Local)
                .is_none()
        );
    }

    #[test]
    fn declared_project_window_overrides_oracle_suffix_alias_guard() {
        let sessions = vec![session(
            "bar-oracle",
            vec![window_with_kind(0, "bar-oracle", RepoKind::Project)],
        )];

        assert_eq!(
            resolve_session_alias_window_target("bar-oracle", &sessions, RouteType::Local),
            Some(ResolveResult::Local {
                target: "bar-oracle:0".to_owned(),
            })
        );
    }

    #[test]
    fn declared_oracle_window_without_suffix_keeps_oracle_suffix_guard() {
        let sessions = vec![session(
            "foo",
            vec![window_with_kind(0, "foo", RepoKind::Oracle)],
        )];

        assert!(
            resolve_session_alias_window_target("foo-oracle", &sessions, RouteType::Local)
                .is_none()
        );
    }

    #[test]
    fn find_window_preserves_pane_suffix_for_named_window() {
        let sessions = vec![session("dev", vec![window(5, "main")])];

        assert_eq!(
            find_window(&sessions, "dev:main.2"),
            Some("dev:5.2".to_owned())
        );
    }

}
