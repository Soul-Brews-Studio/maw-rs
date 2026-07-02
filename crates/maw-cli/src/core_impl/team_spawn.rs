const DISPATCH_127: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, Default)]
struct TeamT5SpawnOptions127 {
    team: String,
    role: String,
    engine: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    prompt: Option<String>,
    exec: bool,
    parent_session_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TeamT5SpawnFromOptions127 { path: String, approve: bool, exec: bool }

fn team_t5_spawn(argv: &[String]) -> Result<String, String> {
    let opts = team_t5_parse_spawn(argv)?;
    team_t5_spawn_one(&opts)
}

fn team_t5_spawn_from(argv: &[String]) -> Result<String, String> {
    let opts = team_t5_parse_spawn_from(argv)?;
    let charter = team_read_charter_path(&opts.path)?;
    if charter.governance_requires_human_approval && !opts.approve {
        return Err("governance requires human approval; re-run with --approve to spawn local target:auto members".to_owned());
    }
    let unsupported = charter.members.iter().filter(|m| m.target.as_deref().unwrap_or("auto") != "auto").map(|m| format!("{}={}", m.role, m.target.as_deref().unwrap_or("auto"))).collect::<Vec<_>>();
    if !unsupported.is_empty() { return Err(format!("charter spawn currently supports only target:auto; blocked {}", unsupported.join(", "))); }

    team_validate_name(&charter.name)?;
    team_create(&["create".to_owned(), charter.name.clone(), "--description".to_owned(), charter.description.clone()])?;
    let mut spawned = Vec::new();
    for member in &charter.members {
        let prompt = team_t5_member_prompt(&charter, member);
        let spawn = TeamT5SpawnOptions127 { team: charter.name.clone(), role: member.role.clone(), engine: member.engine.clone(), model: member.model.clone(), cwd: member.cwd.clone(), prompt, exec: opts.exec, ..Default::default() };
        team_t5_spawn_one(&spawn)?;
        spawned.push(member.role.clone());
    }
    Ok(team_t5_format_spawn_from(&charter, &spawned, &opts))
}

fn team_t5_parse_spawn(argv: &[String]) -> Result<TeamT5SpawnOptions127, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw team spawn <team> <role> [--engine <engine>] [--model <model>] [--cwd <path>] [--worktree <path>] [--prompt <text>] [--parent-session-id <id>] [--session-id <id>] [--exec]".to_owned())?.clone();
    let role = argv.get(2).ok_or_else(|| "usage: maw team spawn <team> <role> [--engine <engine>] [--model <model>] [--cwd <path>] [--worktree <path>] [--prompt <text>] [--parent-session-id <id>] [--session-id <id>] [--exec]".to_owned())?.clone();
    team_validate_name(&team)?;
    team_validate_name(&role)?;
    let mut opts = TeamT5SpawnOptions127 { team, role, ..Default::default() };
    let mut i = 3;
    while i < argv.len() {
        match argv[i].as_str() {
            "--engine" | "-e" => { i += 1; opts.engine = Some(team_t5_safe_token(team_t5_next(argv, i, "--engine")?, "engine")?); },
            "--model" => { i += 1; opts.model = Some(team_t5_safe_token(team_t5_next(argv, i, "--model")?, "model")?); },
            "--cwd" | "--worktree" => { i += 1; opts.cwd = Some(team_t5_next(argv, i, "--cwd")?); },
            "--parent" | "--parent-session-id" => { i += 1; opts.parent_session_id = Some(team_t5_safe_token(team_t5_next(argv, i, "--parent-session-id")?, "parent-session-id")?); },
            "--session-id" => { i += 1; opts.session_id = Some(team_t5_safe_token(team_t5_next(argv, i, "--session-id")?, "session-id")?); },
            "--exec" => opts.exec = true,
            "--prompt" => { opts.prompt = Some(team_t5_collect_prompt(argv, i + 1)?); break; },
            other if other.starts_with('-') => return Err(format!("team spawn: unknown argument {other}")),
            other => return Err(format!("team spawn: unexpected argument {other}")),
        }
        i += 1;
    }
    if let Some(cwd) = &opts.cwd { team_t5_validate_work_path(cwd)?; }
    Ok(opts)
}

fn team_t5_parse_spawn_from(argv: &[String]) -> Result<TeamT5SpawnFromOptions127, String> {
    let path = argv.get(1).ok_or_else(|| "usage: maw team spawn-from <team.yaml|team.json> [--approve] [--exec]".to_owned())?.clone();
    team_validate_path_arg(&path)?;
    let mut opts = TeamT5SpawnFromOptions127 { path, ..Default::default() };
    for arg in argv.iter().skip(2) {
        match arg.as_str() {
            "--approve" => opts.approve = true,
            "--exec" => opts.exec = true,
            other if other.starts_with('-') => return Err(format!("team spawn-from: unknown argument {other}")),
            other => return Err(format!("team spawn-from: unexpected argument {other}")),
        }
    }
    Ok(opts)
}

