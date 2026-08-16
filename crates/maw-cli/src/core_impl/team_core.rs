const DISPATCH_122: &[DispatcherEntry] = &[];

const TEAM_USAGE: &str = "usage: maw team <create|new|list|ls|status|tasks|oracle-members|members|lives|history|plan|preflight|check|load|spawn|spawn-from|send|msg|broadcast|inbox|invite|up|bring|apply|reassign|liveness|down|remove|delete|rm|prune|gc|shutdown|resume|enter|send-enter|add|task|done|assign>";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TeamConfig122 {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    members: Vec<TeamMember122>,
    #[serde(default)]
    created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lead_session_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TeamMember122 {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tmux_pane_id: Option<String>,
    #[serde(default, rename = "agentId", skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    backend_type: Option<String>,
    #[serde(default, skip_serializing_if = "team_false")]
    is_active: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn team_false(value: &bool) -> bool { !*value }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TeamTask122 {
    id: u64,
    subject: String,
    status: String,
    #[serde(default)]
    assignee: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TeamOracleRegistry122 {
    name: String,
    #[serde(default)]
    members: Vec<TeamOracleMember122>,
    #[serde(default)]
    created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TeamOracleMember122 {
    oracle: String,
    role: String,
    #[serde(default)]
    added_at: String,
}

#[derive(Debug, Clone, Default)]
struct TeamCharter122 {
    name: String,
    project: Option<String>,
    description: String,
    goal: String,
    session: Option<String>,
    members: Vec<TeamCharterMember122>,
    defaults: std::collections::BTreeMap<String, String>,
    engines: std::collections::BTreeMap<String, String>,
    defaults_worktree: bool,
    governance_requires_human_approval: bool,
}

#[derive(Debug, Clone, Default)]
struct TeamCharterMember122 {
    role: String,
    name: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    engine: Option<String>,
    target: Option<String>,
    prompt: Option<String>,
    worktree: Option<String>,
    worktree_opt_out: bool,
    branch: Option<String>,
}

fn team_run_command(argv: &[String]) -> CliOutput {
    if wants_help_before_positionals(argv, &[]) {
        return help_output(TEAM_USAGE);
    }
    team_output_from_result(team_run(argv))
}

fn team_output_from_result(result: Result<String, String>) -> CliOutput {
    match result {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) if message == TEAM_USAGE => CliOutput { code: 0, stdout: format!("{TEAM_USAGE}\n"), stderr: String::new() },
        Err(message) if message.starts_with("__TEAM_INVITE_EXIT2__") => CliOutput { code: 2, stdout: String::new(), stderr: format!("{}\n", message.trim_start_matches("__TEAM_INVITE_EXIT2__")) },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn team_run(argv: &[String]) -> Result<String, String> {
    let sub = argv.first().map_or("list", String::as_str);
    match sub {
        "help" | "--help" | "-h" => Err(TEAM_USAGE.to_owned()),
        "create" | "new" => team_create(argv),
        "list" | "ls" => team_list(argv),
        "status" => team_status(argv),
        "tasks" => team_tasks(argv),
        "oracle-members" | "members" => team_oracle_members(argv),
        "lives" | "history" => team_lives(argv),
        "plan" => team_plan(argv),
        "preflight" | "check" => team_preflight(argv),
        "load" => team_load(argv),
        "spawn" => team_t5_spawn(argv),
        "spawn-from" => team_t5_spawn_from(argv),
        "send" | "msg" => team_send(argv),
        "broadcast" => team_broadcast(argv),
        "inbox" => team_inbox(argv),
        "invite" => team_invite(argv),
        "up" => team_t5b_up(argv),
        "bring" => team_t5b_bring(argv),
        "apply" => team_t5b_apply(argv),
        "reassign" => reassign_run(argv),
        "liveness" | "live-check" => team_t3_liveness(argv),
        "down" => team_down(argv),
        "remove" => team_remove(argv),
        "delete" | "rm" => team_delete(argv),
        "prune" => team_prune(argv),
        "gc" => team_gc(argv),
        "shutdown" => team_shutdown(argv),
        "resume" => team_resume(argv),
        "enter" | "send-enter" => team_enter_send_enter(argv),
        "add" | "task" | "done" | "assign" => team_task_ops241(argv),
        other if other.starts_with('-') => Err(format!("team: unknown argument {other}")),
        _ => Err(TEAM_USAGE.to_owned()),
    }
}

fn team_create(argv: &[String]) -> Result<String, String> {
    let name = argv.get(1).ok_or_else(|| "usage: maw team create <name> [--description <text>]".to_owned())?;
    team_validate_name(name)?;
    let description = team_description_arg(argv);
    let paths = team_paths(name);
    if paths.vault_manifest.exists() { return Err(format!("team '{name}' already exists at {}", paths.vault_dir.display())); }
    let created_at = team_now_millis();
    let manifest = serde_json::json!({"name":name,"createdAt":created_at,"members":[],"description":description,"leadSessionId":team_current_session_id()});
    let config = TeamConfig122 { name: name.to_owned(), description, members: Vec::new(), created_at, lead_session_id: team_current_session_id() };
    team_write_json_atomic_0600(&paths.vault_manifest, &manifest)?;
    team_write_json_atomic_0600(&paths.tool_config, &config)?;
    Ok(format!("\x1b[32m✓\x1b[0m team '{name}' created\n  \x1b[90m{}/manifest.json\x1b[0m\n", paths.vault_dir.display()))
}

fn team_list(argv: &[String]) -> Result<String, String> {
    team_only_flags(argv, &["--all"])?;
    let show_all = argv.iter().any(|arg| arg == "--all");
    let teams = team_collect_teams(show_all);
    if teams.is_empty() {
        return Ok("\x1b[90mNo teams found.\x1b[0m\n\x1b[90m  looked in: ~/.claude/teams/ (tool) + ψ/memory/mailbox/teams/ (vault)\x1b[0m\n".to_owned());
    }
    let mut out = "\n  \x1b[36;1mTEAM                          STORE  MEMBERS  STATUS          ZOMBIES\x1b[0m\n".to_owned();
    for item in teams { team_push_list_row(&mut out, &item); }
    out.push('\n');
    Ok(out)
}

fn team_status(argv: &[String]) -> Result<String, String> {
    let names = if let Some(name) = argv.get(1) { team_validate_name(name)?; vec![name.to_owned()] } else { team_team_names() };
    if names.is_empty() { return Ok("\x1b[36mℹ\x1b[0m no active teams\n".to_owned()); }
    let mut out = String::new();
    for name in names { team_push_status(&mut out, &name); }
    out.push('\n');
    Ok(out)
}

fn team_tasks(argv: &[String]) -> Result<String, String> {
    team_task_ops241(argv)
}


fn team_oracle_members(argv: &[String]) -> Result<String, String> {
    let team = team_team_arg(argv, 1)?;
    let registry = team_read_oracle_registry(&team);
    let Some(registry) = registry.filter(|r| !r.members.is_empty()) else {
        return Ok(format!("\x1b[90mNo oracle members in team '{team}'.\x1b[0m\n\x1b[90m  add one: maw team oracle-invite <oracle-name> --team {team}\x1b[0m\n"));
    };
    let mut out = format!("\n  \x1b[36;1mOracle members of '{team}'\x1b[0m ({})\n\n", registry.members.len());
    for member in registry.members { team_push_member_row(&mut out, &member); }
    out.push('\n');
    Ok(out)
}

fn team_lives(argv: &[String]) -> Result<String, String> {
    let agent = argv.get(1).ok_or_else(|| "usage: maw team lives <agent>".to_owned())?;
    team_validate_name(agent)?;
    let dir = team_psi_dir().join("memory").join("mailbox").join(agent);
    if !dir.exists() { return Ok(format!("\x1b[90mNo past lives found for '{agent}'\x1b[0m\n  \x1b[90mlooked in: {}\x1b[0m\n", dir.display())); }
    Ok(team_render_lives(agent, &dir))
}

fn team_plan(argv: &[String]) -> Result<String, String> {
    let path = argv.get(1).ok_or_else(|| "usage: maw team plan <team.yaml|team.json>".to_owned())?;
    let charter = team_read_charter_path(path)?;
    Ok(team_format_plan(&charter))
}

fn team_preflight(argv: &[String]) -> Result<String, String> {
    let path = argv.get(1).ok_or_else(|| "usage: maw team preflight <team.yaml|team.json>".to_owned())?;
    let charter = team_read_charter_path(path)?;
    let (out, errors) = team_format_preflight(&charter);
    if errors { Err(format!("preflight failed\n{out}")) } else { Ok(out) }
}

fn team_load(argv: &[String]) -> Result<String, String> {
    let path = argv.get(1).ok_or_else(|| "usage: maw team load <team.yaml|team.json> --no-spawn".to_owned())?;
    if !argv.iter().any(|arg| arg == "--no-spawn") { return Err("usage: maw team load <team.yaml|team.json> --no-spawn\nPhase 1 only supports materializing charter files; spawning remains a separate future step.".to_owned()); }
    let charter = team_read_charter_path(path)?;
    team_validate_name(&charter.name)?;
    team_load_charter_no_spawn(&charter)
}

#[derive(Debug, Clone)]
struct TeamPaths122 { tool_dir: std::path::PathBuf, tool_config: std::path::PathBuf, vault_dir: std::path::PathBuf, vault_manifest: std::path::PathBuf }

fn team_paths(name: &str) -> TeamPaths122 {
    let tool_dir = team_home_dir().join(".claude").join("teams").join(name);
    let vault_dir = team_psi_dir().join("memory").join("mailbox").join("teams").join(name);
    TeamPaths122 { tool_config: tool_dir.join("config.json"), tool_dir, vault_manifest: vault_dir.join("manifest.json"), vault_dir }
}

fn team_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")), std::path::PathBuf::from)
}

fn team_maw_home_dir() -> std::path::PathBuf {
    std::env::var_os("MAW_HOME").map_or_else(|| team_home_dir().join(".maw"), std::path::PathBuf::from)
}

fn team_state_dir() -> std::path::PathBuf {
    std::env::var_os("MAW_STATE_DIR").map_or_else(team_maw_home_dir, std::path::PathBuf::from)
}

fn team_psi_dir() -> std::path::PathBuf {
    std::env::var_os("MAW_RS_TEAM_PSI").map_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("ψ"), std::path::PathBuf::from)
}

fn team_current_session_id() -> Option<String> {
    ["CLAUDE_SESSION_ID", "CODEX_THREAD_ID", "OMX_SESSION_ID", "ATUIN_SESSION"].iter().find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
}

fn team_now_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

fn team_description_arg(argv: &[String]) -> String {
    if let Some(index) = argv.iter().position(|arg| arg == "--description") { return argv[index + 1..].join(" "); }
    String::new()
}

fn team_validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() { return Err("team name is empty".to_owned()); }
    if name.starts_with('-') { return Err(format!("unsafe team name '{name}': leading dash rejected")); }
    if name.contains("..") || name.contains('/') || name.contains('\\') { return Err(format!("unsafe team name '{name}': path traversal rejected")); }
    if name.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("unsafe team name: control character rejected".to_owned()); }
    Ok(())
}

fn team_validate_path_arg(path: &str) -> Result<(), String> {
    if path.is_empty() || path.starts_with('-') || path.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("unsafe path argument {path:?}")); }
    Ok(())
}

