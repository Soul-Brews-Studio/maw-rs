const DISPATCH_94: &[DispatcherEntry] = &[
    DispatcherEntry { command: "soul-sync", handler: Handler::Sync(soulsync_command) },
    DispatcherEntry { command: "soulsync", handler: Handler::Sync(soulsync_command) },
    DispatcherEntry { command: "ss", handler: Handler::Sync(soulsync_command) },
];

const SOULSYNC_USAGE: &str = "usage: maw soul-sync [peer] [--from <peer>] [--project]";
const SOULSYNC_DIRS: &[&str] = &["memory/learnings", "memory/retrospectives", "memory/traces", "memory/collaborations"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum SoulsyncMode {
    Oracle,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SoulsyncArgs {
    target: Option<String>,
    from: Option<String>,
    mode: SoulsyncMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SoulsyncSyncResult {
    from: String,
    to: String,
    synced: Vec<(String, usize)>,
    total: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SoulsyncProjectResult {
    project: String,
    oracle: String,
    synced: Vec<(String, usize)>,
    total: usize,
}

trait SoulsyncHost {
    fn soulsync_current_dir(&mut self) -> std::path::PathBuf;
    fn soulsync_tmux_cwd(&mut self) -> Option<std::path::PathBuf>;
    fn soulsync_git_common_dir(&mut self, cwd: &std::path::Path) -> Option<std::path::PathBuf>;
    fn soulsync_git_top_level(&mut self, cwd: &std::path::Path) -> Option<std::path::PathBuf>;
    fn soulsync_now(&mut self) -> String;
}

#[derive(Default)]
struct SoulsyncSystemHost;

impl SoulsyncHost for SoulsyncSystemHost {
    fn soulsync_current_dir(&mut self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    fn soulsync_tmux_cwd(&mut self) -> Option<std::path::PathBuf> {
        soulsync_run_process("tmux", &["display-message", "-p", "#{pane_current_path}"])
            .ok()
            .and_then(|text| soulsync_first_output_path(&text))
    }

    fn soulsync_git_common_dir(&mut self, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
        soulsync_validate_exec_path(cwd).ok()?;
        let cwd_text = cwd.to_string_lossy();
        let out = soulsync_run_process("git", &["-C", &cwd_text, "rev-parse", "--git-common-dir", "--"]).ok()?;
        let raw = soulsync_first_non_separator_line(&out)?;
        let path = std::path::PathBuf::from(raw);
        Some(if path.is_absolute() { path } else { cwd.join(path) })
    }

    fn soulsync_git_top_level(&mut self, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
        soulsync_validate_exec_path(cwd).ok()?;
        let cwd_text = cwd.to_string_lossy();
        let out = soulsync_run_process("git", &["-C", &cwd_text, "rev-parse", "--show-toplevel", "--"]).ok()?;
        soulsync_first_non_separator_line(&out).map(std::path::PathBuf::from)
    }

    fn soulsync_now(&mut self) -> String {
        now_iso_utc()
    }
}

fn soulsync_command(argv: &[String]) -> CliOutput {
    let mut host = SoulsyncSystemHost;
    soulsync_command_with(argv, &mut host, load_native_fleet)
}

fn soulsync_command_with(argv: &[String], host: &mut impl SoulsyncHost, fleet_loader: impl Fn() -> Vec<NativeFleetSession>) -> CliOutput {
    match soulsync_run(argv, host, fleet_loader) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn soulsync_run(argv: &[String], host: &mut impl SoulsyncHost, fleet_loader: impl Fn() -> Vec<NativeFleetSession>) -> Result<String, String> {
    let parsed = soulsync_parse_args(argv)?;
    match parsed.mode {
        SoulsyncMode::Oracle => soulsync_run_oracle(&parsed, host, &fleet_loader()),
        SoulsyncMode::Project => Ok(soulsync_run_project(host, &fleet_loader())),
    }
}

fn soulsync_parse_args(argv: &[String]) -> Result<SoulsyncArgs, String> {
    let mut target = None::<String>;
    let mut from = None::<String>;
    let mut project = false;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(SOULSYNC_USAGE.to_owned()),
            "--" => return Err("soul-sync: -- separator is not supported".to_owned()),
            "--project" => project = true,
            "--from" => {
                let value = soulsync_next_value(argv, index, "--from")?;
                from = Some(soulsync_validate_name(value, "from")?);
                index += 1;
            }
            value if value.starts_with("--from=") => {
                from = Some(soulsync_validate_name(&value["--from=".len()..], "from")?);
            }
            value if value.starts_with('-') => return Err(format!("soul-sync: unknown argument {value}")),
            value => {
                if target.is_some() { return Err(SOULSYNC_USAGE.to_owned()); }
                target = Some(soulsync_validate_name(value, "target")?);
            }
        }
        index += 1;
    }
    if project && (target.is_some() || from.is_some()) { return Err("soul-sync: --project cannot be combined with peer targets".to_owned()); }
    Ok(SoulsyncArgs { target, from, mode: if project { SoulsyncMode::Project } else { SoulsyncMode::Oracle } })
}

fn soulsync_next_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1).map(String::as_str) else { return Err(format!("soul-sync: missing value for {flag}")); };
    if value.starts_with('-') { return Err(format!("soul-sync: missing value for {flag}")); }
    Ok(value)
}

fn soulsync_run_oracle(args: &SoulsyncArgs, host: &mut impl SoulsyncHost, fleet: &[NativeFleetSession]) -> Result<String, String> {
    let cwd = soulsync_effective_cwd(host);
    let oracle_path = soulsync_oracle_path_from_cwd(host, &cwd);
    let oracle_name = soulsync_repo_base(&cwd).trim_end_matches("-oracle").to_owned();
    soulsync_validate_name(&oracle_name, "oracle")?;
    let peers = soulsync_oracle_peers(args, &oracle_name, fleet);
    if peers.is_empty() { return Ok(soulsync_render_no_peers(&oracle_name)); }
    let pulling = args.from.is_some();
    let mut out = soulsync_render_oracle_header(pulling, &oracle_name, &peers);
    let mut results = Vec::<SoulsyncSyncResult>::new();
    for peer in peers {
        soulsync_validate_name(&peer, "peer")?;
        let repos_root = soulsync_repos_root_from_repo(&oracle_path);
        let Some(peer_path) = soulsync_resolve_oracle_path(&peer, fleet, &repos_root) else {
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m {peer}: repo not found, skipping");
            continue;
        };
        let result = if pulling {
            soulsync_sync_oracle_vaults(&peer_path, &oracle_path, &peer, &oracle_name, host)
        } else {
            soulsync_sync_oracle_vaults(&oracle_path, &peer_path, &oracle_name, &peer, host)
        };
        soulsync_render_oracle_result(&mut out, &result);
        results.push(result);
    }
    soulsync_render_total(&mut out, results.iter().map(|result| result.total).sum(), "synced");
    Ok(out)
}

fn soulsync_run_project(host: &mut impl SoulsyncHost, fleet: &[NativeFleetSession]) -> String {
    let cwd = soulsync_effective_cwd(host);
    let current_repo = host.soulsync_git_top_level(&cwd).unwrap_or(cwd);
    let github_root = soulsync_repos_root_from_repo(&current_repo);
    let repo_slug = soulsync_project_slug(&current_repo, &github_root);
    let repo_base = soulsync_repo_base(&current_repo);
    let is_oracle = repo_base.ends_with("-oracle");
    let mut out = format!("\n  \x1b[36m⚡ Soul Sync (project)\x1b[0m — {} {repo_base}\n\n", if is_oracle { "absorbing into" } else { "exporting from" });
    let mut totals = Vec::<usize>::new();
    if is_oracle { soulsync_project_from_oracle(&mut out, &current_repo, &repo_base, fleet, host, &mut totals); }
    else { soulsync_project_from_repo(&mut out, &current_repo, &github_root, repo_slug.as_deref(), fleet, host, &mut totals); }
    soulsync_render_total(&mut out, totals.iter().sum(), "absorbed");
    out
}

fn soulsync_project_from_oracle(out: &mut String, oracle_repo: &std::path::Path, repo_base: &str, fleet: &[NativeFleetSession], host: &mut impl SoulsyncHost, totals: &mut Vec<usize>) {
    let oracle_name = repo_base.trim_end_matches("-oracle");
    let projects = soulsync_projects_for_oracle(oracle_name, fleet);
    if projects.is_empty() {
        let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m no project_repos configured for '{oracle_name}'");
        let _ = writeln!(out, "  \x1b[90mAdd \"project_repos\": [\"org/repo\"] to fleet config for {oracle_name}.\x1b[0m");
        return;
    }
    let github_root = soulsync_repos_root_from_repo(oracle_repo);
    for project in projects {
        let project_path = github_root.join(&project);
        if !project_path.exists() {
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m {project}: not found at {}, skipping", project_path.display());
            continue;
        }
        let result = soulsync_sync_project_vault(&project_path, oracle_repo, &project, oracle_name, host);
        totals.push(result.total);
        soulsync_render_project_result(out, &result);
    }
}

fn soulsync_project_from_repo(out: &mut String, project_repo: &std::path::Path, github_root: &std::path::Path, repo_slug: Option<&str>, fleet: &[NativeFleetSession], host: &mut impl SoulsyncHost, totals: &mut Vec<usize>) {
    let Some(slug) = repo_slug else {
        let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m cannot resolve project slug from {} (not under repos root {})", project_repo.display(), github_root.display());
        return;
    };
    let Some(oracle_name) = soulsync_oracle_for_project(slug, fleet) else {
        let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m no oracle owns project '{slug}'");
        let _ = writeln!(out, "  \x1b[90mAdd \"project_repos\": [\"{slug}\"] to an oracle's fleet config.\x1b[0m");
        return;
    };
    let Some(oracle_path) = soulsync_resolve_oracle_path(&oracle_name, fleet, github_root) else {
        let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m oracle '{oracle_name}' repo not found locally");
        return;
    };
    let result = soulsync_sync_project_vault(project_repo, &oracle_path, slug, &oracle_name, host);
    totals.push(result.total);
    soulsync_render_project_result(out, &result);
}

fn soulsync_effective_cwd(host: &mut impl SoulsyncHost) -> std::path::PathBuf {
    host.soulsync_tmux_cwd().unwrap_or_else(|| host.soulsync_current_dir())
}

fn soulsync_oracle_path_from_cwd(host: &mut impl SoulsyncHost, cwd: &std::path::Path) -> std::path::PathBuf {
    host.soulsync_git_common_dir(cwd).filter(|path| path.file_name().and_then(std::ffi::OsStr::to_str) != Some(".git")).and_then(|path| path.parent().map(std::path::Path::to_path_buf)).unwrap_or_else(|| cwd.to_path_buf())
}

fn soulsync_oracle_peers(args: &SoulsyncArgs, oracle_name: &str, fleet: &[NativeFleetSession]) -> Vec<String> {
    if let Some(source) = &args.from { return vec![source.clone()]; }
    args.target.as_ref().map_or_else(|| soulsync_peers_for_oracle(oracle_name, fleet), |target| vec![target.clone()])
}

fn soulsync_peers_for_oracle(oracle_name: &str, fleet: &[NativeFleetSession]) -> Vec<String> {
    fleet.iter().find(|session| soulsync_session_name(&session.name) == oracle_name).map_or_else(Vec::new, |session| session.sync_peers.clone())
}

fn soulsync_projects_for_oracle(oracle_name: &str, fleet: &[NativeFleetSession]) -> Vec<String> {
    fleet.iter().find(|session| soulsync_session_name(&session.name) == oracle_name).map_or_else(Vec::new, |session| session.project_repos.clone())
}

fn soulsync_oracle_for_project(project_repo: &str, fleet: &[NativeFleetSession]) -> Option<String> {
    fleet.iter().find(|session| session.project_repos.iter().any(|repo| repo == project_repo)).map(|session| soulsync_session_name(&session.name))
}

fn soulsync_resolve_oracle_path(name: &str, fleet: &[NativeFleetSession], repos_root: &std::path::Path) -> Option<std::path::PathBuf> {
    let stem = name.trim_end_matches("-oracle");
    if let Some(path) = soulsync_find_oracle_repo(repos_root, stem) { return Some(path); }
    fleet.iter().find(|session| soulsync_session_name(&session.name) == stem).and_then(|session| session.windows.first()).map(|window| repos_root.join(&window.repo)).filter(|path| path.exists())
}

fn soulsync_find_oracle_repo(repos_root: &std::path::Path, stem: &str) -> Option<std::path::PathBuf> {
    let wanted = format!("{stem}-oracle").to_lowercase();
    let Ok(orgs) = std::fs::read_dir(repos_root) else { return None; };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(repos) = std::fs::read_dir(org.path()) else { continue; };
        for repo in repos.flatten().filter(|entry| entry.path().is_dir()) {
            if repo.file_name().to_string_lossy().eq_ignore_ascii_case(&wanted) { return Some(repo.path()); }
        }
    }
    None
}

fn soulsync_sync_oracle_vaults(from_path: &std::path::Path, to_path: &std::path::Path, from_name: &str, to_name: &str, host: &mut impl SoulsyncHost) -> SoulsyncSyncResult {
    let synced = soulsync_sync_dirs(from_path, to_path);
    let total = synced.iter().map(|(_, count)| *count).sum();
    let result = SoulsyncSyncResult { from: from_name.to_owned(), to: to_name.to_owned(), synced, total };
    if total > 0 { soulsync_append_log(to_path, &format!("{from_name} → {to_name}"), total, &result.synced, host); }
    result
}

fn soulsync_sync_project_vault(project_path: &std::path::Path, oracle_path: &std::path::Path, project_repo: &str, oracle_name: &str, host: &mut impl SoulsyncHost) -> SoulsyncProjectResult {
    let synced = soulsync_sync_dirs(project_path, oracle_path);
    let total = synced.iter().map(|(_, count)| *count).sum();
    let result = SoulsyncProjectResult { project: project_repo.to_owned(), oracle: oracle_name.to_owned(), synced, total };
    if total > 0 { soulsync_append_log(oracle_path, &format!("project:{project_repo} → {oracle_name}"), total, &result.synced, host); }
    result
}

fn soulsync_sync_dirs(from_path: &std::path::Path, to_path: &std::path::Path) -> Vec<(String, usize)> {
    let mut synced = Vec::<(String, usize)>::new();
    for dir in SOULSYNC_DIRS {
        let count = soulsync_sync_dir(&from_path.join("ψ").join(dir), &to_path.join("ψ").join(dir));
        if count > 0 { synced.push(((*dir).to_owned(), count)); }
    }
    synced
}

fn soulsync_sync_dir(src: &std::path::Path, dst: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(src) else { return 0; };
    let mut count = 0_usize;
    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() { count += soulsync_sync_dir(&src_path, &dst_path); }
        else if !dst_path.exists() { count += soulsync_copy_new_file(&src_path, &dst_path); }
    }
    count
}

