fn snapshots_dir() -> std::path::PathBuf {
    maw_state_dir(&snapshots_xdg_env()).join("work-snapshots")
}

fn snapshots_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let keys = ["MAW_HOME", "MAW_STATE_DIR", "MAW_XDG", "XDG_STATE_HOME"];
    MawXdgEnv::with_vars(home, keys.into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key, value))))
}

fn snapshots_default_name() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("snapshot-{seconds}")
}

fn snapshots_file(dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    snapshots_validate_name(name)?;
    Ok(dir.join(format!("{name}.json")))
}

fn snapshots_list(dir: &std::path::Path, json: bool) -> Result<String, String> {
    let mut names = Vec::<String>::new();
    for entry in std::fs::read_dir(dir).map_err(|error| format!("snapshots: list: {error}"))?.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str) {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    if json {
        let body = names.iter().map(|name| json_string(name)).collect::<Vec<_>>().join(",");
        return Ok(format!("{{\"command\":\"snapshots\",\"snapshots\":[{body}]}}\n"));
    }
    Ok(if names.is_empty() { "no snapshots\n".to_owned() } else { format!("{}\n", names.join("\n")) })
}

fn snapshots_create(dir: &std::path::Path, name: &str, json: bool) -> Result<String, String> {
    let file = snapshots_file(dir, name)?;
    if file.exists() {
        return Err(format!("snapshots: snapshot exists: {name}"));
    }
    let cwd = std::env::current_dir().map_err(|error| format!("snapshots: cwd: {error}"))?;
    let body = format!("{{\"name\":{},\"cwd\":{},\"createdBy\":\"maw snapshots\"}}\n", json_string(name), json_string(&cwd.display().to_string()));
    std::fs::write(&file, &body).map_err(|error| format!("snapshots: write: {error}"))?;
    if json { Ok(body) } else { Ok(format!("created snapshot {name}\n")) }
}

fn snapshots_show(dir: &std::path::Path, name: &str, json: bool) -> Result<String, String> {
    let file = snapshots_file(dir, name)?;
    let body = std::fs::read_to_string(&file).map_err(|_| format!("snapshots: snapshot not found: {name}"))?;
    if json { Ok(body) } else { Ok(format!("{name}: {body}")) }
}

#[cfg(test)]
mod work_bundle_tests {
    include!("workspace_setup_work_bundle_tests/01_work_bundle_env_guard_to_promote_mutates_missi_ad5b6f.rs");
    include!("workspace_setup_work_bundle_tests/02_promote_refuses_ambig_557be9_to_preflight_json_repo_87c4bd.rs");
}
