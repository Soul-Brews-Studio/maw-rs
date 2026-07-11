fn plugin_scaffold_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs plugin-scaffold validate-name --name <name> [--plan-json]\n       maw-rs plugin-scaffold manifest --name <name> (--rust|--as) [--plan-json]\n       maw-rs plugin-scaffold constants [--plan-json]\n"
        ),
    }
}

fn run_plugin_plan(argv: &[String]) -> CliOutput {
    let action = match parse_plugin_args(argv) {
        Ok(action) => action,
        Err(PluginParseError::Usage(message)) => return plugin_usage_error(&message),
        Err(PluginParseError::Help) => return plugin_ls_help(),
    };

    match action {
        PluginAction::Ls { options, ls_options } => {
            let report = discover_packages(&options);
            CliOutput {
                code: 0,
                stdout: render_plugin_ls(&report.plugins, &ls_options),
                stderr: String::new(),
            }
        }
        PluginAction::InferCapabilities { source, plan_json } => {
            let caps = infer_plugin_capabilities(&source);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!("{{\"command\":\"plugin\",\"kind\":\"infer-capabilities\",\"capabilities\":{}}}\n", json_string_array(&caps))
                } else {
                    format!("{}\n", caps.join("\n"))
                },
                stderr: String::new(),
            }
        }
        PluginAction::Build { dir, emit_types, plan_json } => match build_js_plugin_dir(&dir, emit_types) {
            Ok(summary) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_plugin_build_summary_json(&summary)
                } else {
                    format!("built {}@{} {}\n", summary.name, summary.version, path_string(&summary.bundle_path))
                },
                stderr: String::new(),
            },
            Err(message) => plugin_usage_error(&message),
        },
        PluginAction::Init { name, dir, plan_json } => match init_js_plugin_dir(&name, &dir) {
            Ok(summary) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!("{{\"command\":\"plugin\",\"kind\":\"init\",\"name\":{},\"dir\":{},\"manifestPath\":{},\"entryPath\":{}}}\n", json_string(&summary.name), json_string(&path_string(&summary.dir)), json_string(&path_string(&summary.manifest_path)), json_string(&path_string(&summary.entry_path)))
                } else {
                    format!("initialized {} {}\n", summary.name, path_string(&summary.dir))
                },
                stderr: String::new(),
            },
            Err(message) => plugin_usage_error(&message),
        },
        PluginAction::Install { source, install_root, plan_json } => {
            let install_root = install_root.unwrap_or_else(resolve_default_plugin_root);
            let result = match source {
                InstallSource::Local(source_dir) => install_built_plugin_dir(&source_dir, &install_root)
                    .map(|summary| PluginInstallOutcome { summary, warning: None }),
                InstallSource::Git { url, reference, sha256, warn_unpinned } => {
                    install_from_git(&url, reference.as_deref(), sha256.as_deref(), warn_unpinned, &install_root, true)
                }
            };
            match result {
                Ok(outcome) => CliOutput {
                    code: 0,
                    stdout: render_plugin_install_summary(&outcome.summary, plan_json),
                    stderr: outcome.warning.map_or_else(String::new, |warning| format!("{warning}\n")),
                },
                Err(message) => plugin_usage_error(&message),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallSource {
    Git {
        url: String,
        reference: Option<String>,
        sha256: Option<String>,
        warn_unpinned: bool,
    },
    Local(std::path::PathBuf),
}

struct PluginInstallOutcome {
    summary: maw_plugin_manifest::PluginInstallSummary,
    warning: Option<String>,
}

enum PluginAction {
    Ls {
        options: DiscoverPackagesOptions,
        ls_options: PluginLsOptions,
    },
    InferCapabilities { source: String, plan_json: bool },
    Build { dir: std::path::PathBuf, emit_types: bool, plan_json: bool },
    Init { name: String, dir: std::path::PathBuf, plan_json: bool },
    Install { source: InstallSource, install_root: Option<std::path::PathBuf>, plan_json: bool },
}

#[derive(Default)]
struct PluginLsOptions {
    verbose: bool,
    tiers: Vec<PluginTier>,
    api_only: bool,
}

enum PluginParseError {
    Usage(String),
    Help,
}

fn parse_plugin_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err(PluginParseError::Usage("plugin: expected ls".to_owned()));
    };
    match kind {
        "ls" | "list" => parse_plugin_ls_args(&argv[1..]),
        "infer-capabilities" => parse_plugin_infer_args(&argv[1..]),
        "build" => parse_plugin_build_args(&argv[1..]),
        "init" => parse_plugin_init_args(&argv[1..]),
        "install" => parse_plugin_install_args(&argv[1..]),
        other => Err(PluginParseError::Usage(format!(
            "plugin: unknown subcommand {other}"
        ))),
    }
}


