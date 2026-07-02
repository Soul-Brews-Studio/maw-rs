const DISPATCH_110: &[DispatcherEntry] = &[
    DispatcherEntry { command: "find", handler: Handler::Sync(find_run_command) },
];

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct FindFleetSession {
    name: String,
    #[serde(default)]
    windows: Vec<FindFleetWindow>,
    #[serde(default)]
    sync_peers: Vec<String>,
    #[serde(default)]
    project_repos: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct FindFleetWindow {
    name: String,
    #[serde(default)]
    repo: String,
}

#[derive(Debug, Clone)]
struct FindArgs {
    keyword: String,
    oracle: Option<String>,
}

#[derive(Debug, Clone)]
struct FindOracleMatch {
    name: String,
    slug: String,
}

#[derive(Debug, Clone)]
struct FindCodeMatch {
    oracle: String,
    file: String,
    line: String,
}

fn find_run_command(cli_args: &[String]) -> CliOutput {
    let parsed = match find_parse_args(cli_args) {
        Ok(parsed) => parsed,
        Err(error) => return find_error(&error),
    };
    CliOutput { code: 0, stdout: find_native_render(&parsed), stderr: String::new() }
}

fn find_parse_args(argv: &[String]) -> Result<FindArgs, String> {
    let mut keyword = None::<String>;
    let mut oracle = None::<String>;
    let mut index = 0;
    while index < argv.len() {
        let token = &argv[index];
        if token == "--" {
            index += 1;
            while index < argv.len() {
                find_push_keyword(&mut keyword, &argv[index])?;
                index += 1;
            }
            break;
        }
        match token.as_str() {
            "--oracle" => {
                oracle = Some(find_take_value(argv, &mut index, "--oracle")?);
            }
            "--help" | "-h" => return Err(find_usage().to_owned()),
            _ if token.starts_with('-') => return Err(format!("find: unknown flag {token}")),
            _ => {
                find_push_keyword(&mut keyword, token)?;
                index += 1;
            }
        }
    }
    let Some(keyword) = keyword else { return Err(find_usage().to_owned()); };
    Ok(FindArgs { keyword, oracle })
}

fn find_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err(format!("find: missing {flag} value")); };
    find_validate_value(flag, value)?;
    *index += 1;
    Ok(value.clone())
}

fn find_push_keyword(keyword: &mut Option<String>, value: &str) -> Result<(), String> {
    find_validate_value("keyword", value)?;
    if keyword.is_some() {
        return Err("usage: maw find <keyword> [--oracle <name>]".to_owned());
    }
    *keyword = Some(value.to_owned());
    Ok(())
}

fn find_validate_value(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.contains('\n') {
        return Err(format!("find: invalid {kind} value"));
    }
    Ok(())
}

fn find_usage() -> &'static str { "usage: maw find <keyword> [--oracle <name>]" }

fn find_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn find_native_render(args: &FindArgs) -> String {
    let kw = args.keyword.to_lowercase();
    let repos_root = find_ghq_root().join("github.com");
    let fleet = find_load_fleet();
    let oracle_matches = find_oracle_matches(&repos_root, &kw, args.oracle.as_deref());
    let fleet_matches = find_fleet_matches(&fleet, &kw, args.oracle.as_deref());
    let targets = find_code_targets(&repos_root, &fleet, args.oracle.as_deref());
    let code_results = find_code_matches(&targets, &kw);
    find_native_render_sections(args, &oracle_matches, &fleet_matches, &code_results, targets.len())
}

fn find_oracle_matches(
    repos_root: &std::path::Path,
    kw: &str,
    oracle_filter: Option<&str>,
) -> Vec<FindOracleMatch> {
    let mut matches = Vec::new();
    let Ok(orgs) = std::fs::read_dir(repos_root) else { return matches; };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        find_scan_org(&org, kw, oracle_filter, &mut matches);
    }
    matches.sort_by(|left, right| (left.name.as_str(), left.slug.as_str()).cmp(&(right.name.as_str(), right.slug.as_str())));
    matches
}

fn find_scan_org(
    org: &std::fs::DirEntry,
    kw: &str,
    oracle_filter: Option<&str>,
    out: &mut Vec<FindOracleMatch>,
) {
    let org_name = org.file_name().to_string_lossy().into_owned();
    let Ok(repos) = std::fs::read_dir(org.path()) else { return; };
    for repo in repos.flatten().filter(|entry| entry.path().is_dir()) {
        find_maybe_push_oracle(&org_name, &repo, kw, oracle_filter, out);
    }
}

