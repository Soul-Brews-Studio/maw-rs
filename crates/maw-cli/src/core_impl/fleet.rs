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
        FleetCommand::Census => Ok((0, fleet_render_census(&state, options.json)?)),
        FleetCommand::Doctor | FleetCommand::Health => fleet_run_doctor(&state, &options),
        FleetCommand::Gc => fleet_run_gc(&state, &options, &mut maw_tmux::CommandTmuxRunner::new()),
        FleetCommand::Wake => fleet_run_wake(&state, &options),
        FleetCommand::Sleep => fleet_run_sleep(&state, &options),
        FleetCommand::Init => fleet_run_named_plan(&state, &options, "init"),
        FleetCommand::Consolidate => fleet_run_named_plan(&state, &options, "consolidate"),
        FleetCommand::Resume => fleet_run_named_plan(&state, &options, "resume"),
        FleetCommand::Sync => fleet_run_named_plan(&state, &options, "sync"),
    }
}

fn fleet_parse_args(argv: &[String]) -> Result<FleetOptions, String> {
    let mut options = fleet_default_options();
    let mut command_seen = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(fleet_usage()),
            "--json" => options.json = true,
            "--dry-run" => options.dry_run = true,
            "--fix" => options.fix = true,
            "--reboot" => options.reboot = true,
            "--all" => options.all = true,
            "--kill" => options.kill = true,
            "--resume" => options.resume = true,
            value if value.starts_with('-') => return Err(format!("fleet: unknown argument {value}")),
            value => fleet_parse_positional(&mut options, &mut command_seen, value)?,
        }
    }
    if matches!(options.command, FleetCommand::Add) && options.target.is_none() {
        return Err("fleet add: missing session".to_owned());
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
    }
}

fn fleet_parse_positional(options: &mut FleetOptions, seen: &mut bool, value: &str) -> Result<(), String> {
    if !*seen {
        return fleet_set_command(options, seen, value);
    }
    if matches!(options.command, FleetCommand::Add) && options.target.is_none() {
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
        "wake" | "wake-all" => FleetCommand::Wake,
        "sleep" => FleetCommand::Sleep,
        _ => return Err(format!("fleet: unknown subcommand {value}")),
    };
    *seen = true;
    Ok(())
}

