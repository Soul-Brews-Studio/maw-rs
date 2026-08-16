const DISPATCH_63: &[DispatcherEntry] = &[
    DispatcherEntry { command: "oracle", handler: Handler::Sync(run_oracle_command) },
    DispatcherEntry { command: "oracles", handler: Handler::Sync(run_oracle_command) },
];

const ORACLE_USAGE: &str = "usage: maw oracle [ls|scan|search <query>|recruit <fleet> <oracle>|prune|register <name>|set-nickname <name> <nickname>|get-nickname <name>|about <name>]";

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

#[derive(Debug, Clone)]
struct OracleFleetEntry { session: NativeFleetSession }

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct OracleListOptions { json: bool, awake: bool, org: Option<String>, path: bool, scan: bool, stale: bool, sort_by: Option<String> }

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct OracleScanOptions { json: bool, stale: bool, verbose: bool, all: bool, quiet: bool, force: bool }

#[derive(Default)]
struct OracleTmux { runner: maw_tmux::CommandTmuxRunner }

fn run_oracle_command(argv: &[String]) -> CliOutput {
    if argv.first().is_some_and(|arg| arg == "scan") {
        let opts = match oracle_parse_scan_options(argv, 1) {
            Ok(opts) => opts,
            Err(message) => return CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
        };
        return match oracle_scan_with_progress(&opts) {
            Ok((stdout, stderr)) => CliOutput { code: 0, stdout, stderr },
            Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
        };
    }
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
        "recruit" => oracle_recruit_run(&argv[1..]),
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

fn oracle_parse_scan_options(argv: &[String], start: usize) -> Result<OracleScanOptions, String> {
    let mut opts = OracleScanOptions::default();
    let mut i = start;
    while i < argv.len() {
        match argv[i].as_str() {
            "--json" => opts.json = true,
            "--stale" => opts.stale = true,
            "--all" => opts.all = true,
            "--verbose" | "-v" => opts.verbose = true,
            "--quiet" | "-q" => opts.quiet = true,
            "--force" => opts.force = true,
            "--local" => {},
            "--remote" => return Err("oracle scan: --remote is not available in native offline mode".to_owned()),
            value if value.starts_with('-') => return Err(format!("oracle: unknown argument {value}")),
            _ => return Err(ORACLE_USAGE.to_owned()),
        }
        i += 1;
    }
    Ok(opts)
}

fn oracle_list(opts: &OracleListOptions, tmux: &mut OracleTmux) -> Result<String, String> {
    let registry = if opts.scan {
        let previous = oracle_read_registry();
        let registry = oracle_merge_scan_registry(oracle_scan_registry()?, &previous);
        oracle_guard_scan_write(&registry, &previous, false)?;
        oracle_write_registry(&registry)?;
        registry
    } else {
        oracle_read_registry()
    };
    let awake = tmux.oracle_awake_oracles();
    let mut entries = oracle_enriched_entries(&registry, &awake);
    if opts.awake { entries.retain(|entry| awake.contains_key(&entry.name)); }
    if let Some(org) = &opts.org { entries.retain(|entry| entry.org == *org); }
    oracle_sort_entries(&mut entries, &awake, opts.sort_by.as_deref());
    if opts.json { return oracle_json_list(&registry, &entries, &awake); }
    Ok(oracle_text_list(&registry, &entries, &awake, opts.path))
}

fn oracle_scan(opts: &OracleScanOptions) -> Result<String, String> {
    if opts.stale { return Ok(oracle_stale(opts.json)); }
    let previous = oracle_read_registry();
    let registry = oracle_merge_scan_registry(oracle_scan_registry()?, &previous);
    oracle_guard_scan_write(&registry, &previous, opts.force)?;
    oracle_write_registry(&registry)?;
    if opts.json { return serde_json::to_string_pretty(&registry).map(|value| format!("{value}\n")).map_err(|error| error.to_string()); }
    Ok(format!("\n  \x1b[32m✓\x1b[0m {} oracles locally (cache written)\n\n", registry.oracles.len()))
}

fn oracle_scan_with_progress(opts: &OracleScanOptions) -> Result<(String, String), String> {
    if opts.stale {
        return Ok((oracle_stale(opts.json), String::new()));
    }
    let emit_progress = !opts.json && !opts.quiet && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let previous = oracle_read_registry();
    let (registry, mut stderr) = oracle_scan_registry_with_progress(opts.verbose, emit_progress)?;
    let registry = oracle_merge_scan_registry(registry, &previous);
    oracle_guard_scan_write(&registry, &previous, opts.force)?;
    oracle_write_registry(&registry)?;
    if opts.json { return serde_json::to_string_pretty(&registry).map(|value| (format!("{value}\n"), stderr)).map_err(|error| error.to_string()); }
    if opts.all && emit_progress {
        stderr.push_str("  remote phase: GitHub API scan requested via --all (native offline mode)\n");
    }
    Ok((format!("\n  \x1b[32m✓\x1b[0m {} oracles locally (cache written)\n\n", registry.oracles.len()), stderr))
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

fn oracle_scan_registry() -> Result<OracleRegistry, String> {
    oracle_scan_registry_with_progress(false, false).map(|(registry, _progress)| registry)
}

fn oracle_scan_registry_with_progress(verbose: bool, show_progress: bool) -> Result<(OracleRegistry, String), String> {
    let mut entries = Vec::<OracleEntry>::new();
    let mut progress = String::new();
    let root = ghq_root();
    let repos_root = root.join("github.com");
    let orgs = std::fs::read_dir(&repos_root).map_err(|error| format!("oracle scan: failed to read ghq github root {}: {error}", repos_root.display()))?;
    let mut candidates = Vec::<(String, std::path::PathBuf)>::new();
    for org_entry in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let org = org_entry.file_name().to_string_lossy().to_string();
        let Ok(repos) = std::fs::read_dir(org_entry.path()) else { continue; };
        for repo_entry in repos.flatten().filter(|entry| entry.path().is_dir()) {
            candidates.push((org.clone(), repo_entry.path()));
        }
    }
    let total = candidates.len();
    for (index, (org, path)) in candidates.into_iter().enumerate() {
        if let Some(entry) = oracle_entry_from_repo(&org, &path) {
            if show_progress {
                if verbose {
                    let mut flags = Vec::<&str>::new();
                    if entry.has_psi { flags.push("ψ/"); }
                    if entry.has_fleet_config { flags.push("fleet-config"); }
                    let flags = if flags.is_empty() { "(none)".to_owned() } else { flags.join(",") };
                    let _ = writeln!(progress, "  scanning {} ({}/{})  path={}  flags={}", entry.repo, index + 1, total, entry.local_path, flags);
                } else {
                    let _ = writeln!(progress, "  scanning {} ({}/{})", entry.repo, index + 1, total);
                }
            }
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((OracleRegistry { schema: 1, local_scanned_at: oracle_now_string(), ghq_root: root.display().to_string(), oracles: entries, retired: Vec::new() }, progress))
}

// `oracles.json` is the only place `federation_node` is ever stored — locate/roster/recruit
// read it, nothing else writes it — so an entry the scan drops is gone for good (#732).
// #773 made a *missing* ghq root a hard error; an existing-but-wrong root (a bare `~/Code`
// that happens to hold a `github.com/`) still walked straight into the write with 0 hits.
// A scan that collapses the cached fleet is far more often a mis-resolved root than a real
// wipe, so refuse the write and make the caller say --force.
const ORACLE_SCAN_COLLAPSE_FLOOR: usize = 4;

fn oracle_scan_collapses_registry(scanned: &OracleRegistry, previous: &OracleRegistry) -> bool {
    if previous.oracles.is_empty() { return false; }
    // Only rows that were on disk are this scan's responsibility: entries registered from
    // tmux/fleet carry an empty local_path and a filesystem scan never re-finds them, so
    // counting them would fire the guard on healthy registries.
    //
    // This denominator MUST be computed before the zero-scan branch. It used to sit after
    // it, so a registry made entirely of never-cloned rows (which `oracle register` writes
    // for an awake-but-not-cloned oracle, and `oracle stale` exists to report) hard-errored
    // on a CORRECT, genuinely empty ghq root — the tool refusing while its own `oracle stale`
    // called the same rows DEAD. Nothing recoverable was at stake there.
    let on_disk = previous.oracles.iter().filter(|entry| !entry.local_path.is_empty()).count();
    if on_disk == 0 { return false; }
    if scanned.oracles.is_empty() { return true; }
    on_disk >= ORACLE_SCAN_COLLAPSE_FLOOR && scanned.oracles.len() * 2 < on_disk
}

fn oracle_guard_scan_write(scanned: &OracleRegistry, previous: &OracleRegistry, force: bool) -> Result<(), String> {
    if force || !oracle_scan_collapses_registry(scanned, previous) { return Ok(()); }
    let (cached, found) = (previous.oracles.len(), scanned.oracles.len());
    Err(format!(
        "oracle scan: refusing to overwrite {cached} cached {} — the scan of {} found {found} {}.\n  That usually means the ghq root is wrong, not that the fleet shrank (federation_node lives only in this cache).\n  Re-run as `maw oracle scan --force` to overwrite anyway, or `maw oracle ls` to read the cache untouched.",
        oracle_plural_oracles(cached),
        if scanned.ghq_root.is_empty() { "(unknown root)" } else { &scanned.ghq_root },
        oracle_plural_oracles(found),
    ))
}

fn oracle_plural_oracles(count: usize) -> &'static str { if count == 1 { "oracle" } else { "oracles" } }

fn oracle_merge_scan_registry(mut scanned: OracleRegistry, previous: &OracleRegistry) -> OracleRegistry {
    let previous_by_name = previous.oracles.iter().map(|entry| (entry.name.as_str(), entry)).collect::<BTreeMap<_, _>>();
    for entry in &mut scanned.oracles {
        if let Some(previous) = previous_by_name.get(entry.name.as_str()) {
            entry.budded_from = previous.budded_from.clone();
            entry.budded_at = previous.budded_at.clone();
            entry.federation_node = previous.federation_node.clone();
            entry.nickname = previous.nickname.clone();
        }
    }
    scanned.retired.clone_from(&previous.retired);
    scanned
}

fn oracle_entry_from_repo(org: &str, path: &std::path::Path) -> Option<OracleEntry> {
    let repo = path.file_name()?.to_string_lossy().to_string();
    if !oracle_repo_path_is_oracle(path, &repo) { return None; }
    let name = repo.strip_suffix("-oracle").unwrap_or(&repo).to_owned();
    Some(OracleEntry { org: org.to_owned(), repo, name, local_path: path.display().to_string(), has_psi: oracle_repo_has_local_signal(path), has_fleet_config: oracle_repo_has_fleet_config(path), detected_at: oracle_now_string(), ..OracleEntry::default() })
}

fn oracle_entries_from_fleet(fleet: &OracleFleetEntry) -> Vec<OracleEntry> {
    let mut out = Vec::new();
    for window in &fleet.session.windows {
        let Some(name) = oracle_fleet_window_oracle_name(window) else { continue; };
        let (org, repo) = oracle_split_repo(&window.repo, &name);
        let path = native_fleet_repo_path(&window.repo).filter(|path| path.is_dir());
        let local_path = path.as_ref().map_or_else(String::new, |path| path.display().to_string());
        let has_psi = path.as_ref().is_some_and(|path| oracle_repo_has_local_signal(path));
        out.push(OracleEntry { org, repo, name, local_path, has_psi, has_fleet_config: true, detected_at: oracle_now_string(), ..OracleEntry::default() });
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
    fleet_load_entries()
        .into_iter()
        .filter(fleet_entry_is_session)
        .map(|entry| OracleFleetEntry { session: entry.session })
        .collect()
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
fn oracle_split_repo(repo: &str, name: &str) -> (String, String) {
    let repo = repo.strip_prefix("github.com/").unwrap_or(repo);
    repo.split_once('/').map_or(("(unknown)".to_owned(), format!("{name}-oracle")), |(org, repo)| (org.to_owned(), repo.to_owned()))
}
fn oracle_entry_haystack(entry: &OracleEntry) -> String { format!("{} {} {} {} {}", entry.name, entry.org, entry.repo, entry.budded_from.clone().unwrap_or_default(), entry.nickname.clone().unwrap_or_default()).to_lowercase() }
fn oracle_repo_has_fleet_config(path: &std::path::Path) -> bool { let repo = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default(); oracle_fleet_entries().iter().any(|fleet| fleet.session.windows.iter().any(|window| window.repo.ends_with(repo))) }
fn oracle_repo_has_local_signal(path: &std::path::Path) -> bool { path.join("ψ").exists() }
fn oracle_repo_path_is_oracle(path: &std::path::Path, repo: &str) -> bool { native_repo_path_is_oracle(path, repo) || repo.ends_with("-oracle") && oracle_repo_has_local_signal(path) }
fn oracle_fleet_window_oracle_name(window: &NativeFleetWindow) -> Option<String> {
    native_fleet_window_oracle_name(window).or_else(|| oracle_project_window_oracle_name(window))
}
fn oracle_project_window_oracle_name(window: &NativeFleetWindow) -> Option<String> {
    if window.kind != Some(NativeRepoKind::Project) { return None; }
    let repo_stem = oracle_repo_stem(&window.repo)?;
    let path = native_fleet_repo_path(&window.repo)?;
    if !oracle_repo_has_local_signal(&path) { return None; }
    let source = if window.name.trim().is_empty() { repo_stem.as_str() } else { window.name.trim() };
    let name = oracle_normalize_window_oracle_name(source)?;
    (name == repo_stem).then_some(name)
}
fn oracle_repo_stem(repo: &str) -> Option<String> {
    repo.strip_prefix("github.com/")
        .unwrap_or(repo)
        .rsplit('/')
        .next()
        .and_then(|repo| repo.strip_suffix("-oracle"))
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}
fn oracle_normalize_window_oracle_name(source: &str) -> Option<String> {
    let without_slot = source
        .split_once('-')
        .filter(|(prefix, suffix)| !prefix.is_empty() && !suffix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit()))
        .map_or(source, |(_, suffix)| suffix);
    let name = without_slot.strip_suffix("-oracle").unwrap_or(without_slot).trim();
    (!name.is_empty()).then(|| name.to_owned())
}
fn oracle_registry_path() -> std::path::PathBuf { maw_cache_path(&current_xdg_env(), &["oracles.json"]) }
fn oracle_legacy_registry_path() -> std::path::PathBuf { maw_config_path(&current_xdg_env(), &["oracles.json"]) }
fn oracle_now_string() -> String { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs()).to_string() }
fn oracle_schema_one() -> u8 { 1 }

#[cfg(test)]
mod oracle_tests {
    use super::*;
    fn oracle_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
    fn oracle_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("maw-rs-oracle-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }
    fn oracle_test_env(root: &std::path::Path, ghq_root: &std::path::Path) -> Vec<EnvVarRestore> {
        let guards = ["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "GHQ_ROOT"].map(EnvVarRestore::capture).into_iter().collect();
        std::env::set_var("HOME", root.join("home"));
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CACHE_DIR", root.join("cache"));
        std::env::set_var("GHQ_ROOT", ghq_root);
        guards
    }
    fn oracle_test_entry(name: &str) -> OracleEntry {
        OracleEntry { org: "acme".to_owned(), repo: format!("{name}-oracle"), name: name.to_owned(), local_path: format!("/old/{name}-oracle"), has_psi: true, detected_at: "1".to_owned(), ..OracleEntry::default() }
    }
    fn oracle_test_retired() -> Vec<serde_json::Value> {
        ["argus-codex", "beacon", "cipher-codex-2", "cipher-codex-3", "cipher-codex-4", "cipher-codex-5", "sentinel", "ui-designer", "ux-designer"]
            .into_iter()
            .map(|name| serde_json::json!({"name":name}))
            .collect()
    }
    #[test]
    fn oracle_parser_blocks_leading_dash_values() { assert!(oracle_parse_list_options(&oracle_strings(&["ls", "--org", "-bad"]), 1).is_err()); assert!(oracle_parse_scan_options(&oracle_strings(&["scan", "--remote"]), 1).is_err()); }
    #[test]
    fn oracle_scan_parser_supports_verbose_and_all() {
        let opts = oracle_parse_scan_options(&oracle_strings(&["scan", "--verbose", "--all", "--quiet"]), 1).expect("scan opts");
        assert!(opts.verbose);
        assert!(opts.all);
        assert!(opts.quiet);
    }

    #[test]
    fn oracle_scan_progress_reports_lines() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _cache = EnvVarRestore::capture("MAW_CACHE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = oracle_temp_root("scan-progress");
        let org = root.join("ghq/github.com/acme");
        std::fs::create_dir_all(org.join("neo-oracle/ψ")).expect("neo-oracle");
        std::fs::create_dir_all(org.join("sol-oracle")).expect("sol-oracle");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::write(
            root.join("config/fleet/01-neo.json"),
            r#"{"name":"01-neo","windows":[{"name":"neo-oracle","repo":"acme/neo-oracle","kind":"oracle"}]}"#,
        )
        .expect("fleet entry");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CACHE_DIR", root.join("cache"));
        std::env::set_var("GHQ_ROOT", root.join("ghq"));

        let (_registry, progress) = oracle_scan_registry_with_progress(false, true).expect("scan");
        assert!(progress.contains("scanning neo-oracle"));
        assert!(progress.contains("scanning sol-oracle"));

        let (_, verbose_progress) = oracle_scan_registry_with_progress(true, true).expect("verbose scan");
        assert!(verbose_progress.contains("path="));
        assert!(verbose_progress.contains("flags="));
        assert!(verbose_progress.contains("ψ/"));
    }
    #[test]
    fn oracle_registry_roundtrip_defaults() { let value = serde_json::from_str::<OracleRegistry>(r#"{"oracles":[{"org":"o","repo":"neo-oracle","name":"neo"}]}"#).unwrap(); assert_eq!(value.schema, 1); assert_eq!(value.oracles[0].name, "neo"); }
    #[test]
    fn oracle_format_row_marks_fleet_and_psi() { let entry = OracleEntry { org: "org".to_owned(), repo: "neo-oracle".to_owned(), name: "neo".to_owned(), has_psi: true, has_fleet_config: true, ..OracleEntry::default() }; assert!(oracle_format_row(&entry, true, false).contains("fleet+awake")); }

    #[test]
    fn oracle_scan_failure_keeps_existing_registry_byte_identical() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-failure");
        let _env = oracle_test_env(&root, &root.join("missing-ghq"));
        let registry = OracleRegistry { schema: 1, local_scanned_at: "old".to_owned(), ghq_root: "old-ghq".to_owned(), oracles: vec![oracle_test_entry("neo")], retired: oracle_test_retired() };
        oracle_write_registry(&registry).expect("seed registry");
        let before = std::fs::read_to_string(oracle_registry_path()).expect("registry before");

        let error = oracle_scan(&OracleScanOptions::default()).expect_err("scan should fail");

        assert!(error.contains("failed to read ghq github root"), "{error}");
        assert!(error.contains("missing-ghq/github.com"), "{error}");
        assert_eq!(std::fs::read_to_string(oracle_registry_path()).expect("registry after"), before);
    }

    #[test]
    fn oracle_scan_merges_retired_while_replacing_oracles() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-retired");
        let _env = oracle_test_env(&root, &root.join("ghq"));
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle/ψ")).expect("neo");
        let registry = OracleRegistry {
            schema: 1,
            oracles: vec![oracle_test_entry("neo"), oracle_test_entry("deleted")],
            retired: oracle_test_retired(),
            ..OracleRegistry::default()
        };
        oracle_write_registry(&registry).expect("seed registry");

        oracle_scan(&OracleScanOptions::default()).expect("scan");
        let scanned = oracle_read_registry();

        assert_eq!(scanned.oracles.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), vec!["neo"]);
        assert_eq!(scanned.retired, oracle_test_retired());
    }

    #[test]
    fn oracle_scan_writes_fresh_registry_without_prior_cache() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-fresh");
        let _env = oracle_test_env(&root, &root.join("ghq"));
        std::fs::create_dir_all(root.join("ghq/github.com/acme/fresh-oracle/ψ")).expect("fresh");

        oracle_scan(&OracleScanOptions::default()).expect("fresh scan");
        let scanned = oracle_read_registry();

        assert_eq!(scanned.oracles.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), vec!["fresh"]);
        assert!(scanned.retired.is_empty());
    }

    #[test]
    fn oracle_scan_carries_forward_scan_unknown_fields_for_survivors() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-fields");
        let _env = oracle_test_env(&root, &root.join("ghq"));
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle/ψ")).expect("neo");
        let mut entry = oracle_test_entry("neo");
        entry.budded_from = Some("atlas".to_owned());
        entry.budded_at = Some("2026-08-03T00:00:00Z".to_owned());
        entry.federation_node = Some("edge-a".to_owned());
        entry.nickname = Some("Neo Prime".to_owned());
        oracle_write_registry(&OracleRegistry { schema: 1, oracles: vec![entry], ..OracleRegistry::default() }).expect("seed registry");

        oracle_scan(&OracleScanOptions::default()).expect("scan");
        let entry = oracle_read_registry().oracles.into_iter().find(|entry| entry.name == "neo").expect("neo");

        assert_eq!(entry.budded_from.as_deref(), Some("atlas"));
        assert_eq!(entry.budded_at.as_deref(), Some("2026-08-03T00:00:00Z"));
        assert_eq!(entry.federation_node.as_deref(), Some("edge-a"));
        assert_eq!(entry.nickname.as_deref(), Some("Neo Prime"));
    }

    // #732 — the ghq root exists but is not the fleet's root, so the scan finds nothing.
    // #773 only covers a *missing* root; an existing-but-wrong one still reached the write.
    fn oracle_seed_wrong_root(name: &str) -> (std::path::PathBuf, Vec<EnvVarRestore>, String) {
        let root = oracle_temp_root(name);
        let env = oracle_test_env(&root, &root.join("wrong-root"));
        std::fs::create_dir_all(root.join("wrong-root/github.com")).expect("empty github root");
        let mut neo = oracle_test_entry("neo");
        neo.federation_node = Some("edge-a".to_owned());
        let registry = OracleRegistry { schema: 1, local_scanned_at: "old".to_owned(), ghq_root: "old-ghq".to_owned(), oracles: vec![neo, oracle_test_entry("trinity")], retired: oracle_test_retired() };
        oracle_write_registry(&registry).expect("seed registry");
        let before = std::fs::read_to_string(oracle_registry_path()).expect("registry before");
        (root, env, before)
    }

    // Pins the over-fire the guard's first version had: it short-circuited on an
    // empty scan BEFORE computing the on-disk denominator, so a registry made
    // entirely of never-cloned rows hard-errored on a CORRECT, empty ghq root.
    // `oracle register` writes exactly such rows for an awake-but-not-cloned
    // oracle, and `oracle stale` reports them DEAD — the tool refused to write
    // while its own diagnostic said those rows were already gone. Nothing
    // recoverable was at stake, which is what makes it an over-fire rather than
    // caution.
    #[test]
    fn oracle_scan_guard_ignores_rows_that_were_never_on_disk() {
        let never_cloned = |name: &str| OracleEntry { name: name.to_owned(), ..Default::default() };
        let previous = OracleRegistry {
            oracles: vec![never_cloned("a"), never_cloned("b"), never_cloned("c")],
            ..Default::default()
        };
        let scanned = OracleRegistry::default();
        assert!(
            !oracle_scan_collapses_registry(&scanned, &previous),
            "rows with an empty local_path are not a filesystem scan's responsibility"
        );
        assert!(oracle_guard_scan_write(&scanned, &previous, false).is_ok());
    }

    #[test]
    fn oracle_scan_refuses_to_wipe_registry_when_existing_root_finds_nothing() {
        let _guard = env_test_lock();
        let (_root, _env, before) = oracle_seed_wrong_root("scan-empty-root");

        let error = oracle_scan_with_progress(&OracleScanOptions::default()).expect_err("scan should refuse to wipe");

        assert!(error.contains("found 0 oracles"), "{error}");
        assert!(error.contains("--force"), "{error}");
        assert_eq!(std::fs::read_to_string(oracle_registry_path()).expect("registry after"), before);
        let kept = oracle_read_registry();
        assert_eq!(kept.oracles.len(), 2);
        assert_eq!(kept.oracles.iter().find(|entry| entry.name == "neo").and_then(|entry| entry.federation_node.as_deref()), Some("edge-a"));
    }

    #[test]
    fn oracle_list_scan_refuses_to_wipe_registry_when_existing_root_finds_nothing() {
        let _guard = env_test_lock();
        let (_root, _env, before) = oracle_seed_wrong_root("list-scan-empty-root");

        let opts = OracleListOptions { scan: true, ..OracleListOptions::default() };
        let error = oracle_list(&opts, &mut OracleTmux::default()).expect_err("ls --scan should refuse to wipe");

        assert!(error.contains("found 0 oracles"), "{error}");
        assert_eq!(std::fs::read_to_string(oracle_registry_path()).expect("registry after"), before);
    }

    #[test]
    fn oracle_scan_force_still_overwrites_with_an_empty_result() {
        let _guard = env_test_lock();
        let (_root, _env, _before) = oracle_seed_wrong_root("scan-empty-root-forced");

        oracle_scan_with_progress(&OracleScanOptions { force: true, ..OracleScanOptions::default() }).expect("forced scan");

        assert!(oracle_read_registry().oracles.is_empty());
    }

    #[test]
    fn oracle_scan_refuses_a_majority_drop_but_allows_a_minority_one() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-majority-drop");
        let _env = oracle_test_env(&root, &root.join("ghq"));
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle/ψ")).expect("neo");
        let seed = |names: &[&str]| {
            let oracles = names.iter().map(|name| oracle_test_entry(name)).collect::<Vec<_>>();
            oracle_write_registry(&OracleRegistry { schema: 1, oracles, ..OracleRegistry::default() }).expect("seed registry");
        };

        seed(&["neo", "trinity", "morpheus", "atlas"]);
        let error = oracle_scan_with_progress(&OracleScanOptions::default()).expect_err("4 -> 1 should refuse");
        assert!(error.contains("found 1 oracle"), "{error}");
        assert_eq!(oracle_read_registry().oracles.len(), 4);

        seed(&["neo", "trinity"]);
        oracle_scan_with_progress(&OracleScanOptions::default()).expect("2 -> 1 stays allowed");
        assert_eq!(oracle_read_registry().oracles.len(), 1);
    }

    #[test]
    fn oracle_scan_still_allowed_when_the_lost_rows_were_never_on_disk() {
        let _guard = env_test_lock();
        let root = oracle_temp_root("scan-uncloned-rows");
        let _env = oracle_test_env(&root, &root.join("ghq"));
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle/ψ")).expect("neo");
        // Two rows a filesystem scan can find, four registered from tmux/fleet with no path.
        let mut oracles = vec![oracle_test_entry("neo"), oracle_test_entry("trinity")];
        for name in ["ghost-a", "ghost-b", "ghost-c", "ghost-d"] {
            oracles.push(OracleEntry { local_path: String::new(), ..oracle_test_entry(name) });
        }
        oracle_write_registry(&OracleRegistry { schema: 1, oracles, ..OracleRegistry::default() }).expect("seed registry");

        oracle_scan_with_progress(&OracleScanOptions::default()).expect("6 rows -> 1 on-disk hit stays allowed");

        assert_eq!(oracle_read_registry().oracles.len(), 1);
    }

    #[test]
    fn oracle_scan_parser_records_force() {
        assert!(oracle_parse_scan_options(&oracle_strings(&["scan", "--force"]), 1).expect("scan opts").force);
        assert!(!oracle_parse_scan_options(&oracle_strings(&["scan", "--local"]), 1).expect("scan opts").force);
    }

    #[test]
    fn oracle_repo_scan_uses_declared_kind_before_suffix() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = oracle_temp_root("kind");
        let foo = root.join("ghq/github.com/acme/foo");
        let bar = root.join("ghq/github.com/acme/bar-oracle");
        std::fs::create_dir_all(&foo).expect("foo");
        std::fs::create_dir_all(&bar).expect("bar");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::write(
            root.join("config/fleet/99-kind.json"),
            r#"{"name":"99-kind","windows":[{"name":"foo","repo":"acme/foo","kind":"oracle"},{"name":"bar-oracle","repo":"acme/bar-oracle","kind":"project"}]}"#,
        )
        .expect("fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        assert_eq!(oracle_entry_from_repo("acme", &foo).expect("foo oracle").name, "foo");
        assert!(oracle_entry_from_repo("acme", &bar).is_none());
    }

    #[test]
    fn oracle_fleet_entries_use_declared_kind_before_window_suffix() {
        let fleet = OracleFleetEntry {
            session: NativeFleetSession {
                name: "99-kind".to_owned(),
                windows: vec![
                    NativeFleetWindow { name: "foo".to_owned(), repo: "acme/foo".to_owned(), kind: Some(NativeRepoKind::Oracle) },
                    NativeFleetWindow { name: "bar-oracle".to_owned(), repo: "acme/bar-oracle".to_owned(), kind: Some(NativeRepoKind::Project) },
                ],
                ..NativeFleetSession::default()
            },
        };

        let entries = oracle_entries_from_fleet(&fleet);

        assert_eq!(entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), vec!["foo"]);
    }

    #[test]
    fn oracle_scan_registry_captures_numeric_leading_oracle_name() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _cache = EnvVarRestore::capture("MAW_CACHE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = oracle_temp_root("numeric-scan");
        let repo = root.join("ghq/github.com/laris-co/3e-infra-oracle");
        std::fs::create_dir_all(repo.join("ψ")).expect("repo psi");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::write(
            root.join("config/fleet/47-3e-infra.json"),
            r#"{"auto_registered":true,"created_at":"2026-07-06T09:38:21.000Z","created_by":"maw wake","name":"47-3e-infra","windows":[{"kind":"project","name":"3e-infra","repo":"github.com/laris-co/3e-infra-oracle"}]}"#,
        )
        .expect("write fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CACHE_DIR", root.join("cache"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let (registry, progress) = oracle_scan_registry_with_progress(true, true).expect("scan");
        let entry = registry.oracles.into_iter().find(|entry| entry.name == "3e-infra").expect("3e-infra entry");
        assert_eq!(entry.org, "laris-co");
        assert_eq!(entry.repo, "3e-infra-oracle");
        assert_eq!(entry.local_path, repo.display().to_string());
        assert!(entry.has_psi);
        assert!(entry.has_fleet_config);
        assert!(progress.contains("3e-infra-oracle"), "{progress}");
    }

    #[test]
    fn oracle_register_discovers_numeric_slot_prefixed_fleet_window_name() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _cache = EnvVarRestore::capture("MAW_CACHE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = oracle_temp_root("numeric-register");
        let repo = root.join("ghq/github.com/laris-co/3e-infra-oracle");
        std::fs::create_dir_all(repo.join("ψ")).expect("repo psi");
        let _ = std::fs::create_dir_all(root.join("config/fleet"));
        std::fs::write(
            root.join("config/fleet/47-3e-infra.json"),
            r#"{"auto_registered":true,"created_at":"2026-07-06T09:38:21.000Z","created_by":"maw wake","name":"47-3e-infra","windows":[{"kind":"project","name":"3e-infra","repo":"github.com/laris-co/3e-infra-oracle"}]}"#,
        )
        .expect("write fleet");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CACHE_DIR", root.join("cache"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let mut tmux = OracleTmux::default();
        let output = oracle_register(&oracle_strings(&["register", "3e-infra"]), &mut tmux).expect("register");
        let registry = oracle_read_registry();

        let plain = maw_tmux::strip_tmux_ansi(&output);
        assert!(plain.contains("Registered 3e-infra"), "expected register output to name the discovered oracle, got: {output:?}");
        let entry = registry.oracles.iter().find(|entry| entry.name == "3e-infra").expect("registered entry");
        assert_eq!(entry.org, "laris-co");
        assert_eq!(entry.repo, "3e-infra-oracle");
        assert_eq!(entry.local_path, repo.display().to_string());
        assert!(entry.has_psi);
    }
}
