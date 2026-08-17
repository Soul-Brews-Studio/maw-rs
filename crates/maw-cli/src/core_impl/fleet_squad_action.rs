// Doing one thing to every member of a squad.
//
// wake, sleep and gather all walk the same roster and differ mainly in what they
// run per member and what they print, so they share the sweep, the per-member
// session/window resolution and the post-wake hooks rather than each
// reimplementing them slightly differently.

fn fleet_run_wake(state: &FleetState, options: &FleetOptions) -> Result<(i32, String), String> {
    if let Some(group) = options.target.as_deref() {
        return fleet_run_group_action(state, options, "wake", group);
    }
    let sessions = fleet_sweep_targets(state);
    if options.json { return Ok((0, fleet_json_action(state, "wake", &sessions, options)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🌅 Fleet wake plan node: {}", state.config.node);
    let _ = writeln!(out, "  sessions: {} · disabled skipped: {}", sessions.len(), state.disabled_count);
    if options.kill { let _ = writeln!(out, "  preflight: sleep existing sessions first"); }
    if options.resume { let _ = writeln!(out, "  resume: yes"); }
    fleet_write_session_plan(&mut out, &sessions);
    Ok((0, out))
}

// Squadron roster files (#291, `members` present) describe squads, not sessions — never sweep targets.
fn fleet_sweep_targets(state: &FleetState) -> Vec<FleetSessionSummary> {
    let rosters = state
        .fleet_entries
        .iter()
        .filter(|entry| entry.session.members.is_some())
        .map(|entry| entry.session.name.as_str())
        .collect::<BTreeSet<_>>();
    state.sessions.iter().filter(|session| !rosters.contains(session.name.as_str())).cloned().collect()
}

fn fleet_run_group_action(
    state: &FleetState,
    options: &FleetOptions,
    action: &str,
    group: &str,
) -> Result<(i32, String), String> {
    if options.all {
        return Err(format!("fleet {action}: pass a squad or --all, not both"));
    }
    let entry = state
        .fleet_entries
        .iter()
        .find(|entry| fleet_roster_entry_matches(entry, group))
        .ok_or_else(|| format!("fleet {action}: no squad named {group} — try: maw fleet create {group}"))?;
    let members = entry.session.members.as_deref().unwrap_or_default();
    if members.is_empty() {
        return Err(format!("fleet {action}: squad {group} has no members"));
    }
    let candidates = fleet_sweep_targets(state);
    let mut resolved: Vec<(&str, &FleetSessionSummary)> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for member in members {
        match fleet_member_session(&member.handle, &candidates) {
            Some(session) => resolved.push((member.handle.as_str(), session)),
            None => skipped.push(member.handle.as_str()),
        }
    }
    if action == "wake" && !options.dry_run {
        fleet_run_group_post_wake_hooks(&resolved);
    }
    if options.json { return fleet_json_group_action(state, action, group, options, &resolved, &skipped); }
    let mut out = String::new();
    let icon = if action == "wake" { "🌅" } else { "🌙" };
    let _ = writeln!(out, "{icon} Fleet {action} plan node: {}", state.config.node);
    let _ = writeln!(out, "  squad: {group} · members: {} · sessions: {} · skipped: {}", members.len(), resolved.len(), skipped.len());
    for (handle, session) in &resolved { let _ = writeln!(out, "  - {handle} -> {}", session.name); }
    for handle in &skipped { let _ = writeln!(out, "  - {handle} skipped: no session"); }
    Ok((0, out))
}

fn fleet_run_group_post_wake_hooks(resolved: &[(&str, &FleetSessionSummary)]) {
    // A squad has no single repo path, so group hooks keep the process-cwd
    // (global) config read (`None`); per-member dir-aware resolution is an
    // explicit follow-up to #600 — do not change fleet semantics here.
    let hooks = wake_config_post_wake_hooks(None);
    if hooks.is_empty() {
        return;
    }
    for (handle, session) in resolved {
        let window = fleet_member_hook_window(handle, session);
        wake_run_post_wake_hooks(handle, &session.name, &window, &hooks);
    }
}

fn fleet_member_hook_window(handle: &str, session: &FleetSessionSummary) -> String {
    let wanted = maw_matcher::normalized_match_names(handle);
    session
        .windows
        .iter()
        .find(|window| {
            maw_matcher::normalized_match_names(&window.name)
                .iter()
                .any(|name| wanted.contains(name))
        })
        .or_else(|| session.windows.first())
        .map_or_else(|| session.name.clone(), |window| window.name.clone())
}

fn fleet_member_session<'a>(handle: &str, sessions: &'a [FleetSessionSummary]) -> Option<&'a FleetSessionSummary> {
    let wanted = maw_matcher::normalized_match_names(handle);
    sessions.iter().find(|session| {
        maw_matcher::normalized_match_names(&session.name)
            .iter()
            .any(|name| wanted.contains(name))
            || session
                .windows
                .iter()
                .any(|window| {
                    maw_matcher::normalized_match_names(&window.name)
                        .iter()
                        .any(|name| wanted.contains(name))
                })
    })
}

fn fleet_json_group_action(
    state: &FleetState,
    action: &str,
    group: &str,
    options: &FleetOptions,
    resolved: &[(&str, &FleetSessionSummary)],
    skipped: &[&str],
) -> Result<(i32, String), String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "action": action,
        "dryRun": options.dry_run,
        "squad": group,
        "sessionCount": resolved.len(),
        "sessions": resolved.iter().map(|(_, session)| session.name.clone()).collect::<Vec<_>>(),
        "members": resolved.iter().map(|(handle, session)| serde_json::json!({"handle": handle, "session": session.name})).collect::<Vec<_>>(),
        "skipped": skipped.iter().map(|handle| serde_json::json!({"handle": handle, "reason": "no session"})).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| (0, format!("{text}\n"))).map_err(|error| error.to_string())
}

