const DISPATCH_49: &[DispatcherEntry] = &[
    DispatcherEntry { command: "workon", handler: Handler::Sync(run_workon_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkonOptions {
    repo: String,
    task: Option<String>,
    wt: Option<WorkonWorktreeRequest>,
    fresh: bool,
    name: Option<String>,
    engine: Option<String>,
    layout: WorkonLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkonWorktreeRequest {
    Auto,
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkonLayout {
    Nested,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkonOutsideSession {
    Existing(String),
    Create(String),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkonResolvedWorktreeName {
    slug: String,
    named: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkonWorktreePlan {
    Reuse {
        path: std::path::PathBuf,
    },
    Create {
        wt_name: String,
        wt_path: std::path::PathBuf,
        branch: String,
        branch_exists: bool,
    },
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
    let mut wt = None;
    let mut fresh = false;
    let mut name = None;
    let mut engine = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(workon_usage()),
            "--layout" => {
                let Some(value) = argv.get(index + 1) else { return Err("workon: --layout must be nested or legacy".to_owned()); };
                layout = workon_parse_layout(value)?;
                index += 2;
            }
            value if value.starts_with("--layout=") => {
                layout = workon_parse_layout(&value["--layout=".len()..])?;
                index += 1;
            }
            "--wt" => {
                if let Some(value) = argv.get(index + 1).filter(|value| !value.starts_with('-')) {
                    workon_validate_slug_input(value, "--wt")?;
                    wt = Some(WorkonWorktreeRequest::Named(value.clone()));
                    index += 2;
                } else {
                    wt = Some(WorkonWorktreeRequest::Auto);
                    index += 1;
                }
            }
            value if value.starts_with("--wt=") => {
                let value = &value["--wt=".len()..];
                workon_validate_slug_input(value, "--wt")?;
                wt = Some(WorkonWorktreeRequest::Named(value.to_owned()));
                index += 1;
            }
            "--fresh" | "--new" => {
                fresh = true;
                index += 1;
            }
            "--name" => {
                let Some(value) = argv.get(index + 1) else { return Err("workon: --name requires a value".to_owned()); };
                workon_validate_slug_input(value, "--name")?;
                name = Some(value.clone());
                index += 2;
            }
            value if value.starts_with("--name=") => {
                let value = &value["--name=".len()..];
                workon_validate_slug_input(value, "--name")?;
                name = Some(value.to_owned());
                index += 1;
            }
            "-e" | "--engine" => {
                let Some(value) = argv.get(index + 1) else { return Err(format!("workon: {} requires a value", argv[index])); };
                workon_validate_query(value, "engine")?;
                engine = Some(value.clone());
                index += 2;
            }
            value if value.starts_with("--engine=") => {
                let value = &value["--engine=".len()..];
                workon_validate_query(value, "engine")?;
                engine = Some(value.to_owned());
                index += 1;
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
    if let Some(task) = positional.get(1) { workon_validate_slug_input(task, "task")?; }
    if wt.is_some() && positional.len() > 1 {
        return Err("workon: use either positional task or --wt, not both".to_owned());
    }
    if wt.is_none() && positional.len() == 1 && (fresh || name.is_some()) {
        return Err("workon: --fresh/--name requires --wt or a task".to_owned());
    }
    Ok(WorkonOptions { repo, task: positional.get(1).cloned(), wt, fresh, name, engine, layout })
}

fn workon_parse_layout(raw: &str) -> Result<WorkonLayout, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "nested" => Ok(WorkonLayout::Nested),
        "legacy" => Ok(WorkonLayout::Legacy),
        _ => Err("workon: --layout must be nested or legacy".to_owned()),
    }
}

fn workon_usage() -> String {
    "usage: maw workon <repo|.|path|url> [task] [--wt [slug]] [--fresh] [--name <stable>] [-e <engine>] [--layout nested|legacy]".to_owned()
}

fn workon_cmd(options: &WorkonOptions) -> Result<String, String> {
    let repo = workon_resolve_repo(&options.repo)?;
    let (stdout, attach_session) = workon_cmd_with_runner(options, &repo, &mut maw_tmux::CommandTmuxRunner::new())?;
    let Some(session) = attach_session else { return Ok(stdout) };
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return Ok(format!("{stdout}run: tmux attach -t {session}\n"));
    }
    print!("{stdout}");
    workon_attach_interactive(&session)?;
    Ok(String::new())
}

fn workon_attach_interactive(session: &str) -> Result<(), String> {
    let status = std::process::Command::new("tmux")
        .args(["attach", "-t", session])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|error| format!("workon: failed to attach tmux: {error}"))?;
    if status.success() { Ok(()) } else { Err(format!("workon: tmux attach exited with status {status}")) }
}

fn workon_cmd_with_runner<R: maw_tmux::TmuxRunner>(
    options: &WorkonOptions,
    repo: &WorkonRepo,
    runner: &mut R,
) -> Result<(String, Option<String>), String> {
    let mut stdout = String::new();
    let mut target_path = repo.repo_path.clone();
    let mut window_name = repo.repo_name.clone();
    let mut taskless_oracle = false;

    if let Some(request) = workon_resolve_worktree_name(options)? {
        let worktrees = workon_find_worktrees(&repo.parent_dir, &repo.repo_name);
        let branches = workon_agent_branches(&repo.repo_path)?;
        match workon_plan_worktree(repo, &request, options.fresh, options.layout, &worktrees, &branches)? {
            WorkonWorktreePlan::Reuse { path } => {
                let _ = writeln!(stdout, "\x1b[33m⚡\x1b[0m reusing worktree: {}", path.display());
                target_path = path;
            }
            WorkonWorktreePlan::Create { wt_path, branch, branch_exists, .. } => {
                workon_create_worktree(repo, &wt_path, &branch, branch_exists, options.layout)?;
                let suffix = if branch_exists { ", reused branch" } else { "" };
                let _ = writeln!(stdout, "\x1b[32m+\x1b[0m worktree: {} ({branch}{suffix})", wt_path.display());
                target_path = wt_path;
            }
        }
        window_name = format!("{}-{}", repo.repo_name, request.slug);
    } else if native_repo_path_is_oracle(&repo.repo_path, &repo.repo_name) {
        taskless_oracle = true;
    }

    if std::env::var_os("TMUX").is_some() {
        let session = workon_tmux_run(runner, "display-message", &["-p", "#{session_name}"])?;
        if session.is_empty() { return Err("could not detect current tmux session".to_owned()); }
        workon_ensure_window(
            runner,
            WorkonWindowLaunch {
                session: &session,
                window_name: &window_name,
                target_path: &target_path,
                taskless_oracle,
                force_new_window: options.fresh,
                engine: options.engine.as_deref(),
            },
            &mut stdout,
        )?;
        return Ok((stdout, None));
    }

    // outside tmux: attach-or-create a session for the repo
    // (deliberate divergence — maw-js errors "not in a tmux session" here)
    match workon_resolve_outside_session(runner, &repo.repo_name)? {
        WorkonOutsideSession::Create(session) => {
            workon_tmux_run(
                runner,
                "new-session",
                &["-d", "-s", &session, "-c", workon_path_str(&target_path)?, "-n", &window_name],
            )?;
            workon_send_window_command(runner, &session, &window_name, &target_path, options.engine.as_deref())?;
            if taskless_oracle {
                if let WorkonFleetStatus::Created = workon_ensure_fleet_session_entry(&session, &window_name, &target_path)? {
                    let _ = writeln!(stdout, "\x1b[32m+\x1b[0m fleet registered {session}:{window_name}");
                }
            }
            let _ = writeln!(stdout, "\x1b[32m✅\x1b[0m workon '{window_name}' in new session {session} → {}", target_path.display());
            Ok((stdout, Some(session)))
        }
        WorkonOutsideSession::Existing(session) => {
            workon_ensure_window(
                runner,
                WorkonWindowLaunch {
                    session: &session,
                    window_name: &window_name,
                    target_path: &target_path,
                    taskless_oracle,
                    force_new_window: options.fresh,
                    engine: options.engine.as_deref(),
                },
                &mut stdout,
            )?;
            Ok((stdout, Some(session)))
        }
    }
}

fn workon_resolve_outside_session<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    repo_name: &str,
) -> Result<WorkonOutsideSession, String> {
    workon_validate_tmux_target(repo_name)?;
    if workon_tmux_run(runner, "has-session", &["-t", &format!("={repo_name}")]).is_ok() {
        return Ok(WorkonOutsideSession::Existing(repo_name.to_owned()));
    }

    let sessions = workon_list_sessions(runner);
    match maw_matcher::resolve_numeric_fleet_stem_exact(repo_name, &sessions) {
        ResolveResult::Exact { matched } => {
            workon_validate_tmux_target(&matched)?;
            Ok(WorkonOutsideSession::Existing(matched))
        }
        ResolveResult::Ambiguous { candidates } => Err(format!(
            "workon: '{repo_name}' matches multiple numbered fleet sessions: {}\n  refusing to create sibling session {repo_name}",
            candidates.join(", ")
        )),
        ResolveResult::None { .. } | ResolveResult::Fuzzy { .. } => {
            Ok(WorkonOutsideSession::Create(repo_name.to_owned()))
        }
    }
}

#[derive(Clone, Copy)]
struct WorkonWindowLaunch<'a> {
    session: &'a str,
    window_name: &'a str,
    target_path: &'a std::path::Path,
    taskless_oracle: bool,
    force_new_window: bool,
    engine: Option<&'a str>,
}

fn workon_ensure_window<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    launch: WorkonWindowLaunch<'_>,
    stdout: &mut String,
) -> Result<(), String> {
    let WorkonWindowLaunch {
        session,
        window_name,
        target_path,
        taskless_oracle,
        force_new_window,
        engine,
    } = launch;

    workon_validate_tmux_target(session)?;
    workon_validate_tmux_target(&format!("{session}:{window_name}"))?;

    let windows = workon_list_windows(runner, session)?;
    if !force_new_window && windows.iter().any(|name| name == window_name) {
        workon_tmux_run(runner, "select-window", &["-t", &format!("{session}:{window_name}")])?;
        let _ = writeln!(stdout, "\x1b[33m⚡\x1b[0m reusing existing window '{window_name}' in {session}");
        return Ok(());
    }

    let new_target = workon_new_window(runner, session, window_name, target_path)?;
    workon_send_window_command_to_target(runner, &new_target, window_name, target_path, engine)?;

    if taskless_oracle {
        if let WorkonFleetStatus::Created = workon_ensure_fleet_session_entry(session, window_name, target_path)? {
            let _ = writeln!(stdout, "\x1b[32m+\x1b[0m fleet registered {session}:{window_name}");
        }
    }

    let _ = writeln!(stdout, "\x1b[32m✅\x1b[0m workon '{window_name}' in {session} → {}", target_path.display());
    Ok(())
}

fn workon_send_window_command<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    session: &str,
    window_name: &str,
    target_path: &std::path::Path,
    engine: Option<&str>,
) -> Result<(), String> {
    let target = format!("{session}:{window_name}");
    workon_send_window_command_to_target(runner, &target, window_name, target_path, engine)
}

fn workon_send_window_command_to_target<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    window_name: &str,
    target_path: &std::path::Path,
    engine: Option<&str>,
) -> Result<(), String> {
    let command = workon_build_command_in_dir(window_name, target_path, engine);
    #[cfg(test)]
    let sleeper = |_| {};
    #[cfg(not(test))]
    let sleeper = std::thread::sleep;
    sendtext_send_text(runner, target, &command, sleeper)
        .map(|_| ())
        .map_err(|error| error.message)
}