fn team_only_flags(argv: &[String], allowed: &[&str]) -> Result<(), String> {
    for arg in argv.iter().skip(1).filter(|arg| arg.starts_with('-')) { if !allowed.contains(&arg.as_str()) { return Err(format!("team: unknown argument {arg}")); } }
    Ok(())
}

fn team_team_arg(argv: &[String], start: usize) -> Result<String, String> {
    let mut team = None;
    let mut index = start;
    while index < argv.len() {
        match argv[index].as_str() {
            "--team" => { index += 1; team = argv.get(index).cloned(); },
            value if value.starts_with("--team=") => team = Some(value["--team=".len()..].to_owned()),
            value if value.starts_with('-') => return Err(format!("team: unknown argument {value}")),
            value if team.is_none() => team = Some(value.to_owned()),
            _ => {},
        }
        index += 1;
    }
    let name = team.or_else(|| std::env::var("MAW_TEAM").ok()).unwrap_or_else(|| "default".to_owned());
    team_validate_name(&name)?;
    Ok(name)
}

fn team_write_json_atomic_0600<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value).map_err(|error| format!("team: encode json failed: {error}"))? + "\n";
    team_atomic_write_0600(path, &body)
}

fn team_atomic_write_0600(path: &std::path::Path, body: &str) -> Result<(), String> {
    // Mechanic shared with every other secret-bearing writer — see
    // `atomic_write_0600` in config.rs (#838).
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = parent.join(format!(".{}.team-{}.tmp", path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("state"), std::process::id()));
    atomic_write_0600(path, &tmp, body, "team")
}

