const DISPATCH_63: &[DispatcherEntry] = &[
    DispatcherEntry { command: "oracle", handler: Handler::Sync(run_oracle_command) },
    DispatcherEntry { command: "oracles", handler: Handler::Sync(run_oracle_command) },
];

const ORACLE_USAGE: &str = "usage: maw oracle [ls|scan|search <query>|prune|register <name>|set-nickname <name> <nickname>|get-nickname <name>|about <name>]";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
struct OracleEntry {
    org: String,
    repo: String,
    name: String,
    #[serde(default)]
    local_path: String,
    #[serde(default)]
    has_psi: bool,
    #[serde(default)]
    has_fleet_config: bool,
    #[serde(default)]
    budded_from: Option<String>,
    #[serde(default)]
    budded_at: Option<String>,
    #[serde(default)]
    federation_node: Option<String>,
    #[serde(default)]
    detected_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
struct OracleRegistry {
    #[serde(default = "oracle_schema_one")]
    schema: u8,
    #[serde(default)]
    local_scanned_at: String,
    #[serde(default)]
    ghq_root: String,
    #[serde(default)]
    oracles: Vec<OracleEntry>,
    #[serde(default)]
    retired: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct OracleFleetSession { #[serde(default)] windows: Vec<OracleFleetWindow> }

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct OracleFleetWindow { name: String, #[serde(default)] repo: String }

#[derive(Debug, Clone)]
struct OracleFleetEntry { session: OracleFleetSession }

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct OracleListOptions { json: bool, awake: bool, org: Option<String>, path: bool, scan: bool, stale: bool, sort_by: Option<String> }

#[derive(Default)]
struct OracleTmux { runner: maw_tmux::CommandTmuxRunner }

fn run_oracle_command(argv: &[String]) -> CliOutput {
    match oracle_run(argv, &mut OracleTmux::default()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn oracle_run(argv: &[String], tmux: &mut OracleTmux) -> Result<String, String> {
    let sub = argv.first().map_or("ls", String::as_str).to_lowercase();
    match sub.as_str() {
        "--help" | "-h" => Ok(format!("{ORACLE_USAGE}\n")),
        "ls" | "list" => oracle_list(&oracle_parse_list_options(argv, 1)?, tmux),
        "fleet" => { let mut out = "\x1b[33m⚠  maw oracle fleet is deprecated — use \x1b[36mmaw oracle ls\x1b[0m\x1b[33m instead\x1b[0m\n".to_owned(); out.push_str(&oracle_list(&oracle_parse_list_options(argv, 1)?, tmux)?); Ok(out) },
        "scan" => oracle_scan(&oracle_parse_scan_options(argv, 1)?),
        "stale" => Ok(oracle_stale(oracle_parse_json_flag(argv, 1)?)),
        "prune" => oracle_prune(argv, tmux),
        "register" => oracle_register(argv, tmux),
        "search" | "find" => oracle_search(argv, tmux),
        "about" => oracle_about(argv, tmux),
        "set-nickname" | "nickname" => oracle_set_nickname(argv),
        "get-nickname" => oracle_get_nickname(argv),
        value if value.starts_with('-') => Err(format!("oracle: unknown argument {value}")),
        _ => Err(ORACLE_USAGE.to_owned()),
    }
}

fn oracle_parse_list_options(argv: &[String], start: usize) -> Result<OracleListOptions, String> {
    let mut opts = OracleListOptions::default();
    let mut i = start;
    while i < argv.len() {
        match argv[i].as_str() {
            "--json" => opts.json = true,
            "--awake" => opts.awake = true,
            "--scan" => opts.scan = true,
            "--stale" => opts.stale = true,
            "--path" | "-p" => opts.path = true,
            "--org" => { i += 1; let value = oracle_required_value(argv, i, "--org")?; oracle_validate_name(value, "org")?; opts.org = Some(value.clone()); },
            "--sort-by" => { i += 1; let value = oracle_required_value(argv, i, "--sort-by")?; oracle_validate_name(value, "sort")?; opts.sort_by = Some(value.clone()); },
            value if value.starts_with('-') => return Err(format!("oracle: unknown argument {value}")),
            _ => return Err(ORACLE_USAGE.to_owned()),
        }
        i += 1;
    }
    Ok(opts)
}

fn oracle_parse_scan_options(argv: &[String], start: usize) -> Result<OracleListOptions, String> {
    let mut opts = OracleListOptions::default();
    let mut i = start;
    while i < argv.len() {
        match argv[i].as_str() {
            "--json" => opts.json = true,
            "--stale" => opts.stale = true,
            "--force" | "--local" | "--all" | "--verbose" | "-v" | "--quiet" | "-q" => {},
            "--remote" => return Err("oracle scan: --remote is not available in native offline mode".to_owned()),
            value if value.starts_with('-') => return Err(format!("oracle: unknown argument {value}")),
            _ => return Err(ORACLE_USAGE.to_owned()),
        }
        i += 1;
    }
    Ok(opts)
}

fn oracle_list(opts: &OracleListOptions, tmux: &mut OracleTmux) -> Result<String, String> {
    let registry = if opts.scan { oracle_scan_registry() } else { oracle_read_registry() };
    if opts.scan { oracle_write_registry(&registry)?; }
    let awake = tmux.oracle_awake_oracles();
    let mut entries = oracle_enriched_entries(&registry, &awake);
    if opts.awake { entries.retain(|entry| awake.contains_key(&entry.name)); }
    if let Some(org) = &opts.org { entries.retain(|entry| entry.org == *org); }
    oracle_sort_entries(&mut entries, &awake, opts.sort_by.as_deref());
    if opts.json { return oracle_json_list(&registry, &entries, &awake); }
    Ok(oracle_text_list(&registry, &entries, &awake, opts.path))
}

fn oracle_scan(opts: &OracleListOptions) -> Result<String, String> {
    if opts.stale { return Ok(oracle_stale(opts.json)); }
    let registry = oracle_scan_registry();
    oracle_write_registry(&registry)?;
    if opts.json { return serde_json::to_string_pretty(&registry).map(|value| format!("{value}\n")).map_err(|error| error.to_string()); }
    Ok(format!("\n  \x1b[32m✓\x1b[0m {} oracles locally (cache written)\n\n", registry.oracles.len()))
}

fn oracle_stale(json: bool) -> String {
    let registry = oracle_read_registry();
    let stale = registry.oracles.iter().filter(|entry| !entry.has_psi && !entry.has_fleet_config && entry.local_path.is_empty()).cloned().collect::<Vec<_>>();
    if json { return format!("{}\n", serde_json::json!({"schema":1,"count":stale.len(),"oracles":stale})); }
    format!("\n  Stale oracle scan  (DEAD {}  STALE 0)\n\n", stale.len())
}

fn oracle_prune(argv: &[String], tmux: &mut OracleTmux) -> Result<String, String> {
    let mut force = false; let mut json = false;
    for arg in &argv[1..] { match arg.as_str() { "--force" => force = true, "--json" => json = true, "--stale" => {}, value if value.starts_with('-') => return Err(format!("oracle: unknown argument {value}")), _ => return Err(ORACLE_USAGE.to_owned()) } }
    let mut registry = oracle_read_registry();
    let awake = tmux.oracle_awake_oracles();
    let candidates = registry.oracles.iter().filter(|e| !e.has_psi && !e.has_fleet_config && e.budded_from.is_none() && !awake.contains_key(&e.name) && e.federation_node.is_none()).cloned().collect::<Vec<_>>();
    if json { return Ok(format!("{}\n", serde_json::json!({"schema":1,"count":candidates.len(),"dry_run":!force,"candidates":candidates}))); }
    if candidates.is_empty() { return Ok("\n  \x1b[32m✓\x1b[0m No prune candidates — registry is clean.\n\n".to_owned()); }
    if !force { return Ok(oracle_prune_preview(&candidates)); }
    oracle_retire_candidates(&mut registry, &candidates)?;
    Ok(format!("\n  \x1b[32m✓\x1b[0m Retired {} oracle(s) → retired[] in registry.\n\n", candidates.len()))
}

fn oracle_register(argv: &[String], tmux: &mut OracleTmux) -> Result<String, String> {
    let name = argv.get(1).ok_or_else(|| "usage: maw oracle register <name>".to_owned())?;
    oracle_validate_name(name, "name")?;
    let json = oracle_parse_json_flag(argv, 2)?;
    let mut registry = oracle_read_registry();
    if registry.oracles.iter().any(|entry| entry.name == *name) { return Err(format!("oracle '{name}' is already registered")); }
    let entry = oracle_discover_one(name, tmux).ok_or_else(|| format!("oracle '{name}' not found in fleet, tmux, or filesystem — try: maw oracle scan"))?;
    registry.oracles.push(entry.clone());
    oracle_write_registry(&registry)?;
    if json { return Ok(format!("{}\n", serde_json::json!({"schema":1,"registered":entry}))); }
    Ok(format!("\n  \x1b[32m✓\x1b[0m Registered \x1b[36m{name}\x1b[0m\n  Org:     {}\n  Repo:    {}\n\n", entry.org, entry.repo))
}

fn oracle_search(argv: &[String], tmux: &mut OracleTmux) -> Result<String, String> {
    let query = argv.get(1).ok_or_else(|| "usage: maw oracle search <query>".to_owned())?;
    oracle_validate_name(query, "query")?;
    let opts = oracle_parse_list_options(argv, 2)?;
    let registry = oracle_read_registry();
    let awake = tmux.oracle_awake_oracles();
    let mut matched = oracle_enriched_entries(&registry, &awake).into_iter().filter(|entry| oracle_entry_haystack(entry).contains(&query.to_lowercase())).collect::<Vec<_>>();
    if opts.awake { matched.retain(|entry| awake.contains_key(&entry.name)); }
    if let Some(org) = &opts.org { matched.retain(|entry| entry.org == *org); }
    if opts.json { return Ok(format!("{}\n", serde_json::json!({"query":query,"total":matched.len(),"oracles":matched}))); }
    if matched.is_empty() { return Ok(format!("\n  No oracles matching \x1b[36m{query}\x1b[0m\n\n")); }
    let mut out = format!("\n  \x1b[36m{} oracle{} matching \"{query}\"\x1b[0m\n\n", matched.len(), if matched.len() == 1 { "" } else { "s" });
    for entry in matched { out.push_str(&oracle_format_row(&entry, awake.contains_key(&entry.name), false)); }
    out.push('\n'); Ok(out)
}

fn oracle_about(argv: &[String], tmux: &mut OracleTmux) -> Result<String, String> {
    let name = argv.get(1).ok_or_else(|| ORACLE_USAGE.to_owned())?;
    oracle_validate_name(name, "name")?;
    let registry = oracle_read_registry();
    let awake = tmux.oracle_awake_oracles();
    let entry = registry.oracles.iter().find(|entry| entry.name == *name).cloned().or_else(|| oracle_discover_one(name, tmux)).ok_or_else(|| format!("no oracle named '{name}' — try: maw oracle ls"))?;
    let session = awake.get(&entry.name).map_or("(none)", String::as_str);
    Ok(format!("\n  \x1b[36mOracle — {name}\x1b[0m\n\n  Repo:      {}\n  Session:   {session}\n  Fleet:     {}\n\n", if entry.local_path.is_empty() { "(not found)" } else { &entry.local_path }, if entry.has_fleet_config { "configured" } else { "(no config)" }))
}

fn oracle_set_nickname(argv: &[String]) -> Result<String, String> {
    let name = argv.get(1).ok_or_else(|| "usage: maw oracle set-nickname <oracle> \"<nickname>\"".to_owned())?;
    let nickname = argv.get(2).ok_or_else(|| "usage: maw oracle set-nickname <oracle> \"<nickname>\"".to_owned())?;
    oracle_validate_name(name, "name")?; oracle_validate_nickname(nickname)?;
    let json = oracle_parse_json_flag(argv, 3)?;
    let mut registry = oracle_read_registry();
    let entry = registry.oracles.iter_mut().find(|entry| entry.name == *name).ok_or_else(|| format!("oracle '{name}' not found in registry — try: maw oracle scan"))?;
    entry.nickname = if nickname.is_empty() { None } else { Some(nickname.clone()) };
    oracle_write_nickname(&entry.local_path, nickname)?;
    oracle_write_registry(&registry)?;
    if json { return Ok(format!("{}\n", serde_json::json!({"schema":1,"name":name,"nickname": if nickname.is_empty() { None } else { Some(nickname) }}))); }
    Ok(if nickname.is_empty() { format!("  \x1b[32m✓\x1b[0m cleared nickname for \x1b[36m{name}\x1b[0m\n") } else { format!("  \x1b[32m✓\x1b[0m \x1b[36m{name}\x1b[0m nickname set to \x1b[33m{nickname}\x1b[0m\n") })
}

fn oracle_get_nickname(argv: &[String]) -> Result<String, String> {
    let name = argv.get(1).ok_or_else(|| "usage: maw oracle get-nickname <oracle>".to_owned())?;
    oracle_validate_name(name, "name")?;
    let json = oracle_parse_json_flag(argv, 2)?;
    let registry = oracle_read_registry();
    let value = registry.oracles.iter().find(|entry| entry.name == *name).and_then(|entry| entry.nickname.clone().or_else(|| oracle_read_nickname(&entry.local_path)));
    if json { return Ok(format!("{}\n", serde_json::json!({"schema":1,"name":name,"nickname":value}))); }
    value.map_or_else(|| Err(format!("no nickname set for {name}")), |value| Ok(format!("{value}\n")))
}

impl OracleTmux {
    fn oracle_awake_oracles(&mut self) -> BTreeMap<String, String> {
        let args = ["-a".to_owned(), "-F".to_owned(), "#{session_name}|||#{window_name}".to_owned()];
        let Ok(raw) = maw_tmux::TmuxRunner::run(&mut self.runner, "list-windows", &args) else { return BTreeMap::new(); };
        let mut out = BTreeMap::new();
        for line in raw.lines() { let mut parts = line.split("|||"); if let (Some(session), Some(window)) = (parts.next(), parts.next()) { if let Some(name) = window.strip_suffix("-oracle") { out.entry(name.to_owned()).or_insert_with(|| session.to_owned()); } } }
        out
    }
}

fn oracle_enriched_entries(registry: &OracleRegistry, awake: &BTreeMap<String, String>) -> Vec<OracleEntry> {
    let mut by_name = BTreeMap::<String, OracleEntry>::new();
    for entry in &registry.oracles { by_name.insert(entry.name.clone(), entry.clone()); }
    for entry in oracle_fleet_entries().into_iter().flat_map(|fleet| oracle_entries_from_fleet(&fleet)) { by_name.entry(entry.name.clone()).or_insert(entry); }
    for name in awake.keys() { by_name.entry(name.clone()).or_insert_with(|| OracleEntry { org: "(unregistered)".to_owned(), repo: format!("{name}-oracle"), name: name.clone(), detected_at: oracle_now_string(), ..OracleEntry::default() }); }
    by_name.into_values().collect()
}

fn oracle_scan_registry() -> OracleRegistry {
    let mut entries = Vec::<OracleEntry>::new();
    let repos_root = ghq_root().join("github.com");
    let Ok(orgs) = std::fs::read_dir(&repos_root) else { return OracleRegistry { schema: 1, local_scanned_at: oracle_now_string(), ghq_root: ghq_root().display().to_string(), oracles: entries, retired: Vec::new() }; };
    for org_entry in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let org = org_entry.file_name().to_string_lossy().to_string();
        let Ok(repos) = std::fs::read_dir(org_entry.path()) else { continue; };
        for repo_entry in repos.flatten().filter(|entry| entry.path().is_dir()) { if let Some(entry) = oracle_entry_from_repo(&org, &repo_entry.path()) { entries.push(entry); } }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    OracleRegistry { schema: 1, local_scanned_at: oracle_now_string(), ghq_root: ghq_root().display().to_string(), oracles: entries, retired: Vec::new() }
}

fn oracle_entry_from_repo(org: &str, path: &std::path::Path) -> Option<OracleEntry> {
    let repo = path.file_name()?.to_string_lossy().to_string();
    let name = repo.strip_suffix("-oracle").unwrap_or(&repo).to_owned();
    if !repo.ends_with("-oracle") { return None; }
    Some(OracleEntry { org: org.to_owned(), repo, name, local_path: path.display().to_string(), has_psi: path.join("ψ").exists(), has_fleet_config: oracle_repo_has_fleet_config(path), detected_at: oracle_now_string(), ..OracleEntry::default() })
}

fn oracle_entries_from_fleet(fleet: &OracleFleetEntry) -> Vec<OracleEntry> {
    let mut out = Vec::new();
    for window in &fleet.session.windows {
        let Some(name) = oracle_name_from_window(&window.name) else { continue; };
        let (org, repo) = oracle_split_repo(&window.repo, &name);
        out.push(OracleEntry { org, repo, name, has_fleet_config: true, detected_at: oracle_now_string(), ..OracleEntry::default() });
    }
    out
}

fn oracle_discover_one(name: &str, tmux: &mut OracleTmux) -> Option<OracleEntry> {
    oracle_fleet_entries().iter().flat_map(oracle_entries_from_fleet).find(|entry| entry.name == name).or_else(|| oracle_find_filesystem(name)).or_else(|| tmux.oracle_awake_oracles().contains_key(name).then(|| OracleEntry { org: "(unregistered)".to_owned(), repo: format!("{name}-oracle"), name: name.to_owned(), detected_at: oracle_now_string(), ..OracleEntry::default() }))
}

fn oracle_find_filesystem(name: &str) -> Option<OracleEntry> {
    let repos_root = ghq_root().join("github.com");
    let Ok(orgs) = std::fs::read_dir(repos_root) else { return None; };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) { let org_name = org.file_name().to_string_lossy().to_string(); let path = org.path().join(format!("{name}-oracle")); if path.is_dir() { return oracle_entry_from_repo(&org_name, &path); } }
    None
}

fn oracle_fleet_entries() -> Vec<OracleFleetEntry> {
    let Ok(entries) = std::fs::read_dir(active_config_dir().join("fleet")) else { return Vec::new(); };
    let mut files = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")).collect::<Vec<_>>();
    files.sort();
    files.iter().filter_map(oracle_parse_fleet).collect()
}

fn oracle_parse_fleet(path: &std::path::PathBuf) -> Option<OracleFleetEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let session = serde_json::from_str::<OracleFleetSession>(&text).ok()?;
    Some(OracleFleetEntry { session })
}

fn oracle_text_list(registry: &OracleRegistry, entries: &[OracleEntry], awake: &BTreeMap<String, String>, show_path: bool) -> String {
    let mut out = format!("\n  \x1b[36mOracle Fleet\x1b[0m  ({}/{} awake)\n  cache: {}\n\n", entries.iter().filter(|entry| awake.contains_key(&entry.name)).count(), entries.len(), if registry.local_scanned_at.is_empty() { "?" } else { &registry.local_scanned_at });
    for entry in entries { out.push_str(&oracle_format_row(entry, awake.contains_key(&entry.name), show_path)); }
    out.push('\n'); out
}

fn oracle_format_row(entry: &OracleEntry, awake: bool, show_path: bool) -> String {
    let source = if entry.has_fleet_config && awake { "fleet+awake" } else if entry.has_fleet_config { "fleet      " } else if awake { "awake      " } else { "fs         " };
    let psi = if entry.has_psi { "oracle (ψ/)" } else if entry.local_path.is_empty() { "not cloned" } else { "oracle (?)" };
    let nick = entry.nickname.as_ref().map_or(String::new(), |value| format!(" · {value}"));
    let path = if show_path && !entry.local_path.is_empty() { format!(" · {}", entry.local_path) } else { String::new() };
    format!("  {source}  {}  {}  {psi}{nick}{path}\n", entry.name, entry.org)
}

fn oracle_json_list(registry: &OracleRegistry, entries: &[OracleEntry], awake: &BTreeMap<String, String>) -> Result<String, String> {
    let oracles = entries.iter().map(|entry| { let mut value = serde_json::to_value(entry).unwrap_or_default(); value["awake"] = serde_json::Value::Bool(awake.contains_key(&entry.name)); value["session"] = awake.get(&entry.name).map_or(serde_json::Value::Null, |s| serde_json::Value::String(s.clone())); value }).collect::<Vec<_>>();
    Ok(format!("{}\n", serde_json::to_string_pretty(&serde_json::json!({"cache_scanned_at":registry.local_scanned_at,"total":entries.len(),"awake":entries.iter().filter(|entry| awake.contains_key(&entry.name)).count(),"oracles":oracles})).map_err(|error| error.to_string())?))
}

fn oracle_sort_entries(entries: &mut [OracleEntry], awake: &BTreeMap<String, String>, sort_by: Option<&str>) {
    if sort_by == Some("born") { entries.sort_by(|a, b| b.detected_at.cmp(&a.detected_at).then_with(|| a.name.cmp(&b.name))); } else { entries.sort_by(|a, b| a.org.cmp(&b.org).then_with(|| awake.contains_key(&b.name).cmp(&awake.contains_key(&a.name))).then_with(|| a.name.cmp(&b.name))); }
}

fn oracle_read_registry() -> OracleRegistry {
    let path = oracle_registry_path();
    let raw = std::fs::read_to_string(path).or_else(|_| std::fs::read_to_string(oracle_legacy_registry_path())).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

fn oracle_write_registry(registry: &OracleRegistry) -> Result<(), String> {
    let path = oracle_registry_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let text = serde_json::to_string_pretty(registry).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| error.to_string())
}

fn oracle_retire_candidates(registry: &mut OracleRegistry, candidates: &[OracleEntry]) -> Result<(), String> {
    let names = candidates.iter().map(|entry| entry.name.clone()).collect::<BTreeSet<_>>();
    for entry in candidates { registry.retired.push(serde_json::json!({"name":entry.name,"retired_at":oracle_now_string(),"retired_reasons":["empty lineage","no tmux","no federation"]})); }
    registry.oracles.retain(|entry| !names.contains(&entry.name));
    oracle_write_registry(registry)
}

fn oracle_prune_preview(candidates: &[OracleEntry]) -> String {
    let mut out = format!("\n  \x1b[36mPrune candidates\x1b[0m ({})  \x1b[90m[dry-run — use --force to retire]\x1b[0m\n\n", candidates.len());
    for entry in candidates { let _ = writeln!(out, "          {:24} \x1b[90mempty lineage, no tmux, no federation\x1b[0m", entry.name); }
    out.push_str("\n  Run with \x1b[36m--force\x1b[0m to retire these entries (moves to retired[] — reversible).\n\n"); out
}

fn oracle_write_nickname(repo_path: &str, nickname: &str) -> Result<(), String> {
    if repo_path.is_empty() { return Err("oracle has no local path (not cloned) — clone it before setting a nickname".to_owned()); }
    let path = std::path::Path::new(repo_path).join("ψ/nickname");
    if nickname.is_empty() { let _ = std::fs::remove_file(path); return Ok(()); }
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    std::fs::write(path, format!("{nickname}\n")).map_err(|error| error.to_string())
}

fn oracle_read_nickname(repo_path: &str) -> Option<String> {
    let value = std::fs::read_to_string(std::path::Path::new(repo_path).join("ψ/nickname")).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn oracle_parse_json_flag(argv: &[String], start: usize) -> Result<bool, String> {
    let mut json = false;
    for arg in &argv[start..] { match arg.as_str() { "--json" => json = true, value if value.starts_with('-') => return Err(format!("oracle: unknown argument {value}")), _ => return Err(ORACLE_USAGE.to_owned()) } }
    Ok(json)
}

fn oracle_required_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a String, String> { argv.get(index).filter(|value| !value.starts_with('-')).ok_or_else(|| format!("oracle: {flag} requires a value")) }
fn oracle_validate_name(value: &str, label: &str) -> Result<(), String> { if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') { Err(format!("oracle: invalid {label} '{value}'")) } else { Ok(()) } }
fn oracle_validate_nickname(value: &str) -> Result<(), String> { if value.chars().any(|ch| ch == '\n' || ch == '\r') { Err("oracle: nickname must be one line".to_owned()) } else { Ok(()) } }
fn oracle_name_from_window(window: &str) -> Option<String> { window.strip_suffix("-oracle").map(str::to_owned) }
fn oracle_split_repo(repo: &str, name: &str) -> (String, String) { repo.split_once('/').map_or(("(unknown)".to_owned(), format!("{name}-oracle")), |(org, repo)| (org.to_owned(), repo.to_owned())) }
fn oracle_entry_haystack(entry: &OracleEntry) -> String { format!("{} {} {} {} {}", entry.name, entry.org, entry.repo, entry.budded_from.clone().unwrap_or_default(), entry.nickname.clone().unwrap_or_default()).to_lowercase() }
fn oracle_repo_has_fleet_config(path: &std::path::Path) -> bool { let repo = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default(); oracle_fleet_entries().iter().any(|fleet| fleet.session.windows.iter().any(|window| window.repo.ends_with(repo))) }
fn oracle_registry_path() -> std::path::PathBuf { maw_cache_path(&current_xdg_env(), &["oracles.json"]) }
fn oracle_legacy_registry_path() -> std::path::PathBuf { maw_config_path(&current_xdg_env(), &["oracles.json"]) }
fn oracle_now_string() -> String { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs()).to_string() }
fn oracle_schema_one() -> u8 { 1 }

#[cfg(test)]
mod oracle_tests {
    use super::*;
    fn oracle_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
    #[test]
    fn oracle_parser_blocks_leading_dash_values() { assert!(oracle_parse_list_options(&oracle_strings(&["ls", "--org", "-bad"]), 1).is_err()); assert!(oracle_parse_scan_options(&oracle_strings(&["scan", "--remote"]), 1).is_err()); }
    #[test]
    fn oracle_registry_roundtrip_defaults() { let value = serde_json::from_str::<OracleRegistry>(r#"{"oracles":[{"org":"o","repo":"neo-oracle","name":"neo"}]}"#).unwrap(); assert_eq!(value.schema, 1); assert_eq!(value.oracles[0].name, "neo"); }
    #[test]
    fn oracle_format_row_marks_fleet_and_psi() { let entry = OracleEntry { org: "org".to_owned(), repo: "neo-oracle".to_owned(), name: "neo".to_owned(), has_psi: true, has_fleet_config: true, ..OracleEntry::default() }; assert!(oracle_format_row(&entry, true, false).contains("fleet+awake")); }
}