fn workon_new_window<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    session: &str,
    window_name: &str,
    target_path: &std::path::Path,
) -> Result<String, String> {
    let session_target = format!("{session}:");
    let window_id = workon_tmux_run(
        runner,
        "new-window",
        &[
            "-P",
            "-F",
            "#{window_id}",
            "-t",
            &session_target,
            "-n",
            window_name,
            "-c",
            workon_path_str(target_path)?,
        ],
    )?;
    Ok(if window_id.is_empty() { format!("{session}:{window_name}") } else { window_id })
}

fn workon_resolve_worktree_name(options: &WorkonOptions) -> Result<Option<WorkonResolvedWorktreeName>, String> {
    let Some(raw) = workon_raw_worktree_slug(options) else { return Ok(None); };
    let requested = workon_sanitize_task_slug(&raw);
    if requested.is_empty() {
        return Err("workon: worktree slug collapsed to empty".to_owned());
    }
    let stable = options
        .name
        .as_deref()
        .map(workon_sanitize_task_slug)
        .filter(|value| !value.is_empty());
    let slug = if let Some(stable) = &stable {
        if options.wt.is_some() && &requested != stable {
            workon_sanitize_task_slug(&format!("{stable}-{requested}"))
        } else {
            stable.clone()
        }
    } else {
        requested
    };
    if slug.is_empty() {
        return Err("workon: worktree slug collapsed to empty".to_owned());
    }
    Ok(Some(WorkonResolvedWorktreeName { slug, named: stable.is_some() && !options.fresh }))
}