fn fleet_usage() -> String {
    "usage: maw fleet [add <session>|create <group>|show <group>|status <group>|ls|doctor|health|gc|init|consolidate|resume|sync|wake|sleep|token <group> [ls|status]] [--json] [--dry-run] [--fix] [--reboot] [--all] [--kill] [--resume]".to_owned()
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

fn fleet_entries_to_summaries(entries: &[NativeFleetEntry]) -> Vec<FleetSessionSummary> {
    entries
        .iter()
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

fn fleet_render_census(state: &FleetState, json: bool) -> Result<String, String> {
    if json { return fleet_json_census(state); }
    let windows = fleet_window_count(state);
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[36mfleet\x1b[0m node {}", state.config.node);
    let _ = writeln!(out, "  sessions: {} ({} windows, {} disabled)", state.sessions.len(), windows, state.disabled_count);
    let _ = writeln!(out, "  peers: {}", state.config.peers.len());
    let _ = writeln!(out, "  agents: {}", state.config.agents.len());
    for session in &state.sessions {
        let _ = writeln!(out, "  - {} ({} windows)", session.name, session.windows.len());
    }
    Ok(out)
}

fn fleet_json_census(state: &FleetState) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "configDir": state.config_dir,
        "sessions": state.sessions.iter().map(fleet_json_session).collect::<Vec<_>>(),
        "sessionCount": state.sessions.len(),
        "windowCount": fleet_window_count(state),
        "disabledCount": state.disabled_count,
        "peerCount": state.config.peers.len(),
        "agentCount": state.config.agents.len(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
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

fn fleet_window_count(state: &FleetState) -> usize {
    state.sessions.iter().map(|session| session.windows.len()).sum()
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

fn fleet_run_doctor(state: &FleetState, options: &FleetOptions) -> Result<(i32, String), String> {
    let mut findings = fleet_findings(state);
    if options.reboot { findings.extend(fleet_reboot_findings(state)); }
    let code = fleet_exit_code(&findings);
    if options.json { return Ok((code, fleet_json_doctor(state, &findings)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🩺 Fleet Doctor node: {}", state.config.node);
    let _ = writeln!(out, "  peers: {} · agents: {} · sessions: {}", state.config.peers.len(), state.config.agents.len(), state.sessions.len());
    if options.fix || options.dry_run { let _ = writeln!(out, "  mode: dry-run repair plan"); }
    fleet_write_findings(&mut out, &findings);
    Ok((code, out))
}

fn fleet_json_doctor(state: &FleetState, findings: &[FleetFinding]) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "findings": findings.iter().map(fleet_json_finding).collect::<Vec<_>>(),
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

fn fleet_findings(state: &FleetState) -> Vec<FleetFinding> {
    let mut findings = Vec::new();
    fleet_duplicate_peer_findings(state, &mut findings);
    fleet_self_peer_findings(state, &mut findings);
    fleet_agent_findings(state, &mut findings);
    fleet_repo_findings(state, &mut findings);
    fleet_duplicate_session_findings(state, &mut findings);
    findings
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
    for entry in &state.fleet_entries {
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

fn fleet_run_wake(state: &FleetState, options: &FleetOptions) -> Result<(i32, String), String> {
    let sessions = fleet_wake_targets(state, options.all);
    if options.json { return Ok((0, fleet_json_action(state, "wake", &sessions, options)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🌅 Fleet wake plan node: {}", state.config.node);
    let _ = writeln!(out, "  sessions: {} · disabled skipped: {}", sessions.len(), state.disabled_count);
    if options.kill { let _ = writeln!(out, "  preflight: sleep existing sessions first"); }
    if options.resume { let _ = writeln!(out, "  resume: yes"); }
    fleet_write_session_plan(&mut out, &sessions);
    Ok((0, out))
}

fn fleet_wake_targets(state: &FleetState, all: bool) -> Vec<FleetSessionSummary> {
    state.sessions.iter().filter(|session| all || !fleet_is_dormant_session(&session.name)).cloned().collect()
}

fn fleet_is_dormant_session(name: &str) -> bool {
    let digits = name.chars().take_while(char::is_ascii_digit).collect::<String>();
    digits.parse::<u32>().is_ok_and(|number| (20..99).contains(&number))
}

fn fleet_write_session_plan(out: &mut String, sessions: &[FleetSessionSummary]) {
    for session in sessions {
        let _ = writeln!(out, "  - {}", session.name);
        for window in &session.windows {
            let _ = writeln!(out, "      {} -> {}", window.name, window.repo);
        }
    }
}

fn fleet_run_sleep(state: &FleetState, options: &FleetOptions) -> Result<(i32, String), String> {
    if options.json { return Ok((0, fleet_json_action(state, "sleep", &state.sessions, options)?)); }
    let mut out = String::new();
    let _ = writeln!(out, "🌙 Fleet sleep plan node: {}", state.config.node);
    fleet_write_session_plan(&mut out, &state.sessions);
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
    let path = dir.join(format!("{session}.json"));
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
            windows.push(FleetWindowSummary { name: name.to_owned(), repo: repo.to_owned(), kind });
        }
    }
    for update in updates.iter().filter(|window| !window.name.trim().is_empty()) {
        if let Some(existing) = windows.iter_mut().find(|window| window.name == update.name) {
            existing.repo.clone_from(&update.repo);
            if update.kind.is_some() {
                existing.kind = update.kind;
            }
        } else {
            windows.push(update.clone());
        }
    }
    windows
        .into_iter()
        .map(|window| fleet_json_window(&window))
        .collect()
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
    if let Some(root) = repos_root {
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
            } else {
                Err(format!("unexpected command {program} {args:?}"))
            }
        }

        fn fleet_list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
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
        assert!(fleet_parse_args(&fleet_strings(&["--", "wake"])).expect_err("separator guard").contains("unknown argument"));
        assert!(fleet_parse_args(&fleet_strings(&["-oProxyCommand=bad"])).expect_err("leading dash").contains("unknown argument"));
    }

    #[test]
    fn fleet_census_is_hermetic_and_golden() {
        fleet_with_fixture(|_| {
            let output = run_fleet_command(&fleet_strings(&["ls"]));
            assert_eq!(output.code, 0);
            assert!(output.stderr.is_empty());
            assert_eq!(output.stdout, "\u{1b}[36mfleet\u{1b}[0m node alpha\n  sessions: 1 (2 windows, 1 disabled)\n  peers: 1\n  agents: 2\n  - 03-alpha (2 windows)\n");
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
        assert_eq!(json["windows"][0]["repo"], "github.com/Soul-Brews-Studio/maw-rs");
        assert_eq!(json["windows"][0]["kind"], "oracle");
    }

    #[test]
    fn fleet_wake_skips_dormant_without_real_tmux() {
        fleet_with_fixture(|_| {
            let output = run_fleet_command(&fleet_strings(&["wake", "--json", "--dry-run"]));
            assert_eq!(output.code, 0);
            assert!(output.stdout.contains("\"action\": \"wake\""));
            assert!(output.stdout.contains("\"sessionCount\": 1"));
            assert!(!output.stdout.contains("22-dormant"));
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
}
