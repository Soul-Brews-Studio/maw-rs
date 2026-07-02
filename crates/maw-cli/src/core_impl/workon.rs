const DISPATCH_49: &[DispatcherEntry] = &[
    DispatcherEntry { command: "workon", handler: Handler::Sync(run_workon_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkonOptions {
    repo: String,
    task: Option<String>,
    layout: WorkonLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkonLayout {
    Nested,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkonRepo {
    repo_path: std::path::PathBuf,
    repo_name: String,
    parent_dir: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkonWorktree {
    path: std::path::PathBuf,
    name: String,
}

impl maw_matcher::Named for WorkonWorktree {
    fn name(&self) -> &str { &self.name }
}

fn run_workon_command(argv: &[String]) -> CliOutput {
    match workon_parse_args(argv).and_then(|options| workon_cmd(&options)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn workon_parse_args(argv: &[String]) -> Result<WorkonOptions, String> {
    let mut positional = Vec::new();
    let mut layout = WorkonLayout::Nested;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(workon_usage()),
            "--layout" => {
                let Some(value) = argv.get(index + 1) else { return Err("workon: --layout must be nested or legacy".to_owned()); };
                layout = workon_parse_layout(value)?;
                index += 2;
            }
            value if value.starts_with('-') => return Err(workon_usage()),
            value => {
                positional.push(value.to_owned());
                index += 1;
            }
        }
    }
    let Some(repo) = positional.first().cloned() else { return Err(workon_usage()); };
    if positional.len() > 2 { return Err(workon_usage()); }
    workon_validate_query(&repo, "repo")?;
    if let Some(task) = positional.get(1) { workon_validate_query(task, "task")?; }
    Ok(WorkonOptions { repo, task: positional.get(1).cloned(), layout })
}

fn workon_parse_layout(raw: &str) -> Result<WorkonLayout, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "nested" => Ok(WorkonLayout::Nested),
        "legacy" => Ok(WorkonLayout::Legacy),
        _ => Err("workon: --layout must be nested or legacy".to_owned()),
    }
}

fn workon_usage() -> String { "usage: maw workon <repo> [task] [--layout nested|legacy]".to_owned() }

fn workon_cmd(options: &WorkonOptions) -> Result<String, String> {
    let repo = workon_resolve_repo(&options.repo)?;
    workon_cmd_with_runner(options, &repo, &mut maw_tmux::CommandTmuxRunner::new())
}

fn workon_cmd_with_runner<R: maw_tmux::TmuxRunner>(
    options: &WorkonOptions,
    repo: &WorkonRepo,
    runner: &mut R,
) -> Result<String, String> {
    let mut stdout = String::new();
    let mut target_path = repo.repo_path.clone();
    let mut window_name = repo.repo_name.clone();
    let mut taskless_oracle = false;

    if let Some(task) = &options.task {
        let task = workon_sanitize_task_slug(task);
        let worktrees = workon_find_worktrees(&repo.parent_dir, &repo.repo_name);
        match maw_matcher::resolve_worktree_target(&task, &worktrees) {
            ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => {
                let _ = writeln!(stdout, "\x1b[33m⚡\x1b[0m reusing worktree: {}", matched.path.display());
                target_path = matched.path;
            }
            ResolveResult::Ambiguous { candidates } => {
                let _ = writeln!(stdout, "\x1b[31m✗\x1b[0m '{task}' is ambiguous — matches {} worktrees:", candidates.len());
                for candidate in &candidates {
                    let _ = writeln!(stdout, "\x1b[90m    • {}\x1b[0m", candidate.name);
                }
                let _ = writeln!(stdout, "\x1b[90m  use the full name: maw workon {} <exact-worktree>\x1b[0m", options.repo);
                return Err(stdout.trim_end().to_owned());
            }
            ResolveResult::None { .. } => {
                let wt_name = format!("{}-{task}", workon_next_worktree_number(&worktrees));
                let wt_path = workon_worktree_path_for_layout(repo, &wt_name, options.layout);
                let branch = format!("agents/{wt_name}");
                workon_delete_branch(&repo.repo_path, &branch);
                if matches!(options.layout, WorkonLayout::Nested) {
                    std::fs::create_dir_all(repo.repo_path.join("agents"))
                        .map_err(|error| format!("workon: create agents dir: {error}"))?;
                }
                workon_git(&repo.repo_path, &["worktree", "add", workon_path_str(&wt_path)?, "-b", &branch])?;
                let _ = writeln!(stdout, "\x1b[32m+\x1b[0m worktree: {} ({branch})", wt_path.display());
                target_path = wt_path;
            }
        }
        window_name = format!("{}-{task}", repo.repo_name);
    } else if repo.repo_name.ends_with("-oracle") {
        taskless_oracle = true;
    }

    if std::env::var_os("TMUX").is_none() { return Err("not in a tmux session — run inside tmux".to_owned()); }
    let session = workon_tmux_run(runner, "display-message", &["-p", "#{session_name}"])?;
    if session.is_empty() { return Err("could not detect current tmux session".to_owned()); }
    workon_validate_tmux_target(&session)?;
    workon_validate_tmux_target(&format!("{session}:{window_name}"))?;

    let windows = workon_list_windows(runner, &session)?;
    if windows.iter().any(|name| name == &window_name) {
        workon_tmux_run(runner, "select-window", &["-t", &format!("{session}:{window_name}")])?;
        let _ = writeln!(stdout, "\x1b[33m⚡\x1b[0m reusing existing window '{window_name}' in {session}");
        return Ok(stdout);
    }

    workon_tmux_run(
        runner,
        "new-window",
        &["-t", &session, "-n", &window_name, "-c", workon_path_str(&target_path)?],
    )?;
    let command = workon_build_command_in_dir(&window_name, &target_path);
    let send_text_args = maw_tmux::tmux_send_keys_literal_args(&format!("{session}:{window_name}"), &command);
    workon_tmux_run_owned(runner, "send-keys", &send_text_args)?;
    let send_enter_args = maw_tmux::tmux_send_enter_args(&format!("{session}:{window_name}"));
    workon_tmux_run_owned(runner, "send-keys", &send_enter_args)?;

    if taskless_oracle {
        if let WorkonFleetStatus::Created = workon_ensure_fleet_session_entry(&session, &window_name, &target_path)? {
            let _ = writeln!(stdout, "\x1b[32m+\x1b[0m fleet registered {session}:{window_name}");
        }
    }

    let _ = writeln!(stdout, "\x1b[32m✅\x1b[0m workon '{window_name}' in {session} → {}", target_path.display());
    Ok(stdout)
}

fn workon_resolve_repo(repo: &str) -> Result<WorkonRepo, String> {
    let search_term = repo.rsplit('/').next().unwrap_or(repo);
    let Some(repo_path) = workon_ghq_find(search_term) else { return Err(format!("repo not found: {repo}")); };
    let repo_name = repo_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_owned();
    let parent_dir = repo_path.parent().ok_or_else(|| format!("workon: repo has no parent: {}", repo_path.display()))?.to_path_buf();
    Ok(WorkonRepo { repo_path, repo_name, parent_dir })
}

fn workon_ghq_find(search_term: &str) -> Option<std::path::PathBuf> {
    if search_term.is_empty() || search_term.starts_with('-') || search_term.contains("..") { return None; }
    let root = ghq_root().join("github.com");
    let mut matches = Vec::new();
    let Ok(orgs) = std::fs::read_dir(root) else { return None; };
    for org in orgs.flatten() {
        let candidate = org.path().join(search_term);
        if candidate.is_dir() { matches.push(candidate); }
    }
    matches.sort();
    matches.into_iter().next()
}

fn workon_find_worktrees(parent_dir: &std::path::Path, repo_name: &str) -> Vec<WorkonWorktree> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let prefix = format!("{repo_name}.wt-");
            if path.is_dir() && name.starts_with(&prefix) && path.join(".git").exists() {
                out.push(WorkonWorktree { name: name[prefix.len()..].to_owned(), path });
            }
        }
    }
    let nested = parent_dir.join(repo_name).join("agents");
    if let Ok(entries) = std::fs::read_dir(nested) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".git").exists() {
                out.push(WorkonWorktree { name: entry.file_name().to_string_lossy().into_owned(), path });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

fn workon_sanitize_task_slug(task: &str) -> String { task.replace('/', "-") }

fn workon_next_worktree_number(worktrees: &[WorkonWorktree]) -> i32 {
    worktrees.iter().filter_map(|wt| workon_parse_js_i32_prefix(&wt.name)).max().unwrap_or(0) + 1
}

fn workon_parse_js_i32_prefix(value: &str) -> Option<i32> {
    let trimmed = value.trim_start();
    let (sign, digits) = trimmed
        .strip_prefix('-')
        .map_or((1_i32, trimmed), |tail| (-1_i32, tail));
    let digits = digits
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<i32>().ok().and_then(|number| number.checked_mul(sign)))
        .flatten()
}

fn workon_worktree_path_for_layout(repo: &WorkonRepo, wt_name: &str, layout: WorkonLayout) -> std::path::PathBuf {
    match layout {
        WorkonLayout::Legacy => repo.parent_dir.join(format!("{}.wt-{wt_name}", repo.repo_name)),
        WorkonLayout::Nested => repo.repo_path.join("agents").join(wt_name),
    }
}

fn workon_delete_branch(repo_path: &std::path::Path, branch: &str) {
    let _ = std::process::Command::new("git").arg("-C").arg(repo_path).args(["branch", "-D", branch]).output();
}

fn workon_git(repo_path: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .map_err(|error| format!("workon: failed to execute git: {error}"))?;
    if output.status.success() { return Ok(String::from_utf8_lossy(&output.stdout).into_owned()); }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if stderr.is_empty() { "workon: git failed".to_owned() } else { format!("workon: git failed: {stderr}") })
}