fn team_collect_teams(show_all: bool) -> Vec<(String, String, usize)> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let tool = team_home_dir().join(".claude").join("teams");
    for (name, config) in team_read_tool_teams(&tool) {
        if !show_all && !config.members.is_empty() { /* visible */ }
        seen.insert(name.clone());
        out.push((name, "tool".to_owned(), config.members.iter().filter(|m| m.agent_type.as_deref() != Some("team-lead")).count()));
    }
    for (name, count) in team_read_vault_only(&seen) { out.push((name, "vault".to_owned(), count)); }
    out
}

fn team_read_tool_teams(root: &std::path::Path) -> Vec<(String, TeamConfig122)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(config) = team_read_json::<TeamConfig122>(&entry.path().join("config.json")) { out.push((name, config)); }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn team_read_vault_only(seen: &std::collections::BTreeSet<String>) -> Vec<(String, usize)> {
    let root = team_psi_dir().join("memory").join("mailbox").join("teams");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if seen.contains(&name) { continue; }
        if let Some(value) = team_read_json::<serde_json::Value>(&entry.path().join("manifest.json")) { out.push((name, value["members"].as_array().map_or(0, Vec::len))); }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn team_push_list_row(out: &mut String, item: &(String, String, usize)) {
    use std::fmt::Write as _;
    let (name, store, members) = item;
    let status = if store == "vault" { "\x1b[90mprep-only\x1b[0m" } else { "\x1b[90mno live panes\x1b[0m" };
    let zombies = if store == "vault" { "\x1b[90m—\x1b[0m" } else { "0" };
    let _ = writeln!(out, "  {name:<30}{store:<7}{members:<9}{status:<26}{zombies}");
}

fn team_team_names() -> Vec<String> {
    team_read_tool_teams(&team_home_dir().join(".claude").join("teams")).into_iter().map(|(name, _)| name).collect()
}

fn team_push_status(out: &mut String, name: &str) {
    use std::fmt::Write as _;
    let Some(config) = team_read_json::<TeamConfig122>(&team_paths(name).tool_config) else { let _ = writeln!(out, "\x1b[33m⚠\x1b[0m team not found: {name}"); return; };
    let members: Vec<_> = config.members.iter().filter(|m| m.agent_type.as_deref() != Some("team-lead")).collect();
    let _ = writeln!(out, "\n\x1b[36;1mTeam: {name}\x1b[0m ({} agents)\n", members.len());
    let _ = writeln!(out, "  Agent           Status    Task                          Pane");
    let _ = writeln!(out, "  ─────────────── ───────── ───────────────────────────── ────────");
    for member in &members { let _ = writeln!(out, "  {:<15} \x1b[90midle\x1b[0m      {:<29} {}", member.name, "-", member.tmux_pane_id.as_deref().unwrap_or("-" )); }
    let tasks = team_read_tasks(name);
    let done = tasks.iter().filter(|task| task.status == "completed").count();
    let _ = writeln!(out, "\n  \x1b[90mTasks: {done}/{} done | Agents: 0 working, {} idle\x1b[0m", tasks.len(), members.len());
}

fn team_read_tasks(team: &str) -> Vec<TeamTask122> {
    let dir = team_state_dir().join("teams").join(team).join("tasks");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("_counter.json") { continue; }
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") { if let Some(task) = team_read_json::<TeamTask122>(&path) { out.push(task); } }
    }
    out.sort_by_key(|task| task.id);
    out
}

