const DISPATCH_72: &[DispatcherEntry] = &[
    DispatcherEntry { command: "artifact-manager", handler: Handler::Sync(run_artifactmgr_command) },
    DispatcherEntry { command: "art", handler: Handler::Sync(run_artifactmgr_command) },
];

const ARTIFACTMGR_USAGE: &str = "usage: maw art [ls|get|write|attach|init] [--json] [--team <team>]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ArtifactmgrOptions { subcommand: String, args: Vec<String>, json: bool, team: Option<String> }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArtifactmgrMeta {
    team: String,
    task_id: String,
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArtifactmgrSummary {
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
struct ArtifactmgrFull { meta: ArtifactmgrMeta, spec: String, result: Option<String>, attachments: Vec<String>, dir: String }

fn run_artifactmgr_command(argv: &[String]) -> CliOutput {
    match artifactmgr_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn artifactmgr_run(argv: &[String]) -> Result<String, String> {
    let options = artifactmgr_parse_args(argv)?;
    match options.subcommand.as_str() {
        "ls" | "list" => artifactmgr_run_list(&options),
        "get" | "show" => artifactmgr_run_get(&options),
        "write" => artifactmgr_run_write(&options),
        "attach" => artifactmgr_run_attach(&options),
        "init" | "create" => artifactmgr_run_create(&options),
        _ => Err(ARTIFACTMGR_USAGE.to_owned()),
    }
}

fn artifactmgr_parse_args(argv: &[String]) -> Result<ArtifactmgrOptions, String> {
    let mut options = ArtifactmgrOptions { subcommand: "ls".to_owned(), ..ArtifactmgrOptions::default() };
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if arg == "--" { artifactmgr_push_tail(argv, index + 1, &mut positionals)?; break; }
        match arg.as_str() {
            "--help" | "-h" => return Err(ARTIFACTMGR_USAGE.to_owned()),
            "--json" => options.json = true,
            "--team" => { options.team = Some(artifactmgr_take_value(argv, index, "--team")?); index += 2; continue; }
            value if value.starts_with("--team=") => options.team = Some(artifactmgr_eq_value(value, "--team")?),
            value if value.starts_with('-') => return Err(format!("artifact-manager: unknown argument {value}")),
            value => { artifactmgr_validate_value(value, "argument")?; positionals.push(value.to_owned()); }
        }
        index += 1;
    }
    if let Some(subcommand) = positionals.first() { options.subcommand.clone_from(subcommand); }
    options.args = positionals;
    Ok(options)
}

fn artifactmgr_push_tail(argv: &[String], start: usize, positionals: &mut Vec<String>) -> Result<(), String> {
    for value in &argv[start..] { artifactmgr_validate_value(value, "argument")?; positionals.push(value.clone()); }
    Ok(())
}

fn artifactmgr_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("artifact-manager: missing value for {flag}"))?;
    artifactmgr_validate_value(value, flag)?;
    Ok(value.clone())
}

fn artifactmgr_eq_value(arg: &str, flag: &str) -> Result<String, String> {
    let value = arg.split_once('=').map_or("", |(_, value)| value);
    artifactmgr_validate_value(value, flag)?;
    Ok(value.to_owned())
}

fn artifactmgr_validate_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("artifact-manager: empty value for {label}")); }
    if value.starts_with('-') { return Err(format!("artifact-manager: {label} value must not start with '-'")); }
    if value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) { return Err(format!("artifact-manager: invalid control character in {label}")); }
    Ok(())
}

fn artifactmgr_run_list(options: &ArtifactmgrOptions) -> Result<String, String> {
    let team = options.args.get(1).or(options.team.as_ref()).map(String::as_str);
    if let Some(team) = team { artifactmgr_validate_slug(team, "team")?; }
    let items = artifactmgr_list(team);
    if options.json { return artifactmgr_json(&items); }
    if items.is_empty() { return Ok("No artifacts.\n".to_owned()); }
    Ok(artifactmgr_render_list(&items))
}

fn artifactmgr_run_get(options: &ArtifactmgrOptions) -> Result<String, String> {
    let (team, task_id) = artifactmgr_team_task(options, "usage: maw art get <team> <task-id>")?;
    let artifact = artifactmgr_get(&team, &task_id).ok_or_else(|| format!("not found: {team}/{task_id}"))?;
    if options.json { return artifactmgr_json(&artifact); }
    Ok(artifactmgr_render_get(&artifact))
}

