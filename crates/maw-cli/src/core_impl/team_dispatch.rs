const DISPATCH_122: &[DispatcherEntry] = &[];

const TEAM_USAGE: &str = "usage: maw team <create|new|list|ls|status|tasks|oracle-members|members|lives|history|plan|preflight|check|load|send|msg|broadcast|inbox|invite|up|bring|apply|reassign|liveness|down|remove|delete|rm|prune|shutdown|enter|send-enter|add|task|done|assign>";

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
}

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
    description: String,
    goal: String,
    session: Option<String>,
    members: Vec<TeamCharterMember122>,
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
}

fn team_run_command(argv: &[String]) -> CliOutput {
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
    use std::io::Write as _;
    #[cfg(unix)] use std::os::unix::fs::OpenOptionsExt as _;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("team: create {} failed: {error}", parent.display()))?;
    let tmp = parent.join(format!(".{}.team-{}.tmp", path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("state"), std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true).truncate(false);
    #[cfg(unix)] opts.mode(0o600);
    let mut file = opts.open(&tmp).map_err(|error| format!("team: create tmp {} failed: {error}", tmp.display()))?;
    file.write_all(body.as_bytes()).map_err(|error| format!("team: write tmp failed: {error}"))?;
    file.sync_all().map_err(|error| format!("team: sync tmp failed: {error}"))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|error| { let _ = std::fs::remove_file(&tmp); format!("team: atomic rename {} failed: {error}", path.display()) })
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
    writeln!(out, "  {name:<30}{store:<7}{members:<9}{status:<26}{zombies}").expect("write string");
}

fn team_team_names() -> Vec<String> {
    team_read_tool_teams(&team_home_dir().join(".claude").join("teams")).into_iter().map(|(name, _)| name).collect()
}

fn team_push_status(out: &mut String, name: &str) {
    use std::fmt::Write as _;
    let Some(config) = team_read_json::<TeamConfig122>(&team_paths(name).tool_config) else { writeln!(out, "\x1b[33m⚠\x1b[0m team not found: {name}").expect("write string"); return; };
    let members: Vec<_> = config.members.iter().filter(|m| m.agent_type.as_deref() != Some("team-lead")).collect();
    writeln!(out, "\n\x1b[36;1mTeam: {name}\x1b[0m ({} agents)\n", members.len()).expect("write string");
    writeln!(out, "  Agent           Status    Task                          Pane").expect("write string");
    writeln!(out, "  ─────────────── ───────── ───────────────────────────── ────────").expect("write string");
    for member in &members { writeln!(out, "  {:<15} \x1b[90midle\x1b[0m      {:<29} {}", member.name, "-", member.tmux_pane_id.as_deref().unwrap_or("-" )).expect("write string"); }
    let tasks = team_read_tasks(name);
    let done = tasks.iter().filter(|task| task.status == "completed").count();
    writeln!(out, "\n  \x1b[90mTasks: {done}/{} done | Agents: 0 working, {} idle\x1b[0m", tasks.len(), members.len()).expect("write string");
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
    writeln!(out, "  \x1b[32m●\x1b[0m {:<30} \x1b[90mrole:\x1b[0m {:<15} \x1b[90madded:\x1b[0m {added}", member.oracle, member.role).expect("write string");
}