fn workon_raw_worktree_slug(options: &WorkonOptions) -> Option<String> {
    match &options.wt {
        Some(WorkonWorktreeRequest::Named(value)) => Some(value.clone()),
        Some(WorkonWorktreeRequest::Auto) => options
            .name
            .clone()
            .or_else(|| options.engine.clone())
            .or_else(|| Some("codex".to_owned())),
        None => options.task.clone(),
    }
}

fn workon_plan_worktree(
    repo: &WorkonRepo,
    request: &WorkonResolvedWorktreeName,
    fresh: bool,
    layout: WorkonLayout,
    worktrees: &[WorkonWorktree],
    branches: &std::collections::BTreeSet<String>,
) -> Result<WorkonWorktreePlan, String> {
    if !fresh {
        if let Some(reuse) = workon_find_reusable_worktree(&request.slug, worktrees)? {
            return Ok(WorkonWorktreePlan::Reuse { path: reuse.path });
        }
    }

    if request.named {
        let wt_name = request.slug.clone();
        let wt_path = workon_worktree_path_for_layout(repo, &wt_name, layout);
        let branch = format!("agents/{wt_name}");
        let branch_exists = branches.contains(&branch);
        return Ok(WorkonWorktreePlan::Create { wt_name, wt_path, branch, branch_exists });
    }

    let mut next = workon_next_worktree_number(worktrees, branches);
    for _ in 0..1000 {
        let wt_name = format!("{next}-{}", request.slug);
        let wt_path = workon_worktree_path_for_layout(repo, &wt_name, layout);
        let branch = format!("agents/{wt_name}");
        let known_worktree = worktrees.iter().any(|wt| wt.name == wt_name || wt.path == wt_path);
        if known_worktree || branches.contains(&branch) {
            next += 1;
            continue;
        }
        return Ok(WorkonWorktreePlan::Create { wt_name, wt_path, branch, branch_exists: false });
    }
    Err(format!("workon: could not allocate worktree for {}", request.slug))
}

