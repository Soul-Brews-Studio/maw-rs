// `maw update` / `maw upgrade` — native self-update from GitHub releases.
// Planning is pure (update_plan.rs); this file parses argv and runs effects.
// Invariant: this command never reads or writes any file under ~/.maw
// (the maw-js updater once wiped namedPeers/federationToken — see tests).

const DISPATCH_150: &[DispatcherEntry] = &[
    DispatcherEntry { command: "update", handler: Handler::Sync(update_run_command) },
    DispatcherEntry { command: "upgrade", handler: Handler::Sync(upgrade_run_command) },
];

const UPDATE_USAGE: &str = "usage: maw update [--check] [--channel stable|alpha] [--version vYY.M.DD] [--force]\n\n  Self-update maw-rs from GitHub releases (Soul-Brews-Studio/maw-rs).\n  The channel defaults to the running build's channel: a hyphenated\n  version (e.g. 26.7.16-alpha.1159) means alpha, plain means stable.\n\n  Flags:\n    --check            report the newest release on the channel, change nothing\n    --channel <name>   stable | alpha (also accepted as a bare positional)\n    --version <tag>    install an exact release tag, e.g. v26.7.16\n    --force            reinstall even when already up to date (or a dev build)\n    --yes, -y          accepted for script compatibility (no prompt exists)\n    --help, -h         show this message and exit (no side effects)\n\n  sha256 verification against the release .sha256 sidecar is mandatory;\n  a mismatch always refuses to install (there is no override flag).";

const UPDATE_USER_AGENT: &str = "maw-rs-update";

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateRequest150 {
    command: &'static str,
    help: bool,
    check: bool,
    force: bool,
    channel: Option<UpdateChannel>,
    version: Option<String>,
}

fn update_run_command(argv: &[String]) -> CliOutput { update_run_command_in("update", argv) }

fn upgrade_run_command(argv: &[String]) -> CliOutput { update_run_command_in("upgrade", argv) }

fn update_run_command_in(command: &'static str, argv: &[String]) -> CliOutput {
    match update_parse_request(command, argv) {
        Ok(request) if request.help => update_output(0, format!("{UPDATE_USAGE}\n"), String::new()),
        Ok(request) => match update_execute(&request) {
            Ok(stdout) => update_output(0, stdout, String::new()),
            Err(message) => update_output(1, String::new(), format!("\u{1b}[31merror\u{1b}[0m: {message}\n")),
        },
        Err(message) => update_output(1, String::new(), format!("\u{1b}[31merror\u{1b}[0m: {message}\n")),
    }
}

fn update_parse_request(command: &'static str, argv: &[String]) -> Result<UpdateRequest150, String> {
    let mut request = UpdateRequest150 {
        command,
        help: false,
        check: false,
        force: false,
        channel: None,
        version: None,
    };
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("arguments must not contain NUL or control characters".to_owned());
        }
        match arg.as_str() {
            "--help" | "-h" => request.help = true,
            "--check" => request.check = true,
            "--force" => request.force = true,
            "--yes" | "-y" => {}
            "--" => return Err(format!("-- separator is not allowed for maw {command}")),
            "--channel" => {
                let value = iter.next().ok_or_else(|| "--channel requires a value (stable|alpha)".to_owned())?;
                request.channel = Some(update_channel_from_name(value)?);
            }
            "--version" => {
                let value = iter.next().ok_or_else(|| "--version requires a value, e.g. v26.7.16".to_owned())?;
                request.version = Some(update_validate_version_arg(value)?);
            }
            "stable" | "alpha" => request.channel = Some(update_channel_from_name(arg)?),
            value => {
                if let Some(rest) = value.strip_prefix("--channel=") {
                    request.channel = Some(update_channel_from_name(rest)?);
                } else if let Some(rest) = value.strip_prefix("--version=") {
                    request.version = Some(update_validate_version_arg(rest)?);
                } else if value.starts_with('v') && value.contains('.') {
                    request.version = Some(update_validate_version_arg(value)?);
                } else if value.starts_with('-') {
                    return Err(format!("unknown flag \"{value}\" — run `maw {command} --help` for usage"));
                } else {
                    return Err(format!(
                        "unexpected argument \"{value}\" — expected stable, alpha, or a release tag like v26.7.16"
                    ));
                }
            }
        }
    }
    Ok(request)
}

