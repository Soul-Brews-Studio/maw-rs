#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct NativeScope {
    name: String,
    members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lead: Option<String>,
    created: String,
    ttl: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
struct NativeFleetSession {
    name: String,
    #[serde(default, rename = "squadName", alias = "groupName")]
    squad_name: String,
    #[serde(default)]
    windows: Vec<NativeFleetWindow>,
    #[serde(default, alias = "syncPeers")]
    sync_peers: Vec<String>,
    #[serde(default, alias = "projectRepos")]
    project_repos: Vec<String>,
    #[serde(default, alias = "skipCommand")]
    skip_command: Option<serde_json::Value>,
    #[serde(default, alias = "buddedFrom")]
    budded_from: Option<String>,
    #[serde(default)]
    members: Option<Vec<NativeFleetMember>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
struct NativeFleetWindow {
    name: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    kind: Option<NativeRepoKind>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum NativeRepoKind {
    Oracle,
    Project,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeFleetEntry {
    file: String,
    path: std::path::PathBuf,
    session: NativeFleetSession,
}

#[allow(dead_code)]
fn run_scope_command(argv: &[String]) -> CliOutput {
    let positional = argv
        .iter()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let Some(sub) = positional.first().copied() else {
        return CliOutput { code: 0, stdout: format!("{}\n", scope_help()), stderr: String::new() };
    };

    match sub {
        "list" | "ls" => match scope_list() {
            Ok(scopes) => CliOutput { code: 0, stdout: format!("{}\n", format_scope_list(&scopes)), stderr: String::new() },
            Err(error) => scope_error(&error),
        },
        "create" | "new" => run_scope_create(argv, &positional),
        "show" | "info" => run_scope_show(&positional),
        "delete" | "rm" | "remove" => run_scope_delete(argv, &positional),
        _ => CliOutput {
            code: 1,
            stdout: format!("{}\n", scope_help()),
            stderr: format!("maw scope: unknown subcommand \"{sub}\" (expected list|create|show|delete)\n"),
        },
    }
}

#[allow(dead_code)]
fn run_scope_create(argv: &[String], positional: &[&str]) -> CliOutput {
    let Some(name) = positional.get(1).copied() else {
        return scope_error("usage: maw scope create <name> --members <a,b,c> [--lead <m>] [--ttl <iso>]");
    };
    let Some(members_raw) = flag_value(argv, "--members") else {
        return scope_error(&format!("usage: maw scope create {name} --members <a,b,c> [--lead <m>] [--ttl <iso>]"));
    };
    let members = members_raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    match scope_create(name, members, flag_value(argv, "--lead"), flag_value(argv, "--ttl")) {
        Ok(scope) => CliOutput {
            code: 0,
            stdout: format!(
                "created scope \"{}\" ({} member{})\n  {}\n",
                scope.name,
                scope.members.len(),
                if scope.members.len() == 1 { "" } else { "s" },
                scope_path(&scope.name).display()
            ),
            stderr: String::new(),
        },
        Err(error) => scope_error(&error),
    }
}

#[allow(dead_code)]
fn run_scope_show(positional: &[&str]) -> CliOutput {
    let Some(name) = positional.get(1).copied() else {
        return scope_error("usage: maw scope show <name>");
    };
    if let Err(error) = validate_scope_name(name) {
        return scope_error(&error);
    }
    match load_scope(name) {
        Ok(Some(scope)) => match serde_json::to_string_pretty(&scope) {
            Ok(json) => CliOutput { code: 0, stdout: format!("{json}\n"), stderr: String::new() },
            Err(error) => scope_error(&format!("scope: failed to render {name}: {error}")),
        },
        Ok(None) => scope_error(&format!("scope \"{name}\" not found")),
        Err(error) => scope_error(&error),
    }
}

#[allow(dead_code)]
fn run_scope_delete(argv: &[String], positional: &[&str]) -> CliOutput {
    let Some(name) = positional.get(1).copied() else {
        return scope_error("usage: maw scope delete <name> [--yes]");
    };
    if !argv.iter().any(|arg| matches!(arg.as_str(), "--yes" | "-y")) {
        return CliOutput {
            code: 1,
            stdout: format!("refusing to delete scope \"{name}\" without --yes\n  to confirm: maw scope delete {name} --yes\n"),
            stderr: "delete requires --yes\n".to_owned(),
        };
    }
    match scope_delete(name) {
        Ok(true) => CliOutput { code: 0, stdout: format!("deleted scope \"{name}\"\n"), stderr: String::new() },
        Ok(false) => CliOutput { code: 0, stdout: format!("no-op: scope \"{name}\" not present\n"), stderr: String::new() },
        Err(error) => scope_error(&error),
    }
}

#[allow(dead_code)]
fn scope_help() -> &'static str {
    "usage: maw scope <list|create|show|delete> [...]
  list                                                    — list all scopes used by the live ACL gate
  create   <name> --members <a,b,c> [--lead <m>] [--ttl <iso>]
                                                          — create new scope (refuses overwrite)
  show     <name>                                         — print one scope's JSON
  delete   <name> [--yes]                                 — remove scope file (confirms unless --yes)

storage: <CONFIG_DIR>/scopes/<name>.json (one file per scope)
trust:   <STATE_DIR>/scope-trust.json (symmetric sender/target approvals)

ACL is live for peer sends: same-scope and trusted sender/target pairs deliver;
cross-scope untrusted peer sends queue for human approval via `maw inbox pending`.
Use `maw hey|send --approve` for one human-approved replay, and
`--approve --trust` to add a symmetric scope trust pair before delivery.
If scope/trust files are corrupt, peer send fails open with a loud stderr warning."
}

#[allow(dead_code)]
fn scope_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

#[allow(dead_code)]
fn validate_scope_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("invalid scope name \"\" (must match ^[a-z0-9][a-z0-9_-]{0,63}$)".to_owned());
    };
    if name.len() > 64 || !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(format!("invalid scope name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-')) {
        return Err(format!("invalid scope name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    Ok(())
}

#[allow(dead_code)]
fn scope_create(name: &str, members: Vec<String>, lead: Option<String>, ttl: Option<String>) -> Result<NativeScope, String> {
    validate_scope_name(name)?;
    if members.is_empty() {
        return Err(format!("scope \"{name}\" must have at least one member"));
    }
    if members.iter().any(String::is_empty) {
        return Err(format!("scope \"{name}\" has an empty/invalid member entry"));
    }
    if let Some(lead) = &lead {
        if !members.contains(lead) {
            return Err(format!("scope \"{name}\" lead \"{lead}\" is not in members"));
        }
    }
    std::fs::create_dir_all(scopes_dir()).map_err(|error| format!("scope: create scopes dir: {error}"))?;
    let path = scope_path(name);
    if path.exists() {
        return Err(format!("scope \"{name}\" already exists at {} — delete it first to recreate", path.display()));
    }
    let scope = NativeScope { name: name.to_owned(), members, lead, created: now_iso_utc(), ttl: ttl.or(Some(String::new())).filter(|value| !value.is_empty()) };
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&scope).map_err(|error| format!("scope: render {name}: {error}"))? + "\n";
    std::fs::write(&tmp, json).map_err(|error| format!("scope: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|error| format!("scope: rename {}: {error}", path.display()))?;
    Ok(scope)
}

#[allow(dead_code)]
fn scope_delete(name: &str) -> Result<bool, String> {
    validate_scope_name(name)?;
    let path = scope_path(name);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|error| format!("scope: delete {}: {error}", path.display()))?;
    Ok(true)
}

#[allow(dead_code)]
fn scope_list() -> Result<Vec<NativeScope>, String> {
    std::fs::create_dir_all(scopes_dir()).map_err(|error| format!("scope: create scopes dir: {error}"))?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(scopes_dir()).map_err(|error| format!("scope: read scopes dir: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(scope) = serde_json::from_str::<NativeScope>(&text) {
                out.push(scope);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[allow(dead_code)]
fn load_scope(name: &str) -> Result<Option<NativeScope>, String> {
    let path = scope_path(name);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("scope: read {}: {error}", path.display()))?;
    Ok(serde_json::from_str(&text).ok())
}

#[allow(dead_code)]
fn format_scope_list(rows: &[NativeScope]) -> String {
    if rows.is_empty() {
        return "no scopes".to_owned();
    }
    let header = ["name", "members", "lead", "ttl", "created"];
    let data = rows.iter().map(|row| {
        [row.name.clone(), row.members.join(","), row.lead.clone().unwrap_or_else(|| "-".to_owned()), row.ttl.clone().unwrap_or_else(|| "-".to_owned()), row.created.clone()]
    }).collect::<Vec<_>>();
    let widths = (0..header.len()).map(|idx| {
        data.iter().map(|cols| cols[idx].len()).chain([header[idx].len()]).max().unwrap_or(0)
    }).collect::<Vec<_>>();
    let format_row = |cols: &[String]| -> String {
        cols.iter().enumerate().map(|(idx, col)| format!("{col:<width$}", width = widths[idx])).collect::<Vec<_>>().join("  ")
    };
    let mut lines = Vec::new();
    lines.push(format_row(&header.map(str::to_owned)));
    lines.push(format_row(&widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>()));
    lines.extend(data.iter().map(|cols| format_row(cols)));
    lines.join("\n")
}

#[allow(dead_code)]
fn scopes_dir() -> std::path::PathBuf { active_config_dir().join("scopes") }
#[allow(dead_code)]
fn scope_path(name: &str) -> std::path::PathBuf { scopes_dir().join(format!("{name}.json")) }

#[allow(dead_code)]
fn run_find_command(argv: &[String]) -> CliOutput {
    let Some(keyword) = argv.first().filter(|arg| !arg.starts_with('-')) else {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "usage: maw find <keyword> [--oracle <name>]\n".to_owned(),
        };
    };
    let oracle = flag_value(argv, "--oracle");
    CliOutput {
        code: 0,
        stdout: find_render(keyword, oracle.as_deref()),
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_lines)]
#[allow(dead_code)]
fn find_render(keyword: &str, oracle_filter: Option<&str>) -> String {
    let kw = keyword.to_lowercase();
    let repos_root = ghq_root().join("github.com");
    let fleet = load_native_fleet();
    let mut out = format!("\n  \x1b[36m🔍 Searching\x1b[0m — \"{keyword}\"\n\n");

    let mut oracle_matches = Vec::<(String, String)>::new();
    if let Ok(orgs) = std::fs::read_dir(&repos_root) {
        for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
            let org_name = org.file_name().to_string_lossy().into_owned();
            if let Ok(repos) = std::fs::read_dir(org.path()) {
                for repo in repos.flatten().filter(|entry| entry.path().is_dir()) {
                    let repo_name_raw = repo.file_name().to_string_lossy().into_owned();
                    let repo_name = repo_name_raw
                        .strip_suffix("-oracle")
                        .unwrap_or(&repo_name_raw)
                        .to_owned();
                    let slug = format!("{org_name}/{repo_name_raw}");
                    if oracle_filter.is_some_and(|wanted| wanted != repo_name) {
                        continue;
                    }
                    if repo_name.to_lowercase().contains(&kw) || slug.to_lowercase().contains(&kw) {
                        oracle_matches.push((repo_name, slug));
                    }
                }
            }
        }
    }
    oracle_matches.sort();

    let mut fleet_matches = Vec::<String>::new();
    for session in &fleet {
        let oracle_name = session
            .name
            .trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-');
        if oracle_filter.is_some_and(|wanted| wanted != oracle_name) {
            continue;
        }
        if session.name.to_lowercase().contains(&kw) || oracle_name.to_lowercase().contains(&kw) {
            fleet_matches.push(format!("session {}", session.name));
        }
        for window in &session.windows {
            if window.name.to_lowercase().contains(&kw) || window.repo.to_lowercase().contains(&kw) {
                let detail = if window.repo.is_empty() {
                    format!("window {}", window.name)
                } else {
                    format!("window {} ({})", window.name, window.repo)
                };
                fleet_matches.push(detail);
            }
        }
        for peer in &session.sync_peers {
            if peer.to_lowercase().contains(&kw) {
                fleet_matches.push(format!("sync_peer {peer}"));
            }
        }
        for repo in &session.project_repos {
            if repo.to_lowercase().contains(&kw) {
                fleet_matches.push(format!("project_repo {repo}"));
            }
        }
    }

    let mut targets = Vec::<(String, std::path::PathBuf)>::new();
    for session in &fleet {
        let oracle_name = session
            .name
            .trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-')
            .to_owned();
        if oracle_filter.is_some_and(|wanted| wanted != oracle_name) {
            continue;
        }
        let Some(window) = session.windows.first() else {
            continue;
        };
        if window.repo.is_empty() {
            continue;
        }
        let psi = repos_root.join(&window.repo).join("ψ").join("memory");
        if psi.exists() {
            targets.push((oracle_name, psi));
        }
    }
    let local_psi = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("ψ")
        .join("memory");
    if local_psi.exists() && !targets.iter().any(|(_, path)| *path == local_psi) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("local"));
        let name = cwd
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("local")
            .trim_end_matches("-oracle")
            .to_owned();
        targets.push((name, local_psi));
    }

    let mut code_results = Vec::<(String, String, String)>::new();
    for (name, root) in &targets {
        collect_find_code_matches(name, root, &kw, &mut code_results);
    }

    let total = oracle_matches.len() + fleet_matches.len() + code_results.len();
    if total == 0 {
        let _ = write!(
            out,
            "  \x1b[90m○\x1b[0m no matches found across {} oracle(s)\n\n",
            targets.len()
        );
        return out;
    }
    if !oracle_matches.is_empty() {
        out.push_str("  \x1b[36m── Oracles ──\x1b[0m\n");
        for (name, slug) in &oracle_matches {
            let _ = writeln!(out, "    \x1b[1m{name}\x1b[0m \x1b[90m({slug})\x1b[0m");
        }
        out.push('\n');
    }
    if !fleet_matches.is_empty() {
        out.push_str("  \x1b[36m── Fleet ──\x1b[0m\n");
        for detail in &fleet_matches {
            let _ = writeln!(out, "    \x1b[90m{detail}\x1b[0m");
        }
        out.push('\n');
    }
    if !code_results.is_empty() {
        out.push_str("  \x1b[36m── Code ──\x1b[0m\n");
        let mut grouped: BTreeMap<&str, Vec<&(String, String, String)>> = BTreeMap::new();
        for result in &code_results {
            grouped.entry(&result.0).or_default().push(result);
        }
        for (oracle, matches) in grouped {
            let _ = writeln!(
                out,
                "    \x1b[36m{oracle}\x1b[0m ({} match{})",
                matches.len(),
                if matches.len() == 1 { "" } else { "es" }
            );
            for (_, file, line) in matches.iter().take(10) {
                let _ = writeln!(out, "      \x1b[90m{file}\x1b[0m");
                if !line.is_empty() {
                    let truncated = line.chars().take(120).collect::<String>();
                    let _ = writeln!(out, "        {truncated}");
                }
            }
            if matches.len() > 10 {
                let _ = writeln!(out, "      \x1b[90m... and {} more\x1b[0m", matches.len() - 10);
            }
        }
        out.push('\n');
    }
    let mut parts = Vec::new();
    if !oracle_matches.is_empty() {
        parts.push(format!("{} oracle(s)", oracle_matches.len()));
    }
    if !fleet_matches.is_empty() {
        parts.push(format!("{} fleet", fleet_matches.len()));
    }
    if !code_results.is_empty() {
        parts.push(format!("{} code", code_results.len()));
    }
    let _ = write!(out, "  \x1b[32m{total} match(es)\x1b[0m — {}\n\n", parts.join(", "));
    out
}

#[allow(dead_code)]
fn collect_find_code_matches(name: &str, root: &std::path::Path, kw: &str, out: &mut Vec<(String, String, String)>) {
    let Ok(entries) = std::fs::read_dir(root) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_find_code_matches(name, &path, kw, out);
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue; };
        let Some(line) = text.lines().find(|line| line.to_lowercase().contains(kw)) else { continue; };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        out.push((name.to_owned(), rel, line.trim().to_owned()));
    }
}

