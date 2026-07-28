// The only place a fleet registry file is written.
//
// Every other verb reads the registry; upsert is the single writer, so it owns
// the awkward parts: merging a live tmux window list into an existing entry
// without dropping fields it does not understand, canonicalising repo slugs that
// arrive in three historical shapes, deduping aliased windows, and writing
// atomically so a crash cannot leave a half-registry behind.

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetRegistryWrite {
    path: std::path::PathBuf,
    created: bool,
    window_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetWindowRepair {
    session: String,
    path: std::path::PathBuf,
    removed: usize,
}

fn fleet_registry_upsert_session(
    session: &str,
    windows: &[FleetWindowSummary],
    created_by: &str,
) -> Result<FleetRegistryWrite, String> {
    fleet_registry_upsert_session_for_env(&current_xdg_env(), session, windows, created_by)
}

fn fleet_registry_upsert_session_for_env(
    env: &MawXdgEnv,
    session: &str,
    windows: &[FleetWindowSummary],
    created_by: &str,
) -> Result<FleetRegistryWrite, String> {
    fleet_validate_session_name(session)?;
    let dir = env.home_dir().join(".maw").join("fleet");
    std::fs::create_dir_all(&dir).map_err(|error| format!("fleet registry: create {}: {error}", dir.display()))?;

    let mut windows_by_repo: BTreeSet<String> = BTreeSet::new();
    for window in windows {
        windows_by_repo.insert(fleet_repo_canonical_key(&window.repo));
    }
    let target_stem = fleet_session_stem(session);
    // Duplicate guard (#299): an entry that already owns this exact session
    // name always wins — a session revived from the registry by `maw wake`
    // (#312) must update its own file, never merge into a same-stem sibling.
    // Only when no exact entry exists does the write fold into a same-stem
    // entry whose windows overlap on canonical repo path. Loading is
    // best-effort (non-strict): a corrupt unrelated registry file must not
    // fail wake/fleet-add registration.
    let entries = fleet_load_entries_for_env(env);
    let path = entries
        .iter()
        .find(|entry| fleet_entry_is_session(entry) && entry.session.name == session)
        .or_else(|| {
            entries.iter().find(|entry| {
                fleet_entry_is_session(entry)
                    && fleet_session_stem(&entry.session.name) == target_stem
                    && entry
                        .session
                        .windows
                        .iter()
                        .any(|window| windows_by_repo.contains(&fleet_repo_canonical_key(&window.repo)))
            })
        })
        .map_or_else(|| dir.join(format!("{session}.json")), |entry| entry.path.clone());

    let (created, mut value) = fleet_registry_read_value(&path)?;
    {
        let object = fleet_registry_object(&mut value);
        object.insert("name".to_owned(), serde_json::json!(session));
        object
            .entry("created_at".to_owned())
            .or_insert_with(|| serde_json::json!(fleet_registry_now_iso()));
        object.insert("created_by".to_owned(), serde_json::json!(created_by));
        object.insert(
            "auto_registered".to_owned(),
            serde_json::json!(created_by != "maw fleet add"),
        );
        let merged = fleet_registry_merge_windows(object.get("windows"), windows);
        object.insert("windows".to_owned(), serde_json::json!(merged));
    }
    let body = serde_json::to_string_pretty(&value).map_err(|error| format!("fleet registry: render json: {error}"))? + "\n";
    std::fs::write(&path, body).map_err(|error| format!("fleet registry: write {}: {error}", path.display()))?;
    let window_count = value
        .get("windows")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    Ok(FleetRegistryWrite { path, created, window_count })
}

fn fleet_registry_read_value(path: &std::path::Path) -> Result<(bool, serde_json::Value), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok((
            false,
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((true, serde_json::json!({}))),
        Err(error) => Err(format!("fleet registry: read {}: {error}", path.display())),
    }
}

fn fleet_session_stem(value: &str) -> &str {
    value
        .split_once('-')
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(value, |(_, stem)| stem)
}

fn fleet_repo_canonical_key(repo: &str) -> String {
    // Canonicalize when the repo is cloned (resolves symlinked checkouts);
    // otherwise fall back to the ghq path so `acme/x` and `github.com/acme/x`
    // still hash to the same key.
    native_fleet_repo_path(repo).map_or_else(
        || repo.to_owned(),
        |path| {
            let path = path.canonicalize().unwrap_or(path);
            path.to_string_lossy().to_string()
        },
    )
}

fn fleet_fix_duplicate_windows(entries: &[NativeFleetEntry], live: &[TmuxSession]) -> Result<Vec<FleetWindowRepair>, String> {
    let mut repairs = Vec::new();
    for entry in entries.iter().filter(|entry| fleet_entry_is_session(entry)) {
        let text = std::fs::read_to_string(&entry.path).map_err(|error| format!("fleet doctor: read {}: {error}", entry.path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("fleet doctor: parse {}: {error}", entry.path.display()))?;
        let Some(windows) = value.get("windows").and_then(serde_json::Value::as_array) else { continue };
        let mergeable = fleet_windows_by_repo(entry)
            .into_iter()
            .filter(|(_, windows)| {
                windows.len() > 1
                    && fleet_windows_share_alias(windows)
                    && fleet_distinct_live_window_ids(entry, windows, live).len() <= 1
            })
            .map(|(repo, _)| repo)
            .collect::<BTreeSet<_>>();
        let (deduped, removed) = fleet_dedupe_window_values(windows, &mergeable);
        if removed == 0 { continue; }
        fleet_registry_object(&mut value).insert("windows".to_owned(), serde_json::json!(deduped));
        fleet_write_json_atomic(&entry.path, &value)?;
        repairs.push(FleetWindowRepair { session: entry.session.name.clone(), path: entry.path.clone(), removed });
    }
    Ok(repairs)
}

fn fleet_dedupe_window_values(windows: &[serde_json::Value], mergeable: &BTreeSet<String>) -> (Vec<serde_json::Value>, usize) {
    let mut by_repo = BTreeMap::<String, Vec<usize>>::new();
    for (index, window) in windows.iter().enumerate() {
        if let Some(repo) = window.get("repo").and_then(serde_json::Value::as_str).filter(|repo| !repo.trim().is_empty()) {
            by_repo.entry(fleet_repo_canonical_key(repo)).or_default().push(index);
        }
    }
    let mut remove = BTreeSet::new();
    for indices in by_repo.iter().filter(|(repo, indices)| mergeable.contains(*repo) && indices.len() > 1).map(|(_, indices)| indices) {
        // Prefer descriptive typed metadata; array order breaks ties because
        // auto-registration appends newer observations after older ones.
        let kept = indices
            .iter()
            .rev()
            .find(|&&index| windows[index].get("kind").and_then(serde_json::Value::as_str).and_then(native_repo_kind_from_role).is_some())
            .copied()
            .unwrap_or(indices[indices.len() - 1]);
        remove.extend(indices.iter().copied().filter(|index| *index != kept));
    }
    let deduped = windows.iter().enumerate().filter(|(index, _)| !remove.contains(index)).map(|(_, window)| window.clone()).collect();
    (deduped, remove.len())
}

fn fleet_write_json_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp-{}-{seq}", std::process::id()));
    let body = serde_json::to_string_pretty(value).map_err(|error| format!("fleet doctor: render json: {error}"))? + "\n";
    std::fs::write(&tmp, body).map_err(|error| format!("fleet doctor: write {}: {error}", tmp.display()))?;
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Err(error) = std::fs::set_permissions(&tmp, metadata.permissions()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("fleet doctor: set permissions {}: {error}", tmp.display()));
        }
    }
    std::fs::rename(&tmp, path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp);
        format!("fleet doctor: replace {}: {error}", path.display())
    })
}

