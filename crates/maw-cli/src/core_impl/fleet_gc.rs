// Retiring registry entries whose repo is gone.
//
// An auto-registered session whose repos no longer exist on disk is dead weight,
// but a hand-written entry might be intentional -- so gc only disables what it
// can show is stale, renames rather than deletes, and defaults to a dry run.

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGcCandidate {
    name: String,
    path: std::path::PathBuf,
    disabled_path: std::path::PathBuf,
    missing_repos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetGcResult {
    name: String,
    path: std::path::PathBuf,
    disabled_path: std::path::PathBuf,
    missing_repos: Vec<String>,
    status: String,
    detail: Option<String>,
}

fn fleet_run_gc<R: maw_tmux::TmuxRunner>(
    state: &FleetState,
    options: &FleetOptions,
    runner: &mut R,
) -> Result<(i32, String), String> {
    let live = fleet_live_session_names(runner)?;
    let candidates = fleet_gc_candidates(state, &live);
    let results = if options.dry_run {
        candidates
            .into_iter()
            .map(|candidate| fleet_gc_result(candidate, "planned", None))
            .collect::<Vec<_>>()
    } else {
        fleet_apply_gc_candidates(candidates)
    };
    let code = i32::from(results.iter().any(|result| result.status == "failed"));
    if options.json {
        return Ok((code, fleet_json_gc(state, options, &live, &results)?));
    }
    Ok((code, fleet_render_gc(state, options, &live, &results)))
}

fn fleet_live_session_names<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<BTreeSet<String>, String> {
    let args = ["-F".to_owned(), "#{session_name}".to_owned()];
    let raw = match runner.run("list-sessions", &args) {
        Ok(raw) => raw,
        Err(error) if error.message.contains("no server running") => String::new(),
        Err(error) => return Err(format!("fleet gc: cannot list tmux sessions: {}", error.message)),
    };
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn fleet_gc_candidates(state: &FleetState, live: &BTreeSet<String>) -> Vec<FleetGcCandidate> {
    let mut candidates = Vec::new();
    for entry in state.fleet_entries.iter().filter(|entry| fleet_entry_is_session(entry)) {
        if live.contains(&entry.session.name) {
            continue;
        }
        let repos = fleet_session_repo_slugs(&entry.session);
        let missing = repos
            .iter()
            .filter(|repo| !fleet_repo_path(&state.repos_root, repo).exists())
            .cloned()
            .collect::<Vec<_>>();
        let should_reap = match fleet_entry_auto_registered(entry) {
            Some(auto_registered) => auto_registered,
            None => !repos.is_empty() && missing.len() == repos.len(),
        };
        if !should_reap {
            continue;
        }
        candidates.push(FleetGcCandidate {
            name: entry.session.name.clone(),
            path: entry.path.clone(),
            disabled_path: fleet_disabled_path(&entry.path),
            missing_repos: missing,
        });
    }
    candidates
}

fn fleet_entry_auto_registered(entry: &NativeFleetEntry) -> Option<bool> {
    std::fs::read_to_string(&entry.path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("auto_registered").and_then(serde_json::Value::as_bool))
}

fn fleet_session_repo_slugs(session: &NativeFleetSession) -> Vec<String> {
    let mut repos = session
        .windows
        .iter()
        .map(|window| window.repo.trim())
        .filter(|repo| !repo.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    repos.sort();
    repos.dedup();
    repos
}

fn fleet_apply_gc_candidates(candidates: Vec<FleetGcCandidate>) -> Vec<FleetGcResult> {
    candidates
        .into_iter()
        .map(|candidate| {
            if candidate.disabled_path.exists() {
                return fleet_gc_result(candidate, "skipped", Some("disabled file already exists".to_owned()));
            }
            match std::fs::rename(&candidate.path, &candidate.disabled_path) {
                Ok(()) => fleet_gc_result(candidate, "disabled", None),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => fleet_gc_result(candidate, "skipped", Some("source file is already gone".to_owned())),
                Err(error) => fleet_gc_result(candidate, "failed", Some(error.to_string())),
            }
        })
        .collect()
}

fn fleet_gc_result(candidate: FleetGcCandidate, status: &str, detail: Option<String>) -> FleetGcResult {
    FleetGcResult {
        name: candidate.name,
        path: candidate.path,
        disabled_path: candidate.disabled_path,
        missing_repos: candidate.missing_repos,
        status: status.to_owned(),
        detail,
    }
}

fn fleet_render_gc(
    state: &FleetState,
    options: &FleetOptions,
    live: &BTreeSet<String>,
    results: &[FleetGcResult],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fleet gc node {}", state.config.node);
    let _ = writeln!(out, "  live sessions: {}", live.len());
    let _ = writeln!(out, "  candidates: {}", results.len());
    if results.is_empty() {
        out.push_str("  ok: no stale fleet entries\n");
        return out;
    }
    for result in results {
        let verb = if options.dry_run {
            "[dry-run] would disable"
        } else {
            result.status.as_str()
        };
        let _ = write!(
            out,
            "  - {verb} {} -> {}",
            result.path.display(),
            result.disabled_path.display()
        );
        if !result.missing_repos.is_empty() {
            let _ = write!(out, " (missing repos: {})", result.missing_repos.join(", "));
        }
        if let Some(detail) = &result.detail {
            let _ = write!(out, " ({detail})");
        }
        out.push('\n');
    }
    out
}

fn fleet_json_gc(
    state: &FleetState,
    options: &FleetOptions,
    live: &BTreeSet<String>,
    results: &[FleetGcResult],
) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "dryRun": options.dry_run,
        "liveSessionCount": live.len(),
        "candidateCount": results.len(),
        "candidates": results.iter().map(fleet_json_gc_result).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_json_gc_result(result: &FleetGcResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "path": result.path,
        "disabledPath": result.disabled_path,
        "missingRepos": result.missing_repos,
        "status": result.status,
        "detail": result.detail,
    })
}
