
/// Resolve a user query with optional tmux caller context.
///
/// The exact token `me` is a self-target alias for the current tmux session's
/// oracle window. Callers outside tmux pass `None` and receive a clear error.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn resolve_target_with_current_session(
    query: &str,
    config: &MawConfig,
    sessions: &[Session],
    current_session: Option<&str>,
) -> ResolveResult {
    if query.is_empty() {
        return error(
            "empty_query",
            "no target specified",
            Some("usage: maw hey <agent> <message>"),
        );
    }

    if is_self_target_alias(query) {
        return resolve_self_target_alias(current_session, sessions);
    }

    let writable = writable_sessions(sessions);
    let self_node = config.node.as_deref().unwrap_or("local");

    if let Some(result) =
        resolve_explicit_local_session_window_target(query, &writable, RouteType::Local)
    {
        return result;
    }

    if !query.contains(':') {
        if let Some(result) =
            resolve_session_alias_window_target(query, &writable, RouteType::Local)
        {
            return result;
        }
    }

    if let Some(local_target) = find_window(&writable, query) {
        return ResolveResult::Local {
            target: local_target,
        };
    }

    if query.contains(':') && !query.contains('/') {
        let (node_name, agent_name) = query.split_once(':').unwrap_or(("", ""));
        if node_name.is_empty() || agent_name.is_empty() {
            return error(
                "empty_node_or_agent",
                format!("invalid format: '{query}'"),
                Some("use node:agent format (e.g. mba:homekeeper)"),
            );
        }

        if node_name == self_node || node_name == "local" {
            if let Some(result) =
                resolve_session_alias_window_target(agent_name, &writable, RouteType::SelfNode)
            {
                return result;
            }
            if let Some(self_target) = find_window(&writable, agent_name) {
                return ResolveResult::SelfNode {
                    target: self_target,
                };
            }
            return error(
                "self_not_running",
                format!("'{agent_name}' not found in local sessions on {self_node}"),
                Some(format!("maw wake {agent_name}")),
            );
        }

        if let Some(peer_url) = find_peer_url(node_name, config) {
            return ResolveResult::Peer {
                peer_url,
                target: agent_name.to_owned(),
                node: node_name.to_owned(),
            };
        }

        return error(
            "unknown_node",
            format!("node '{node_name}' not in namedPeers or peers"),
            Some("add to maw.config.json namedPeers"),
        );
    }

    let stripped_query = query.strip_suffix("-oracle").unwrap_or(query);
    let agent_node = config
        .agents
        .get(query)
        .or_else(|| config.agents.get(stripped_query));

    if let Some(agent_node) = agent_node {
        if agent_node == self_node {
            return error(
                "self_not_running",
                format!("'{query}' mapped to {self_node} (local) but not found in sessions"),
                Some(format!("maw wake {query}")),
            );
        }
        if let Some(peer_url) = find_peer_url(agent_node, config) {
            return ResolveResult::Peer {
                peer_url,
                target: query.to_owned(),
                node: agent_node.clone(),
            };
        }
        return error(
            "no_peer_url",
            format!("'{query}' mapped to node '{agent_node}' but no URL found"),
            Some(format!("add {agent_node} to maw.config.json namedPeers")),
        );
    }

    error(
        "not_found",
        format!("'{query}' not in local sessions or agents map"),
        Some("check: maw ls"),
    )
}

#[must_use]
pub fn is_self_target_alias(query: &str) -> bool {
    query.trim().eq_ignore_ascii_case("me")
}

#[must_use]
pub fn resolve_self_target_alias(
    current_session: Option<&str>,
    sessions: &[Session],
) -> ResolveResult {
    let Some(current_session) = current_session.map(str::trim).filter(|value| !value.is_empty())
    else {
        return error(
            "me_needs_tmux",
            "'me' needs a tmux context",
            Some("run inside tmux so maw can resolve the current session"),
        );
    };
    let writable = writable_sessions(sessions);
    resolve_self_target_alias_window(current_session, &writable, RouteType::Local)
}

fn writable_sessions(sessions: &[Session]) -> Vec<Session> {
    sessions
        .iter()
        .filter(|session| {
            !session.name.ends_with("-view")
                && session
                    .source
                    .as_deref()
                    .is_none_or(|source| source == "local")
        })
        .cloned()
        .collect()
}
