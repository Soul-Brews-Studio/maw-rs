// What this fleet currently consists of.
//
// Rendering only: sessions, windows, squads and their members, as text or JSON.
// Kept apart from doctor because counting what exists and judging whether it is
// healthy are different jobs that happen to read the same state.

fn fleet_render_census(state: &FleetState, options: &FleetOptions) -> Result<String, String> {
    let sessions = fleet_census_sessions(state, &options.squads);
    let groups = fleet_census_groups(state, &options.squads);
    if options.json { return fleet_json_census(state, &sessions, &groups); }
    let windows = fleet_window_count(&sessions);
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[36mfleet\x1b[0m node {}", state.config.node);
    let _ = writeln!(out, "  sessions: {} ({} windows, {} disabled)", sessions.len(), windows, state.disabled_count);
    let _ = writeln!(out, "  peers: {}", state.config.peers.len());
    let _ = writeln!(out, "  agents: {}", state.config.agents.len());
    let _ = writeln!(out, "  session list:");
    for session in &sessions {
        let _ = writeln!(out, "  - {} ({} windows)", session.name, session.windows.len());
    }
    let _ = writeln!(out, "  squads: {}", groups.len());
    for group in &groups {
        let _ = writeln!(
            out,
            "  - {} ({} members, {} sessions)",
            group.name,
            group.members.len(),
            group.sessions.len()
        );
        for member in &group.members {
            if let Some(session) = &member.session {
                let _ = writeln!(out, "      {} -> {}", member.handle, session);
            } else {
                let _ = writeln!(out, "      {} -> none", member.handle);
            }
        }
    }
    Ok(out)
}

fn fleet_json_census(state: &FleetState, sessions: &[FleetSessionSummary], groups: &[FleetGroupSummary]) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "configDir": state.config_dir,
        "sessions": sessions.iter().map(fleet_json_session).collect::<Vec<_>>(),
        "sessionCount": sessions.len(),
        "windowCount": fleet_window_count(sessions),
        "disabledCount": state.disabled_count,
        "peerCount": state.config.peers.len(),
        "agentCount": state.config.agents.len(),
        "squads": groups.iter().map(fleet_json_group).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_census_sessions(state: &FleetState, groups: &[String]) -> Vec<FleetSessionSummary> {
    let mut sessions = fleet_sweep_targets(state);
    if groups.is_empty() {
        return sessions;
    }
    let mut wanted = BTreeSet::new();
    let group_members = fleet_census_groups(state, groups);
    for group in group_members {
        for name in group.sessions {
            wanted.insert(name);
        }
    }
    sessions.retain(|session| wanted.contains(&session.name));
    sessions
}

fn fleet_census_groups(state: &FleetState, groups: &[String]) -> Vec<FleetGroupSummary> {
    let candidates = fleet_sweep_targets(state);
    let filtered = if groups.is_empty() {
        BTreeSet::<String>::new()
    } else {
        groups.iter().map(std::borrow::ToOwned::to_owned).collect()
    };
    let mut output = Vec::new();
    for entry in &state.fleet_entries {
        let Some(squad_name) = fleet_roster_squad_name(entry) else { continue; };
        if !groups.is_empty() && !filtered.iter().any(|group| fleet_roster_entry_matches(entry, group)) {
            continue;
        }
        let mut member_summaries = Vec::new();
        let mut sessions = Vec::new();
        for member in entry.session.members.clone().unwrap_or_default() {
            let session = fleet_member_session(&member.handle, &candidates).map(|session| session.name.clone());
            if let Some(name) = &session {
                sessions.push(name.to_owned());
            }
            member_summaries.push(FleetGroupMemberSummary { handle: member.handle, session });
        }
        sessions.sort();
        sessions.dedup();
        output.push(FleetGroupSummary {
            name: squad_name,
            path: entry.path.clone(),
            members: member_summaries,
            sessions,
        });
    }
    output
}

fn fleet_json_group(group: &FleetGroupSummary) -> serde_json::Value {
    serde_json::json!({
        "name": group.name,
        "path": group.path,
        "memberCount": group.members.len(),
        "sessionCount": group.sessions.len(),
        "sessions": group.sessions,
        "members": group.members.iter().map(fleet_json_group_member).collect::<Vec<_>>(),
    })
}

fn fleet_json_group_member(member: &FleetGroupMemberSummary) -> serde_json::Value {
    serde_json::json!({
        "handle": member.handle,
        "session": member.session,
    })
}

fn fleet_json_session(session: &FleetSessionSummary) -> serde_json::Value {
    serde_json::json!({
        "name": session.name,
        "windows": session.windows.iter().map(fleet_json_window).collect::<Vec<_>>(),
    })
}

fn fleet_json_window(window: &FleetWindowSummary) -> serde_json::Value {
    let mut value = serde_json::json!({ "name": window.name, "repo": window.repo });
    if let Some(kind) = window.kind {
        value["kind"] = serde_json::json!(native_repo_kind_label(kind));
    }
    value
}

fn fleet_window_count(sessions: &[FleetSessionSummary]) -> usize {
    sessions.iter().map(|session| session.windows.len()).sum()
}
