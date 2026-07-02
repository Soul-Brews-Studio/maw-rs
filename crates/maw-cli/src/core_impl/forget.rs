const DISPATCH_56: &[DispatcherEntry] = &[ DispatcherEntry { command: "forget", handler: Handler::Sync(run_forget_command) } ];

fn forget_usage() -> &'static str { "usage: maw forget <oracle> [--dry-run] [--yes|--force] [--json]" }

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgetOptions {
    oracle: String,
    dry_run: bool,
    yes: bool,
    json: bool,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ForgetResult {
    oracle: String,
    resolved: ForgetResolved,
    dry_run: bool,
    confirmed: bool,
    actions: Vec<ForgetAction>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ForgetResolved {
    repo_path: String,
    repo_name: String,
    session_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct ForgetAction {
    layer: &'static str,
    target: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgetRepo {
    repo_path: std::path::PathBuf,
    repo_name: String,
    parent_dir: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgetWorktree {
    path: std::path::PathBuf,
    name: String,
}

#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ForgetFleetSession {
    name: String,
    #[serde(default, alias = "group_name")]
    group_name: String,
    #[serde(default)]
    windows: Vec<ForgetFleetWindow>,
}

#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
struct ForgetFleetWindow {
    name: String,
    #[serde(default)]
    repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgetFleetEntry {
    path: std::path::PathBuf,
    session: ForgetFleetSession,
}

trait ForgetTmux {
    fn forget_list_all(&mut self) -> Vec<TmuxSession>;
    fn forget_kill_session(&mut self, name: &str) -> Result<(), String>;
}

struct ForgetNativeTmux {
    client: TmuxClient<maw_tmux::CommandTmuxRunner>,
}

impl ForgetNativeTmux {
    fn forget_local() -> Self {
        Self { client: TmuxClient::local() }
    }
}

impl ForgetTmux for ForgetNativeTmux {
    fn forget_list_all(&mut self) -> Vec<TmuxSession> {
        self.client.list_all()
    }

    fn forget_kill_session(&mut self, name: &str) -> Result<(), String> {
        forget_validate_tmux_target_arg(name, "session")?;
        self.client.kill_session(name);
        Ok(())
    }
}

trait ForgetDoneRunner {
    fn forget_done_all(&mut self, cwd: &std::path::Path, oracle: &str) -> Result<String, String>;
    fn forget_done_worktree(&mut self, cwd: &std::path::Path, worktree: &str) -> Result<(), String>;
}

struct ForgetCommandDoneRunner;

impl ForgetDoneRunner for ForgetCommandDoneRunner {
    fn forget_done_all(&mut self, cwd: &std::path::Path, oracle: &str) -> Result<String, String> {
        forget_validate_existing_dir(cwd, "repo path")?;
        forget_validate_target_arg(oracle, "oracle")?;
        forget_run_native_done(cwd, &["--all", "--force", "--clean-branch", oracle])
        .map(|stdout| {
            let count = forget_processed_count(&stdout).unwrap_or(0);
            let mut detail = String::from("done --all processed ");
            detail.push_str(&count.to_string());
            detail
        })
    }

    fn forget_done_worktree(&mut self, cwd: &std::path::Path, worktree: &str) -> Result<(), String> {
        forget_validate_existing_dir(cwd, "repo path")?;
        forget_validate_target_arg(worktree, "worktree")?;
        forget_run_native_done(cwd, &[worktree, "--force", "--clean-branch"]).map(|_| ())
    }
}

fn run_forget_command(argv: &[String]) -> CliOutput {
    match forget_run_command_impl(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn forget_run_command_impl(argv: &[String]) -> Result<String, String> {
    let options = forget_parse_args(argv)?;
    let env = current_xdg_env();
    let ghq = ghq_root();
    let mut tmux = ForgetNativeTmux::forget_local();
    let mut done = ForgetCommandDoneRunner;
    let mut result = forget_plan_with_env(&options, &env, &ghq, &mut tmux)?;
    if options.json && options.dry_run {
        return forget_render_json(&result);
    }
    if options.dry_run || !options.yes {
        if !options.json && !options.dry_run && !options.yes {
            result.actions.push(ForgetAction {
                layer: "confirm",
                target: options.oracle.clone(),
                status: "skipped",
                detail: Some("not confirmed; no changes made".to_owned()),
            });
            result.dry_run = true;
        }
        return forget_render_result(&result, options.json);
    }
    forget_apply(&mut result, &env, &mut tmux, &mut done)?;
    forget_render_result(&result, options.json)
}

fn forget_parse_args(argv: &[String]) -> Result<ForgetOptions, String> {
    let mut oracle = None::<String>;
    let mut dry_run = false;
    let mut yes = false;
    let mut json = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(forget_usage().to_owned()),
            "--all" => {}
            "--dry-run" => dry_run = true,
            "--yes" | "-y" | "--force" => yes = true,
            "--json" => json = true,
            value if value.starts_with('-') => return Err(format!("forget: unknown argument {value}")),
            value => {
                if oracle.is_some() {
                    return Err(format!("{}\nunexpected positional args: {value}", forget_usage()));
                }
                forget_validate_target_arg(value, "oracle")?;
                oracle = Some(value.to_owned());
            }
        }
    }
    let Some(oracle) = oracle else { return Err(forget_usage().to_owned()); };
    Ok(ForgetOptions { oracle, dry_run, yes, json })
}

fn forget_plan_with_env<T: ForgetTmux>(
    options: &ForgetOptions,
    env: &MawXdgEnv,
    ghq: &std::path::Path,
    tmux: &mut T,
) -> Result<ForgetResult, String> {
    let repo = forget_resolve_repo(&options.oracle, ghq)?;
    let fleet_entry = forget_resolve_fleet_entry(&options.oracle, &repo.repo_name, &maw_config_dir(env))?;
    let session_name = forget_resolve_session_name(&options.oracle, &repo.repo_name, fleet_entry.as_ref(), &tmux.forget_list_all())?;
    let snapshot_files = forget_matching_snapshot_files(&options.oracle, &repo.repo_name, session_name.as_deref(), env)?;
    let mut actions = vec![ForgetAction {
        layer: "worktrees",
        target: forget_path_string(&repo.repo_path)?,
        status: "planned",
        detail: Some("maw done --all + linked worktree sweep".to_owned()),
    }];
    if let Some(session_name) = &session_name {
        actions.push(ForgetAction { layer: "tmux", target: session_name.clone(), status: "planned", detail: Some("kill-session".to_owned()) });
    }
    if let Some(entry) = &fleet_entry {
        actions.push(ForgetAction { layer: "fleet", target: forget_path_string(&entry.path)?, status: "planned", detail: None });
    }
    for path in snapshot_files {
        actions.push(ForgetAction { layer: "snapshots", target: forget_path_string(&path)?, status: "planned", detail: None });
    }
    let confirmed = options.yes && !options.dry_run;
    Ok(ForgetResult {
        oracle: options.oracle.clone(),
        resolved: ForgetResolved {
            repo_path: forget_path_string(&repo.repo_path)?,
            repo_name: repo.repo_name,
            session_name,
        },
        dry_run: options.dry_run || !confirmed,
        confirmed,
        actions,
    })
}

fn forget_apply<T: ForgetTmux, D: ForgetDoneRunner>(
    result: &mut ForgetResult,
    env: &MawXdgEnv,
    tmux: &mut T,
    done: &mut D,
) -> Result<(), String> {
    let repo_path = std::path::PathBuf::from(&result.resolved.repo_path);
    forget_validate_existing_dir(&repo_path, "repo path")?;
    let parent_dir = repo_path.parent().ok_or_else(|| "forget: repo path has no parent".to_owned())?.to_path_buf();
    match done.forget_done_all(&repo_path, &result.oracle) {
        Ok(mut detail) => {
            let mut swept = 0_usize;
            for worktree in forget_find_worktrees(&parent_dir, &result.resolved.repo_name) {
                if done.forget_done_worktree(&repo_path, &worktree.name).is_ok() {
                    swept += 1;
                }
            }
            detail.push_str("; swept ");
            detail.push_str(&swept.to_string());
            detail.push_str(" linked worktree(s)");
            forget_mark_action(result, "worktrees", &result.resolved.repo_path.clone(), "removed", Some(detail));
        }
        Err(error) => forget_mark_action(result, "worktrees", &result.resolved.repo_path.clone(), "failed", Some(error)),
    }

    if let Some(session) = result.resolved.session_name.clone() {
        match tmux.forget_kill_session(&session) {
            Ok(()) => forget_mark_action(result, "tmux", &session, "removed", None),
            Err(error) => forget_mark_action(result, "tmux", &session, "failed", Some(error)),
        }
    }

    let config_dir = maw_config_dir(env);
    let snapshots_dirs = forget_snapshot_dirs(env);
    for index in 0..result.actions.len() {
        let layer = result.actions[index].layer;
        if !matches!(layer, "fleet" | "snapshots") {
            continue;
        }
        let target = result.actions[index].target.clone();
        let path = std::path::PathBuf::from(&target);
        let allowed = if layer == "fleet" {
            forget_path_inside(&path, &config_dir.join("fleet"))
        } else {
            snapshots_dirs.iter().any(|dir| forget_path_inside(&path, dir))
        };
        if !allowed {
            result.actions[index].status = "failed";
            result.actions[index].detail = Some("refused path outside maw config/state snapshots".to_owned());
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => result.actions[index].status = "removed",
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => result.actions[index].status = "skipped",
            Err(error) => {
                result.actions[index].status = "failed";
                result.actions[index].detail = Some(error.to_string());
            }
        }
    }
    Ok(())
}

fn forget_mark_action(
    result: &mut ForgetResult,
    layer: &'static str,
    target: &str,
    status: &'static str,
    detail: Option<String>,
) {
    if let Some(action) = result.actions.iter_mut().find(|action| action.layer == layer && action.target == target) {
        action.status = status;
        if detail.is_some() {
            action.detail = detail;
        }
    }
}

fn forget_render_result(result: &ForgetResult, json: bool) -> Result<String, String> {
    if json {
        return forget_render_json(result);
    }
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[36mforget\x1b[0m {} → {}", result.oracle, result.resolved.repo_name);
    for action in &result.actions {
        let verb = if result.dry_run && action.status == "planned" { "[dry-run] would" } else { action.status };
        let _ = write!(out, "  \x1b[36m⬡\x1b[0m {verb} {}: {}", action.layer, action.target);
        if let Some(detail) = &action.detail {
            let _ = write!(out, " ({detail})");
        }
        out.push('\n');
    }
    if !result.dry_run {
        let removed = result.actions.iter().filter(|action| action.status == "removed").count();
        let failed = result.actions.iter().any(|action| action.status == "failed");
        let _ = write!(out, "  \x1b[32m✓\x1b[0m forget removed ");
        let _ = write!(out, "{removed}");
        out.push_str(" item(s)");
        if failed {
            out.push_str(", failures present");
        }
        out.push('\n');
    } else if result.actions.iter().any(|action| action.layer == "confirm") {
        out.push_str("  \x1b[90m○\x1b[0m not confirmed; no changes made\n");
    }
    Ok(out)
}

fn forget_render_json(result: &ForgetResult) -> Result<String, String> {
    serde_json::to_string_pretty(result)
        .map(|json| {
            let mut out = json;
            out.push('\n');
            out
        })
        .map_err(|error| format!("forget: failed to render json: {error}"))
}

fn forget_resolve_repo(oracle: &str, ghq: &std::path::Path) -> Result<ForgetRepo, String> {
    forget_validate_target_arg(oracle, "oracle")?;
    let root = ghq.join("github.com");
    let mut matches = Vec::<std::path::PathBuf>::new();
    let Ok(orgs) = std::fs::read_dir(&root) else { return Err(format!("forget: repo not found for {oracle}")); };
    let wanted = [format!("{oracle}-oracle"), oracle.to_owned()];
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        for name in &wanted {
            let candidate = org.path().join(name);
            if candidate.is_dir() {
                matches.push(candidate);
            }
        }
    }
    matches.sort();
    matches.dedup();
    if matches.len() > 1 {
        let joined = matches.iter().filter_map(|path| path.to_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("forget '{oracle}' is ambiguous in worktrees: {joined}"));
    }
    let Some(repo_path) = matches.pop() else { return Err(format!("forget: repo not found for {oracle}")); };
    forget_validate_existing_dir(&repo_path, "repo path")?;
    let repo_name = repo_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_owned();
    forget_validate_target_arg(&repo_name, "repo name")?;
    let parent_dir = repo_path.parent().ok_or_else(|| format!("forget: repo has no parent: {}", repo_path.display()))?.to_path_buf();
    Ok(ForgetRepo { repo_path, repo_name, parent_dir })
}

fn forget_resolve_fleet_entry(
    oracle: &str,
    repo_name: &str,
    config_dir: &std::path::Path,
) -> Result<Option<ForgetFleetEntry>, String> {
    let aliases = forget_aliases_for(oracle, repo_name);
    let mut matches = forget_load_fleet_entries(config_dir)
        .into_iter()
        .filter(|entry| forget_fleet_entry_matches(entry, &aliases))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| a.path.cmp(&b.path));
    if matches.len() > 1 {
        let names = matches.iter().map(|entry| entry.session.name.as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("forget '{oracle}' is ambiguous in fleet: {names}"));
    }
    Ok(matches.pop())
}

fn forget_load_fleet_entries(config_dir: &std::path::Path) -> Vec<ForgetFleetEntry> {
    let fleet_dir = config_dir.join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else { return Vec::new(); };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let session = serde_json::from_str::<ForgetFleetSession>(&text).ok()?;
            Some(ForgetFleetEntry { path, session })
        })
        .collect()
}

fn forget_fleet_entry_matches(entry: &ForgetFleetEntry, aliases: &BTreeSet<String>) -> bool {
    let mut candidates = vec![
        entry.session.name.clone(),
        forget_strip_numeric_prefix(&entry.session.name).to_owned(),
        entry.session.group_name.clone(),
    ];
    for window in &entry.session.windows {
        candidates.push(window.name.clone());
        candidates.push(forget_strip_oracle_suffix(&window.name).to_owned());
        if !window.repo.is_empty() {
            let base = window.repo.rsplit('/').next().unwrap_or(&window.repo);
            candidates.push(base.to_owned());
            candidates.push(forget_strip_oracle_suffix(base).to_owned());
        }
    }
    candidates.iter().any(|candidate| aliases.contains(&candidate.to_lowercase()))
}

fn forget_resolve_session_name(
    oracle: &str,
    repo_name: &str,
    fleet_entry: Option<&ForgetFleetEntry>,
    sessions: &[TmuxSession],
) -> Result<Option<String>, String> {
    let mut aliases = forget_aliases_for(oracle, repo_name);
    if let Some(entry) = fleet_entry {
        aliases.insert(entry.session.name.to_lowercase());
    }
    let mut matches = sessions
        .iter()
        .filter(|session| {
            let name = session.name.to_lowercase();
            aliases.contains(&name) || aliases.contains(forget_strip_numeric_prefix(&name))
        })
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    if matches.len() > 1 {
        return Err(format!("forget '{oracle}' is ambiguous in tmux: {}", matches.join(", ")));
    }
    Ok(matches.pop().or_else(|| fleet_entry.map(|entry| entry.session.name.clone())))
}

fn forget_matching_snapshot_files(
    oracle: &str,
    repo_name: &str,
    session_name: Option<&str>,
    env: &MawXdgEnv,
) -> Result<Vec<std::path::PathBuf>, String> {
    let aliases = forget_aliases_for(oracle, repo_name);
    let mut out = Vec::new();
    for dir in forget_snapshot_dirs(env) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue; };
        let mut files = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
            .collect::<Vec<_>>();
        files.sort();
        for path in files {
            if !forget_path_inside(&path, &dir) {
                continue;
            }
            if forget_snapshot_matches(&path, &aliases, session_name)? {
                out.push(path);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn forget_snapshot_dirs(env: &MawXdgEnv) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![maw_state_path(env, &["snapshots"]), maw_config_path(env, &["snapshots"] )];
    dirs.sort();
    dirs.dedup();
    dirs
}

fn forget_snapshot_matches(
    path: &std::path::Path,
    aliases: &BTreeSet<String>,
    session_name: Option<&str>,
) -> Result<bool, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("forget: read {}: {error}", path.display()))?;
    let value = serde_json::from_str::<serde_json::Value>(&text).map_err(|error| format!("forget: parse {}: {error}", path.display()))?;
    let sessions = value.get("sessions").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    for session in sessions {
        let name = session.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_lowercase();
        if session_name.is_some_and(|session_name| name == session_name.to_lowercase())
            || aliases.contains(&name)
            || aliases.contains(forget_strip_numeric_prefix(&name))
        {
            return Ok(true);
        }
        let windows = session.get("windows").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
        for window in windows {
            let win = window.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_lowercase();
            if aliases.contains(&win) || aliases.contains(forget_strip_oracle_suffix(&win)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn forget_find_worktrees(parent_dir: &std::path::Path, repo_name: &str) -> Vec<ForgetWorktree> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent_dir) {
        let prefix = format!("{repo_name}.wt-");
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() && name.starts_with(&prefix) && path.join(".git").exists() {
                out.push(ForgetWorktree { name: name[prefix.len()..].to_owned(), path });
            }
        }
    }
    let nested = parent_dir.join(repo_name).join("agents");
    if let Ok(entries) = std::fs::read_dir(nested) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".git").exists() {
                out.push(ForgetWorktree { name: entry.file_name().to_string_lossy().into_owned(), path });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

fn forget_run_native_done(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    for arg in args {
        if matches!(*arg, "--all" | "--force" | "--clean-branch") {
            continue;
        }
        forget_validate_target_arg(arg, "maw done argument")?;
    }
    let done_args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    done_run_with_cwd(cwd, &done_args, &mut DoneLocal::default())
}

fn forget_processed_count(stdout: &str) -> Option<usize> {
    let marker = "processed";
    let index = stdout.find(marker)? + marker.len();
    stdout[index..]
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|part| !part.is_empty())?
        .parse()
        .ok()
}

fn forget_aliases_for(input: &str, repo_name: &str) -> BTreeSet<String> {
    let raw = input.trim().to_lowercase();
    let repo = repo_name.trim().to_lowercase();
    let bare = forget_strip_oracle_suffix(&repo).to_owned();
    [
        raw.clone(),
        forget_strip_oracle_suffix(&raw).to_owned(),
        repo,
        bare,
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect()
}

fn forget_strip_oracle_suffix(name: &str) -> &str {
    name.strip_suffix("-oracle").unwrap_or(name)
}

fn forget_strip_numeric_prefix(name: &str) -> &str {
    name.split_once('-')
        .filter(|(prefix, suffix)| prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.is_empty())
        .map_or(name, |(_, suffix)| suffix)
}

fn forget_validate_target_arg(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("forget: {name} must be non-empty, unpadded, not start with '-', and contain no control characters"));
    }
    if value.contains("..") || value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return Err(format!("forget: {name} contains a refused path segment"));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '/' | ':')) {
        return Err(format!("forget: {name} contains unsupported characters"));
    }
    Ok(())
}

