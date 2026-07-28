// Turning what someone typed into exactly one window to wake.
//
// `maw wake rpro-ent` has to become a concrete (oracle, repo path, session,
// window). The fleet registry, the oracles cache and a ghq scan each know part
// of it, and the same alias can legitimately name several windows -- so this is
// mostly about refusing to guess: candidates are typed, ranked, and a tie is only
// broken when exactly one candidate is live (#711). Everything here is pure
// resolution; nothing in this file touches tmux.

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeRepoResolution {
    path: std::path::PathBuf,
    fuzzy_match: Option<String>,
    warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeRepoCandidate {
    name: String,
    path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedRegistryCandidate {
    candidate: maw_matcher::ResolveTypedCandidate,
    oracle: String,
    window: String,
    session: String,
    repo: String,
    repo_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedRepoCandidate {
    candidate: maw_matcher::ResolveTypedCandidate,
    path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedResolution {
    oracle: String,
    repo: WakeRepoResolution,
    session_hint: Option<String>,
    /// The literal name of the registry window the resolver actually
    /// matched, when the match came from an existing registry entry.
    /// `oracle` answers "whose identity is this" (repo-derived, used for
    /// display/session naming); this answers "which window do I act on."
    /// The two agree whenever a window happens to be named after its
    /// oracle, which is why the difference went unnoticed -- they diverge
    /// exactly in the fan-out case #711 describes, where sibling windows on
    /// one repo all derive the same oracle but are literally named
    /// differently. `None` for a fresh-spawn/repo-fuzzy match, where there
    /// is no existing window to preserve the identity of.
    matched_window: Option<String>,
}

impl maw_matcher::Named for WakeRepoCandidate {
    fn name(&self) -> &str { &self.name }
}

fn wake_oracle(options: &WakeOptionsNative) -> Result<String, String> {
    let slug = workon_github_slug(&options.target);
    let raw = options
        .name
        .as_deref()
        .or_else(|| slug.as_deref().and_then(|value| value.rsplit('/').next()))
        .or_else(|| options.target.trim_end_matches('/').split('/').next_back())
        .unwrap_or(&options.target);
    // Accept `session:window` (exactly one colon) -- the form wake's own
    // ambiguous-target error already prints back at the caller -- by taking
    // just the window part. This is only ever a degenerate fallback
    // identity: a colon target that matches a registry candidate resolves
    // via its literal name in wake_resolve_registry_target before this
    // value is used for anything but validation. Two or more colons is
    // `node:session:window` -- wake takes a node via `--peer`, never via the
    // positional target -- so that shape is left untouched here and still
    // rejected below: silently taking its last segment let a real local
    // repo/session sharing that segment's name resolve in place of the
    // node the caller actually named (#711 fix 5 follow-up).
    let raw = if raw.matches(':').count() == 1 { raw.rsplit(':').next().unwrap_or(raw) } else { raw };
    let raw = raw.strip_suffix(".git").unwrap_or(raw);
    let oracle = raw.strip_suffix("-oracle").unwrap_or(raw).trim();
    wake_validate_slug(oracle, "oracle")?;
    Ok(oracle.to_owned())
}

fn wake_typed_resolution(
    options: &WakeOptionsNative,
    oracle: &str,
    fleet_entries: &[NativeFleetEntry],
    sessions: &[TmuxSession],
) -> Result<Option<WakeTypedResolution>, String> {
    if wake_should_bypass_typed_resolution(options) { return Ok(None); }
    if let Some(resolution) = wake_resolve_exact_registry_session(&options.target, fleet_entries)? { return Ok(Some(resolution)); }
    if let Some(resolution) = wake_resolve_registry_target(&options.target, fleet_entries, sessions)? { return Ok(Some(resolution)); }
    wake_resolve_repo_target(oracle, fleet_entries).map(Some)
}

fn wake_should_bypass_typed_resolution(options: &WakeOptionsNative) -> bool {
    options.repo_path.is_some()
        || options.repo.is_some()
        || options.incubate.is_some()
        || workon_github_slug(&options.target).is_some()
        || options.target == "."
        || options.target.starts_with("./")
        || options.target.starts_with('/')
}

fn wake_repo_path(options: &WakeOptionsNative, oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeRepoResolution, String> {
    // `--repo-path <dir>` is an explicit filesystem override (used by `team up`
    // to point at the bound worktree) — it bypasses ghq/fleet resolution.
    if let Some(repo_path) = &options.repo_path {
        return wake_normalize_repo_path(repo_path).map(wake_exact_repo_resolution);
    }
    if let Some(repo) = &options.repo { return wake_resolve_workon_repo(repo); }
    if let Some(repo) = &options.incubate { return wake_resolve_workon_repo(repo); }
    if workon_github_slug(&options.target).is_some()
        || options.target == "."
        || options.target.starts_with("./")
        || options.target.starts_with('/')
    {
        return wake_resolve_workon_repo(&options.target);
    }
    wake_find_repo(oracle, fleet_entries)
}

fn wake_normalize_repo_path(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("wake: cannot resolve repo path: {error}"))?
            .join(path)
    };
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

fn wake_ghq_root() -> std::path::PathBuf { ghq_root() }

fn wake_exact_repo_resolution(path: std::path::PathBuf) -> WakeRepoResolution {
    WakeRepoResolution { path, fuzzy_match: None, warning: None }
}

fn wake_resolve_workon_repo(input: &str) -> Result<WakeRepoResolution, String> {
    let repo = workon_resolve_repo(input).map_err(|error| format!("wake: {error}"))?;
    Ok(wake_exact_repo_resolution(repo.repo_path))
}

fn wake_find_repo(oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeRepoResolution, String> {
    if let Some((repo_slug, path)) = wake_registry_repo_for_oracle(oracle, fleet_entries) {
        if path.is_dir() { return Ok(wake_exact_repo_resolution(path)); }
        if let Some((_, fallback)) = wake_registry_repo_fallback(&[oracle], &repo_slug) { return Ok(fallback); }
        return Err(wake_registry_missing_repo_message(oracle, &repo_slug, &path));
    }
    wake_resolve_repo_target(oracle, fleet_entries).map(|resolution| resolution.repo)
}

fn wake_resolve_exact_registry_session(target: &str, fleet_entries: &[NativeFleetEntry]) -> Result<Option<WakeTypedResolution>, String> {
    let matches = fleet_entries
        .iter()
        .filter(|entry| entry.session.name == target || entry.file == target)
        .collect::<Vec<_>>();
    let Some(entry) = matches.first() else { return Ok(None); };
    if matches.len() > 1 {
        return Err(format!("wake: ambiguous registry session for {target}"));
    }
    let stem = maw_identity::parse_session_name(&entry.session.name).stem;
    let Some(window) = wake_primary_registry_window(entry, &stem) else { return Ok(None); };
    let Some(path) = native_fleet_repo_path(&window.repo) else { return Ok(None); };
    let oracle = wake_oracle_from_repo_slug(&window.repo).unwrap_or_else(|| stem.clone());
    let (oracle, repo) = if path.is_dir() {
        (oracle, wake_exact_repo_resolution(path))
    } else {
        wake_registry_repo_fallback(&[&stem, &oracle], &window.repo)
            .ok_or_else(|| wake_registry_missing_repo_message(&entry.session.name, &window.repo, &path))?
    };
    Ok(Some(WakeTypedResolution {
        oracle,
        repo,
        session_hint: Some(entry.session.name.clone()),
        matched_window: Some(window.name.clone()),
    }))
}

fn wake_primary_registry_window<'a>(entry: &'a NativeFleetEntry, stem: &str) -> Option<&'a NativeFleetWindow> {
    entry
        .session
        .windows
        .iter()
        .find(|window| window.name == stem)
        .or_else(|| entry.session.windows.first())
}

fn wake_resolve_registry_target(
    target: &str,
    fleet_entries: &[NativeFleetEntry],
    sessions: &[TmuxSession],
) -> Result<Option<WakeTypedResolution>, String> {
    let candidates = wake_typed_registry_candidates(fleet_entries, sessions);
    let typed = candidates.iter().map(|candidate| candidate.candidate.clone()).collect::<Vec<_>>();
    match maw_matcher::resolve_typed_target(target, &typed) {
        maw_matcher::ResolveTypedResult::None => Ok(None),
        maw_matcher::ResolveTypedResult::Match { matched } => {
            let candidate = candidates
                .into_iter()
                .find(|candidate| candidate.candidate == matched.candidate)
                .ok_or_else(|| format!("wake: internal resolver mismatch for {target}"))?;
            let window = candidate.window.clone();
            let stem = maw_identity::parse_session_name(&candidate.session).stem;
            let (oracle, repo) = if candidate.repo_path.is_dir() {
                (candidate.oracle, wake_exact_repo_resolution(candidate.repo_path))
            } else {
                wake_registry_repo_fallback(&[target, &stem, &candidate.oracle], &candidate.repo)
                    .ok_or_else(|| wake_registry_missing_repo_message(&candidate.session, &candidate.repo, &candidate.repo_path))?
            };
            Ok(Some(WakeTypedResolution {
                oracle,
                repo,
                session_hint: Some(candidate.session),
                matched_window: Some(window),
            }))
        }
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => Err(format!(
            "wake: ambiguous registry target for {target}: {}",
            candidates.into_iter().map(|candidate| candidate.candidate.name).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn wake_resolve_repo_target(oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeTypedResolution, String> {
    let candidates = wake_typed_repo_candidates(fleet_entries);
    let typed = candidates.iter().map(|candidate| candidate.candidate.clone()).collect::<Vec<_>>();
    match maw_matcher::resolve_typed_target(oracle, &typed) {
        maw_matcher::ResolveTypedResult::Match { matched } => {
            let candidate = candidates
                .into_iter()
                .find(|candidate| candidate.candidate == matched.candidate)
                .ok_or_else(|| format!("wake: internal resolver mismatch for {oracle}"))?;
            let fuzzy_match = (matched.rank == maw_matcher::ResolveMatchRank::Fuzzy).then_some(candidate.candidate.name);
            let oracle = wake_oracle_from_repo_path(&candidate.path).unwrap_or_else(|| oracle.to_owned());
            Ok(WakeTypedResolution {
                oracle,
                repo: WakeRepoResolution { path: candidate.path, fuzzy_match, warning: None },
                session_hint: None,
                matched_window: None,
            })
        }
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => Err(format!(
            "wake: ambiguous fuzzy repo for {oracle}: {}",
            candidates.into_iter().map(|candidate| candidate.candidate.name).collect::<Vec<_>>().join(", ")
        )),
        maw_matcher::ResolveTypedResult::None => Err(wake_repo_not_found_message(oracle, &typed)),
    }
}

fn wake_repo_not_found_message(oracle: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> String {
    let mut all = candidates.to_vec();
    all.extend(deadend_oracle_candidates());
    let suggestions = deadend_suggestion_matches(oracle, &all);
    let mut out = deadend_suggestions_text("wake", oracle, &suggestions);
    out.push_str("  next: maw oracle scan  # refresh oracles.json\n  next: maw ls -a        # inspect live/sleeping sessions\n");
    out
}

fn wake_registry_repo_for_oracle(
    oracle: &str,
    fleet_entries: &[NativeFleetEntry],
) -> Option<(String, std::path::PathBuf)> {
    let mut repos = BTreeSet::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let repo = window.repo.strip_prefix("github.com/").unwrap_or(&window.repo);
            let Some(name) = repo.rsplit('/').next() else { continue; };
            if !wake_repo_name_matches(name, oracle) {
                continue;
            }
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            let _ = repos.insert((window.repo.clone(), wake_canonicalize_path(&path)));
        }
    }
    if repos.len() == 1 {
        repos.into_iter().next()
    } else {
        None
    }
}

fn wake_oracles_repo_fallback(names: &[&str]) -> Option<(String, WakeRepoResolution)> {
    let entry = locate_load_registry_cache()?.oracles.into_iter().find(|entry| {
        names.iter().any(|name| entry.name.eq_ignore_ascii_case(name))
    })?;
    let path = std::path::PathBuf::from(entry.local_path.trim());
    if !path.is_dir() { return None; }
    let path = wake_canonicalize_path(&path);
    let warning = format!("registry repo stale, using oracles.json: {}", path.display());
    Some((entry.name, WakeRepoResolution { path, fuzzy_match: None, warning: Some(warning) }))
}

fn wake_registry_repo_fallback(names: &[&str], recorded_repo: &str) -> Option<(String, WakeRepoResolution)> {
    let basename = recorded_repo.rsplit('/').next()?.trim();
    if !basename.is_empty() {
        if let Some(candidate) = wake_repo_candidates(&[])
            .into_iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(basename))
        {
            let oracle = wake_oracle_from_repo_path(&candidate.path).unwrap_or_else(|| basename.to_owned());
            let warning = format!(
                "registry repo {recorded_repo} not found; using disk basename match: {}",
                candidate.path.display()
            );
            return Some((
                oracle,
                WakeRepoResolution {
                    path: candidate.path,
                    fuzzy_match: None,
                    warning: Some(warning),
                },
            ));
        }
    }
    wake_oracles_repo_fallback(names)
}

fn wake_registry_session_hint(
    oracle: &str,
    repo_path: &std::path::Path,
    fleet_entries: &[NativeFleetEntry],
    sessions: &[TmuxSession],
) -> Option<String> {
    wake_resolve_registry_target(oracle, fleet_entries, sessions)
        .ok()
        .flatten()
        .filter(|resolution| wake_canonicalize_path(&resolution.repo.path) == wake_canonicalize_path(repo_path))
        .and_then(|resolution| resolution.session_hint)
}

// `sessions` decides whether a candidate is tagged SleepingRegistry or
// LiveSession, which in turn decides whether maw-matcher's live_tiebreak
// (#719) can prefer it in a same-rank alias tie -- #711 part 2's chosen
// policy: prefer the live window when exactly one candidate in a tie is
// live, otherwise stay ambiguous. Pass `&[]` for a check that must judge
// the registry's own naming, independent of what happens to be running
// right now (see fleet_resolvability_findings in fleet.rs, #714) -- a
// fleet with a genuinely ambiguous alias is worth flagging even while one
// window is transiently live to break the tie for wake's purposes.
fn wake_typed_registry_candidates(
    fleet_entries: &[NativeFleetEntry],
    sessions: &[TmuxSession],
) -> Vec<WakeTypedRegistryCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            let oracle = wake_oracle_from_repo_slug(&window.repo).unwrap_or_else(|| window.name.clone());
            let name = format!("{}:{}", entry.session.name, window.name);
            if !seen.insert((name.clone(), path.clone())) { continue; }
            let kind = if wake_registry_window_is_live(entry, window, sessions) {
                maw_matcher::ResolveCandidateKind::LiveSession
            } else {
                maw_matcher::ResolveCandidateKind::SleepingRegistry
            };
            candidates.push(WakeTypedRegistryCandidate {
                candidate: maw_matcher::ResolveTypedCandidate {
                    kind,
                    name,
                    aliases: wake_registry_aliases(window, &oracle),
                },
                oracle,
                window: window.name.clone(),
                session: entry.session.name.clone(),
                repo: window.repo.clone(),
                repo_path: path,
            });
        }
    }
    candidates
}

fn wake_registry_window_is_live(entry: &NativeFleetEntry, window: &NativeFleetWindow, sessions: &[TmuxSession]) -> bool {
    sessions
        .iter()
        .any(|session| session.name == entry.session.name && session.windows.iter().any(|live| live.name == window.name))
}

fn wake_registry_aliases(window: &NativeFleetWindow, oracle: &str) -> Vec<String> {
    let mut aliases = vec![window.name.clone(), oracle.to_owned()];
    if let Some(repo_name) = window.repo.rsplit('/').next().filter(|name| !name.is_empty()) { aliases.push(repo_name.to_owned()); }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn wake_typed_repo_candidates(fleet_entries: &[NativeFleetEntry]) -> Vec<WakeTypedRepoCandidate> {
    wake_repo_candidates(fleet_entries)
        .into_iter()
        .map(|candidate| WakeTypedRepoCandidate {
            candidate: maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::Repo,
                name: candidate.name,
                aliases: Vec::new(),
            },
            path: candidate.path,
        })
        .collect()
}

fn wake_repo_candidates(fleet_entries: &[NativeFleetEntry]) -> Vec<WakeRepoCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let root = wake_ghq_root().join("github.com");
    if let Ok(orgs) = std::fs::read_dir(root) {
        for org in orgs.flatten() { wake_collect_repo_candidates(&org.path(), &mut candidates, &mut seen); }
    }
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            wake_push_repo_candidate(path, &mut candidates, &mut seen);
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates
}

fn wake_collect_repo_candidates(
    org_path: &std::path::Path,
    candidates: &mut Vec<WakeRepoCandidate>,
    seen: &mut BTreeSet<std::path::PathBuf>,
) {
    let Ok(repos) = std::fs::read_dir(org_path) else { return; };
    for repo in repos.flatten() {
        let path = repo.path();
        if path.is_dir() { wake_push_repo_candidate(path, candidates, seen); }
    }
}

fn wake_push_repo_candidate(
    path: std::path::PathBuf,
    candidates: &mut Vec<WakeRepoCandidate>,
    seen: &mut BTreeSet<std::path::PathBuf>,
) {
    if !path.is_dir() || !seen.insert(path.clone()) { return; }
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { return; };
    candidates.push(WakeRepoCandidate { name: name.to_owned(), path });
}

fn wake_repo_name_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name == format!("{oracle}-oracle") || name.trim_end_matches("-oracle") == oracle
}

fn wake_oracle_from_repo_slug(repo: &str) -> Option<String> {
    let name = repo.rsplit('/').next()?.trim();
    (!name.is_empty()).then(|| name.strip_suffix("-oracle").unwrap_or(name).to_owned())
}

fn wake_oracle_from_repo_path(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| (!name.is_empty()).then(|| name.strip_suffix("-oracle").unwrap_or(name).to_owned()))
}

