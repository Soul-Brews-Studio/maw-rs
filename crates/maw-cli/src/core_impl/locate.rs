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
    repo_path: Option<String>,
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
    local_path: String,
    has_psi: bool,
    has_fleet_config: bool,
    federation_node: Option<String>,
}

fn run_locate_command(argv: &[String]) -> CliOutput {
    match locate_parse_args(argv) {
        Ok((oracle, opts)) => match locate_cmd(&oracle, &opts) {
            Ok(stdout) => CliOutput {
                code: 0,
                stdout,
                stderr: String::new(),
            },
            Err(message) => CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("{message}\n"),
            },
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

fn locate_cmd(oracle: &str, opts: &LocateOptions) -> Result<String, String> {
    let mut tmux = TmuxClient::local();
    let sessions = tmux.list_all();
    locate_cmd_with_sessions(oracle, opts, &sessions)
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

fn locate_gather_info(
    oracle: &str,
    scan_federation: bool,
    sessions: &[TmuxSession],
) -> Result<LocateResult, String> {
    locate_validate_name(oracle)?;
    let repo_path = locate_ghq_find(&format!("/{oracle}-oracle"))
        .or_else(|| locate_ghq_find(&format!("/{oracle}")));
    let has_psi = repo_path
        .as_deref()
        .is_some_and(|path| std::path::Path::new(path).join("ψ").exists());
    let (session_name, window_count) = locate_resolve_session(oracle, sessions)
        .map_or((None, 0), |session| (Some(session.name.clone()), session.windows.len()));
    let fleet_config_path = locate_find_fleet_config_path(oracle, session_name.as_deref());
    let manifest_entry = locate_lookup_manifest_entry(oracle);
    let config = locate_load_config();
    let in_agents_config = config.agents.contains_key(oracle);
    let federation_node = if in_agents_config {
        config.agents.get(oracle).cloned()
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

    Ok(LocateResult {
        name: oracle.to_owned(),
        repo_path: repo_path.or_else(|| manifest_entry.as_ref().and_then(|entry| entry.local_path.clone())),
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

fn locate_resolve_session<'a>(oracle: &str, sessions: &'a [TmuxSession]) -> Option<&'a TmuxSession> {
    let wanted = locate_normalized_names(oracle);
    sessions.iter().find(|session| locate_session_matches(session, &wanted))
}

fn locate_session_matches(session: &TmuxSession, wanted: &BTreeSet<String>) -> bool {
    locate_normalized_names(&session.name)
        .iter()
        .any(|name| wanted.contains(name))
        || session.windows.iter().any(|locate_window| {
            locate_normalized_names(&locate_window.name)
                .iter()
                .any(|name| wanted.contains(name))
        })
}

fn locate_normalized_names(name: &str) -> BTreeSet<String> {
    let raw = name.trim().to_lowercase();
    let unnumbered = raw
        .split_once('-')
        .filter(|(prefix, _)| prefix.chars().all(|ch| ch.is_ascii_digit()))
        .map_or(raw.as_str(), |(_, tail)| tail);
    [
        raw.clone(),
        raw.strip_suffix("-oracle").unwrap_or(&raw).to_owned(),
        unnumbered.to_owned(),
        unnumbered.strip_suffix("-oracle").unwrap_or(unnumbered).to_owned(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect()
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
            .any(|locate_window| names.contains(locate_window.name.as_str()))
}

fn locate_load_fleet_entries() -> Vec<LocateFleetEntry> {
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else {
        return Vec::new();
    };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let session = serde_json::from_str::<NativeFleetSession>(&text).ok()?;
            Some(LocateFleetEntry {
                file: path.file_name()?.to_str()?.to_owned(),
                path: path_string(path),
                session,
            })
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
            let Some(name) = locate_name_from_window(&locate_window.name) else {
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

fn locate_name_from_window(window_name: &str) -> Option<String> {
    let name = window_name.strip_suffix("-oracle").unwrap_or(window_name).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

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
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    let Ok(text) = std::fs::read_to_string(path) else {
        return LocateConfig::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return LocateConfig::default();
    };
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
    fn locate_rejects_option_injection_targets() {
        assert!(locate_parse_args(&["--json".to_owned(), "-bad".to_owned()]).is_err());
        assert!(locate_parse_args(&["../bad".to_owned()]).is_err());
        assert!(locate_validate_name("good-oracle").is_ok());
    }
}
