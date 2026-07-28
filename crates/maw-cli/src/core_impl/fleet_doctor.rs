// What is wrong with this fleet that nobody has noticed yet.
//
// Each check is an invariant the registry is supposed to hold: no duplicate
// peers, no agent routed to an unknown node, no window pointing at a repo that
// is not cloned, no two aliases resolving to one live window, and no oracle name
// so ambiguous that `maw wake` could never pick a target (#711/#714). A finding
// carries its own reason, because a health check that only says "unhealthy" just
// moves the investigation somewhere else.

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetFinding {
    level: String,
    code: String,
    subject: String,
    detail: String,
}

fn fleet_run_doctor(state: &FleetState, options: &FleetOptions, runtime: &mut impl FleetRuntime) -> Result<(i32, String), String> {
    let apply_fix = options.fix && !options.dry_run;
    let live = runtime.fleet_list_all();
    let repairs = if apply_fix { fleet_fix_duplicate_windows(&state.fleet_entries, &live)? } else { Vec::new() };
    let refreshed = if apply_fix { Some(fleet_load_state_with(runtime)?) } else { None };
    let state = refreshed.as_ref().unwrap_or(state);
    let mut findings = fleet_findings(state, &live);
    if options.reboot { findings.extend(fleet_reboot_findings(state)); }
    let code = fleet_exit_code(&findings);
    if options.json { return Ok((code, fleet_json_doctor(state, apply_fix, &findings, &repairs)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🩺 Fleet Doctor node: {}", state.config.node);
    let _ = writeln!(out, "  peers: {} · agents: {} · sessions: {}", state.config.peers.len(), state.config.agents.len(), state.sessions.len());
    let _ = writeln!(out, "  mode: {}", if apply_fix { "repairs applied" } else { "dry-run repair plan" });
    for repair in &repairs {
        let _ = writeln!(out, "  [fixed] duplicate-window-repo {} — removed {} from {}", repair.session, repair.removed, repair.path.display());
    }
    fleet_write_findings(&mut out, &findings);
    Ok((code, out))
}

fn fleet_json_doctor(state: &FleetState, fix_applied: bool, findings: &[FleetFinding], repairs: &[FleetWindowRepair]) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "dryRun": !fix_applied,
        "findings": findings.iter().map(fleet_json_finding).collect::<Vec<_>>(),
        "repairs": repairs.iter().map(|repair| serde_json::json!({
            "session": repair.session,
            "path": repair.path,
            "removed": repair.removed,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_json_finding(finding: &FleetFinding) -> serde_json::Value {
    serde_json::json!({
        "level": finding.level,
        "code": finding.code,
        "subject": finding.subject,
        "detail": finding.detail,
    })
}

fn fleet_write_findings(out: &mut String, findings: &[FleetFinding]) {
    if findings.is_empty() {
        let _ = writeln!(out, "  ok: no fleet findings");
        return;
    }
    for finding in findings {
        let _ = writeln!(out, "  [{}] {} {} — {}", finding.level, finding.code, finding.subject, finding.detail);
    }
}

fn fleet_findings(state: &FleetState, live: &[TmuxSession]) -> Vec<FleetFinding> {
    let mut findings = Vec::new();
    fleet_duplicate_peer_findings(state, &mut findings);
    fleet_self_peer_findings(state, &mut findings);
    fleet_agent_findings(state, &mut findings);
    fleet_repo_findings(state, &mut findings);
    fleet_duplicate_window_findings(state, live, &mut findings);
    fleet_duplicate_session_findings(state, &mut findings);
    fleet_resolvability_findings(state, &mut findings);
    findings
}

// #711: sibling windows on the same repo inherit identical repo-derived
// aliases (`wake_registry_aliases`), so `maw wake <oracle>` can be
// permanently ambiguous even though the registry itself looks fine --
// `fleet_duplicate_window_findings` above only catches windows that also
// share a *name* alias, a different case. This runs the same resolver wake
// itself uses and flags any oracle identity that can never pick a single
// target, so it shows up in `doctor` instead of at the moment someone
// actually tries to wake it.
fn fleet_resolvability_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    // `&[]`, deliberately: this judges the registry's own naming, never
    // whether a window happens to be live right now (#711 part 2). A fleet
    // with a genuinely ambiguous alias is worth flagging even while one
    // window is transiently live to break the tie for wake's own purposes --
    // that tie doesn't hold the moment the live window closes.
    let candidates = wake_typed_registry_candidates(&state.fleet_entries, &[]);
    let typed = candidates.iter().map(|candidate| candidate.candidate.clone()).collect::<Vec<_>>();
    let mut checked = BTreeSet::new();
    for candidate in &candidates {
        if !checked.insert(candidate.oracle.clone()) {
            continue;
        }
        if let maw_matcher::ResolveTypedResult::Ambiguous { candidates: ambiguous } =
            maw_matcher::resolve_typed_target(&candidate.oracle, &typed)
        {
            let names = ambiguous.iter().map(|matched| matched.candidate.name.as_str()).collect::<Vec<_>>().join(", ");
            findings.push(fleet_finding(
                "fatal",
                "ambiguous-oracle",
                &candidate.oracle,
                &format!(
                    "resolves to {} registry windows ({names}); 'maw wake {}' can never pick one",
                    ambiguous.len(),
                    candidate.oracle,
                ),
            ));
        }
    }
}

fn fleet_duplicate_window_findings(state: &FleetState, live: &[TmuxSession], findings: &mut Vec<FleetFinding>) {
    for entry in state.fleet_entries.iter().filter(|entry| fleet_entry_is_session(entry)) {
        let by_repo = fleet_windows_by_repo(entry);
        for windows in by_repo.values().filter(|windows| windows.len() > 1) {
            if !fleet_windows_share_alias(windows) || fleet_distinct_live_window_ids(entry, windows, live).len() > 1 { continue; }
            let kept = windows.iter().rev().find(|window| window.kind.is_some()).unwrap_or(&windows[windows.len() - 1]);
            let names = windows.iter().map(|window| window.name.as_str()).collect::<Vec<_>>().join(", ");
            findings.push(fleet_finding(
                "fatal",
                "duplicate-window-repo",
                &entry.session.name,
                &format!(
                    "{} aliases ({names}) share repo {} and resolve to at most one live window; --fix keeps {} (last entry with explicit kind, otherwise last entry)",
                    windows.len(),
                    fleet_repo_storage_slug(&windows[0].repo),
                    kept.name,
                ),
            ));
        }
    }
}

fn fleet_windows_by_repo(entry: &NativeFleetEntry) -> BTreeMap<String, Vec<&NativeFleetWindow>> {
    let mut by_repo = BTreeMap::new();
    for window in &entry.session.windows {
        if !window.repo.trim().is_empty() {
            by_repo.entry(fleet_repo_canonical_key(&window.repo)).or_insert_with(Vec::new).push(window);
        }
    }
    by_repo
}

fn fleet_windows_share_alias(windows: &[&NativeFleetWindow]) -> bool {
    let Some(first) = windows.first() else { return false };
    let mut common = maw_matcher::normalized_match_names(&first.name);
    for window in &windows[1..] {
        let aliases = maw_matcher::normalized_match_names(&window.name);
        common.retain(|alias| aliases.contains(alias));
    }
    !common.is_empty()
}

fn fleet_distinct_live_window_ids(entry: &NativeFleetEntry, windows: &[&NativeFleetWindow], live: &[TmuxSession]) -> BTreeSet<u32> {
    let Some(session) = live.iter().find(|session| session.name.eq_ignore_ascii_case(&entry.session.name)) else { return BTreeSet::new() };
    windows
        .iter()
        .flat_map(|window| fleet_live_window_candidates(&session.name, &window.name, &session.windows))
        .collect()
}

fn fleet_live_window_candidates(session: &str, registry: &str, live: &[maw_tmux::TmuxWindow]) -> BTreeSet<u32> {
    let wanted = fleet_doctor_window_name(registry);
    let exact = live
        .iter()
        .filter(|window| fleet_live_window_names(session, &window.name).contains(&wanted))
        .map(|window| window.index)
        .collect::<BTreeSet<_>>();
    if !exact.is_empty() { return exact; }
    let wanted = maw_matcher::normalized_match_names(&wanted);
    live.iter()
        .filter(|window| {
            fleet_live_window_names(session, &window.name)
                .iter()
                .any(|name| maw_matcher::normalized_match_names(name).iter().any(|alias| wanted.contains(alias)))
        })
        .map(|window| window.index)
        .collect()
}

fn fleet_live_window_names(session: &str, window: &str) -> BTreeSet<String> {
    let name = fleet_doctor_window_name(window);
    let stem = fleet_doctor_window_name(fleet_session_stem(session));
    let mut names = BTreeSet::from([name.clone()]);
    if let Some(tail) = name.strip_prefix(&format!("{stem}-")) {
        names.insert(fleet_doctor_window_name(tail));
    }
    names
}

fn fleet_doctor_window_name(name: &str) -> String {
    let normalized = name.trim().to_lowercase();
    normalized.strip_suffix('-').unwrap_or(&normalized).to_owned()
}

fn fleet_duplicate_peer_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    let mut seen = BTreeSet::new();
    for peer in &state.config.peers {
        if !seen.insert(peer.name.clone()) {
            findings.push(fleet_finding("fatal", "duplicate-peer", &peer.name, "peer name appears more than once"));
        }
    }
}

fn fleet_self_peer_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    for peer in &state.config.peers {
        if peer.name == state.config.node {
            findings.push(fleet_finding("warn", "self-peer", &peer.name, "named peer points at this node"));
        }
    }
}

fn fleet_agent_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    let peers = fleet_known_nodes(state);
    for (agent, node) in &state.config.agents {
        if !peers.contains(node) {
            findings.push(fleet_finding("warn", "missing-agent-peer", agent, &format!("agent routes to unknown node {node}")));
        }
    }
}

fn fleet_known_nodes(state: &FleetState) -> BTreeSet<String> {
    let mut peers = BTreeSet::from([state.config.node.clone(), "local".to_owned()]);
    peers.extend(state.config.peers.iter().map(|peer| peer.name.clone()));
    peers
}

fn fleet_repo_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    for session in &state.sessions {
        for window in &session.windows {
            if window.repo.trim().is_empty() {
                continue;
            }
            let path = fleet_repo_path(&state.repos_root, &window.repo);
            if !path.exists() {
                findings.push(fleet_finding("warn", "missing-repo", &window.repo, &format!("{} missing", path.display())));
            }
        }
    }
}

fn fleet_duplicate_session_findings(state: &FleetState, findings: &mut Vec<FleetFinding>) {
    let mut seen = BTreeSet::new();
    for session in &state.sessions {
        if !seen.insert(session.name.clone()) {
            findings.push(fleet_finding("fatal", "duplicate-session", &session.name, "fleet session appears more than once"));
        }
    }
}

fn fleet_reboot_findings(state: &FleetState) -> Vec<FleetFinding> {
    if state.sessions.is_empty() {
        return vec![fleet_finding("warn", "reboot-empty-fleet", &state.config.node, "no fleet sessions configured")];
    }
    Vec::new()
}

fn fleet_finding(level: &str, code: &str, subject: &str, detail: &str) -> FleetFinding {
    FleetFinding { level: level.to_owned(), code: code.to_owned(), subject: subject.to_owned(), detail: detail.to_owned() }
}

fn fleet_exit_code(findings: &[FleetFinding]) -> i32 {
    if findings.iter().any(|finding| finding.level == "fatal") {
        2
    } else {
        i32::from(!findings.is_empty())
    }
}
