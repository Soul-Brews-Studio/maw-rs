const DISPATCH_145: &[DispatcherEntry] = &[DispatcherEntry {
    command: "worktree",
    handler: Handler::Sync(run_worktree_command),
}];

const WORKTREE_USAGE: &str =
    "usage: maw worktree <ls|clean> [--dry-run] or maw worktree add <name> [--base <ref>]";
const WORKTREE_MERGE_TARGET: &str = "origin/alpha";
const WORKTREE_TMUX_PANE_FORMAT: &str = "#{session_name}:#{window_name}|||#{pane_current_path}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorktreeCommand {
    Ls,
    Clean,
    Add,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorktreeOptions<'a> {
    command: WorktreeCommand,
    name: Option<&'a str>,
    base: &'a str,
    dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeRecord {
    path: std::path::PathBuf,
    branch: Option<String>,
    main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct WorktreeStatus {
    path: std::path::PathBuf,
    branch: Option<String>,
    main: bool,
    merged: bool,
    dirty: bool,
    live: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorktreeLiveKind {
    Fleet,
    TmuxPane,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeLiveRef {
    path: std::path::PathBuf,
    label: String,
    kind: WorktreeLiveKind,
}

trait WorktreeRuntime {
    fn worktree_cwd(&self) -> std::path::PathBuf;
    fn worktree_git(
        &mut self,
        cwd: &std::path::Path,
        args: &[&str],
    ) -> Result<String, String>;
    fn worktree_tmux(&mut self, subcommand: &str, args: &[String]) -> Result<String, String>;
    fn worktree_fleet_entries(&mut self) -> Result<Vec<NativeFleetEntry>, String>;
    fn worktree_path_exists(&self, path: &std::path::Path) -> bool;
    fn worktree_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String>;
}

struct WorktreeSystemRuntime;

impl WorktreeRuntime for WorktreeSystemRuntime {
    fn worktree_cwd(&self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    fn worktree_git(
        &mut self,
        cwd: &std::path::Path,
        args: &[&str],
    ) -> Result<String, String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .map_err(|error| format!("worktree: failed to run git: {error}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            Err("worktree: git failed".to_owned())
        } else {
            Err(format!("worktree: git failed: {stderr}"))
        }
    }

    fn worktree_tmux(&mut self, subcommand: &str, args: &[String]) -> Result<String, String> {
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(&mut runner, subcommand, args).map_err(|error| error.message)
    }

    fn worktree_fleet_entries(&mut self) -> Result<Vec<NativeFleetEntry>, String> {
        fleet_load_entries_result("worktree")
    }

    fn worktree_path_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn worktree_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(path)
            .map_err(|error| format!("worktree add: create {}: {error}", path.display()))
    }
}

fn run_worktree_command(argv: &[String]) -> CliOutput {
    match worktree_run_with(argv, &mut WorktreeSystemRuntime) {
        Ok(output) => output,
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn worktree_run_with(
    argv: &[String],
    runtime: &mut impl WorktreeRuntime,
) -> Result<CliOutput, (i32, String)> {
    if matches!(argv.first().map(String::as_str), Some("--help" | "-h")) {
        return Ok(CliOutput {
            code: 0,
            stdout: format!("{WORKTREE_USAGE}\n"),
            stderr: String::new(),
        });
    }
    let options = worktree_parse_args(argv)?;
    let stdout = match options.command {
        WorktreeCommand::Ls => {
            let statuses = worktree_collect_statuses(runtime).map_err(|message| (1, message))?;
            worktree_render_ls(&statuses)
        }
        WorktreeCommand::Clean => {
            let statuses = worktree_collect_statuses(runtime).map_err(|message| (1, message))?;
            worktree_run_clean(runtime, &statuses, options.dry_run).map_err(|message| (1, message))?
        }
        WorktreeCommand::Add => {
            let records = worktree_list_records(runtime).map_err(|message| (1, message))?;
            worktree_run_add(runtime, &records, &options).map_err(|message| (1, message))?
        }
    };
    Ok(CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    })
}

fn worktree_parse_args(argv: &[String]) -> Result<WorktreeOptions<'_>, (i32, String)> {
    let Some(command) = argv.first().map(String::as_str) else {
        return Err((2, WORKTREE_USAGE.to_owned()));
    };
    let command = match command {
        "ls" | "list" => WorktreeCommand::Ls,
        "clean" => WorktreeCommand::Clean,
        "add" => WorktreeCommand::Add,
        value => return Err((2, format!("worktree: unknown subcommand {value}\n{WORKTREE_USAGE}"))),
    };
    let mut dry_run = false;
    let mut name = None;
    let mut base = WORKTREE_MERGE_TARGET;
    let mut index = 1;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--dry-run" if matches!(command, WorktreeCommand::Clean) => dry_run = true,
            "--dry-run" => {
                return Err((2, format!("worktree add: --dry-run is only valid with clean\n{WORKTREE_USAGE}")));
            }
            "--base" if matches!(command, WorktreeCommand::Add) => {
                index += 1;
                let Some(value) = argv.get(index).map(String::as_str) else {
                    return Err((2, format!("worktree add: --base requires a value\n{WORKTREE_USAGE}")));
                };
                worktree_validate_ref(value).map_err(|message| (2, message))?;
                base = value;
            }
            "--base" => {
                return Err((2, format!("worktree: --base is only valid with add\n{WORKTREE_USAGE}")));
            }
            "--help" | "-h" => return Err((2, WORKTREE_USAGE.to_owned())),
            value if value.starts_with('-') => {
                return Err((2, format!("worktree: unknown argument {value}\n{WORKTREE_USAGE}")));
            }
            value if matches!(command, WorktreeCommand::Add) && name.is_none() => {
                worktree_validate_name(value).map_err(|message| (2, message))?;
                name = Some(value);
            }
            value if matches!(command, WorktreeCommand::Add) => {
                return Err((2, format!("worktree add: unexpected argument {value}\n{WORKTREE_USAGE}")));
            }
            value => return Err((2, format!("worktree: unexpected argument {value}\n{WORKTREE_USAGE}"))),
        }
        index += 1;
    }
    if dry_run && !matches!(command, WorktreeCommand::Clean) {
        return Err((2, format!("worktree ls: --dry-run is only valid with clean\n{WORKTREE_USAGE}")));
    }
    if matches!(command, WorktreeCommand::Add) && name.is_none() {
        return Err((2, format!("worktree add: missing name\n{WORKTREE_USAGE}")));
    }
    Ok(WorktreeOptions {
        command,
        name,
        base,
        dry_run,
    })
}

fn worktree_validate_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("worktree add: name must contain only ASCII letters, digits, '.', '_' or '-'".to_owned());
    }
    Ok(())
}

