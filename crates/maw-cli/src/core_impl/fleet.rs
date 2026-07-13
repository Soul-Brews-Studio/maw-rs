const DISPATCH_61: &[DispatcherEntry] = &[DispatcherEntry {
    command: "fleet",
    handler: Handler::Sync(run_fleet_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct FleetOptions {
    command: FleetCommand,
    target: Option<String>,
    json: bool,
    dry_run: bool,
    fix: bool,
    reboot: bool,
    all: bool,
    kill: bool,
    resume: bool,
    scatter: bool,
    include_99: bool,
    only_99: bool,
    squads: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FleetCommand {
    Add,
    Census,
    Doctor,
    Gc,
    Init,
    Health,
    Consolidate,
    Resume,
    Sync,
    Wake,
    Sleep,
    Gather,
    Renumber,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetConfigSummary {
    node: String,
    peers: Vec<FleetPeerSummary>,
    agents: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetPeerSummary {
    name: String,
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetSessionSummary {
    name: String,
    windows: Vec<FleetWindowSummary>,
    disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGroupMemberSummary {
    handle: String,
    session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGroupSummary {
    name: String,
    path: std::path::PathBuf,
    members: Vec<FleetGroupMemberSummary>,
    sessions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetWindowSummary {
    name: String,
    repo: String,
    kind: Option<NativeRepoKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetState {
    config_dir: std::path::PathBuf,
    repos_root: std::path::PathBuf,
    config: FleetConfigSummary,
    fleet_entries: Vec<NativeFleetEntry>,
    sessions: Vec<FleetSessionSummary>,
    disabled_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetFinding {
    level: String,
    code: String,
    subject: String,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetWindowRepair {
    session: String,
    path: std::path::PathBuf,
    removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGcCandidate {
    name: String,
    path: std::path::PathBuf,
    disabled_path: std::path::PathBuf,
    missing_repos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGcResult {
    name: String,
    path: std::path::PathBuf,
    disabled_path: std::path::PathBuf,
    missing_repos: Vec<String>,
    status: String,
    detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetRegistryWrite {
    path: std::path::PathBuf,
    created: bool,
    window_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetRenumberItem {
    old_name: String,
    new_name: String,
    old_file: String,
    new_file: String,
    path: std::path::PathBuf,
    changed: bool,
    tmux: Option<String>,
    tmux_error: Option<String>,
}

trait FleetRuntime {
    fn fleet_run_command(&mut self, program: &str, args: &[String]) -> Result<String, String>;
    fn fleet_list_all(&mut self) -> Vec<TmuxSession>;
}

struct FleetSystemRuntime;

impl FleetRuntime for FleetSystemRuntime {
    fn fleet_run_command(&mut self, program: &str, args: &[String]) -> Result<String, String> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn fleet_list_all(&mut self) -> Vec<TmuxSession> {
        TmuxClient::local().list_all()
    }
}

fn run_fleet_command(argv: &[String]) -> CliOutput {
    if argv.first().is_some_and(|arg| arg == "token") {
        return zai_fleet_token(&argv[1..]);
    }
    match fleet_run(argv) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn fleet_run(argv: &[String]) -> Result<(i32, String), String> {
    if let Some(result) = fleet_roster_intercept(argv) {
        return result;
    }
    let mut runtime = FleetSystemRuntime;
    fleet_run_with(argv, &mut runtime)
}

fn fleet_run_with(argv: &[String], runtime: &mut impl FleetRuntime) -> Result<(i32, String), String> {
    let options = fleet_parse_args(argv)?;
    let state = fleet_load_state_with(runtime)?;
    match options.command {
        FleetCommand::Add => fleet_run_add(&state, &options, runtime),
        FleetCommand::Census => Ok((0, fleet_render_census(&state, &options)?)),
        FleetCommand::Doctor | FleetCommand::Health => fleet_run_doctor(&state, &options, runtime),
        FleetCommand::Gc => fleet_run_gc(&state, &options, &mut maw_tmux::CommandTmuxRunner::new()),
        FleetCommand::Wake => fleet_run_wake(&state, &options),
        FleetCommand::Sleep => fleet_run_sleep(&state, &options),
        FleetCommand::Gather => fleet_run_gather(&state, &options, runtime),
        FleetCommand::Renumber => fleet_run_renumber(&state, &options, runtime),
        FleetCommand::Init => fleet_run_named_plan(&state, &options, "init"),
        FleetCommand::Consolidate => fleet_run_named_plan(&state, &options, "consolidate"),
        FleetCommand::Resume => fleet_run_named_plan(&state, &options, "resume"),
        FleetCommand::Sync => fleet_run_named_plan(&state, &options, "sync"),
    }
}

fn fleet_parse_args(argv: &[String]) -> Result<FleetOptions, String> {
    let mut options = fleet_default_options();
    let mut command_seen = false;
    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].as_str();
        match arg {
            "--help" | "-h" => return Err(fleet_usage()),
            "--json" => options.json = true,
            "--dry-run" => options.dry_run = true,
            "--fix" => options.fix = true,
            "--reboot" => options.reboot = true,
            "--all" => options.all = true,
            "--kill" => options.kill = true,
            "--resume" => options.resume = true,
            "--scatter" => options.scatter = true,
            "--include-99" => options.include_99 = true,
            "--only-99" => options.only_99 = true,
            "--squads" => {
                index += 1;
                let Some(raw) = argv.get(index) else { return Err("fleet: --squads requires a value".to_owned()); };
                let values = fleet_parse_squad_filter(raw);
                if values.is_empty() {
                    return Err("fleet: --squads requires at least one value".to_owned());
                }
                options.squads.extend(values);
            }
            value if value.starts_with("--squads=") => {
                let raw = value["--squads=".len()..].trim();
                if raw.is_empty() {
                    return Err("fleet: --squads requires a value".to_owned());
                }
                let values = fleet_parse_squad_filter(raw);
                if values.is_empty() {
                    return Err("fleet: --squads requires at least one value".to_owned());
                }
                options.squads.extend(values);
            }
            value if value.starts_with('-') => return Err(format!("fleet: unknown argument {value}")),
            value => fleet_parse_positional(&mut options, &mut command_seen, value)?,
        }
        index += 1;
    }
    if matches!(options.command, FleetCommand::Add) && options.target.is_none() {
        return Err("fleet add: missing session".to_owned());
    }
    if matches!(options.command, FleetCommand::Wake | FleetCommand::Sleep) && options.target.is_none() && !options.all {
        let action = if options.command == FleetCommand::Wake { "wake" } else { "sleep" };
        return Err(format!("fleet {action}: specify a squad, or --all to {action} every registered session on this node"));
    }
    if matches!(options.command, FleetCommand::Gather) && options.target.is_none() {
        return Err("fleet gather: missing squad".to_owned());
    }
    Ok(options)
}

fn fleet_default_options() -> FleetOptions {
    FleetOptions {
        command: FleetCommand::Census,
        target: None,
        json: false,
        dry_run: false,
        fix: false,
        reboot: false,
        all: false,
        kill: false,
        resume: false,
        scatter: false,
        include_99: false,
        only_99: false,
        squads: Vec::new(),
    }
}

fn fleet_parse_positional(options: &mut FleetOptions, seen: &mut bool, value: &str) -> Result<(), String> {
    if !*seen {
        return fleet_set_command(options, seen, value);
    }
    if matches!(options.command, FleetCommand::Add | FleetCommand::Wake | FleetCommand::Sleep | FleetCommand::Gather) && options.target.is_none() {
        fleet_validate_session_name(value)?;
        options.target = Some(value.to_owned());
        return Ok(());
    }
    Err(fleet_usage())
}

fn fleet_set_command(options: &mut FleetOptions, seen: &mut bool, value: &str) -> Result<(), String> {
    if *seen { return Err(fleet_usage()); }
    options.command = match value {
        "add" => FleetCommand::Add,
        "ls" | "list" | "census" => FleetCommand::Census,
        "doctor" => FleetCommand::Doctor,
        "gc" | "garbage-collect" => FleetCommand::Gc,
        "init" => FleetCommand::Init,
        "health" => FleetCommand::Health,
        "consolidate" => FleetCommand::Consolidate,
        "resume" => FleetCommand::Resume,
        "sync" => FleetCommand::Sync,
        "wake" => FleetCommand::Wake,
        "wake-all" => {
            options.all = true;
            FleetCommand::Wake
        }
        "sleep" => FleetCommand::Sleep,
        "gather" => FleetCommand::Gather,
        "renumber" => FleetCommand::Renumber,
        _ => return Err(format!("fleet: unknown subcommand {value}")),
    };
    *seen = true;
    Ok(())
}

fn fleet_parse_squad_filter(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn fleet_usage() -> String {
    "usage: maw fleet [add <session>|create <squad>|show <squad>|status <squad>|join <fleet> --code <code>|remove <squad> <handle>|leave <squad>|ls|doctor|health|gc|init|consolidate|resume|sync|wake <squad|--all>|sleep <squad|--all>|gather <squad>|renumber|token <squad> [ls|status]] [--json] [--dry-run] [--include-99|--only-99] [--fix] [--reboot] [--all] [--kill] [--resume] [--scatter] [--squads <squad[,squad]...>]".to_owned()
}

fn fleet_load_state_with(runtime: &mut impl FleetRuntime) -> Result<FleetState, String> {
    let env = current_xdg_env();
    let config_dir = maw_config_dir(&env);
    let repos_root = fleet_repos_root(runtime);
    let config = fleet_load_config(&env);
    let fleet_entries = fleet_load_entries_result_for_env(&env, "fleet")?;
    let sessions = fleet_entries_to_summaries(&fleet_entries);
    let disabled_count = fleet_disabled_count_for_env(&env);
    Ok(FleetState { config_dir, repos_root, config, fleet_entries, sessions, disabled_count })
}

fn fleet_repos_root(runtime: &mut impl FleetRuntime) -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("GHQ_ROOT") {
        return fleet_normalize_repos_root(std::path::PathBuf::from(root));
    }
    if let Ok(stdout) = runtime.fleet_run_command("ghq", &["root".to_owned()]) {
        let root = stdout.trim();
        if !root.is_empty() {
            return fleet_normalize_repos_root(std::path::PathBuf::from(root));
        }
    }
    fleet_normalize_repos_root(std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from(".").join("Code"),
        |home| std::path::PathBuf::from(home).join("Code"),
    ))
}

fn fleet_normalize_repos_root(root: std::path::PathBuf) -> std::path::PathBuf {
    if root.file_name().is_some_and(|name| name == "github.com") {
        root
    } else {
        root.join("github.com")
    }
}

fn fleet_repo_path(repos_root: &std::path::Path, repo: &str) -> std::path::PathBuf {
    let repo = repo.trim().strip_prefix("github.com/").unwrap_or(repo.trim());
    repos_root.join(repo)
}

fn fleet_load_config(env: &MawXdgEnv) -> FleetConfigSummary {
    let value = merged_config_value_for_env(env);
    let node = value.get("node").and_then(serde_json::Value::as_str).unwrap_or("local").to_owned();
    let peers = fleet_parse_peers(&value);
    let agents = fleet_parse_agents(&value);
    FleetConfigSummary { node, peers, agents }
}

fn fleet_parse_peers(value: &serde_json::Value) -> Vec<FleetPeerSummary> {
    value
        .get("namedPeers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(fleet_peer_from_value)
        .collect()
}

fn fleet_peer_from_value(value: &serde_json::Value) -> Option<FleetPeerSummary> {
    let name = value.get("name")?.as_str()?.to_owned();
    let url = value.get("url").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    Some(FleetPeerSummary { name, url })
}

fn fleet_parse_agents(value: &serde_json::Value) -> BTreeMap<String, String> {
    let mut agents = BTreeMap::new();
    let Some(map) = value.get("agents").and_then(serde_json::Value::as_object) else { return agents; };
    for (name, route) in map {
        if let Some(text) = route.as_str() {
            agents.insert(name.clone(), fleet_agent_node(text));
        } else if let Some(text) = route.get("node").and_then(serde_json::Value::as_str) {
            agents.insert(name.clone(), text.to_owned());
        }
    }
    agents
}

fn fleet_agent_node(value: &str) -> String {
    value.split(':').next().unwrap_or(value).to_owned()
}

fn fleet_entry_is_session(entry: &NativeFleetEntry) -> bool {
    entry.session.members.is_none()
}

fn fleet_entries_to_summaries(entries: &[NativeFleetEntry]) -> Vec<FleetSessionSummary> {
    entries
        .iter()
        .filter(|entry| fleet_entry_is_session(entry))
        .map(|entry| FleetSessionSummary {
            name: entry.session.name.clone(),
            windows: entry
                .session
                .windows
                .iter()
                .map(|window| FleetWindowSummary {
                    name: if window.name.is_empty() { "main".to_owned() } else { window.name.clone() },
                    repo: window.repo.clone(),
                    kind: window.kind,
                })
                .collect(),
            disabled: false,
        })
        .collect()
}

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

fn fleet_run_add(
    state: &FleetState,
    options: &FleetOptions,
    runtime: &mut impl FleetRuntime,
) -> Result<(i32, String), String> {
    let session = options.target.as_deref().ok_or_else(|| "fleet add: missing session".to_owned())?;
    let live = runtime
        .fleet_list_all()
        .into_iter()
        .find(|item| item.name == session)
        .ok_or_else(|| format!("fleet add: live session not found: {session}"))?;
    let windows = fleet_registry_windows_from_tmux(&live.windows, Some(&state.repos_root));
    if windows.is_empty() {
        return Err(format!("fleet add: no repo-backed windows found in session {session}"));
    }
    let result = fleet_registry_upsert_session(session, &windows, "maw fleet add")?;
    if options.json {
        return Ok((0, fleet_json_add(session, &result)?));
    }
    Ok((0, fleet_render_add(session, &result)))
}

fn fleet_json_add(session: &str, result: &FleetRegistryWrite) -> Result<String, String> {
    let value = serde_json::json!({
        "action": "add",
        "session": session,
        "path": result.path,
        "status": if result.created { "created" } else { "updated" },
        "windowCount": result.window_count,
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_render_add(session: &str, result: &FleetRegistryWrite) -> String {
    format!(
        "fleet add {session}: {} {} ({} window{})\n",
        if result.created { "created" } else { "updated" },
        result.path.display(),
        result.window_count,
        if result.window_count == 1 { "" } else { "s" },
    )
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
    findings
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
    let mut common = locate_normalized_names(&first.name);
    for window in &windows[1..] {
        let aliases = locate_normalized_names(&window.name);
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
    let wanted = locate_normalized_names(&wanted);
    live.iter()
        .filter(|window| {
            fleet_live_window_names(session, &window.name)
                .iter()
                .any(|name| locate_normalized_names(name).iter().any(|alias| wanted.contains(alias)))
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

fn fleet_run_gc<R: maw_tmux::TmuxRunner>(
    state: &FleetState,
    options: &FleetOptions,
    runner: &mut R,
) -> Result<(i32, String), String> {
    let live = fleet_live_session_names(runner)?;
    let candidates = fleet_gc_candidates(state, &live);
    let results = if options.dry_run {
        candidates
            .into_iter()
            .map(|candidate| fleet_gc_result(candidate, "planned", None))
            .collect::<Vec<_>>()
    } else {
        fleet_apply_gc_candidates(candidates)
    };
    let code = i32::from(results.iter().any(|result| result.status == "failed"));
    if options.json {
        return Ok((code, fleet_json_gc(state, options, &live, &results)?));
    }
    Ok((code, fleet_render_gc(state, options, &live, &results)))
}

fn fleet_live_session_names<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<BTreeSet<String>, String> {
    let args = ["-F".to_owned(), "#{session_name}".to_owned()];
    let raw = match runner.run("list-sessions", &args) {
        Ok(raw) => raw,
        Err(error) if error.message.contains("no server running") => String::new(),
        Err(error) => return Err(format!("fleet gc: cannot list tmux sessions: {}", error.message)),
    };
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn fleet_gc_candidates(state: &FleetState, live: &BTreeSet<String>) -> Vec<FleetGcCandidate> {
    let mut candidates = Vec::new();
    for entry in state.fleet_entries.iter().filter(|entry| fleet_entry_is_session(entry)) {
        if live.contains(&entry.session.name) {
            continue;
        }
        let repos = fleet_session_repo_slugs(&entry.session);
        if repos.is_empty() {
            continue;
        }
        let missing = repos
            .iter()
            .filter(|repo| !fleet_repo_path(&state.repos_root, repo).exists())
            .cloned()
            .collect::<Vec<_>>();
        if missing.len() == repos.len() {
            candidates.push(FleetGcCandidate {
                name: entry.session.name.clone(),
                path: entry.path.clone(),
                disabled_path: fleet_disabled_path(&entry.path),
                missing_repos: missing,
            });
        }
    }
    candidates
}

fn fleet_session_repo_slugs(session: &NativeFleetSession) -> Vec<String> {
    let mut repos = session
        .windows
        .iter()
        .map(|window| window.repo.trim())
        .filter(|repo| !repo.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    repos.sort();
    repos.dedup();
    repos
}

fn fleet_apply_gc_candidates(candidates: Vec<FleetGcCandidate>) -> Vec<FleetGcResult> {
    candidates
        .into_iter()
        .map(|candidate| {
            if candidate.disabled_path.exists() {
                return fleet_gc_result(candidate, "skipped", Some("disabled file already exists".to_owned()));
            }
            match std::fs::rename(&candidate.path, &candidate.disabled_path) {
                Ok(()) => fleet_gc_result(candidate, "disabled", None),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => fleet_gc_result(candidate, "skipped", Some("source file is already gone".to_owned())),
                Err(error) => fleet_gc_result(candidate, "failed", Some(error.to_string())),
            }
        })
        .collect()
}

fn fleet_gc_result(candidate: FleetGcCandidate, status: &str, detail: Option<String>) -> FleetGcResult {
    FleetGcResult {
        name: candidate.name,
        path: candidate.path,
        disabled_path: candidate.disabled_path,
        missing_repos: candidate.missing_repos,
        status: status.to_owned(),
        detail,
    }
}

fn fleet_render_gc(
    state: &FleetState,
    options: &FleetOptions,
    live: &BTreeSet<String>,
    results: &[FleetGcResult],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fleet gc node {}", state.config.node);
    let _ = writeln!(out, "  live sessions: {}", live.len());
    let _ = writeln!(out, "  candidates: {}", results.len());
    if results.is_empty() {
        out.push_str("  ok: no stale fleet entries\n");
        return out;
    }
    for result in results {
        let verb = if options.dry_run {
            "[dry-run] would disable"
        } else {
            result.status.as_str()
        };
        let _ = write!(
            out,
            "  - {verb} {} -> {}",
            result.path.display(),
            result.disabled_path.display()
        );
        if !result.missing_repos.is_empty() {
            let _ = write!(out, " (missing repos: {})", result.missing_repos.join(", "));
        }
        if let Some(detail) = &result.detail {
            let _ = write!(out, " ({detail})");
        }
        out.push('\n');
    }
    out
}

fn fleet_json_gc(
    state: &FleetState,
    options: &FleetOptions,
    live: &BTreeSet<String>,
    results: &[FleetGcResult],
) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "dryRun": options.dry_run,
        "liveSessionCount": live.len(),
        "candidateCount": results.len(),
        "candidates": results.iter().map(fleet_json_gc_result).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_json_gc_result(result: &FleetGcResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "path": result.path,
        "disabledPath": result.disabled_path,
        "missingRepos": result.missing_repos,
        "status": result.status,
        "detail": result.detail,
    })
}

fn fleet_run_renumber(state: &FleetState, options: &FleetOptions, runtime: &mut impl FleetRuntime) -> Result<(i32, String), String> {
    let mut items = fleet_renumber_plan(&state.fleet_entries, options.include_99, options.only_99);
    let live = runtime.fleet_list_all().into_iter().map(|session| session.name).collect::<Vec<_>>();
    if !options.dry_run {
        fleet_apply_renumber(&mut items, &live, runtime)?;
    }
    if options.json {
        return Ok((0, fleet_json_renumber(state, options, &items)?));
    }
    Ok((0, fleet_render_renumber(state, options, &items)))
}

fn fleet_renumber_plan(entries: &[NativeFleetEntry], include_99: bool, only_99: bool) -> Vec<FleetRenumberItem> {
    if only_99 {
        return fleet_renumber_only_99_plan(entries);
    }
    let mut candidates = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, include_99))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, (_, stem, entry))| fleet_renumber_item(entry, &format!("{:02}-{stem}", index + 1)))
        .collect()
}

fn fleet_renumber_only_99_plan(entries: &[NativeFleetEntry]) -> Vec<FleetRenumberItem> {
    let mut used = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, true))
        .filter_map(|(number, _, entry)| (number != 99).then_some(entry.session.name.clone()))
        .filter_map(|name| name.split_once('-').and_then(|(prefix, _)| prefix.parse::<u32>().ok()))
        .collect::<BTreeSet<_>>();
    let mut candidates = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, true))
        .filter(|(number, stem, _)| *number == 99 && stem != "overview")
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.1.cmp(&right.1));
    candidates
        .into_iter()
        .filter_map(|(_, stem, entry)| {
            let next = (1..=99).find(|number| !used.contains(number))?;
            used.insert(next);
            Some(fleet_renumber_item(entry, &format!("{next:02}-{stem}")))
        })
        .collect()
}

fn fleet_renumber_item(entry: &NativeFleetEntry, new_name: &str) -> FleetRenumberItem {
    let new_file = format!("{new_name}.json");
    FleetRenumberItem {
        old_name: entry.session.name.clone(),
        new_name: new_name.to_owned(),
        old_file: entry.file.clone(),
        new_file,
        path: entry.path.clone(),
        changed: entry.session.name != new_name,
        tmux: None,
        tmux_error: None,
    }
}

fn fleet_renumber_candidate(entry: &NativeFleetEntry, include_99: bool) -> Option<(u32, String, &NativeFleetEntry)> {
    if !fleet_entry_is_session(entry) {
        return None;
    }
    let (prefix, stem) = entry.session.name.split_once('-')?;
    if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = prefix.parse::<u32>().ok()?;
    if number == 99 && !include_99 {
        return None;
    }
    Some((number, stem.to_owned(), entry))
}

fn fleet_apply_renumber(items: &mut [FleetRenumberItem], live: &[String], runtime: &mut impl FleetRuntime) -> Result<(), String> {
    for item in items.iter_mut().filter(|item| item.changed) {
        fleet_write_renumbered_config(item)?;
        let stem = fleet_session_stem(&item.old_name);
        let running = live
            .iter()
            .find(|name| name.as_str() == item.old_name)
            .or_else(|| live.iter().find(|name| fleet_session_stem(name) == stem));
        if let Some(running) = running.filter(|running| running.as_str() != item.new_name) {
            match runtime.fleet_run_command("tmux", &["rename-session".to_owned(), "-t".to_owned(), running.clone(), item.new_name.clone()]) {
                Ok(_) => item.tmux = Some(running.clone()),
                Err(error) => item.tmux_error = Some(format!("{running}: {}", error.trim())),
            }
        }
    }
    Ok(())
}

fn fleet_write_renumbered_config(item: &FleetRenumberItem) -> Result<(), String> {
    let text = std::fs::read_to_string(&item.path).map_err(|error| format!("fleet renumber: read {}: {error}", item.path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("fleet renumber: parse {}: {error}", item.path.display()))?;
    value["name"] = serde_json::json!(item.new_name);
    let body = serde_json::to_string_pretty(&value).map_err(|error| format!("fleet renumber: render {}: {error}", item.new_name))? + "\n";
    let dir = item.path.parent().ok_or_else(|| format!("fleet renumber: no parent for {}", item.path.display()))?;
    let target = dir.join(&item.new_file);
    let tmp = dir.join(format!(".tmp-{}", item.new_file));
    std::fs::write(&tmp, body).map_err(|error| format!("fleet renumber: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|error| format!("fleet renumber: rename {} -> {}: {error}", tmp.display(), target.display()))?;
    if target != item.path && item.path.exists() {
        std::fs::remove_file(&item.path).map_err(|error| format!("fleet renumber: remove {}: {error}", item.path.display()))?;
    }
    Ok(())
}

fn fleet_render_renumber(state: &FleetState, options: &FleetOptions, items: &[FleetRenumberItem]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fleet renumber plan node: {}", state.config.node);
    let _ = writeln!(out, "  dry-run: {} · include-99: {} · only-99: {} · configs: {}", options.dry_run, options.include_99, options.only_99, items.len());
    if items.is_empty() {
        out.push_str("  ok: no numbered fleet configs\n");
        return out;
    }
    for item in items {
        if item.changed {
            let verb = if options.dry_run { "would rename" } else { "renamed" };
            let _ = write!(out, "  - {verb} {} -> {}", item.old_file, item.new_file);
            if let Some(tmux) = &item.tmux {
                let _ = write!(out, " (tmux: {tmux} -> {})", item.new_name);
            }
            if let Some(error) = &item.tmux_error {
                let _ = write!(out, " (tmux rename failed: {error})");
            }
            out.push('\n');
        } else {
            let _ = writeln!(out, "  - {} (unchanged)", item.old_file);
        }
    }
    out
}

fn fleet_json_renumber(state: &FleetState, options: &FleetOptions, items: &[FleetRenumberItem]) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "action": "renumber",
        "dryRun": options.dry_run,
        "include99": options.include_99,
        "only99": options.only_99,
        "configCount": items.len(),
        "configs": items.iter().map(fleet_json_renumber_item).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_json_renumber_item(item: &FleetRenumberItem) -> serde_json::Value {
    serde_json::json!({
        "oldName": item.old_name,
        "newName": item.new_name,
        "oldFile": item.old_file,
        "newFile": item.new_file,
        "changed": item.changed,
        "tmux": item.tmux,
        "tmuxError": item.tmux_error,
    })
}

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

// Registry resolution mirrors locate's hash-slot rules: `NN-` prefixes and `-oracle` suffixes are
// stripped on both sides, and window names count (a member can live as a window of a shared session).

fn fleet_run_group_post_wake_hooks(resolved: &[(&str, &FleetSessionSummary)]) {
    let hooks = wake_config_post_wake_hooks();
    if hooks.is_empty() {
        return;
    }
    for (handle, session) in resolved {
        let window = fleet_member_hook_window(handle, session);
        wake_run_post_wake_hooks(handle, &session.name, &window, &hooks);
    }
}

fn fleet_member_hook_window(handle: &str, session: &FleetSessionSummary) -> String {
    let wanted = locate_normalized_names(handle);
    session
        .windows
        .iter()
        .find(|window| locate_normalized_names(&window.name).iter().any(|name| wanted.contains(name)))
        .or_else(|| session.windows.first())
        .map_or_else(|| session.name.clone(), |window| window.name.clone())
}

fn fleet_member_session<'a>(handle: &str, sessions: &'a [FleetSessionSummary]) -> Option<&'a FleetSessionSummary> {
    let wanted = locate_normalized_names(handle);
    sessions.iter().find(|session| {
        locate_normalized_names(&session.name).iter().any(|name| wanted.contains(name))
            || session
                .windows
                .iter()
                .any(|window| locate_normalized_names(&window.name).iter().any(|name| wanted.contains(name)))
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
    let live = runtime.fleet_list_all().into_iter().map(|session| session.name).collect::<BTreeSet<_>>();
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

fn fleet_registry_upsert_session(
    session: &str,
    windows: &[FleetWindowSummary],
    created_by: &str,
) -> Result<FleetRegistryWrite, String> {
    fleet_registry_upsert_session_for_env(&current_xdg_env(), session, windows, created_by)
}

fn fleet_registry_upsert_session_for_env(
    env: &MawXdgEnv,
    session: &str,
    windows: &[FleetWindowSummary],
    created_by: &str,
) -> Result<FleetRegistryWrite, String> {
    fleet_validate_session_name(session)?;
    let dir = env.home_dir().join(".maw").join("fleet");
    std::fs::create_dir_all(&dir).map_err(|error| format!("fleet registry: create {}: {error}", dir.display()))?;

    let mut windows_by_repo: BTreeSet<String> = BTreeSet::new();
    for window in windows {
        windows_by_repo.insert(fleet_repo_canonical_key(&window.repo));
    }
    let target_stem = fleet_session_stem(session);
    // Duplicate guard (#299): an entry that already owns this exact session
    // name always wins — a session revived from the registry by `maw wake`
    // (#312) must update its own file, never merge into a same-stem sibling.
    // Only when no exact entry exists does the write fold into a same-stem
    // entry whose windows overlap on canonical repo path. Loading is
    // best-effort (non-strict): a corrupt unrelated registry file must not
    // fail wake/fleet-add registration.
    let entries = fleet_load_entries_for_env(env);
    let path = entries
        .iter()
        .find(|entry| fleet_entry_is_session(entry) && entry.session.name == session)
        .or_else(|| {
            entries.iter().find(|entry| {
                fleet_entry_is_session(entry)
                    && fleet_session_stem(&entry.session.name) == target_stem
                    && entry
                        .session
                        .windows
                        .iter()
                        .any(|window| windows_by_repo.contains(&fleet_repo_canonical_key(&window.repo)))
            })
        })
        .map_or_else(|| dir.join(format!("{session}.json")), |entry| entry.path.clone());

    let (created, mut value) = fleet_registry_read_value(&path)?;
    {
        let object = fleet_registry_object(&mut value);
        object.insert("name".to_owned(), serde_json::json!(session));
        object
            .entry("created_at".to_owned())
            .or_insert_with(|| serde_json::json!(fleet_registry_now_iso()));
        object.insert("created_by".to_owned(), serde_json::json!(created_by));
        object.insert("auto_registered".to_owned(), serde_json::json!(true));
        let merged = fleet_registry_merge_windows(object.get("windows"), windows);
        object.insert("windows".to_owned(), serde_json::json!(merged));
    }
    let body = serde_json::to_string_pretty(&value).map_err(|error| format!("fleet registry: render json: {error}"))? + "\n";
    std::fs::write(&path, body).map_err(|error| format!("fleet registry: write {}: {error}", path.display()))?;
    let window_count = value
        .get("windows")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    Ok(FleetRegistryWrite { path, created, window_count })
}

fn fleet_registry_read_value(path: &std::path::Path) -> Result<(bool, serde_json::Value), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok((
            false,
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((true, serde_json::json!({}))),
        Err(error) => Err(format!("fleet registry: read {}: {error}", path.display())),
    }
}

fn fleet_session_stem(value: &str) -> &str {
    value
        .split_once('-')
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(value, |(_, stem)| stem)
}

fn fleet_repo_canonical_key(repo: &str) -> String {
    // Canonicalize when the repo is cloned (resolves symlinked checkouts);
    // otherwise fall back to the ghq path so `acme/x` and `github.com/acme/x`
    // still hash to the same key.
    native_fleet_repo_path(repo).map_or_else(
        || repo.to_owned(),
        |path| {
            let path = path.canonicalize().unwrap_or(path);
            path.to_string_lossy().to_string()
        },
    )
}

fn fleet_fix_duplicate_windows(entries: &[NativeFleetEntry], live: &[TmuxSession]) -> Result<Vec<FleetWindowRepair>, String> {
    let mut repairs = Vec::new();
    for entry in entries.iter().filter(|entry| fleet_entry_is_session(entry)) {
        let text = std::fs::read_to_string(&entry.path).map_err(|error| format!("fleet doctor: read {}: {error}", entry.path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("fleet doctor: parse {}: {error}", entry.path.display()))?;
        let Some(windows) = value.get("windows").and_then(serde_json::Value::as_array) else { continue };
        let mergeable = fleet_windows_by_repo(entry)
            .into_iter()
            .filter(|(_, windows)| {
                windows.len() > 1
                    && fleet_windows_share_alias(windows)
                    && fleet_distinct_live_window_ids(entry, windows, live).len() <= 1
            })
            .map(|(repo, _)| repo)
            .collect::<BTreeSet<_>>();
        let (deduped, removed) = fleet_dedupe_window_values(windows, &mergeable);
        if removed == 0 { continue; }
        fleet_registry_object(&mut value).insert("windows".to_owned(), serde_json::json!(deduped));
        fleet_write_json_atomic(&entry.path, &value)?;
        repairs.push(FleetWindowRepair { session: entry.session.name.clone(), path: entry.path.clone(), removed });
    }
    Ok(repairs)
}

fn fleet_dedupe_window_values(windows: &[serde_json::Value], mergeable: &BTreeSet<String>) -> (Vec<serde_json::Value>, usize) {
    let mut by_repo = BTreeMap::<String, Vec<usize>>::new();
    for (index, window) in windows.iter().enumerate() {
        if let Some(repo) = window.get("repo").and_then(serde_json::Value::as_str).filter(|repo| !repo.trim().is_empty()) {
            by_repo.entry(fleet_repo_canonical_key(repo)).or_default().push(index);
        }
    }
    let mut remove = BTreeSet::new();
    for indices in by_repo.iter().filter(|(repo, indices)| mergeable.contains(*repo) && indices.len() > 1).map(|(_, indices)| indices) {
        // Prefer descriptive typed metadata; array order breaks ties because
        // auto-registration appends newer observations after older ones.
        let kept = indices
            .iter()
            .rev()
            .find(|&&index| windows[index].get("kind").and_then(serde_json::Value::as_str).and_then(native_repo_kind_from_role).is_some())
            .copied()
            .unwrap_or(indices[indices.len() - 1]);
        remove.extend(indices.iter().copied().filter(|index| *index != kept));
    }
    let deduped = windows.iter().enumerate().filter(|(index, _)| !remove.contains(index)).map(|(_, window)| window.clone()).collect();
    (deduped, remove.len())
}

fn fleet_write_json_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp-{}-{seq}", std::process::id()));
    let body = serde_json::to_string_pretty(value).map_err(|error| format!("fleet doctor: render json: {error}"))? + "\n";
    std::fs::write(&tmp, body).map_err(|error| format!("fleet doctor: write {}: {error}", tmp.display()))?;
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Err(error) = std::fs::set_permissions(&tmp, metadata.permissions()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("fleet doctor: set permissions {}: {error}", tmp.display()));
        }
    }
    std::fs::rename(&tmp, path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp);
        format!("fleet doctor: replace {}: {error}", path.display())
    })
}

fn fleet_registry_object(value: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    if !value.is_object() {
        *value = serde_json::json!({});
    }
    value.as_object_mut().expect("object assigned above")
}

fn fleet_registry_merge_windows(
    existing: Option<&serde_json::Value>,
    updates: &[FleetWindowSummary],
) -> Vec<serde_json::Value> {
    let mut windows = Vec::<FleetWindowSummary>::new();
    if let Some(existing) = existing.and_then(serde_json::Value::as_array) {
        for item in existing {
            let Some(name) = item.get("name").and_then(serde_json::Value::as_str).filter(|value| !value.trim().is_empty()) else {
                continue;
            };
            let repo = item.get("repo").and_then(serde_json::Value::as_str).unwrap_or_default();
            let kind = item.get("kind").and_then(serde_json::Value::as_str).and_then(native_repo_kind_from_role);
            windows.push(FleetWindowSummary {
                name: name.to_owned(),
                repo: fleet_repo_storage_slug(repo),
                kind,
            });
        }
    }
    let existing_repo_counts = windows.iter().fold(BTreeMap::<String, usize>::new(), |mut counts, window| {
        *counts.entry(fleet_repo_canonical_key(&window.repo)).or_default() += 1;
        counts
    });
    let update_repo_counts = updates
        .iter()
        .filter(|window| !window.name.trim().is_empty())
        .fold(BTreeMap::<String, usize>::new(), |mut counts, window| {
            *counts.entry(fleet_repo_canonical_key(&window.repo)).or_default() += 1;
            counts
        });
    for update in updates.iter().filter(|window| !window.name.trim().is_empty()) {
        let mut update = update.clone();
        update.repo = fleet_repo_storage_slug(&update.repo);
        let repo_key = fleet_repo_canonical_key(&update.repo);
        // A single live tmux window and a single registry window for the
        // same canonical repo are two names for one physical window, not a
        // reason to append a second entry (#457).
        let lone_repo_alias =
            existing_repo_counts.get(&repo_key) == Some(&1) && update_repo_counts.get(&repo_key) == Some(&1);
        if let Some(existing) = windows.iter_mut().find(|window| window.name == update.name) {
            existing.repo.clone_from(&update.repo);
            if update.kind.is_some() {
                existing.kind = update.kind;
            }
        } else if lone_repo_alias {
            let existing = windows
                .iter_mut()
                .find(|window| fleet_repo_canonical_key(&window.repo) == repo_key)
                .expect("single canonical repo counted above");
            *existing = update;
        } else {
            windows.push(update);
        }
    }
    windows
        .into_iter()
        .map(|window| fleet_json_window(&window))
        .collect()
}

fn fleet_repo_storage_slug(repo: &str) -> String {
    repo.trim().strip_prefix("github.com/").unwrap_or(repo.trim()).to_owned()
}

fn fleet_registry_windows_from_tmux(
    windows: &[maw_tmux::TmuxWindow],
    repos_root: Option<&std::path::Path>,
) -> Vec<FleetWindowSummary> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for window in windows {
        let Some(cwd) = window.cwd.as_deref().and_then(|cwd| fleet_repo_slug_from_path(std::path::Path::new(cwd), repos_root)) else {
            continue;
        };
        let name = if window.name.is_empty() { "main".to_owned() } else { window.name.clone() };
        if seen.insert(name.clone()) {
            let kind = Some(fleet_kind_from_window_name(&name));
            result.push(FleetWindowSummary { name, repo: cwd, kind });
        }
    }
    result
}

fn fleet_kind_from_window_name(name: &str) -> NativeRepoKind {
    if name.trim().ends_with("-oracle") { NativeRepoKind::Oracle } else { NativeRepoKind::Project }
}

fn native_repo_kind_label(kind: NativeRepoKind) -> &'static str {
    match kind {
        NativeRepoKind::Oracle => "oracle",
        NativeRepoKind::Project => "project",
    }
}

fn fleet_repo_slug_from_path(path: &std::path::Path, repos_root: Option<&std::path::Path>) -> Option<String> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(root) = repos_root {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if let Ok(rel) = path.strip_prefix(root) {
            return fleet_repo_slug_from_components(rel.components());
        }
    }
    let mut saw_github = false;
    let mut parts = Vec::new();
    for component in path.components() {
        let value = component.as_os_str().to_string_lossy();
        if saw_github {
            parts.push(value.to_string());
            if parts.len() == 2 {
                return Some(format!("github.com/{}/{}", parts[0], parts[1]));
            }
        } else if value == "github.com" {
            saw_github = true;
        }
    }
    None
}

fn fleet_repo_slug_from_components(mut components: std::path::Components<'_>) -> Option<String> {
    let org = components.next()?.as_os_str().to_string_lossy();
    let repo = components.next()?.as_os_str().to_string_lossy();
    Some(format!("github.com/{org}/{repo}"))
}

fn fleet_registry_now_iso() -> String {
    if let Ok(value) = std::env::var("MAW_RS_FLEET_REGISTRY_NOW") {
        return value;
    }
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    let (year, month, day, hour, minute, sec) = epoch_secs_to_ymd_hms(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{sec:02}.000Z")
}

fn fleet_validate_session_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        Err("fleet: invalid session".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod fleet_tests {
    use super::*;

    fn fleet_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[derive(Default)]
    struct FleetMockTmux {
        sessions: String,
    }

    impl maw_tmux::TmuxRunner for FleetMockTmux {
        fn run(&mut self, subcommand: &str, _args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            if subcommand == "list-sessions" {
                Ok(self.sessions.clone())
            } else {
                Err(maw_tmux::TmuxError::new(format!("unexpected tmux command {subcommand}")))
            }
        }
    }

    #[derive(Default)]
    struct FleetFakeRuntime {
        ghq_root: Option<String>,
        commands: Vec<(String, Vec<String>)>,
        sessions: Vec<TmuxSession>,
    }

    impl FleetRuntime for FleetFakeRuntime {
        fn fleet_run_command(&mut self, program: &str, args: &[String]) -> Result<String, String> {
            self.commands.push((program.to_owned(), args.to_vec()));
            if program == "ghq" && args == ["root".to_owned()] {
                self.ghq_root.clone().ok_or_else(|| "fake ghq root failed".to_owned())
            } else if program == "tmux" && args.first().is_some_and(|arg| arg == "rename-session") {
                Ok(String::new())
            } else {
                Err(format!("unexpected command {program} {args:?}"))
            }
        }

        fn fleet_list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
        }
    }

    fn fleet_live_session(name: &str, windows: &[&str]) -> TmuxSession {
        TmuxSession {
            name: name.to_owned(),
            windows: windows
                .iter()
                .enumerate()
                .map(|(index, window)| maw_tmux::TmuxWindow {
                    index: u32::try_from(index).expect("window index"),
                    name: (*window).to_owned(),
                    active: index == 0,
                    cwd: None,
                })
                .collect(),
        }
    }

    fn fleet_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-fleet-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn fleet_fixture() -> std::path::PathBuf {
        let root = fleet_temp_root("fixture");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::create_dir_all(root.join("ghq/github.com/acme/maw-rs")).expect("repo");
        std::fs::write(root.join("config/maw.config.json"), fleet_config_json()).expect("config");
        std::fs::write(root.join("config/fleet/03-alpha.json"), fleet_session_json()).expect("session");
        std::fs::write(root.join("config/fleet/22-dormant.disabled"), "{}\n").expect("disabled");
        root
    }

    fn fleet_config_json() -> &'static str {
        r#"{"node":"alpha","namedPeers":[{"name":"beta","url":"http://127.0.0.1:4111"}],"agents":{"nova":"alpha:nova","wish":{"node":"beta"}}}"#
    }

    fn fleet_session_json() -> &'static str {
        r#"{"name":"03-alpha","windows":[{"name":"maw","repo":"acme/maw-rs"},{"name":"ghost","repo":"acme/missing"}]}"#
    }

    fn fleet_with_fixture<F>(test: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let root = fleet_fixture();
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("TMUX");
        test(&root);
    }

    #[test]
    fn fleet_parse_flags_and_guard_option_injection() {
        let parsed = fleet_parse_args(&fleet_strings(&["wake", "--json", "--dry-run", "--all", "--kill", "--resume"])).expect("parse");
        assert_eq!(parsed.command, FleetCommand::Wake);
        assert!(parsed.json && parsed.dry_run && parsed.all && parsed.kill && parsed.resume);
        let renumber = fleet_parse_args(&fleet_strings(&["renumber", "--include-99", "--dry-run"])).expect("renumber parse");
        assert_eq!(renumber.command, FleetCommand::Renumber);
        assert!(renumber.include_99 && renumber.dry_run);
        let only_99 = fleet_parse_args(&fleet_strings(&["renumber", "--only-99", "--dry-run"])).expect("only 99 parse");
        assert!(only_99.only_99 && only_99.dry_run);
        assert!(fleet_parse_args(&fleet_strings(&["--", "wake"])).expect_err("separator guard").contains("unknown argument"));
        assert!(fleet_parse_args(&fleet_strings(&["-oProxyCommand=bad"])).expect_err("leading dash").contains("unknown argument"));
        let scoped = fleet_parse_args(&fleet_strings(&["wake", "3e"])).expect("group target");
        assert_eq!((scoped.command, scoped.target.as_deref()), (FleetCommand::Wake, Some("3e")));
        let groups = fleet_parse_args(&fleet_strings(&["ls", "--squads", "3e,drift"])).expect("squad filter");
        assert_eq!(groups.squads, vec!["3e".to_owned(), "drift".to_owned()]);
        let alias = fleet_parse_args(&fleet_strings(&["wake-all"])).expect("alias");
        assert!(alias.all, "wake-all implies --all");
        let bare = fleet_parse_args(&fleet_strings(&["wake"])).expect_err("bare wake");
        assert!(bare.contains("specify a squad, or --all to wake every registered session on this node"), "{bare}");
        let sleep = fleet_parse_args(&fleet_strings(&["sleep", "--json"])).expect_err("bare sleep");
        assert!(sleep.contains("fleet sleep: specify a squad"), "{sleep}");
    }

    #[test]
    fn fleet_census_is_hermetic_and_golden() {
        fleet_with_fixture(|_| {
            let output = run_fleet_command(&fleet_strings(&["ls"]));
            assert_eq!(output.code, 0);
            assert!(output.stderr.is_empty());
            assert_eq!(
                output.stdout,
                "\u{1b}[36mfleet\u{1b}[0m node alpha\n  sessions: 1 (2 windows, 1 disabled)\n  peers: 1\n  agents: 2\n  session list:\n  - 03-alpha (2 windows)\n  squads: 0\n"
            );
        });
    }

    #[test]
    fn fleet_census_lists_squads_and_filters_membership() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let unfiltered = run_fleet_command(&fleet_strings(&["ls", "--json"]));
            assert_eq!(unfiltered.code, 0, "{}", unfiltered.stderr);
            let raw: serde_json::Value = serde_json::from_str(&unfiltered.stdout).expect("json");
            assert_eq!(raw["squads"].as_array().expect("squads").len(), 1);
            assert_eq!(raw["squads"][0]["name"], serde_json::json!("3e"));
            assert_eq!(raw["sessionCount"], 1); // rosters are excluded from sessions
            assert_eq!(raw["sessions"][0]["name"], serde_json::json!("03-alpha"));

            let filtered = run_fleet_command(&fleet_strings(&["ls", "--squads", "3e", "--json"]));
            assert_eq!(filtered.code, 0, "{}", filtered.stderr);
            let filtered_json: serde_json::Value = serde_json::from_str(&filtered.stdout).expect("json");
            assert_eq!(filtered_json["squads"][0]["name"], serde_json::json!("3e"));
            assert_eq!(filtered_json["sessionCount"], 1);
            assert_eq!(filtered_json["sessions"][0]["name"], serde_json::json!("03-alpha"));
            let muted = run_fleet_command(&fleet_strings(&["ls", "--squads", "nope", "--json"]));
            assert_eq!(muted.code, 0, "{}", muted.stderr);
            let muted_json: serde_json::Value = serde_json::from_str(&muted.stdout).expect("json");
            assert_eq!(muted_json["sessionCount"], 0);
            assert_eq!(muted_json["sessions"], serde_json::json!([]));
            assert_eq!(muted_json["squads"], serde_json::json!([]));
        });
    }

    #[test]
    fn fleet_doctor_json_reports_seeded_missing_repo_only() {
        fleet_with_fixture(|root| {
            let output = run_fleet_command(&fleet_strings(&["doctor", "--json"]));
            assert_eq!(output.code, 1);
            assert!(output.stderr.is_empty());
            assert!(output.stdout.contains("\"node\": \"alpha\""));
            assert!(output.stdout.contains("\"code\": \"missing-repo\""));
            assert!(output.stdout.contains(&root.join("ghq/github.com/acme/missing").display().to_string()));
        });
    }

    #[test]
    fn fleet_doctor_detects_and_fixes_aliases_for_the_same_live_window() {
        fleet_with_fixture(|root| {
            let fleet_dir = root.join("config/fleet");
            for (file, session) in [("04-agora.json", "04-agora"), ("05-bud.json", "05-bud")] {
                std::fs::write(
                    fleet_dir.join(file),
                    format!(
                        r#"{{"name":"{session}","windows":[{{"name":"{session}-oracle","repo":"github.com/acme/maw-rs","legacy":true}},{{"name":"{session}","repo":"acme/maw-rs","kind":"project","preferred":true}}]}}"#,
                    ),
                )
                .expect("duplicate registry");
            }
            let mut runtime = FleetFakeRuntime {
                sessions: vec![
                    fleet_live_session("04-agora", &["04-agora"]),
                    fleet_live_session("05-bud", &["05-bud"]),
                ],
                ..Default::default()
            };

            let (_, dry_run) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("dry run");
            let dry_json: serde_json::Value = serde_json::from_str(&dry_run).expect("dry json");
            assert_eq!(
                dry_json["findings"]
                    .as_array()
                    .expect("findings")
                    .iter()
                    .filter(|finding| finding["code"] == "duplicate-window-repo")
                    .count(),
                2
            );
            let unchanged: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(fleet_dir.join("04-agora.json")).expect("dry-run registry"),
            )
            .expect("dry-run json");
            assert_eq!(unchanged["windows"].as_array().expect("windows").len(), 2);

            let (_, fixed) = fleet_run_with(&fleet_strings(&["doctor", "--fix", "--json"]), &mut runtime).expect("fix");
            let fixed_json: serde_json::Value = serde_json::from_str(&fixed).expect("fixed json");
            assert_eq!(fixed_json["repairs"].as_array().expect("repairs").len(), 2);
            for file in ["04-agora.json", "05-bud.json"] {
                let registry: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(fleet_dir.join(file)).expect("fixed registry"),
                )
                .expect("fixed registry json");
                let windows = registry["windows"].as_array().expect("windows");
                assert_eq!(windows.len(), 1);
                assert_eq!(windows[0]["kind"], "project");
                assert_eq!(windows[0]["preferred"], true);
                assert!(windows[0].get("legacy").is_none());
            }
        });
    }

    #[test]
    fn fleet_doctor_preserves_distinct_live_windows_sharing_one_repo() {
        fleet_with_fixture(|root| {
            std::fs::create_dir_all(root.join("ghq/github.com/acme/missing")).expect("seed missing repo");
            let path = root.join("config/fleet/41-team.json");
            std::fs::write(
                &path,
                r#"{"name":"41-team","windows":[{"name":"coder-one","repo":"acme/maw-rs"},{"name":"coder-two","repo":"github.com/acme/maw-rs"},{"name":"coder-three","repo":"acme/maw-rs"}]}"#,
            )
            .expect("team registry");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![fleet_live_session("41-team", &["coder-one", "coder-two", "coder-three"])],
                ..Default::default()
            };

            let (code, dry_run) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("dry run");
            let dry_json: serde_json::Value = serde_json::from_str(&dry_run).expect("dry json");
            assert_eq!(code, 0, "{dry_run}");
            assert_eq!(dry_json["findings"], serde_json::json!([]));

            let (_, fixed) = fleet_run_with(&fleet_strings(&["doctor", "--fix", "--json"]), &mut runtime).expect("fix");
            let fixed_json: serde_json::Value = serde_json::from_str(&fixed).expect("fixed json");
            assert_eq!(fixed_json["repairs"], serde_json::json!([]));
            let registry: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
            assert_eq!(registry["windows"].as_array().expect("windows").len(), 3);
        });
    }

    #[test]
    fn fleet_doctor_uses_ghq_root_once_for_host_prefixed_repo_slugs() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = fleet_temp_root("doctor-ghq-root");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");
        std::fs::write(
            root.join("config/fleet/188-maw-rs.json"),
            r#"{"name":"188-maw-rs","windows":[{"name":"maw-rs-oracle","repo":"github.com/Soul-Brews-Studio/missing"}]}"#,
        )
        .expect("fleet json");
        std::env::set_var("HOME", root.join("wrong-home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("GHQ_ROOT");
        let mut runtime = FleetFakeRuntime {
            ghq_root: Some(root.join("real-ghq").display().to_string()),
            ..Default::default()
        };

        let (code, stdout) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("doctor");

        assert_eq!(code, 1);
        assert!(runtime.commands.iter().any(|(program, args)| program == "ghq" && args == &["root".to_owned()]));
        let single = root.join("real-ghq/github.com/Soul-Brews-Studio/missing").display().to_string();
        assert!(stdout.contains(&single), "{stdout}");
        assert!(!stdout.contains("github.com/github.com"), "{stdout}");
        assert!(!stdout.contains("wrong-home"), "{stdout}");
    }

    #[test]
    fn fleet_add_registers_live_session_windows_from_fake_tmux() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _now = EnvVarRestore::capture("MAW_RS_FLEET_REGISTRY_NOW");
        let root = fleet_temp_root("add");
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("GHQ_ROOT");
        std::env::set_var("MAW_RS_FLEET_REGISTRY_NOW", "2026-07-03T01:02:03.000Z");
        let repo = root.join("real-ghq/github.com/Soul-Brews-Studio/maw-rs");
        let mut runtime = FleetFakeRuntime {
            ghq_root: Some(root.join("real-ghq").display().to_string()),
            sessions: vec![TmuxSession {
                name: "188-maw-rs".to_owned(),
                windows: vec![
                    maw_tmux::TmuxWindow {
                        index: 0,
                        name: "maw-rs-oracle".to_owned(),
                        active: true,
                        cwd: Some(repo.join("agents/fleet-register").display().to_string()),
                    },
                    maw_tmux::TmuxWindow {
                        index: 1,
                        name: "scratch".to_owned(),
                        active: false,
                        cwd: Some("/tmp/scratch".to_owned()),
                    },
                ],
            }],
            ..Default::default()
        };

        let (code, stdout) = fleet_run_with(&fleet_strings(&["add", "188-maw-rs"]), &mut runtime).expect("add");

        assert_eq!(code, 0);
        assert!(stdout.contains("fleet add 188-maw-rs: created"), "{stdout}");
        let path = root.join("home/.maw/fleet/188-maw-rs.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
        assert_eq!(json["name"], "188-maw-rs");
        assert_eq!(json["created_at"], "2026-07-03T01:02:03.000Z");
        assert_eq!(json["created_by"], "maw fleet add");
        assert_eq!(json["auto_registered"], true);
        assert_eq!(json["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(json["windows"][0]["name"], "maw-rs-oracle");
        assert_eq!(json["windows"][0]["repo"], "Soul-Brews-Studio/maw-rs");
        assert_eq!(json["windows"][0]["kind"], "oracle");
    }

    const FLEET_SQUADRON_JSON: &str =
        r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"},{"handle":"drift"}]}"#;

    #[test]
    fn fleet_renumber_dry_run_skips_99_by_default() {
        fleet_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/99-bud.json"),
                r#"{"name":"99-bud","windows":[],"mystery":true}"#,
            )
            .expect("bud");
            let mut runtime = FleetFakeRuntime::default();
            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--dry-run", "--json"]), &mut runtime).expect("renumber");
            assert_eq!(code, 0);
            let value: serde_json::Value = serde_json::from_str(&stdout).expect("json");
            assert_eq!(value["dryRun"], true);
            assert_eq!(value["include99"], false);
            assert_eq!(value["configs"].as_array().expect("configs").len(), 1);
            assert_eq!(value["configs"][0]["oldName"], "03-alpha");
            assert_eq!(value["configs"][0]["newName"], "01-alpha");
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(root.join("config/fleet/99-bud.json").exists());
        });
    }

    #[test]
    fn fleet_renumber_include_99_rewrites_configs_and_renames_tmux() {
        fleet_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/99-bud.json"),
                r#"{"name":"99-bud","windows":[],"mystery":true}"#,
            )
            .expect("bud");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![
                    TmuxSession { name: "03-alpha".to_owned(), windows: Vec::new() },
                    TmuxSession { name: "99-bud".to_owned(), windows: Vec::new() },
                ],
                ..Default::default()
            };

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--include-99"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            assert!(stdout.contains("renamed 03-alpha.json -> 01-alpha.json"), "{stdout}");
            assert!(stdout.contains("renamed 99-bud.json -> 02-bud.json"), "{stdout}");
            assert!(!root.join("config/fleet/03-alpha.json").exists());
            assert!(!root.join("config/fleet/99-bud.json").exists());
            let alpha: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/01-alpha.json")).expect("alpha")).expect("alpha json");
            let bud: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/02-bud.json")).expect("bud")).expect("bud json");
            assert_eq!(alpha["name"], "01-alpha");
            assert_eq!(bud["name"], "02-bud");
            assert_eq!(bud["mystery"], true);
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "03-alpha", "01-alpha"])));
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "99-bud", "02-bud"])));
        });
    }


    #[test]
    fn fleet_renumber_only_99_dry_run_fills_gaps_without_touching_existing() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-root.json"), r#"{"name":"01-root","windows":[]}"#).expect("root");
            std::fs::write(root.join("config/fleet/99-bud.json"), r#"{"name":"99-bud","windows":[],"mystery":true}"#).expect("bud");
            std::fs::write(root.join("config/fleet/99-cat.json"), r#"{"name":"99-cat","windows":[]}"#).expect("cat");
            std::fs::write(root.join("config/fleet/99-overview.json"), r#"{"name":"99-overview","windows":[]}"#).expect("overview");
            let mut runtime = FleetFakeRuntime::default();

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--only-99", "--dry-run", "--json"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            let value: serde_json::Value = serde_json::from_str(&stdout).expect("json");
            assert_eq!(value["only99"], true);
            assert_eq!(value["configs"].as_array().expect("configs").len(), 2);
            assert_eq!(value["configs"][0]["oldName"], "99-bud");
            assert_eq!(value["configs"][0]["newName"], "02-bud");
            assert_eq!(value["configs"][1]["newName"], "04-cat");
            assert!(root.join("config/fleet/01-root.json").exists());
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(root.join("config/fleet/99-bud.json").exists());
            assert!(runtime.commands.is_empty());
        });
    }

    #[test]
    fn fleet_renumber_only_99_rewrites_only_99_and_renames_tmux() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-root.json"), r#"{"name":"01-root","windows":[]}"#).expect("root");
            std::fs::write(root.join("config/fleet/99-bud.json"), r#"{"name":"99-bud","windows":[],"mystery":true}"#).expect("bud");
            let mut runtime = FleetFakeRuntime { sessions: vec![TmuxSession { name: "99-bud".to_owned(), windows: Vec::new() }], ..Default::default() };

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--only-99"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            assert!(stdout.contains("only-99: true"), "{stdout}");
            assert!(stdout.contains("renamed 99-bud.json -> 02-bud.json"), "{stdout}");
            assert!(root.join("config/fleet/01-root.json").exists());
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(!root.join("config/fleet/99-bud.json").exists());
            let bud: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/02-bud.json")).expect("bud")).expect("json");
            assert_eq!(bud["name"], "02-bud");
            assert_eq!(bud["mystery"], true);
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "99-bud", "02-bud"])));
        });
    }

    #[test]
    fn fleet_wake_bare_errors_and_all_sweep_excludes_roster_files() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let bare = run_fleet_command(&fleet_strings(&["wake"]));
            assert_eq!(bare.code, 1);
            assert!(bare.stderr.contains("specify a squad, or --all"), "{}", bare.stderr);
            let all = run_fleet_command(&fleet_strings(&["wake", "--all", "--json", "--dry-run"]));
            assert_eq!(all.code, 0, "{}", all.stderr);
            assert!(all.stdout.contains("\"action\": \"wake\"") && all.stdout.contains("\"sessionCount\": 1"), "{}", all.stdout);
            assert!(all.stdout.contains("03-alpha") && !all.stdout.contains("01-3e"), "{}", all.stdout);
            assert!(!all.stdout.contains("22-dormant"), "disabled entries stay skipped");
            let alias = run_fleet_command(&fleet_strings(&["wake-all", "--dry-run"]));
            assert_eq!(alias.code, 0, "{}", alias.stderr);
            assert!(alias.stdout.contains("  - 03-alpha") && !alias.stdout.contains("01-3e"), "{}", alias.stdout);
            let sleep = run_fleet_command(&fleet_strings(&["sleep", "--all", "--dry-run"]));
            assert_eq!(sleep.code, 0, "{}", sleep.stderr);
            assert!(sleep.stdout.contains("  - 03-alpha") && !sleep.stdout.contains("01-3e"), "{}", sleep.stdout);
        });
    }

    #[test]
    fn fleet_wake_group_scopes_plan_to_squadron_members() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let plan = run_fleet_command(&fleet_strings(&["wake", "3e", "--dry-run"]));
            assert_eq!(plan.code, 0, "{}", plan.stderr);
            assert!(plan.stdout.contains("squad: 3e · members: 2 · sessions: 1 · skipped: 1"), "{}", plan.stdout);
            assert!(plan.stdout.contains("  - alpha -> 03-alpha"), "{}", plan.stdout);
            assert!(plan.stdout.contains("  - drift skipped: no session"), "{}", plan.stdout);
            let json = run_fleet_command(&fleet_strings(&["sleep", "3e", "--json", "--dry-run"]));
            assert_eq!(json.code, 0, "{}", json.stderr);
            let value: serde_json::Value = serde_json::from_str(&json.stdout).expect("json");
            assert_eq!(value["action"], "sleep");
            assert_eq!(value["squad"], "3e");
            assert_eq!(value["dryRun"], true);
            assert_eq!(value["sessions"], serde_json::json!(["03-alpha"]));
            assert_eq!(value["members"][0]["handle"], "alpha");
            assert_eq!(value["skipped"][0], serde_json::json!({"handle": "drift", "reason": "no session"}));
        });
    }


    #[test]
    fn fleet_gather_dry_run_plans_live_and_asleep_members() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![TmuxSession { name: "03-alpha".to_owned(), windows: Vec::new() }],
                ..FleetFakeRuntime::default()
            };
            let (code, stdout) = fleet_run_with(&fleet_strings(&["gather", "3e", "--dry-run"]), &mut runtime).expect("gather");
            assert_eq!(code, 0);
            assert!(stdout.contains("fleet gather plan node: alpha"), "{stdout}");
            assert!(stdout.contains("  - alpha live: join 03-alpha:maw"), "{stdout}");
            assert!(stdout.contains("  - drift asleep: skipped (no auto-wake in v1)"), "{stdout}");
            assert!(stdout.contains("  - layout: main-vertical"), "{stdout}");
            let (code, stdout) = fleet_run_with(&fleet_strings(&["gather", "3e", "--scatter", "--dry-run"]), &mut runtime).expect("scatter");
            assert_eq!(code, 0);
            assert!(stdout.contains("fleet scatter plan node: alpha"), "{stdout}");
            assert!(stdout.contains("  - alpha live: break 03-alpha:maw"), "{stdout}");
            assert!(!stdout.contains("layout:"), "{stdout}");
        });
    }

    #[test]
    fn fleet_wake_group_runs_config_post_wake_hook_per_member() {
        fleet_with_fixture(|root| {
            let marker = root.join("fleet-ready.txt");
            let hook = format!(
                "printf '%s|%s|%s\\n' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" >> {}",
                wake_shell_quote(&marker.display().to_string())
            );
            std::fs::write(
                root.join("config/maw.config.json"),
                serde_json::to_string(&serde_json::json!({"node":"alpha","hooks":{"postWake":[hook]}})).expect("json"),
            )
            .expect("write config hook");
            std::fs::write(
                root.join("config/fleet/01-hooks.json"),
                r#"{"name":"01-hooks","squadName":"hooks","windows":[],"members":[{"handle":"maw"},{"handle":"ghost"}]}"#,
            )
            .expect("roster");

            let output = run_fleet_command(&fleet_strings(&["wake", "hooks"]));

            assert_eq!(output.code, 0, "{}", output.stderr);
            let lines = std::fs::read_to_string(&marker).expect("marker");
            assert_eq!(lines.lines().collect::<Vec<_>>(), vec!["maw|03-alpha|maw", "ghost|03-alpha|ghost"]);
        });
    }

    #[test]
    fn fleet_wake_group_errors_for_missing_or_empty_squadron() {
        fleet_with_fixture(|root| {
            let missing = run_fleet_command(&fleet_strings(&["wake", "nope"]));
            assert_eq!(missing.code, 1);
            assert!(missing.stderr.contains("fleet wake: no squad named nope"), "{}", missing.stderr);
            std::fs::write(
                root.join("config/fleet/02-empty.json"),
                r#"{"name":"02-empty","squadName":"empty","windows":[],"members":[]}"#,
            )
            .expect("roster");
            let empty = run_fleet_command(&fleet_strings(&["wake", "empty"]));
            assert_eq!(empty.code, 1);
            assert!(empty.stderr.contains("fleet wake: squad empty has no members"), "{}", empty.stderr);
            let both = run_fleet_command(&fleet_strings(&["wake", "empty", "--all"]));
            assert_eq!(both.code, 1);
            assert!(both.stderr.contains("pass a squad or --all, not both"), "{}", both.stderr);
        });
    }

    #[test]
    fn fleet_gc_dry_run_lists_only_nonlive_entries_with_all_repos_missing() {
        fleet_with_fixture(|root| {
            let ghost = root.join("config/fleet/04-ghost.json");
            std::fs::write(
                &ghost,
                r#"{"name":"04-ghost","windows":[{"name":"ghost","repo":"acme/ghost"}]}"#,
            )
            .expect("ghost");
            let mut runtime = FleetFakeRuntime::default();
            let state = fleet_load_state_with(&mut runtime).expect("state");
            let options = fleet_parse_args(&fleet_strings(&["gc", "--dry-run"])).expect("parse");
            let mut tmux = FleetMockTmux { sessions: String::new() };
            let (code, stdout) = fleet_run_gc(&state, &options, &mut tmux).expect("gc");

            assert_eq!(code, 0);
            assert!(stdout.contains("[dry-run] would disable"));
            assert!(stdout.contains("04-ghost.json"));
            assert!(!stdout.contains("03-alpha.json"));
            assert!(ghost.exists());
            assert!(!ghost.with_file_name("04-ghost.json.disabled").exists());
        });
    }

    #[test]
    fn fleet_session_consumers_ignore_squad_rosters() {
        fleet_with_fixture(|root| {
            let roster = root.join("config/fleet/squads/01-3e/squad.json");
            std::fs::create_dir_all(roster.parent().expect("roster parent")).expect("roster dir");
            std::fs::write(
                &roster,
                r#"{"name":"01-3e","squadName":"3e","windows":[{"name":"durable","repo":"acme/missing-squad"}],"members":[]}"#,
            )
            .expect("roster");
            let mut runtime = FleetFakeRuntime::default();
            let state = fleet_load_state_with(&mut runtime).expect("state");
            assert!(state.sessions.iter().all(|session| session.name != "01-3e"));
            assert!(fleet_gc_candidates(&state, &BTreeSet::new())
                .iter()
                .all(|candidate| candidate.path != roster));
        });
    }

    #[test]
    fn fleet_upsert_never_writes_a_session_snapshot_into_a_squad_folder() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = fleet_temp_root("upsert-squad-boundary");
        let roster = root.join("state/fleet/squads/01-3e/squad.json");
        std::fs::create_dir_all(roster.parent().expect("roster parent")).expect("roster dir");
        let roster_body = r#"{"name":"01-3e","squadName":"3e","unknown":"keep","windows":[{"name":"durable","repo":"acme/roster"}],"members":[]}"#;
        std::fs::write(&roster, roster_body).expect("roster");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        let windows = vec![FleetWindowSummary {
            name: "live".to_owned(),
            repo: "acme/live".to_owned(),
            kind: None,
        }];

        let written = fleet_registry_upsert_session_for_env(
            &current_xdg_env(),
            "01-3e",
            &windows,
            "maw fleet add",
        )
        .expect("upsert");

        assert_eq!(written.path, root.join("home/.maw/fleet/01-3e.json"));
        assert_eq!(std::fs::read_to_string(roster).expect("unchanged roster"), roster_body);
    }

    #[test]
    fn fleet_upsert_session_follows_stem_matches_and_repo_overlap_across_state_and_home_dirs() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-cross-dir");
        std::fs::create_dir_all(root.join("config/fleet")).expect("config fleet dir");
        std::fs::create_dir_all(root.join("state/fleet")).expect("state fleet dir");
        std::fs::write(root.join("config/fleet/63-homekeeper.json"), r#"{"name":"63-homekeeper","windows":[{"name":"main","repo":"github.com/acme/homekeeper-oracle","kind":"oracle"}]}"#)
            .expect("state fixture");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/acme/homekeeper-oracle".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "158-homekeeper", &windows, "maw fleet add").expect("upsert");

        assert_eq!(written.path, root.join("config/fleet/63-homekeeper.json"));
        let merged = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(merged["name"], "158-homekeeper");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["repo"], "acme/homekeeper-oracle");
    }

    #[test]
    fn fleet_upsert_uses_canonical_repo_overlap_to_merge_symlinked_paths() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-symlink-canonical");
        std::fs::create_dir_all(root.join("state/fleet")).expect("state fleet dir");
        let real = root.join("ghq/github.com/acme/homekeeper-oracle");
        let linked = root.join("ghq/github.com/acme/homelab");
        std::fs::create_dir_all(&real).expect("repo");
        #[cfg(unix)] {
            use std::os::unix::fs::symlink;
            symlink(&real, &linked).expect("symlink repo");
        }

        std::fs::write(
            root.join("state/fleet/63-homelab.json"),
            r#"{"name":"63-homelab","windows":[{"name":"main","repo":"github.com/acme/homekeeper-oracle","kind":"oracle"}] }"#,
        )
        .expect("state fixture");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/acme/homelab".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "158-homelab", &windows, "maw fleet add").expect("upsert");

        assert_eq!(written.path, root.join("state/fleet/63-homelab.json"));
        let merged = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(merged["name"], "158-homelab");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["repo"], "acme/homelab");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fleet_upsert_deduplicates_bud_and_wake_names_for_one_live_window() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-bud-wake-dedup");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");
        let path = root.join("config/fleet/10-oracle-dig-ui.json");
        let bud = BudContext {
            stem: "oracle-dig-ui".to_owned(),
            org: "Soul-Brews-Studio".to_owned(),
            parent: None,
            repo_name: "oracle-dig-ui-oracle".to_owned(),
            slug: "Soul-Brews-Studio/oracle-dig-ui-oracle".to_owned(),
            repo_path: root.join("ghq/github.com/Soul-Brews-Studio/oracle-dig-ui-oracle"),
        };
        let mut registered = serde_json::json!({
            "name": "10-oracle-dig-ui",
            "windows": [],
        });
        bud_fleet_ensure_window(&mut registered, &bud).expect("bud registration");
        std::fs::write(&path, serde_json::to_string_pretty(&registered).expect("bud json"))
            .expect("bud fleet file");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq"));

        let live_windows = vec![FleetWindowSummary {
            name: "oracle-dig-ui".to_owned(),
            repo: "github.com/Soul-Brews-Studio/oracle-dig-ui-oracle".to_owned(),
            kind: Some(NativeRepoKind::Project),
        }];
        fleet_registry_upsert_session_for_env(
            &current_xdg_env(),
            "10-oracle-dig-ui",
            &live_windows,
            "maw wake",
        )
        .expect("wake registration");

        let merged: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["name"], "oracle-dig-ui");
        assert_eq!(merged["windows"][0]["repo"], "Soul-Brews-Studio/oracle-dig-ui-oracle");
    }

    #[test]
    fn fleet_upsert_prefers_exact_name_entry_over_stem_sibling_for_revived_session() {
        // #312 revives session names from the registry; when that session
        // re-registers itself the upsert must update its own entry in place —
        // not get treated as a duplicate of an earlier-sorting same-stem
        // sibling (which would mint a second entry with the same name).
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-revive-exact");
        std::env::remove_var("MAW_HOME");
        std::fs::create_dir_all(root.join("config/fleet")).expect("config fleet dir");
        std::fs::write(
            root.join("config/fleet/63-mother.json"),
            r#"{"name":"63-mother","windows":[{"name":"main","repo":"github.com/laris-co/mother-oracle","kind":"oracle"}]}"#,
        )
        .expect("stale sibling fixture");
        std::fs::write(
            root.join("config/fleet/99-mother.json"),
            r#"{"name":"99-mother","windows":[{"name":"main","repo":"github.com/laris-co/mother-oracle","kind":"oracle"}]}"#,
        )
        .expect("revived fixture");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/laris-co/mother-oracle".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "99-mother", &windows, "maw wake").expect("upsert");

        assert_eq!(written.path, root.join("config/fleet/99-mother.json"));
        assert!(!written.created);
        let revived = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(revived["name"], "99-mother");
        assert_eq!(revived["windows"].as_array().expect("windows").len(), 1);
        let sibling = serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(root.join("config/fleet/63-mother.json")).expect("sibling"),
        )
        .expect("sibling json");
        assert_eq!(sibling["name"], "63-mother");
        assert!(!root.join("home/.maw/fleet/99-mother.json").exists());

        let _ = std::fs::remove_dir_all(root);
    }
}