fn update_validate_version_arg(value: &str) -> Result<String, String> {
    let shaped = value.starts_with('v')
        && value.len() > 1
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'));
    if shaped {
        Ok(value.to_owned())
    } else {
        Err(format!("invalid version \"{value}\" — expected a release tag like v26.7.16 or v26.7.16-alpha.1159"))
    }
}

fn update_execute(request: &UpdateRequest150) -> Result<String, String> {
    use std::fmt::Write as _;

    let local = MAW_RS_BUILD_VERSION;
    let channel = request.channel.unwrap_or_else(|| update_infer_channel(local));
    let mut out = String::new();
    let _ = writeln!(out, "maw {}: current {local} ({} channel)", request.command, channel.as_str());

    let target = if let Some(version) = &request.version {
        update_classify_tag(version).ok_or_else(|| {
            format!("unrecognized release tag \"{version}\" — expected vYY.M.DD or vYY.M.DD-alpha.HMM")
        })?
    } else {
        let tags = update_fetch_release_tags()?;
        update_pick_latest_tag(&tags, channel).ok_or_else(|| {
            format!("no {} releases found in the last 50 releases of Soul-Brews-Studio/maw-rs", channel.as_str())
        })?
    };

    let compare = update_compare_to_local(local, &target);
    let (status, proceed) = match compare {
        UpdateCompare::RemoteNewer => ("update available", true),
        UpdateCompare::Equal => ("already up to date", request.force),
        UpdateCompare::RemoteOlder => ("older than the current build", request.force),
        UpdateCompare::LocalDev => ("current build is a dev build (git describe) — not comparable", request.force),
    };
    let _ = writeln!(out, "  target: {} — {status}", target.tag);

    if request.check {
        return Ok(out);
    }
    if !proceed {
        let advice = if compare == UpdateCompare::LocalDev {
            "  dev builds update via git + cargo; pass --force to overwrite with the release binary anyway\n"
        } else {
            "  nothing to do (pass --force to reinstall)\n"
        };
        out.push_str(advice);
        return Ok(out);
    }

    let install_target = update_resolve_install_target()?;
    let asset = update_asset_for_platform(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "no prebuilt release asset for {}/{} — supported: macos/aarch64, linux/x86_64 (musl); build from source instead",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let staging = std::env::temp_dir().join(format!("maw-update-{}", std::process::id()));
    std::fs::create_dir_all(&staging).map_err(|error| format!("cannot create staging dir {}: {error}", staging.display()))?;
    let result = update_download_verify_replace(&mut out, &target.tag, asset, &staging, &install_target);
    let _ = std::fs::remove_dir_all(&staging);
    result?;

    let proof = update_spawn_version_proof(&install_target)?;
    let _ = writeln!(out, "  proof (`{} version`):\n{proof}", install_target.display());
    if update_maw_serve_running() {
        out.push_str("  maw-serve runs under PM2 — restart it to pick up the new binary: pm2 restart maw-serve\n");
    }
    Ok(out)
}

fn update_download_verify_replace(
    out: &mut String,
    tag: &str,
    asset: &str,
    staging: &std::path::Path,
    install_target: &std::path::Path,
) -> Result<(), String> {
    use std::fmt::Write as _;

    let asset_url = update_download_url(tag, asset);
    let _ = writeln!(out, "  downloading {asset} ({tag}) ...");
    let binary_bytes = update_http_get_bytes(&asset_url, std::time::Duration::from_mins(10))?;
    let sidecar_bytes = update_http_get_bytes(&format!("{asset_url}.sha256"), std::time::Duration::from_mins(1))?;
    let expected = update_parse_sha256_sidecar(&String::from_utf8_lossy(&sidecar_bytes))
        .ok_or_else(|| format!("malformed sha256 sidecar for {asset} — refusing to install"))?;

    let staged = staging.join(asset);
    std::fs::write(&staged, &binary_bytes).map_err(|error| format!("cannot write {}: {error}", staged.display()))?;
    update_verify_sha256(&staged, &expected)?;
    let _ = writeln!(out, "  sha256 verified: {expected}");

    let backup = update_safe_replace(install_target, &staged)?;
    if let Some(backup) = backup {
        let _ = std::fs::remove_file(&backup);
    }
    let _ = writeln!(out, "  installed: {} (old binary removed first — new inode)", install_target.display());
    Ok(())
}

fn update_resolve_install_target() -> Result<std::path::PathBuf, String> {
    let path = if let Some(value) = std::env::var_os("MAW_RS_SELF_BIN") {
        std::path::PathBuf::from(value)
    } else {
        std::env::current_exe().map_err(|error| format!("cannot locate the running binary: {error}"))?
    };
    let resolved = std::fs::canonicalize(&path).unwrap_or(path);
    update_target_dir_guard(&resolved)?;
    Ok(resolved)
}

/// sha256 verification is mandatory; a mismatch always refuses (no override).
fn update_verify_sha256(path: &std::path::Path, expected_hex: &str) -> Result<(), String> {
    let observed = hash_file(path)?;
    let observed_hex = observed.strip_prefix("sha256:").unwrap_or(observed.as_str());
    if observed_hex == expected_hex {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch for {}: expected {expected_hex}, got {observed_hex} — refusing to install",
            path.display()
        ))
    }
}

