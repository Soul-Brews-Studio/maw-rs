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







trait FleetRuntime {
    fn fleet_run_command(&mut self, program: &str, args: &[String]) -> Result<String, String>;
    fn fleet_list_all(&mut self) -> Result<Vec<TmuxSession>, String>;
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

    fn fleet_list_all(&mut self) -> Result<Vec<TmuxSession>, String> {
        TmuxClient::local()
            .list_all()
            .map_err(|error| format!("tmux unreachable: {error}"))
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
        // #605: never surface invalid manifest names (argv junk, dot/backup
        // names) as fleet agents.
        if !agent_name_is_valid(name) {
            continue;
        }
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










fn fleet_run_add(
    state: &FleetState,
    options: &FleetOptions,
    runtime: &mut impl FleetRuntime,
) -> Result<(i32, String), String> {
    let session = options.target.as_deref().ok_or_else(|| "fleet add: missing session".to_owned())?;
    let live = runtime
        .fleet_list_all()?
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














































// Registry resolution uses the shared resolver's `NN-` prefix and `-oracle` suffix normalization.
// Window names count because a member can live as a window of a shared session.
