fn soulsync_copy_new_file(src: &std::path::Path, dst: &std::path::Path) -> usize {
    if let Some(parent) = dst.parent() { let _ = std::fs::create_dir_all(parent); }
    std::fs::copy(src, dst).map_or(0, |_| 1)
}

fn soulsync_append_log(to_path: &std::path::Path, label: &str, total: usize, synced: &[(String, usize)], host: &mut impl SoulsyncHost) {
    let log_dir = to_path.join("ψ/.soul-sync");
    if std::fs::create_dir_all(&log_dir).is_err() { return; }
    let summary = soulsync_summary(synced);
    let line = format!("{} | {label} | {total} files | {summary}\n", host.soulsync_now());
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("sync.log")).and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn soulsync_render_no_peers(oracle_name: &str) -> String {
    format!("  \x1b[33m⚠\x1b[0m soul-sync: no sync_peers configured for '{oracle_name}'\n  \x1b[90mAdd \"sync_peers\": [\"name\"] to fleet config, or run: maw ss <peer>\x1b[0m\n")
}

fn soulsync_render_oracle_header(pulling: bool, oracle_name: &str, peers: &[String]) -> String {
    let label = if pulling { format!("pulling {} → {oracle_name}", peers.first().map_or("", String::as_str)) } else { format!("pushing {oracle_name} → {}", peers.join(", ")) };
    format!("\n  \x1b[36m⚡ Soul Sync\x1b[0m — {label}\n\n")
}

