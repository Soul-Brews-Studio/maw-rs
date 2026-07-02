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
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct NativeFleetSession {
    name: String,
    #[serde(default)]
    windows: Vec<NativeFleetWindow>,
    #[serde(default)]
    sync_peers: Vec<String>,
    #[serde(default)]
    project_repos: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct NativeFleetWindow {
    name: String,
    #[serde(default)]
    repo: String,
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
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

fn ghq_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(|| {
        std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from(".").join("Code"), |home| std::path::PathBuf::from(home).join("Code"))
    }, |value| {
        let mut path = std::path::PathBuf::from(value);
        if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("github.com") { path.pop(); }
        path
    })
}

fn load_native_fleet() -> Vec<NativeFleetSession> {
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else { return Vec::new(); };
    let mut files = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")).collect::<Vec<_>>();
    files.sort();
    files.into_iter().filter_map(|path| std::fs::read_to_string(path).ok()).filter_map(|text| serde_json::from_str(&text).ok()).collect()
}

fn flag_value(argv: &[String], flag: &str) -> Option<String> {
    argv.windows(2).find_map(|window| (window[0] == flag).then(|| window[1].clone()))
}

fn now_iso_utc() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("{seconds}")
}
