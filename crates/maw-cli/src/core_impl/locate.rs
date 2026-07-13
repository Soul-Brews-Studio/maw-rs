const DISPATCH_41: &[DispatcherEntry] = &[DispatcherEntry {
    command: "locate",
    handler: Handler::Sync(run_locate_command),
}];

const LOCATE_USAGE: &str = "usage: maw locate <oracle> [--path | --json]\n  e.g. maw locate mawjs";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocateOptions {
    path: bool,
    json: bool,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LocateResult {
    name: String,
    session: String,
    handle: String,
    repo_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site_source: Option<String>,
    has_psi: bool,
    session_name: Option<String>,
    window_count: usize,
    fleet_config_path: Option<String>,
    federation_node: Option<String>,
    in_agents_config: bool,
    federation: Vec<LocateFederationHit>,
    manifest_entry: Option<LocateManifestEntry>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LocateFederationHit {
    alias: String,
    node: Option<String>,
    url: Option<String>,
    session_name: String,
    window_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LocateManifestEntry {
    name: String,
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_psi: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_fleet_config: Option<bool>,
    is_live: bool,
}

#[derive(Debug, Clone, Default)]
struct LocateConfig {
    node: Option<String>,
    agents: HashMap<String, String>,
    sessions: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct LocateFleetEntry {
    file: String,
    path: String,
    session: NativeFleetSession,
    window_sites: HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LocateRegistryCache {
    schema: u64,
    #[serde(default)]
    oracles: Vec<LocateOracleCacheEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LocateOracleCacheEntry {
    org: String,
    repo: String,
    name: String,
    site: Option<String>,
    pages: Option<String>,
    local_path: String,
    has_psi: bool,
    has_fleet_config: bool,
    federation_node: Option<String>,
}

fn run_locate_command(argv: &[String]) -> CliOutput {
    let mut tmux = TmuxClient::local();
    run_locate_command_with_sessions(argv, &tmux.list_all())
}

fn run_locate_command_with_sessions(argv: &[String], sessions: &[TmuxSession]) -> CliOutput {
    match locate_parse_args(argv) {
        Ok((oracle, opts)) => match locate_picker_target(&oracle, &opts, sessions)
            .and_then(|target| locate_cmd_with_sessions(&target, &opts, sessions).map_err(|message| CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }))
        {
            Ok(stdout) => CliOutput {
                code: 0,
                stdout,
                stderr: String::new(),
            },
            Err(output) => output,
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn locate_parse_args(argv: &[String]) -> Result<(String, LocateOptions), String> {
    let mut opts = LocateOptions {
        path: false,
        json: false,
    };
    let mut oracle: Option<String> = None;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(LOCATE_USAGE.to_owned()),
            "--path" | "-p" => opts.path = true,
            "--json" => opts.json = true,
            value if value.starts_with('-') => return Err(LOCATE_USAGE.to_owned()),
            value => {
                if oracle.replace(value.to_owned()).is_some() {
                    return Err(LOCATE_USAGE.to_owned());
                }
            }
        }
    }
    let Some(oracle) = oracle else {
        return Err(LOCATE_USAGE.to_owned());
    };
    locate_validate_name(&oracle)?;
    Ok((oracle, opts))
}

fn locate_cmd_with_sessions(
    oracle: &str,
    opts: &LocateOptions,
    sessions: &[TmuxSession],
) -> Result<String, String> {
    let info = locate_gather_info(oracle, !opts.path, sessions)?;

    if info.repo_path.is_none()
        && info.session_name.is_none()
        && info.fleet_config_path.is_none()
        && info.federation.is_empty()
        && info.manifest_entry.is_none()
    {
        return Err(format!("no oracle named '{oracle}' — try: maw oracle ls"));
    }

    if opts.json {
        return serde_json::to_string_pretty(&info)
            .map(|json| format!("{json}\n"))
            .map_err(|error| format!("locate: failed to render json: {error}"));
    }

    if opts.path {
        if let Some(path) = info.repo_path {
            return Ok(format!("{path}\n"));
        }
        return Err(format!(
            "no repo path for '{oracle}' (session: {}, fleet: {})",
            info.session_name.as_deref().unwrap_or("none"),
            if info.fleet_config_path.is_some() { "yes" } else { "no" }
        ));
    }

    Ok(locate_render_text(oracle, &info))
}

fn locate_picker_target(target: &str, opts: &LocateOptions, sessions: &[TmuxSession]) -> Result<String, CliOutput> {
    match typed_picker_plan(target, &locate_typed_candidates(sessions), locate_kind_priority, locate_picker_row) {
        TypedPickerPlan::Target(target) => Ok(target),
        TypedPickerPlan::Pick { context, rows } => picker_choose_target("locate", target, context, &rows, opts.json),
    }
}

fn locate_typed_candidates(sessions: &[TmuxSession]) -> Vec<maw_matcher::ResolveTypedCandidate> {
    let alive = sessions.iter().map(|session| session.name.clone()).collect::<BTreeSet<_>>();
    let mut candidates = local_resolver_candidates(&alive);
    candidates.retain(|candidate| candidate.kind != maw_matcher::ResolveCandidateKind::FleetSquad && candidate.kind != maw_matcher::ResolveCandidateKind::Peer);
    candidates.extend(sessions.iter().flat_map(|session| session.windows.iter().map(|window| maw_matcher::ResolveTypedCandidate {
        kind: maw_matcher::ResolveCandidateKind::Window, name: window.name.clone(), aliases: Vec::new(),
    })));
    for entry in locate_load_manifest() {
        let aliases = [entry.session, entry.window, entry.repo].into_iter().flatten().collect::<Vec<_>>();
        if let Some(candidate) = candidates.iter_mut().find(|candidate| candidate.kind == maw_matcher::ResolveCandidateKind::Oracle && candidate.name.eq_ignore_ascii_case(&entry.name)) {
            candidate.aliases.extend(aliases);
        } else {
            candidates.push(maw_matcher::ResolveTypedCandidate { kind: maw_matcher::ResolveCandidateKind::Oracle, name: entry.name, aliases });
        }
    }
    candidates
}

fn locate_kind_priority(kind: maw_matcher::ResolveCandidateKind) -> u8 {
    match kind {
        maw_matcher::ResolveCandidateKind::Oracle => 0,
        maw_matcher::ResolveCandidateKind::SleepingRegistry => 1,
        maw_matcher::ResolveCandidateKind::Repo => 2,
        maw_matcher::ResolveCandidateKind::LiveSession | maw_matcher::ResolveCandidateKind::Window => 3,
        _ => 4,
    }
}

fn locate_picker_row(matched: maw_matcher::ResolveMatch) -> PickerRow {
    PickerRow { action: format!("maw locate {}", matched.candidate.name), detail: None, matched }
}

fn locate_gather_info(
    oracle: &str,
    scan_federation: bool,
    sessions: &[TmuxSession],
) -> Result<LocateResult, String> {
    locate_validate_name(oracle)?;
    let aliases = locate_enrichment_names(oracle);
    let repo_path = aliases.iter().find_map(|alias| locate_find_oracle_repo_path(alias));
    let (session_name, window_count) = aliases
        .iter()
        .find_map(|alias| locate_resolve_session(alias, sessions))
        .map_or((None, 0), |session| (Some(session.name.clone()), session.windows.len()));
    let fleet_config_path = aliases
        .iter()
        .find_map(|alias| locate_find_fleet_config_path(alias, session_name.as_deref()));
    let has_psi = repo_path
        .as_deref()
        .is_some_and(|path| std::path::Path::new(path).join("ψ").exists());
    let manifest_entry = aliases
        .iter()
        .find_map(|alias| locate_lookup_manifest_entry(alias));
    let config = locate_load_config();
    let in_agents_config = aliases.iter().any(|alias| config.agents.contains_key(alias.as_str()));
    let federation_node = if in_agents_config {
        aliases
            .iter()
            .find_map(|alias| config.agents.get(alias.as_str()).cloned())
    } else if session_name.is_some() {
        Some(config.node.unwrap_or_else(|| "local".to_owned()))
    } else {
        manifest_entry
            .as_ref()
            .and_then(|entry| entry.node.clone())
            .or(config.node)
    };
    let federation = if scan_federation {
        locate_find_federation_hits(oracle)
    } else {
        Vec::new()
    };
    let result_repo_path = repo_path.or_else(|| manifest_entry.as_ref().and_then(|entry| entry.local_path.clone()));
    let (site, site_source) = locate_resolve_site(manifest_entry.as_ref(), result_repo_path.as_deref());

    Ok(LocateResult {
        name: aliases
            .last()
            .cloned()
            .unwrap_or_else(|| oracle.to_owned()),
        session: session_name
            .clone()
            .or_else(|| manifest_entry.as_ref().and_then(|entry| entry.session.clone()))
            .unwrap_or_else(|| oracle.to_owned()),
        handle: aliases
            .last()
            .cloned()
            .unwrap_or_else(|| oracle.to_owned()),
        repo_path: result_repo_path,
        site,
        site_source,
        has_psi: if has_psi {
            true
        } else {
            manifest_entry.as_ref().and_then(|entry| entry.has_psi).unwrap_or(false)
        },
        session_name,
        window_count,
        fleet_config_path,
        federation_node,
        in_agents_config,
        federation,
        manifest_entry,
    })
}

fn locate_validate_name(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || trimmed.starts_with('-') {
        return Err("locate: oracle name must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.contains("..") || value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return Err("locate: oracle name contains a refused path segment".to_owned());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-'))
    {
        return Err("locate: oracle name contains unsupported characters".to_owned());
    }
    Ok(())
}

fn locate_ghq_find(suffix: &str) -> Option<String> {
    if suffix.starts_with('-') || suffix.contains("..") {
        return None;
    }
    let root = ghq_root().join("github.com");
    let Ok(orgs) = std::fs::read_dir(root) else {
        return None;
    };
    let mut repos = Vec::new();
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(entries) = std::fs::read_dir(org.path()) else {
            continue;
        };
        repos.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()));
    }
    repos.sort();
    repos
        .into_iter()
        .find(|path| path_string(path).ends_with(suffix))
        .map(path_string)
}

fn locate_find_oracle_repo_path(oracle: &str) -> Option<String> {
    locate_declared_oracle_repo_path(oracle)
    .or_else(|| locate_ghq_find_oracle_suffix(oracle))
    .or_else(|| locate_ghq_find(&format!("/{oracle}")).filter(|path| native_repo_path_is_oracle(std::path::Path::new(path), oracle)))
}

// Keep raw/stem ordering for stable output fields and filesystem/config enrichment.
// Match normalization belongs to `maw_matcher`.
fn locate_enrichment_names(oracle: &str) -> Vec<String> {
    let parsed = maw_identity::parse_session_name(oracle);
    let mut aliases = vec![oracle.to_owned()];
    if parsed.stem != oracle && !aliases.contains(&parsed.stem) {
        aliases.push(parsed.stem);
    }
    aliases
}

fn locate_declared_oracle_repo_path(oracle: &str) -> Option<String> {
    for entry in fleet_load_entries().into_iter().filter(fleet_entry_is_session) {
        for window in &entry.session.windows {
            if window.kind != Some(NativeRepoKind::Oracle) {
                continue;
            }
            let Some(name) = native_fleet_window_oracle_name(window) else { continue; };
            if name != oracle {
                continue;
            }
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            if path.exists() {
                return Some(path_string(path));
            }
        }
    }
    None
}

fn locate_ghq_find_oracle_suffix(oracle: &str) -> Option<String> {
    let path = locate_ghq_find(&format!("/{oracle}-oracle"))?;
    native_repo_path_is_oracle(std::path::Path::new(&path), &format!("{oracle}-oracle")).then_some(path)
}

fn locate_resolve_session<'a>(oracle: &str, sessions: &'a [TmuxSession]) -> Option<&'a TmuxSession> {
    let wanted = maw_matcher::normalized_match_names(oracle);
    sessions.iter().find(|session| locate_session_matches(session, &wanted))
}

fn locate_session_matches(session: &TmuxSession, wanted: &[String]) -> bool {
    maw_matcher::normalized_match_names(&session.name)
        .iter()
        .any(|name| wanted.contains(name))
        || session.windows.iter().any(|locate_window| {
            maw_matcher::normalized_match_names(&locate_window.name)
                .iter()
                .any(|name| wanted.contains(name))
        })
}

fn locate_find_fleet_config_path(oracle: &str, session_name: Option<&str>) -> Option<String> {
    let mut names = BTreeSet::from([oracle.to_owned(), format!("{oracle}-oracle")]);
    if let Some(session_name) = session_name {
        names.insert(session_name.to_owned());
    }
    locate_load_fleet_entries()
        .into_iter()
        .find(|entry| locate_fleet_entry_matches(entry, &names))
        .map(|entry| entry.path)
}

fn locate_fleet_entry_matches(entry: &LocateFleetEntry, names: &BTreeSet<String>) -> bool {
    let file_base = entry.file.strip_suffix(".json").unwrap_or(&entry.file);
    [file_base, entry.session.name.as_str()]
        .into_iter()
        .any(|name| names.contains(name))
        || entry
            .session
            .windows
            .iter()
            .any(|locate_window| native_fleet_window_is_oracle(locate_window) && names.contains(locate_window.name.as_str()))
}

fn locate_load_fleet_entries() -> Vec<LocateFleetEntry> {
    fleet_load_entries()
        .into_iter()
        .filter(fleet_entry_is_session)
        .map(|entry| LocateFleetEntry {
            window_sites: locate_load_fleet_window_sites(&entry.path),
            file: entry.file,
            path: path_string(entry.path),
            session: entry.session,
        })
        .collect()
}

#[derive(Debug, serde::Deserialize)]
struct LocateFleetSiteFile {
    #[serde(default)]
    windows: Vec<LocateFleetSiteWindow>,
}

#[derive(Debug, serde::Deserialize)]
struct LocateFleetSiteWindow {
    name: String,
    site: Option<String>,
    pages: Option<String>,
}

fn locate_load_fleet_window_sites(path: &std::path::Path) -> HashMap<String, String> {
    let Some(file) = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<LocateFleetSiteFile>(&text).ok()) else {
        return HashMap::new();
    };
    file.windows
        .into_iter()
        .filter_map(|window| {
            let site = window.site.as_deref().or(window.pages.as_deref()).and_then(locate_clean_site_url)?;
            Some((window.name, site))
        })
        .collect()
}

fn locate_lookup_manifest_entry(oracle: &str) -> Option<LocateManifestEntry> {
    let manifest = locate_load_manifest();
    let stripped = oracle.strip_suffix("-oracle").unwrap_or(oracle);
    manifest
        .iter()
        .find(|entry| entry.name == oracle)
        .cloned()
        .or_else(|| {
            (stripped != oracle)
                .then(|| manifest.into_iter().find(|entry| entry.name == stripped))
                .flatten()
        })
}

fn locate_load_manifest() -> Vec<LocateManifestEntry> {
    let config = locate_load_config();
    let mut by_name = BTreeMap::<String, LocateManifestEntry>::new();
    for fleet in locate_load_fleet_entries() {
        for locate_window in &fleet.session.windows {
            let Some(name) = locate_name_from_window(locate_window) else {
                continue;
            };
            let entry = locate_ensure_manifest_entry(&mut by_name, &name);
            locate_add_manifest_source(entry, "fleet");
            entry.has_fleet_config = Some(true);
            entry.session.get_or_insert_with(|| fleet.session.name.clone());
            entry.window.get_or_insert_with(|| locate_window.name.clone());
            if !locate_window.repo.is_empty() {
                entry.repo.get_or_insert_with(|| locate_window.repo.clone());
            }
            if let Some(site) = fleet.window_sites.get(&locate_window.name).and_then(|site| locate_clean_site_url(site)) {
                entry.site.get_or_insert(site);
            }
            entry.node.get_or_insert_with(|| "local".to_owned());
        }
    }
    for (name, session_id) in &config.sessions {
        let entry = locate_ensure_manifest_entry(&mut by_name, name);
        locate_add_manifest_source(entry, "session");
        if !session_id.is_empty() {
            entry.session_id.get_or_insert_with(|| session_id.clone());
        }
    }
    for (raw_name, node) in &config.agents {
        let name = raw_name.strip_suffix("-oracle").unwrap_or(raw_name);
        let entry = locate_ensure_manifest_entry(&mut by_name, name);
        locate_add_manifest_source(entry, "agent");
        if !node.is_empty() && (entry.node.is_none() || !raw_name.ends_with("-oracle")) {
            entry.node = Some(node.clone());
        }
    }
    if let Some(cache) = locate_load_registry_cache() {
        for oracle in cache.oracles {
            let entry = locate_ensure_manifest_entry(&mut by_name, &oracle.name);
            locate_add_manifest_source(entry, "oracles-json");
            if entry.repo.is_none() && !oracle.org.is_empty() && !oracle.repo.is_empty() {
                entry.repo = Some(format!("{}/{}", oracle.org, oracle.repo));
            }
            if entry.site.is_none() {
                entry.site = oracle.site.as_deref().or(oracle.pages.as_deref()).and_then(locate_clean_site_url);
            }
            if entry.local_path.is_none() && !oracle.local_path.is_empty() {
                entry.local_path = Some(oracle.local_path);
            }
            entry.has_psi.get_or_insert(oracle.has_psi);
            entry.has_fleet_config.get_or_insert(oracle.has_fleet_config);
            if entry.node.is_none() {
                entry.node = oracle.federation_node;
            }
        }
    }
    by_name.into_values().collect()
}

fn locate_ensure_manifest_entry<'a>(
    by_name: &'a mut BTreeMap<String, LocateManifestEntry>,
    name: &str,
) -> &'a mut LocateManifestEntry {
    by_name.entry(name.to_owned()).or_insert_with(|| LocateManifestEntry {
        name: name.to_owned(),
        sources: Vec::new(),
        node: None,
        session: None,
        window: None,
        repo: None,
        site: None,
        local_path: None,
        session_id: None,
        has_psi: None,
        has_fleet_config: None,
        is_live: false,
    })
}