fn artifactmgr_run_write(options: &ArtifactmgrOptions) -> Result<String, String> {
    let (team, task_id) = artifactmgr_team_task(options, "usage: maw art write <team> <task-id> <message...>")?;
    let rest = options.args.get(3..).unwrap_or_default();
    if rest.is_empty() { return Err("usage: maw art write <team> <task-id> <message...>".to_owned()); }
    artifactmgr_write_result(&team, &task_id, &rest.join(" "))?;
    Ok(format!("\x1b[32m✓\x1b[0m result written → {}/result.md\n", artifactmgr_dir(&team, &task_id).display()))
}

fn artifactmgr_run_attach(options: &ArtifactmgrOptions) -> Result<String, String> {
    let (team, task_id) = artifactmgr_team_task(options, "usage: maw art attach <team> <task-id> <file-path>")?;
    let file_path = options.args.get(3).ok_or_else(|| "usage: maw art attach <team> <task-id> <file-path>".to_owned())?;
    artifactmgr_validate_value(file_path, "file-path")?;
    let data = std::fs::read(file_path).map_err(|error| error.to_string())?;
    let name = std::path::Path::new(file_path).file_name().and_then(|value| value.to_str()).unwrap_or("attachment");
    let dest = artifactmgr_add_attachment(&team, &task_id, name, &data)?;
    Ok(format!("\x1b[32m✓\x1b[0m attached → {}\n", dest.display()))
}

fn artifactmgr_run_create(options: &ArtifactmgrOptions) -> Result<String, String> {
    let (team, task_id) = artifactmgr_team_task(options, "usage: maw art init <team> <task-id> <subject> [description...]")?;
    let subject = options.args.get(3).ok_or_else(|| "usage: maw art init <team> <task-id> <subject> [description...]".to_owned())?;
    artifactmgr_validate_value(subject, "subject")?;
    let desc = options.args.get(4..).map_or_else(|| subject.clone(), |parts| parts.join(" "));
    let dir = artifactmgr_create(&team, &task_id, subject, &desc)?;
    Ok(format!("\x1b[32m✓\x1b[0m artifact created → {}\n", dir.display()))
}

fn artifactmgr_team_task(options: &ArtifactmgrOptions, usage: &str) -> Result<(String, String), String> {
    let team = options.args.get(1).ok_or_else(|| usage.to_owned())?;
    let task_id = options.args.get(2).ok_or_else(|| usage.to_owned())?;
    artifactmgr_validate_slug(team, "team")?;
    artifactmgr_validate_slug(task_id, "task-id")?;
    Ok((team.clone(), task_id.clone()))
}

fn artifactmgr_validate_slug(value: &str, label: &str) -> Result<(), String> {
    artifactmgr_validate_value(value, label)?;
    if value.contains('/') || value.contains('\\') || value == "." || value == ".." { return Err(format!("artifact-manager: invalid {label}")); }
    Ok(())
}

fn artifactmgr_list(team_filter: Option<&str>) -> Vec<ArtifactmgrSummary> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for root in artifactmgr_roots_for_read() { artifactmgr_collect_root(&root, team_filter, &mut seen, &mut items); }
    items.sort_by(|a, b| a.team.cmp(&b.team).then(a.task_id.cmp(&b.task_id)));
    items
}

fn artifactmgr_collect_root(root: &std::path::Path, team_filter: Option<&str>, seen: &mut BTreeSet<String>, items: &mut Vec<ArtifactmgrSummary>) {
    if !root.exists() { return; }
    let teams = artifactmgr_team_names(root, team_filter);
    for team in teams { artifactmgr_collect_team(root, &team, seen, items); }
}

fn artifactmgr_team_names(root: &std::path::Path, team_filter: Option<&str>) -> Vec<String> {
    if let Some(team) = team_filter { return vec![team.to_owned()]; }
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new(); };
    let mut names = entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>();
    names.sort();
    names
}

fn artifactmgr_collect_team(root: &std::path::Path, team: &str, seen: &mut BTreeSet<String>, items: &mut Vec<ArtifactmgrSummary>) {
    let team_dir = root.join(team);
    let Ok(entries) = std::fs::read_dir(&team_dir) else { return; };
    for entry in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
        if let Some(item) = artifactmgr_summary(&team_dir, team, &entry.file_name().to_string_lossy()) {
            let key = format!("{}\0{}", item.team, item.task_id);
            if seen.insert(key) { items.push(item); }
        }
    }
}