fn find_maybe_push_oracle(
    org_name: &str,
    repo: &std::fs::DirEntry,
    kw: &str,
    oracle_filter: Option<&str>,
    out: &mut Vec<FindOracleMatch>,
) {
    let repo_name_raw = repo.file_name().to_string_lossy().into_owned();
    let repo_name = repo_name_raw.strip_suffix("-oracle").unwrap_or(&repo_name_raw).to_owned();
    let slug = format!("{org_name}/{repo_name_raw}");
    if oracle_filter.is_some_and(|wanted| wanted != repo_name) {
        return;
    }
    if repo_name.to_lowercase().contains(kw) || slug.to_lowercase().contains(kw) {
        out.push(FindOracleMatch { name: repo_name, slug });
    }
}

fn find_fleet_matches(fleet: &[FindFleetSession], kw: &str, oracle_filter: Option<&str>) -> Vec<String> {
    let mut matches = Vec::new();
    for session in fleet {
        find_maybe_push_fleet_session(session, kw, oracle_filter, &mut matches);
    }
    matches
}

fn find_maybe_push_fleet_session(
    session: &FindFleetSession,
    kw: &str,
    oracle_filter: Option<&str>,
    out: &mut Vec<String>,
) {
    let oracle_name = find_oracle_name(&session.name);
    if oracle_filter.is_some_and(|wanted| wanted != oracle_name) {
        return;
    }
    if session.name.to_lowercase().contains(kw) || oracle_name.to_lowercase().contains(kw) {
        out.push(format!("session {}", session.name));
    }
    for window in &session.windows {
        find_maybe_push_window(window, kw, out);
    }
    for peer in &session.sync_peers {
        if peer.to_lowercase().contains(kw) {
            out.push(format!("sync_peer {peer}"));
        }
    }
    for repo in &session.project_repos {
        if repo.to_lowercase().contains(kw) {
            out.push(format!("project_repo {repo}"));
        }
    }
}

fn find_maybe_push_window(window: &FindFleetWindow, kw: &str, out: &mut Vec<String>) {
    if window.name.to_lowercase().contains(kw) || window.repo.to_lowercase().contains(kw) {
        let detail = if window.repo.is_empty() {
            format!("window {}", window.name)
        } else {
            format!("window {} ({})", window.name, window.repo)
        };
        out.push(detail);
    }
}

fn find_code_targets(
    repos_root: &std::path::Path,
    fleet: &[FindFleetSession],
    oracle_filter: Option<&str>,
) -> Vec<(String, std::path::PathBuf)> {
    let mut targets = Vec::new();
    for session in fleet {
        find_maybe_push_fleet_target(repos_root, session, oracle_filter, &mut targets);
    }
    find_maybe_push_local_target(&mut targets);
    targets
}

fn find_maybe_push_fleet_target(
    repos_root: &std::path::Path,
    session: &FindFleetSession,
    oracle_filter: Option<&str>,
    targets: &mut Vec<(String, std::path::PathBuf)>,
) {
    let oracle_name = find_oracle_name(&session.name).to_owned();
    if oracle_filter.is_some_and(|wanted| wanted != oracle_name) {
        return;
    }
    let Some(window) = session.windows.first() else { return; };
    if window.repo.is_empty() {
        return;
    }
    let psi = repos_root.join(&window.repo).join("ψ").join("memory");
    if psi.exists() {
        targets.push((oracle_name, psi));
    }
}

fn find_maybe_push_local_target(targets: &mut Vec<(String, std::path::PathBuf)>) {
    let local_psi = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("ψ")
        .join("memory");
    if !local_psi.exists() || targets.iter().any(|(_, path)| *path == local_psi) {
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("local"));
    let name = cwd
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("local")
        .trim_end_matches("-oracle")
        .to_owned();
    targets.push((name, local_psi));
}

fn find_code_matches(targets: &[(String, std::path::PathBuf)], kw: &str) -> Vec<FindCodeMatch> {
    let mut results = Vec::new();
    for (name, root) in targets {
        find_collect_code_matches(name, root, kw, &mut results);
    }
    results
}

fn find_collect_code_matches(name: &str, root: &std::path::Path, kw: &str, out: &mut Vec<FindCodeMatch>) {
    let Ok(entries) = std::fs::read_dir(root) else { return; };
    for entry in entries.flatten() {
        find_scan_code_entry(name, root, kw, &entry.path(), out);
    }
}

fn find_scan_code_entry(
    name: &str,
    root: &std::path::Path,
    kw: &str,
    path: &std::path::Path,
    out: &mut Vec<FindCodeMatch>,
) {
    if path.is_dir() {
        find_collect_code_matches(name, path, kw, out);
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return; };
    let Some(line) = text.lines().find(|line| line.to_lowercase().contains(kw)) else { return; };
    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
    out.push(FindCodeMatch { oracle: name.to_owned(), file: rel, line: line.trim().to_owned() });
}