fn team_t5_spawn_one(opts: &TeamT5SpawnOptions127) -> Result<String, String> {
    use std::fmt::Write as _;
    let paths = team_paths(&opts.team);
    if !paths.vault_manifest.exists() { return Err(format!("team '{}' not found — run: maw team create {}", opts.team, opts.team)); }
    let engine = opts.engine.clone().unwrap_or_else(|| "claude".to_owned());
    team_t5_safe_token(&engine, "engine")?;
    if let Some(model) = &opts.model { team_t5_safe_token(model, "model")?; }
    let cwd = opts.cwd.as_deref().map(team_t5_canonical_work_path).transpose()?;
    let prompt = team_t5_spawn_prompt(&opts.team, &opts.role, opts.prompt.as_deref(), &opts.role);
    let prompt_path = paths.vault_dir.join(format!("{}-spawn-prompt.md", opts.role));
    team_atomic_write_0600(&prompt_path, &prompt)?;
    team_t5_upsert_manifest_member(&paths.vault_manifest, &opts.role)?;
    team_t5_upsert_tool_member(&paths.tool_config, &opts.role, &engine, opts.model.as_deref())?;

    let mut out = format!("\x1b[32m✓\x1b[0m spawn prompt written for '{}'\n  \x1b[90mpast life: no\x1b[0m\n  \x1b[90mengine: {engine}\x1b[0m\n", opts.role);
    if let Some(model) = &opts.model { let _ = writeln!(out, "  \x1b[90mmodel: {model}\x1b[0m"); }
    let _ = write!(out, "  \x1b[90mprompt: {}\x1b[0m\n\n", prompt_path.display());
    let invocation = team_t5_controlled_maw_invocation(opts, &engine, cwd.as_deref());
    if opts.exec {
        team_t5_spawn_controlled(&invocation, cwd.as_deref())?;
        let _ = writeln!(out, "  \x1b[32m✓ --exec\x1b[0m spawned {} with controlled maw invocation", opts.role);
    } else {
        let _ = writeln!(out, "  \x1b[36mRun:\x1b[0m {}", team_t5_render_command(&invocation));
    }
    Ok(out)
}

fn team_t5_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    argv.get(index).cloned().ok_or_else(|| format!("team spawn: {flag} requires a value"))
}

fn team_t5_collect_prompt(argv: &[String], start: usize) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut i = start;
    while i < argv.len() {
        match argv[i].as_str() {
            "--exec" => {},
            "--engine" | "-e" | "--model" | "--cwd" | "--worktree" | "--parent" | "--parent-session-id" | "--session-id" => i += 1,
            value => tail.push(value.to_owned()),
        }
        i += 1;
    }
    let prompt = tail.join(" ");
    if prompt.chars().any(|ch| ch == '\0') { return Err("invalid team prompt: NUL rejected".to_owned()); }
    if prompt.is_empty() { Err("team spawn: --prompt requires text".to_owned()) } else { Ok(prompt) }
}

fn team_t5_safe_token(value: impl AsRef<str>, label: &str) -> Result<String, String> {
    let value = value.as_ref();
    if value.is_empty() { return Err(format!("team {label} is empty")); }
    if value.starts_with('-') { return Err(format!("invalid team {label} '{value}': leading dash rejected")); }
    if value.contains("..") || value.contains('/') || value.contains('\\') { return Err(format!("invalid team {label} '{value}': path traversal rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid team {label}: control character rejected")); }
    Ok(value.to_owned())
}

fn team_t5_validate_work_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.starts_with('-') || path.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid worktree path {path:?}")); }
    if path.split(['/', '\\']).any(|part| part == "..") { return Err(format!("invalid worktree path {path:?}: traversal rejected")); }
    Ok(())
}

fn team_t5_canonical_work_path(path: &str) -> Result<std::path::PathBuf, String> {
    team_t5_validate_work_path(path)?;
    let raw = std::path::PathBuf::from(path);
    let full = if raw.is_absolute() { raw } else { std::env::current_dir().map_err(|error| error.to_string())?.join(raw) };
    let canonical = full.canonicalize().map_err(|error| format!("team spawn: canonicalize {} failed: {error}", full.display()))?;
    let root = team_t5_repo_root(&std::env::current_dir().map_err(|error| error.to_string())?)?.canonicalize().map_err(|error| format!("team spawn: canonicalize repo root failed: {error}"))?;
    if !canonical.starts_with(&root) { return Err(format!("invalid worktree path {}: outside repo root {}", canonical.display(), root.display())); }
    Ok(canonical)
}

fn team_t5_repo_root(start: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() { return Ok(dir); }
        let Some(parent) = dir.parent() else { return Err("team spawn: repo root not found (.git)".to_owned()); };
        if parent == dir { return Err("team spawn: repo root not found (.git)".to_owned()); }
        dir = parent.to_path_buf();
    }
}

fn team_t5_spawn_prompt(team: &str, role: &str, prompt: Option<&str>, fallback: &str) -> String {
    let mut parts = vec![format!("You are '{role}' on team '{team}'.")];
    if let Some(prompt) = prompt.filter(|p| !p.trim().is_empty()) { parts.push(prompt.to_owned()); }
    if parts.len() == 1 && !fallback.is_empty() { let _ = fallback; }
    parts.join("\n\n")
}

