const DISPATCH_241: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
enum TeamTaskOp241 {
    Add { team: String, subject: String, description: Option<String>, assign: Option<String> },
    List { team: String },
    Done { team: String, id: u64 },
    Assign { team: String, id: u64, agent: String },
}

fn team_task_ops241(argv: &[String]) -> Result<String, String> {
    match team_task_parse241(argv)? {
        TeamTaskOp241::Add { team, subject, description, assign } => team_task_add241(&team, &subject, description.as_deref(), assign.as_deref()),
        TeamTaskOp241::List { team } => Ok(team_task_list241(&team)),
        TeamTaskOp241::Done { team, id } => team_task_done241(&team, id),
        TeamTaskOp241::Assign { team, id, agent } => team_task_assign241(&team, id, &agent),
    }
}

fn team_task_parse241(argv: &[String]) -> Result<TeamTaskOp241, String> {
    let sub = argv.first().map_or("tasks", String::as_str);
    match sub {
        "add" | "task" => {
            let parsed = team_task_parse_flags241(&argv[1..], &["--team", "--assign", "--description"])?;
            let subject = parsed.positionals.join(" ");
            team_validate_task_text241(&subject, "subject")?;
            let team = parsed.team.unwrap_or_else(team_task_context_team241);
            team_validate_name(&team)?;
            if let Some(assign) = &parsed.assign { team_validate_task_member241(assign)?; }
            if let Some(description) = &parsed.description { team_validate_task_text241(description, "description")?; }
            Ok(TeamTaskOp241::Add { team, subject, description: parsed.description, assign: parsed.assign })
        }
        "tasks" => {
            let parsed = team_task_parse_flags241(&argv[1..], &["--team"])?;
            let team = parsed.team.or_else(|| parsed.positionals.first().cloned()).unwrap_or_else(team_task_context_team241);
            team_validate_name(&team)?;
            Ok(TeamTaskOp241::List { team })
        }
        "done" => {
            let parsed = team_task_parse_flags241(&argv[1..], &["--team"])?;
            let id = parsed.positionals.first().ok_or_else(|| "usage: maw team done <task-id> [--team <name>]".to_owned()).and_then(|raw| team_parse_task_id241(raw))?;
            let team = parsed.team.unwrap_or_else(team_task_context_team241);
            team_validate_name(&team)?;
            Ok(TeamTaskOp241::Done { team, id })
        }
        "assign" => {
            let parsed = team_task_parse_flags241(&argv[1..], &["--team"])?;
            let id = parsed.positionals.first().ok_or_else(|| "usage: maw team assign <task-id> <agent> [--team <name>]".to_owned()).and_then(|raw| team_parse_task_id241(raw))?;
            let agent = parsed.positionals.get(1).ok_or_else(|| "usage: maw team assign <task-id> <agent> [--team <name>]".to_owned())?.clone();
            team_validate_task_member241(&agent)?;
            let team = parsed.team.unwrap_or_else(team_task_context_team241);
            team_validate_name(&team)?;
            Ok(TeamTaskOp241::Assign { team, id, agent })
        }
        _ => Err(TEAM_USAGE.to_owned()),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TeamTaskFlags241 {
    team: Option<String>,
    assign: Option<String>,
    description: Option<String>,
    positionals: Vec<String>,
}

fn team_task_parse_flags241(args: &[String], allowed: &[&str]) -> Result<TeamTaskFlags241, String> {
    let mut parsed = TeamTaskFlags241::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some((flag, value)) = arg.split_once('=') {
            if !allowed.contains(&flag) { return Err(format!("team: unknown argument {flag}")); }
            team_task_set_flag241(&mut parsed, flag, value)?;
        } else if allowed.contains(&arg.as_str()) {
            index += 1;
            let value = args.get(index).ok_or_else(|| format!("{arg} requires a value"))?;
            team_task_set_flag241(&mut parsed, arg, value)?;
        } else if arg.starts_with('-') {
            return Err(format!("invalid team argument {arg}: leading dash rejected"));
        } else {
            parsed.positionals.push(arg.clone());
        }
        index += 1;
    }
    Ok(parsed)
}

fn team_task_set_flag241(parsed: &mut TeamTaskFlags241, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "--team" => { team_validate_name(value)?; parsed.team = Some(value.to_owned()); }
        "--assign" => { team_validate_task_member241(value)?; parsed.assign = Some(value.to_owned()); }
        "--description" => { team_validate_task_text241(value, "description")?; parsed.description = Some(value.to_owned()); }
        _ => return Err(format!("team: unknown argument {flag}")),
    }
    Ok(())
}

fn team_task_context_team241() -> String {
    if let Ok(team) = std::env::var("MAW_TEAM").map(|value| value.trim().to_owned()) { if !team.is_empty() { return team; } }
    let teams_dir = team_home_dir().join(".claude").join("teams");
    let mut live = Vec::new();
    if let Ok(entries) = std::fs::read_dir(teams_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().join("config.json").exists() { live.push(name); }
        }
    }
    live.sort();
    if live.len() == 1 { live.remove(0) } else { "default".to_owned() }
}