fn fleet_write_session_plan(out: &mut String, sessions: &[FleetSessionSummary]) {
    for session in sessions {
        let _ = writeln!(out, "  - {}", session.name);
        for window in &session.windows {
            let _ = writeln!(out, "      {} -> {}", window.name, window.repo);
        }
    }
}

fn fleet_run_gather(
    state: &FleetState,
    options: &FleetOptions,
    runtime: &mut impl FleetRuntime,
) -> Result<(i32, String), String> {
    let group = options.target.as_deref().ok_or_else(|| "fleet gather: missing squad".to_owned())?;
    let entry = state
        .fleet_entries
        .iter()
        .find(|entry| fleet_roster_entry_matches(entry, group))
        .ok_or_else(|| format!("fleet gather: no squad named {group} — try: maw fleet create {group}"))?;
    let members = entry.session.members.as_deref().unwrap_or_default();
    if members.is_empty() { return Err(format!("fleet gather: squad {group} has no members")); }
    let registered = fleet_sweep_targets(state);
    let live = runtime.fleet_list_all()?.into_iter().map(|session| session.name).collect::<BTreeSet<_>>();
    let plan = members.iter().map(|member| {
        let session = fleet_member_session(&member.handle, &registered);
        let live_session = session.filter(|candidate| live.contains(&candidate.name));
        (member.handle.as_str(), live_session)
    }).collect::<Vec<_>>();
    if options.json { return fleet_json_gather(state, group, options, &plan); }
    if options.dry_run { return Ok((0, fleet_render_gather(state, group, options, &plan, None))); }

    let mut runner = maw_tmux::CommandTmuxRunner::new();
    let target = fleet_gather_current_target(&mut runner)?;
    let mut changed = false;
    for (_, session) in &plan {
        let Some(session) = session else { continue; };
        let window = session.windows.first().map_or("main", |window| window.name.as_str());
        let source = format!("{}:{window}", session.name);
        if options.scatter {
            tmux_break_with_runner(&[source.clone(), "--force".to_owned()], &mut runner)
                .map_err(|(_, message)| format!("fleet gather: {message}"))?;
        } else {
            join_with_runner(&[source, "--to".to_owned(), target.clone()], &mut runner)
                .map_err(|(_, message)| format!("fleet gather: {message}"))?;
        }
        changed = true;
    }
    if changed && !options.scatter {
        tmux_layout_current_with_runner("main-vertical", &mut runner)
            .map_err(|(_, message)| format!("fleet gather: {message}"))?;
    }
    Ok((0, fleet_render_gather(state, group, options, &plan, Some(&target))))
}

