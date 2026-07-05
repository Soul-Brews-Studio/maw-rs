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