fn team_render_lives(agent: &str, dir: &std::path::Path) -> String {
    use std::fmt::Write as _;
    let files = team_dir_names(dir);
    let mut out = format!("\n  \x1b[36;1m{agent} — past lives\x1b[0m\n\n");
    writeln!(out, "  standing orders: {}", if files.iter().any(|f| f == "standing-orders.md") { "\x1b[32myes\x1b[0m" } else { "\x1b[90mno\x1b[0m" }).expect("write string");
    let findings: Vec<_> = files.iter().filter(|f| f.ends_with("_findings.md")).collect();
    writeln!(out, "  findings: {}", if findings.is_empty() { "\x1b[90mnone\x1b[0m".to_owned() } else { format!("\x1b[32m{}\x1b[0m", findings.len()) }).expect("write string");
    for file in findings { writeln!(out, "    \x1b[90m{} ({} lines)\x1b[0m", file, team_line_count(&dir.join(file))).expect("write string"); }
    let other: Vec<_> = files.into_iter().filter(|f| f != "standing-orders.md" && !f.ends_with("_findings.md")).collect();
    if !other.is_empty() { writeln!(out, "  other: \x1b[90m{}\x1b[0m", other.join(", ")).expect("write string"); }
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
    let session = value["session"].as_str().map(str::to_owned);
    let members = value["members"].as_array().map_or_else(Vec::new, |items| items.iter().map(team_member_from_json).collect());
    let governance = value["governance"]["requires_human_approval"].as_bool().unwrap_or(false);
    team_charter_finish(&name, value["description"].as_str().unwrap_or("").to_owned(), value["goal"].as_str().unwrap_or("").to_owned(), session, members, governance)
}

fn team_member_from_json(value: &serde_json::Value) -> TeamCharterMember122 {
    TeamCharterMember122 { role: value["role"].as_str().unwrap_or("").to_owned(), name: value["name"].as_str().map(str::to_owned), model: value["model"].as_str().map(str::to_owned), cwd: value["cwd"].as_str().map(str::to_owned), engine: value["engine"].as_str().map(str::to_owned), target: value["target"].as_str().map(str::to_owned), prompt: value["prompt"].as_str().map(str::to_owned), worktree: value["worktree"].as_str().map(str::to_owned).or_else(|| value["worktree"].as_bool().map(|v| v.to_string())) }
}

fn team_parse_yaml_charter(text: &str) -> Result<TeamCharter122, String> {
    let mut charter = TeamCharter122::default();
    let mut current: Option<TeamCharterMember122> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() { continue; }
        team_yaml_line(line, &mut charter, &mut current);
    }
    if let Some(member) = current.take() { charter.members.push(member); }
    team_charter_finish(&charter.name, charter.description, charter.goal, charter.session, charter.members, charter.governance_requires_human_approval)
}

fn team_yaml_line(line: &str, charter: &mut TeamCharter122, current: &mut Option<TeamCharterMember122>) {
    if let Some(rest) = line.strip_prefix("name:") { charter.name = team_unquote(rest); return; }
    if let Some(rest) = line.strip_prefix("description:") { charter.description = team_unquote(rest); return; }
    if let Some(rest) = line.strip_prefix("goal:") { charter.goal = team_unquote(rest); return; }
    if let Some(rest) = line.strip_prefix("session:") { charter.session = Some(team_unquote(rest)); return; }
    if line.trim() == "requires_human_approval: true" { charter.governance_requires_human_approval = true; return; }
    if let Some(rest) = line.trim_start().strip_prefix("- role:") { if let Some(member) = current.take() { charter.members.push(member); } *current = Some(TeamCharterMember122 { role: team_unquote(rest), ..Default::default() }); return; }
    if let Some(member) = current.as_mut() { team_yaml_member_line(line, member); }
}

fn team_yaml_member_line(line: &str, member: &mut TeamCharterMember122) {
    for (key, slot) in [("name:", &mut member.name), ("model:", &mut member.model), ("cwd:", &mut member.cwd), ("engine:", &mut member.engine), ("target:", &mut member.target), ("prompt:", &mut member.prompt), ("worktree:", &mut member.worktree)] {
        if let Some(rest) = line.trim_start().strip_prefix(key) { *slot = Some(team_unquote(rest)); }
    }
}

fn team_unquote(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_owned()
}