fn team_task_add241(team: &str, subject: &str, description: Option<&str>, assign: Option<&str>) -> Result<String, String> {
    team_task_ensure_dir241(team)?;
    let id = team_task_next_id241(team)?;
    let now = team_task_timestamp241();
    let mut task = serde_json::Map::new();
    task.insert("id".to_owned(), serde_json::json!(id));
    task.insert("subject".to_owned(), serde_json::json!(subject));
    if let Some(description) = description { task.insert("description".to_owned(), serde_json::json!(description)); }
    task.insert("status".to_owned(), serde_json::json!("pending"));
    if let Some(assign) = assign { task.insert("assignee".to_owned(), serde_json::json!(assign)); }
    task.insert("createdAt".to_owned(), serde_json::json!(now));
    task.insert("updatedAt".to_owned(), serde_json::json!(now));
    team_task_write_value241(&team_task_path241(team, id), &serde_json::Value::Object(task))?;
    Ok(format!("\x1b[32m✓\x1b[0m task #{id} created: {subject}\n"))
}

fn team_task_list241(team: &str) -> String {
    use std::fmt::Write as _;
    let tasks = team_task_read_all241(team);
    if tasks.is_empty() { return format!("\x1b[36mℹ\x1b[0m no tasks for team \"{team}\"\n"); }
    let mut out = format!("\x1b[36mℹ\x1b[0m tasks for team \"{team}\" ({}):\n", tasks.len());
    for task in tasks {
        let id = team_task_id_from_value241(&task).unwrap_or(0);
        let subject = task.get("subject").and_then(serde_json::Value::as_str).unwrap_or("");
        let status = task.get("status").and_then(serde_json::Value::as_str).unwrap_or("pending");
        let assignee = task.get("assignee").and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).map_or_else(String::new, |value| format!(" → {value}"));
        writeln!(out, "  #{id}  [{}]  {subject}{assignee}", team_task_status_color241(status)).expect("write string");
    }
    out
}

fn team_task_done241(team: &str, id: u64) -> Result<String, String> {
    team_task_ensure_dir241(team)?;
    let Some(path) = team_task_existing_path241(team, id) else { return Ok(format!("\x1b[33m⚠\x1b[0m task #{id} not found in team \"{team}\"\n")); };
    let mut task = team_task_read_value241(&path).ok_or_else(|| format!("\x1b[33m⚠\x1b[0m task #{id} not found in team \"{team}\""))?;
    team_task_set_field241(&mut task, "status", serde_json::json!("completed"));
    team_task_set_field241(&mut task, "updatedAt", serde_json::json!(team_task_timestamp241()));
    team_task_write_value241(&team_task_path241(team, id), &task)?;
    Ok(format!("\x1b[32m✓\x1b[0m task #{id} marked completed\n"))
}

fn team_task_assign241(team: &str, id: u64, agent: &str) -> Result<String, String> {
    team_task_ensure_dir241(team)?;
    let Some(path) = team_task_existing_path241(team, id) else { return Ok(format!("\x1b[33m⚠\x1b[0m task #{id} not found in team \"{team}\"\n")); };
    let mut task = team_task_read_value241(&path).ok_or_else(|| format!("\x1b[33m⚠\x1b[0m task #{id} not found in team \"{team}\""))?;
    team_task_set_field241(&mut task, "assignee", serde_json::json!(agent));
    team_task_set_field241(&mut task, "status", serde_json::json!("in_progress"));
    team_task_set_field241(&mut task, "updatedAt", serde_json::json!(team_task_timestamp241()));
    team_task_write_value241(&team_task_path241(team, id), &task)?;
    Ok(format!("\x1b[32m✓\x1b[0m task #{id} assigned to {agent}\n"))
}

fn team_task_set_field241(task: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    if !task.is_object() { *task = serde_json::Value::Object(serde_json::Map::new()); }
    task.as_object_mut().expect("object").insert(key.to_owned(), value);
}

fn team_task_read_all241(team: &str) -> Vec<serde_json::Value> {
    let mut by_id = std::collections::BTreeMap::<u64, serde_json::Value>::new();
    let dirs = team_task_existing_dirs241(team);
    for dir in dirs.into_iter().rev() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue; };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("_counter.json") || path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") { continue; }
            let Some(task) = team_task_read_value241(&path) else { continue; };
            if let Some(id) = team_task_id_from_value241(&task) { by_id.insert(id, task); }
        }
    }
    by_id.into_values().collect()
}

fn team_task_id_from_value241(task: &serde_json::Value) -> Option<u64> { task.get("id").and_then(serde_json::Value::as_u64) }

fn team_task_next_id241(team: &str) -> Result<u64, String> {
    let primary = team_task_counter_path241(team);
    let read_path = if primary.exists() { primary.clone() } else { team_task_legacy_counter_path241(team) };
    let mut counter = if read_path.exists() { team_task_read_value241(&read_path).unwrap_or_else(|| serde_json::json!({"next":1})) } else { serde_json::json!({"next":1}) };
    let id = counter.get("next").and_then(serde_json::Value::as_u64).unwrap_or(1);
    team_task_set_field241(&mut counter, "next", serde_json::json!(id.saturating_add(1)));
    team_task_write_value241(&primary, &counter)?;
    Ok(id)
}