fn soulsync_render_oracle_result(out: &mut String, result: &SoulsyncSyncResult) {
    if result.total == 0 { let _ = writeln!(out, "  \x1b[90m○\x1b[0m {} → {}: nothing new", result.from, result.to); }
    else { let _ = writeln!(out, "  \x1b[32m✓\x1b[0m {} → {}: {}", result.from, result.to, soulsync_summary(&result.synced)); }
}

fn soulsync_render_project_result(out: &mut String, result: &SoulsyncProjectResult) {
    if result.total == 0 { let _ = writeln!(out, "  \x1b[90m○\x1b[0m project:{} → {}: nothing new", result.project, result.oracle); }
    else { let _ = writeln!(out, "  \x1b[32m✓\x1b[0m project:{} → {}: {}", result.project, result.oracle, soulsync_summary(&result.synced)); }
}

fn soulsync_render_total(out: &mut String, total: usize, verb: &str) {
    if total > 0 { let _ = writeln!(out, "\n  \x1b[32m{total} file(s) {verb}.\x1b[0m\n"); }
    else { out.push('\n'); }
}

fn soulsync_summary(synced: &[(String, usize)]) -> String {
    synced.iter().map(|(dir, count)| format!("{count} {}", dir.rsplit('/').next().unwrap_or(dir))).collect::<Vec<_>>().join(", ")
}

