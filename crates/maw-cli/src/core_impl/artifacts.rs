const DISPATCH_71: &[DispatcherEntry] = &[
    DispatcherEntry { command: "artifacts", handler: Handler::Sync(run_artifacts_command) },
    DispatcherEntry { command: "artifact", handler: Handler::Sync(run_artifacts_command) },
];

const ARTIFACTS_USAGE: &str = "usage: maw artifacts [ls|get] [team] [task-id] [--json]";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArtifactsMeta {
    team: String,
    task_id: String,
    subject: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArtifactsSummary {
    team: String,
    task_id: String,
    subject: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    files: usize,
    has_result: bool,
    created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ArtifactsFull {
    meta: ArtifactsMeta,
    spec: String,
    result: Option<String>,
    attachments: Vec<String>,
    dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactsAction { List, Get }

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactsOptions { action: ArtifactsAction, json: bool, team: Option<String>, task_id: Option<String> }

fn run_artifacts_command(argv: &[String]) -> CliOutput {
    match artifacts_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn artifacts_run(argv: &[String]) -> Result<String, String> {
    let options = artifacts_parse_args(argv)?;
    match options.action {
        ArtifactsAction::List => artifacts_list(&options),
        ArtifactsAction::Get => artifacts_get(&options),
    }
}

fn artifacts_parse_args(argv: &[String]) -> Result<ArtifactsOptions, String> {
    let mut json = false;
    let mut positionals = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(ARTIFACTS_USAGE.to_owned()),
            "--json" => json = true,
            value if value.starts_with('-') => return Err(format!("artifacts: unknown argument {value}")),
            value => { artifacts_validate_value(value, "argument")?; positionals.push(value.to_owned()); },
        }
    }
    artifacts_options_from_positionals(json, &positionals)
}

fn artifacts_options_from_positionals(json: bool, positionals: &[String]) -> Result<ArtifactsOptions, String> {
    match positionals {
        [] => Ok(ArtifactsOptions { action: ArtifactsAction::List, json, team: None, task_id: None }),
        [sub] if matches!(sub.as_str(), "ls" | "list") => Ok(ArtifactsOptions { action: ArtifactsAction::List, json, team: None, task_id: None }),
        [sub, team] if matches!(sub.as_str(), "ls" | "list") => Ok(ArtifactsOptions { action: ArtifactsAction::List, json, team: Some(team.clone()), task_id: None }),
        [sub, team, task] if matches!(sub.as_str(), "get" | "show") => Ok(ArtifactsOptions { action: ArtifactsAction::Get, json, team: Some(team.clone()), task_id: Some(task.clone()) }),
        [sub, ..] if matches!(sub.as_str(), "get" | "show") => Err("usage: maw artifacts get <team> <task-id>".to_owned()),
        _ => Err(ARTIFACTS_USAGE.to_owned()),
    }
}

fn artifacts_validate_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.contains('/') || value.contains('\\') {
        return Err(format!("artifacts: invalid {label} {value}"));
    }
    Ok(())
}

fn artifacts_list(options: &ArtifactsOptions) -> Result<String, String> {
    let items = artifacts_list_items(options.team.as_deref())?;
    if options.json { return artifacts_json(&items); }
    if items.is_empty() { return Ok(format!("No artifacts found.{}\n", options.team.as_ref().map_or(String::new(), |team| format!(" (team: {team})")))); }
    Ok(artifacts_render_table(&items))
}

fn artifacts_get(options: &ArtifactsOptions) -> Result<String, String> {
    let team = options.team.as_ref().ok_or_else(|| "usage: maw artifacts get <team> <task-id>".to_owned())?;
    let task_id = options.task_id.as_ref().ok_or_else(|| "usage: maw artifacts get <team> <task-id>".to_owned())?;
    let artifact = artifacts_read_full(team, task_id)?.ok_or_else(|| format!("artifact not found: {team}/{task_id}"))?;
    if options.json { return artifacts_json(&artifact); }
    Ok(artifacts_render_full(&artifact))
}

fn artifacts_list_items(team_filter: Option<&str>) -> Result<Vec<ArtifactsSummary>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut results = Vec::new();
    for root in artifacts_roots_for_read() {
        if !root.is_dir() { continue; }
        let teams = artifacts_team_dirs(&root, team_filter);
        for team in teams { artifacts_collect_team(&root, &team, &mut seen, &mut results)?; }
    }
    results.sort_by(|a, b| a.team.cmp(&b.team).then(a.task_id.cmp(&b.task_id)));
    Ok(results)
}