fn wake_registry_missing_repo_message(name: &str, repo: &str, path: &std::path::Path) -> String {
    let slug = repo.strip_prefix("github.com/").unwrap_or(repo);
    format!(
        "wake: registry entry for {name} exists, but its recorded repo {repo} is not cloned under {}; probed {}\n\
         → repo not cloned. clone it: maw work https://github.com/{slug}",
        wake_ghq_root().display(),
        path.display()
    )
}

fn wake_detect_session(oracle: &str, sessions: &[TmuxSession]) -> Option<String> {
    sessions.iter().find(|session| wake_session_matches(&session.name, oracle)).map(|session| session.name.clone())
}

fn wake_detect_session_from_fleet_registry(oracle: &str, repo_path: &std::path::Path, fleet_entries: &[NativeFleetEntry]) -> Option<String> {
    let canonical = wake_canonicalize_path(repo_path);
    let mut sessions = Vec::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let repo_name = window.repo.rsplit('/').next().unwrap_or_default();
            if !wake_repo_name_matches(repo_name, oracle) {
                continue;
            }
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            if wake_canonicalize_path(&path) == canonical {
                sessions.push(entry.session.name.clone());
            }
        }
    }
    sessions.sort();
    sessions.dedup();
    if sessions.len() == 1 { Some(sessions[0].clone()) } else { None }
}