fn active_config_dir() -> std::path::PathBuf {
    let env = current_xdg_env();
    maw_config_dir(&env)
}

fn current_xdg_env() -> MawXdgEnv {
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
        "MAW_TEST_MODE",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

fn ghq_root() -> std::path::PathBuf {
    ghq_root_resolve(std::env::var_os("GHQ_ROOT"), ghq_root_from_git_config, std::env::var_os("HOME"))
}

// Resolution order mirrors ghq itself: $GHQ_ROOT env → `git config ghq.root` → ~/Code.
// Without the git-config step, `maw wake <name>` only works in shells that happen to
// export GHQ_ROOT (e.g. inside a direnv tree) while `ghq` resolves everywhere (#134).
fn ghq_root_resolve(
    env_root: Option<std::ffi::OsString>,
    git_config_root: impl FnOnce() -> Option<String>,
    home: Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    if let Some(value) = env_root {
        return ghq_root_strip_host(std::path::PathBuf::from(value));
    }
    if let Some(value) = git_config_root() {
        let expanded = ghq_root_expand_tilde(value.trim(), home.as_deref());
        if !expanded.as_os_str().is_empty() {
            return ghq_root_strip_host(expanded);
        }
    }
    home.map_or_else(|| std::path::PathBuf::from(".").join("Code"), |home| std::path::PathBuf::from(home).join("Code"))
}

fn ghq_root_from_git_config() -> Option<String> {
    let output = std::process::Command::new("git").args(["config", "--get", "ghq.root"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn ghq_root_expand_tilde(value: &str, home: Option<&std::ffi::OsStr>) -> std::path::PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(value)
}

fn ghq_root_strip_host(mut path: std::path::PathBuf) -> std::path::PathBuf {
    if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("github.com") {
        path.pop();
    }
    path
}

fn fleet_read_dirs_for_env(env: &MawXdgEnv) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        maw_state_path(env, &["fleet"]),
        env.home_dir().join(".maw").join("fleet"),
        maw_config_path(env, &["fleet"]),
    ];
    dirs.dedup();
    dirs
}

fn fleet_disabled_count_for_env(env: &MawXdgEnv) -> usize {
    fleet_read_dirs_for_env(env)
        .into_iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| fleet_is_disabled_file(path))
        .count()
}