fn artifacts_team_dirs(root: &std::path::Path, team_filter: Option<&str>) -> Vec<String> {
    if let Some(team) = team_filter { return vec![team.to_owned()]; }
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new(); };
    let mut teams = entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).filter_map(|entry| entry.file_name().into_string().ok()).collect::<Vec<_>>();
    teams.sort();
    teams
}

fn artifacts_collect_team(root: &std::path::Path, team: &str, seen: &mut std::collections::BTreeSet<String>, results: &mut Vec<ArtifactsSummary>) -> Result<(), String> {
    let team_dir = root.join(team);
    let Ok(entries) = std::fs::read_dir(&team_dir) else { return Ok(()); };
    let mut tasks = entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).collect::<Vec<_>>();
    tasks.sort_by_key(std::fs::DirEntry::file_name);
    for task in tasks { artifacts_collect_task(team, &task.path(), seen, results)?; }
    Ok(())
}

fn artifacts_collect_task(team: &str, task_dir: &std::path::Path, seen: &mut std::collections::BTreeSet<String>, results: &mut Vec<ArtifactsSummary>) -> Result<(), String> {
    let Some(task_id) = task_dir.file_name().and_then(std::ffi::OsStr::to_str) else { return Ok(()); };
    let key = format!("{team}\0{task_id}");
    if !seen.insert(key) { return Ok(()); }
    let meta_path = task_dir.join("meta.json");
    if !meta_path.is_file() { return Ok(()); }
    let meta = artifacts_read_meta(&meta_path)?;
    results.push(ArtifactsSummary { team: team.to_owned(), task_id: task_id.to_owned(), subject: meta.subject, status: meta.status, owner: meta.owner, files: artifacts_file_count(task_dir), has_result: task_dir.join("result.md").is_file(), created_at: meta.created_at });
    Ok(())
}

fn artifacts_read_full(team: &str, task_id: &str) -> Result<Option<ArtifactsFull>, String> {
    let Some(dir) = artifacts_existing_dir(team, task_id) else { return Ok(None); };
    let meta = artifacts_read_meta(&dir.join("meta.json"))?;
    let spec = std::fs::read_to_string(dir.join("spec.md")).unwrap_or_default();
    let result = std::fs::read_to_string(dir.join("result.md")).ok();
    let attachments = artifacts_attachments(&dir);
    Ok(Some(ArtifactsFull { meta, spec, result, attachments, dir: dir.display().to_string() }))
}

fn artifacts_existing_dir(team: &str, task_id: &str) -> Option<std::path::PathBuf> {
    for root in artifacts_roots_for_read() {
        let dir = root.join(team).join(task_id);
        if dir.join("meta.json").is_file() { return Some(dir); }
    }
    None
}

fn artifacts_roots_for_read() -> Vec<std::path::PathBuf> {
    let mut roots = vec![artifacts_root()];
    if let Some(legacy) = artifacts_legacy_root() { if legacy != roots[0] { roots.push(legacy); } }
    roots
}

fn artifacts_root() -> std::path::PathBuf { maw_cache_path(&current_xdg_env(), &["artifacts"]) }

fn artifacts_legacy_root() -> Option<std::path::PathBuf> {
    if std::env::var_os("MAW_HOME").is_some() || std::env::var_os("MAW_CACHE_DIR").is_some() { return None; }
    Some(std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from(".maw/artifacts"), |home| std::path::PathBuf::from(home).join(".maw/artifacts")))
}

fn artifacts_read_meta(path: &std::path::Path) -> Result<ArtifactsMeta, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("artifacts: cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("artifacts: invalid json {}: {error}", path.display()))
}

fn artifacts_file_count(task_dir: &std::path::Path) -> usize {
    let direct = std::fs::read_dir(task_dir).map_or(0, |entries| entries.flatten().count());
    let attachments = std::fs::read_dir(task_dir.join("attachments")).map_or(0, |entries| entries.flatten().count());
    direct + attachments
}

fn artifacts_attachments(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir.join("attachments")) else { return Vec::new(); };
    let mut names = entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect::<Vec<_>>();
    names.sort();
    names
}

