
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