fn team_read_oracle_registry(team: &str) -> Option<TeamOracleRegistry122> {
    let primary = team_state_dir().join("teams").join(team).join("oracle-members.json");
    let legacy = team_maw_home_dir().join("config").join("teams").join(team).join("oracle-members.json");
    team_read_json(&primary).or_else(|| team_read_json(&legacy))
}

fn team_push_member_row(out: &mut String, member: &TeamOracleMember122) {
    use std::fmt::Write as _;
    let added = member.added_at.split('T').next().unwrap_or("");
    let _ = writeln!(out, "  \x1b[32m●\x1b[0m {:<30} \x1b[90mrole:\x1b[0m {:<15} \x1b[90madded:\x1b[0m {added}", member.oracle, member.role);
}

fn team_render_lives(agent: &str, dir: &std::path::Path) -> String {
    use std::fmt::Write as _;
    let files = team_dir_names(dir);
    let mut out = format!("\n  \x1b[36;1m{agent} — past lives\x1b[0m\n\n");
    let _ = writeln!(out, "  standing orders: {}", if files.iter().any(|f| f == "standing-orders.md") { "\x1b[32myes\x1b[0m" } else { "\x1b[90mno\x1b[0m" });
    let findings: Vec<_> = files.iter().filter(|f| f.ends_with("_findings.md")).collect();
    let _ = writeln!(out, "  findings: {}", if findings.is_empty() { "\x1b[90mnone\x1b[0m".to_owned() } else { format!("\x1b[32m{}\x1b[0m", findings.len()) });
    for file in findings { let _ = writeln!(out, "    \x1b[90m{} ({} lines)\x1b[0m", file, team_line_count(&dir.join(file))); }
    let other: Vec<_> = files.into_iter().filter(|f| f != "standing-orders.md" && !f.ends_with("_findings.md")).collect();
    if !other.is_empty() { let _ = writeln!(out, "  other: \x1b[90m{}\x1b[0m", other.join(", ")); }
    out.push('\n');
    out
}

fn team_line_count(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

fn team_read_charter_path(path: &str) -> Result<TeamCharter122, String> {
    team_validate_path_arg(path)?;
    let text = std::fs::read_to_string(path).map_err(|error| format!("team charter read failed: {error}"))?;
    team_parse_charter(&text)
}

fn team_parse_charter(text: &str) -> Result<TeamCharter122, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() { return Err("team charter is empty".to_owned()); }
    if trimmed.starts_with('{') { return team_parse_json_charter(trimmed); }
    team_parse_yaml_charter(trimmed)
}

fn team_parse_json_charter(text: &str) -> Result<TeamCharter122, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let name = value["name"].as_str().unwrap_or("").to_owned();
    let project = value["project"].as_str().map(str::to_owned);
    let session = value["session"].as_str().map(str::to_owned);
    let members = value["members"].as_array().map_or_else(Vec::new, |items| items.iter().map(team_member_from_json).collect());
    let governance = value["governance"]["requires_human_approval"].as_bool().unwrap_or(false);
    let defaults = team_string_map_from_json(&value["defaults"]);
    let engines = team_string_map_from_json(&value["engines"]);
    let defaults_worktree = defaults.contains_key("worktree");
    team_charter_finish(TeamCharter122 { name, project, description: value["description"].as_str().unwrap_or("").to_owned(), goal: value["goal"].as_str().unwrap_or("").to_owned(), session, members, defaults, engines, defaults_worktree, governance_requires_human_approval: governance })
}

fn team_string_map_from_json(value: &serde_json::Value) -> std::collections::BTreeMap<String, String> {
    value
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| team_json_scalar_to_string(value).map(|value| (key.clone(), value)))
                .collect()
        })
        .unwrap_or_default()
}

fn team_json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_bool().map(|value| value.to_string()))
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .or_else(|| value.as_f64().map(|value| value.to_string()))
}