fn fleet_registry_object(value: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    if !value.is_object() {
        *value = serde_json::json!({});
    }
    let serde_json::Value::Object(object) = value else {
        unreachable!("fleet registry value must be an object because non-object values are replaced immediately above");
    };
    object
}

fn fleet_registry_merge_windows(
    existing: Option<&serde_json::Value>,
    updates: &[FleetWindowSummary],
) -> Vec<serde_json::Value> {
    let mut windows = Vec::<FleetWindowSummary>::new();
    if let Some(existing) = existing.and_then(serde_json::Value::as_array) {
        for item in existing {
            let Some(name) = item.get("name").and_then(serde_json::Value::as_str).filter(|value| !value.trim().is_empty()) else {
                continue;
            };
            let repo = item.get("repo").and_then(serde_json::Value::as_str).unwrap_or_default();
            let kind = item.get("kind").and_then(serde_json::Value::as_str).and_then(native_repo_kind_from_role);
            windows.push(FleetWindowSummary {
                name: name.to_owned(),
                repo: fleet_repo_storage_slug(repo),
                kind,
            });
        }
    }
    let existing_repo_counts = windows.iter().fold(BTreeMap::<String, usize>::new(), |mut counts, window| {
        *counts.entry(fleet_repo_canonical_key(&window.repo)).or_default() += 1;
        counts
    });
    let update_repo_counts = updates
        .iter()
        .filter(|window| !window.name.trim().is_empty())
        .fold(BTreeMap::<String, usize>::new(), |mut counts, window| {
            *counts.entry(fleet_repo_canonical_key(&window.repo)).or_default() += 1;
            counts
        });
    for update in updates.iter().filter(|window| !window.name.trim().is_empty()) {
        let mut update = update.clone();
        update.repo = fleet_repo_storage_slug(&update.repo);
        let repo_key = fleet_repo_canonical_key(&update.repo);
        // A single live tmux window and a single registry window for the
        // same canonical repo are two names for one physical window, not a
        // reason to append a second entry (#457).
        let lone_repo_alias =
            existing_repo_counts.get(&repo_key) == Some(&1) && update_repo_counts.get(&repo_key) == Some(&1);
        if let Some(existing) = windows.iter_mut().find(|window| window.name == update.name) {
            existing.repo.clone_from(&update.repo);
            if update.kind.is_some() {
                existing.kind = update.kind;
            }
        } else if lone_repo_alias {
            if let Some(existing) = windows
                .iter_mut()
                .find(|window| fleet_repo_canonical_key(&window.repo) == repo_key) {
                *existing = update;
            } else {
                windows.push(update);
            }
        } else {
            windows.push(update);
        }
    }
    windows
        .into_iter()
        .map(|window| fleet_json_window(&window))
        .collect()
}

