const DISPATCH_151: &[DispatcherEntry] = &[
    DispatcherEntry { command: "ls", handler: Handler::Sync(run_ls_plan) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LsPeerRow151 {
    alias: String,
    node: Option<String>,
    url: String,
}

#[derive(Debug, Clone)]
struct LsFetchedNode151 {
    alias: String,
    node: Option<String>,
    url: Option<String>,
    local: bool,
    sessions: Vec<serde_json::Value>,
    error: Option<String>,
}

fn ls_validate_value(value: &str, label: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("maw ls: {label} requires a value"));
    }
    if trimmed.starts_with('-') {
        return Err(format!("maw ls: {label} must not start with '-'"));
    }
    if trimmed.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err(format!("maw ls: {label} contains control characters"));
    }
    Ok(())
}

fn ls_render_federation(options: &LsPlanOptions, panes: &[LsPanePlan]) -> CliOutput {
    if let Some(peer) = &options.peer {
        return ls_render_peer_drilldown(peer, options);
    }

    let peers = ls_load_peers().unwrap_or_default();
    let node_filter = options.node.as_deref().or(options.filter.as_deref());
    let mut nodes: Vec<LsFetchedNode151> = Vec::new();
    if ls_node_matches("local", Some("local"), None, node_filter) || !panes.is_empty() {
        nodes.push(ls_local_node(panes));
    }
    for peer in peers
        .iter()
        .filter(|peer| ls_node_matches(&peer.alias, peer.node.as_deref(), Some(&peer.url), node_filter))
    {
        nodes.push(ls_fetch_peer_node(peer));
    }

    if node_filter.is_some() && nodes.is_empty() {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!(
                "no local or peer node matches '{}'\n",
                node_filter.unwrap_or_default()
            ),
        };
    }

    if options.json {
        let total_sessions = nodes.iter().map(|node| node.sessions.len()).sum::<usize>();
        let reachable_nodes = nodes.iter().filter(|node| node.error.is_none()).count();
        return CliOutput {
            code: 0,
            stdout: format!(
                "{{\"nodes\":[{}],\"totalSessions\":{},\"reachableNodes\":{},\"totalNodes\":{},\"timeoutMs\":2000}}\n",
                nodes.iter().map(ls_node_json).collect::<Vec<_>>().join(","),
                total_sessions,
                reachable_nodes,
                nodes.len()
            ),
            stderr: String::new(),
        };
    }

    let mut out = String::new();
    let reachable = nodes.iter().filter(|node| node.error.is_none()).count();
    let total_sessions = nodes.iter().map(|node| node.sessions.len()).sum::<usize>();
    let _ = writeln!(
        out,
        "{}",
        ls_color(
            "36",
            &format!(
                "📡 fleet view · {reachable}/{} node{} reachable · {total_sessions} session{} total",
                nodes.len(),
                if nodes.len() == 1 { "" } else { "s" },
                if total_sessions == 1 { "" } else { "s" }
            )
        )
    );
    out.push('\n');
    for node in &nodes {
        let label = &node.alias;
        let location = if node.local { "local".to_owned() } else { node.url.clone().unwrap_or_else(|| "peer".to_owned()) };
        if let Some(error) = &node.error {
            let _ = writeln!(
                out,
                "  {} {} {}",
                ls_color("31", "✗"),
                label,
                ls_color("90", &format!("({location}) — {error}"))
            );
            continue;
        }
        let _ = writeln!(
            out,
            "  {} {} {}",
            ls_color("34", "●"),
            ls_color("36", label),
            ls_color(
                "90",
                &format!(
                    "({location}) · {} session{}",
                    node.sessions.len(),
                    if node.sessions.len() == 1 { "" } else { "s" }
                )
            )
        );
        for session in &node.sessions {
            if let Some(name) = session.get("name").and_then(serde_json::Value::as_str) {
                let _ = writeln!(out, "     {} {name}", ls_color("90", "●"));
            }
        }
    }
    let _ = writeln!(out, "\n{}", ls_color("90", "  → maw ls   list only local sessions (fast default)"));
    CliOutput { code: 0, stdout: out, stderr: String::new() }
}