fn team_member_from_json(value: &serde_json::Value) -> TeamCharterMember122 {
    TeamCharterMember122 { role: value["role"].as_str().unwrap_or("").to_owned(), name: value["name"].as_str().map(str::to_owned), model: value["model"].as_str().map(str::to_owned), cwd: value["cwd"].as_str().map(str::to_owned), engine: value["engine"].as_str().map(str::to_owned), target: value["target"].as_str().map(str::to_owned), prompt: value["prompt"].as_str().map(str::to_owned), worktree: team_member_worktree_from_json(value), worktree_opt_out: team_member_worktree_opt_out_from_json(value), branch: value["branch"].as_str().map(str::to_owned) }
}

fn team_member_worktree_from_json(value: &serde_json::Value) -> Option<String> {
    let worktree = value.get("worktree")?;
    if team_json_worktree_is_opt_out(worktree) { return None; }
    worktree.as_str().map(str::to_owned).or_else(|| worktree.as_bool().filter(|enabled| *enabled).map(|_| team_member_name_path(value)))
}

fn team_member_worktree_opt_out_from_json(value: &serde_json::Value) -> bool {
    value.get("worktree").is_some_and(team_json_worktree_is_opt_out)
}

fn team_json_worktree_is_opt_out(value: &serde_json::Value) -> bool {
    value.is_null() || value.as_bool() == Some(false) || value.as_str().is_some_and(team_worktree_literal_is_opt_out)
}

fn team_member_name_path(value: &serde_json::Value) -> String {
    value["name"].as_str().filter(|name| !name.trim().is_empty()).or_else(|| value["role"].as_str()).unwrap_or("").to_owned()
}

fn team_parse_yaml_charter(text: &str) -> Result<TeamCharter122, String> {
    let mut charter = TeamCharter122::default();
    let mut current: Option<TeamCharterMember122> = None;
    let mut block = TeamYamlBlock122::None;
    let mut member_block_scalar: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() { continue; }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let trimmed = line.trim();
        if indent == 0 {
            // A new top-level section ends the current member: flush it so a
            // trailing block's fields (e.g. `lifecycle:\n  worktree: true`)
            // cannot leak into and overwrite the last member's worktree (#658).
            if let Some(member) = current.take() {
                charter.members.push(member);
            }
            block = TeamYamlBlock122::from_header(trimmed);
            member_block_scalar = None;
        } else if team_yaml_block_line(trimmed, block, &mut charter) {
            continue;
        }
        if let Some(block_indent) = member_block_scalar {
            if indent > block_indent {
                continue;
            }
            member_block_scalar = None;
        }
        if let Some(block_indent) = team_yaml_line(line, &mut charter, &mut current) {
            member_block_scalar = Some(block_indent);
        }
    }
    if let Some(member) = current.take() { charter.members.push(member); }
    team_charter_finish(charter)
}

#[derive(Clone, Copy)]
enum TeamYamlBlock122 {
    None,
    Defaults,
    Engines,
}

impl TeamYamlBlock122 {
    fn from_header(trimmed: &str) -> Self {
        match trimmed {
            "defaults:" => Self::Defaults,
            "engines:" => Self::Engines,
            _ => Self::None,
        }
    }
}

fn team_yaml_block_line(trimmed: &str, block: TeamYamlBlock122, charter: &mut TeamCharter122) -> bool {
    match block {
        TeamYamlBlock122::Defaults => {
            if let Some((key, value)) = team_yaml_key_value(trimmed) {
                if key == "worktree" { charter.defaults_worktree = true; }
                charter.defaults.insert(key, value);
            }
            true
        }
        TeamYamlBlock122::Engines => {
            if let Some((key, value)) = team_yaml_key_value(trimmed) {
                charter.engines.insert(key, value);
            }
            true
        }
        TeamYamlBlock122::None => false,
    }
}

fn team_yaml_key_value(trimmed: &str) -> Option<(String, String)> {
    let (key, value) = trimmed.split_once(':')?;
    let key = key.trim();
    if key.is_empty() { return None; }
    Some((key.to_owned(), team_unquote(value)))
}

fn team_yaml_line(line: &str, charter: &mut TeamCharter122, current: &mut Option<TeamCharterMember122>) -> Option<usize> {
    if let Some(rest) = line.strip_prefix("name:") { charter.name = team_unquote(rest); return None; }
    if let Some(rest) = line.strip_prefix("project:") { charter.project = Some(team_unquote(rest)); return None; }
    if let Some(rest) = line.strip_prefix("description:") { charter.description = team_unquote(rest); return None; }
    if let Some(rest) = line.strip_prefix("goal:") { charter.goal = team_unquote(rest); return None; }
    if let Some(rest) = line.strip_prefix("session:") { charter.session = Some(team_unquote(rest)); return None; }
    if line.trim() == "requires_human_approval: true" { charter.governance_requires_human_approval = true; return None; }
    if let Some(rest) = line.trim_start().strip_prefix("- role:") {
        if let Some(member) = current.take() { charter.members.push(member); }
        *current = Some(TeamCharterMember122 { role: team_unquote(rest), ..Default::default() });
        return None;
    }
    current.as_mut().and_then(|member| team_yaml_member_line(line, member))
}