fn soulsync_repos_root_from_repo(repo_root: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for component in repo_root.components() {
        out.push(component.as_os_str());
        if component.as_os_str() == "github.com" { return out; }
    }
    ghq_root().join("github.com")
}

fn soulsync_project_slug(project_repo: &std::path::Path, github_root: &std::path::Path) -> Option<String> {
    let rel = project_repo.strip_prefix(github_root).ok()?.components().filter_map(|item| item.as_os_str().to_str()).collect::<Vec<_>>();
    if rel.len() >= 4 && rel[2] == "agents" { return Some(format!("{}/{}", rel[0], rel[1])); }
    if rel.len() < 2 { return None; }
    Some(format!("{}/{}", rel[0], rel[1].replace(".wt-", "#").split('#').next().unwrap_or(rel[1])))
}

fn soulsync_repo_base(path: &std::path::Path) -> String {
    let parts = path.components().filter_map(|part| part.as_os_str().to_str()).collect::<Vec<_>>();
    if parts.len() >= 3 && parts[parts.len() - 2] == "agents" { return parts[parts.len() - 3].to_owned(); }
    let base = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default();
    base.split(".wt-").next().unwrap_or(base).to_owned()
}

fn soulsync_session_name(name: &str) -> String {
    name.split_once('-').filter(|(prefix, suffix)| prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.is_empty()).map_or(name, |(_, suffix)| suffix).trim_end_matches("-oracle").to_owned()
}