fn fleet_is_json_file(path: &std::path::Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")
}

fn fleet_is_disabled_file(path: &std::path::Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str) == Some("disabled")
}

fn fleet_disabled_path(path: &std::path::Path) -> std::path::PathBuf {
    let file = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("fleet.json");
    path.with_file_name(format!("{file}.disabled"))
}

fn fleet_load_entries() -> Vec<NativeFleetEntry> {
    fleet_load_entries_for_env(&current_xdg_env())
}

fn fleet_load_entries_result(label: &str) -> Result<Vec<NativeFleetEntry>, String> {
    fleet_load_entries_result_for_env(&current_xdg_env(), label)
}

fn fleet_load_entries_for_env(env: &MawXdgEnv) -> Vec<NativeFleetEntry> {
    fleet_load_entries_impl(fleet_read_dirs_for_env(env), false, "fleet").unwrap_or_default()
}

fn fleet_load_entries_result_for_env(env: &MawXdgEnv, label: &str) -> Result<Vec<NativeFleetEntry>, String> {
    fleet_load_entries_impl(fleet_read_dirs_for_env(env), true, label)
}

fn fleet_load_entries_impl(dirs: Vec<std::path::PathBuf>, strict: bool, label: &str) -> Result<Vec<NativeFleetEntry>, String> {
    let mut entries = Vec::new();
    let mut seen_prior_dirs = BTreeSet::new();
    for dir in dirs {
        fleet_migrate_squad_files(&dir, strict, label)?;
        let mut seen_this_dir = BTreeSet::new();
        let mut files = match std::fs::read_dir(&dir) {
            Ok(values) => values
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| fleet_is_json_file(path))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) if strict => return Err(format!("{label}: read {}: {error}", dir.display())),
            Err(_) => Vec::new(),
        };
        if let Ok(groups) = std::fs::read_dir(dir.join("squads")) {
            files.extend(groups.flatten().map(|entry| entry.path().join("squad.json")).filter(|path| path.is_file()));
        }
        files.sort();
        for path in files {
            let Some(entry) = fleet_parse_entry(&path, strict, label)? else { continue; };
            if !seen_prior_dirs.contains(&entry.session.name) {
                seen_this_dir.insert(entry.session.name.clone());
                entries.push(entry);
            }
        }
        seen_prior_dirs.extend(seen_this_dir);
    }
    Ok(entries)
}

