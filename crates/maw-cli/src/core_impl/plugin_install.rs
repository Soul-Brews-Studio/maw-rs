// Getting a plugin from wherever it lives onto this box.
//
// A source may be a local directory, a git URL or `owner/repo/pkg` shorthand,
// and each has to end up as the same thing: a verified package directory. The
// verification is the point -- a shipped wasm artifact is checked against its
// declared sha256 before it is ever placed, so a tampered or truncated download
// fails at install rather than at first invoke.

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallSource {
    Git {
        url: String,
        reference: Option<String>,
        sha256: Option<String>,
        warn_unpinned: bool,
        subpath: Option<std::path::PathBuf>,
    },
    Local {
        dir: std::path::PathBuf,
        sha256: Option<String>,
    },
}

struct PluginInstallOutcome {
    summary: maw_plugin_manifest::PluginInstallSummary,
    warning: Option<String>,
}

fn parse_plugin_install_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut source = None;
    let mut install_root = None;
    let mut reference = None;
    let mut sha256 = None;
    let mut subpath = None;
    let mut plan_json = false;
    let mut force = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--force" => force = true,
            "--root" => {
                install_root = Some(take_plugin_manifest_path(argv, index, "--root").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            "--ref" => {
                reference = Some(take_plugin_manifest_value(argv, index, "--ref").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            "--sha256" => {
                let value = take_plugin_manifest_value(argv, index, "--sha256").map_err(PluginParseError::Usage)?;
                sha256 = Some(normalize_plugin_install_sha256(&value).map_err(PluginParseError::Usage)?);
                index += 1;
            }
            "--path" => {
                subpath = Some(take_plugin_manifest_path(argv, index, "--path").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            other if !other.starts_with('-') && source.is_none() => source = Some(other.to_owned()),
            other => return Err(PluginParseError::Usage(format!("plugin install: unknown argument {other}"))),
        }
        index += 1;
    }
    let source = source.ok_or_else(|| PluginParseError::Usage("plugin install: source dir or git url is required".to_owned()))?;
    Ok(PluginAction::Install {
        source: classify_plugin_install_source_with_subpath(&source, reference, sha256, subpath)
            .map_err(PluginParseError::Usage)?,
        install_root,
        plan_json,
        force,
    })
}

#[cfg(test)]
fn classify_plugin_install_source(
    value: &str,
    reference: Option<String>,
    sha256: Option<String>,
) -> Result<InstallSource, String> {
    classify_plugin_install_source_with_subpath(value, reference, sha256, None)
}

fn classify_plugin_install_source_with_subpath(
    value: &str,
    reference: Option<String>,
    sha256: Option<String>,
    requested_subpath: Option<std::path::PathBuf>,
) -> Result<InstallSource, String> {
    if is_explicit_git_install_source(value) {
        return Ok(InstallSource::Git {
            url: value.to_owned(),
            reference,
            sha256,
            warn_unpinned: false,
            subpath: requested_subpath.map(normalize_plugin_install_subpath).transpose()?,
        });
    }

    let path = std::path::PathBuf::from(value);
    if let Some((github, inline_ref, derived_subpath)) = parse_github_shorthand_install_source(value, &path) {
        if reference.is_some() && inline_ref.is_some() {
            return Err("plugin install: use either owner/repo@ref or --ref, not both".to_owned());
        }
        if requested_subpath.is_some() && derived_subpath.is_some() {
            return Err("plugin install: use either owner/repo/subpath or --path, not both".to_owned());
        }
        let reference = reference.or(inline_ref);
        let warn_unpinned = reference.is_none() && sha256.is_none();
        return Ok(InstallSource::Git {
            url: format!("https://github.com/{github}"),
            reference,
            sha256,
            warn_unpinned,
            subpath: requested_subpath.or(derived_subpath)
                .map(normalize_plugin_install_subpath).transpose()?,
        });
    }

    if reference.is_some() {
        return Err("plugin install: --ref is only supported for git sources".to_owned());
    }
    if requested_subpath.is_some() {
        return Err("plugin install: --path is only supported for git sources".to_owned());
    }
    Ok(InstallSource::Local { dir: path, sha256 })
}

fn is_explicit_git_install_source(value: &str) -> bool {
    value.starts_with("http")
        || value.starts_with("git@")
        || std::path::Path::new(value)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("git"))
        || value.contains("://")
}

fn parse_github_shorthand_install_source(
    value: &str,
    path: &std::path::Path,
) -> Option<(String, Option<String>, Option<std::path::PathBuf>)> {
    if path.exists()
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with("~/")
        || value.contains('\\')
    {
        return None;
    }
    let mut parts = value.split('/');
    let owner = parts.next()?;
    let raw_repo = parts.next()?;
    let tail = parts.collect::<Vec<_>>();
    let (repo, reference) = raw_repo
        .split_once('@')
        .map_or((raw_repo, None), |(repo, reference)| (repo, Some(reference.to_owned())));
    (!owner.is_empty()
        && !repo.is_empty()
        && reference.as_ref().is_none_or(|value| !value.is_empty())
        && owner != "."
        && owner != ".."
        && repo != "."
        && repo != ".."
        && tail.iter().all(|part| !part.is_empty()))
    .then(|| {
        let subpath = (!tail.is_empty()).then(|| tail.iter().collect());
        (format!("{owner}/{repo}"), reference, subpath)
    })
}

fn normalize_plugin_install_subpath(path: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    let valid = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|component| matches!(component, std::path::Component::Normal(_)));
    if valid {
        Ok(path)
    } else {
        Err("plugin install: --path must be a non-empty relative directory without '.' or '..'".to_owned())
    }
}

/// Default install root, unified with discovery (mawx WI-3): an explicit
/// `MAW_PLUGINS_DIR` — discovery's exclusive scan root — wins, otherwise
/// `maw_data_path(["plugins"])`, which `scan_dirs()` also scans. Either way
/// the installed plugin is discoverable by construction.
fn resolve_default_plugin_root() -> std::path::PathBuf {
    std::env::var_os("MAW_PLUGINS_DIR").map_or_else(
        || maw_data_path(&real_xdg_env(), &["plugins"]),
        std::path::PathBuf::from,
    )
}

/// Local-directory install (`maw plugin install <dir>`): verify exactly like
/// the git route — a `target=wasm` package must carry a committed artifact
/// hashing to its `artifact.sha256` pin and must satisfy `--sha256`/plugins.lock
/// — then copy and record the resolved pin into plugins.lock.
fn install_from_local_dir(
    source: &std::path::Path,
    expected_sha256: Option<&str>,
    root: &std::path::Path,
    force: bool,
) -> Result<PluginInstallOutcome, String> {
    let verification = match verify_package_dir(source, expected_sha256, false, force)? {
        ResolvedPackage::Wasm(verification) => verification,
        ResolvedPackage::NotWasm => verify_local_artifact_install(source, expected_sha256, force)?,
    };
    let summary = install_plugin_dir(source, root, force)?;
    record_plugin_install_pin(
        &summary,
        verification.resolved_sha256.as_deref(),
        &local_install_lock_source(source),
    )?;
    Ok(PluginInstallOutcome { summary, warning: verification.warning })
}

/// Verify a local non-wasm plugin dir before copy: when the manifest pins an
/// artifact (`artifact.path` + `artifact.sha256`), the committed file must
/// hash to that pin and satisfy `--sha256`/plugins.lock; unpinned dev dirs
/// keep installing as-is (nothing verifiable to pin — no lock entry written).
fn verify_local_artifact_install(
    source: &std::path::Path,
    expected_sha256: Option<&str>,
    force: bool,
) -> Result<PluginInstallVerification, String> {
    let plugin = load_manifest_from_dir(source)?
        .ok_or_else(|| format!("no plugin.json in {}", source.display()))?;
    let Some(artifact) = plugin.manifest.artifact.as_ref() else {
        if expected_sha256.is_some() {
            return Err(format!(
                "plugin install: --sha256 given but {} declares no artifact to verify",
                source.display()
            ));
        }
        return Ok(PluginInstallVerification { warning: None, resolved_sha256: None });
    };
    let Some(pin) = artifact.sha256.as_deref().filter(|pin| !pin.is_empty()) else {
        if expected_sha256.is_some() {
            return Err(format!(
                "plugin install: --sha256 given but {} has no artifact.sha256 pin — run `maw plugin build` first",
                source.display()
            ));
        }
        return Ok(PluginInstallVerification { warning: None, resolved_sha256: None });
    };
    let pin = normalize_plugin_install_sha256(pin).map_err(|_| {
        format!(
            "plugin install: package '{}' artifact.sha256 must be 64 lowercase hex chars (optionally 'sha256:'-prefixed)",
            plugin.manifest.name
        )
    })?;
    let artifact_path = source.join(&artifact.path);
    let observed = hash_file(&artifact_path)
        .map_err(|error| format!("plugin install: hash {} failed: {error}", artifact.path))?;
    if observed != pin {
        return Err(format!(
            "plugin '{}' artifact sha256 mismatch — refusing to install.\n  plugin.json: {pin}\n  committed:   {observed}",
            plugin.manifest.name
        ));
    }
    verify_plugin_install_pin(
        &plugin.manifest.name,
        &plugin.manifest.version,
        Some(&observed),
        expected_sha256,
        false,
        force,
    )
}

fn install_plugin_dir(
    source: &std::path::Path,
    root: &std::path::Path,
    force: bool,
) -> Result<maw_plugin_manifest::PluginInstallSummary, String> {
    let plugin = load_manifest_from_dir(source)?
        .ok_or_else(|| format!("no plugin.json in {}", source.display()))?;
    let destination = root.join(&plugin.manifest.name);
    match std::fs::symlink_metadata(&destination) {
        Ok(_) if !force => {
            return Err(format!(
                "plugin '{}' is already installed; use --force to reinstall",
                plugin.manifest.name
            ));
        }
        Ok(metadata) if metadata.file_type().is_dir() => std::fs::remove_dir_all(&destination)
            .map_err(|error| format!("plugin install: remove existing failed: {error}"))?,
        Ok(_) => std::fs::remove_file(&destination)
            .map_err(|error| format!("plugin install: remove existing failed: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("plugin install: inspect destination failed: {error}")),
    }
    install_built_plugin_dir(source, root)
}

fn install_from_git(
    url: &str,
    reference: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    subpath: Option<&std::path::Path>,
    root: &std::path::Path,
    force: bool,
) -> Result<PluginInstallOutcome, String> {
    let tmp = create_plugin_install_temp_dir()?;
    let target = PluginInstallTarget { root, force };
    let result = install_from_git_in_temp(url, reference, expected_sha256, warn_unpinned, subpath, target, &tmp);
    let cleanup = std::fs::remove_dir_all(&tmp);
    match (result, cleanup) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(message), _) => Err(message),
        (Ok(_), Err(error)) => Err(format!("plugin install: temp cleanup failed: {error}")),
    }
}

#[derive(Clone, Copy)]
struct PluginInstallTarget<'a> {
    root: &'a std::path::Path,
    force: bool,
}