fn wake_session_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name.ends_with(&format!("-{oracle}")) || name.ends_with(&format!("-{oracle}-oracle"))
}

fn wake_session_name(oracle: &str, sessions: &[TmuxSession]) -> String {
    let start = wake_slot(oracle);
    let mut slot = start;
    for _ in 0..80 {
        if !wake_session_slot_occupied(slot, sessions) {
            return format!("{slot:02}-{oracle}");
        }
        slot = (slot % 89) + 1;
        if slot < 10 {
            slot = 10;
        }
    }
    format!("{start:02}-{oracle}")
}

fn wake_session_slot_occupied(slot: u32, sessions: &[TmuxSession]) -> bool {
    let prefix = format!("{slot:02}-");
    sessions.iter().any(|session| session.name.starts_with(&prefix))
}

fn wake_canonicalize_path(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn wake_slot(oracle: &str) -> u32 {
    let mut hash = 0_u32;
    for byte in oracle.bytes() { hash = hash.wrapping_mul(33).wrapping_add(u32::from(byte)); }
    10 + (hash % 80)
}

fn wake_window_name(options: &WakeOptionsNative, oracle: &str, matched_window: Option<&str>) -> String {
    let suffix = options.wt.as_deref().or(options.task.as_deref()).map(wake_sanitize_branch);
    match suffix {
        // `--wt`/`--task` asks for a derived window, not the one that was
        // matched -- oracle-derived naming applies regardless of a match.
        Some(task) => format!("{oracle}-{task}"),
        None => matched_window.map_or_else(|| oracle.to_owned(), str::to_owned),
    }
}

fn wake_sanitize_branch(value: &str) -> String {
    value.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '-' }).collect()
}