/// Safe replace honoring the macOS code-sign scar: never write into the live
/// inode. Stage a copy beside the target, move the old binary aside first,
/// then rename the new file into place. Returns the backup path, if any.
fn update_safe_replace(
    target: &std::path::Path,
    staged: &std::path::Path,
) -> Result<Option<std::path::PathBuf>, String> {
    let dir = target.parent().ok_or_else(|| format!("install target {} has no parent directory", target.display()))?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("install target {} has no file name", target.display()))?;
    let tmp = dir.join(format!(".{name}.update-new-{}", std::process::id()));
    std::fs::copy(staged, &tmp).map_err(|error| {
        format!("cannot stage new binary at {} (install dir not writable?): {error}", tmp.display())
    })?;
    update_mark_executable(&tmp)?;
    let backup = if target.exists() {
        let aside = dir.join(format!("{name}.bak-update-{}", std::process::id()));
        std::fs::rename(target, &aside)
            .map_err(|error| format!("cannot move old binary aside to {}: {error}", aside.display()))?;
        Some(aside)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&tmp, target) {
        if let Some(aside) = &backup {
            let _ = std::fs::rename(aside, target);
        }
        return Err(format!("cannot move new binary into place at {}: {error}", target.display()));
    }
    Ok(backup)
}

#[cfg(unix)]
fn update_mark_executable(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot chmod {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn update_mark_executable(_path: &std::path::Path) -> Result<(), String> { Ok(()) }

fn update_spawn_version_proof(binary: &std::path::Path) -> Result<String, String> {
    let output = std::process::Command::new(binary)
        .arg("version")
        .output()
        .map_err(|error| format!("new binary failed to spawn ({} version): {error} — backup kept beside it", binary.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
    } else {
        Err(format!("new binary exited nonzero from `{} version`", binary.display()))
    }
}

fn update_maw_serve_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "maw-serve"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn update_fetch_release_tags() -> Result<Vec<String>, String> {
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/releases?per_page=50");
    let bytes = update_http_get_bytes(&url, std::time::Duration::from_secs(30))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("releases API returned invalid JSON: {error}"))?;
    let releases = value
        .as_array()
        .ok_or_else(|| "releases API returned a non-array response".to_owned())?;
    Ok(releases
        .iter()
        .filter_map(|release| release.get("tag_name").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .collect())
}

fn update_http_get_bytes(url: &str, timeout: std::time::Duration) -> Result<Vec<u8>, String> {
    let url = url.to_owned();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("update runtime failed: {error}"))?;
        runtime.block_on(update_http_get_bytes_async(&url, timeout))
    });
    handle
        .join()
        .unwrap_or_else(|_| Err("update download thread panicked".to_owned()))
}

