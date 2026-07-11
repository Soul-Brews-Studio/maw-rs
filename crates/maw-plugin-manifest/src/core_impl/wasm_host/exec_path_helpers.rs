fn is_safe_ssh_host_token(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-'))
}

fn is_safe_tmux_target_token(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-'))
}

fn protected_file(path: PathBuf) -> ProtectedPath {
    ProtectedPath {
        path,
        kind: ProtectedPathKind::File,
    }
}
fn protected_dir(path: PathBuf) -> ProtectedPath {
    ProtectedPath {
        path,
        kind: ProtectedPathKind::Dir,
    }
}

fn resolve_protected_path(protected: ProtectedPath) -> Result<ProtectedPath, HostResult<Value>> {
    if protected.path.exists() {
        Ok(ProtectedPath {
            path: canonicalize_checked_path(&protected.path)?,
            kind: protected.kind,
        })
    } else {
        Ok(protected)
    }
}

fn path_is_protected_security_state(path: &Path, protected: &[ProtectedPath]) -> bool {
    protected
        .iter()
        .any(|protected_path| match protected_path.kind {
            ProtectedPathKind::File => path == protected_path.path,
            ProtectedPathKind::Dir => path.starts_with(&protected_path.path),
        })
}

fn resolve_write_path(raw: &Path) -> Result<PathBuf, HostResult<Value>> {
    if std::fs::symlink_metadata(raw).is_ok() {
        return canonicalize_checked_path(raw);
    }
    let parent = raw
        .parent()
        .ok_or_else(|| HostResult::err(HostErrorCode::InvalidArgs, "write path requires parent"))?;
    let parent = canonicalize_checked_path(parent)?;
    let file_name = raw.file_name().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "write path requires file name")
    })?;
    Ok(parent.join(file_name))
}

fn executable_basename(cmd: &str) -> String {
    Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
        .to_owned()
}
fn is_hard_denied_exec(cmd: &str, args: &[String]) -> bool {
    matches!(
        executable_basename(cmd).as_str(),
        "sudo" | "su" | "doas" | "pkexec"
    ) || args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--pty" | "--ffi"))
}
fn sanitize_env(
    env: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, HostResult<Value>> {
    let mut clean = BTreeMap::new();
    clean.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
    if let Some(env) = env {
        for (key, value) in env {
            let lower = key.to_lowercase();
            if lower.contains("secret") || lower.contains("token") || lower.contains("peerkey") {
                return Err(HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "secret-like env keys are denied",
                ));
            }
            if key.starts_with("MAW_") {
                clean.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(clean)
}

fn run_child(
    mut cmd: Command,
    stdin: Option<&str>,
    timeout_ms: u64,
) -> Result<std::process::Output, HostErrorCode> {
    let mut child = cmd.spawn().map_err(|_| HostErrorCode::ProcessFailed)?;
    if let Some(input) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(input.as_bytes())
                .map_err(|_| HostErrorCode::IoError)?;
        }
    }
    let deadline = Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|_| HostErrorCode::IoError),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HostErrorCode::Timeout);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => return Err(HostErrorCode::ProcessFailed),
        }
    }
}