fn find_native_render_sections(
    args: &FindArgs,
    oracle_matches: &[FindOracleMatch],
    fleet_matches: &[String],
    code_results: &[FindCodeMatch],
    target_count: usize,
) -> String {
    let total = oracle_matches.len() + fleet_matches.len() + code_results.len();
    let mut out = format!("\n  \x1b[36m🔍 Searching\x1b[0m — \"{}\"\n\n", args.keyword);
    if total == 0 {
        let _ = write!(out, "  \x1b[90m○\x1b[0m no matches found across {target_count} oracle(s)\n\n");
        return out;
    }
    find_native_render_oracles(&mut out, oracle_matches);
    find_native_render_fleet(&mut out, fleet_matches);
    find_native_render_code(&mut out, code_results);
    find_native_render_summary(&mut out, oracle_matches.len(), fleet_matches.len(), code_results.len());
    out
}

fn find_native_render_oracles(out: &mut String, matches: &[FindOracleMatch]) {
    if matches.is_empty() {
        return;
    }
    out.push_str("  \x1b[36m── Oracles ──\x1b[0m\n");
    for item in matches {
        let _ = writeln!(out, "    \x1b[1m{}\x1b[0m \x1b[90m({})\x1b[0m", item.name, item.slug);
    }
    out.push('\n');
}

fn find_native_render_fleet(out: &mut String, matches: &[String]) {
    if matches.is_empty() {
        return;
    }
    out.push_str("  \x1b[36m── Fleet ──\x1b[0m\n");
    for detail in matches {
        let _ = writeln!(out, "    \x1b[90m{detail}\x1b[0m");
    }
    out.push('\n');
}

fn find_native_render_code(out: &mut String, matches: &[FindCodeMatch]) {
    if matches.is_empty() {
        return;
    }
    out.push_str("  \x1b[36m── Code ──\x1b[0m\n");
    let mut grouped: BTreeMap<&str, Vec<&FindCodeMatch>> = BTreeMap::new();
    for result in matches {
        grouped.entry(&result.oracle).or_default().push(result);
    }
    for (oracle, group) in grouped {
        find_native_render_code_group(out, oracle, &group);
    }
    out.push('\n');
}

fn find_native_render_code_group(out: &mut String, oracle: &str, matches: &[&FindCodeMatch]) {
    let _ = writeln!(out, "    \x1b[36m{oracle}\x1b[0m ({} match{})", matches.len(), if matches.len() == 1 { "" } else { "es" });
    for item in matches.iter().take(10) {
        let _ = writeln!(out, "      \x1b[90m{}\x1b[0m", item.file);
        if !item.line.is_empty() {
            let truncated = item.line.chars().take(120).collect::<String>();
            let _ = writeln!(out, "        {truncated}");
        }
    }
    if matches.len() > 10 {
        let _ = writeln!(out, "      \x1b[90m... and {} more\x1b[0m", matches.len() - 10);
    }
}

fn find_native_render_summary(out: &mut String, oracle_count: usize, fleet_count: usize, code_count: usize) {
    let total = oracle_count + fleet_count + code_count;
    let mut parts = Vec::new();
    if oracle_count > 0 {
        parts.push(format!("{oracle_count} oracle(s)"));
    }
    if fleet_count > 0 {
        parts.push(format!("{fleet_count} fleet"));
    }
    if code_count > 0 {
        parts.push(format!("{code_count} code"));
    }
    let _ = write!(out, "  \x1b[32m{total} match(es)\x1b[0m — {}\n\n", parts.join(", "));
}

fn find_oracle_name(session_name: &str) -> &str {
    session_name.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-')
}

fn find_ghq_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(
        || {
            std::env::var_os("HOME")
                .map_or_else(|| std::path::PathBuf::from(".").join("Code"), |home| std::path::PathBuf::from(home).join("Code"))
        },
        |value| {
            let mut path = std::path::PathBuf::from(value);
            if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("github.com") {
                path.pop();
            }
            path
        },
    )
}

fn find_load_fleet() -> Vec<FindFleetSession> {
    let fleet_dir = find_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else { return Vec::new(); };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect()
}

fn find_config_dir() -> std::path::PathBuf {
    let env = find_current_xdg_env();
    maw_config_dir(&env)
}

fn find_current_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "MAW_STATE_DIR",
        "XDG_DATA_HOME",
        "MAW_DATA_DIR",
        "XDG_CACHE_HOME",
        "MAW_CACHE_DIR",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}