fn artifactmgr_summary(team_dir: &std::path::Path, team: &str, task_id: &str) -> Option<ArtifactmgrSummary> {
    let dir = team_dir.join(task_id);
    let meta = artifactmgr_read_meta(&dir)?;
    let attachments = artifactmgr_attachment_count(&dir);
    let files = std::fs::read_dir(&dir).ok()?.flatten().count() + attachments;
    Some(ArtifactmgrSummary { team: team.to_owned(), task_id: task_id.to_owned(), subject: meta.subject, status: meta.status, owner: meta.owner, files, has_result: dir.join("result.md").exists(), created_at: meta.created_at })
}

fn artifactmgr_get(team: &str, task_id: &str) -> Option<ArtifactmgrFull> {
    let dir = artifactmgr_existing_dir(team, task_id)?;
    let meta = artifactmgr_read_meta(&dir)?;
    let spec = std::fs::read_to_string(dir.join("spec.md")).unwrap_or_default();
    let result = std::fs::read_to_string(dir.join("result.md")).ok();
    let attachments = artifactmgr_attachment_names(&dir);
    Some(ArtifactmgrFull { meta, spec, result, attachments, dir: dir.display().to_string() })
}

fn artifactmgr_create(team: &str, task_id: &str, subject: &str, description: &str) -> Result<std::path::PathBuf, String> {
    let dir = artifactmgr_dir(team, task_id);
    std::fs::create_dir_all(dir.join("attachments")).map_err(|error| error.to_string())?;
    std::fs::write(dir.join("spec.md"), format!("# {subject}\n\n{description}\n")).map_err(|error| error.to_string())?;
    let now = artifactmgr_now_iso();
    let meta = ArtifactmgrMeta { team: team.to_owned(), task_id: task_id.to_owned(), subject: subject.to_owned(), owner: None, status: "pending".to_owned(), created_at: now.clone(), updated_at: now, commit_hash: None };
    artifactmgr_write_meta(&dir, &meta)?;
    Ok(dir)
}

fn artifactmgr_write_result(team: &str, task_id: &str, content: &str) -> Result<(), String> {
    let dir = artifactmgr_existing_dir(team, task_id).unwrap_or_else(|| artifactmgr_dir(team, task_id));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    std::fs::write(dir.join("result.md"), content).map_err(|error| error.to_string())?;
    artifactmgr_update_status(&dir, "completed")
}

fn artifactmgr_add_attachment(team: &str, task_id: &str, name: &str, data: &[u8]) -> Result<std::path::PathBuf, String> {
    let dir = artifactmgr_existing_dir(team, task_id).unwrap_or_else(|| artifactmgr_dir(team, task_id)).join("attachments");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let dest = dir.join(artifactmgr_safe_name(name));
    std::fs::write(&dest, data).map_err(|error| error.to_string())?;
    Ok(dest)
}

fn artifactmgr_update_status(dir: &std::path::Path, status: &str) -> Result<(), String> {
    let Some(mut meta) = artifactmgr_read_meta(dir) else { return Ok(()); };
    status.clone_into(&mut meta.status);
    meta.updated_at = artifactmgr_now_iso();
    artifactmgr_write_meta(dir, &meta)
}

fn artifactmgr_read_meta(dir: &std::path::Path) -> Option<ArtifactmgrMeta> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).ok()?).ok()
}

fn artifactmgr_write_meta(dir: &std::path::Path, meta: &ArtifactmgrMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta).map_err(|error| error.to_string())?;
    std::fs::write(dir.join("meta.json"), text).map_err(|error| error.to_string())
}

fn artifactmgr_existing_dir(team: &str, task_id: &str) -> Option<std::path::PathBuf> {
    artifactmgr_roots_for_read().into_iter().map(|root| root.join(team).join(task_id)).find(|dir| dir.join("meta.json").exists())
}

fn artifactmgr_roots_for_read() -> Vec<std::path::PathBuf> {
    let mut roots = vec![artifactmgr_root()];
    if let Some(legacy) = artifactmgr_legacy_root() { roots.push(legacy); }
    roots
}

fn artifactmgr_root() -> std::path::PathBuf { maw_cache_path(&current_xdg_env(), &["artifacts"]) }

fn artifactmgr_legacy_root() -> Option<std::path::PathBuf> {
    if std::env::var_os("MAW_HOME").is_some() || std::env::var_os("MAW_CACHE_DIR").is_some() { return None; }
    let legacy = current_xdg_env().home_dir().join(".maw").join("artifacts");
    (legacy != artifactmgr_root()).then_some(legacy)
}

fn artifactmgr_dir(team: &str, task_id: &str) -> std::path::PathBuf { artifactmgr_root().join(team).join(task_id) }

fn artifactmgr_attachment_count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join("attachments")).map_or(0, |entries| entries.flatten().count())
}