fn team_t5_member_prompt(charter: &TeamCharter122, member: &TeamCharterMember122) -> Option<String> {
    let mut parts = Vec::new();
    if !charter.goal.trim().is_empty() { parts.push(format!("## Team goal\n{}", charter.goal)); }
    if let Some(prompt) = member.prompt.as_ref().filter(|p| !p.trim().is_empty()) { parts.push(format!("## Role prompt\n{prompt}")); }
    if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
}

fn team_t5_upsert_manifest_member(path: &std::path::Path, role: &str) -> Result<(), String> {
    let mut value: serde_json::Value = team_read_json(path).ok_or_else(|| format!("team spawn: invalid manifest {}", path.display()))?;
    let members = value["members"].as_array_mut().ok_or_else(|| "team spawn: manifest members must be an array".to_owned())?;
    if !members.iter().any(|item| item.as_str() == Some(role)) { members.push(serde_json::json!(role)); }
    team_write_json_atomic_0600(path, &value)
}

fn team_t5_upsert_tool_member(path: &std::path::Path, role: &str, engine: &str, model: Option<&str>) -> Result<(), String> {
    let Some(mut config) = team_read_json::<TeamConfig122>(path) else { return Ok(()); };
    if !config.members.iter().any(|member| member.name == role) {
        config.members.push(TeamMember122 { name: role.to_owned(), model: model.map(str::to_owned), backend_type: (engine != "claude").then(|| engine.to_owned()), ..Default::default() });
        team_write_json_atomic_0600(path, &config)?;
    }
    Ok(())
}

fn team_t5_controlled_maw_invocation(opts: &TeamT5SpawnOptions127, engine: &str, cwd: Option<&std::path::Path>) -> Vec<String> {
    let mut args = vec!["wake".to_owned(), opts.role.clone(), "--no-attach".to_owned(), "--session".to_owned(), opts.team.clone(), "-e".to_owned(), engine.to_owned()];
    if let Some(cwd) = cwd { args.extend(["--repo-path".to_owned(), cwd.display().to_string()]); }
    if let Some(id) = &opts.parent_session_id { args.extend(["--parent-session-id".to_owned(), id.clone()]); }
    if let Some(id) = &opts.session_id { args.extend(["--session-id".to_owned(), id.clone()]); }
    args
}

fn team_t5_spawn_controlled(args: &[String], cwd: Option<&std::path::Path>) -> Result<(), String> {
    use std::process::Stdio;
    if let Ok(log) = std::env::var("MAW_RS_TEAM_FAKE_SPAWN_LOG") {
        let record = serde_json::json!({"program":team_t5_self_bin()?.display().to_string(),"args":args,"cwd":cwd.map(|p| p.display().to_string())});
        let path = std::path::Path::new(&log);
        let mut body = std::fs::read_to_string(path).unwrap_or_default();
        body.push_str(&record.to_string());
        body.push('\n');
        return team_atomic_write_0600(path, &body);
    }
    let mut child = std::process::Command::new(team_t5_self_bin()?).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).current_dir(cwd.unwrap_or_else(|| std::path::Path::new("."))).spawn().map_err(|error| format!("team spawn: controlled maw spawn failed: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().map_err(|error| format!("team spawn: wait failed: {error}"))? { return if status.success() { Ok(()) } else { Err(format!("team spawn: controlled maw exited with {status}")) }; }
        if std::time::Instant::now() >= deadline { let _ = child.kill(); return Err("team spawn: controlled maw timed out".to_owned()); }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn team_t5_self_bin() -> Result<std::path::PathBuf, String> {
    std::env::var_os("MAW_RS_SELF_BIN").map(std::path::PathBuf::from).map_or_else(|| std::env::current_exe().map_err(|error| format!("team spawn: current_exe failed: {error}")), Ok)
}

fn team_t5_render_command(args: &[String]) -> String {
    let mut parts = vec![team_t5_self_bin().map_or_else(|_| "maw".to_owned(), |p| p.display().to_string())];
    parts.extend(args.iter().map(|arg| if arg.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')) { arg.clone() } else { team_t5_shell_quote(arg) }));
    parts.join(" ")
}

fn team_t5_shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn team_t5_format_spawn_from(charter: &TeamCharter122, roles: &[String], opts: &TeamT5SpawnFromOptions127) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team charter spawn complete: {}\n\nroles ({}):\n", charter.name, roles.len());
    for role in roles { let _ = writeln!(out, "  - {role}"); }
    out.push_str("\nspawn safety:\n  - preflight passed\n");
    out.push_str(if opts.approve { "  - governance approval flag present\n" } else { "  - no governance approval required\n" });
    out.push_str(if opts.exec { "  - --exec passed through to local cmdTeamSpawn\n" } else { "  - spawn prompts written; no tmux panes spawned without --exec\n" });
    out.push_str("  - existing:* and new:* targets blocked in this implementation\n");
    out
}