fn soulsync_validate_name(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("soul-sync: invalid {label} {value:?}"));
    }
    Ok(value.to_owned())
}

fn soulsync_validate_exec_path(path: &std::path::Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if text.is_empty() || text.starts_with('-') || text.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) { return Err("soul-sync: invalid exec path".to_owned()); }
    Ok(())
}

fn soulsync_run_process(program: &str, args: &[&str]) -> Result<String, String> {
    soulsync_validate_process_argv(program, args)?;
    let output = std::process::Command::new(program).args(args).output().map_err(|error| format!("soul-sync: {program} failed: {error}"))?;
    if !output.status.success() { return Err(format!("soul-sync: {program} exited with {}", output.status)); }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn soulsync_validate_process_argv(program: &str, args: &[&str]) -> Result<(), String> {
    if program.is_empty() || program.starts_with('-') { return Err("soul-sync: invalid program".to_owned()); }
    if program == "git" && !args.contains(&"--") { return Err("soul-sync: git argv missing -- separator".to_owned()); }
    if args.iter().any(|arg| arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control())) { return Err("soul-sync: invalid argv".to_owned()); }
    Ok(())
}

fn soulsync_first_output_path(text: &str) -> Option<std::path::PathBuf> {
    soulsync_first_non_separator_line(text).map(std::path::PathBuf::from)
}