fn fleet_migrate_squad_files(dir: &std::path::Path, strict: bool, label: &str) -> Result<(), String> {
    fleet_migrate_legacy_group_dir(dir, label)?;
    fleet_migrate_legacy_squad_file_names(dir, label)?;
    let files = match std::fs::read_dir(dir) {
        Ok(values) => values.flatten().map(|entry| entry.path()).filter(|path| fleet_is_json_file(path)).collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) if strict => return Err(format!("{label}: read {}: {error}", dir.display())),
        Err(_) => return Ok(()),
    };
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if !value.get("members").is_some_and(serde_json::Value::is_array) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str) else { continue };
        let target_dir = dir.join("squads").join(stem);
        let target = target_dir.join("squad.json");
        fleet_move_squad_file(&path, &target_dir, &target, label)?;
    }
    Ok(())
}

fn fleet_migrate_legacy_group_dir(dir: &std::path::Path, label: &str) -> Result<(), String> {
    let legacy = dir.join("groups");
    let Ok(entries) = std::fs::read_dir(&legacy) else { return Ok(()) };
    for entry in entries.flatten() {
        let source = entry.path().join("group.json");
        if !source.is_file() {
            continue;
        }
        let target_dir = dir.join("squads").join(entry.file_name());
        let target = target_dir.join("squad.json");
        fleet_move_squad_file(&source, &target_dir, &target, label)?;
        let _ = std::fs::remove_dir(entry.path());
    }
    let _ = std::fs::remove_dir(&legacy);
    Ok(())
}