fn install_from_git_in_temp(
    url: &str,
    reference: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    subpath: Option<&std::path::Path>,
    target: PluginInstallTarget<'_>,
    tmp: &std::path::Path,
) -> Result<PluginInstallOutcome, String> {
    git_clone_plugin_repo(url, reference, tmp)?;
    let source = subpath.map_or_else(|| tmp.to_owned(), |path| tmp.join(path));
    if !source.is_dir() {
        return Err(format!("plugin install: subpath not found: {}", source.display()));
    }
    let verification = match verify_package_dir(&source, expected_sha256, warn_unpinned, target.force)? {
        ResolvedPackage::Wasm(verification) => verification,
        // target=js (or absent): the JS builder owns validation and errors.
        ResolvedPackage::NotWasm => {
            let build = build_js_plugin_dir(&source, false)?;
            verify_plugin_install_pin(
                &build.name,
                &build.version,
                Some(&build.sha256),
                expected_sha256,
                warn_unpinned,
                target.force,
            )?
        }
    };
    let summary = install_plugin_dir(&source, target.root, target.force)?;
    record_plugin_install_pin(
        &summary,
        verification.resolved_sha256.as_deref(),
        &git_install_lock_source(url, subpath, reference),
    )?;
    Ok(PluginInstallOutcome { summary, warning: verification.warning })
}

