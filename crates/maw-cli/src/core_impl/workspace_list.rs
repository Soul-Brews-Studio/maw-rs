const DISPATCH_140: &[DispatcherEntry] = &[
    DispatcherEntry { command: "workspace", handler: Handler::Sync(run_workspace_command) },
    DispatcherEntry { command: "ws", handler: Handler::Sync(run_workspace_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceConfig134 {
    id: String,
    name: String,
    hub_url: String,
    shared_agents: Vec<String>,
    joined_at: String,
    last_status: Option<String>,
}

fn run_workspace_command(argv: &[String]) -> CliOutput {
    CliOutput { code: 0, stdout: workspace_run(argv), stderr: String::new() }
}

fn workspace_run(argv: &[String]) -> String {
    let subcommand = argv.first().map_or_else(|| "ls".to_owned(), |arg| arg.to_lowercase());
    match subcommand.as_str() {
        "ls" | "list" | "" => workspace_render_ls(&workspace_load_all()),
        _ => workspace_help(),
    }
}

fn workspace_dirs() -> Vec<std::path::PathBuf> {
    let env = current_xdg_env();
    let primary = maw_data_path(&env, &["workspaces"]);
    let legacy = maw_config_path(&env, &["workspaces"]);
    if primary == legacy { vec![primary] } else { vec![primary, legacy] }
}

fn workspace_load_all() -> Vec<WorkspaceConfig134> {
    let mut by_id = std::collections::BTreeMap::<String, (usize, WorkspaceConfig134)>::new();
    let mut order = 0_usize;
    let mut dirs = workspace_dirs();
    dirs.reverse();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue; };
        let mut files = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
            .collect::<Vec<_>>();
        files.sort();
        for path in files {
            let Ok(raw) = std::fs::read_to_string(&path) else { continue; };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { continue; };
            let Some(workspace) = workspace_normalize(&value) else { continue; };
            let index = by_id.get(&workspace.id).map_or_else(|| { let current = order; order += 1; current }, |(existing, _)| *existing);
            by_id.insert(workspace.id.clone(), (index, workspace));
        }
    }
    let mut rows = by_id.into_values().collect::<Vec<_>>();
    rows.sort_by_key(|(index, _)| *index);
    rows.into_iter().map(|(_, workspace)| workspace).collect()
}

fn workspace_normalize(value: &serde_json::Value) -> Option<WorkspaceConfig134> {
    let object = value.as_object()?;
    let id = object.get("id")?.as_str()?.to_owned();
    if id.is_empty() { return None; }
    let name = object.get("name").and_then(serde_json::Value::as_str).unwrap_or("(unnamed)").to_owned();
    let hub_url = object.get("hubUrl").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
    let joined_at = object
        .get("joinedAt")
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("createdAt").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_owned();
    let shared_agents = object
        .get("sharedAgents")
        .and_then(serde_json::Value::as_array)
        .map(|agents| agents.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let last_status = object
        .get("lastStatus")
        .and_then(serde_json::Value::as_str)
        .filter(|status| matches!(*status, "connected" | "disconnected"))
        .map(str::to_owned);
    Some(WorkspaceConfig134 { id, name, hub_url, shared_agents, joined_at, last_status })
}

fn workspace_render_ls(workspaces: &[WorkspaceConfig134]) -> String {
    const CYAN_BOLD: &str = "\x1b[36;1m";
    const CYAN: &str = "\x1b[36m";
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const WHITE_BOLD: &str = "\x1b[37;1m";
    const DIM: &str = "\x1b[90m";
    const RESET: &str = "\x1b[0m";

    if workspaces.is_empty() {
        return format!(
            "{DIM}No workspaces configured.{RESET}\n{DIM}  maw workspace create <name>   Create a new workspace{RESET}\n{DIM}  maw workspace join <code>     Join with invite code{RESET}\n"
        );
    }

    let mut out = format!("\n{CYAN_BOLD}Workspaces{RESET}  {DIM}{} joined{RESET}\n\n", workspaces.len());
    for workspace in workspaces {
        let status_dot = if workspace.last_status.as_deref() == Some("connected") { format!("{GREEN}●{RESET}") } else { format!("{RED}●{RESET}") };
        let agent_count = workspace.shared_agents.len();
        let agent_label = if agent_count == 0 {
            format!("{DIM}no agents shared{RESET}")
        } else {
            format!("{agent_count} agent{} shared", if agent_count == 1 { "" } else { "s" })
        };
        let _ = writeln!(out, "  {status_dot}  {WHITE_BOLD}{}{RESET}  {DIM}({}){RESET}", workspace.name, workspace.id);
        let _ = writeln!(out, "     {CYAN}Hub:{RESET}     {}", workspace.hub_url);
        let _ = writeln!(out, "     {CYAN}Agents:{RESET}  {agent_label}");
        if !workspace.shared_agents.is_empty() {
            let _ = writeln!(out, "     {DIM}         {}{RESET}", workspace.shared_agents.join(", "));
        }
        let _ = writeln!(out, "     {DIM}Joined:  {}{RESET}", workspace.joined_at);
    }
    out.push('\n');
    out
}

fn workspace_help() -> String {
    "\x1b[36mmaw workspace\x1b[0m — Multi-node workspace management\n\n  maw workspace create <name>          Create workspace on hub\n  maw workspace join <code>            Join with invite code\n  maw workspace share <agent...>       Share agents to workspace\n  maw workspace unshare <agent...>     Remove agents from workspace\n  maw workspace ls                     List joined workspaces\n  maw workspace agents [workspace-id]  List all agents in workspace\n  maw workspace invite [workspace-id]  Show join code\n  maw workspace leave [workspace-id]   Leave workspace\n  maw workspace status                 Connection status to hub(s)\n\n\x1b[90mAlias: maw ws ...\x1b[0m\n".to_owned()
}