fn fleet_migrate_legacy_squad_file_names(dir: &std::path::Path, label: &str) -> Result<(), String> {
    let squads = dir.join("squads");
    let Ok(entries) = std::fs::read_dir(&squads) else { return Ok(()) };
    for entry in entries.flatten() {
        let source = entry.path().join("group.json");
        if !source.is_file() {
            continue;
        }
        let target = entry.path().join("squad.json");
        fleet_move_squad_file(&source, &entry.path(), &target, label)?;
    }
    Ok(())
}

fn fleet_move_squad_file(
    source: &std::path::Path,
    target_dir: &std::path::Path,
    target: &std::path::Path,
    label: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(target_dir).map_err(|error| format!("{label}: create {}: {error}", target_dir.display()))?;
    if target.exists() {
        std::fs::remove_file(source).map_err(|error| format!("{label}: remove duplicate {}: {error}", source.display()))?;
    } else {
        std::fs::rename(source, target).map_err(|error| format!("{label}: move {} to {}: {error}", source.display(), target.display()))?;
    }
    fleet_rewrite_squad_name(target, label)?;
    eprintln!("fleet: migrated squad roster {} -> {}", source.display(), target.display());
    Ok(())
}

fn fleet_rewrite_squad_name(path: &std::path::Path, label: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{label}: read {}: {error}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("{label}: parse {}: {error}", path.display()))?;
    let Some(object) = value.as_object_mut() else { return Ok(()) };
    if let Some(group_name) = object.remove("groupName") {
        object.entry("squadName").or_insert(group_name);
        let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
        std::fs::write(path, format!("{body}\n")).map_err(|error| format!("{label}: write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn fleet_parse_entry(path: &std::path::Path, strict: bool, label: &str) -> Result<Option<NativeFleetEntry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if strict => return Err(format!("{label}: read {}: {error}", path.display())),
        Err(_) => return Ok(None),
    };
    let mut session = match serde_json::from_str::<NativeFleetSession>(&text) {
        Ok(session) => session,
        Err(error) if strict => return Err(format!("{label}: parse {}: {error}", path.display())),
        Err(_) => return Ok(None),
    };
    if session.name.is_empty() {
        return Ok(None);
    }
    native_fleet_apply_role_markers(&mut session);
    let file = if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("squad.json") {
        path.parent()
            .and_then(std::path::Path::file_name)
            .and_then(std::ffi::OsStr::to_str)
            .map_or_else(|| "squad".to_owned(), str::to_owned)
    } else {
        path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_owned()
    };
    Ok(Some(NativeFleetEntry { file, path: path.to_path_buf(), session }))
}

fn load_native_fleet() -> Vec<NativeFleetSession> {
    fleet_load_entries().into_iter().map(|entry| entry.session).collect()
}

fn native_fleet_apply_role_markers(session: &mut NativeFleetSession) {
    for window in &mut session.windows {
        if window.kind.is_none() {
            window.kind = native_repo_marker_kind_for_slug(&window.repo);
        }
    }
}

fn native_repo_kind_from_role(value: &str) -> Option<NativeRepoKind> {
    match value.trim() {
        "oracle" => Some(NativeRepoKind::Oracle),
        "project" => Some(NativeRepoKind::Project),
        _ => None,
    }
}

fn native_repo_marker_kind(path: &std::path::Path) -> Option<NativeRepoKind> {
    let text = std::fs::read_to_string(path.join(".maw/role")).ok()?;
    native_repo_kind_from_role(&text)
}

fn native_repo_marker_kind_for_slug(repo: &str) -> Option<NativeRepoKind> {
    let path = native_fleet_repo_path(repo)?;
    native_repo_marker_kind(&path)
}

fn native_fleet_repo_path(repo: &str) -> Option<std::path::PathBuf> {
    let repo = repo.trim();
    if repo.is_empty() {
        return None;
    }
    let repo = repo.strip_prefix("github.com/").unwrap_or(repo);
    Some(ghq_root().join("github.com").join(repo))
}

fn native_repo_kind_for_path(path: &std::path::Path) -> Option<NativeRepoKind> {
    let slugs = native_repo_slugs_for_path(path);
    for entry in fleet_load_entries() {
        for window in &entry.session.windows {
            if window.kind.is_some() && native_fleet_window_matches_slugs(window, &slugs) {
                return window.kind;
            }
        }
    }
    native_repo_marker_kind(path)
}

fn native_repo_slugs_for_path(path: &std::path::Path) -> BTreeSet<String> {
    let mut slugs = BTreeSet::new();
    let root = ghq_root().join("github.com");
    if let Ok(rel) = path.strip_prefix(root) {
        let parts = rel.components().take(2).map(|part| part.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>();
        if parts.len() == 2 {
            slugs.insert(format!("{}/{}", parts[0], parts[1]));
            slugs.insert(format!("github.com/{}/{}", parts[0], parts[1]));
        }
    }
    let mut github_parts = Vec::new();
    for component in path.components() {
        let value = component.as_os_str().to_string_lossy();
        if github_parts.is_empty() {
            if value == "github.com" {
                github_parts.push(String::new());
            }
            continue;
        }
        github_parts.push(value.to_string());
        if github_parts.len() == 3 {
            slugs.insert(format!("{}/{}", github_parts[1], github_parts[2]));
            slugs.insert(format!("github.com/{}/{}", github_parts[1], github_parts[2]));
            break;
        }
    }
    slugs
}

fn native_fleet_window_matches_slugs(window: &NativeFleetWindow, slugs: &BTreeSet<String>) -> bool {
    let repo = window.repo.trim();
    !repo.is_empty()
        && (slugs.contains(repo) || repo.strip_prefix("github.com/").is_some_and(|stripped| slugs.contains(stripped)))
}

fn native_repo_path_is_oracle(path: &std::path::Path, fallback_name: &str) -> bool {
    match native_repo_kind_for_path(path) {
        Some(NativeRepoKind::Oracle) => true,
        Some(NativeRepoKind::Project) => false,
        None => fallback_name.ends_with("-oracle"),
    }
}

fn native_fleet_window_is_oracle(window: &NativeFleetWindow) -> bool {
    match window.kind {
        Some(NativeRepoKind::Oracle) => true,
        Some(NativeRepoKind::Project) => false,
        None => window.name.ends_with("-oracle"),
    }
}

fn native_fleet_window_oracle_name(window: &NativeFleetWindow) -> Option<String> {
    if !native_fleet_window_is_oracle(window) {
        return None;
    }
    let source = if window.name.trim().is_empty() {
        window.repo.rsplit('/').next().unwrap_or_default()
    } else {
        window.name.trim()
    };
    let without_slot = source
        .split_once('-')
        .filter(|(prefix, suffix)| {
            !prefix.is_empty() && !suffix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())
        })
        .map_or(source, |(_, suffix)| suffix);
    let name = without_slot.strip_suffix("-oracle").unwrap_or(without_slot).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod native_fleet_loader_tests {
    use super::*;

    fn fleet_loader_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("maw-rs-fleet-loader-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn fleet_loader_write(path: &std::path::Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, text).expect("write");
    }

    fn fleet_loader_env<F>(root: &std::path::Path, test: F)
    where
        F: FnOnce(),
    {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _maw_xdg = EnvVarRestore::capture("MAW_XDG");
        let _maw_state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _maw_config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _xdg_config = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _xdg_state = EnvVarRestore::capture("XDG_STATE_HOME");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_XDG");
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_STATE_HOME");
        test();
    }

    #[test]
    fn native_fleet_loader_merges_state_legacy_and_config_with_precedence() {
        let root = fleet_loader_temp_root("merge");
        fleet_loader_write(
            &root.join("state/fleet/01-alpha.json"),
            r#"{"name":"01-alpha","windows":[{"name":"state","repo":"org/state"}],"syncPeers":["beta"],"projectRepos":["org/project"],"skipCommand":true,"buddedFrom":"root"}"#,
        );
        fleet_loader_write(
            &root.join("home/.maw/fleet/01-alpha.json"),
            r#"{"name":"01-alpha","windows":[{"name":"legacy","repo":"org/legacy"}]}"#,
        );
        fleet_loader_write(
            &root.join("home/.maw/fleet/02-beta.json"),
            r#"{"name":"02-beta","windows":[{"name":"beta","repo":"org/beta"}]}"#,
        );
        fleet_loader_write(
            &root.join("config/fleet/03-gamma.json"),
            r#"{"name":"03-gamma","squadName":"fallback","windows":[{"name":"gamma","repo":"org/gamma"}]}"#,
        );
        fleet_loader_write(&root.join("state/fleet/99-disabled.json.disabled"), "{}");

        fleet_loader_env(&root, || {
            let entries = fleet_load_entries();
            let names = entries.iter().map(|entry| entry.session.name.as_str()).collect::<Vec<_>>();
            assert_eq!(names, vec!["01-alpha", "02-beta", "03-gamma"]);
            assert_eq!(entries[0].session.windows[0].repo, "org/state");
            assert_eq!(entries[0].session.sync_peers, vec!["beta"]);
            assert_eq!(entries[0].session.project_repos, vec!["org/project"]);
            assert_eq!(entries[0].session.skip_command, Some(serde_json::json!(true)));
            assert_eq!(entries[0].session.budded_from.as_deref(), Some("root"));
            assert_eq!(entries[2].session.squad_name, "fallback");
            assert_eq!(fleet_disabled_count_for_env(&current_xdg_env()), 1);
        });
    }

    #[test]
    fn native_fleet_loader_preserves_same_dir_duplicates_for_ambiguity_checks() {
        let root = fleet_loader_temp_root("duplicates");
        fleet_loader_write(
            &root.join("config/fleet/01-alpha-a.json"),
            r#"{"name":"01-alpha","windows":[{"name":"a","repo":"org/a"}]}"#,
        );
        fleet_loader_write(
            &root.join("config/fleet/01-alpha-b.json"),
            r#"{"name":"01-alpha","windows":[{"name":"b","repo":"org/b"}]}"#,
        );

        fleet_loader_env(&root, || {
            let entries = fleet_load_entries();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].session.name, "01-alpha");
            assert_eq!(entries[1].session.name, "01-alpha");
        });
    }

    #[test]
    fn native_fleet_loader_parses_kind_and_role_marker_with_json_precedence() {
        let root = fleet_loader_temp_root("kind");
        fleet_loader_write(
            &root.join("state/fleet/01-kind.json"),
            r#"{"name":"01-kind","windows":[{"name":"plain","repo":"acme/plain","kind":"oracle"},{"name":"legacy","repo":"acme/legacy"},{"name":"marker","repo":"acme/marker"},{"name":"override","repo":"acme/override","kind":"project"}]}"#,
        );
        fleet_loader_write(&root.join("ghq/github.com/acme/marker/.maw/role"), "oracle\n");
        fleet_loader_write(&root.join("ghq/github.com/acme/override/.maw/role"), "oracle\n");

        fleet_loader_env(&root, || {
            let entries = fleet_load_entries();
            let windows = &entries[0].session.windows;
            assert_eq!(windows[0].kind, Some(NativeRepoKind::Oracle));
            assert_eq!(windows[1].kind, None);
            assert_eq!(windows[2].kind, Some(NativeRepoKind::Oracle));
            assert_eq!(windows[3].kind, Some(NativeRepoKind::Project));
        });
    }

    #[test]
    fn native_fleet_window_oracle_name_strips_numeric_session_prefix() {
        let window = NativeFleetWindow {
            name: "47-3e-infra-oracle".to_owned(),
            repo: "laris-co/3e-infra-oracle".to_owned(),
            kind: Some(NativeRepoKind::Oracle),
        };
        assert_eq!(native_fleet_window_oracle_name(&window), Some("3e-infra".to_owned()));

        let fallback = NativeFleetWindow {
            name: "3e-infra".to_owned(),
            repo: "laris-co/3e-infra-oracle".to_owned(),
            kind: Some(NativeRepoKind::Oracle),
        };
        assert_eq!(native_fleet_window_oracle_name(&fallback), Some("3e-infra".to_owned()));
    }
}