fn read_raw_plugin_install_manifest(
    dir: &std::path::Path,
) -> Result<Option<serde_json::Value>, String> {
    let path = dir.join("plugin.json");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("invalid plugin.json: {error}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("invalid plugin.json: {error}"))
}

/// Outcome of [`verify_package_dir`]: either a fully pin-verified wasm
/// package, or a non-wasm package whose verification/build the calling route
/// owns.
#[derive(Debug)]
enum ResolvedPackage {
    /// `target=wasm`: the committed artifact exists, stays inside the
    /// package, hashes to the manifest `artifact.sha256` pin, and satisfies
    /// the `--sha256`/plugins.lock rules.
    Wasm(PluginInstallVerification),
    /// No plugin.json, or `target` is not `wasm` — the caller owns the rest
    /// of the route (local artifact verify / JS build).
    NotWasm,
}

/// The ONE canonical package-dir verification every install route calls
/// (git clone and local dir today; the mawx fetch routes — WI-5/WI-6 —
/// later). Reads the raw plugin.json and, for `target=wasm` packages, runs
/// the full pin gate via [`verify_wasm_package_install`]: artifact.path
/// present, traversal-guarded, committed artifact hashing to the
/// `artifact.sha256` pin, and the `--sha256`/plugins.lock rules (explicit
/// `--sha256` mismatch stays fatal even with `force`; `warn_unpinned`
/// surfaces the unpinned-source warning). Non-wasm packages return
/// [`ResolvedPackage::NotWasm`] untouched.
fn verify_package_dir(
    dir: &std::path::Path,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    force: bool,
) -> Result<ResolvedPackage, String> {
    match read_raw_plugin_install_manifest(dir)? {
        Some(raw) if raw.get("target").and_then(serde_json::Value::as_str) == Some("wasm") => {
            verify_wasm_package_install(dir, &raw, expected_sha256, warn_unpinned, force)
                .map(ResolvedPackage::Wasm)
        }
        _ => Ok(ResolvedPackage::NotWasm),
    }
}

/// Verify a `target=wasm` package before install (shared by the git clone and
/// the local `--path`/dir routes): the committed wasm artifact must exist,
/// hash to the manifest `artifact.sha256` pin, and satisfy the same
/// `--sha256`/plugins.lock rules as the JS build route.
fn verify_wasm_package_install(
    source: &std::path::Path,
    raw: &serde_json::Value,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    force: bool,
) -> Result<PluginInstallVerification, String> {
    let name = raw
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unnamed>");
    let artifact = raw.get("artifact").and_then(serde_json::Value::as_object);
    let Some(path) = artifact
        .and_then(|artifact| artifact.get("path"))
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())
    else {
        return Err(format!(
            "plugin install: package '{name}' targets wasm but plugin.json has no artifact.path — git installs need a committed .wasm artifact pinned by artifact.path + artifact.sha256"
        ));
    };
    let Some(pin) = artifact
        .and_then(|artifact| artifact.get("sha256"))
        .and_then(serde_json::Value::as_str)
        .filter(|pin| !pin.is_empty())
    else {
        return Err(format!(
            "plugin install: package '{name}' targets wasm but plugin.json has no artifact.sha256 — pin the committed {path} (maw plugin build writes the pin) before git install"
        ));
    };
    let pin = normalize_plugin_install_sha256(pin).map_err(|_| {
        format!("plugin install: package '{name}' artifact.sha256 must be 64 lowercase hex chars (optionally 'sha256:'-prefixed)")
    })?;
    let relative = std::path::Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "plugin install: package '{name}' artifact.path must stay inside the package: {path}"
        ));
    }
    let wasm_path = source.join(relative);
    if !wasm_path.is_file() {
        return Err(format!(
            "plugin install: package '{name}' targets wasm but the artifact {path} is not committed — build and commit the .wasm before git install"
        ));
    }
    let observed = hash_file(&wasm_path)
        .map_err(|error| format!("plugin install: hash {path} failed: {error}"))?;
    if observed != pin {
        return Err(format!(
            "plugin '{name}' artifact sha256 mismatch — refusing to install.\n  plugin.json: {pin}\n  committed:   {observed}"
        ));
    }
    let plugin = load_manifest_from_dir(source)?
        .ok_or_else(|| format!("no plugin.json in {}", source.display()))?;
    verify_plugin_install_pin(
        &plugin.manifest.name,
        &plugin.manifest.version,
        Some(&observed),
        expected_sha256,
        warn_unpinned,
        force,
    )
}