fn artifacts_render_table(items: &[ArtifactsSummary]) -> String {
    let mut out = String::new();
    artifacts_push_padded(&mut out, "TEAM", 18); artifacts_push_padded(&mut out, "TASK", 6); artifacts_push_padded(&mut out, "STATUS", 12); artifacts_push_padded(&mut out, "OWNER", 16); artifacts_push_padded(&mut out, "FILES", 6); artifacts_push_padded(&mut out, "RESULT", 8); out.push_str("SUBJECT\n");
    out.push_str(&"-".repeat(90)); out.push('\n');
    for item in items { artifacts_push_row(&mut out, item); }
    out
}

fn artifacts_push_row(out: &mut String, item: &ArtifactsSummary) {
    artifacts_push_padded(out, &item.team, 18);
    artifacts_push_padded(out, &item.task_id, 6);
    artifacts_push_padded(out, &artifacts_color_status(&item.status), 12);
    artifacts_push_padded(out, item.owner.as_deref().unwrap_or("-"), 16);
    artifacts_push_padded(out, &item.files.to_string(), 6);
    artifacts_push_padded(out, if item.has_result { "\x1b[32myes\x1b[0m" } else { "\x1b[90mno\x1b[0m" }, 8);
    out.push_str(&item.subject.chars().take(40).collect::<String>());
    out.push('\n');
}

fn artifacts_push_padded(out: &mut String, value: &str, width: usize) {
    out.push_str(value);
    let visible = artifacts_visible_len(value);
    if visible < width { out.push_str(&" ".repeat(width - visible)); }
}

fn artifacts_visible_len(value: &str) -> usize {
    let mut len = 0;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' { for next in chars.by_ref() { if next == 'm' { break; } } } else { len += 1; }
    }
    len
}

fn artifacts_render_full(artifact: &ArtifactsFull) -> String {
    let mut out = format!("\x1b[1m{}\x1b[0m\n", artifact.meta.subject);
    let _ = writeln!(out, "Team: {} | Task: {} | Status: {}", artifact.meta.team, artifact.meta.task_id, artifacts_color_status(&artifact.meta.status));
    let _ = writeln!(out, "Owner: {} | Created: {}", artifact.meta.owner.as_deref().unwrap_or("-"), artifact.meta.created_at);
    if let Some(commit) = &artifact.meta.commit_hash { let _ = writeln!(out, "Commit: {commit}"); }
    out.push_str("\n\x1b[36m── spec.md ──\x1b[0m\n");
    out.push_str(artifact.spec.trim()); out.push_str("\n\n");
    artifacts_push_result(&mut out, artifact);
    artifacts_push_attachments(&mut out, &artifact.attachments);
    let _ = writeln!(out, "\n\x1b[90mDir: {}\x1b[0m", artifact.dir);
    out
}

fn artifacts_push_result(out: &mut String, artifact: &ArtifactsFull) {
    if let Some(result) = &artifact.result { out.push_str("\x1b[32m── result.md ──\x1b[0m\n"); out.push_str(result.trim()); out.push_str("\n\n"); } else { out.push_str("\x1b[90m(no result.md yet)\x1b[0m\n\n"); }
}

fn artifacts_push_attachments(out: &mut String, attachments: &[String]) {
    if attachments.is_empty() { return; }
    let _ = writeln!(out, "\x1b[33m── attachments ({}) ──\x1b[0m", attachments.len());
    for name in attachments { let _ = writeln!(out, "  {name}"); }
}

fn artifacts_color_status(status: &str) -> String {
    match status { "completed" => "\x1b[32mcompleted\x1b[0m".to_owned(), "in_progress" => "\x1b[33min_progress\x1b[0m".to_owned(), _ => status.to_owned() }
}

fn artifacts_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map(|mut out| { out.push('\n'); out }).map_err(|error| error.to_string())
}

#[cfg(test)]
mod artifacts_tests {
    use super::*;

    #[test]
    fn artifacts_parser_accepts_alias_shape_and_json() {
        let argv = vec!["get".to_owned(), "team".to_owned(), "42".to_owned(), "--json".to_owned()];
        let opts = artifacts_parse_args(&argv).unwrap();
        assert_eq!(opts.action, ArtifactsAction::Get);
        assert!(opts.json);
        assert_eq!(opts.team.as_deref(), Some("team"));
    }

    #[test]
    fn artifacts_parser_rejects_leading_dash_values() {
        let argv = vec!["get".to_owned(), "-team".to_owned(), "42".to_owned()];
        assert_eq!(artifacts_parse_args(&argv).unwrap_err(), "artifacts: unknown argument -team");
    }

    #[test]
    fn artifacts_visible_padding_ignores_ansi() {
        assert_eq!(artifacts_visible_len("\x1b[32myes\x1b[0m"), 3);
    }
}