fn parse_plugin_infer_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut plan_json = false;
    let mut source = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--source" => {
                source = Some(take_plugin_manifest_value(argv, index, "--source").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            "--file" => {
                let path = take_plugin_manifest_path(argv, index, "--file").map_err(PluginParseError::Usage)?;
                source = Some(std::fs::read_to_string(&path).map_err(|error| {
                    PluginParseError::Usage(format!("plugin infer-capabilities: read failed: {error}"))
                })?);
                index += 1;
            }
            other => return Err(PluginParseError::Usage(format!("plugin infer-capabilities: unknown argument {other}"))),
        }
        index += 1;
    }
    Ok(PluginAction::InferCapabilities {
        source: source.ok_or_else(|| PluginParseError::Usage("plugin infer-capabilities: --source or --file is required".to_owned()))?,
        plan_json,
    })
}

fn parse_plugin_build_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut dir = std::path::PathBuf::from(".");
    let mut emit_types = false;
    let mut plan_json = false;
    let mut positional = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--types" => emit_types = true,
            "--plan-json" => plan_json = true,
            other if !other.starts_with('-') && !positional => {
                dir = std::path::PathBuf::from(other);
                positional = true;
            }
            other => return Err(PluginParseError::Usage(format!("plugin build: unknown argument {other}"))),
        }
        index += 1;
    }
    Ok(PluginAction::Build { dir, emit_types, plan_json })
}

fn parse_plugin_init_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut name = None;
    let mut dir = None;
    let mut plan_json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--dir" => {
                dir = Some(take_plugin_manifest_path(argv, index, "--dir").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            other if !other.starts_with('-') && name.is_none() => name = Some(other.to_owned()),
            other => return Err(PluginParseError::Usage(format!("plugin init: unknown argument {other}"))),
        }
        index += 1;
    }
    let name = name.ok_or_else(|| PluginParseError::Usage("plugin init: name is required".to_owned()))?;
    let dir = dir.unwrap_or_else(|| std::path::PathBuf::from(&name));
    Ok(PluginAction::Init { name, dir, plan_json })
}

fn parse_plugin_install_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut source = None;
    let mut install_root = None;
    let mut reference = None;
    let mut sha256 = None;
    let mut plan_json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
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
            other if !other.starts_with('-') && source.is_none() => source = Some(other.to_owned()),
            other => return Err(PluginParseError::Usage(format!("plugin install: unknown argument {other}"))),
        }
        index += 1;
    }
    let source = source.ok_or_else(|| PluginParseError::Usage("plugin install: source dir or git url is required".to_owned()))?;
    Ok(PluginAction::Install {
        source: classify_plugin_install_source(&source, reference, sha256).map_err(PluginParseError::Usage)?,
        install_root,
        plan_json,
    })
}

fn classify_plugin_install_source(
    value: &str,
    reference: Option<String>,
    sha256: Option<String>,
) -> Result<InstallSource, String> {
    if is_explicit_git_install_source(value) {
        return Ok(InstallSource::Git {
            url: value.to_owned(),
            reference,
            sha256,
            warn_unpinned: false,
        });
    }

    let path = std::path::PathBuf::from(value);
    if let Some((github, inline_ref)) = parse_github_shorthand_install_source(value, &path) {
        if reference.is_some() && inline_ref.is_some() {
            return Err("plugin install: use either owner/repo@ref or --ref, not both".to_owned());
        }
        let reference = reference.or(inline_ref);
        let warn_unpinned = reference.is_none() && sha256.is_none();
        return Ok(InstallSource::Git {
            url: format!("https://github.com/{github}"),
            reference,
            sha256,
            warn_unpinned,
        });
    }

    if reference.is_some() {
        return Err("plugin install: --ref is only supported for git sources".to_owned());
    }
    if sha256.is_some() {
        return Err("plugin install: --sha256 is only supported for git sources".to_owned());
    }
    Ok(InstallSource::Local(path))
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
) -> Option<(String, Option<String>)> {
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
    if parts.next().is_some() {
        return None;
    }
    let (repo, reference) = raw_repo
        .split_once('@')
        .map_or((raw_repo, None), |(repo, reference)| (repo, Some(reference.to_owned())));
    (!owner.is_empty()
        && !repo.is_empty()
        && reference.as_ref().is_none_or(|value| !value.is_empty())
        && owner != "."
        && owner != ".."
        && repo != "."
        && repo != "..").then(|| (format!("{owner}/{repo}"), reference))
}

fn resolve_default_plugin_root() -> std::path::PathBuf {
    maw_data_path(&real_xdg_env(), &["plugins"])
}

fn install_from_git(
    url: &str,
    reference: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    root: &std::path::Path,
    build: bool,
) -> Result<PluginInstallOutcome, String> {
    let tmp = create_plugin_install_temp_dir()?;
    let result = install_from_git_in_temp(url, reference, expected_sha256, warn_unpinned, root, build, &tmp);
    let cleanup = std::fs::remove_dir_all(&tmp);
    match (result, cleanup) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(message), _) => Err(message),
        (Ok(_), Err(error)) => Err(format!("plugin install: temp cleanup failed: {error}")),
    }
}