fn normalize_plugin_install_sha256(value: &str) -> Result<String, String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()) {
        Ok(format!("sha256:{hex}"))
    } else {
        Err("plugin install: --sha256 must be 64 lowercase hex chars".to_owned())
    }
}

/// Result of a pre-install pin verification: the optional user-facing warning
/// plus the resolved artifact `sha256` recorded into plugins.lock on success.
#[derive(Debug)]
struct PluginInstallVerification {
    warning: Option<String>,
    resolved_sha256: Option<String>,
}

fn create_plugin_install_temp_dir() -> Result<std::path::PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for attempt in 0..100 {
        let dir = std::env::temp_dir().join(format!(
            "maw-rs-plugin-install-{}-{nanos}-{attempt}",
            std::process::id()
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("plugin install: temp dir create failed: {error}")),
        }
    }
    Err("plugin install: temp dir collision".to_owned())
}

fn git_clone_plugin_repo(
    url: &str,
    reference: Option<&str>,
    dest: &std::path::Path,
) -> Result<(), String> {
    let mut command = std::process::Command::new("git");
    command
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .stdin(std::process::Stdio::null());
    if let Some(reference) = reference {
        command.arg("--branch").arg(reference);
    }
    let output = command
        .arg(url)
        .arg(dest)
        .output()
        .map_err(|error| format!("plugin install: failed to run git clone: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "plugin install: git clone failed{}",
        command_failure_detail(&output)
    ))
}

fn command_failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

fn render_plugin_install_summary(
    summary: &maw_plugin_manifest::PluginInstallSummary,
    plan_json: bool,
) -> String {
    if plan_json {
        let copied = summary.copied_files.iter().map(path_string).collect::<Vec<_>>();
        format!("{{\"command\":\"plugin\",\"kind\":\"install\",\"name\":{},\"version\":{},\"sourceDir\":{},\"installDir\":{},\"copiedFiles\":{}}}\n", json_string(&summary.name), json_string(&summary.version), json_string(&path_string(&summary.source_dir)), json_string(&path_string(&summary.install_dir)), json_string_array(&copied))
    } else {
        format!(
            "installed {}@{} {}\n",
            summary.name,
            summary.version,
            path_string(&summary.install_dir)
        )
    }
}

fn plugin_install_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}