fn fleet_gather_current_target<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, String> {
    let raw = runner.run("display-message", &["-p".to_owned(), "#{pane_id}".to_owned()])
        .map_err(|error| format!("fleet gather: current tmux pane unavailable: {}", error.message))?;
    let pane = raw.trim();
    if pane.is_empty() { Err("fleet gather: current tmux pane unavailable".to_owned()) } else { Ok(pane.to_owned()) }
}

fn fleet_render_gather(state: &FleetState, group: &str, options: &FleetOptions, plan: &[(&str, Option<&FleetSessionSummary>)], target: Option<&str>) -> String {
    let mut out = String::new();
    let action = if options.scatter { "scatter" } else { "gather" };
    let _ = writeln!(out, "fleet {action} plan node: {}", state.config.node);
    let _ = writeln!(out, "  squad: {group} · dry-run: {}", options.dry_run);
    if let Some(target) = target { let _ = writeln!(out, "  target: {target}"); }
    for (handle, session) in plan {
        if let Some(session) = session {
            let window = session.windows.first().map_or("main", |window| window.name.as_str());
            let verb = if options.scatter { "break" } else { "join" };
            let _ = writeln!(out, "  - {handle} live: {verb} {}:{window}", session.name);
        } else {
            let _ = writeln!(out, "  - {handle} asleep: skipped (no auto-wake in v1)");
        }
    }
    if plan.iter().any(|(_, session)| session.is_some()) && !options.scatter { out.push_str("  - layout: main-vertical\n"); }
    out
}

fn fleet_json_gather(state: &FleetState, group: &str, options: &FleetOptions, plan: &[(&str, Option<&FleetSessionSummary>)]) -> Result<(i32, String), String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "action": if options.scatter { "scatter" } else { "gather" },
        "dryRun": options.dry_run,
        "squad": group,
        "members": plan.iter().map(|(handle, session)| serde_json::json!({
            "handle": handle,
            "state": if session.is_some() { "live" } else { "asleep" },
            "session": session.map(|session| session.name.clone()),
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| (0, format!("{text}\n"))).map_err(|error| error.to_string())
}

fn fleet_run_sleep(state: &FleetState, options: &FleetOptions) -> Result<(i32, String), String> {
    if let Some(group) = options.target.as_deref() {
        return fleet_run_group_action(state, options, "sleep", group);
    }
    let sessions = fleet_sweep_targets(state);
    if options.json { return Ok((0, fleet_json_action(state, "sleep", &sessions, options)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🌙 Fleet sleep plan node: {}", state.config.node);
    fleet_write_session_plan(&mut out, &sessions);
    Ok((0, out))
}

fn fleet_run_named_plan(state: &FleetState, options: &FleetOptions, action: &str) -> Result<(i32, String), String> {
    if options.json { return Ok((0, fleet_json_action(state, action, &state.sessions, options)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "fleet {action} plan node: {}", state.config.node);
    let _ = writeln!(out, "  dry-run: {}", options.dry_run || matches!(action, "init" | "consolidate" | "resume" | "sync"));
    let _ = writeln!(out, "  sessions: {} · peers: {}", state.sessions.len(), state.config.peers.len());
    Ok((0, out))
}

fn fleet_json_action(
    state: &FleetState,
    action: &str,
    sessions: &[FleetSessionSummary],
    options: &FleetOptions,
) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "action": action,
        "dryRun": options.dry_run,
        "all": options.all,
        "sessionCount": sessions.len(),
        "sessions": sessions.iter().map(|session| session.name.clone()).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}
