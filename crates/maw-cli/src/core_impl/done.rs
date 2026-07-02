const DISPATCH_57: &[DispatcherEntry] = &[
    DispatcherEntry { command: "done", handler: Handler::Sync(run_done_command) },
    DispatcherEntry { command: "finish", handler: Handler::Sync(run_done_command) },
];

const DONE_USAGE: &str = "usage: maw done <window-name> [--force] [--dry-run] [--clean-branch] or maw done --all [<oracle>] [--force] [--dry-run] [--clean-branch]  (see: maw sleep/kill for non-worktree shutdown)";
const DONE_ALL_USAGE: &str = "usage: maw done --all [<oracle>] [--force] [--dry-run] [--clean-branch]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct DoneOptions { all: bool, force: bool, dry_run: bool, clean_branch: bool, target: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoneWindow { session: String, index: i32, name: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoneWorktree { main_path: std::path::PathBuf, full_path: std::path::PathBuf, label: String }

#[derive(Debug, Clone)]
struct DoneContext {
    repos_root: std::path::PathBuf,
    config_dir: std::path::PathBuf,
}

impl DoneContext {
    fn from_env() -> Self {
        Self {
            repos_root: ghq_root().join("github.com"),
            config_dir: active_config_dir(),
        }
    }

    fn with_cwd(cwd: &std::path::Path) -> Self {
        Self {
            repos_root: done_repos_root_from_cwd(cwd)
                .unwrap_or_else(|| ghq_root().join("github.com")),
            config_dir: active_config_dir(),
        }
    }
}

#[derive(Default)]
struct DoneLocal { runner: maw_tmux::CommandTmuxRunner }