fn locate_add_manifest_source(entry: &mut LocateManifestEntry, source: &str) {
    if !entry.sources.iter().any(|existing| existing == source) {
        entry.sources.push(source.to_owned());
    }
}

fn locate_name_from_window(window: &NativeFleetWindow) -> Option<String> { native_fleet_window_oracle_name(window) }

fn locate_load_registry_cache() -> Option<LocateRegistryCache> {
    let env = current_xdg_env();
    let primary = maw_cache_path(&env, &["oracles.json"]);
    let legacy = maw_config_path(&env, &["oracles.json"]);
    let path = if primary.exists() { primary } else { legacy };
    let text = std::fs::read_to_string(path).ok()?;
    let cache = serde_json::from_str::<LocateRegistryCache>(&text).ok()?;
    (cache.schema == 1).then_some(cache)
}

fn locate_load_config() -> LocateConfig {
    let value = merged_config_value();
    LocateConfig {
        node: value.get("node").and_then(serde_json::Value::as_str).map(ToOwned::to_owned),
        agents: locate_string_map(value.get("agents")),
        sessions: locate_string_map(value.get("sessions")),
    }
}

fn locate_string_map(value: Option<&serde_json::Value>) -> HashMap<String, String> {
    value
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn locate_resolve_site(manifest_entry: Option<&LocateManifestEntry>, repo_path: Option<&str>) -> (Option<String>, Option<String>) {
    if let Some(site) = manifest_entry.and_then(|entry| entry.site.clone()) {
        return (Some(site), None);
    }
    let repo = manifest_entry
        .and_then(|entry| entry.repo.as_deref())
        .map(str::to_owned)
        .or_else(|| repo_path.and_then(locate_repo_slug_from_path));
    locate_derive_github_pages_site(repo.as_deref()).map_or((None, None), |site| (Some(site), Some("derived".to_owned())))
}

fn locate_clean_site_url(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && (value.starts_with("https://") || value.starts_with("http://"))
        && !value.chars().any(|ch| ch.is_control() || ch.is_whitespace()))
    .then(|| value.to_owned())
}