fn forget_validate_tmux_target_arg(value: &str, name: &str) -> Result<(), String> {
    forget_validate_target_arg(value, name)?;
    if value.contains('/') {
        return Err(format!("forget: {name} must not contain '/'"));
    }
    Ok(())
}

fn forget_validate_existing_dir(path: &std::path::Path, name: &str) -> Result<(), String> {
    if path.as_os_str().is_empty() || !path.is_absolute() || path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err(format!("forget: refused risky {name}: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("forget: {name} is not a directory: {}", path.display()));
    }
    Ok(())
}

fn forget_path_inside(path: &std::path::Path, dir: &std::path::Path) -> bool {
    let Ok(path) = path.canonicalize() else { return false; };
    let Ok(dir) = dir.canonicalize() else { return false; };
    path.starts_with(dir)
}

fn forget_path_string(path: &std::path::Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("forget: path is not utf8: {}", path.display()))
}

#[cfg(test)]
mod forget_tests {
    use super::*;

    #[derive(Default)]
    struct ForgetMockTmux {
        sessions: Vec<TmuxSession>,
        killed: Vec<String>,
    }

    impl ForgetTmux for ForgetMockTmux {
        fn forget_list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
        }

        fn forget_kill_session(&mut self, name: &str) -> Result<(), String> {
            self.killed.push(name.to_owned());
            Ok(())
        }
    }

    #[derive(Default)]
    struct ForgetMockDone {
        all: Vec<(String, String)>,
        worktrees: Vec<String>,
    }

    impl ForgetDoneRunner for ForgetMockDone {
        fn forget_done_all(&mut self, cwd: &std::path::Path, oracle: &str) -> Result<String, String> {
            self.all.push((cwd.display().to_string(), oracle.to_owned()));
            Ok("done --all processed 2\n".to_owned())
        }

        fn forget_done_worktree(&mut self, _cwd: &std::path::Path, worktree: &str) -> Result<(), String> {
            self.worktrees.push(worktree.to_owned());
            Ok(())
        }
    }

    fn forget_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn forget_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "maw-rs-forget-{name}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn forget_write_script(path: &std::path::Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("script parent");
        }
        std::fs::write(path, body).expect("script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(path).expect("script metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("script chmod");
        }
    }

    fn forget_fixture() -> (std::path::PathBuf, MawXdgEnv, std::path::PathBuf) {
        let root = forget_temp_root("fixture");
        let ghq = root.join("ghq");
        let repo = ghq.join("github.com/acme/neo-oracle");
        std::fs::create_dir_all(repo.join("agents/143-forget/.git")).expect("repo");
        std::fs::write(repo.join(".git"), "gitdir: real\n").expect("git");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");
        std::fs::write(
            root.join("config/fleet/03-neo.json"),
            r#"{"name":"03-neo","groupName":"knights","windows":[{"name":"neo","repo":"acme/neo-oracle"}]}"#,
        )
        .expect("fleet");
        std::fs::create_dir_all(root.join("state/snapshots")).expect("state snapshots");
        std::fs::write(
            root.join("state/snapshots/neo.json"),
            r#"{"sessions":[{"name":"03-neo","windows":[{"name":"neo"}]}]}"#,
        )
        .expect("snapshot");
        let env = MawXdgEnv::with_vars(
            root.join("home"),
            [
                ("MAW_CONFIG_DIR", root.join("config").display().to_string()),
                ("MAW_STATE_DIR", root.join("state").display().to_string()),
            ],
        );
        (root, env, ghq)
    }

    #[test]
    fn forget_parse_flags_and_option_injection_guard() {
        let parsed = forget_parse_args(&forget_strings(&["neo", "--dry-run", "--yes", "--force", "--json", "--all"])).expect("parse");
        assert_eq!(parsed.oracle, "neo");
        assert!(parsed.dry_run);
        assert!(parsed.yes);
        assert!(parsed.json);
        assert!(forget_parse_args(&forget_strings(&["-oProxyCommand=touch-pwned"])).expect_err("guard").contains("unknown argument"));
        assert!(forget_validate_target_arg("../neo", "oracle").is_err());
        assert!(forget_validate_tmux_target_arg("-bad", "session").is_err());
    }

    #[test]
    fn forget_plan_is_hermetic_and_matches_fixture() {
        let (root, env, ghq) = forget_fixture();
        let options = ForgetOptions { oracle: "neo".to_owned(), dry_run: true, yes: false, json: false };
        let mut tmux = ForgetMockTmux { sessions: vec![TmuxSession { name: "03-neo".to_owned(), windows: Vec::new() }], killed: Vec::new() };
        let result = forget_plan_with_env(&options, &env, &ghq, &mut tmux).expect("plan");
        assert_eq!(result.resolved.repo_name, "neo-oracle");
        assert_eq!(result.resolved.session_name.as_deref(), Some("03-neo"));
        assert!(result.actions.iter().any(|action| action.layer == "fleet"));
        assert!(result.actions.iter().any(|action| action.layer == "snapshots"));
        let text = forget_render_result(&result, false).expect("render");
        let normalized = text.replace(&root.display().to_string(), "<ROOT>");
        assert_eq!(
            normalized,
            include_str!("../../tests/fixtures/native-forget/dry-run.stdout")
        );
    }

    #[test]
    fn forget_json_uses_maw_js_field_names() {
        let (_root, env, ghq) = forget_fixture();
        let options = ForgetOptions { oracle: "neo".to_owned(), dry_run: true, yes: false, json: true };
        let mut tmux = ForgetMockTmux::default();
        let result = forget_plan_with_env(&options, &env, &ghq, &mut tmux).expect("plan");
        let json = forget_render_json(&result).expect("json");
        assert!(json.contains("\"repoPath\""));
        assert!(json.contains("\"sessionName\""));
        assert!(json.contains("\"dryRun\""));
    }

    #[test]
    fn forget_apply_removes_fleet_and_snapshots_after_yes() {
        let (_root, env, ghq) = forget_fixture();
        let wt = ghq.join("github.com/acme/neo-oracle/agents/143-forget/.git");
        assert!(wt.exists());
        let options = ForgetOptions { oracle: "neo".to_owned(), dry_run: false, yes: true, json: false };
        let mut tmux = ForgetMockTmux { sessions: vec![TmuxSession { name: "03-neo".to_owned(), windows: Vec::new() }], killed: Vec::new() };
        let mut result = forget_plan_with_env(&options, &env, &ghq, &mut tmux).expect("plan");
        let mut done = ForgetMockDone::default();
        forget_apply(&mut result, &env, &mut tmux, &mut done).expect("apply");
        assert_eq!(tmux.killed, vec!["03-neo".to_owned()]);
        assert_eq!(done.worktrees, vec!["143-forget".to_owned()]);
        assert!(result.actions.iter().any(|action| action.layer == "fleet" && action.status == "removed"));
        assert!(result.actions.iter().any(|action| action.layer == "snapshots" && action.status == "removed"));
    }

    #[test]
    fn forget_native_done_runner_uses_explicit_cwd_without_path_maw_or_global_cwd_change() {
        let _lock = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _path = EnvVarRestore::capture("PATH");

        let cwd_before = std::env::current_dir().expect("cwd before");
        let root = forget_temp_root("native-done");
        let bin = root.join("bin");
        let home = root.join("home");
        let ghq = root.join("ghq");
        let repo = ghq.join("github.com/acme/neo-oracle");
        let worktree = repo.join("agents/143-forget");
        std::fs::create_dir_all(&worktree).expect("worktree");
        std::fs::write(worktree.join(".git"), "gitdir: linked\n").expect("worktree git");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");

        let delegated_maw = root.join("delegated-maw.log");
        forget_write_script(
            &bin.join("maw"),
            &format!("#!/bin/sh\nprintf 'DELEGATED-MAW %s\\n' \"$*\" >> '{}'\nexit 77\n", delegated_maw.display()),
        );
        forget_write_script(
            &bin.join("tmux"),
            "#!/bin/sh\ncase \"$1\" in\n  list-windows) exit 0 ;;\n  display-message) exit 1 ;;\n  *) exit 0 ;;\nesac\n",
        );
        forget_write_script(
            &bin.join("git"),
            "#!/bin/sh\ncase \"$*\" in\n  *'rev-parse --abbrev-ref HEAD'*) printf 'feature-forget\\n' ;;\nesac\nexit 0\n",
        );

        std::env::set_var("HOME", &home);
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", &ghq);
        std::env::set_var("PATH", &bin);

        let mut runner = ForgetCommandDoneRunner;
        runner
            .forget_done_worktree(&repo, "143-forget")
            .expect("native done worktree");

        assert!(!delegated_maw.exists(), "forget must not shell to PATH maw");
        assert_eq!(std::env::current_dir().expect("cwd after"), cwd_before);
    }
}