fn ls_render_peer_drilldown(peer: &str, options: &LsPlanOptions) -> CliOutput {
    let rows = ls_load_peers().unwrap_or_default();
    let Some(row) = rows.iter().find(|row| row.alias == peer || row.node.as_deref() == Some(peer)) else {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("unknown peer alias: {peer} (see: maw peers list)\n"),
        };
    };
    let node = ls_fetch_peer_node(row);
    if let Some(error) = &node.error {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{error}\n"),
        };
    }
    if options.json {
        return CliOutput {
            code: 0,
            stdout: format!(
                "{{\"peer\":{},\"url\":{},\"sessions\":[{}]}}\n",
                json_string(&row.alias),
                json_string(&row.url),
                node.sessions
                    .iter()
                    .map(serde_json::Value::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            stderr: String::new(),
        };
    }
    let mut lines = vec![
        ls_color(
            "36",
            &format!(
                "📡 {} @ {} · {} session{}",
                row.alias,
                row.url,
                node.sessions.len(),
                if node.sessions.len() == 1 { "" } else { "s" }
            ),
        ),
        String::new(),
    ];
    if node.sessions.is_empty() {
        lines.push(ls_color("90", "  (no sessions)"));
    } else {
        for session in &node.sessions {
            let name = session
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            lines.push(format!("  {} {}", ls_color("34", "●"), ls_color("36", name)));
            if let Some(windows) = session.get("windows").and_then(serde_json::Value::as_array) {
                for window in windows {
                    let active = window
                        .get("active")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let dot = if active { ls_color("32", "●") } else { ls_color("90", "●") };
                    let name = window
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown");
                    let prefix = window
                        .get("index")
                        .and_then(serde_json::Value::as_u64)
                        .map_or_else(String::new, |idx| format!("{idx}: "));
                    lines.push(format!("     {dot} {prefix}{name}"));
                }
            }
        }
    }
    lines.push(String::new());
    lines.push(ls_color(
        "90",
        &format!("  → maw hey {}:<session>:<window>   send a message", row.alias),
    ));
    CliOutput { code: 0, stdout: lines.join("\n") + "\n", stderr: String::new() }
}

fn ls_local_node(panes: &[LsPanePlan]) -> LsFetchedNode151 {
    let mut sessions = Vec::new();
    for (session, panes) in group_ls_sessions(panes) {
        let windows = panes
            .iter()
            .enumerate()
            .map(|(idx, pane)| {
                serde_json::json!({
                    "name": pane.title,
                    "index": idx,
                    "active": pane.status == "active",
                })
            })
            .collect::<Vec<_>>();
        sessions.push(serde_json::json!({
            "name": session,
            "windows": windows,
        }));
    }
    LsFetchedNode151 {
        alias: "local".to_owned(),
        node: Some("local".to_owned()),
        url: None,
        local: true,
        sessions,
        error: None,
    }
}

fn ls_fetch_peer_node(peer: &LsPeerRow151) -> LsFetchedNode151 {
    match ls_fetch_peer_sessions(&peer.url) {
        Ok(sessions) => LsFetchedNode151 {
            alias: peer.alias.clone(),
            node: peer.node.clone(),
            url: Some(peer.url.clone()),
            local: false,
            sessions,
            error: None,
        },
        Err(error) => LsFetchedNode151 {
            alias: peer.alias.clone(),
            node: peer.node.clone(),
            url: Some(peer.url.clone()),
            local: false,
            sessions: Vec::new(),
            error: Some(error),
        },
    }
}

fn ls_node_json(node: &LsFetchedNode151) -> String {
    let mut fields = vec![format!("\"alias\":{}", json_string(&node.alias))];
    if let Some(node_name) = &node.node {
        fields.push(format!("\"node\":{}", json_string(node_name)));
    }
    if let Some(url) = &node.url {
        fields.push(format!("\"url\":{}", json_string(url)));
    }
    if node.local {
        fields.push("\"local\":true".to_owned());
    }
    fields.push(format!(
        "\"sessions\":[{}]",
        node.sessions
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join(",")
    ));
    if let Some(error) = &node.error {
        fields.push(format!("\"error\":{}", json_string(error)));
    }
    format!("{{{}}}", fields.join(","))
}

fn ls_fetch_peer_sessions(peer_url: &str) -> Result<Vec<serde_json::Value>, String> {
    ls_validate_peer_url(peer_url)?;
    let url = format!("{}/api/ls", peer_url.trim_end_matches('/'));
    let output = std::process::Command::new("curl")
        .args(["-fsS", "--max-time", "2", "--"])
        .arg(&url)
        .output()
        .map_err(|error| format!("peer ls failed: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            "peer ls failed".to_owned()
        } else {
            detail
        });
    }
    let raw = String::from_utf8(output.stdout)
        .map_err(|error| format!("peer ls response was not utf8: {error}"))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("peer ls response was not json: {error}"))?;
    Ok(ls_sessions_from_payload(&value))
}