fn team_yaml_member_line(line: &str, member: &mut TeamCharterMember122) -> Option<usize> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("worktree:") {
        let value = team_unquote(rest);
        member.worktree_opt_out = team_worktree_literal_is_opt_out(&value);
        member.worktree = (!member.worktree_opt_out).then_some(value);
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("prompt:") {
        let value = team_unquote(rest);
        let block_indent = team_yaml_value_is_block_scalar(&value).then(|| line.chars().take_while(|ch| *ch == ' ').count());
        member.prompt = Some(value);
        return block_indent;
    }
    for (key, slot) in [("name:", &mut member.name), ("model:", &mut member.model), ("cwd:", &mut member.cwd), ("engine:", &mut member.engine), ("target:", &mut member.target), ("branch:", &mut member.branch)] {
        if let Some(rest) = trimmed.strip_prefix(key) {
            *slot = Some(team_unquote(rest));
            return None;
        }
    }
    None
}

fn team_yaml_value_is_block_scalar(value: &str) -> bool {
    value
        .trim()
        .strip_prefix(['|', '>'])
        .is_some_and(|rest| rest.chars().all(|ch| ch == '+' || ch == '-' || ch.is_ascii_digit()))
}

fn team_unquote(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_owned()
}

fn team_worktree_literal_is_opt_out(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || value == "~" || value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("null")
}

fn team_charter_finish(mut charter: TeamCharter122) -> Result<TeamCharter122, String> {
    let name = charter.name.trim().to_owned();
    if name.is_empty() { return Err("team charter requires name".to_owned()); }
    if charter.members.is_empty() { return Err("team charter requires at least one member".to_owned()); }
    charter.name = name;
    for member in &mut charter.members {
        if member.worktree_opt_out {
            member.worktree = None;
            continue;
        }
        if let Some(worktree) = member.worktree.as_deref() {
            if team_worktree_literal_is_opt_out(worktree) {
                member.worktree = None;
                member.worktree_opt_out = true;
            } else if worktree == "true" {
                member.worktree = Some(member.name.as_deref().filter(|name| !name.trim().is_empty()).unwrap_or(&member.role).to_owned());
            }
        }
    }
    Ok(charter)
}

fn team_format_plan(charter: &TeamCharter122) -> String {
    let paths = team_paths(&charter.name);
    let mut artifacts = vec![paths.tool_config.clone()];
    artifacts.extend(charter.members.iter().map(|m| paths.tool_dir.join("inboxes").join(format!("{}.json", m.role))));
    artifacts.push(paths.vault_manifest.clone());
    team_render_charter_plan("team charter plan", charter, &artifacts, &["read-only plan only", "no files written", "no tmux panes changed", "no claude processes spawned", "no maw bud or fleet writes"])
}

fn team_render_charter_plan(title: &str, charter: &TeamCharter122, artifacts: &[std::path::PathBuf], actions: &[&str]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("{title}: {}\n", charter.name);
    if !charter.description.is_empty() { let _ = writeln!(out, "description: {}", charter.description); }
    if !charter.goal.is_empty() { let _ = writeln!(out, "goal: {}", charter.goal.lines().next().unwrap_or("")); }
    let _ = writeln!(out, "\nmembers ({}):", charter.members.len());
    for member in &charter.members { let _ = writeln!(out, "  - {} ({})", member.role, team_member_bits(member)); }
    let _ = writeln!(out, "\nwould prepare artifacts:");
    for artifact in artifacts { let _ = writeln!(out, "  - {}", artifact.display()); }
    let _ = writeln!(out, "\nphase-0 safety:");
    for action in actions { let _ = writeln!(out, "  - {action}"); }
    out
}

fn team_member_bits(member: &TeamCharterMember122) -> String {
    let mut bits = vec![format!("target={}", member.target.as_deref().unwrap_or("auto"))];
    if let Some(name) = &member.name { bits.push(format!("name={name}")); }
    if let Some(model) = &member.model { bits.push(format!("model={model}")); }
    if let Some(cwd) = &member.cwd { bits.push(format!("cwd={cwd}")); }
    if let Some(worktree) = &member.worktree { bits.push(format!("worktree={worktree}")); }
    if let Some(branch) = &member.branch { bits.push(format!("branch={branch}")); }
    if let Some(engine) = &member.engine { bits.push(format!("engine={engine}")); }
    bits.join(", ")
}

fn team_format_preflight(charter: &TeamCharter122) -> (String, bool) {
    use std::fmt::Write as _;
    let checks = team_preflight_checks(charter);
    let errors = checks.iter().any(|(ok, _, _)| !ok);
    let mut out = format!("team charter preflight: {}\nstatus: {}\n\nchecks:\n", charter.name, if errors { "failed" } else { "passed" });
    for (ok, label, detail) in checks { let _ = writeln!(out, "  {} {label}: {detail}", if ok { "✓" } else { "✗" }); }
    out.push_str("\npreflight safety:\n  - read-only preflight only\n  - no files written\n  - no tmux panes changed\n  - no claude processes spawned\n  - no maw bud or fleet writes\n");
    (out, errors)
}

fn team_plan_artifacts(charter: &TeamCharter122) -> Vec<std::path::PathBuf> {
    let paths = team_paths(&charter.name);
    let mut artifacts = vec![paths.tool_config];
    artifacts.extend(charter.members.iter().map(|m| paths.tool_dir.join("inboxes").join(format!("{}.json", m.role))));
    artifacts.push(paths.vault_manifest);
    artifacts
}