fn team_task_status_color241(status: &str) -> String {
    match status {
        "completed" => format!("\x1b[32m{status}\x1b[0m"),
        "in_progress" => format!("\x1b[36m{status}\x1b[0m"),
        _ => format!("\x1b[33m{status}\x1b[0m"),
    }
}

fn team_task_ensure_dir241(team: &str) -> Result<(), String> {
    let dir = team_task_dir241(team);
    std::fs::create_dir_all(&dir).map_err(|error| format!("team task: create {} failed: {error}", dir.display()))
}

fn team_task_existing_dirs241(team: &str) -> Vec<std::path::PathBuf> {
    let primary = team_task_dir241(team);
    let legacy = team_task_legacy_dir241(team);
    let mut out = Vec::new();
    if primary.exists() { out.push(primary.clone()); }
    if legacy != primary && legacy.exists() { out.push(legacy); }
    out
}

fn team_task_existing_path241(team: &str, id: u64) -> Option<std::path::PathBuf> {
    let primary = team_task_path241(team, id);
    if primary.exists() { return Some(primary); }
    let legacy = team_task_legacy_path241(team, id);
    legacy.exists().then_some(legacy)
}

fn team_task_read_value241(path: &std::path::Path) -> Option<serde_json::Value> { std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()) }

fn team_task_write_value241(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value).map_err(|error| format!("team task: encode json failed: {error}"))? + "\n";
    team_task_atomic_write_0600_241(path, &body)
}

fn team_task_atomic_write_0600_241(path: &std::path::Path, body: &str) -> Result<(), String> {
    use std::io::Write as _;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt as _;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("team task: create {} failed: {error}", parent.display()))?;
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut file = opts.open(&tmp).map_err(|error| format!("team task: open {} failed: {error}", tmp.display()))?;
    file.write_all(body.as_bytes()).map_err(|error| format!("team task: write {} failed: {error}", tmp.display()))?;
    file.sync_all().map_err(|error| format!("team task: sync {} failed: {error}", tmp.display()))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|error| format!("team task: rename {} -> {} failed: {error}", tmp.display(), path.display()))
}

fn team_task_dir241(team: &str) -> std::path::PathBuf { maw_xdg::maw_state_dir(&current_xdg_env()).join("teams").join(team).join("tasks") }
fn team_task_legacy_dir241(team: &str) -> std::path::PathBuf { maw_xdg::maw_config_dir(&current_xdg_env()).join("teams").join(team).join("tasks") }
fn team_task_counter_path241(team: &str) -> std::path::PathBuf { team_task_dir241(team).join("_counter.json") }
fn team_task_legacy_counter_path241(team: &str) -> std::path::PathBuf { team_task_legacy_dir241(team).join("_counter.json") }
fn team_task_path241(team: &str, id: u64) -> std::path::PathBuf { team_task_dir241(team).join(format!("{id}.json")) }
fn team_task_legacy_path241(team: &str, id: u64) -> std::path::PathBuf { team_task_legacy_dir241(team).join(format!("{id}.json")) }

fn team_parse_task_id241(raw: &str) -> Result<u64, String> {
    team_validate_task_token241(raw, "task id")?;
    let id = raw.parse::<u64>().map_err(|_| "team task id must be numeric".to_owned())?;
    if id == 0 { return Err("team task id must be numeric".to_owned()); }
    Ok(id)
}

fn team_validate_task_member241(value: &str) -> Result<(), String> {
    team_validate_task_token241(value, "member")?;
    if value.contains("..") || value.contains('/') || value.contains('\\') { return Err(format!("invalid team member '{value}': path traversal rejected")); }
    Ok(())
}

fn team_validate_task_text241(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("team {label} is empty")); }
    if value.starts_with('-') { return Err(format!("invalid team {label}: leading dash rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid team {label}: control character rejected")); }
    Ok(())
}

fn team_validate_task_token241(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("team {label} is empty")); }
    if value.starts_with('-') { return Err(format!("invalid team {label}: leading dash rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid team {label}: control character rejected")); }
    Ok(())
}

fn team_task_timestamp241() -> String { std::env::var("MAW_RS_TEAM_FIXED_TIME").unwrap_or_else(|_| team_now_millis().to_string()) }

#[cfg(test)]
mod team_task_tests241 {
    use super::*;

    #[test]
    fn team_task_dispatch_fragment_declared_without_duplicate_team_owner() {
        assert!(DISPATCH_241.is_empty());
    }

    #[test]
    fn team_task_parse_rejects_injection_values() {
        assert!(team_task_parse241(&team_task_strings241(&["add", "-bad"])).expect_err("subject").contains("leading dash"));
        assert!(team_task_parse241(&team_task_strings241(&["assign", "1", "../bad"])).expect_err("member").contains("path traversal"));
        assert!(team_task_parse241(&team_task_strings241(&["done", "-1"])).expect_err("id").contains("leading dash"));
    }

    fn team_task_strings241(args: &[&str]) -> Vec<String> { args.iter().map(|arg| (*arg).to_owned()).collect() }
}