fn soulsync_first_non_separator_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty() && *line != "--")
}

#[cfg(test)]
mod soulsync_tests {
    use super::*;

    #[derive(Clone)]
    struct SoulsyncFakeHost {
        cwd: std::path::PathBuf,
        tmux: Option<std::path::PathBuf>,
        common: Option<std::path::PathBuf>,
        top: Option<std::path::PathBuf>,
        now: String,
    }

    impl SoulsyncHost for SoulsyncFakeHost {
        fn soulsync_current_dir(&mut self) -> std::path::PathBuf { self.cwd.clone() }
        fn soulsync_tmux_cwd(&mut self) -> Option<std::path::PathBuf> { self.tmux.clone() }
        fn soulsync_git_common_dir(&mut self, _: &std::path::Path) -> Option<std::path::PathBuf> { self.common.clone() }
        fn soulsync_git_top_level(&mut self, _: &std::path::Path) -> Option<std::path::PathBuf> { self.top.clone() }
        fn soulsync_now(&mut self) -> String { self.now.clone() }
    }

    fn soulsync_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn soulsync_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("maw-rs-soulsync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn soulsync_host(cwd: std::path::PathBuf) -> SoulsyncFakeHost {
        SoulsyncFakeHost { cwd, tmux: None, common: None, top: None, now: "2026-06-25T00:00:00.000Z".to_owned() }
    }

    fn soulsync_session(name: &str, repo: &str, peers: &[&str], projects: &[&str]) -> NativeFleetSession {
        NativeFleetSession { name: name.to_owned(), windows: vec![NativeFleetWindow { name: name.to_owned(), repo: repo.to_owned() }], sync_peers: peers.iter().map(|value| (*value).to_owned()).collect(), project_repos: projects.iter().map(|value| (*value).to_owned()).collect() }
    }

    fn soulsync_empty_fleet() -> Vec<NativeFleetSession> { Vec::new() }