fn team_unique_roles(members: &[TeamCharterMember122]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    members.iter().all(|member| !member.role.is_empty() && seen.insert(member.role.clone()))
}

fn team_load_charter_no_spawn(charter: &TeamCharter122) -> Result<String, String> {
    let paths = team_paths(&charter.name);
    let collisions: Vec<_> = [paths.tool_config.as_path(), paths.vault_manifest.as_path()].into_iter().filter(|path| path.exists()).map(|path| path.display().to_string()).collect();
    if !collisions.is_empty() { return Err(format!("team '{}' already exists; refusing to overwrite {}", charter.name, collisions.join(", "))); }
    let created_at = team_now_millis();
    let members: Vec<_> = charter.members.iter().map(team_config_member_from_charter).collect();
    let config = TeamConfig122 { name: charter.name.clone(), description: charter.description.clone(), members, created_at, lead_session_id: None };
    let mut manifest = serde_json::json!({"name":charter.name,"createdAt":created_at,"description":charter.description,"goal":charter.goal,"members":charter.members.iter().map(|m| m.role.clone()).collect::<Vec<_>>(),"source":"team-charter"});
    let member_engines = team_charter_member_engines(charter);
    if !member_engines.is_empty() {
        manifest["memberEngines"] = serde_json::json!(member_engines);
    }
    // Snapshot semantics: resume replays what launched, not what the charter
    // now says. Edit `engines:` and re-spawn to change it.
    let member_engine_commands = team_charter_member_engine_commands(charter);
    if !member_engine_commands.is_empty() {
        manifest["memberEngineCommands"] = serde_json::json!(member_engine_commands);
    }
    team_write_json_atomic_0600(&paths.tool_config, &config)?;
    for member in &charter.members { team_write_json_atomic_0600(&paths.tool_dir.join("inboxes").join(format!("{}.json", member.role)), &serde_json::json!([]))?; }
    team_write_json_atomic_0600(&paths.vault_manifest, &manifest)?;
    Ok(team_format_load(charter, &team_plan_artifacts(charter)))
}

fn team_config_member_from_charter(member: &TeamCharterMember122) -> TeamMember122 {
    TeamMember122 { name: member.role.clone(), model: member.model.clone(), backend_type: member.target.as_ref().filter(|t| t.as_str() != "auto").cloned(), ..Default::default() }
}

/// Per-member engine names declared by the charter.
fn team_charter_member_engines(charter: &TeamCharter122) -> std::collections::BTreeMap<String, String> {
    charter
        .members
        .iter()
        .filter_map(|member| {
            member
                .engine
                .as_deref()
                .map(str::trim)
                .filter(|engine| !engine.is_empty())
                .map(|engine| (member.role.clone(), engine.to_owned()))
        })
        .collect()
}

/// Launch lines for members whose `engine:` names an entry in the charter's
/// `engines:` block.
///
/// A member's `engine:` is a KEY, not always an engine name: `engines:` maps
/// it to a full command line, and `team_t3_classify` already resolves it that
/// way into `engine_command` (#738/#758). Recording only the key would lose
/// that resolution, and `team resume` would replay `-e omx-1` — a token the
/// unified ladder cannot resolve, since `commands.omx-1` does not exist —
/// leaving it to exec a binary literally named `omx-1`. Recorded alongside the
/// name so resume can reproduce the pair the spawn actually used.
fn team_charter_member_engine_commands(charter: &TeamCharter122) -> std::collections::BTreeMap<String, String> {
    team_charter_member_engines(charter)
        .into_iter()
        .filter_map(|(role, engine)| charter.engines.get(&engine).map(|command| (role, command.clone())))
        .collect()
}

fn team_format_load(charter: &TeamCharter122, artifacts: &[std::path::PathBuf]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team charter loaded: {}\n\nwrote artifacts:\n", charter.name);
    for artifact in artifacts { let _ = writeln!(out, "  - {}", artifact.display()); }
    out.push_str("\nload safety:\n  - --no-spawn respected\n  - no tmux panes changed\n  - no claude processes spawned\n  - no maw bud or fleet writes\n\nnext: maw team list\n");
    out
}

fn team_read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
}

fn team_dir_names(path: &std::path::Path) -> Vec<String> {
    let mut out = std::fs::read_dir(path).map_or_else(|_| Vec::new(), |entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect());
    out.sort();
    out
}

#[cfg(test)]
mod team_tests {
    use super::*;

    #[test]
    fn team_validate_rejects_injection_names() {
        for name in ["", "-bad", "../bad", "bad/name", "bad\0name", "bad\nname"] { assert!(team_validate_name(name).is_err(), "{name:?}"); }
        assert!(team_validate_name("alpha_team-1").is_ok());
    }