fn team_charter_finish(name: &str, description: String, goal: String, session: Option<String>, members: Vec<TeamCharterMember122>, governance_requires_human_approval: bool) -> Result<TeamCharter122, String> {
    if name.trim().is_empty() { return Err("team charter requires name".to_owned()); }
    if members.is_empty() { return Err("team charter requires at least one member".to_owned()); }
    Ok(TeamCharter122 { name: name.trim().to_owned(), description, goal, session, members, governance_requires_human_approval })
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
    if !charter.description.is_empty() { writeln!(out, "description: {}", charter.description).expect("write string"); }
    if !charter.goal.is_empty() { writeln!(out, "goal: {}", charter.goal.lines().next().unwrap_or("")).expect("write string"); }
    writeln!(out, "\nmembers ({}):", charter.members.len()).expect("write string");
    for member in &charter.members { writeln!(out, "  - {} ({})", member.role, team_member_bits(member)).expect("write string"); }
    writeln!(out, "\nwould prepare artifacts:").expect("write string");
    for artifact in artifacts { writeln!(out, "  - {}", artifact.display()).expect("write string"); }
    writeln!(out, "\nphase-0 safety:").expect("write string");
    for action in actions { writeln!(out, "  - {action}").expect("write string"); }
    out
}

fn team_member_bits(member: &TeamCharterMember122) -> String {
    let mut bits = vec![format!("target={}", member.target.as_deref().unwrap_or("auto"))];
    if let Some(name) = &member.name { bits.push(format!("name={name}")); }
    if let Some(model) = &member.model { bits.push(format!("model={model}")); }
    if let Some(cwd) = &member.cwd { bits.push(format!("cwd={cwd}")); }
    if let Some(worktree) = &member.worktree { bits.push(format!("worktree={worktree}")); }
    if let Some(engine) = &member.engine { bits.push(format!("engine={engine}")); }
    bits.join(", ")
}

fn team_format_preflight(charter: &TeamCharter122) -> (String, bool) {
    use std::fmt::Write as _;
    let mut checks = Vec::new();
    checks.push((team_validate_name(&charter.name).is_ok(), "team name".to_owned(), format!("'{}' is accepted", charter.name)));
    checks.push((team_unique_roles(&charter.members), "member roles".to_owned(), format!("{} unique role(s)", charter.members.len())));
    let collisions = team_plan_artifacts(charter).into_iter().filter(|path| path.exists()).map(|p| p.display().to_string()).collect::<Vec<_>>();
    checks.push((collisions.is_empty(), "existing artifacts".to_owned(), if collisions.is_empty() { "no config/inbox/manifest collisions found".to_owned() } else { format!("would refuse to overwrite: {}", collisions.join(", ")) }));
    let errors = checks.iter().any(|(ok, _, _)| !ok);
    let mut out = format!("team charter preflight: {}\nstatus: {}\n\nchecks:\n", charter.name, if errors { "failed" } else { "passed" });
    for (ok, label, detail) in checks { writeln!(out, "  {} {label}: {detail}", if ok { "✓" } else { "✗" }).expect("write string"); }
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
    let manifest = serde_json::json!({"name":charter.name,"createdAt":created_at,"description":charter.description,"goal":charter.goal,"members":charter.members.iter().map(|m| m.role.clone()).collect::<Vec<_>>(),"source":"team-charter"});
    team_write_json_atomic_0600(&paths.tool_config, &config)?;
    for member in &charter.members { team_write_json_atomic_0600(&paths.tool_dir.join("inboxes").join(format!("{}.json", member.role)), &serde_json::json!([]))?; }
    team_write_json_atomic_0600(&paths.vault_manifest, &manifest)?;
    Ok(team_format_load(charter, &team_plan_artifacts(charter)))
}

fn team_config_member_from_charter(member: &TeamCharterMember122) -> TeamMember122 {
    TeamMember122 { name: member.role.clone(), model: member.model.clone(), backend_type: member.target.as_ref().filter(|t| t.as_str() != "auto").cloned(), ..Default::default() }
}

fn team_format_load(charter: &TeamCharter122, artifacts: &[std::path::PathBuf]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team charter loaded: {}\n\nwrote artifacts:\n", charter.name);
    for artifact in artifacts { writeln!(out, "  - {}", artifact.display()).expect("write string"); }
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
    fn team_dispatch_fragment_moved_to_part240() {
        assert!(DISPATCH_122.is_empty());
        assert_eq!(DISPATCH_240[0].command, "team");
    }
}