fn install_from_git_in_temp(
    url: &str,
    reference: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    root: &std::path::Path,
    build: bool,
    tmp: &std::path::Path,
) -> Result<PluginInstallOutcome, String> {
    git_clone_plugin_repo(url, reference, tmp)?;
    let build_summary = if build { Some(build_js_plugin_dir(tmp, false)?) } else { None };
    let (name, version, observed) = if let Some(build) = build_summary.as_ref() {
        (build.name.clone(), build.version.clone(), Some(build.sha256.clone()))
    } else {
        let plugin = load_manifest_from_dir(tmp)?.ok_or_else(|| format!("no plugin.json in {}", tmp.display()))?;
        let sha = plugin.manifest.artifact.as_ref().and_then(|artifact| artifact.sha256.clone());
        (plugin.manifest.name, plugin.manifest.version, sha)
    };
    let warning = verify_plugin_install_pin(&name, &version, observed.as_deref(), expected_sha256, warn_unpinned)?;
    let summary = install_built_plugin_dir(tmp, root)?;
    Ok(PluginInstallOutcome { summary, warning })
}

fn normalize_plugin_install_sha256(value: &str) -> Result<String, String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()) {
        Ok(format!("sha256:{hex}"))
    } else {
        Err("plugin install: --sha256 must be 64 lowercase hex chars".to_owned())
    }
}

fn verify_plugin_install_pin(
    name: &str,
    version: &str,
    observed_sha256: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
) -> Result<Option<String>, String> {
    let observed = observed_sha256.ok_or_else(|| "plugin install: sha256 unavailable after build".to_owned())?;
    let locked = read_plugin_lock_entry(name)?;
    if let Some(entry) = &locked {
        if entry.version != version {
            return Err(format!("plugin '{name}' version mismatch: plugins.lock={} install={version}", entry.version));
        }
        if entry.sha256 != observed {
            return Err(format!("plugin '{name}' sha256 mismatch — refusing to install.\n  plugins.lock: {}\n  install:      {observed}", entry.sha256));
        }
    }
    if let Some(expected) = expected_sha256 {
        if expected != observed {
            return Err(format!("plugin '{name}' sha256 mismatch — refusing to install.\n  expected: {expected}\n  install:  {observed}"));
        }
    }
    Ok((locked.is_none() && expected_sha256.is_none() && warn_unpinned).then(|| {
        format!("warning: plugin install {name} is unpinned; use owner/repo@ref and --sha256 {observed}")
    }))
}

struct PluginLockEntry { version: String, sha256: String }

fn read_plugin_lock_entry(name: &str) -> Result<Option<PluginLockEntry>, String> {
    let path = std::env::var_os("MAW_PLUGINS_LOCK").map_or_else(
        || maw_data_path(&real_xdg_env(), &["plugins.lock"]),
        std::path::PathBuf::from,
    );
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("plugins.lock: read {}: {error}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("plugins.lock: invalid JSON at {}: {error}", path.display()))?;
    let plugins = json.get("plugins").and_then(serde_json::Value::as_object)
        .ok_or_else(|| "plugins.lock: 'plugins' must be an object".to_owned())?;
    let Some(entry) = plugins.get(name) else { return Ok(None); };
    let version = entry.get("version").and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("plugins.lock: entry '{name}' missing version"))?;
    let sha256 = normalize_plugin_install_sha256(entry.get("sha256").and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("plugins.lock: entry '{name}' missing sha256"))?)?;
    Ok(Some(PluginLockEntry { version: version.to_owned(), sha256 }))
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

#[cfg(test)]
mod plugin_install_tests {
    use super::{classify_plugin_install_source, verify_plugin_install_pin, InstallSource};
    use std::sync::{Mutex, OnceLock};

    fn lock_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn temp_existing_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maw-rs-plugin-install-classifier-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn classifier_accepts_explicit_git_url_forms() {
        assert_eq!(
            classify_plugin_install_source("https://github.com/owner/repo", None, None).expect("https"),
            InstallSource::Git {
                url: "https://github.com/owner/repo".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
            }
        );
        assert_eq!(
            classify_plugin_install_source(
                "git@github.com:owner/repo.git",
                Some("main".to_owned()),
                None,
            )
            .expect("ssh"),
            InstallSource::Git {
                url: "git@github.com:owner/repo.git".to_owned(),
                reference: Some("main".to_owned()),
                sha256: None,
                warn_unpinned: false,
            }
        );
        assert_eq!(
            classify_plugin_install_source("file:///tmp/plugin-fixture", None, None).expect("file"),
            InstallSource::Git {
                url: "file:///tmp/plugin-fixture".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
            }
        );
        assert_eq!(
            classify_plugin_install_source("owner/repo.git", None, None).expect("suffix"),
            InstallSource::Git {
                url: "owner/repo.git".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
            }
        );
    }

    #[test]
    fn classifier_maps_owner_repo_shorthand_to_github_when_not_local() {
        assert_eq!(
            classify_plugin_install_source("Soul-Brews-Studio/maw-js", Some("alpha".to_owned()), None)
                .expect("shorthand"),
            InstallSource::Git {
                url: "https://github.com/Soul-Brews-Studio/maw-js".to_owned(),
                reference: Some("alpha".to_owned()),
                sha256: None,
                warn_unpinned: false,
            }
        );
        assert_eq!(
            classify_plugin_install_source("Soul-Brews-Studio/maw-js@v1", None, None)
                .expect("inline ref"),
            InstallSource::Git {
                url: "https://github.com/Soul-Brews-Studio/maw-js".to_owned(),
                reference: Some("v1".to_owned()),
                sha256: None,
                warn_unpinned: false,
            }
        );
    }