fn artifactmgr_attachment_names(dir: &std::path::Path) -> Vec<String> {
    let mut names = std::fs::read_dir(dir.join("attachments")).map_or_else(|_| Vec::new(), |entries| entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>());
    names.sort();
    names
}

fn artifactmgr_safe_name(name: &str) -> String {
    let safe = name.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') { ch } else { '_' }).collect::<String>();
    if safe.is_empty() { "attachment".to_owned() } else { safe }
}

fn artifactmgr_now_iso() -> String {
    std::env::var("MAW_TEST_NOW").unwrap_or_else(|_| contacts_now_iso8601())
}

fn artifactmgr_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn artifactmgr_render_list(items: &[ArtifactmgrSummary]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}{}{}{}{}{}SUBJECT", artifactmgr_col("TEAM", 16), artifactmgr_col("ID", 6), artifactmgr_col("STATUS", 12), artifactmgr_col("OWNER", 14), artifactmgr_col("FILES", 6), artifactmgr_col("RESULT", 8));
    let _ = writeln!(out, "{}", "─".repeat(80));
    for item in items { artifactmgr_render_summary(item, &mut out); }
    out
}

fn artifactmgr_render_summary(item: &ArtifactmgrSummary, out: &mut String) {
    let status = match item.status.as_str() { "completed" => "\x1b[32m✓\x1b[0m done", "in_progress" => "\x1b[33m⚡\x1b[0m wip", _ => "pending" };
    let result = if item.has_result { "\x1b[32myes\x1b[0m" } else { "\x1b[90m—\x1b[0m" };
    let _ = writeln!(out, "{}{}{}{}{}{}{}", artifactmgr_col(&item.team, 16), artifactmgr_col(&item.task_id, 6), artifactmgr_col(status, 12), artifactmgr_col(item.owner.as_deref().unwrap_or("—"), 14), artifactmgr_col(&item.files.to_string(), 6), artifactmgr_col(result, 8), artifactmgr_clip(&item.subject, 36));
}

fn artifactmgr_render_get(artifact: &ArtifactmgrFull) -> String {
    let mut out = format!("\x1b[1m{}\x1b[0m\n{} / {} · {} · {}\n", artifact.meta.subject, artifact.meta.team, artifact.meta.task_id, artifact.meta.status, artifact.meta.owner.as_deref().unwrap_or("unowned"));
    if let Some(commit) = &artifact.meta.commit_hash { let _ = writeln!(out, "commit: {commit}"); }
    out.push_str("\n\x1b[36m─── spec ───\x1b[0m\n");
    out.push_str(&artifactmgr_clip(artifact.spec.trim(), 500));
    if let Some(result) = &artifact.result { out.push_str("\n\n\x1b[32m─── result ───\x1b[0m\n"); out.push_str(&artifactmgr_clip(result.trim(), 1000)); }
    if !artifact.attachments.is_empty() { let _ = writeln!(out, "\n\x1b[33m─── attachments ({}) ───\x1b[0m", artifact.attachments.len()); for name in &artifact.attachments { let _ = writeln!(out, "  📎 {name}"); } }
    let _ = writeln!(out, "\n\x1b[90m{}\x1b[0m", artifact.dir);
    out
}

fn artifactmgr_col(s: &str, n: usize) -> String {
    let visible = artifactmgr_visible_len(s);
    if visible >= n { s.to_owned() } else { format!("{}{}", s, " ".repeat(n - visible)) }
}

fn artifactmgr_visible_len(s: &str) -> usize {
    let mut count = 0_usize;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' { while chars.next().is_some_and(|c| c != 'm') {} } else { count += 1; }
    }
    count
}

fn artifactmgr_clip(s: &str, n: usize) -> String { s.chars().take(n).collect() }

#[cfg(test)]
mod artifactmgr_tests {
    use super::*;

    const ENV_KEYS: &[&str] = &["HOME", "MAW_HOME", "MAW_XDG", "MAW_CACHE_DIR", "XDG_CACHE_HOME", "TMUX", "MAW_TEST_NOW"];