fn canonicalize_checked(path: &str) -> Result<PathBuf, HostResult<Value>> {
    canonicalize_checked_path(Path::new(path))
}
fn canonicalize_checked_path(path: &Path) -> Result<PathBuf, HostResult<Value>> {
    std::fs::canonicalize(path).map_err(|error| {
        HostResult::err(
            if error.kind() == std::io::ErrorKind::NotFound {
                HostErrorCode::NotFound
            } else {
                HostErrorCode::IoError
            },
            format!("canonicalize failed: {error}"),
        )
    })
}
fn deny_special_path(path: &Path) -> bool {
    path.starts_with("/proc")
        || path.starts_with("/dev")
        || path.starts_with("/sys")
        || path.starts_with("/root")
}
fn default_config_root(config_root: Option<&Path>) -> PathBuf {
    if let Some(path) = config_root {
        return path.to_path_buf();
    }
    if let Some(path) = std::env::var_os("MAW_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("MAW_HOME") {
        return PathBuf::from(path).join("config");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".config").join("maw"),
        |home| PathBuf::from(home).join(".config").join("maw"),
    )
}
fn default_state_root() -> PathBuf {
    if let Some(path) = std::env::var_os("MAW_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("MAW_HOME") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("maw");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".local").join("state").join("maw"),
        |home| PathBuf::from(home).join(".local").join("state").join("maw"),
    )
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Fixed registry mapping a manifest capability scope NAME to an absolute host
/// path, rooted at `home`. This is the ONLY source of filesystem roots for
/// `maw.fs.*` in production: a manifest may name one of these scopes but can
/// never inject a path of its own. Extend by adding an arm here — never by
/// reading a path from a manifest.
fn known_fs_root(
    scope: &str,
    home: &Path,
    config_root: Option<&Path>,
    vault_root: Option<&Path>,
) -> Option<PathBuf> {
    match scope {
        "teams" => Some(home.join(".claude").join("teams")),
        "claude-projects" => Some(configured_claude_projects_root(home)),
        "repos" => Some(configured_repos_root(home)),
        "cwd" => std::env::current_dir().ok(),
        "maw-cache" => Some(configured_maw_cache_root(home)),
        "maw-legacy" => Some(home.join(".maw")),
        "fleet-state" => Some(default_state_root().join("fleet")),
        "fleet-legacy" => Some(home.join(".maw").join("fleet")),
        "fleet-config" => Some(default_config_root(config_root).join("fleet")),
        "vault" => configured_vault_root(home, config_root, vault_root).ok(),
        _ => None,
    }
}

fn known_fs_root_should_create(scope: &str) -> bool {
    matches!(scope, "teams" | "maw-cache")
}

fn configured_maw_cache_root(home: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("MAW_HOME").filter(|value| !value.is_empty()) {
        return resolve_configured_path(PathBuf::from(path), home);
    }
    if let Some(path) = std::env::var_os("MAW_CACHE_DIR").filter(|value| !value.is_empty()) {
        return resolve_configured_path(PathBuf::from(path), home);
    }
    if std::env::var("MAW_XDG").is_ok_and(|value| {
        matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
    }) {
        return std::env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(|| home.join(".cache"), PathBuf::from)
            .join("maw");
    }
    home.join(".maw")
}

fn configured_repos_root(home: &Path) -> PathBuf {
    std::env::var_os("GHQ_ROOT").filter(|value| !value.is_empty()).map_or_else(
        || home.join("Code").join("github.com"),
        |value| {
            let root = resolve_configured_path(PathBuf::from(value), home);
            if root.file_name().and_then(std::ffi::OsStr::to_str) == Some("github.com") { root } else { root.join("github.com") }
        },
    )
}

fn configured_claude_projects_root(home: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("MAW_CLAUDE_PROJECTS_DIR").filter(|value| !value.is_empty()) {
        return resolve_configured_path(PathBuf::from(path), home);
    }
    if let Some(path) = std::env::var_os("CLAUDE_HOME").filter(|value| !value.is_empty()) {
        return resolve_configured_path(PathBuf::from(path), home).join("projects");
    }
    home.join(".claude").join("projects")
}

fn configured_vault_root(
    home: &Path,
    config_root: Option<&Path>,
    vault_root: Option<&Path>,
) -> Result<PathBuf, HostResult<Value>> {
    if let Some(root) = vault_root {
        return Ok(resolve_configured_path(root.to_path_buf(), home));
    }
    if let Some(root) = std::env::var_os("MAW_VAULT_ROOT").filter(|value| !value.is_empty()) {
        return Ok(resolve_configured_path(PathBuf::from(root), home));
    }
    let config = read_config_json(&default_config_root(config_root).join("maw.config.json"))?;
    for key in ["vaultRoot", "vault.root"] {
        if let Some(root) = get_json_path(&config, key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(resolve_configured_path(PathBuf::from(root), home));
        }
    }
    Err(HostResult::err(
        HostErrorCode::NotFound,
        "vault root is not configured; set MAW_VAULT_ROOT or maw.config.json vaultRoot",
    ))
}

fn resolve_configured_path(path: PathBuf, home: &Path) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        home.join(path)
    }
}

fn contains_glob_pattern(path: &str) -> bool {
    path.chars()
        .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}