    #[test]
    fn classifier_keeps_local_paths_local() {
        assert_eq!(
            classify_plugin_install_source("local-plugin", None, None).expect("plain local"),
            InstallSource::Local(std::path::PathBuf::from("local-plugin"))
        );

        let dir = temp_existing_dir("existing");
        assert_eq!(
            classify_plugin_install_source(&dir.display().to_string(), None, None).expect("existing"),
            InstallSource::Local(dir.clone())
        );

        let pathish = "./missing-plugin";
        assert_eq!(
            classify_plugin_install_source(pathish, None, None).expect("pathish"),
            InstallSource::Local(std::path::PathBuf::from(pathish))
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn classifier_rejects_ref_for_local_source() {
        let error = classify_plugin_install_source("local-plugin", Some("main".to_owned()), None)
            .expect_err("local ref rejected");
        assert!(error.contains("--ref is only supported for git sources"));
    }

    #[test]
    fn pin_verifier_matches_mismatches_warns_and_checks_lock() {
        let _guard = lock_guard();
        let old = std::env::var_os("MAW_PLUGINS_LOCK");
        let path = temp_existing_dir("lock").join("plugins.lock");
        std::env::set_var("MAW_PLUGINS_LOCK", &path);
        let sha = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(verify_plugin_install_pin("demo", "0.1.0", Some(sha), Some(sha), true).expect("match"), None);
        let err = verify_plugin_install_pin("demo", "0.1.0", Some(sha), Some("sha256:1111111111111111111111111111111111111111111111111111111111111111"), false).expect_err("mismatch");
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert!(verify_plugin_install_pin("demo", "0.1.0", Some(sha), None, true).expect("warn").expect("warning").contains("unpinned"));
        let locked = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        std::fs::write(&path, format!(r#"{{"schema":1,"plugins":{{"demo":{{"version":"0.1.0","sha256":"{locked}","source":"github:o/r@v1"}}}}}}"#)).expect("lock");
        assert_eq!(verify_plugin_install_pin("demo", "0.1.0", Some(locked), None, true).expect("lock match"), None);
        let err = verify_plugin_install_pin("demo", "0.1.0", Some(sha), None, false).expect_err("lock mismatch");
        assert!(err.contains("plugins.lock"), "{err}");
        match old { Some(value) => std::env::set_var("MAW_PLUGINS_LOCK", value), None => std::env::remove_var("MAW_PLUGINS_LOCK") }
    }
}

fn parse_plugin_ls_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut options = DiscoverPackagesOptions {
        runtime_version: "1.0.0".to_owned(),
        ..DiscoverPackagesOptions::default()
    };
    let mut ls_options = PluginLsOptions::default();
    let mut scan_dirs = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "-v" | "--verbose" => ls_options.verbose = true,
            "--core" => ls_options.tiers.push(PluginTier::Core),
            "--standard" => ls_options.tiers.push(PluginTier::Standard),
            "--extra" => ls_options.tiers.push(PluginTier::Extra),
            "--api" => ls_options.api_only = true,
            "--help" | "-h" => return Err(PluginParseError::Help),
            "--scan-dir" => {
                scan_dirs.push(
                    take_plugin_manifest_path(argv, index, "--scan-dir")
                        .map_err(PluginParseError::Usage)?,
                );
                index += 1;
            }
            "--disabled" => {
                options.disabled_plugins.push(
                    take_plugin_manifest_value(argv, index, "--disabled")
                        .map_err(PluginParseError::Usage)?,
                );
                index += 1;
            }
            "--runtime-version" => {
                options.runtime_version = take_plugin_manifest_value(argv, index, "--runtime-version")
                    .map_err(PluginParseError::Usage)?;
                index += 1;
            }
            "--use-cache" => options.use_cache = true,
            other => {
                return Err(PluginParseError::Usage(format!(
                    "plugin ls: unknown argument {other}"
                )));
            }
        }
        index += 1;
    }
    if !scan_dirs.is_empty() {
        options.scan_dirs = scan_dirs;
    }

    Ok(PluginAction::Ls { options, ls_options })
}

fn plugin_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs plugin ls [-v|--verbose] [--core] [--standard] [--extra] [--api] [--scan-dir <dir>]... [--disabled <name>]... [--runtime-version <version>] [--use-cache]\n       maw-rs plugin <infer-capabilities|build|init|install> [args]\n"
        ),
    }
}

fn plugin_ls_help() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: "usage: maw plugin <init|build|install|create|ls|info|remove|enable <name...>|disable> [args]\n  ls: compact by default; use -v for full table; filters: --core --standard --extra --api\n".to_owned(),
        stderr: String::new(),
    }
}