fn flag_value(argv: &[String], flag: &str) -> Option<String> {
    argv.windows(2).find_map(|window| (window[0] == flag).then(|| window[1].clone()))
}

fn now_iso_utc() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("{seconds}")
}

#[cfg(test)]
mod scopefind_ghq_root_tests {
    use super::{ghq_root_expand_tilde, ghq_root_resolve};
    use std::ffi::OsString;
    use std::path::PathBuf;

    // Shorthand for the injected env-lookup shape; always-Some is the point —
    // call sites read `os("/opt/Code")` against a `None` alternative.
    #[allow(clippy::unnecessary_wraps)]
    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn env_var_wins_over_git_config() {
        let root = ghq_root_resolve(os("/opt/Code"), || Some("/elsewhere".to_owned()), os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/opt/Code"));
    }

    #[test]
    fn env_var_github_host_suffix_is_stripped() {
        let root = ghq_root_resolve(os("/opt/Code/github.com"), || None, os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/opt/Code"));
    }

    #[test]
    fn git_config_root_used_when_env_unset() {
        let root = ghq_root_resolve(None, || Some("/opt/Code\n".to_owned()), os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/opt/Code"));
    }

    #[test]
    fn git_config_root_expands_tilde() {
        let root = ghq_root_resolve(None, || Some("~/ghq".to_owned()), os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/Users/nat/ghq"));
    }

    #[test]
    fn git_config_github_host_suffix_is_stripped() {
        let root = ghq_root_resolve(None, || Some("/opt/Code/github.com".to_owned()), os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/opt/Code"));
    }

    #[test]
    fn empty_git_config_falls_back_to_home_code() {
        let root = ghq_root_resolve(None, || Some("   ".to_owned()), os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/Users/nat/Code"));
    }

    #[test]
    fn no_sources_falls_back_to_home_code() {
        let root = ghq_root_resolve(None, || None, os("/Users/nat"));
        assert_eq!(root, PathBuf::from("/Users/nat/Code"));
    }

    #[test]
    fn no_home_falls_back_to_relative_code() {
        let root = ghq_root_resolve(None, || None, None);
        assert_eq!(root, PathBuf::from(".").join("Code"));
    }

    #[test]
    fn tilde_without_home_stays_literal() {
        assert_eq!(ghq_root_expand_tilde("~/ghq", None), PathBuf::from("~/ghq"));
    }
}