fn run_done_command(argv: &[String]) -> CliOutput {
    match done_run(argv, &mut DoneLocal::default()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn done_run(argv: &[String], local: &mut DoneLocal) -> Result<String, String> {
    let context = DoneContext::from_env();
    done_run_with_context(argv, local, &context)
}

fn done_run_with_cwd(cwd: &std::path::Path, argv: &[String], local: &mut DoneLocal) -> Result<String, String> {
    let context = DoneContext::with_cwd(cwd);
    done_run_with_context(argv, local, &context)
}

fn done_run_with_context(argv: &[String], local: &mut DoneLocal, context: &DoneContext) -> Result<String, String> {
    let options = done_parse_args(argv)?;
    if options.all { return Ok(done_run_all(&options, local, context)); }
    let target = options.target.clone().ok_or_else(|| DONE_USAGE.to_owned())?;
    done_run_one_with_context(&target, &options, None, local, context)
}

fn done_parse_args(argv: &[String]) -> Result<DoneOptions, String> {
    let mut options = DoneOptions::default();
    let mut positionals = Vec::<String>::new();
    for arg in argv {
        match arg.as_str() {
            "--all" => options.all = true,
            "--force" => options.force = true,
            "--dry-run" => options.dry_run = true,
            "--clean-branch" => options.clean_branch = true,
            "--help" | "-h" => return Err(DONE_USAGE.to_owned()),
            value if value.starts_with('-') => return Err(format!("done: unknown argument {value}")),
            value => positionals.push(value.to_owned()),
        }
    }
    if options.all && positionals.len() > 1 {
        return Err(format!("unexpected extra positional arg(s) for maw done --all: {}\n  {DONE_ALL_USAGE}", positionals[1..].join(" ")));
    }
    if !options.all && positionals.len() > 1 {
        let hint = if positionals.first().is_some_and(|value| value.eq_ignore_ascii_case("all")) { "\n  did you mean `maw done --all`?" } else { "" };
        return Err(format!("unexpected extra positional arg(s) for maw done: {}{hint}\n  {DONE_USAGE}", positionals[1..].join(" ")));
    }
    if let Some(target) = positionals.first() { done_validate_target_arg(target, "target")?; options.target = Some(done_normalize_target(target)); }
    if !options.all && options.target.is_none() { return Err(DONE_USAGE.to_owned()); }
    Ok(options)
}

fn done_run_one(target: &str, options: &DoneOptions, session_filter: Option<&str>, local: &mut DoneLocal) -> Result<String, String> {
    let context = DoneContext::from_env();
    done_run_one_with_context(target, options, session_filter, local, &context)
}

fn done_run_one_with_context(target: &str, options: &DoneOptions, session_filter: Option<&str>, local: &mut DoneLocal, context: &DoneContext) -> Result<String, String> {
    let mut stdout = String::new();
    let sessions = local.done_list_windows();
    let target_lower = target.to_lowercase();
    let matched = done_find_window(&sessions, &target_lower, session_filter);
    if let Some(window) = &matched { done_assert_may_target_lead(window, &sessions, local, &mut stdout)?; }
    if let Some(window) = &matched {
        if !options.force {
            done_auto_save(window, options, local, &mut stdout);
            if options.dry_run { return Ok(stdout); }
        }
    } else if options.dry_run {
        let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] window '{target}' not running — nothing to auto-save");
    }
    if let Some(window) = &matched { done_kill_window(window, options, local, &mut stdout); } else { let _ = writeln!(stdout, "  \x1b[90m○\x1b[0m window '{target}' not running"); }
    let removed_worktree = done_remove_worktree_via_config(&target_lower, context, options, &mut stdout)? || done_remove_worktree_by_scan(target, &context.repos_root, options, &mut stdout)?;
    if !removed_worktree { stdout.push_str("  \x1b[90m○\x1b[0m no worktree to remove (may be a main window)\n"); }
    if options.dry_run {
        if matched.is_none() && !removed_worktree { done_fail_missing_target(target)?; }
        let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would remove '{target_lower}' from fleet config if present\n");
        return Ok(stdout);
    }
    let removed_config = done_remove_from_fleet_config(&target_lower, context, &mut stdout);
    if !removed_config { stdout.push_str("  \x1b[90m○\x1b[0m not in any fleet config\n"); }
    if matched.is_none() && !removed_worktree && !removed_config { done_fail_missing_target(target)?; }
    stdout.push('\n');
    Ok(stdout)
}

fn done_run_all(options: &DoneOptions, local: &mut DoneLocal, context: &DoneContext) -> String {
    let mut stdout = String::new();
    let sessions = local.done_list_windows();
    let session_name = done_current_session_name(&sessions, options.target.as_deref(), local);
    let Some(session_name) = session_name else {
        let reason = if let Some(oracle) = &options.target { format!("no tmux session found for oracle '{oracle}'") } else if sessions.is_empty() { "no tmux sessions to clean".to_owned() } else { "could not identify current tmux session; run inside tmux".to_owned() };
        let _ = writeln!(stdout, "  \x1b[90m○\x1b[0m {reason}");
        return stdout;
    };
    let targets = done_non_lead_windows(&sessions, &session_name);
    if targets.is_empty() { let _ = writeln!(stdout, "  \x1b[90m○\x1b[0m no non-lead windows in {session_name}"); return stdout; }
    let mode = if options.dry_run { "would process" } else { "processing" };
    let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m {mode} {} non-lead window(s) in {session_name}", targets.len());
    let mut processed = 0_usize;
    let mut skipped = 0_usize;
    for window in targets {
        let _ = writeln!(stdout, "\n\x1b[36m→\x1b[0m done {session_name}:{}", window.name);
        match done_run_one_with_context(&window.name, options, Some(&session_name), local, context) { Ok(text) => { stdout.push_str(&text); processed += 1; }, Err(error) => { skipped += 1; let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m skipped {}: {error}", window.name); } }
    }
    let verb = if options.dry_run { "would process" } else { "processed" };
    let suffix = if skipped == 0 { String::new() } else { format!(", skipped {skipped}") };
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m done --all {verb} {processed} window(s){suffix}");
    stdout
}

impl DoneLocal {
    fn done_list_windows(&mut self) -> Vec<DoneWindow> {
        let args = ["-a".to_owned(), "-F".to_owned(), "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}".to_owned()];
        let Ok(raw) = maw_tmux::TmuxRunner::run(&mut self.runner, "list-windows", &args) else { return Vec::new(); };
        raw.lines().filter_map(done_parse_window_line).collect()
    }

    fn done_current_identity(&mut self) -> Option<(String, i32)> {
        let args = ["-p".to_owned(), "#{session_name}\t#{window_index}".to_owned()];
        let raw = maw_tmux::TmuxRunner::run(&mut self.runner, "display-message", &args).ok()?;
        let (session, index) = raw.trim().split_once('\t')?;
        Some((session.to_owned(), index.parse::<i32>().ok()?))
    }

    fn done_pane_info(&mut self, target: &str) -> Option<(String, String)> {
        done_validate_tmux_target(target).ok()?;
        let args = ["-t".to_owned(), target.to_owned(), "-p".to_owned(), "#{pane_current_command}\t#{pane_current_path}".to_owned()];
        let raw = maw_tmux::TmuxRunner::run(&mut self.runner, "display-message", &args).ok()?;
        let (command, cwd) = raw.trim_end().split_once('\t').unwrap_or((raw.trim(), ""));
        Some((command.trim().to_owned(), cwd.trim().to_owned()))
    }

    fn done_tmux(&mut self, command: &str, args: &[String]) -> Result<String, String> {
        maw_tmux::TmuxRunner::run(&mut self.runner, command, args).map_err(|error| error.message)
    }
}

fn done_parse_window_line(line: &str) -> Option<DoneWindow> {
    let mut parts = line.split("|||");
    let session = parts.next()?.to_owned();
    let index = parts.next()?.parse::<i32>().ok()?;
    let name = parts.next()?.to_owned();
    if session.is_empty() || name.is_empty() { return None; }
    Some(DoneWindow { session, index, name })
}

fn done_find_window(windows: &[DoneWindow], target_lower: &str, session_filter: Option<&str>) -> Option<DoneWindow> {
    windows.iter().find(|window| session_filter.is_none_or(|session| session == window.session) && window.name.eq_ignore_ascii_case(target_lower)).cloned()
}

fn done_assert_may_target_lead(window: &DoneWindow, windows: &[DoneWindow], local: &mut DoneLocal, stdout: &mut String) -> Result<(), String> {
    let Some(lead) = done_lead_window(windows, &window.session) else { return Ok(()); };
    if lead.index != window.index { return Ok(()); }
    if let Some((current_session, current_index)) = local.done_current_identity() { if current_session == window.session && current_index == lead.index { return Ok(()); } }
    let message = format!("refusing to done lead window '{}' in session '{}' from a non-lead context", window.name, window.session);
    let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m {message}");
    stdout.push_str("  \x1b[90m  run from the lead window, or target a non-lead agent window\x1b[0m\n");
    Err(message)
}

fn done_lead_window(windows: &[DoneWindow], session: &str) -> Option<DoneWindow> {
    windows.iter().filter(|window| window.session == session).min_by_key(|window| window.index).cloned()
}

fn done_non_lead_windows(windows: &[DoneWindow], session: &str) -> Vec<DoneWindow> {
    let Some(lead) = done_lead_window(windows, session) else { return Vec::new(); };
    let mut out = windows.iter().filter(|window| window.session == session && window.index != lead.index).cloned().collect::<Vec<_>>();
    out.sort_by_key(|window| window.index);
    out
}

fn done_current_session_name(windows: &[DoneWindow], oracle: Option<&str>, local: &mut DoneLocal) -> Option<String> {
    let sessions = done_session_names(windows);
    if let Some(oracle) = oracle {
        let wanted = done_session_stem(oracle);
        if let Some(name) = sessions.iter().find(|name| done_session_stem(name) == wanted) { return Some(name.clone()); }
        let matches = sessions.iter().filter(|name| done_compact_stem(name) == done_compact_stem(oracle)).cloned().collect::<Vec<_>>();
        if matches.len() == 1 { return matches.first().cloned(); }
        return None;
    }
    if let Some((session, _)) = local.done_current_identity() { if sessions.contains(&session) { return Some(session); } }
    if sessions.len() == 1 { sessions.first().cloned() } else { None }
}

fn done_session_names(windows: &[DoneWindow]) -> Vec<String> {
    let mut names = windows.iter().map(|window| window.session.clone()).collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn done_session_stem(value: &str) -> String { value.trim().to_lowercase().trim_start_matches(|c: char| c.is_ascii_digit() || c == '-').trim_end_matches("-oracle").to_owned() }

fn done_compact_stem(value: &str) -> String { done_session_stem(value).chars().filter(char::is_ascii_alphanumeric).collect() }

fn done_repos_root_from_cwd(cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    cwd.ancestors()
        .find(|path| path.file_name().and_then(std::ffi::OsStr::to_str) == Some("github.com"))
        .map(std::path::Path::to_path_buf)
}

fn done_auto_save(window: &DoneWindow, options: &DoneOptions, local: &mut DoneLocal, stdout: &mut String) {
    let target = format!("{}:{}", window.session, window.name);
    let (command, cwd) = local.done_pane_info(&target).unwrap_or_default();
    let retro = done_retrospective_command(&command);
    if options.dry_run {
        if let Some(retro) = retro { let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would send {retro} to {target} and wait 10s"); } else { stdout.push_str("  \x1b[36m⬡\x1b[0m [dry-run] would skip retro (no retrospective command for this engine)\n"); }
        if !cwd.is_empty() { let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would git add + commit + push in {cwd}"); }
        let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would kill window {target}");
        stdout.push_str("  \x1b[36m⬡\x1b[0m [dry-run] would remove worktree + fleet config\n\n");
        return;
    }
    if let Some(retro) = retro { let _ = local.done_tmux("send-keys", &maw_tmux::tmux_send_keys_literal_args(&target, retro)); let _ = local.done_tmux("send-keys", &maw_tmux::tmux_send_enter_args(&target)); }
    if !cwd.is_empty() { let _ = done_git(&["-C".to_owned(), cwd.clone(), "add".to_owned(), "--".to_owned(), ".".to_owned()]); let _ = done_git(&["-C".to_owned(), cwd.clone(), "commit".to_owned(), "-m".to_owned(), "chore: auto-save before done".to_owned()]); let _ = done_git(&["-C".to_owned(), cwd, "push".to_owned()]); }
}

fn done_kill_window(window: &DoneWindow, options: &DoneOptions, local: &mut DoneLocal, stdout: &mut String) {
    let target = format!("{}:{}", window.session, window.name);
    if options.dry_run { let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would kill window {target}"); return; }
    match local.done_tmux("kill-window", &["-t".to_owned(), target.clone()]) { Ok(_) => { let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m killed window {target}"); }, Err(_) => stdout.push_str("  \x1b[33m⚠\x1b[0m could not kill window (may already be closed)\n") }
}

fn done_retrospective_command(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    if lower.contains("omx") || lower.contains("oh-my-codex") { Some("$rrr") } else if lower.is_empty() || lower.contains("codex") || lower.contains("aider") || lower.contains("opencode") { None } else { Some("/rrr") }
}

fn done_remove_worktree_via_config(window_lower: &str, context: &DoneContext, options: &DoneOptions, stdout: &mut String) -> Result<bool, String> {
    for file in done_fleet_config_files(context) {
        let Ok(raw) = std::fs::read_to_string(&file) else { continue; };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else { continue; };
        let Some(windows) = json.get("windows").and_then(serde_json::Value::as_array) else { continue; };
        let Some(repo) = windows.iter().find(|item| item.get("name").and_then(serde_json::Value::as_str).is_some_and(|name| name.eq_ignore_ascii_case(window_lower))).and_then(|item| item.get("repo")).and_then(serde_json::Value::as_str) else { continue; };
        let Some(worktree) = done_parse_worktree_path(&context.repos_root.join(repo), &context.repos_root) else { break; };
        if options.dry_run { let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would remove worktree {repo}"); return Ok(true); }
        done_remove_worktree(&worktree, options, stdout)?;
        return Ok(true);
    }
    Ok(false)
}

fn done_remove_worktree_by_scan(target: &str, repos_root: &std::path::Path, options: &DoneOptions, stdout: &mut String) -> Result<bool, String> {
    let matches = done_find_worktree_paths(target, repos_root);
    if matches.len() > 1 { let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m refusing to remove worktree '{}' — matches {} repos", target, matches.len()); return Ok(false); }
    let Some(worktree) = matches.first() else { return Ok(false); };
    if options.dry_run { let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would remove worktree {}", worktree.label); return Ok(true); }
    done_remove_worktree(worktree, options, stdout)?;
    Ok(true)
}

fn done_remove_worktree(worktree: &DoneWorktree, options: &DoneOptions, stdout: &mut String) -> Result<(), String> {
    done_validate_exec_path(&worktree.main_path)?;
    done_validate_exec_path(&worktree.full_path)?;
    let branch = done_git(&["-C".to_owned(), worktree.full_path.display().to_string(), "rev-parse".to_owned(), "--abbrev-ref".to_owned(), "HEAD".to_owned()]).unwrap_or_default().trim().to_owned();
    done_git(&["-C".to_owned(), worktree.main_path.display().to_string(), "worktree".to_owned(), "remove".to_owned(), "--force".to_owned(), "--".to_owned(), worktree.full_path.display().to_string()])?;
    done_git(&["-C".to_owned(), worktree.main_path.display().to_string(), "worktree".to_owned(), "prune".to_owned()])?;
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m removed worktree {}", worktree.label);
    done_cleanup_branch(&worktree.main_path, &branch, options, stdout);
    Ok(())
}

fn done_cleanup_branch(main_path: &std::path::Path, branch: &str, options: &DoneOptions, stdout: &mut String) {
    if branch.is_empty() || branch == "main" || branch == "HEAD" { return; }
    if options.clean_branch { let _ = done_git(&["-C".to_owned(), main_path.display().to_string(), "branch".to_owned(), "-D".to_owned(), "--".to_owned(), branch.to_owned()]); let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m deleted branch {branch}"); } else { let _ = writeln!(stdout, "  \x1b[90m○\x1b[0m branch {branch} retained (use --clean-branch to delete)"); }
}

fn done_find_worktree_paths(target: &str, repos_root: &std::path::Path) -> Vec<DoneWorktree> {
    let mut out = Vec::new();
    let target_lower = target.to_lowercase();
    let Ok(orgs) = std::fs::read_dir(repos_root) else { return out; };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(repos) = std::fs::read_dir(org.path()) else { continue; };
        for repo in repos.flatten().filter(|entry| entry.path().is_dir()) { done_scan_repo_worktrees(&repo.path(), repos_root, &target_lower, &mut out); }
    }
    out.sort_by(|a, b| a.full_path.cmp(&b.full_path));
    out
}

fn done_scan_repo_worktrees(repo_path: &std::path::Path, repos_root: &std::path::Path, target_lower: &str, out: &mut Vec<DoneWorktree>) {
    let Some(name) = repo_path.file_name().and_then(std::ffi::OsStr::to_str) else { return; };
    if name.to_lowercase().ends_with(&format!(".wt-{target_lower}")) { if let Some(worktree) = done_parse_worktree_path(repo_path, repos_root) { out.push(worktree); } }
    let agents = repo_path.join("agents");
    let Ok(entries) = std::fs::read_dir(agents) else { return; };
    for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(target_lower) { if let Some(worktree) = done_parse_worktree_path(&entry.path(), repos_root) { out.push(worktree); } }
    }
}

fn done_parse_worktree_path(full_path: &std::path::Path, repos_root: &std::path::Path) -> Option<DoneWorktree> {
    let rel = full_path.strip_prefix(repos_root).ok()?;
    let parts = rel.components().map(|part| part.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>();
    if parts.len() >= 4 && parts.get(2).is_some_and(|part| part == "agents") {
        let main_path = repos_root.join(&parts[0]).join(&parts[1]);
        let label = parts.join("/");
        return Some(DoneWorktree { main_path, full_path: full_path.to_path_buf(), label });
    }
    if parts.len() == 2 && parts[1].contains(".wt-") {
        let repo = parts[1].split_once(".wt-")?.0;
        let main_path = repos_root.join(&parts[0]).join(repo);
        return Some(DoneWorktree { main_path, full_path: full_path.to_path_buf(), label: parts[1].clone() });
    }
    None
}

fn done_remove_from_fleet_config(window_lower: &str, context: &DoneContext, stdout: &mut String) -> bool {
    let mut removed = false;
    for file in done_fleet_config_files(context) {
        let Ok(raw) = std::fs::read_to_string(&file) else { continue; };
        let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&raw) else { continue; };
        let before = json.get("windows").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
        if let Some(windows) = json.get_mut("windows").and_then(serde_json::Value::as_array_mut) { windows.retain(|item| !item.get("name").and_then(serde_json::Value::as_str).is_some_and(|name| name.eq_ignore_ascii_case(window_lower))); }
        if json.get("windows").and_then(serde_json::Value::as_array).map_or(0, Vec::len) < before {
            if let Ok(text) = serde_json::to_string_pretty(&json) { let _ = std::fs::write(&file, format!("{text}\n")); }
            let file_name = file.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("fleet.json");
            let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m removed from {file_name}");
            removed = true;
        }
    }
    removed
}

fn done_fleet_config_files(context: &DoneContext) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(context.config_dir.join("fleet")) else { return Vec::new(); };
    let mut files = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")).collect::<Vec<_>>();
    files.sort();
    files
}

fn done_git(args: &[String]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(args).output().map_err(|error| format!("git failed: {error}"))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).to_string()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

fn done_fail_missing_target(window_name: &str) -> Result<(), String> {
    let hint = if window_name.eq_ignore_ascii_case("all") { "\n  did you mean `maw done --all`?" } else { "" };
    Err(format!("no done target matched '{window_name}'{hint}"))
}

fn done_normalize_target(value: &str) -> String { value.trim().to_owned() }

fn done_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed != value { return Err(format!("done: invalid {label} '{value}'")); }
    Ok(())
}

fn done_validate_tmux_target(value: &str) -> Result<(), String> { if value.trim().is_empty() || value.starts_with('-') { Err(format!("done: invalid tmux target '{value}'")) } else { Ok(()) } }

fn done_validate_exec_path(path: &std::path::Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.components().any(|part| part.as_os_str().to_string_lossy().starts_with('-')) { return Err(format!("done: refusing leading-dash path '{}'", path.display())); }
    Ok(())
}

#[cfg(test)]
mod done_tests {
    use super::*;

    #[test]
    fn done_parse_rejects_leading_dash_positionals() {
        assert_eq!(done_parse_args(&["-Sbad".to_owned()]).unwrap_err(), "done: unknown argument -Sbad");
    }

    #[test]
    fn done_parse_matches_js_extra_positionals() {
        let err = done_parse_args(&["all".to_owned(), "x".to_owned()]).unwrap_err();
        assert!(err.contains("did you mean `maw done --all`?"), "{err}");
    }

    #[test]
    fn done_worktree_path_parses_agents_and_dot_wt() {
        let root = std::path::Path::new("/tmp/ghq/github.com");
        let agents = done_parse_worktree_path(std::path::Path::new("/tmp/ghq/github.com/org/repo/agents/task"), root).unwrap();
        assert_eq!(agents.main_path, std::path::PathBuf::from("/tmp/ghq/github.com/org/repo"));
        let dot = done_parse_worktree_path(std::path::Path::new("/tmp/ghq/github.com/org/repo.wt-task"), root).unwrap();
        assert_eq!(dot.main_path, std::path::PathBuf::from("/tmp/ghq/github.com/org/repo"));
    }
}