    struct ArtifactmgrEnv { root: std::path::PathBuf, saved: Vec<(&'static str, Option<std::ffi::OsString>)> }

    impl ArtifactmgrEnv {
        fn artifactmgr_new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("maw-rs-artifactmgr-{name}-{}", SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()));
            let home = root.join("home");
            let cache = root.join("cache");
            std::fs::create_dir_all(&home).expect("home");
            std::fs::create_dir_all(&cache).expect("cache");
            let saved = ENV_KEYS.iter().map(|key| (*key, std::env::var_os(key))).collect::<Vec<_>>();
            for key in ENV_KEYS { std::env::remove_var(key); }
            std::env::set_var("HOME", &home);
            std::env::set_var("MAW_XDG", "1");
            std::env::set_var("XDG_CACHE_HOME", &cache);
            std::env::set_var("MAW_TEST_NOW", "2026-06-24T00:00:00.000Z");
            Self { root, saved }
        }
    }

    impl Drop for ArtifactmgrEnv {
        fn drop(&mut self) {
            for key in ENV_KEYS { std::env::remove_var(key); }
            for (key, value) in self.saved.drain(..) { if let Some(value) = value { std::env::set_var(key, value); } }
        }
    }

    fn artifactmgr_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn artifactmgr_seed() -> ArtifactmgrEnv {
        let env = ArtifactmgrEnv::artifactmgr_new("seed");
        artifactmgr_create("team-a", "t1", "First task", "Spec body").expect("create one");
        artifactmgr_create("team-a", "t2", "Second task", "Other spec").expect("create two");
        artifactmgr_write_result("team-a", "t2", "done").expect("result");
        std::fs::write(artifactmgr_dir("team-a", "t2").join("attachments").join("note.txt"), "note").expect("attachment");
        env
    }

    #[test]
    fn artifactmgr_dispatch_fragment_registers_artifact_manager_and_art_alias() {
        let commands = DISPATCH_72.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, vec!["artifact-manager", "art"]);
    }

    #[test]
    fn artifactmgr_parse_flags_and_rejects_leading_dash_values() {
        let options = artifactmgr_parse_args(&artifactmgr_args(&["ls", "--json", "--team", "team-a"])).expect("parse");
        assert!(options.json);
        assert_eq!(options.team.as_deref(), Some("team-a"));
        assert!(artifactmgr_parse_args(&artifactmgr_args(&["ls", "--team", "--bad"])).expect_err("dash").contains("must not start"));
        assert!(artifactmgr_parse_args(&artifactmgr_args(&["get", "--", "team-a", "t1"])).is_ok());
    }

    #[test]
    fn artifactmgr_empty_list_is_hermetic() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = ArtifactmgrEnv::artifactmgr_new("empty");
        let output = run_artifactmgr_command(&artifactmgr_args(&["ls"]));
        assert_eq!(output.stdout, "No artifacts.\n");
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn artifactmgr_lists_seeded_artifacts_as_json() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = artifactmgr_seed();
        let output = run_artifactmgr_command(&artifactmgr_args(&["list", "team-a", "--json"]));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("\"taskId\": \"t2\""));
        assert!(output.stdout.contains("\"hasResult\": true"));
    }

    #[test]
    fn artifactmgr_get_write_attach_create_are_hermetic() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let env = ArtifactmgrEnv::artifactmgr_new("ops");
        let created = run_artifactmgr_command(&artifactmgr_args(&["init", "team-b", "t9", "Subject", "Long", "desc"]));
        assert_eq!(created.code, 0);
        assert!(created.stdout.contains("artifact created"));
        let written = run_artifactmgr_command(&artifactmgr_args(&["write", "team-b", "t9", "final", "answer"]));
        assert_eq!(written.code, 0);
        let source = env.root.join("source file.txt");
        std::fs::write(&source, "payload").expect("source");
        let attached = run_artifactmgr_command(&artifactmgr_args(&["attach", "team-b", "t9", source.to_str().expect("utf8")]));
        assert_eq!(attached.code, 0);
        let shown = run_artifactmgr_command(&artifactmgr_args(&["show", "team-b", "t9"]));
        assert!(shown.stdout.contains("final answer"));
        assert!(shown.stdout.contains("source_file.txt"));
    }

    #[test]
    fn artifactmgr_legacy_read_fallback_only_when_unoverridden() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let env = ArtifactmgrEnv::artifactmgr_new("legacy");
        let legacy_dir = env.root.join("home/.maw/artifacts/old/t1");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        let meta = ArtifactmgrMeta { team: "old".to_owned(), task_id: "t1".to_owned(), subject: "Legacy".to_owned(), owner: None, status: "pending".to_owned(), created_at: "then".to_owned(), updated_at: "then".to_owned(), commit_hash: None };
        artifactmgr_write_meta(&legacy_dir, &meta).expect("meta");
        let output = run_artifactmgr_command(&artifactmgr_args(&["ls", "old", "--json"]));
        assert!(output.stdout.contains("Legacy"));
    }
}