fn fleet_repo_storage_slug(repo: &str) -> String {
    repo.trim().strip_prefix("github.com/").unwrap_or(repo.trim()).to_owned()
}

fn fleet_registry_windows_from_tmux(
    windows: &[maw_tmux::TmuxWindow],
    repos_root: Option<&std::path::Path>,
) -> Vec<FleetWindowSummary> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for window in windows {
        let Some(cwd) = window.cwd.as_deref().and_then(|cwd| fleet_repo_slug_from_path(std::path::Path::new(cwd), repos_root)) else {
            continue;
        };
        let name = if window.name.is_empty() { "main".to_owned() } else { window.name.clone() };
        if seen.insert(name.clone()) {
            let kind = Some(fleet_kind_from_window_name(&name));
            result.push(FleetWindowSummary { name, repo: cwd, kind });
        }
    }
    result
}

fn fleet_kind_from_window_name(name: &str) -> NativeRepoKind {
    if name.trim().ends_with("-oracle") { NativeRepoKind::Oracle } else { NativeRepoKind::Project }
}

fn native_repo_kind_label(kind: NativeRepoKind) -> &'static str {
    match kind {
        NativeRepoKind::Oracle => "oracle",
        NativeRepoKind::Project => "project",
    }
}

fn fleet_repo_slug_from_path(path: &std::path::Path, repos_root: Option<&std::path::Path>) -> Option<String> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(root) = repos_root {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if let Ok(rel) = path.strip_prefix(root) {
            return fleet_repo_slug_from_components(rel.components());
        }
    }
    let mut saw_github = false;
    let mut parts = Vec::new();
    for component in path.components() {
        let value = component.as_os_str().to_string_lossy();
        if saw_github {
            parts.push(value.to_string());
            if parts.len() == 2 {
                return Some(format!("github.com/{}/{}", parts[0], parts[1]));
            }
        } else if value == "github.com" {
            saw_github = true;
        }
    }
    None
}

fn fleet_repo_slug_from_components(mut components: std::path::Components<'_>) -> Option<String> {
    let org = components.next()?.as_os_str().to_string_lossy();
    let repo = components.next()?.as_os_str().to_string_lossy();
    Some(format!("github.com/{org}/{repo}"))
}

fn fleet_registry_now_iso() -> String {
    if let Ok(value) = std::env::var("MAW_RS_FLEET_REGISTRY_NOW") {
        return value;
    }
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    let (year, month, day, hour, minute, sec) = epoch_secs_to_ymd_hms(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{sec:02}.000Z")
}

fn fleet_validate_session_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        Err("fleet: invalid session".to_owned())
    } else {
        Ok(())
    }
}