fn render_plugin_build_summary_json(summary: &maw_plugin_manifest::PluginBuildSummary) -> String {
    let dts = summary
        .dts_path
        .as_ref()
        .map_or_else(|| "null".to_owned(), |path| json_string(&path_string(path)));
    format!(
        r#"{{"command":"plugin","kind":"build","name":{},"version":{},"dir":{},"bundlePath":{},"sizeBytes":{},"capabilities":{},"inferredOnly":{},"declaredOnly":{},"sha256":{},"manifestPath":{},"dtsPath":{dts}}}
"#,
        json_string(&summary.name),
        json_string(&summary.version),
        json_string(&path_string(&summary.dir)),
        json_string(&path_string(&summary.bundle_path)),
        summary.size_bytes,
        json_string_array(&summary.capabilities),
        json_string_array(&summary.inferred_only),
        json_string_array(&summary.declared_only),
        json_string(&summary.sha256),
        json_string(&path_string(&summary.manifest_path)),
    )
}

fn render_plugin_ls(plugins: &[LoadedPlugin], options: &PluginLsOptions) -> String {
    let mut rows = plugins
        .iter()
        .map(PluginLsRow::new)
        .filter(|row| options.tiers.is_empty() || options.tiers.contains(&row.tier))
        .filter(|row| !options.api_only || row.api_path.is_some())
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (plugin_tier_order(row.tier), row.name.to_owned()));

    if rows.is_empty() {
        return if plugins.is_empty() {
            "no plugins installed\n".to_owned()
        } else {
            format!("no plugins{}.\n", plugin_ls_filter_label(options))
        };
    }

    if !options.verbose {
        return render_plugin_ls_compact(&rows, options);
    }

    render_plugin_ls_table(&rows)
}

fn render_plugin_ls_compact(rows: &[PluginLsRow<'_>], options: &PluginLsOptions) -> String {
    let active = rows.iter().filter(|row| !row.disabled).count();
    let disabled = rows.len() - active;
    let core = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Core)
        .count();
    let standard = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Standard)
        .count();
    let extra = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Extra)
        .count();
    let cli = rows.iter().filter(|row| row.has_cli).count();
    let api = rows.iter().filter(|row| row.api_path.is_some()).count();
    let missing = rows.iter().filter(|row| row.missing_executable).count();
    let health = if missing == 0 {
        "ok".to_owned()
    } else {
        format!(
            "{missing} missing executable{}",
            if missing == 1 { "" } else { "s" }
        )
    };

    format!(
        "{} plugin{} ({} active, {} disabled){}\n  core: {core} · standard: {standard} · extra: {extra}\n  cli: {cli} · api: {api} · health: {health}\n",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        active,
        disabled,
        plugin_ls_filter_label(options)
    )
}

fn render_plugin_ls_table(rows: &[PluginLsRow<'_>]) -> String {
    let mut output = String::new();
    for tier in [PluginTier::Core, PluginTier::Standard, PluginTier::Extra] {
        let tier_rows = rows
            .iter()
            .filter(|row| row.tier == tier)
            .collect::<Vec<_>>();
        if tier_rows.is_empty() {
            continue;
        }
        let widths = PluginLsWidths::new(&tier_rows);

        let _ = writeln!(output, "\n\x1b[1m{}\x1b[0m ({})", tier.as_str(), tier_rows.len());
        writeln_padded_row(
            &mut output,
            &["name", "version", "tier", "surfaces", "dir"],
            &widths,
        );
        writeln_separator(&mut output, &widths);

        for row in tier_rows {
            let tier_label = format!(
                "{} {}",
                plugin_ls_tier_icon(row.tier, row.disabled),
                if row.disabled { "disabled" } else { row.tier.as_str() }
            );
            writeln_padded_row(
                &mut output,
                &[row.name, row.version, &tier_label, &row.surfaces, &row.dir],
                &widths,
            );
        }
    }

    let active = rows.iter().filter(|row| !row.disabled).count();
    let disabled = rows.len() - active;
    if disabled > 0 {
        let _ = writeln!(
            output,
            "\n{active} active. {disabled} disabled — use 'maw plugin ls --all' to see them."
        );
    } else {
        let _ = writeln!(output, "\n{active} active");
    }
    output
}

fn plugin_ls_filter_label(options: &PluginLsOptions) -> String {
    let mut parts = options
        .tiers
        .iter()
        .map(|tier| tier.as_str())
        .collect::<Vec<_>>();
    if options.api_only {
        parts.push("api");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" matching {}", parts.join("+"))
    }
}

struct PluginLsRow<'a> {
    name: &'a str,
    version: &'a str,
    tier: PluginTier,
    surfaces: String,
    dir: String,
    disabled: bool,
    has_cli: bool,
    missing_executable: bool,
    api_path: Option<&'a str>,
}

impl<'a> PluginLsRow<'a> {
    fn new(plugin: &'a LoadedPlugin) -> Self {
        let manifest = &plugin.manifest;
        let cli_command = plugin_ls_cli_command(plugin);
        let api_path = manifest.api.as_ref().map(|api| api.path.as_str());
        let executable_path = match plugin.kind {
            LoadedPluginKind::Ts => plugin.entry_path.as_ref(),
            LoadedPluginKind::Wasm => (!plugin.wasm_path.as_os_str().is_empty()).then_some(&plugin.wasm_path),
        };
        Self {
            name: &manifest.name,
            version: &manifest.version,
            tier: plugin_ls_effective_tier(manifest),
            surfaces: plugin_ls_surfaces(cli_command.as_deref(), api_path),
            dir: shorten_home(&plugin.dir),
            disabled: plugin.disabled,
            has_cli: cli_command.is_some(),
            missing_executable: executable_path.is_some_and(|path| !path.exists()),
            api_path,
        }
    }
}