async fn update_http_get_bytes_async(url: &str, timeout: std::time::Duration) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(UPDATE_USER_AGENT)
        .build()
        .map_err(|error| format!("http client failed: {error}"))?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json, application/octet-stream")
        .send()
        .await
        .map_err(|error| format!("GET {url} failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("GET {url} -> HTTP {status} (release or asset not found?)"));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("GET {url} body failed: {error}"))
}

fn update_output(code: i32, stdout: String, stderr: String) -> CliOutput { CliOutput { code, stdout, stderr } }

#[cfg(test)]
mod update_upgrade_tests150 {
    use super::*;

    static UPDATE_TEMP_COUNTER: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn update_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// Unique-per-test temp root (pid alone is shared across parallel tests).
    fn update_temp_root(label: &str) -> std::path::PathBuf {
        let counter = UPDATE_TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "maw-rs-update-{label}-{}-{counter}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    #[test]
    fn update_dispatch_registers_update_and_upgrade_native() {
        assert_eq!(dispatcher_status("update"), DispatchKind::Native);
        assert_eq!(dispatcher_status("upgrade"), DispatchKind::Native);
        assert_eq!(DISPATCH_150.len(), 2);
        assert_eq!(DISPATCH_150[0].command, "update");
        assert_eq!(DISPATCH_150[1].command, "upgrade");
    }

    #[test]
    fn update_parse_flags_check_channel_version_force() {
        let parsed = update_parse_request("update", &update_args(&["--check"])).expect("parse");
        assert!(parsed.check);
        assert!(!parsed.force);
        assert_eq!(parsed.channel, None);
        assert_eq!(parsed.version, None);

        let parsed = update_parse_request(
            "upgrade",
            &update_args(&["--channel", "stable", "--force", "--yes"]),
        )
        .expect("parse");
        assert_eq!(parsed.command, "upgrade");
        assert_eq!(parsed.channel, Some(UpdateChannel::Stable));
        assert!(parsed.force);

        let parsed = update_parse_request(
            "update",
            &update_args(&["--channel=alpha", "--version=v26.7.16-alpha.1159"]),
        )
        .expect("parse");
        assert_eq!(parsed.channel, Some(UpdateChannel::Alpha));
        assert_eq!(parsed.version.as_deref(), Some("v26.7.16-alpha.1159"));

        // positional compatibility: channel names and v-tags
        let parsed = update_parse_request("update", &update_args(&["alpha"])).expect("parse");
        assert_eq!(parsed.channel, Some(UpdateChannel::Alpha));
        let parsed = update_parse_request("update", &update_args(&["v26.7.16"])).expect("parse");
        assert_eq!(parsed.version.as_deref(), Some("v26.7.16"));
    }

    #[test]
    fn update_parse_rejects_bad_flags_channels_versions_and_control_chars() {
        assert!(update_parse_request("update", &update_args(&["--yess"]))
            .expect_err("flag")
            .contains("unknown flag"));
        assert!(update_parse_request("update", &update_args(&["--channel", "beta"]))
            .expect_err("channel")
            .contains("unknown channel"));
        assert!(update_parse_request("update", &update_args(&["--version", "26.7.16"]))
            .expect_err("version")
            .contains("invalid version"));
        assert!(update_parse_request("update", &update_args(&["--channel"]))
            .expect_err("value")
            .contains("requires a value"));
        assert!(update_parse_request("update", &update_args(&["--"]))
            .expect_err("separator")
            .contains("not allowed"));
        assert!(update_parse_request("update", &["main\nnext".to_owned()])
            .expect_err("control")
            .contains("control"));
        assert!(update_parse_request("update", &update_args(&["$(whoami)"]))
            .expect_err("positional")
            .contains("unexpected argument"));
    }

    #[test]
    fn update_help_short_circuits_with_native_usage() {
        let out = update_run_command(&update_args(&["--help"]));
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("usage: maw update"));
        assert!(out.stdout.contains("sha256"));
        assert!(out.stderr.is_empty());
    }

    #[test]
    fn update_sha256_verify_refuses_mismatch() {
        let root = update_temp_root("sha");
        let file = root.join("asset");
        std::fs::write(&file, b"correct bytes").expect("write");
        let good = hash_file(&file).expect("hash");
        let good_hex = good.strip_prefix("sha256:").expect("prefix").to_owned();
        assert!(update_verify_sha256(&file, &good_hex).is_ok());

        // feed wrong bytes under the same expected hash: must refuse
        std::fs::write(&file, b"tampered bytes").expect("write");
        let error = update_verify_sha256(&file, &good_hex).expect_err("mismatch");
        assert!(error.contains("sha256 mismatch"));
        assert!(error.contains("refusing to install"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn update_safe_replace_puts_new_inode_at_path_and_moves_old_aside() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let root = update_temp_root("replace");
        let target = root.join("maw");
        std::fs::write(&target, b"old binary").expect("old");
        let old_inode = std::fs::metadata(&target).expect("meta").ino();

        let staged = root.join("staged-download");
        std::fs::write(&staged, b"new binary").expect("staged");

        let backup = update_safe_replace(&target, &staged)
            .expect("replace")
            .expect("backup path");
        let new_meta = std::fs::metadata(&target).expect("meta");
        assert_eq!(std::fs::read(&target).expect("read"), b"new binary");
        assert_ne!(new_meta.ino(), old_inode, "live inode must never be reused");
        assert_eq!(new_meta.permissions().mode() & 0o111, 0o111, "exec bits set");
        assert_eq!(std::fs::read(&backup).expect("backup"), b"old binary");
        assert_eq!(
            std::fs::metadata(&backup).expect("backup meta").ino(),
            old_inode,
            "old inode moved aside, not overwritten"
        );

        // no pre-existing target: replace still installs, no backup
        let fresh = root.join("fresh");
        std::fs::write(&staged, b"fresh binary").expect("staged");
        assert!(update_safe_replace(&fresh, &staged).expect("replace").is_none());
        assert_eq!(std::fs::read(&fresh).expect("read"), b"fresh binary");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn update_never_references_fleet_settings_scar_invariant() {
        // the maw-js updater once wiped namedPeers/federationToken by
        // rewriting the fleet settings file; the native updater must never
        // reference that file, in any code path. Comments are stripped so the
        // invariant checks code, not prose.
        let sources = [
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core_impl/update.rs")),
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core_impl/update_plan.rs")),
        ];
        let code: String = sources
            .iter()
            .flat_map(|source| source.lines())
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        let forbidden = [
            format!(".{}", "maw/"),
            format!("con{}", "fig"),
            format!("home{}", "_dir"),
            format!("named{}", "peers"),
            format!("federation{}", "token"),
        ];
        for token in &forbidden {
            assert!(
                !code.contains(token.as_str()),
                "update source must never reference {token} (fleet settings wipe scar)"
            );
        }
    }

    /// The single allowed live-network test. Run manually:
    /// `cargo test -p maw-cli --lib update_live_releases_api -- --ignored`
    #[test]
    #[ignore = "live network: hits the real GitHub releases API"]
    fn update_live_releases_api_lists_channel_tags() {
        let tags = update_fetch_release_tags().expect("releases API");
        assert!(!tags.is_empty(), "expected at least one release tag");
        let alpha = update_pick_latest_tag(&tags, UpdateChannel::Alpha);
        let stable = update_pick_latest_tag(&tags, UpdateChannel::Stable);
        assert!(
            alpha.is_some() || stable.is_some(),
            "expected at least one classifiable release among {tags:?}"
        );
    }
}