    fn soulsync_write(path: &std::path::Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, text).expect("write");
    }

    #[test]
    fn soulsync_dispatch_registers_command_and_aliases() {
        let commands = DISPATCH_94.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, vec!["soul-sync", "soulsync", "ss"]);
    }

    #[test]
    fn soulsync_parse_rejects_injection_before_io() {
        assert!(soulsync_parse_args(&soulsync_strings(&["--from", "-bad"])).unwrap_err().contains("missing value"));
        assert!(soulsync_parse_args(&soulsync_strings(&["--"])).unwrap_err().contains("separator"));
        assert!(soulsync_parse_args(&soulsync_strings(&["-bad"])).unwrap_err().contains("unknown"));
    }

    #[test]
    fn soulsync_oracle_push_copies_new_files_hermetically() {
        let root = soulsync_root("push");
        let ghq = root.join("github.com/org");
        let neo = ghq.join("neo-oracle");
        let trinity = ghq.join("trinity-oracle");
        soulsync_write(&neo.join("ψ/memory/learnings/a.md"), "new");
        soulsync_write(&trinity.join("ψ/memory/learnings/existing.md"), "old");
        let mut host = soulsync_host(neo.clone());
        let fleet = vec![soulsync_session("01-neo", "org/neo-oracle", &["trinity"], &[]), soulsync_session("02-trinity", "org/trinity-oracle", &[], &[])];
        let out = soulsync_run(&soulsync_strings(&[]), &mut host, || fleet.clone()).expect("run");
        assert!(out.contains("pushing neo → trinity"));
        assert_eq!(std::fs::read_to_string(trinity.join("ψ/memory/learnings/a.md")).expect("copied"), "new");
        assert!(std::fs::read_to_string(trinity.join("ψ/.soul-sync/sync.log")).expect("log").contains("neo → trinity"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn soulsync_oracle_pull_uses_from_peer() {
        let root = soulsync_root("pull");
        let ghq = root.join("github.com/org");
        let neo = ghq.join("neo-oracle");
        let trinity = ghq.join("trinity-oracle");
        soulsync_write(&trinity.join("ψ/memory/traces/t.md"), "trace");
        let mut host = soulsync_host(neo.clone());
        let fleet = vec![soulsync_session("01-neo", "org/neo-oracle", &[], &[]), soulsync_session("02-trinity", "org/trinity-oracle", &[], &[])];
        let out = soulsync_run(&soulsync_strings(&["--from", "trinity"]), &mut host, || fleet.clone()).expect("run");
        assert!(out.contains("pulling trinity → neo"));
        assert_eq!(std::fs::read_to_string(neo.join("ψ/memory/traces/t.md")).expect("copied"), "trace");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn soulsync_project_from_oracle_absorbs_projects() {
        let root = soulsync_root("project-oracle");
        let ghq = root.join("github.com");
        let oracle = ghq.join("org/neo-oracle");
        let project = ghq.join("org/app");
        soulsync_write(&project.join("ψ/memory/collaborations/c.md"), "collab");
        let mut host = soulsync_host(oracle.clone());
        host.top = Some(oracle.clone());
        let fleet = vec![soulsync_session("01-neo", "org/neo-oracle", &[], &["org/app"] )];
        let out = soulsync_run(&soulsync_strings(&["--project"]), &mut host, || fleet.clone()).expect("run");
        assert!(out.contains("absorbing into neo-oracle"));
        assert_eq!(std::fs::read_to_string(oracle.join("ψ/memory/collaborations/c.md")).expect("copied"), "collab");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn soulsync_project_from_repo_exports_to_owner() {
        let root = soulsync_root("project-repo");
        let ghq = root.join("github.com");
        let oracle = ghq.join("org/neo-oracle");
        let project = ghq.join("org/app");
        soulsync_write(&project.join("ψ/memory/learnings/p.md"), "project");
        std::fs::create_dir_all(&oracle).expect("oracle");
        let mut host = soulsync_host(project.clone());
        host.top = Some(project.clone());
        let fleet = vec![soulsync_session("01-neo", "org/neo-oracle", &[], &["org/app"] )];
        let out = soulsync_run(&soulsync_strings(&["--project"]), &mut host, || fleet.clone()).expect("run");
        assert!(out.contains("exporting from app"));
        assert_eq!(std::fs::read_to_string(oracle.join("ψ/memory/learnings/p.md")).expect("copied"), "project");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn soulsync_no_peer_message_does_not_touch_real_state() {
        let root = soulsync_root("nope");
        let oracle = root.join("github.com/org/neo-oracle");
        std::fs::create_dir_all(&oracle).expect("oracle");
        let mut host = soulsync_host(oracle);
        let out = soulsync_run(&soulsync_strings(&[]), &mut host, soulsync_empty_fleet).expect("run");
        assert!(out.contains("no sync_peers configured"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn soulsync_git_argv_requires_separator() {
        assert!(soulsync_validate_process_argv("git", &["-C", "/tmp", "rev-parse"]).unwrap_err().contains("separator"));
        assert!(soulsync_validate_process_argv("git", &["-C", "/tmp", "rev-parse", "--"]).is_ok());
    }
}