fn locate_derive_github_pages_site(repo: Option<&str>) -> Option<String> {
    let repo = repo?.trim().trim_end_matches(".git");
    let repo = repo.strip_prefix("github.com/").unwrap_or(repo);
    let mut parts = repo.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() || !locate_github_pages_segment_ok(owner) || !locate_github_pages_segment_ok(name) {
        return None;
    }
    Some(format!("https://{owner}.github.io/{name}"))
}

fn locate_repo_slug_from_path(path: &str) -> Option<String> {
    let root = ghq_root().join("github.com");
    let path = std::path::Path::new(path);
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = rel.components().map(|part| part.as_os_str().to_string_lossy());
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn locate_github_pages_segment_ok(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn locate_find_federation_hits(_oracle: &str) -> Vec<LocateFederationHit> {
    Vec::new()
}

fn locate_render_text(oracle: &str, info: &LocateResult) -> String {
    let mut out = format!("\n📍 {oracle}\n");
    if let Some(repo_path) = &info.repo_path {
        let _ = writeln!(out, "   repo:     {repo_path}");
        let _ = writeln!(out, "   ψ/:       {}", if info.has_psi { "present" } else { "missing" });
    }
    if let Some(session_name) = &info.session_name {
        let suffix = if info.window_count == 1 { "" } else { "s" };
        let _ = writeln!(out, "   session:  {session_name} ({} window{suffix})", info.window_count);
    }
    if let Some(fleet_config_path) = &info.fleet_config_path {
        let _ = writeln!(out, "   fleet:    {fleet_config_path}");
    }
    if let Some(manifest_entry) = &info.manifest_entry {
        let _ = writeln!(out, "   source:   {}", manifest_entry.sources.join(", "));
        if manifest_entry.repo.is_some() && info.repo_path.is_none() {
            let _ = writeln!(out, "   repo:     {}", manifest_entry.repo.as_deref().unwrap_or_default());
        }
        if manifest_entry.has_fleet_config == Some(true) && info.fleet_config_path.is_none() {
            out.push_str("   fleet:    known (manifest)\n");
        }
    }
    if let Some(node) = &info.federation_node {
        let suffix = if info.in_agents_config {
            " (from config.agents)"
        } else if info.session_name.is_some() {
            " (this node)"
        } else if info.manifest_entry.as_ref().and_then(|entry| entry.node.as_ref()).is_some() {
            " (from manifest)"
        } else {
            " (this node)"
        };
        let _ = writeln!(out, "   node:     {node}{suffix}");
    }
    for hit in &info.federation {
        let label = hit.node.as_ref().unwrap_or(&hit.alias);
        let location = hit.url.as_ref().map_or(String::new(), |url| format!(" ({url})"));
        let suffix = if hit.window_count == 1 { "" } else { "s" };
        let _ = writeln!(
            out,
            "   remote:   {label}:{}{location} ({} window{suffix})",
            hit.session_name, hit.window_count
        );
    }
    out.push('\n');
    out
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod locate_tests {
    use super::*;

    const LOCATE_ENV_KEYS: &[&str] = &[
        "HOME",
        "GHQ_ROOT",
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "MAW_JS_REF_DIR",
        "TMUX",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ];

    struct LocateHermeticEnv {
        home: std::path::PathBuf,
        ghq: std::path::PathBuf,
        xdg_config: std::path::PathBuf,
        xdg_data: std::path::PathBuf,
        xdg_state: std::path::PathBuf,
        xdg_cache: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl LocateHermeticEnv {
        fn new(name: &str) -> Self {
            let root = locate_temp_root(name);
            let home = root.join("home");
            let ghq = root.join("ghq");
            let xdg_config = root.join("xdg-config");
            let xdg_data = root.join("xdg-data");
            let xdg_state = root.join("xdg-state");
            let xdg_cache = root.join("xdg-cache");
            for dir in [&home, &ghq, &xdg_config, &xdg_data, &xdg_state, &xdg_cache] {
                std::fs::create_dir_all(dir).expect("hermetic dir");
            }
            let saved = LOCATE_ENV_KEYS
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            for key in LOCATE_ENV_KEYS {
                std::env::remove_var(key);
            }
            std::env::set_var("HOME", &home);
            std::env::set_var("GHQ_ROOT", &ghq);
            std::env::set_var("XDG_CONFIG_HOME", &xdg_config);
            std::env::set_var("XDG_DATA_HOME", &xdg_data);
            std::env::set_var("XDG_STATE_HOME", &xdg_state);
            std::env::set_var("XDG_CACHE_HOME", &xdg_cache);
            std::env::set_var("MAW_XDG", "1");
            std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
            Self {
                home,
                ghq,
                xdg_config,
                xdg_data,
                xdg_state,
                xdg_cache,
                saved,
            }
        }

        fn maw_config_path(&self, parts: &[&str]) -> std::path::PathBuf {
            let path = maw_config_path(&current_xdg_env(), parts);
            assert!(path.starts_with(&self.xdg_config));
            path
        }

        fn maw_cache_path(&self, parts: &[&str]) -> std::path::PathBuf {
            let path = maw_cache_path(&current_xdg_env(), parts);
            assert!(path.starts_with(&self.xdg_cache));
            path
        }
    }

    impl Drop for LocateHermeticEnv {
        fn drop(&mut self) {
            for key in LOCATE_ENV_KEYS {
                std::env::remove_var(key);
            }
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                }
            }
        }
    }


    fn locate_temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "maw-rs-locate-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp home");
        root
    }

    fn locate_write(path: &std::path::Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, text).expect("write");
    }

    fn locate_expected_golden(fleet_config: &std::path::Path, repo: &std::path::Path) -> String {
        include_str!("../../tests/fixtures/locate/atlas.json")
            .replace("__CONFIG_PLACEHOLDER__/maw/fleet/alpha.json", &path_string(fleet_config))
            .replace("__REPO_PLACEHOLDER__", &path_string(repo))
    }

    fn locate_window(index: u32, name: &str) -> maw_tmux::TmuxWindow {
        maw_tmux::TmuxWindow {
            index,
            name: name.to_owned(),
            active: index == 1,
            cwd: None,
        }
    }

    #[test]
    fn locate_json_matches_committed_golden_and_ignores_missing_js_ref() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("json");
        assert_eq!(std::env::var_os("TMUX"), None);
        assert_eq!(current_xdg_env().home_dir(), env.home.as_path());
        let repo = env.ghq.join("github.com/acme/atlas-oracle");
        std::fs::create_dir_all(repo.join("ψ")).expect("repo");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"node":"local","agents":{"atlas":"edge"},"sessions":{"atlas":"session-uuid"}}"#,
        );
        let fleet_config = env.maw_config_path(&["fleet", "alpha.json"]);
        locate_write(
            &fleet_config,
            r#"{"name":"alpha","windows":[{"name":"atlas-oracle","repo":"acme/atlas-oracle"},{"name":"logs","repo":""}]}"#,
        );
        let registry_cache = env.maw_cache_path(&["oracles.json"]);
        locate_write(
            &registry_cache,
            &format!(
                r#"{{"schema":1,"local_scanned_at":"2026-06-25T00:00:00Z","ghq_root":"{}","oracles":[{{"org":"acme","repo":"atlas-oracle","name":"atlas","local_path":"{}","has_psi":true,"has_fleet_config":true,"budded_from":null,"budded_at":null,"federation_node":"edge","detected_at":"2026-06-25T00:00:00Z"}}]}}"#,
                env.ghq.display(),
                repo.display()
            ),
        );
        assert!(env.xdg_data.exists());
        assert!(env.xdg_state.exists());
        assert!(env.xdg_cache.exists());
        assert!(fleet_config.starts_with(&env.xdg_config));
        assert!(registry_cache.starts_with(&env.xdg_cache));
        let sessions = vec![TmuxSession {
            name: "alpha".to_owned(),
            windows: vec![locate_window(1, "atlas-oracle"), locate_window(2, "logs")],
        }];

        let info = locate_gather_info("atlas", true, &sessions).expect("locate info");
        let rendered = serde_json::to_string_pretty(&info).expect("json") + "\n";
        let expected = locate_expected_golden(&fleet_config, &repo);
        assert_eq!(rendered, expected);
    }

    #[test]
    fn locate_path_is_one_clean_line_from_temp_home() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("path");
        assert_eq!(std::env::var_os("TMUX"), None);
        let repo = env.ghq.join("github.com/acme/pathfinder-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"node":"local","agents":{"pathfinder":"edge"},"sessions":{"pathfinder":"path-session"}}"#,
        );
        locate_write(
            &env.maw_config_path(&["fleet", "pathfinder.json"]),
            r#"{"name":"pathfinder","windows":[{"name":"pathfinder-oracle","repo":"acme/pathfinder-oracle"}]}"#,
        );
        let opts = LocateOptions { path: true, json: false };
        assert_eq!(
            locate_cmd_with_sessions("pathfinder", &opts, &[]).expect("path"),
            format!("{}\n", repo.display())
        );
    }

    #[test]
    fn locate_json_uses_explicit_site_before_derived_pages_default() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("site");
        locate_write(
            &env.maw_config_path(&["fleet", "kru32.json"]),
            r#"{"name":"kru32","windows":[{"name":"kru32-oracle","repo":"owner/kru32-oracle","site":"https://kru32.example.test/feed"}]}"#,
        );

        let info = locate_gather_info("kru32", true, &[]).expect("locate info");
        assert_eq!(info.site.as_deref(), Some("https://kru32.example.test/feed"));
        assert_eq!(info.site_source, None);
        assert_eq!(info.manifest_entry.as_ref().and_then(|entry| entry.site.as_deref()), Some("https://kru32.example.test/feed"));
        let rendered = serde_json::to_value(&info).expect("json");
        assert_eq!(rendered["site"], "https://kru32.example.test/feed");
        assert!(rendered.get("siteSource").is_none());
    }

    #[test]
    fn locate_json_omits_site_when_manifest_and_repo_are_absent() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("no-site");
        locate_write(&env.maw_config_path(&["maw.config.json"]), r#"{"agents":{"ghost":"edge"}}"#);

        let info = locate_gather_info("ghost", true, &[]).expect("locate info");
        assert_eq!(info.site, None);
        assert_eq!(info.site_source, None);
        let rendered = serde_json::to_value(&info).expect("json");
        assert!(rendered.get("site").is_none());
        assert!(rendered.get("siteSource").is_none());
    }

    #[test]
    fn locate_repo_resolution_uses_declared_kind_before_suffix() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("kind");
        let foo = env.ghq.join("github.com/acme/foo");
        let bar = env.ghq.join("github.com/acme/bar-oracle");
        std::fs::create_dir_all(&foo).expect("foo");
        std::fs::create_dir_all(&bar).expect("bar");
        locate_write(
            &env.maw_config_path(&["fleet", "kind.json"]),
            r#"{"name":"kind","windows":[{"name":"foo","repo":"acme/foo","kind":"oracle"},{"name":"bar-oracle","repo":"acme/bar-oracle","kind":"project"}]}"#,
        );
        let opts = LocateOptions { path: true, json: false };

        assert_eq!(
            locate_cmd_with_sessions("foo", &opts, &[]).expect("foo path"),
            format!("{}\n", foo.display())
        );
        assert!(locate_cmd_with_sessions("bar", &opts, &[]).expect_err("bar project").contains("no oracle"));
    }

    #[test]
    fn locate_rejects_option_injection_targets() {
        assert!(locate_parse_args(&["--json".to_owned(), "-bad".to_owned()]).is_err());
        assert!(locate_parse_args(&["../bad".to_owned()]).is_err());
        assert!(locate_validate_name("good-oracle").is_ok());
    }

    #[test]
    fn locate_prefixed_names_resolve_with_stripped_stem() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("prefixed");
        let repo = env.ghq.join("github.com/acme/track-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["fleet", "81-track.json"]),
            r#"{"name":"81-track","windows":[{"name":"track-oracle","repo":"acme/track-oracle"}]}"#,
        );
        let options = LocateOptions { path: true, json: false };
        assert_eq!(
            locate_cmd_with_sessions("81-track", &options, &[]).expect("track path"),
            format!("{}\n", repo.display())
        );
        let info = locate_gather_info("81-track", true, &[]).expect("prefixed locate info");
        let info_plain = locate_gather_info("track", true, &[]).expect("plain locate info");
        assert_eq!(info.repo_path, info_plain.repo_path);
        assert_eq!(info.name, info_plain.name);
        assert_eq!(info.session, "81-track");
        assert_eq!(info.session, info_plain.session);
        assert_eq!(info.handle, "track");
        assert_eq!(info.session_name, info_plain.session_name);
        assert_eq!(info.fleet_config_path, info_plain.fleet_config_path);

        let sessions = vec![TmuxSession {
            name: "81-track".to_owned(),
            windows: vec![locate_window(1, "track-oracle")],
        }];
        let output = run_locate_command_with_sessions(
            &["81-track".to_owned(), "--json".to_owned()],
            &sessions,
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        let rendered: serde_json::Value = serde_json::from_str(&output.stdout).expect("json");
        assert_eq!(rendered["name"], "track");
        assert_eq!(rendered["handle"], "track");
        assert_eq!(rendered["session"], "81-track");
        assert_eq!(rendered["sessionName"], "81-track");
        assert_eq!(rendered["windowCount"], 1);
        assert_eq!(rendered["repoPath"], path_string(&repo));
    }

    #[test]
    fn locate_typed_inventory_routes_exact_and_asks_on_fuzzy() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let env = LocateHermeticEnv::new("typed-picker");
        let repo = env.ghq.join("github.com/acme/track-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["fleet", "81-track.json"]),
            r#"{"name":"81-track","windows":[{"name":"track-oracle","repo":"acme/track-oracle"}]}"#,
        );
        let options = LocateOptions { path: true, json: false };
        assert_eq!(locate_picker_target("81-track", &options, &[]).expect("exact"), "track");

        match typed_picker_plan("trac", &locate_typed_candidates(&[]), locate_kind_priority, locate_picker_row) {
            TypedPickerPlan::Pick { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].matched.candidate.name, "track");
                assert_eq!(rows[0].action, "maw locate track");
            }
            plan @ TypedPickerPlan::Target(_) => panic!("expected fuzzy picker, got {plan:?}"),
        }
    }
}
