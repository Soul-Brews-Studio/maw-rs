const DISPATCH_89: &[DispatcherEntry] = &[DispatcherEntry {
    command: "reunion",
    handler: Handler::Sync(reunion_run_command),
}];

const REUNION_USAGE: &str = "usage: maw reunion [window] [--git-common-dir <path>]";
const REUNION_SYNC_DIRS: &[&str] = &[
    "memory/learnings",
    "memory/retrospectives",
    "memory/traces",
    "memory/collaborations",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReunionArgs {
    window: Option<String>,
    git_common_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReunionCwd {
    Found(std::path::PathBuf),
    Skipped(String),
}

fn reunion_run_command(argv: &[String]) -> CliOutput {
    match reunion_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn reunion_run(argv: &[String]) -> Result<String, String> {
    let options = reunion_parse_args(argv)?;
    let cwd = match reunion_resolve_cwd(options.window.as_deref())? {
        ReunionCwd::Found(cwd) => cwd,
        ReunionCwd::Skipped(message) => return Ok(format!("{message}\n")),
    };
    if !cwd.join("ψ").is_dir() {
        return Ok(format!("  \x1b[90m○\x1b[0m reunion: no ψ/ in {}, skipping\n", cwd.display()));
    }
    let Some(main_root) = reunion_resolve_main_root(&cwd, options.git_common_dir.as_deref()) else {
        return Ok("  \x1b[90m○\x1b[0m reunion: not a worktree (already main), skipping\n".to_owned());
    };
    let synced = reunion_sync_all(&cwd.join("ψ"), &main_root.join("ψ"));
    Ok(reunion_render_result(&main_root, &synced))
}

fn reunion_parse_args(argv: &[String]) -> Result<ReunionArgs, String> {
    if argv.first().is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h" | "help")) { return Err(REUNION_USAGE.to_owned()); }
    let mut parsed = ReunionArgs::default();
    let mut index = 0usize;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--" => return Err("reunion: -- separator is not supported".to_owned()),
            "--git-common-dir" => {
                index += 1;
                parsed.git_common_dir = Some(reunion_parse_common_dir(argv.get(index))?);
            }
            value if value.starts_with("--git-common-dir=") => {
                parsed.git_common_dir = Some(reunion_validate_common_dir(&value["--git-common-dir=".len()..])?);
            }
            value if value.starts_with('-') => return Err(format!("reunion: unknown argument {value}")),
            value => {
                if parsed.window.is_some() { return Err(format!("reunion: unexpected argument {value}")); }
                parsed.window = Some(reunion_validate_window(value)?);
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn reunion_parse_common_dir(value: Option<&String>) -> Result<std::path::PathBuf, String> {
    let Some(value) = value else { return Err("reunion: missing value for --git-common-dir".to_owned()); };
    if value.starts_with('-') { return Err("reunion: missing value for --git-common-dir".to_owned()); }
    reunion_validate_common_dir(value)
}

fn reunion_validate_common_dir(value: &str) -> Result<std::path::PathBuf, String> {
    if value.trim().is_empty() || value.trim() != value || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("reunion: invalid --git-common-dir {value:?}"));
    }
    Ok(std::path::PathBuf::from(value))
}

fn reunion_validate_window(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.contains("..") || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("reunion: invalid window {value:?}"));
    }
    Ok(value.to_owned())
}

fn reunion_resolve_cwd(window: Option<&str>) -> Result<ReunionCwd, String> {
    if let Some(window) = window { return reunion_resolve_window_cwd(window); }
    match reunion_tmux_display(&[]) {
        Ok(raw) => Ok(ReunionCwd::Found(std::path::PathBuf::from(raw.trim()))),
        Err(_) => Ok(ReunionCwd::Skipped("  \x1b[33m⚠\x1b[0m reunion: not in tmux, cannot determine cwd".to_owned())),
    }
}

fn reunion_resolve_window_cwd(window: &str) -> Result<ReunionCwd, String> {
    let mut tmux = TmuxClient::local();
    let wanted = window.to_ascii_lowercase();
    let target = tmux.list_all().into_iter().find_map(|session| {
        session.windows.into_iter().find(|item| item.name.to_ascii_lowercase() == wanted).map(|item| format!("{}:{}", session.name, item.name))
    });
    let Some(target) = target else {
        return Ok(ReunionCwd::Skipped(format!("  \x1b[33m⚠\x1b[0m reunion: window '{window}' not found, skipping")));
    };
    reunion_validate_tmux_target(&target)?;
    match reunion_tmux_display(&["-t", &target]) {
        Ok(raw) => Ok(ReunionCwd::Found(std::path::PathBuf::from(raw.trim()))),
        Err(_) => Ok(ReunionCwd::Skipped(format!("  \x1b[33m⚠\x1b[0m reunion: could not get cwd for {target}"))),
    }
}

fn reunion_tmux_display(extra: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("tmux");
    command.arg("display-message");
    for arg in extra { command.arg(arg); }
    command.arg("-p").arg("#{pane_current_path}");
    reunion_run_output(command, "tmux display-message")
}

fn reunion_resolve_main_root(cwd: &std::path::Path, override_common: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let common = if let Some(common) = override_common { common.to_path_buf() } else {
        match reunion_git_common_dir(cwd) {
            Ok(common) => common,
            Err(_) => return None,
        }
    };
    if common.as_os_str().is_empty() || common == std::path::Path::new(".git") { return None; }
    let main_git = if common.is_absolute() { common } else { cwd.join(common) };
    main_git.parent().map(std::path::Path::to_path_buf)
}

fn reunion_git_common_dir(cwd: &std::path::Path) -> Result<std::path::PathBuf, String> {
    reunion_validate_cwd(cwd)?;
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(cwd).arg("rev-parse").arg("--git-common-dir");
    reunion_run_output(command, "git rev-parse --git-common-dir").map(|raw| std::path::PathBuf::from(raw.trim()))
}

fn reunion_validate_cwd(cwd: &std::path::Path) -> Result<(), String> {
    if cwd.as_os_str().is_empty() { return Err("reunion: empty cwd".to_owned()); }
    Ok(())
}

fn reunion_run_output(mut command: std::process::Command, label: &str) -> Result<String, String> {
    let output = command.output().map_err(|error| format!("reunion: {label}: {error}"))?;
    if !output.status.success() { return Err(format!("reunion: {label} failed")); }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn reunion_sync_all(wt_vault: &std::path::Path, main_vault: &std::path::Path) -> Vec<(String, usize)> {
    REUNION_SYNC_DIRS.iter().filter_map(|dir| {
        let count = reunion_sync_dir(&wt_vault.join(dir), &main_vault.join(dir));
        (count > 0).then(|| ((*dir).to_owned(), count))
    }).collect()
}

fn reunion_sync_dir(src_dir: &std::path::Path, dst_dir: &std::path::Path) -> usize {
    if !src_dir.is_dir() { return 0; }
    let mut count = 0usize;
    reunion_walk_copy(src_dir, dst_dir, &mut count);
    count
}

fn reunion_walk_copy(src: &std::path::Path, dst: &std::path::Path, count: &mut usize) {
    let Ok(entries) = std::fs::read_dir(src) else { return; };
    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() { reunion_walk_copy(&src_path, &dst_path, count); }
        else if !dst_path.exists() && reunion_copy_file(&src_path, &dst_path) { *count += 1; }
    }
}

fn reunion_copy_file(src: &std::path::Path, dst: &std::path::Path) -> bool {
    let Some(parent) = dst.parent() else { return false; };
    std::fs::create_dir_all(parent).and_then(|()| std::fs::copy(src, dst).map(|_| ())).is_ok()
}

fn reunion_render_result(main_root: &std::path::Path, synced: &[(String, usize)]) -> String {
    let repo = main_root.file_name().and_then(|name| name.to_str()).unwrap_or("main");
    if synced.is_empty() { return format!("  \x1b[90m○\x1b[0m reunion: nothing new to sync to main ({repo})\n"); }
    let parts = synced.iter().map(|(dir, count)| format!("{count} {}", reunion_label(dir))).collect::<Vec<_>>();
    format!("  \x1b[32m✓\x1b[0m reunion: synced {} → {repo}/ψ/\n", parts.join(", "))
}

fn reunion_label(dir: &str) -> &str { dir.rsplit('/').next().unwrap_or(dir) }

fn reunion_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("reunion: invalid tmux target {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod reunion_tests {
    use super::*;

    fn reunion_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn reunion_parse_accepts_window_and_common_dir() {
        let args = reunion_parse_args(&reunion_strings(&["Work", "--git-common-dir", "/tmp/main/.git"])).unwrap();
        assert_eq!(args.window.as_deref(), Some("Work"));
        assert_eq!(args.git_common_dir.as_deref(), Some(std::path::Path::new("/tmp/main/.git")));
    }

    #[test]
    fn reunion_parse_rejects_option_injection() {
        assert!(reunion_parse_args(&reunion_strings(&["--", "main"])).unwrap_err().contains("separator"));
        assert!(reunion_parse_args(&reunion_strings(&["-main"])).unwrap_err().contains("unknown"));
        assert!(reunion_parse_args(&reunion_strings(&["../main"])).unwrap_err().contains("invalid window"));
        assert!(reunion_parse_args(&reunion_strings(&["--git-common-dir", "--bad"])).unwrap_err().contains("missing value"));
    }
}