    #[test]
    fn team_charter_preserves_defaults_engines_and_coerces_yaml_worktree_true() {
        let charter = team_parse_charter(
            r"name: alpha
project: org/repo
defaults:
  worktree: true
  branch: agents/default
engines:
  omx-1: CODEX_HOME=$PWD/.codex omx --direct
members:
  - role: coder
    name: agents/coder
    worktree: true
",
        )
        .expect("charter");
        assert_eq!(charter.defaults.get("worktree").map(String::as_str), Some("true"));
        assert_eq!(charter.defaults.get("branch").map(String::as_str), Some("agents/default"));
        assert_eq!(charter.engines.get("omx-1").map(String::as_str), Some("CODEX_HOME=$PWD/.codex omx --direct"));
        assert_eq!(charter.members[0].worktree.as_deref(), Some("agents/coder"));
        assert!(charter.defaults_worktree);
        assert!(team_charter_member_engines(&charter).is_empty());
    }

    #[test]
    fn team_charter_preserves_json_defaults_engines_and_coerces_worktree_true() {
        let charter = team_parse_charter(
            r#"{"name":"alpha","defaults":{"worktree":true},"engines":{"omx-2":"CODEX_HOME=$PWD/.codex2 omx"},"members":[{"role":"reviewer","name":"agents/reviewer","engine":"codex","worktree":true},{"role":"planner","worktree":false}]}"#,
        )
        .expect("charter");
        assert_eq!(charter.defaults.get("worktree").map(String::as_str), Some("true"));
        assert_eq!(charter.engines.get("omx-2").map(String::as_str), Some("CODEX_HOME=$PWD/.codex2 omx"));
        assert_eq!(charter.members[0].worktree.as_deref(), Some("agents/reviewer"));
        assert_eq!(team_charter_member_engines(&charter).get("reviewer").map(String::as_str), Some("codex"));
        assert_eq!(charter.members[1].worktree, None);
        assert!(charter.members[1].worktree_opt_out);
    }

    #[test]
    fn team_charter_last_member_prompt_block_does_not_overwrite_worktree() {
        let charter = team_parse_charter(
            r"name: alpha
project: org/repo
members:
  - role: first
    worktree: agents/first
    branch: agents/first
  - role: second
    worktree: agents/second
    branch: agents/second
    prompt: |
      Startup notes may mention legacy fallback fields.
      worktree: second
      branch: second
",
        )
        .expect("charter");
        assert_eq!(charter.members.len(), 2);
        assert_eq!(charter.members[1].worktree.as_deref(), Some("agents/second"));
        assert_eq!(charter.members[1].branch.as_deref(), Some("agents/second"));
    }

    #[test]
    fn team_charter_trailing_lifecycle_block_does_not_overwrite_last_member_worktree() {
        // #658: a top-level section after the last member (e.g. `lifecycle:`
        // with `worktree: true`) must NOT leak into the still-open last member.
        let charter = team_parse_charter(
            r"name: alpha
project: org/repo
members:
  - role: first
    worktree: agents/first
  - role: last
    worktree: agents/last
lifecycle:
  worktree: true
  merge_on_shutdown: false
",
        )
        .expect("charter");
        assert_eq!(charter.members.len(), 2);
        assert_eq!(charter.members[1].role, "last");
        assert_eq!(
            charter.members[1].worktree.as_deref(),
            Some("agents/last"),
            "trailing lifecycle worktree:true must not clobber the last member"
        );
    }

    #[test]
    fn team_charter_coder_keeps_agents_worktree_when_first_or_last() {
        for (label, yaml) in [
            (
                "coder-first",
                r"name: alpha
project: org/repo
members:
  - role: coder
    worktree: agents/coder
    branch: agents/coder
    prompt: |
      Do not treat prompt body text as member metadata.
      worktree: coder
  - role: lead
    worktree: false
",
            ),
            (
                "coder-last",
                r"name: alpha
project: org/repo
members:
  - role: lead
    worktree: false
  - role: coder
    worktree: agents/coder
    branch: agents/coder
    prompt: |
      Do not treat prompt body text as member metadata.
      worktree: coder
",
            ),
        ] {
            let charter = team_parse_charter(yaml).expect("charter");
            let coder = charter.members.iter().find(|member| member.role == "coder").expect("coder member");
            assert_eq!(coder.worktree.as_deref(), Some("agents/coder"), "{label}");
        }
    }

    #[test]
    fn team_charter_worktree_false_and_yaml_null_opt_out_even_with_defaults() {
        let charter = team_parse_charter(
            r#"name: alpha
defaults:
  worktree: true
members:
  - role: lead
    name: lead-window
    worktree: false
  - role: reviewer
    worktree: "false"
  - role: planner
    worktree: ~
  - role: scribe
    worktree: null
  - role: coder
    name: agents/coder
    worktree: true
"#,
        )
        .expect("charter");
        assert!(charter.defaults_worktree);
        for member in &charter.members[..4] {
            assert_eq!(member.worktree, None, "{}", member.role);
            assert!(member.worktree_opt_out, "{}", member.role);
        }
        assert_eq!(charter.members[4].worktree.as_deref(), Some("agents/coder"));
        assert!(!charter.members[4].worktree_opt_out);
    }

    #[test]
    fn team_dispatch_fragment_moved_to_part240() {
        assert!(DISPATCH_122.is_empty());
        assert_eq!(DISPATCH_240[0].command, "team");
    }
}