struct PluginLsWidths {
    name: usize,
    version: usize,
    tier: usize,
    surfaces: usize,
    dir: usize,
}

impl PluginLsWidths {
    fn new(rows: &[&PluginLsRow<'_>]) -> Self {
        let mut widths = Self {
            name: "name".chars().count(),
            version: "version".chars().count(),
            tier: "tier".chars().count(),
            surfaces: "surfaces".chars().count(),
            dir: "dir".chars().count(),
        };
        for row in rows {
            widths.name = widths.name.max(row.name.chars().count());
            widths.version = widths.version.max(row.version.chars().count());
            let tier_label = format!("{} {}", plugin_ls_tier_icon(row.tier, row.disabled), row.tier.as_str());
            widths.tier = widths.tier.max(tier_label.chars().count());
            widths.surfaces = widths.surfaces.max(row.surfaces.chars().count());
            widths.dir = widths.dir.max(row.dir.chars().count());
        }
        widths
    }
}

fn writeln_padded_row(output: &mut String, cells: &[&str; 5], widths: &PluginLsWidths) {
    let padded = [
        pad_end_chars(cells[0], widths.name),
        pad_end_chars(cells[1], widths.version),
        pad_end_chars(cells[2], widths.tier),
        pad_end_chars(cells[3], widths.surfaces),
        pad_end_chars(cells[4], widths.dir),
    ];
    let _ = writeln!(
        output,
        "{}  {}  {}  {}  {}",
        padded[0], padded[1], padded[2], padded[3], padded[4]
    );
}

fn writeln_separator(output: &mut String, widths: &PluginLsWidths) {
    let _ = writeln!(
        output,
        "{}  {}  {}  {}  {}",
        "─".repeat(widths.name),
        "─".repeat(widths.version),
        "─".repeat(widths.tier),
        "─".repeat(widths.surfaces),
        "─".repeat(widths.dir)
    );
}

fn pad_end_chars(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_owned()
    } else {
        format!("{}{}", value, " ".repeat(width - len))
    }
}

fn plugin_ls_surfaces(cli_command: Option<&str>, api_path: Option<&str>) -> String {
    let mut surfaces = Vec::new();
    if let Some(command) = cli_command {
        surfaces.push(format!("cli:{command}"));
    }
    if let Some(api_path) = api_path {
        surfaces.push(format!("api:{api_path}"));
    }
    if surfaces.is_empty() {
        "—".to_owned()
    } else {
        surfaces.join(", ")
    }
}

fn plugin_ls_cli_command(plugin: &LoadedPlugin) -> Option<String> {
    plugin.manifest.cli.as_ref().map_or_else(
        || match plugin.kind {
            LoadedPluginKind::Ts if plugin.entry_path.is_some() => Some(plugin.manifest.name.clone()),
            LoadedPluginKind::Wasm if !plugin.wasm_path.as_os_str().is_empty() => {
                Some(plugin.manifest.name.clone())
            }
            LoadedPluginKind::Ts | LoadedPluginKind::Wasm => None,
        },
        |cli| Some(cli.command.clone()),
    )
}

fn plugin_ls_effective_tier(manifest: &PluginManifest) -> PluginTier {
    manifest
        .tier
        .unwrap_or_else(|| plugin_ls_weight_to_tier(manifest.weight.unwrap_or(50)))
}

fn plugin_ls_weight_to_tier(weight: u64) -> PluginTier {
    if weight < 10 {
        PluginTier::Core
    } else if weight < 50 {
        PluginTier::Standard
    } else {
        PluginTier::Extra
    }
}

fn plugin_tier_order(tier: PluginTier) -> u8 {
    match tier {
        PluginTier::Core => 0,
        PluginTier::Standard => 1,
        PluginTier::Extra => 2,
    }
}

fn plugin_ls_tier_icon(tier: PluginTier, disabled: bool) -> &'static str {
    if disabled {
        "\x1b[90m○\x1b[0m"
    } else {
        match tier {
            PluginTier::Core => "\x1b[32m●\x1b[0m",
            PluginTier::Standard => "\x1b[36m●\x1b[0m",
            PluginTier::Extra => "\x1b[33m●\x1b[0m",
        }
    }
}

fn shorten_home(path: &Path) -> String {
    let raw = path_string(path);
    std::env::var("HOME").map_or(raw.clone(), |home| {
        raw.strip_prefix(&home)
            .map_or(raw.clone(), |suffix| format!("~{suffix}"))
    })
}