fn worktree_validate_ref(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err("worktree add: --base must be a non-option git ref without whitespace".to_owned());
    }
    Ok(())
}

fn worktree_collect_statuses(
    runtime: &mut impl WorktreeRuntime,
) -> Result<Vec<WorktreeStatus>, String> {
    let records = worktree_list_records(runtime)?;
    let Some(main_path) = records.first().map(|record| record.path.clone()) else {
        return Ok(Vec::new());
    };
    let live_refs = worktree_live_refs(runtime, &main_path)?;
    Ok(records
        .into_iter()
        .map(|record| WorktreeStatus {
            merged: worktree_branch_merged(runtime, &main_path, record.branch.as_deref()),
            dirty: worktree_is_dirty(runtime, &record.path),
            live: worktree_find_live(&record.path, &live_refs),
            path: record.path,
            branch: record.branch,
            main: record.main,
        })
        .collect())
}

fn worktree_list_records(runtime: &mut impl WorktreeRuntime) -> Result<Vec<WorktreeRecord>, String> {
    let cwd = runtime.worktree_cwd();
    let raw = runtime.worktree_git(&cwd, &["worktree", "list", "--porcelain"])?;
    Ok(worktree_parse_list(&raw))
}

fn worktree_parse_list(raw: &str) -> Vec<WorktreeRecord> {
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;
    for line in raw.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(WorktreeRecord {
                path: std::path::PathBuf::from(path),
                branch: None,
                main: false,
            });
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(record) = &mut current {
                record.branch = worktree_branch_name(branch);
            }
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    if let Some(record) = records.first_mut() {
        record.main = true;
    }
    records
}