fn ls_sessions_from_payload(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(output) = value.get("output").and_then(serde_json::Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
            return ls_sessions_from_payload(&parsed);
        }
        return Vec::new();
    }
    if let Some(sessions) = value.as_array() {
        return sessions.clone();
    }
    if let Some(sessions) = value.get("sessions").and_then(serde_json::Value::as_array) {
        return sessions.clone();
    }
    Vec::new()
}

fn ls_validate_peer_url(peer_url: &str) -> Result<(), String> {
    let value = peer_url.trim();
    if value.is_empty()
        || value.starts_with('-')
        || value.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err("peer url is unsafe".to_owned());
    }
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err("peer url must use http/https".to_owned());
    }
    Ok(())
}

fn ls_node_matches(alias: &str, node: Option<&str>, url: Option<&str>, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let q = filter.to_lowercase();
    [Some(alias), node, url]
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&q))
}

fn ls_load_peers() -> Result<Vec<LsPeerRow151>, String> {
    let path = std::env::var_os("PEERS_FILE").map_or_else(
        || maw_state_path(&current_xdg_env(), &["peers.json"]),
        std::path::PathBuf::from,
    );
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| format!("ls: peers.json parse failed: {error}"))?;
    let Some(peers) = value.get("peers").and_then(serde_json::Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = peers
        .iter()
        .filter_map(|(alias, peer)| {
            let url = peer.get("url")?.as_str()?.to_owned();
            let node = peer.get("node").and_then(serde_json::Value::as_str).map(str::to_owned);
            Some(LsPeerRow151 { alias: alias.clone(), node, url })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.alias.cmp(&right.alias));
    Ok(rows)
}

fn ls_render_verify_fix(options: &LsPlanOptions) -> (String, String) {
    if !options.verify && !options.fix {
        return (String::new(), String::new());
    }
    match ls_validate_prune_root(std::env::current_dir().as_deref().unwrap_or_else(|_| std::path::Path::new("."))) {
        Ok(root) => ls_render_prune_for_root(&root, options.fix),
        Err(error) => (format!("\n  {} {error}\n", ls_color("33", "⚠ maw ls:")), String::new()),
    }
}

fn ls_validate_prune_root(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let raw = path.display().to_string();
    if raw.is_empty() || raw.starts_with('-') || raw.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("unsafe worktree root rejected before prune".to_owned());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("worktree root not reachable: {error}"))?;
    let canon = canonical.display().to_string();
    if canon.starts_with('-') || canon.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("unsafe canonical worktree root rejected before prune".to_owned());
    }
    Ok(canonical)
}

fn ls_render_prune_for_root(root: &std::path::Path, fix: bool) -> (String, String) {
    let mut verify = String::new();
    let _ = writeln!(
        verify,
        "\n  {} worktree root validated: {}",
        ls_color("33", "⚠ verify:"),
        root.display()
    );
    if !fix {
        let _ = writeln!(verify, "{}", ls_color("90", "  → maw ls --fix       to prune orphans"));
        return (verify, String::new());
    }

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "prune"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let mut fix_out = String::new();
            let _ = writeln!(fix_out, "\n{}", ls_color("36", "→ pruning orphaned worktrees…"));
            let _ = writeln!(fix_out, "{}", ls_color("90", "  pruned via git worktree prune"));
            (verify, fix_out)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            (verify, format!("\n  {} {}\n", ls_color("31", "✗"), if stderr.is_empty() { "git worktree prune failed".to_owned() } else { stderr }))
        }
        Err(error) => (verify, format!("\n  {} git worktree prune failed: {error}\n", ls_color("31", "✗"))),
    }
}
