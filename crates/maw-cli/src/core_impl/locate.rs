const DISPATCH_41: &[DispatcherEntry] = &[DispatcherEntry {
    command: "locate",
    handler: Handler::Sync(run_locate_command),
}];

const LOCATE_USAGE: &str = "usage: maw locate <oracle> [--path | --json] [--no-remote]\n  e.g. maw locate mawjs\n  --no-remote  skip the default GitHub org scan (offline/fast mode)";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocateOptions {
    path: bool,
    json: bool,
    no_remote: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    repo_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo_confidence: Option<String>,
    has_psi: bool,
    session_name: Option<String>,
    window_count: usize,
    fleet_config_path: Option<String>,
    federation_node: Option<String>,
    in_agents_config: bool,
    federation: Vec<LocateFederationHit>,
    manifest_entry: Option<LocateManifestEntry>,
    layers: LocateLayers,
}

/// Per-layer truth report (#604): each layer answers "does this oracle exist
/// here?" independently, so a manifest-only phantom can no longer masquerade
/// as a located oracle.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LocateLayers {
    manifest: Option<String>,
    disk: Option<String>,
    session: Option<String>,
    registry: Option<String>,
    github: LocateGithubLayer,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LocateGithubLayer {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    orgs: Vec<String>,
}

impl LocateGithubLayer {
    fn skipped(reason: &str) -> Self {
        Self {
            status: "skipped".to_owned(),
            repo: None,
            url: None,
            reason: Some(reason.to_owned()),
            orgs: Vec::new(),
        }
    }

    fn found(org: &str, name: &str) -> Self {
        Self {
            status: "found".to_owned(),
            repo: Some(format!("{org}/{name}")),
            url: Some(format!("https://github.com/{org}/{name}")),
            reason: None,
            orgs: Vec::new(),
        }
    }

    fn not_found(orgs: Vec<String>) -> Self {
        Self {
            status: "not-found".to_owned(),
            repo: None,
            url: None,
            reason: None,
            orgs,
        }
    }
}

/// Injectable `gh repo list` boundary so tests never touch the network.
/// `Err(reason)` means the org could not be scanned (gh missing,
/// unauthenticated, network failure, timeout) — locate degrades to a
/// `github: (skipped: <reason>)` line and never fails because of it.
trait LocateGithubRunner {
    fn locate_gh_repo_names(&mut self, org: &str) -> Result<Vec<String>, String>;
}

struct LocateSystemGithub;