fn worktree_branch_name(value: &str) -> Option<String> {
    let branch = value.strip_prefix("refs/heads/").unwrap_or(value).trim();
    (!branch.is_empty()).then(|| branch.to_owned())
}

fn worktree_branch_merged(
    runtime: &mut impl WorktreeRuntime,
    main_path: &std::path::Path,
    branch: Option<&str>,
) -> bool {
    branch.is_some_and(|branch| {
        runtime
            .worktree_git(main_path, &["merge-base", "--is-ancestor", branch, WORKTREE_MERGE_TARGET])
            .is_ok()
    })
}

fn worktree_is_dirty(runtime: &mut impl WorktreeRuntime, path: &std::path::Path) -> bool {
    runtime
        .worktree_git(path, &["status", "--porcelain"])
        .map_or(true, |raw| !raw.trim().is_empty())
}

fn worktree_live_refs(
    runtime: &mut impl WorktreeRuntime,
    main_path: &std::path::Path,
) -> Result<Vec<WorktreeLiveRef>, String> {
    let mut refs = Vec::new();
    worktree_add_fleet_live_refs(runtime, main_path, &mut refs)?;
    worktree_add_tmux_live_refs(runtime, &mut refs)?;
    Ok(refs)
}

fn worktree_add_fleet_live_refs(
    runtime: &mut impl WorktreeRuntime,
    main_path: &std::path::Path,
    refs: &mut Vec<WorktreeLiveRef>,
) -> Result<(), String> {
    let roots = worktree_github_roots(main_path);
    for entry in runtime.worktree_fleet_entries()? {
        for window in entry.session.windows {
            for path in worktree_fleet_repo_candidates(&window.repo, &roots) {
                refs.push(WorktreeLiveRef {
                    path,
                    label: format!("{}:{}", entry.session.name, window.name),
                    kind: WorktreeLiveKind::Fleet,
                });
            }
        }
    }
    Ok(())
}

fn worktree_add_tmux_live_refs(
    runtime: &mut impl WorktreeRuntime,
    refs: &mut Vec<WorktreeLiveRef>,
) -> Result<(), String> {
    let args = [
        "-a".to_owned(),
        "-F".to_owned(),
        WORKTREE_TMUX_PANE_FORMAT.to_owned(),
    ];
    let raw = match runtime.worktree_tmux("list-panes", &args) {
        Ok(raw) => raw,
        Err(error)
            if error.contains("no server running")
                || error.contains("failed to connect to server") =>
        {
            String::new()
        }
        Err(error) => return Err(format!("worktree: cannot list tmux panes: {error}")),
    };
    refs.extend(raw.lines().filter_map(worktree_parse_tmux_live_ref));
    Ok(())
}

fn worktree_parse_tmux_live_ref(line: &str) -> Option<WorktreeLiveRef> {
    let (label, path) = line.split_once("|||")?;
    if label.trim().is_empty() || path.trim().is_empty() {
        return None;
    }
    Some(WorktreeLiveRef {
        path: std::path::PathBuf::from(path.trim()),
        label: label.trim().to_owned(),
        kind: WorktreeLiveKind::TmuxPane,
    })
}

fn worktree_find_live(path: &std::path::Path, refs: &[WorktreeLiveRef]) -> Option<String> {
    refs.iter()
        .find(|item| match item.kind {
            WorktreeLiveKind::Fleet => item.path == path,
            WorktreeLiveKind::TmuxPane => item.path.starts_with(path),
        })
        .map(|item| item.label.clone())
}

fn worktree_github_roots(main_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = worktree_nearest_github_root(main_path) {
        roots.push(root);
    }
    if let Some(root) = std::env::var_os("GHQ_ROOT").map(std::path::PathBuf::from) {
        roots.push(worktree_normalize_github_root(root));
    }
    roots.push(ghq_root().join("github.com"));
    worktree_dedup_paths(roots)
}