fn workon_tmux_run<R: maw_tmux::TmuxRunner>(runner: &mut R, subcommand: &str, args: &[&str]) -> Result<String, String> {
    runner
        .run(subcommand, &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        .map(|out| out.trim().to_owned())
        .map_err(|error| error.message)
}

fn workon_list_windows<R: maw_tmux::TmuxRunner>(runner: &mut R, session: &str) -> Result<Vec<String>, String> {
    let raw = workon_tmux_run(runner, "list-windows", &["-t", session, "-F", "#{window_name}"])?;
    Ok(raw.lines().map(str::to_owned).filter(|line| !line.is_empty()).collect())
}

fn workon_tmux_run_owned<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[String],
) -> Result<String, String> {
    runner
        .run(subcommand, args)
        .map(|out| out.trim().to_owned())
        .map_err(|error| error.message)
}

fn workon_build_command_in_dir(agent_name: &str, cwd: &std::path::Path) -> String {
    let config = active_config_dir().join("maw.config.json");
    let command = std::fs::read_to_string(config)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("commands").cloned())
        .and_then(|commands| {
            commands.get(agent_name).and_then(serde_json::Value::as_str)
                .or_else(|| commands.get("default").and_then(serde_json::Value::as_str))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "claude".to_owned());
    let _ = cwd;
    command
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkonFleetStatus { Created, Exists, Skipped }

fn workon_ensure_fleet_session_entry(session: &str, window: &str, cwd: &std::path::Path) -> Result<WorkonFleetStatus, String> {
    if !workon_safe_fleet_session_name(session) || window.trim().is_empty() { return Ok(WorkonFleetStatus::Skipped); }
    let repo = workon_repo_from_cwd(cwd).ok_or(WorkonFleetStatus::Skipped).map_err(|_| "workon: skipped fleet registration".to_owned())?;
    let fleet_dir = active_config_dir().join("fleet");
    std::fs::create_dir_all(&fleet_dir).map_err(|error| format!("workon: create fleet dir: {error}"))?;
    let path = fleet_dir.join(format!("{session}.json"));
    if path.exists() { return Ok(WorkonFleetStatus::Exists); }
    let json = serde_json::json!({
        "name": session,
        "created_by": "maw workon",
        "auto_registered": true,
        "windows": [{"name": window, "repo": repo}],
    });
    std::fs::write(&path, serde_json::to_string_pretty(&json).map_err(|error| format!("workon: render fleet json: {error}"))? + "\n")
        .map_err(|error| format!("workon: write {}: {error}", path.display()))?;
    Ok(WorkonFleetStatus::Created)
}

fn workon_repo_from_cwd(cwd: &std::path::Path) -> Option<String> {
    let root = ghq_root().join("github.com");
    let rel = cwd.strip_prefix(root).ok()?;
    let mut comps = rel.components();
    let org = comps.next()?.as_os_str().to_string_lossy();
    let repo = comps.next()?.as_os_str().to_string_lossy();
    Some(format!("{org}/{repo}"))
}

fn workon_safe_fleet_session_name(session: &str) -> bool {
    !session.is_empty() && session.trim() == session && !session.starts_with('-') && session.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn workon_validate_query(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains("..") {
        Err(format!("workon: {name} must be non-empty, unpadded, and not start with '-'"))
    } else { Ok(()) }
}

fn workon_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    Ok(())
}

fn workon_path_str(path: &std::path::Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| format!("workon: path is not utf8: {}", path.display()))
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod workon_tests {
    use super::*;

    #[derive(Default)]
    struct WorkonMockTmux { calls: Vec<(String, Vec<String>)>, session: String, windows: String }

    impl maw_tmux::TmuxRunner for WorkonMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" => Ok(self.session.clone()),
                "list-windows" => Ok(self.windows.clone()),
                "new-window" | "send-keys" | "select-window" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn workon_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn workon_parse_layout_and_usage() {
        assert!(workon_parse_args(&[]).expect_err("usage").contains("usage: maw workon"));
        assert!(workon_parse_args(&workon_strings(&["repo", "task", "extra"])).is_err());
        assert!(workon_parse_args(&workon_strings(&["repo", "--layout", "wide"])).expect_err("layout").contains("nested or legacy"));
    }

    #[test]
    fn workon_reuses_existing_window_before_spawn() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = WorkonOptions { repo: "demo".to_owned(), task: None, layout: WorkonLayout::Nested };
        let mut runner = WorkonMockTmux { session: "50-mawjs\n".to_owned(), windows: "demo\n".to_owned(), ..Default::default() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::set_var("TMUX", "/tmp/tmux,1,0");

        let stdout = workon_cmd_with_runner(&options, &repo, &mut runner).expect("reuse");

        assert_eq!(stdout, "\x1b[33m⚡\x1b[0m reusing existing window 'demo' in 50-mawjs\n");
        assert_eq!(runner.calls[2], ("select-window".to_owned(), workon_strings(&["-t", "50-mawjs:demo"])));
    }

    #[test]
    fn workon_tmux_target_guard_blocks_bad_session() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = WorkonOptions { repo: "demo".to_owned(), task: None, layout: WorkonLayout::Nested };
        let mut runner = WorkonMockTmux { session: "-Sbad\n".to_owned(), windows: String::new(), ..Default::default() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::set_var("TMUX", "/tmp/tmux,1,0");

        let err = workon_cmd_with_runner(&options, &repo, &mut runner).expect_err("guard");

        assert!(err.contains("tmux target/session"));
        assert_eq!(runner.calls.len(), 1);
    }
}