fn run_plugin_manifest_plan(argv: &[String]) -> CliOutput {
    let action = match parse_plugin_manifest_args(argv) {
        Ok(action) => action,
        Err(message) => return plugin_manifest_usage_error(&message),
    };
    match action {
        PluginManifestAction::Parse {
            plan_json,
            dir,
            json_text,
        } => match parse_manifest(&json_text, &dir) {
            Ok(manifest) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!(
                        "{{\"command\":\"plugin-manifest\",\"kind\":\"parse\",\"dir\":{},\"manifest\":{}}}\n",
                        json_string(&path_string(&dir)),
                        render_plugin_manifest_json(&manifest)
                    )
                } else {
                    format!("{}\n", manifest.name)
                },
                stderr: String::new(),
            },
            Err(message) => plugin_manifest_usage_error(&message),
        },
        PluginManifestAction::Load { plan_json, dir } => match load_manifest_from_dir(&dir) {
            Ok(plugin) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    let plugin_json = plugin
                        .as_ref()
                        .map_or_else(|| "null".to_owned(), render_loaded_plugin_json);
                    format!(
                        "{{\"command\":\"plugin-manifest\",\"kind\":\"load\",\"dir\":{},\"present\":{},\"plugin\":{plugin_json}}}\n",
                        json_string(&path_string(&dir)),
                        plugin.is_some()
                    )
                } else {
                    plugin.map_or_else(
                        || "missing\n".to_owned(),
                        |plugin| format!("{} {}\n", plugin.kind.as_str(), plugin.manifest.name),
                    )
                },
                stderr: String::new(),
            },
            Err(message) => plugin_manifest_usage_error(&message),
        },
        PluginManifestAction::Discover { plan_json, options } => {
            let report = discover_packages(&options);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_plugin_discover_json(&options, &report.plugins, &report.warnings)
                } else {
                    let mut names = report
                        .plugins
                        .iter()
                        .map(|plugin| plugin.manifest.name.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    names.push('\n');
                    names
                },
                stderr: String::new(),
            }
        }
        PluginManifestAction::ImportSymbol {
            plan_json,
            options,
            plugin,
            symbol,
            module_symbols,
        } => run_plugin_manifest_import_symbol_plan(
            plan_json,
            &options,
            &plugin,
            &symbol,
            &module_symbols,
        ),
        PluginManifestAction::Invoke {
            plan_json,
            options,
            plugin,
            source,
            args,
        } => run_plugin_manifest_invoke_plan(plan_json, &options, &plugin, source, args),
    }
}

fn run_plugin_manifest_import_symbol_plan(
    plan_json: bool,
    options: &DiscoverPackagesOptions,
    plugin: &str,
    symbol: &str,
    module_symbols: &BTreeMap<String, String>,
) -> CliOutput {
    let report = discover_packages(options);
    let mut module_path = None;
    match import_plugin_symbol(plugin, symbol, &report.plugins, |path| {
        module_path = Some(path.to_path_buf());
        Ok(module_symbols.clone())
    }) {
        Ok(value) => CliOutput {
            code: 0,
            stdout: if plan_json {
                render_plugin_import_symbol_json(
                    plugin,
                    symbol,
                    &value,
                    module_path.as_deref(),
                    &report.warnings,
                )
            } else {
                format!("{value}\n")
            },
            stderr: String::new(),
        },
        Err(message) => plugin_manifest_usage_error(&message),
    }
}

fn run_plugin_manifest_invoke_plan(
    plan_json: bool,
    options: &DiscoverPackagesOptions,
    plugin_name: &str,
    source: InvokeSource,
    args: Vec<String>,
) -> CliOutput {
    let report = discover_packages(options);
    let Some(plugin) = report
        .plugins
        .iter()
        .find(|plugin| plugin.manifest.name == plugin_name)
    else {
        return plugin_manifest_usage_error(&format!("plugin '{plugin_name}' not found"));
    };
    if plugin.disabled {
        return plugin_manifest_usage_error(&format!("plugin '{plugin_name}' is disabled"));
    }
    let ctx = InvokeContext::new(source, args);
    if plugin.kind == LoadedPluginKind::Ts && !plugin_manifest_invoke_is_universal(&ctx) {
        return plugin_manifest_ts_refusal(plugin);
    }

    let mut runtime = ExtismWasmInvokeRuntime::default().with_manifest_fs_roots();
    let result = invoke_plugin(plugin, &ctx, &mut runtime);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_plugin_invoke_json(plugin, &ctx, &result, &report.warnings)
        } else if result.ok {
            result
                .output
                .map_or_else(|| "ok\n".to_owned(), |output| format!("{output}\n"))
        } else {
            format!("{}\n", result.error.unwrap_or_else(|| "error".to_owned()))
        },
        stderr: String::new(),
    }
}

fn plugin_manifest_invoke_is_universal(ctx: &InvokeContext) -> bool {
    matches!(ctx.source, InvokeSource::Cli)
        && ctx.args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "--version"))
}

fn plugin_manifest_ts_refusal(plugin: &LoadedPlugin) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "plugin-manifest invoke: TS source plugin '{}' is not executable in the maw-rs Extism-WASM runtime. Build this plugin to WASM and point plugin.json at the WASM artifact (target=wasm / wasm=<file> or entry.kind=wasm). No Bun/JS subprocess fallback is available.\n",
            plugin.manifest.name
        ),
    }
}