fn worktree_nearest_github_root(path: &std::path::Path) -> Option<std::path::PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "github.com"))
        .map(std::path::Path::to_path_buf)
}

fn worktree_normalize_github_root(path: std::path::PathBuf) -> std::path::PathBuf {
    if path.file_name().is_some_and(|name| name == "github.com") {
        path
    } else {
        path.join("github.com")
    }
}

fn worktree_dedup_paths(paths: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

fn worktree_fleet_repo_candidates(
    repo: &str,
    roots: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let repo = repo.trim();
    if repo.is_empty() {
        return Vec::new();
    }
    let path = std::path::PathBuf::from(repo);
    if path.is_absolute() {
        return vec![path];
    }
    let slug = repo.strip_prefix("github.com/").unwrap_or(repo);
    roots.iter().map(|root| root.join(slug)).collect()
}

fn worktree_run_add(
    runtime: &mut impl WorktreeRuntime,
    records: &[WorktreeRecord],
    options: &WorktreeOptions<'_>,
) -> Result<String, String> {
    let name = options
        .name
        .ok_or_else(|| "worktree add: missing name".to_owned())?;
    let main_path = records
        .first()
        .map(|record| record.path.clone())
        .ok_or_else(|| "worktree add: no git worktrees found".to_owned())?;
    let path = main_path.join("agents").join(name);
    let branch = format!("agents/{name}");
    let branch_ref = format!("refs/heads/{branch}");

    if runtime.worktree_path_exists(&path) {
        return Err(format!("worktree add: path already exists: {}", path.display()));
    }
    if worktree_branch_exists(runtime, &main_path, &branch_ref) {
        return Err(format!("worktree add: branch already exists: {branch}"));
    }
    if let Some(parent) = path.parent() {
        runtime.worktree_create_dir_all(parent)?;
    }
    let path_text = path.to_string_lossy().into_owned();
    runtime.worktree_git(
        &main_path,
        &["worktree", "add", "-b", &branch, &path_text, options.base],
    )?;
    Ok(format!(
        "created {} (branch {} from {})\n",
        path.display(),
        branch,
        options.base
    ))
}

fn worktree_branch_exists(
    runtime: &mut impl WorktreeRuntime,
    main_path: &std::path::Path,
    branch_ref: &str,
) -> bool {
    runtime
        .worktree_git(main_path, &["show-ref", "--verify", "--quiet", branch_ref])
        .is_ok()
}

fn worktree_render_ls(statuses: &[WorktreeStatus]) -> String {
    let mut stdout = "path\tbranch\tmerged?\tdirty?\tlive?\n".to_owned();
    for status in statuses {
        let live = status
            .live
            .as_ref()
            .map_or_else(|| "no".to_owned(), |label| format!("yes:{label}"));
        let _ = writeln!(
            stdout,
            "{}\t{}\t{}\t{}\t{}",
            status.path.display(),
            status.branch.as_deref().unwrap_or("-"),
            worktree_yes_no(status.merged),
            worktree_yes_no(status.dirty),
            live
        );
    }
    stdout
}

fn worktree_yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn worktree_run_clean(
    runtime: &mut impl WorktreeRuntime,
    statuses: &[WorktreeStatus],
    dry_run: bool,
) -> Result<String, String> {
    let Some(main_path) = statuses.first().map(|status| status.path.clone()) else {
        return Ok(String::new());
    };
    let mut stdout = String::new();
    for status in statuses {
        if let Some(reason) = worktree_skip_reason(status) {
            let _ = writeln!(stdout, "skip {} ({reason})", status.path.display());
            continue;
        }
        if dry_run {
            let _ = writeln!(stdout, "would remove {}", status.path.display());
        } else {
            worktree_remove(runtime, &main_path, &status.path)?;
            let _ = writeln!(stdout, "removed {}", status.path.display());
        }
    }
    if !dry_run {
        runtime.worktree_git(&main_path, &["worktree", "prune"])?;
        stdout.push_str("pruned worktrees\n");
    }
    Ok(stdout)
}

fn worktree_skip_reason(status: &WorktreeStatus) -> Option<String> {
    if status.main {
        Some("main".to_owned())
    } else if status.dirty {
        Some("dirty".to_owned())
    } else if let Some(live) = &status.live {
        Some(format!("live: {live}"))
    } else if !status.merged {
        Some("unmerged".to_owned())
    } else {
        None
    }
}

fn worktree_remove(
    runtime: &mut impl WorktreeRuntime,
    main_path: &std::path::Path,
    path: &std::path::Path,
) -> Result<(), String> {
    let path = path.to_string_lossy().into_owned();
    runtime
        .worktree_git(main_path, &["worktree", "remove", &path])
        .map(|_| ())
}

#[cfg(test)]
mod worktree_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Debug)]
    struct FakeWorktreeRuntime {
        cwd: std::path::PathBuf,
        worktrees: String,
        dirty_paths: BTreeSet<std::path::PathBuf>,
        merged_branches: BTreeSet<String>,
        existing_branch_refs: BTreeSet<String>,
        existing_paths: BTreeSet<std::path::PathBuf>,
        fleet_entries: Vec<NativeFleetEntry>,
        tmux_panes: String,
        git_calls: Vec<(std::path::PathBuf, Vec<String>)>,
        tmux_calls: Vec<(String, Vec<String>)>,
        created_dirs: Vec<std::path::PathBuf>,
    }

    impl Default for FakeWorktreeRuntime {
        fn default() -> Self {
            Self {
                cwd: std::path::PathBuf::from("/repo"),
                worktrees: String::new(),
                dirty_paths: BTreeSet::new(),
                merged_branches: BTreeSet::new(),
                existing_branch_refs: BTreeSet::new(),
                existing_paths: BTreeSet::new(),
                fleet_entries: Vec::new(),
                tmux_panes: String::new(),
                git_calls: Vec::new(),
                tmux_calls: Vec::new(),
                created_dirs: Vec::new(),
            }
        }
    }

    impl WorktreeRuntime for FakeWorktreeRuntime {
        fn worktree_cwd(&self) -> std::path::PathBuf {
            self.cwd.clone()
        }

        fn worktree_git(
            &mut self,
            cwd: &std::path::Path,
            args: &[&str],
        ) -> Result<String, String> {
            self.git_calls.push((
                cwd.to_path_buf(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
            match args {
                ["worktree", "list", "--porcelain"] => Ok(self.worktrees.clone()),
                ["status", "--porcelain"] => {
                    if self.dirty_paths.contains(cwd) {
                        Ok(" M src/lib.rs\n".to_owned())
                    } else {
                        Ok(String::new())
                    }
                }
                ["merge-base", "--is-ancestor", branch, "origin/alpha"] => {
                    if self.merged_branches.contains(*branch) {
                        Ok(String::new())
                    } else {
                        Err("not ancestor".to_owned())
                    }
                }
                ["show-ref", "--verify", "--quiet", branch_ref] => {
                    if self.existing_branch_refs.contains(*branch_ref) {
                        Ok(String::new())
                    } else {
                        Err("missing ref".to_owned())
                    }
                }
                ["worktree", "remove", _]
                | ["worktree", "prune"]
                | ["worktree", "add", "-b", _, _, _] => Ok(String::new()),
                other => Err(format!("unexpected git args: {other:?}")),
            }
        }

        fn worktree_tmux(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, String> {
            self.tmux_calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-panes" => Ok(self.tmux_panes.clone()),
                _ => Err(format!("unexpected tmux subcommand: {subcommand}")),
            }
        }

        fn worktree_fleet_entries(&mut self) -> Result<Vec<NativeFleetEntry>, String> {
            Ok(self.fleet_entries.clone())
        }

        fn worktree_path_exists(&self, path: &std::path::Path) -> bool {
            self.existing_paths.contains(path)
        }

        fn worktree_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String> {
            self.created_dirs.push(path.to_path_buf());
            Ok(())
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn worktrees(entries: &[(&str, &str)]) -> String {
        let mut raw = String::new();
        for (path, branch) in entries {
            let _ = writeln!(
                raw,
                "worktree {path}\nHEAD 0000000000000000000000000000000000000000\nbranch refs/heads/{branch}\n"
            );
        }
        raw
    }

    fn runtime_with_two_worktrees(agent: &str, branch: &str) -> FakeWorktreeRuntime {
        FakeWorktreeRuntime {
            cwd: std::path::PathBuf::from("/repo/agents/current"),
            worktrees: worktrees(&[("/repo", "main"), (agent, branch)]),
            merged_branches: BTreeSet::from(["main".to_owned(), branch.to_owned()]),
            ..FakeWorktreeRuntime::default()
        }
    }

    fn fleet_entry(session: &str, window: &str, repo: &str) -> NativeFleetEntry {
        NativeFleetEntry {
            file: format!("{session}.json"),
            path: std::path::PathBuf::from(format!("/fleet/{session}.json")),
            session: NativeFleetSession {
                name: session.to_owned(),
                windows: vec![NativeFleetWindow {
                    name: window.to_owned(),
                    repo: repo.to_owned(),
                }],
                ..NativeFleetSession::default()
            },
        }
    }

    fn command_calls(runtime: &FakeWorktreeRuntime, command: &str) -> Vec<Vec<String>> {
        runtime
            .git_calls
            .iter()
            .filter_map(|(_, args)| (args.first().map(String::as_str) == Some(command)).then_some(args.clone()))
            .collect()
    }

    #[test]
    fn add_creates_agents_path_and_branch_from_origin_alpha_by_default() {
        let mut runtime = FakeWorktreeRuntime {
            cwd: std::path::PathBuf::from("/repo/agents/current"),
            worktrees: worktrees(&[("/repo", "main")]),
            ..FakeWorktreeRuntime::default()
        };

        let output =
            worktree_run_with(&strings(&["add", "codex-5"]), &mut runtime).expect("add output");

        assert_eq!(output.code, 0);
        assert_eq!(
            output.stdout,
            "created /repo/agents/codex-5 (branch agents/codex-5 from origin/alpha)\n"
        );
        assert_eq!(runtime.created_dirs, vec![std::path::PathBuf::from("/repo/agents")]);
        assert_eq!(
            runtime.git_calls,
            vec![
                (
                    std::path::PathBuf::from("/repo/agents/current"),
                    strings(&["worktree", "list", "--porcelain"])
                ),
                (
                    std::path::PathBuf::from("/repo"),
                    strings(&[
                        "show-ref",
                        "--verify",
                        "--quiet",
                        "refs/heads/agents/codex-5"
                    ])
                ),
                (
                    std::path::PathBuf::from("/repo"),
                    strings(&[
                        "worktree",
                        "add",
                        "-b",
                        "agents/codex-5",
                        "/repo/agents/codex-5",
                        "origin/alpha"
                    ])
                ),
            ]
        );
        assert!(runtime.tmux_calls.is_empty(), "add must not inspect tmux");
    }

    #[test]
    fn add_accepts_explicit_base_ref() {
        let mut runtime = FakeWorktreeRuntime {
            worktrees: worktrees(&[("/repo", "main")]),
            ..FakeWorktreeRuntime::default()
        };

        let output = worktree_run_with(
            &strings(&["add", "review", "--base", "origin/beta"]),
            &mut runtime,
        )
        .expect("add output");

        assert!(output
            .stdout
            .contains("created /repo/agents/review (branch agents/review from origin/beta)"));
        assert_eq!(
            runtime.git_calls.last().map(|(_, args)| args.clone()),
            Some(strings(&[
                "worktree",
                "add",
                "-b",
                "agents/review",
                "/repo/agents/review",
                "origin/beta"
            ]))
        );
    }

    #[test]
    fn add_refuses_existing_path_before_branch_probe_or_create() {
        let mut runtime = FakeWorktreeRuntime {
            worktrees: worktrees(&[("/repo", "main")]),
            existing_paths: BTreeSet::from([std::path::PathBuf::from("/repo/agents/old")]),
            ..FakeWorktreeRuntime::default()
        };

        let error =
            worktree_run_with(&strings(&["add", "old"]), &mut runtime).expect_err("path exists");

        assert_eq!(
            error,
            (
                1,
                "worktree add: path already exists: /repo/agents/old".to_owned()
            )
        );
        assert!(runtime.created_dirs.is_empty());
        assert_eq!(
            runtime.git_calls,
            vec![(
                std::path::PathBuf::from("/repo"),
                strings(&["worktree", "list", "--porcelain"])
            )]
        );
    }

    #[test]
    fn add_refuses_existing_branch_before_create_or_worktree_add() {
        let mut runtime = FakeWorktreeRuntime {
            worktrees: worktrees(&[("/repo", "main")]),
            existing_branch_refs: BTreeSet::from(["refs/heads/agents/old".to_owned()]),
            ..FakeWorktreeRuntime::default()
        };

        let error =
            worktree_run_with(&strings(&["add", "old"]), &mut runtime).expect_err("branch exists");

        assert_eq!(
            error,
            (
                1,
                "worktree add: branch already exists: agents/old".to_owned()
            )
        );
        assert!(runtime.created_dirs.is_empty());
        assert!(command_calls(&runtime, "worktree")
            .iter()
            .all(|args| args != &strings(&["worktree", "add"])));
    }

    #[test]
    fn add_rejects_unsafe_name_and_base_before_runtime() {
        let mut runtime = FakeWorktreeRuntime::default();

        let bad_name =
            worktree_run_with(&strings(&["add", "../bad"]), &mut runtime).expect_err("bad name");
        assert_eq!(bad_name.0, 2);
        assert!(bad_name.1.contains("name must contain only ASCII"));

        let bad_base = worktree_run_with(
            &strings(&["add", "safe", "--base", "--upload-pack=sh"]),
            &mut runtime,
        )
        .expect_err("bad base");
        assert_eq!(bad_base.0, 2);
        assert!(bad_base.1.contains("--base must be a non-option git ref"));
        assert!(runtime.git_calls.is_empty());
    }

    #[test]
    fn clean_removes_merged_clean_dead_worktree_and_prunes_afterward() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/old", "agents/old");

        let output =
            worktree_run_with(&strings(&["clean"]), &mut runtime).expect("clean output");

        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("skip /repo (main)"), "{}", output.stdout);
        assert!(output.stdout.contains("removed /repo/agents/old"), "{}", output.stdout);
        let remove_index = runtime
            .git_calls
            .iter()
            .position(|(_, args)| args == &strings(&["worktree", "remove", "/repo/agents/old"]))
            .expect("remove call");
        let prune_index = runtime
            .git_calls
            .iter()
            .position(|(_, args)| args == &strings(&["worktree", "prune"]))
            .expect("prune call");
        assert!(remove_index < prune_index, "prune must follow remove");
        assert!(
            runtime
                .git_calls
                .iter()
                .all(|(_, args)| !args.iter().any(|arg| arg == "--force")),
            "worktree clean must never use --force: {:?}",
            runtime.git_calls
        );
        assert!(
            command_calls(&runtime, "branch").is_empty(),
            "worktree clean must not delete branches"
        );
    }

    #[test]
    fn clean_skips_dirty_worktree_with_reason() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/dirty", "agents/dirty");
        runtime.dirty_paths.insert(std::path::PathBuf::from("/repo/agents/dirty"));

        let output =
            worktree_run_with(&strings(&["clean"]), &mut runtime).expect("clean output");

        assert!(output.stdout.contains("skip /repo/agents/dirty (dirty)"));
        assert!(
            command_calls(&runtime, "worktree")
                .iter()
                .all(|args| args != &strings(&["worktree", "remove", "/repo/agents/dirty"]))
        );
    }

    #[test]
    fn clean_skips_unmerged_worktree_with_reason() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/fresh", "agents/fresh");
        runtime.merged_branches.remove("agents/fresh");

        let output =
            worktree_run_with(&strings(&["clean"]), &mut runtime).expect("clean output");

        assert!(output.stdout.contains("skip /repo/agents/fresh (unmerged)"));
        assert!(
            command_calls(&runtime, "worktree")
                .iter()
                .all(|args| args != &strings(&["worktree", "remove", "/repo/agents/fresh"]))
        );
    }

    #[test]
    fn clean_skips_fleet_live_window_with_reason() {
        let mut runtime = runtime_with_two_worktrees(
            "/opt/Code/github.com/acme/widgets/agents/live",
            "agents/live",
        );
        runtime.cwd = std::path::PathBuf::from("/opt/Code/github.com/acme/widgets/agents/live");
        runtime.worktrees = worktrees(&[
            ("/opt/Code/github.com/acme/widgets", "main"),
            ("/opt/Code/github.com/acme/widgets/agents/live", "agents/live"),
        ]);
        runtime.fleet_entries = vec![fleet_entry(
            "188-maw-rs",
            "maw-rs-codex-4",
            "github.com/acme/widgets/agents/live",
        )];

        let output =
            worktree_run_with(&strings(&["clean"]), &mut runtime).expect("clean output");

        assert!(
            output
                .stdout
                .contains("skip /opt/Code/github.com/acme/widgets/agents/live (live: 188-maw-rs:maw-rs-codex-4)"),
            "{}",
            output.stdout
        );
        assert!(command_calls(&runtime, "worktree").iter().all(|args| {
            args != &strings(&[
                "worktree",
                "remove",
                "/opt/Code/github.com/acme/widgets/agents/live",
            ])
        }));
    }

    #[test]
    fn clean_skips_tmux_live_pane_cwd_prefix_with_reason() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/live", "agents/live");
        runtime.tmux_panes = "188-maw-rs:maw-rs-codex-4|||/repo/agents/live/crates/maw-cli\n"
            .to_owned();

        let output =
            worktree_run_with(&strings(&["clean"]), &mut runtime).expect("clean output");

        assert!(
            output
                .stdout
                .contains("skip /repo/agents/live (live: 188-maw-rs:maw-rs-codex-4)")
        );
    }

    #[test]
    fn dry_run_removes_nothing_and_does_not_prune() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/old", "agents/old");

        let output = worktree_run_with(&strings(&["clean", "--dry-run"]), &mut runtime)
            .expect("dry run");

        assert!(output.stdout.contains("would remove /repo/agents/old"));
        assert!(!output.stdout.contains("removed /repo/agents/old"));
        assert!(runtime.git_calls.iter().all(|(_, args)| {
            args != &strings(&["worktree", "remove", "/repo/agents/old"])
                && args != &strings(&["worktree", "prune"])
        }));
    }

    #[test]
    fn dry_run_never_lists_main_worktree_as_removable() {
        let mut runtime = FakeWorktreeRuntime {
            worktrees: worktrees(&[("/repo", "main")]),
            merged_branches: BTreeSet::from(["main".to_owned()]),
            ..FakeWorktreeRuntime::default()
        };

        let output = worktree_run_with(&strings(&["clean", "--dry-run"]), &mut runtime)
            .expect("dry run");

        assert!(output.stdout.contains("skip /repo (main)"));
        assert!(!output.stdout.contains("would remove /repo"));
    }

    #[test]
    fn ls_renders_status_table_with_live_flags() {
        let mut runtime = runtime_with_two_worktrees("/repo/agents/live", "agents/live");
        runtime.tmux_panes = "188-maw-rs:maw-rs-codex-4|||/repo/agents/live\n".to_owned();

        let output = worktree_run_with(&strings(&["ls"]), &mut runtime).expect("ls output");

        assert_eq!(output.code, 0);
        assert!(output.stdout.starts_with("path\tbranch\tmerged?\tdirty?\tlive?\n"));
        assert!(!output.stdout.contains("/repo\tsmain"));
        assert!(output
            .stdout
            .contains("/repo/agents/live\tagents/live\tyes\tno\tyes:188-maw-rs:maw-rs-codex-4"));
    }

    #[test]
    fn dispatch_fragment_registers_worktree() {
        assert_eq!(DISPATCH_145[0].command, "worktree");
    }
}