fn workon_find_reusable_worktree(
    slug: &str,
    worktrees: &[WorkonWorktree],
) -> Result<Option<WorkonWorktree>, String> {
    match maw_matcher::resolve_worktree_target(slug, worktrees) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => Ok(Some(matched)),
        ResolveResult::None { .. } => Ok(None),
        ResolveResult::Ambiguous { candidates } => Err(workon_ambiguous_worktree_error(slug, &candidates)),
    }
}

fn workon_ambiguous_worktree_error(slug: &str, candidates: &[WorkonWorktree]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[31m✗\x1b[0m '{slug}' is ambiguous — matches {} worktrees:", candidates.len());
    for candidate in candidates {
        let _ = writeln!(out, "\x1b[90m    • {}\x1b[0m", candidate.name);
    }
    let _ = writeln!(out, "\x1b[90m  use the full name: maw workon <repo> <exact-worktree>\x1b[0m");
    out.trim_end().to_owned()
}

fn workon_create_worktree(
    repo: &WorkonRepo,
    wt_path: &std::path::Path,
    branch: &str,
    branch_exists: bool,
    layout: WorkonLayout,
) -> Result<(), String> {
    if matches!(layout, WorkonLayout::Nested) {
        std::fs::create_dir_all(repo.repo_path.join("agents"))
            .map_err(|error| format!("workon: create agents dir: {error}"))?;
    }
    if branch_exists {
        workon_git(&repo.repo_path, &["worktree", "add", workon_path_str(wt_path)?, branch])?;
    } else {
        workon_git(&repo.repo_path, &["worktree", "add", workon_path_str(wt_path)?, "-b", branch])?;
    }
    Ok(())
}

fn workon_resolve_repo(repo: &str) -> Result<WorkonRepo, String> {
    if repo == "." || repo.starts_with("./") || repo.starts_with('/') {
        return workon_resolve_repo_from_path(std::path::Path::new(repo));
    }
    if let Some(slug) = workon_github_slug(repo) {
        let repo_path = ghq_root().join("github.com").join(&slug);
        if !repo_path.is_dir() {
            workon_ghq_get(repo, &slug)?;
        }
        if repo_path.is_dir() {
            return workon_resolve_repo_from_ghq_path(repo_path);
        }
    }
    let search_term = repo.rsplit('/').next().unwrap_or(repo);
    let Some(repo_path) = workon_ghq_find(search_term) else { return Err(format!("repo not found: {repo}")); };
    workon_resolve_repo_from_ghq_path(repo_path)
}

fn workon_resolve_repo_from_ghq_path(repo_path: std::path::PathBuf) -> Result<WorkonRepo, String> {
    let repo_name = repo_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_owned();
    let parent_dir = repo_path.parent().ok_or_else(|| format!("workon: repo has no parent: {}", repo_path.display()))?.to_path_buf();
    Ok(WorkonRepo { repo_path, repo_name, parent_dir })
}

fn workon_resolve_repo_from_path(dir: &std::path::Path) -> Result<WorkonRepo, String> {
    let toplevel = workon_git(dir, &["rev-parse", "--show-toplevel"])
        .map_err(|_| format!("workon: '{}' is not inside a git repository", dir.display()))?;
    let repo_path = std::path::PathBuf::from(toplevel.trim());
    if repo_path.as_os_str().is_empty() {
        return Err(format!("workon: cannot resolve git toplevel for '{}'", dir.display()));
    }
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

fn workon_github_slug(value: &str) -> Option<String> {
    let mut raw = value.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(rest) = raw.strip_prefix("https://github.com/").or_else(|| raw.strip_prefix("http://github.com/")) {
        raw = rest;
    } else if let Some(rest) = raw.strip_prefix("git@github.com:") {
        raw = rest;
    } else if let Some(rest) = raw.strip_prefix("github.com/") {
        raw = rest;
    } else if raw.matches('/').count() != 1 || raw.contains(':') || raw.starts_with('.') {
        return None;
    }
    let (org, repo) = raw.split_once('/')?;
    if workon_valid_github_segment(org) && workon_valid_github_segment(repo) {
        Some(format!("{org}/{repo}"))
    } else {
        None
    }
}

fn workon_valid_github_segment(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('-')
        && !value.contains("..")
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn workon_ghq_get(input: &str, slug: &str) -> Result<(), String> {
    let target = if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("git@") {
        input.to_owned()
    } else {
        format!("github.com/{slug}")
    };
    let output = std::process::Command::new("ghq")
        .args(["get", &target])
        .output()
        .map_err(|error| format!("workon: failed to execute ghq get: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if stderr.is_empty() { "workon: ghq get failed".to_owned() } else { format!("workon: ghq get failed: {stderr}") })
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

fn workon_agent_branches(repo_path: &std::path::Path) -> Result<std::collections::BTreeSet<String>, String> {
    let raw = workon_git(repo_path, &["for-each-ref", "--format=%(refname:short)", "refs/heads/agents"])?;
    Ok(raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToOwned::to_owned).collect())
}

fn workon_sanitize_task_slug(task: &str) -> String {
    let mut out = String::new();
    let mut previous_space = false;
    for ch in task.to_ascii_lowercase().chars() {
        if ch.is_ascii_whitespace() {
            if !previous_space {
                out.push('-');
                previous_space = true;
            }
            continue;
        }
        previous_space = false;
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        }
    }
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    let trimmed = out.trim_matches(|ch| matches!(ch, '-' | '.')).to_owned();
    trimmed.chars().take(50).collect()
}

fn workon_next_worktree_number(
    worktrees: &[WorkonWorktree],
    branches: &std::collections::BTreeSet<String>,
) -> i32 {
    let worktree_max = worktrees.iter().filter_map(|wt| workon_parse_js_i32_prefix(&wt.name)).max();
    let branch_max = branches.iter().filter_map(|branch| branch.strip_prefix("agents/").and_then(workon_parse_js_i32_prefix)).max();
    worktree_max.into_iter().chain(branch_max).max().unwrap_or(0) + 1
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

fn workon_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Vec<String> {
    workon_tmux_run(runner, "list-sessions", &["-F", "#{session_name}"])
        .map(|raw| {
            let mut sessions = raw
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            sessions.sort();
            sessions.dedup();
            sessions
        })
        .unwrap_or_default()
}

fn workon_build_command_in_dir(agent_name: &str, cwd: &std::path::Path, engine: Option<&str>) -> String {
    let config = merged_config_value_in_dir(cwd);
    let commands = config.get("commands");
    if let Some(engine) = engine {
        return commands
            .and_then(|commands| commands.get(engine))
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| engine.to_owned(), str::to_owned);
    }
    commands
        .and_then(|commands| {
            commands.get(agent_name).and_then(serde_json::Value::as_str)
                .or_else(|| commands.get("default").and_then(serde_json::Value::as_str))
        })
        .map_or_else(|| "claude".to_owned(), str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkonFleetStatus { Created, Exists, Skipped }

fn workon_ensure_fleet_session_entry(session: &str, window: &str, cwd: &std::path::Path) -> Result<WorkonFleetStatus, String> {
    if !workon_safe_fleet_session_name(session) || window.trim().is_empty() { return Ok(WorkonFleetStatus::Skipped); }
    let repo = workon_repo_from_cwd(cwd).ok_or(WorkonFleetStatus::Skipped).map_err(|_| "workon: skipped fleet registration".to_owned())?;
    let env = current_xdg_env();
    if fleet_load_entries_for_env(&env).iter().any(|entry| entry.session.name == session) { return Ok(WorkonFleetStatus::Exists); }
    let fleet_dir = maw_state_path(&env, &["fleet"]);
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

fn workon_validate_slug_input(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('\0') || value.chars().any(char::is_control) {
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
    struct WorkonMockTmux {
        calls: Vec<(String, Vec<String>)>,
        session: String,
        sessions: String,
        windows: String,
        has_session: bool,
    }

    impl maw_tmux::TmuxRunner for WorkonMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" => Ok(self.session.clone()),
                "list-sessions" => Ok(self.sessions.clone()),
                "list-windows" => Ok(self.windows.clone()),
                "has-session" => {
                    if self.has_session { Ok(String::new()) } else { Err(maw_tmux::TmuxError::new("no session")) }
                }
                "new-window" | "new-session" | "send-keys" | "select-window" | "capture-pane" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn workon_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn workon_test_options(repo: &str, task: Option<&str>) -> WorkonOptions {
        WorkonOptions {
            repo: repo.to_owned(),
            task: task.map(str::to_owned),
            wt: None,
            fresh: false,
            name: None,
            engine: None,
            layout: WorkonLayout::Nested,
        }
    }

    fn workon_test_repo(root: &std::path::Path) -> WorkonRepo {
        WorkonRepo {
            repo_path: root.join("acme/demo"),
            repo_name: "demo".to_owned(),
            parent_dir: root.join("acme"),
        }
    }

    fn workon_branch_set(values: &[&str]) -> std::collections::BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn workon_temp_root(label: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-workon-{label}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    #[test]
    fn workon_parse_layout_and_usage() {
        assert!(workon_parse_args(&[]).expect_err("usage").contains("usage: maw workon"));
        assert!(workon_parse_args(&workon_strings(&["repo", "task", "extra"])).is_err());
        assert!(workon_parse_args(&workon_strings(&["repo", "--layout", "wide"])).expect_err("layout").contains("nested or legacy"));
        let parsed = workon_parse_args(&workon_strings(&["repo", "--wt", "--fresh", "-e", "codex"])).expect("bare wt");
        assert_eq!(parsed.wt, Some(WorkonWorktreeRequest::Auto));
        assert!(parsed.fresh);
        assert_eq!(parsed.engine.as_deref(), Some("codex"));
        assert!(workon_parse_args(&workon_strings(&["repo", "task", "--wt", "other"])).is_err());
    }

    #[test]
    fn workon_slug_derivation_matches_wake_wt_shape() {
        assert_eq!(workon_sanitize_task_slug("My Task Name"), "my-task-name");
        assert_eq!(workon_sanitize_task_slug("feat/foo"), "featfoo");
        assert_eq!(workon_sanitize_task_slug("foo.."), "foo");
        assert_eq!(workon_sanitize_task_slug("--no-attach"), "no-attach");
        assert_eq!(workon_sanitize_task_slug(&"A".repeat(80)), "a".repeat(50));

        let mut options = workon_test_options("demo", None);
        options.wt = Some(WorkonWorktreeRequest::Auto);
        assert_eq!(
            workon_resolve_worktree_name(&options).expect("name"),
            Some(WorkonResolvedWorktreeName { slug: "codex".to_owned(), named: false })
        );

        options.name = Some("Stable".to_owned());
        assert_eq!(
            workon_resolve_worktree_name(&options).expect("stable"),
            Some(WorkonResolvedWorktreeName { slug: "stable".to_owned(), named: true })
        );

        options.wt = Some(WorkonWorktreeRequest::Named("Issue 139".to_owned()));
        assert_eq!(
            workon_resolve_worktree_name(&options).expect("combined stable"),
            Some(WorkonResolvedWorktreeName { slug: "stable-issue-139".to_owned(), named: true })
        );
    }

    #[test]
    fn workon_plan_increments_past_worktrees_and_stale_agent_branches() {
        let root = std::path::PathBuf::from("/tmp/workon-plan");
        let repo = workon_test_repo(&root);
        let worktrees = vec![
            WorkonWorktree { name: "1-old".to_owned(), path: repo.repo_path.join("agents/1-old") },
            WorkonWorktree { name: "3-other".to_owned(), path: repo.repo_path.join("agents/3-other") },
        ];
        let branches = workon_branch_set(&["agents/4-stale", "main"]);
        let request = WorkonResolvedWorktreeName { slug: "feat".to_owned(), named: false };

        let plan = workon_plan_worktree(&repo, &request, false, WorkonLayout::Nested, &worktrees, &branches).expect("plan");

        assert_eq!(
            plan,
            WorkonWorktreePlan::Create {
                wt_name: "5-feat".to_owned(),
                wt_path: repo.repo_path.join("agents/5-feat"),
                branch: "agents/5-feat".to_owned(),
                branch_exists: false,
            }
        );
    }

    #[test]
    fn workon_plan_reuses_by_slug_unless_fresh() {
        let root = std::path::PathBuf::from("/tmp/workon-reuse");
        let repo = workon_test_repo(&root);
        let existing_path = repo.repo_path.join("agents/2-feat");
        let worktrees = vec![WorkonWorktree { name: "2-feat".to_owned(), path: existing_path.clone() }];
        let branches = workon_branch_set(&["agents/2-feat"]);
        let request = WorkonResolvedWorktreeName { slug: "feat".to_owned(), named: false };

        let reused = workon_plan_worktree(&repo, &request, false, WorkonLayout::Nested, &worktrees, &branches).expect("reuse");
        assert_eq!(reused, WorkonWorktreePlan::Reuse { path: existing_path });

        let fresh = workon_plan_worktree(&repo, &request, true, WorkonLayout::Nested, &worktrees, &branches).expect("fresh");
        assert_eq!(
            fresh,
            WorkonWorktreePlan::Create {
                wt_name: "3-feat".to_owned(),
                wt_path: repo.repo_path.join("agents/3-feat"),
                branch: "agents/3-feat".to_owned(),
                branch_exists: false,
            }
        );
    }

    #[test]
    fn workon_plan_named_stable_worktree_reuses_existing_branch() {
        let root = std::path::PathBuf::from("/tmp/workon-stable");
        let repo = workon_test_repo(&root);
        let request = WorkonResolvedWorktreeName { slug: "stable-issue".to_owned(), named: true };
        let branches = workon_branch_set(&["agents/stable-issue"]);

        let plan = workon_plan_worktree(&repo, &request, false, WorkonLayout::Nested, &[], &branches).expect("stable plan");

        assert_eq!(
            plan,
            WorkonWorktreePlan::Create {
                wt_name: "stable-issue".to_owned(),
                wt_path: repo.repo_path.join("agents/stable-issue"),
                branch: "agents/stable-issue".to_owned(),
                branch_exists: true,
            }
        );
    }

    #[test]
    fn workon_reuses_existing_window_before_spawn() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("demo", None);
        let mut runner = WorkonMockTmux { session: "50-mawjs\n".to_owned(), windows: "demo\n".to_owned(), ..Default::default() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::set_var("TMUX", "/tmp/tmux,1,0");

        let (stdout, attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("reuse");

        assert_eq!(stdout, "\x1b[33m⚡\x1b[0m reusing existing window 'demo' in 50-mawjs\n");
        assert!(attach.is_none());
        assert_eq!(runner.calls[2], ("select-window".to_owned(), workon_strings(&["-t", "50-mawjs:demo"])));
    }

    #[test]
    fn workon_outside_tmux_creates_session_and_requests_attach() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("demo", None);
        let mut runner = WorkonMockTmux {
            sessions: "team-demo\n188-other\n".to_owned(),
            ..Default::default()
        };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::remove_var("TMUX");

        let (stdout, attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("create session");

        assert_eq!(attach.as_deref(), Some("demo"));
        assert!(stdout.contains("workon 'demo' in new session demo"), "{stdout}");
        assert_eq!(runner.calls[0], ("has-session".to_owned(), workon_strings(&["-t", "=demo"])));
        assert_eq!(runner.calls[1], ("list-sessions".to_owned(), workon_strings(&["-F", "#{session_name}"])));
        assert_eq!(runner.calls[2].0, "new-session");
        assert_eq!(&runner.calls[2].1[..3], &workon_strings(&["-d", "-s", "demo"])[..]);
        assert_eq!(&runner.calls[2].1[5..], &workon_strings(&["-n", "demo"])[..]);
        assert_eq!(runner.calls[3].0, "display-message");
        assert_eq!(runner.calls[4].0, "send-keys");
        assert_eq!(runner.calls[5].0, "send-keys");
        assert_eq!(runner.calls[6].0, "capture-pane");
        assert_eq!(runner.calls[7].0, "capture-pane");
        assert_eq!(runner.calls.len(), 8);
    }

    #[test]
    fn workon_outside_tmux_exact_session_wins_before_numbered_fleet() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("demo", None);
        let mut runner = WorkonMockTmux {
            has_session: true,
            sessions: "188-demo\n".to_owned(),
            windows: "demo\n".to_owned(),
            ..Default::default()
        };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::remove_var("TMUX");

        let (stdout, attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("reuse session");

        assert_eq!(attach.as_deref(), Some("demo"));
        assert!(stdout.contains("reusing existing window 'demo' in demo"), "{stdout}");
        assert_eq!(runner.calls[0].0, "has-session");
        assert_eq!(runner.calls[1].0, "list-windows");
        assert_eq!(runner.calls[2], ("select-window".to_owned(), workon_strings(&["-t", "demo:demo"])));
        assert_eq!(runner.calls.len(), 3);
    }

    #[test]
    fn workon_outside_tmux_reuses_numbered_fleet_session() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/maw-rs"), repo_name: "maw-rs".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("maw-rs", None);
        let mut runner = WorkonMockTmux {
            sessions: "188-maw-rs\n".to_owned(),
            windows: "maw-rs\n".to_owned(),
            ..Default::default()
        };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::remove_var("TMUX");

        let (stdout, attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("reuse numbered fleet");

        assert_eq!(attach.as_deref(), Some("188-maw-rs"));
        assert!(stdout.contains("reusing existing window 'maw-rs' in 188-maw-rs"), "{stdout}");
        assert_eq!(runner.calls[0], ("has-session".to_owned(), workon_strings(&["-t", "=maw-rs"])));
        assert_eq!(runner.calls[1], ("list-sessions".to_owned(), workon_strings(&["-F", "#{session_name}"])));
        assert_eq!(runner.calls[2], ("list-windows".to_owned(), workon_strings(&["-t", "188-maw-rs", "-F", "#{window_name}"])));
        assert_eq!(runner.calls[3], ("select-window".to_owned(), workon_strings(&["-t", "188-maw-rs:maw-rs"])));
        assert_eq!(runner.calls.len(), 4);
    }

    #[test]
    fn workon_outside_tmux_ambiguous_numbered_fleet_sessions_error_without_create() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/maw-rs"), repo_name: "maw-rs".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("maw-rs", None);
        let mut runner = WorkonMockTmux {
            sessions: "188-maw-rs\n187-maw-rs\n".to_owned(),
            ..Default::default()
        };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::remove_var("TMUX");

        let err = workon_cmd_with_runner(&options, &repo, &mut runner).expect_err("ambiguous fleet");

        assert!(err.contains("matches multiple numbered fleet sessions"), "{err}");
        assert!(err.contains("187-maw-rs"), "{err}");
        assert!(err.contains("188-maw-rs"), "{err}");
        assert_eq!(runner.calls[0], ("has-session".to_owned(), workon_strings(&["-t", "=maw-rs"])));
        assert_eq!(runner.calls[1], ("list-sessions".to_owned(), workon_strings(&["-F", "#{session_name}"])));
        assert_eq!(runner.calls.len(), 2);
    }

    #[test]
    fn workon_path_arg_resolves_via_git_toplevel() {
        // shells out to real git — hold the env lock so PATH-mutating tests can't race us
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("maw-rs-workon-dot-{}", std::process::id()));
        let repo_dir = base.join("acme").join("demo");
        std::fs::create_dir_all(repo_dir.join("sub")).expect("mkdirs");
        assert!(
            std::process::Command::new("git").arg("-C").arg(&repo_dir).args(["init", "-q"]).status().expect("git init").success()
        );

        let resolved = workon_resolve_repo(repo_dir.join("sub").to_str().expect("utf8")).expect("resolve");

        assert_eq!(resolved.repo_name, "demo");
        assert_eq!(
            std::fs::canonicalize(&resolved.repo_path).expect("canonical repo"),
            std::fs::canonicalize(&repo_dir).expect("canonical dir")
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn workon_tmux_target_guard_blocks_bad_session() {
        let temp = std::env::temp_dir().join("maw-rs-workon-unit");
        let repo = WorkonRepo { repo_path: temp.join("acme/demo"), repo_name: "demo".to_owned(), parent_dir: temp.join("acme") };
        let options = workon_test_options("demo", None);
        let mut runner = WorkonMockTmux { session: "-Sbad\n".to_owned(), windows: String::new(), ..Default::default() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("TMUX");
        std::env::set_var("TMUX", "/tmp/tmux,1,0");

        let err = workon_cmd_with_runner(&options, &repo, &mut runner).expect_err("guard");

        assert!(err.contains("tmux target/session"));
        assert_eq!(runner.calls.len(), 1);
    }

    #[test]
    fn workon_build_command_resolves_weighted_only_commands_config() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = workon_temp_root("commands");
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        std::fs::write(
            root.join("config/maw.config.50.json"),
            r#"{"commands":{"omx":"CODEX_HOME=$PWD/.codex omx --direct","default":"claude --continue"}}"#,
        )
        .expect("config");

        assert!(!root.join("config/maw.config.json").exists());
        assert_eq!(workon_build_command_in_dir("omx", &root, None), "CODEX_HOME=$PWD/.codex omx --direct");
        assert_eq!(workon_build_command_in_dir("unknown", &root, None), "claude --continue");
        assert_eq!(workon_build_command_in_dir("unknown", &root, Some("omx")), "CODEX_HOME=$PWD/.codex omx --direct");
        assert_eq!(workon_build_command_in_dir("unknown", &root, Some("codex")), "codex");
        let _ = std::fs::remove_dir_all(root);
    }
}