enum PluginManifestAction {
    Parse {
        plan_json: bool,
        dir: std::path::PathBuf,
        json_text: String,
    },
    Load {
        plan_json: bool,
        dir: std::path::PathBuf,
    },
    Discover {
        plan_json: bool,
        options: DiscoverPackagesOptions,
    },
    ImportSymbol {
        plan_json: bool,
        options: DiscoverPackagesOptions,
        plugin: String,
        symbol: String,
        module_symbols: BTreeMap<String, String>,
    },
    Invoke {
        plan_json: bool,
        options: DiscoverPackagesOptions,
        plugin: String,
        source: InvokeSource,
        args: Vec<String>,
    },
}

fn parse_plugin_manifest_args(argv: &[String]) -> Result<PluginManifestAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err("plugin-manifest: expected parse or load".to_owned());
    };
    match kind {
        "parse" => parse_plugin_manifest_parse_args(&argv[1..]),
        "load" => parse_plugin_manifest_load_args(&argv[1..]),
        "discover" => parse_plugin_manifest_discover_args(&argv[1..]),
        "import-symbol" => parse_plugin_manifest_import_symbol_args(&argv[1..]),
        "invoke" => parse_plugin_manifest_invoke_args(&argv[1..]),
        other => Err(format!("plugin-manifest: unknown subcommand {other}")),
    }
}

fn parse_plugin_manifest_parse_args(argv: &[String]) -> Result<PluginManifestAction, String> {
    let mut plan_json = false;
    let mut dir = std::path::PathBuf::from(".");
    let mut json_text = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--dir" => {
                dir = take_plugin_manifest_path(argv, index, "--dir")?;
                index += 1;
            }
            "--json" => {
                json_text = Some(take_plugin_manifest_value(argv, index, "--json")?);
                index += 1;
            }
            other => return Err(format!("plugin-manifest parse: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(PluginManifestAction::Parse {
        plan_json,
        dir,
        json_text: json_text
            .ok_or_else(|| "plugin-manifest parse: --json is required".to_owned())?,
    })
}

fn parse_plugin_manifest_load_args(argv: &[String]) -> Result<PluginManifestAction, String> {
    let mut plan_json = false;
    let mut dir = std::path::PathBuf::from(".");
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--dir" => {
                dir = take_plugin_manifest_path(argv, index, "--dir")?;
                index += 1;
            }
            other => return Err(format!("plugin-manifest load: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(PluginManifestAction::Load { plan_json, dir })
}

fn parse_plugin_manifest_discover_args(argv: &[String]) -> Result<PluginManifestAction, String> {
    let (plan_json, options, _) = parse_plugin_manifest_registry_args(argv, false)?;
    Ok(PluginManifestAction::Discover { plan_json, options })
}

fn parse_plugin_manifest_import_symbol_args(
    argv: &[String],
) -> Result<PluginManifestAction, String> {
    let (plan_json, options, import) = parse_plugin_manifest_registry_args(argv, true)?;
    let import = import.expect("import parser requested import args");
    Ok(PluginManifestAction::ImportSymbol {
        plan_json,
        options,
        plugin: import.plugin,
        symbol: import.symbol,
        module_symbols: import.module_symbols,
    })
}

fn parse_plugin_manifest_invoke_args(argv: &[String]) -> Result<PluginManifestAction, String> {
    let mut plan_json = false;
    let mut scan_dirs = Vec::new();
    let mut disabled_plugins = Vec::new();
    let mut runtime_version = "1.0.0".to_owned();
    let mut use_cache = false;
    let mut plugin = None;
    let mut source = InvokeSource::Cli;
    let mut invoke_args = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--scan-dir" => {
                scan_dirs.push(take_plugin_manifest_path(argv, index, "--scan-dir")?);
                index += 1;
            }
            "--disabled" => {
                disabled_plugins.push(take_plugin_manifest_value(argv, index, "--disabled")?);
                index += 1;
            }
            "--runtime-version" => {
                runtime_version = take_plugin_manifest_value(argv, index, "--runtime-version")?;
                index += 1;
            }
            "--use-cache" => use_cache = true,
            "--plugin" => {
                plugin = Some(take_plugin_manifest_value(argv, index, "--plugin")?);
                index += 1;
            }
            "--source" => {
                source = parse_plugin_manifest_invoke_source(&take_plugin_manifest_value(
                    argv, index, "--source",
                )?)?;
                index += 1;
            }
            "--arg" => {
                invoke_args.push(take_plugin_manifest_value(argv, index, "--arg")?);
                index += 1;
            }
            other => return Err(format!("plugin-manifest invoke: unknown argument {other}")),
        }
        index += 1;
    }
    if scan_dirs.is_empty() {
        return Err("plugin-manifest invoke: --scan-dir is required".to_owned());
    }
    Ok(PluginManifestAction::Invoke {
        plan_json,
        options: DiscoverPackagesOptions {
            scan_dirs,
            disabled_plugins,
            runtime_version,
            use_cache,
        },
        plugin: plugin.ok_or_else(|| "plugin-manifest invoke: --plugin is required".to_owned())?,
        source,
        args: invoke_args,
    })
}