impl LocateGithubRunner for LocateSystemGithub {
    fn locate_gh_repo_names(&mut self, org: &str) -> Result<Vec<String>, String> {
        locate_gh_repo_list_with_timeout(org, locate_github_timeout())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocateRepoResolution {
    path: String,
    classification: NativeRepoClassification,
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
    run_locate_command_with_sessions(argv, &tmux.list_all(), &mut LocateSystemGithub)
}

fn run_locate_command_with_sessions(
    argv: &[String],
    sessions: &[TmuxSession],
    github: &mut dyn LocateGithubRunner,
) -> CliOutput {
    match locate_parse_args(argv) {
        Ok((oracle, opts)) => match locate_picker_target(&oracle, &opts, sessions)
            .and_then(|target| locate_cmd_with_sessions(&target, &opts, sessions, github).map_err(|message| CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }))
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
        no_remote: false,
    };
    let mut oracle: Option<String> = None;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(LOCATE_USAGE.to_owned()),
            "--path" | "-p" => opts.path = true,
            "--json" => opts.json = true,
            "--no-remote" => opts.no_remote = true,
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
    github: &mut dyn LocateGithubRunner,
) -> Result<String, String> {
    locate_validate_name(oracle)?;
    let github_layer = if opts.no_remote || opts.path {
        LocateGithubLayer::skipped("--no-remote")
    } else {
        locate_scan_github(oracle, &locate_remote_orgs(&merged_config_value()), github)
    };
    let info = locate_gather_info(oracle, !opts.path, sessions, github_layer)?;

    if info.repo_path.is_none()
        && info.session_name.is_none()
        && info.fleet_config_path.is_none()
        && info.federation.is_empty()
        && info.manifest_entry.is_none()
        && info.layers.github.repo.is_none()
    {
        return Err(format!(
            "no oracle named '{oracle}' — try: maw oracle ls{}",
            locate_github_error_note(&info.layers.github)
        ));
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
    github_layer: LocateGithubLayer,
) -> Result<LocateResult, String> {
    locate_validate_name(oracle)?;
    let aliases = locate_enrichment_names(oracle);
    let repo = aliases.iter().find_map(|alias| locate_find_oracle_repo(alias));
    let repo_path = repo.as_ref().map(|repo| repo.path.clone());
    let repo_source = repo.as_ref().map(|repo| repo.classification.source.label().to_owned());
    let repo_confidence = repo.as_ref().map(|repo| repo.classification.source.confidence().to_owned());
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
    let manifest_layer = locate_agents_map_node(&config, &aliases);
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
    let disk_layer = repo_path.clone().or_else(|| {
        manifest_entry
            .as_ref()
            .and_then(|entry| entry.local_path.clone())
            .filter(|path| std::path::Path::new(path).exists())
    });
    let layers = LocateLayers {
        manifest: manifest_layer,
        disk: disk_layer,
        session: session_name.clone(),
        registry: fleet_config_path.clone(),
        github: github_layer,
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
        repo_source,
        repo_confidence,
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
        layers,
    })
}

/// Layer 1 truth: is the name (or its `-oracle` sibling) in the merged
/// config's `agents` map, and on which node? Broader than `in_agents_config`
/// (kept as-is for JSON compatibility), which only checks exact aliases.
fn locate_agents_map_node(config: &LocateConfig, aliases: &[String]) -> Option<String> {
    aliases.iter().find_map(|alias| {
        config
            .agents
            .get(alias.as_str())
            .or_else(|| config.agents.get(&format!("{alias}-oracle")))
            .cloned()
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

/// Path-only view kept for callers that do not need the classification
/// (`notify`, `send_federation`, `agents`).
fn locate_find_oracle_repo_path(oracle: &str) -> Option<String> {
    locate_find_oracle_repo(oracle).map(|repo| repo.path)
}

fn locate_find_oracle_repo(oracle: &str) -> Option<LocateRepoResolution> {
    locate_declared_oracle_repo(oracle)
        // #750: a bare repo now resolves through an explicit declaration or the
        // ψ/ + CLAUDE.md heuristic. The legacy suffix stays last so existing
        // fleet names remain additive-compatible.
        .or_else(|| locate_ghq_find(&format!("/{oracle}")).and_then(|path| locate_classify_repo_path(&path, oracle)))
        .or_else(|| locate_ghq_find_oracle_suffix(oracle))
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

fn locate_declared_oracle_repo(oracle: &str) -> Option<LocateRepoResolution> {
    for entry in fleet_load_entries().into_iter().filter(fleet_entry_is_session) {
        for window in &entry.session.windows {
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            if path.exists() {
                let path = path_string(path);
                if let Some(repo) = locate_classify_repo_path(&path, &window.name) {
                    let Some(name) = native_fleet_window_oracle_name(window) else { continue; };
                    if name == oracle {
                        return Some(repo);
                    }
                }
            }
        }
    }
    None
}

fn locate_ghq_find_oracle_suffix(oracle: &str) -> Option<LocateRepoResolution> {
    let path = locate_ghq_find(&format!("/{oracle}-oracle"))?;
    locate_classify_repo_path(&path, &format!("{oracle}-oracle"))
}

fn locate_classify_repo_path(path: &str, fallback_name: &str) -> Option<LocateRepoResolution> {
    let classification = native_repo_classification_for_path(std::path::Path::new(path), fallback_name)?;
    (classification.kind == NativeRepoKind::Oracle).then_some(LocateRepoResolution { path: path.to_owned(), classification })
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

/// Built-in fallback used ONLY when the merged config has no `locate.orgs`
/// key; an explicit (even empty) key always wins.
const LOCATE_DEFAULT_ORGS: &[&str] = &["Soul-Brews-Studio", "laris-co"];

fn locate_remote_orgs(config: &serde_json::Value) -> Vec<String> {
    match config.get("locate").and_then(|locate| locate.get("orgs")) {
        Some(value) => value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        None => LOCATE_DEFAULT_ORGS.iter().map(|org| (*org).to_owned()).collect(),
    }
}

fn locate_scan_github(
    oracle: &str,
    orgs: &[String],
    runner: &mut dyn LocateGithubRunner,
) -> LocateGithubLayer {
    if orgs.is_empty() {
        return LocateGithubLayer::skipped("locate.orgs is empty");
    }
    let wanted = locate_github_wanted_names(oracle);
    let mut first_failure: Option<String> = None;
    let mut scanned = Vec::new();
    for org in orgs {
        if !locate_github_pages_segment_ok(org) {
            first_failure.get_or_insert(format!("invalid org name '{org}'"));
            continue;
        }
        match runner.locate_gh_repo_names(org) {
            Ok(names) => {
                scanned.push(org.clone());
                if let Some(name) = names
                    .iter()
                    .find(|name| wanted.iter().any(|want| want.eq_ignore_ascii_case(name)))
                {
                    return LocateGithubLayer::found(org, name);
                }
            }
            Err(reason) => {
                let fatal = reason == "gh not installed";
                first_failure.get_or_insert(reason);
                if fatal {
                    break;
                }
            }
        }
    }
    // Any unscannable org means "✗ not on github" would overclaim — degrade
    // to a skip so the report stays truthful.
    match first_failure {
        Some(reason) => LocateGithubLayer::skipped(&reason),
        None => LocateGithubLayer::not_found(scanned),
    }
}

fn locate_github_wanted_names(oracle: &str) -> Vec<String> {
    let mut wanted = Vec::new();
    for alias in locate_enrichment_names(oracle) {
        let suffixed = format!("{alias}-oracle");
        if !wanted.contains(&alias) {
            wanted.push(alias);
        }
        if !wanted.contains(&suffixed) {
            wanted.push(suffixed);
        }
    }
    wanted
}

fn locate_github_error_note(github: &LocateGithubLayer) -> String {
    match github.status.as_str() {
        "skipped" => format!(
            " (github: skipped: {})",
            github.reason.as_deref().unwrap_or("unknown")
        ),
        "not-found" => format!(" (github: not found in {})", github.orgs.join(", ")),
        _ => String::new(),
    }
}

fn locate_github_timeout() -> std::time::Duration {
    std::env::var("MAW_LOCATE_GH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| (100..=60_000).contains(millis))
        .map_or_else(|| std::time::Duration::from_secs(4), std::time::Duration::from_millis)
}

#[derive(Debug, serde::Deserialize)]
struct LocateGhRepoName {
    name: String,
}

/// Runs `gh repo list <org> --json name` with a hard timeout. The child runs
/// on a helper thread so a stalled network can never hang locate; on timeout
/// the thread is abandoned (the CLI exits right after rendering).
fn locate_gh_repo_list_with_timeout(
    org: &str,
    timeout: std::time::Duration,
) -> Result<Vec<String>, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let org_arg = org.to_owned();
    std::thread::spawn(move || {
        let _ = sender.send(
            std::process::Command::new("gh")
                .args(["repo", "list", &org_arg, "--json", "name", "--limit", "1000"])
                .stdin(std::process::Stdio::null())
                .output(),
        );
    });
    let output = match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("gh not installed".to_owned());
        }
        Ok(Err(error)) => return Err(format!("gh failed to start: {error}")),
        Err(_) => return Err(format!("gh timed out after {}ms", timeout.as_millis())),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
        return Err(if first.is_empty() {
            format!("gh exited {}", output.status)
        } else {
            first.to_owned()
        });
    }
    serde_json::from_slice::<Vec<LocateGhRepoName>>(&output.stdout)
        .map(|repos| repos.into_iter().map(|repo| repo.name).collect())
        .map_err(|error| format!("gh returned unparseable json: {error}"))
}

fn locate_render_text(oracle: &str, info: &LocateResult) -> String {
    let mut out = format!("\n📍 {oracle}\n");
    if let Some(repo_path) = &info.repo_path {
        let _ = writeln!(out, "   repo:     {repo_path}");
        if let Some(source) = &info.repo_source {
            let confidence = info.repo_confidence.as_deref().unwrap_or("unknown");
            let _ = writeln!(out, "   repo_source: {source} ({confidence})");
        }
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
    locate_render_layers(info, &mut out);
    out.push('\n');
    out
}

fn locate_render_layers(info: &LocateResult, out: &mut String) {
    let layers = &info.layers;
    match layers.manifest.as_deref() {
        Some("") => out.push_str("   manifest:  ✓ (agents map)\n"),
        Some(node) => {
            let _ = writeln!(out, "   manifest:  ✓ {node} (agents map)");
        }
        None => out.push_str("   manifest:  ✗ not in agents map\n"),
    }
    match &layers.disk {
        Some(path) => {
            let _ = writeln!(out, "   disk:      ✓ {path}");
        }
        None => out.push_str("   disk:      ✗ not cloned (ghq)\n"),
    }
    match &layers.session {
        Some(name) => {
            let suffix = if info.window_count == 1 { "" } else { "s" };
            let _ = writeln!(out, "   session:   ✓ {name} ({} window{suffix})", info.window_count);
        }
        None => out.push_str("   session:   ✗ none\n"),
    }
    match &layers.registry {
        Some(path) => {
            let _ = writeln!(out, "   registry:  ✓ {path}");
        }
        None => out.push_str("   registry:  ✗ none\n"),
    }
    out.push_str(&locate_render_github_layer(layers));
}

fn locate_render_github_layer(layers: &LocateLayers) -> String {
    let github = &layers.github;
    match github.status.as_str() {
        "found" => {
            let repo = github.repo.as_deref().unwrap_or("?");
            let hint = match (&layers.disk, &github.url) {
                (None, Some(url)) => format!("   → maw work {url}"),
                _ => String::new(),
            };
            format!("   github:    ✓ {repo}{hint}\n")
        }
        "not-found" => format!("   github:    ✗ not on github (orgs: {})\n", github.orgs.join(", ")),
        _ => format!(
            "   github:    (skipped: {})\n",
            github.reason.as_deref().unwrap_or("unknown")
        ),
    }
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

    fn locate_github_off() -> LocateGithubLayer {
        LocateGithubLayer::skipped("--no-remote")
    }

    #[derive(Default)]
    struct LocateFakeGithub {
        calls: Vec<String>,
        responses: HashMap<String, Result<Vec<String>, String>>,
    }

    impl LocateFakeGithub {
        fn with(mut self, org: &str, response: Result<Vec<String>, String>) -> Self {
            self.responses.insert(org.to_owned(), response);
            self
        }
    }

    impl LocateGithubRunner for LocateFakeGithub {
        fn locate_gh_repo_names(&mut self, org: &str) -> Result<Vec<String>, String> {
            self.calls.push(org.to_owned());
            self.responses.get(org).cloned().unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    #[test]
    fn locate_json_matches_committed_golden_and_ignores_missing_js_ref() {
        let _guard = env_test_lock();
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

        let info = locate_gather_info("atlas", true, &sessions, locate_github_off()).expect("locate info");
        let rendered = serde_json::to_string_pretty(&info).expect("json") + "\n";
        let expected = locate_expected_golden(&fleet_config, &repo);
        assert_eq!(rendered, expected);
    }

    #[test]
    fn locate_path_is_one_clean_line_from_temp_home() {
        let _guard = env_test_lock();
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
        let opts = LocateOptions { path: true, json: false, no_remote: false };
        assert_eq!(
            locate_cmd_with_sessions("pathfinder", &opts, &[], &mut LocateFakeGithub::default()).expect("path"),
            format!("{}\n", repo.display())
        );
    }

    #[test]
    fn locate_json_uses_explicit_site_before_derived_pages_default() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("site");
        locate_write(
            &env.maw_config_path(&["fleet", "kru32.json"]),
            r#"{"name":"kru32","windows":[{"name":"kru32-oracle","repo":"owner/kru32-oracle","site":"https://kru32.example.test/feed"}]}"#,
        );

        let info = locate_gather_info("kru32", true, &[], locate_github_off()).expect("locate info");
        assert_eq!(info.site.as_deref(), Some("https://kru32.example.test/feed"));
        assert_eq!(info.site_source, None);
        assert_eq!(info.manifest_entry.as_ref().and_then(|entry| entry.site.as_deref()), Some("https://kru32.example.test/feed"));
        let rendered = serde_json::to_value(&info).expect("json");
        assert_eq!(rendered["site"], "https://kru32.example.test/feed");
        assert!(rendered.get("siteSource").is_none());
    }

    #[test]
    fn locate_json_omits_site_when_manifest_and_repo_are_absent() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("no-site");
        locate_write(&env.maw_config_path(&["maw.config.json"]), r#"{"agents":{"ghost":"edge"}}"#);

        let info = locate_gather_info("ghost", true, &[], locate_github_off()).expect("locate info");
        assert_eq!(info.site, None);
        assert_eq!(info.site_source, None);
        let rendered = serde_json::to_value(&info).expect("json");
        assert!(rendered.get("site").is_none());
        assert!(rendered.get("siteSource").is_none());
    }

    #[test]
    fn locate_repo_resolution_uses_declared_kind_before_suffix() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("kind");
        let foo = env.ghq.join("github.com/acme/foo");
        let bar = env.ghq.join("github.com/acme/bar-oracle");
        let conflict = env.ghq.join("github.com/acme/conflict");
        std::fs::create_dir_all(&foo).expect("foo");
        std::fs::create_dir_all(&bar).expect("bar");
        std::fs::create_dir_all(conflict.join(".maw")).expect("conflict marker");
        locate_write(&conflict.join(".maw/role"), "oracle\n");
        locate_write(
            &env.xdg_state.join("maw/fleet/01-marker.json"),
            r#"{"name":"01-marker","windows":[{"name":"conflict","repo":"acme/conflict"}]}"#,
        );
        locate_write(
            &env.maw_config_path(&["fleet", "kind.json"]),
            r#"{"name":"kind","windows":[{"name":"foo","repo":"acme/foo","kind":"oracle"},{"name":"bar-oracle","repo":"acme/bar-oracle","kind":"project"},{"name":"conflict","repo":"acme/conflict","kind":"project"}]}"#,
        );
        let opts = LocateOptions { path: true, json: false, no_remote: false };

        assert_eq!(
            locate_cmd_with_sessions("foo", &opts, &[], &mut LocateFakeGithub::default()).expect("foo path"),
            format!("{}\n", foo.display())
        );
        assert!(locate_cmd_with_sessions("bar", &opts, &[], &mut LocateFakeGithub::default()).expect_err("bar project").contains("no oracle"));
        assert!(
            locate_cmd_with_sessions("conflict", &opts, &[], &mut LocateFakeGithub::default())
                .expect_err("conflict project")
                .contains("no oracle")
        );
    }

    #[test]
    fn locate_reports_classification_source_and_rejects_unmarked_repo() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("classification");
        let kind = env.ghq.join("github.com/acme/kindbare");
        let marker = env.ghq.join("github.com/acme/markerbare");
        let inferred = env.ghq.join("github.com/acme/inferredbare");
        let suffix = env.ghq.join("github.com/acme/suffix-oracle");
        let plain = env.ghq.join("github.com/acme/plain");
        std::fs::create_dir_all(&kind).expect("kind repo");
        std::fs::create_dir_all(marker.join(".maw")).expect("marker repo");
        locate_write(&marker.join(".maw/role"), "oracle\n");
        std::fs::create_dir_all(inferred.join("ψ")).expect("inferred repo");
        locate_write(&inferred.join("CLAUDE.md"), "# Oracle\n");
        std::fs::create_dir_all(&suffix).expect("suffix repo");
        std::fs::create_dir_all(&plain).expect("plain repo");
        locate_write(
            &env.maw_config_path(&["fleet", "classification.json"]),
            r#"{"name":"classification","windows":[{"name":"kindbare","repo":"acme/kindbare","kind":"oracle"}]}"#,
        );

        let expected = [
            ("kindbare", "window.kind", "declared"),
            ("markerbare", ".maw/role", "declared"),
            ("inferredbare", "ψ/ + CLAUDE.md", "inferred"),
            ("suffix", "*-oracle", "legacy-guess"),
        ];
        for (name, source, confidence) in expected {
            let info = locate_gather_info(name, false, &[], LocateGithubLayer::skipped("test")).expect("classification info");
            assert_eq!(info.repo_source.as_deref(), Some(source), "source for {name}");
            assert_eq!(info.repo_confidence.as_deref(), Some(confidence), "confidence for {name}");
            assert!(info.repo_path.is_some(), "repo path for {name}");
        }

        let plain_info = locate_gather_info("plain", false, &[], LocateGithubLayer::skipped("test")).expect("plain info");
        assert!(plain_info.repo_path.is_none());
        assert!(plain_info.repo_source.is_none());
        assert!(plain_info.repo_confidence.is_none());
    }

    #[test]
    fn locate_rejects_option_injection_targets() {
        assert!(locate_parse_args(&["--json".to_owned(), "-bad".to_owned()]).is_err());
        assert!(locate_parse_args(&["../bad".to_owned()]).is_err());
        assert!(locate_validate_name("good-oracle").is_ok());
    }

    #[test]
    fn locate_prefixed_names_resolve_with_stripped_stem() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("prefixed");
        let repo = env.ghq.join("github.com/acme/track-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["fleet", "81-track.json"]),
            r#"{"name":"81-track","windows":[{"name":"track-oracle","repo":"acme/track-oracle"}]}"#,
        );
        let options = LocateOptions { path: true, json: false, no_remote: false };
        assert_eq!(
            locate_cmd_with_sessions("81-track", &options, &[], &mut LocateFakeGithub::default()).expect("track path"),
            format!("{}\n", repo.display())
        );
        let info = locate_gather_info("81-track", true, &[], locate_github_off()).expect("prefixed locate info");
        let info_plain = locate_gather_info("track", true, &[], locate_github_off()).expect("plain locate info");
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
            &mut LocateFakeGithub::default(),
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
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("typed-picker");
        let repo = env.ghq.join("github.com/acme/track-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["fleet", "81-track.json"]),
            r#"{"name":"81-track","windows":[{"name":"track-oracle","repo":"acme/track-oracle"}]}"#,
        );
        let options = LocateOptions { path: true, json: false, no_remote: false };
        // An exact query for the literal registry session name ("81-track")
        // resolves to itself, not the shorter oracle-derived alias ("track")
        // -- #665's literal_name_tiebreak (predates this by an unrelated
        // fix, landed after this test) correctly makes a candidate's own
        // literal name win over a match that only comes from being someone
        // else's alias. This assertion encoded the pre-#665 behavior;
        // confirmed via a worktree at 3979e88^ that it passed there and
        // fails after -- not an env leak, a stale expectation (#700/#688).
        assert_eq!(locate_picker_target("81-track", &options, &[]).expect("exact"), "81-track");

        match typed_picker_plan("trac", &locate_typed_candidates(&[]), locate_kind_priority, locate_picker_row) {
            TypedPickerPlan::Pick { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].matched.candidate.name, "track");
                assert_eq!(rows[0].action, "maw locate track");
            }
            plan @ TypedPickerPlan::Target(_) => panic!("expected fuzzy picker, got {plan:?}"),
        }
    }

    #[test]
    fn locate_parse_accepts_no_remote_and_rejects_unknown_flags() {
        let (oracle, opts) = locate_parse_args(&["atlas".to_owned(), "--no-remote".to_owned()]).expect("parse");
        assert_eq!(oracle, "atlas");
        assert!(opts.no_remote);
        assert!(!opts.path);
        assert!(locate_parse_args(&["atlas".to_owned(), "--remote".to_owned()]).is_err());
    }

    #[test]
    fn locate_phantom_manifest_reports_every_layer_and_github_work_hint() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("phantom");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"agents":{"unconference":"m5"},"locate":{"orgs":["laris-co"]}}"#,
        );
        let mut github = LocateFakeGithub::default()
            .with("laris-co", Ok(vec!["unrelated".to_owned(), "unconference-oracle".to_owned()]));

        let output = run_locate_command_with_sessions(&["unconference".to_owned()], &[], &mut github);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(github.calls, vec!["laris-co".to_owned()]);
        assert!(output.stdout.contains("   manifest:  ✓ m5 (agents map)\n"), "{}", output.stdout);
        assert!(output.stdout.contains("   disk:      ✗ not cloned (ghq)\n"), "{}", output.stdout);
        assert!(output.stdout.contains("   session:   ✗ none\n"), "{}", output.stdout);
        assert!(output.stdout.contains("   registry:  ✗ none\n"), "{}", output.stdout);
        assert!(
            output.stdout.contains(
                "   github:    ✓ laris-co/unconference-oracle   → maw work https://github.com/laris-co/unconference-oracle\n"
            ),
            "{}",
            output.stdout
        );
    }

    #[test]
    fn locate_no_remote_skips_github_scan_entirely() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("no-remote");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"agents":{"unconference":"m5"}}"#,
        );
        let mut github = LocateFakeGithub::default();

        let output = run_locate_command_with_sessions(
            &["unconference".to_owned(), "--no-remote".to_owned()],
            &[],
            &mut github,
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(github.calls.is_empty(), "remote scanned despite --no-remote: {:?}", github.calls);
        assert!(output.stdout.contains("   github:    (skipped: --no-remote)\n"), "{}", output.stdout);
    }

    #[test]
    fn locate_remote_degrades_to_skip_and_never_breaks_local_answers() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("degrade");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"agents":{"unconference":"m5"}}"#,
        );
        let mut github = LocateFakeGithub::default()
            .with("Soul-Brews-Studio", Err("gh not installed".to_owned()))
            .with("laris-co", Ok(vec!["unconference-oracle".to_owned()]));

        let output = run_locate_command_with_sessions(&["unconference".to_owned()], &[], &mut github);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(github.calls, vec!["Soul-Brews-Studio".to_owned()], "gh-missing must short-circuit");
        assert!(output.stdout.contains("   github:    (skipped: gh not installed)\n"), "{}", output.stdout);
    }

    #[test]
    fn locate_remote_auth_failure_skips_with_reason_after_trying_all_orgs() {
        let mut github = LocateFakeGithub::default()
            .with("acme", Err("To get started with GitHub CLI, please run: gh auth login".to_owned()))
            .with("beta", Ok(Vec::new()));

        let layer = locate_scan_github("ghost", &["acme".to_owned(), "beta".to_owned()], &mut github);
        assert_eq!(github.calls, vec!["acme".to_owned(), "beta".to_owned()]);
        assert_eq!(layer.status, "skipped");
        assert_eq!(layer.reason.as_deref(), Some("To get started with GitHub CLI, please run: gh auth login"));
    }

    #[test]
    fn locate_remote_not_found_keeps_not_found_error_with_github_note() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("remote-miss");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"locate":{"orgs":["acme"]}}"#,
        );
        let mut github = LocateFakeGithub::default().with("acme", Ok(vec!["other".to_owned()]));

        let output = run_locate_command_with_sessions(&["ghost".to_owned()], &[], &mut github);
        assert_eq!(output.code, 1, "{}", output.stdout);
        assert!(output.stderr.contains("no oracle named 'ghost'"), "{}", output.stderr);
        assert!(output.stderr.contains("(github: not found in acme)"), "{}", output.stderr);
    }

    #[test]
    fn locate_default_orgs_apply_only_when_config_key_is_absent() {
        let absent = serde_json::json!({});
        assert_eq!(locate_remote_orgs(&absent), vec!["Soul-Brews-Studio".to_owned(), "laris-co".to_owned()]);

        let configured = serde_json::json!({"locate": {"orgs": ["acme", "beta"]}});
        assert_eq!(locate_remote_orgs(&configured), vec!["acme".to_owned(), "beta".to_owned()]);

        let empty = serde_json::json!({"locate": {"orgs": []}});
        assert_eq!(locate_remote_orgs(&empty), Vec::<String>::new());
        let layer = locate_scan_github("ghost", &[], &mut LocateFakeGithub::default());
        assert_eq!(layer.status, "skipped");
        assert_eq!(layer.reason.as_deref(), Some("locate.orgs is empty"));
    }

    #[test]
    fn locate_scan_refuses_option_injection_org_names() {
        let mut github = LocateFakeGithub::default();
        let layer = locate_scan_github("ghost", &["--bad-org".to_owned()], &mut github);
        assert!(github.calls.is_empty(), "gh invoked with unsafe org: {:?}", github.calls);
        assert_eq!(layer.status, "skipped");
        assert_eq!(layer.reason.as_deref(), Some("invalid org name '--bad-org'"));
    }

    #[test]
    fn locate_github_hit_on_disk_resolved_name_omits_work_hint() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("local-hit");
        let repo = env.ghq.join("github.com/acme/atlas-oracle");
        std::fs::create_dir_all(&repo).expect("repo");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"locate":{"orgs":["acme"]}}"#,
        );
        locate_write(
            &env.maw_config_path(&["fleet", "alpha.json"]),
            r#"{"name":"alpha","windows":[{"name":"atlas-oracle","repo":"acme/atlas-oracle"}]}"#,
        );
        let mut github = LocateFakeGithub::default().with("acme", Ok(vec!["atlas-oracle".to_owned()]));

        let output = run_locate_command_with_sessions(&["atlas".to_owned()], &[], &mut github);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains(&format!("   disk:      ✓ {}\n", repo.display())), "{}", output.stdout);
        assert!(output.stdout.contains("   registry:  ✓ "), "{}", output.stdout);
        assert!(output.stdout.contains("   github:    ✓ acme/atlas-oracle\n"), "{}", output.stdout);
        assert!(!output.stdout.contains("→ maw work"), "hint printed for a cloned repo: {}", output.stdout);
        assert!(output.stdout.contains("   repo:     "), "legacy fields must remain: {}", output.stdout);
    }

    #[test]
    fn locate_json_layers_report_github_and_local_truth() {
        let _guard = env_test_lock();
        let env = LocateHermeticEnv::new("json-layers");
        locate_write(
            &env.maw_config_path(&["maw.config.json"]),
            r#"{"agents":{"unconference":"m5"},"locate":{"orgs":["laris-co"]}}"#,
        );
        let mut github = LocateFakeGithub::default().with("laris-co", Ok(vec!["unconference-oracle".to_owned()]));

        let output = run_locate_command_with_sessions(
            &["unconference".to_owned(), "--json".to_owned()],
            &[],
            &mut github,
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        let rendered: serde_json::Value = serde_json::from_str(&output.stdout).expect("json");
        assert_eq!(rendered["layers"]["manifest"], "m5");
        assert_eq!(rendered["layers"]["disk"], serde_json::Value::Null);
        assert_eq!(rendered["layers"]["session"], serde_json::Value::Null);
        assert_eq!(rendered["layers"]["registry"], serde_json::Value::Null);
        assert_eq!(rendered["layers"]["github"]["status"], "found");
        assert_eq!(rendered["layers"]["github"]["repo"], "laris-co/unconference-oracle");
        assert_eq!(rendered["layers"]["github"]["url"], "https://github.com/laris-co/unconference-oracle");
    }
}
