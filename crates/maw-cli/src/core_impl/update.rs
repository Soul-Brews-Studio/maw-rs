// `maw update` / `maw upgrade` — native self-update from GitHub releases.
// Planning is pure (update_plan.rs); this file parses argv and runs effects.
// Invariant: this command never reads or writes any file under ~/.maw
// (the maw-js updater once wiped namedPeers/federationToken — see tests).

const DISPATCH_150: &[DispatcherEntry] = &[
    DispatcherEntry { command: "update", handler: Handler::Sync(update_run_command) },
    DispatcherEntry { command: "upgrade", handler: Handler::Sync(upgrade_run_command) },
];

const UPDATE_USAGE: &str = "usage: maw update [--check] [--channel stable|alpha] [--version vYY.M.DD] [--force]\n\n  Self-update maw-rs from GitHub releases (Soul-Brews-Studio/maw-rs).\n  The channel defaults to the running build's channel: a hyphenated\n  version (e.g. 26.7.16-alpha.1159) means alpha, plain means stable.\n\n  Flags:\n    --check            report the newest release on the channel, change nothing\n    --channel <name>   stable | alpha (also accepted as a bare positional)\n    --version <tag>    install an exact release tag, e.g. v26.7.16\n    --force            reinstall even when already up to date (or a dev build)\n    --yes, -y          accepted for script compatibility (no prompt exists)\n    --help, -h         show this message and exit (no side effects)\n\n  Linux ships two x86_64 builds and they are not interchangeable:\n    gnu   dynamic, needs a glibc host, resolves .local/mDNS names\n    musl  static, portable across distros, CANNOT resolve .local/mDNS\n  The host libc is detected automatically (glibc preferred, musl when\n  unclear); set MAW_LIBC=gnu|musl to force one.\n\n  sha256 verification against the release .sha256 sidecar is mandatory;\n  a mismatch always refuses to install (there is no override flag).";

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
    let local_is_beta = local.contains("-beta.");
    let (status, proceed) = match compare {
        UpdateCompare::RemoteNewer => ("update available", true),
        UpdateCompare::Equal => ("already up to date", request.force),
        UpdateCompare::RemoteOlder => ("older than the current build", request.force),
        UpdateCompare::LocalDev if local_is_beta => {
            ("current build is a beta build — beta is a separate channel, not comparable to stable/alpha", request.force)
        }
        UpdateCompare::LocalDev => ("current build is a dev build (git describe) — not comparable", request.force),
    };
    let _ = writeln!(out, "  target: {} — {status}", target.tag);

    if request.check {
        return Ok(out);
    }
    if !proceed {
        let advice = if compare == UpdateCompare::LocalDev {
            if local_is_beta {
                "  beta builds are released on their own channel; pass --force to overwrite with the stable/alpha release binary anyway\n"
            } else {
                "  dev builds update via git + cargo; pass --force to overwrite with the release binary anyway\n"
            }
        } else {
            "  nothing to do (pass --force to reinstall)\n"
        };
        out.push_str(advice);
        return Ok(out);
    }

    let install_target = update_resolve_install_target()?;
    let libc = update_detect_host_libc();
    let asset = update_asset_for_platform(std::env::consts::OS, std::env::consts::ARCH, libc).ok_or_else(|| {
        format!(
            "no prebuilt release asset for {}/{} — supported: macos/aarch64, linux/x86_64 (gnu and musl); build from source instead",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    if std::env::consts::OS == "linux" {
        let _ = writeln!(out, "  host libc: {}{}", update_libc_label(libc), update_libc_note(libc));
    }

    // The asset that actually landed, which is not always the one asked for:
    // a release without the gnu artifact degrades to musl. Annotating a proof
    // failure against the requested asset would tell a host that just received
    // the musl build to retry with MAW_LIBC=musl.
    let (backup, installed_asset) =
        update_download_verify_replace(&mut out, &target.tag, asset, &install_target)?;

    let proof = update_prove_and_finalize(&install_target, backup.as_deref())
        .map_err(|error| update_annotate_proof_failure(error, installed_asset))?;
    let _ = writeln!(out, "  proof (`{} version`):\n{proof}", install_target.display());
    if update_maw_serve_running() {
        out.push_str("  maw-serve runs under PM2 — restart it to pick up the new binary: pm2 restart maw-serve\n");
    }
    Ok(out)
}

/// Host libc for Linux asset selection.
///
/// An explicit `MAW_LIBC=gnu|musl` wins (same variable install.sh honors);
/// otherwise gather evidence — `ldd --version` (musl's ldd writes to stderr
/// and exits nonzero, so both streams are read) plus a glibc loader probe —
/// and let the pure `update_classify_libc` judge it. Unclear evidence means
/// musl: a static binary that runs beats a dynamic one that does not.
fn update_detect_host_libc() -> UpdateLibc {
    let forced = std::env::var("MAW_LIBC").ok();
    if let Some(libc) = update_libc_from_override(forced.as_deref()) {
        return libc;
    }
    let ldd = std::process::Command::new("ldd")
        .arg("--version")
        .output()
        .ok()
        .map(|output| {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            text
        });
    let loader_present =
        UPDATE_GLIBC_LOADER_PATHS.iter().any(|path| std::path::Path::new(path).exists());
    update_classify_libc(ldd.as_deref(), loader_present)
}

fn update_libc_label(libc: UpdateLibc) -> &'static str {
    match libc {
        UpdateLibc::Gnu => "glibc",
        UpdateLibc::Musl => "musl",
    }
}

/// A gnu asset that will not start is almost always a glibc floor problem —
/// the release is built against the CI runner's glibc, which can be newer than
/// an older host's. The previous binary has already been restored by then, so
/// the only thing missing is the way out: name it.
fn update_annotate_proof_failure(error: String, asset: &str) -> String {
    if asset.ends_with("-gnu") {
        format!(
            "{error}\n  the glibc build may need a newer glibc than this host has — retry with MAW_LIBC=musl for the static build (which cannot resolve .local/mDNS)"
        )
    } else {
        error
    }
}

/// The operator-facing reason the choice matters (#812).
fn update_libc_note(libc: UpdateLibc) -> &'static str {
    match libc {
        UpdateLibc::Gnu => " — dynamic build, resolves .local/mDNS via the system resolver",
        UpdateLibc::Musl => {
            " — static build, portable but cannot resolve .local/mDNS (override: MAW_LIBC=gnu)"
        }
    }
}

/// Download the asset + sha256 sidecar and swap the new binary into place.
///
/// No shared temp dir: the bytes are staged as a dotfile inside the install
/// directory itself and the sha256 is verified on that exact file before it
/// is renamed into place, so there is no verify-then-copy swap window.
/// Returns the backup path (old binary moved aside); the caller keeps it
/// until the version proof passes and restores it if the proof fails.
/// The asset to try after `asset` turned out not to exist in a release.
///
/// Only ever gnu -> musl, and only once: musl is the static build that runs
/// anywhere, so it is the floor. A missing musl or macOS asset is a real
/// packaging failure with nothing to degrade to, and must stay fatal.
fn update_asset_fallback_after_not_found(asset: &str) -> Option<&'static str> {
    asset.ends_with("-gnu").then_some("maw-rs-linux-x86_64-musl")
}

fn update_download_verify_replace<'a>(
    out: &mut String,
    tag: &str,
    asset: &'a str,
    install_target: &std::path::Path,
) -> Result<(Option<std::path::PathBuf>, &'a str), String> {
    use std::fmt::Write as _;

    let mut asset = asset;
    let mut asset_url = update_download_url(tag, asset);
    let _ = writeln!(out, "  downloading {asset} ({tag}) ...");
    let binary_bytes = match update_http_get_bytes(&asset_url, std::time::Duration::from_mins(10)) {
        Ok(bytes) => bytes,
        // #812: every release cut before the gnu artifact existed — and every
        // pinned or stable tag, permanently — carries musl only. Degrade to it
        // rather than abort, but say so: a glibc host that silently receives
        // the static build loses .local/mDNS resolution, which is the very bug
        // the gnu artifact was added to fix.
        Err(error) => {
            let Some(fallback) =
                update_asset_fallback_after_not_found(asset).filter(|_| error.is_not_found())
            else {
                return Err(error.into_message());
            };
            let _ = writeln!(out, "  {asset} is not published for {tag} — falling back to {fallback}");
            let _ = writeln!(out, "  note: the musl build CANNOT resolve .local/mDNS names (#812)");
            asset = fallback;
            asset_url = update_download_url(tag, asset);
            update_http_get_bytes(&asset_url, std::time::Duration::from_mins(10))
                .map_err(UpdateHttpError::into_message)?
        }
    };
    let sidecar_bytes = update_http_get_bytes(&format!("{asset_url}.sha256"), std::time::Duration::from_mins(1))
        .map_err(UpdateHttpError::into_message)?;
    let expected = update_parse_sha256_sidecar(&String::from_utf8_lossy(&sidecar_bytes))
        .ok_or_else(|| format!("malformed sha256 sidecar for {asset} — refusing to install"))?;

    let backup = update_safe_replace(install_target, &binary_bytes, &expected)?;
    let _ = writeln!(out, "  sha256 verified: {expected}");
    let _ = writeln!(out, "  installed: {} (old binary moved aside — new inode)", install_target.display());
    Ok((backup, asset))
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
/// inode. The new bytes are written to a dotfile beside the target, sha256 is
/// verified on that very file (the one that gets renamed — no swap window),
/// the old binary is moved aside, then the new file is renamed into place.
/// Returns the backup path, if any; the caller deletes it only after the
/// version proof passes.
fn update_safe_replace(
    target: &std::path::Path,
    new_bytes: &[u8],
    expected_sha256: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let dir = target.parent().ok_or_else(|| format!("install target {} has no parent directory", target.display()))?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("install target {} has no file name", target.display()))?;
    let tmp = dir.join(format!(".{name}.update-new-{}", std::process::id()));
    std::fs::write(&tmp, new_bytes).map_err(|error| {
        format!("cannot stage new binary at {} (install dir not writable?): {error}", tmp.display())
    })?;
    if let Err(error) = update_verify_sha256(&tmp, expected_sha256).and_then(|()| update_mark_executable(&tmp)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    let backup = if target.exists() {
        let aside = dir.join(format!("{name}.bak-update-{}", std::process::id()));
        if let Err(error) = std::fs::rename(target, &aside) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("cannot move old binary aside to {}: {error}", aside.display()));
        }
        Some(aside)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
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

/// Run the version proof on the freshly installed binary, then settle the
/// backup: proof passes → delete the backup; proof fails → rename the backup
/// back over the target and say exactly what was restored.
fn update_prove_and_finalize(
    target: &std::path::Path,
    backup: Option<&std::path::Path>,
) -> Result<String, String> {
    match update_spawn_version_proof(target) {
        Ok(proof) => {
            if let Some(backup) = backup {
                let _ = std::fs::remove_file(backup);
            }
            Ok(proof)
        }
        Err(error) => Err(match backup {
            Some(backup) => match std::fs::rename(backup, target) {
                Ok(()) => format!("{error} — restored the previous binary from {}", backup.display()),
                Err(restore_error) => format!(
                    "{error} — and restoring the previous binary from {} failed: {restore_error}",
                    backup.display()
                ),
            },
            None => format!("{error} — no previous binary existed to restore"),
        }),
    }
}

/// `ETXTBSY` — the kernel refuses to `execve` a file some process still holds
/// open for writing. We have just written this binary, and while our own write
/// fd is closed, any thread that forked in that window leaked it into a child
/// that has not `execve`d yet (`O_CLOEXEC` closes at exec, not at fork). The
/// window is microseconds wide, but losing the race here would roll back a
/// perfectly good new binary, so retry briefly before believing it.
const UPDATE_ETXTBSY: i32 = 26;

fn update_spawn_version_proof(binary: &std::path::Path) -> Result<String, String> {
    let mut attempt = 0_u32;
    let output = loop {
        match std::process::Command::new(binary).arg("version").output() {
            Err(error) if error.raw_os_error() == Some(UPDATE_ETXTBSY) && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(5 * u64::from(attempt)));
            }
            other => {
                break other.map_err(|error| {
                    format!(
                        "new binary failed to spawn (`{} version`): {error}",
                        binary.display()
                    )
                })?
            }
        }
    };
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

/// A failed GET, split so callers can tell "this object does not exist" from
/// every other reason a request can fail.
///
/// #812: only a 404 means a release simply does not carry an asset, which is
/// recoverable by choosing a different one. A 403, a rate-limit, a 5xx or a
/// dropped connection must never be read as absence — silently downgrading a
/// glibc host to the musl build because GitHub had a bad minute reintroduces
/// the .local/mDNS bug this change exists to fix, with no indication why.
enum UpdateHttpError {
    NotFound(String),
    Other(String),
}

/// Classify a non-2xx response. Exactly 404 counts as absence — never a 4xx
/// range: a 403 is a rate-limit or a private repo, a 401 is bad credentials,
/// and neither means the asset does not exist.
fn update_http_status_error(url: &str, status: reqwest::StatusCode) -> UpdateHttpError {
    let message = format!("GET {url} -> HTTP {status}");
    if status == reqwest::StatusCode::NOT_FOUND {
        UpdateHttpError::NotFound(message)
    } else {
        UpdateHttpError::Other(message)
    }
}

impl UpdateHttpError {
    fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    fn into_message(self) -> String {
        match self {
            Self::NotFound(message) | Self::Other(message) => message,
        }
    }
}

fn update_fetch_release_tags() -> Result<Vec<String>, String> {
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/releases?per_page=50");
    let bytes = update_http_get_bytes(&url, std::time::Duration::from_secs(30))
        .map_err(UpdateHttpError::into_message)?;
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

fn update_http_get_bytes(url: &str, timeout: std::time::Duration) -> Result<Vec<u8>, UpdateHttpError> {
    let url = url.to_owned();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| UpdateHttpError::Other(format!("update runtime failed: {error}")))?;
        runtime.block_on(update_http_get_bytes_async(&url, timeout))
    });
    handle
        .join()
        .unwrap_or_else(|_| Err(UpdateHttpError::Other("update download thread panicked".to_owned())))
}

async fn update_http_get_bytes_async(
    url: &str,
    timeout: std::time::Duration,
) -> Result<Vec<u8>, UpdateHttpError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(UPDATE_USER_AGENT)
        .build()
        .map_err(|error| UpdateHttpError::Other(format!("http client failed: {error}")))?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json, application/octet-stream")
        .send()
        .await
        .map_err(|error| UpdateHttpError::Other(format!("GET {url} failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(update_http_status_error(url, status));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| UpdateHttpError::Other(format!("GET {url} body failed: {error}")))
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

    // #812: only the gnu asset has somewhere to degrade to. A missing musl or
    // macOS asset is a packaging failure and must stay fatal.
    #[test]
    fn update_asset_fallback_only_downgrades_gnu_to_musl() {
        assert_eq!(
            update_asset_fallback_after_not_found("maw-rs-linux-x86_64-gnu"),
            Some("maw-rs-linux-x86_64-musl")
        );
        assert_eq!(update_asset_fallback_after_not_found("maw-rs-linux-x86_64-musl"), None);
        assert_eq!(update_asset_fallback_after_not_found("maw-rs-macos-arm64"), None);
    }

    // The fallback keys off this classification, so widening it to a 4xx range
    // would turn a rate-limit or an auth failure into a silent downgrade to
    // the musl build — which cannot resolve .local names, the very bug the gnu
    // artifact exists to fix. Only 404 may ever mean "absent".
    #[test]
    fn update_http_only_404_counts_as_a_missing_asset() {
        let url = "https://example.invalid/asset";
        assert!(update_http_status_error(url, reqwest::StatusCode::NOT_FOUND).is_not_found());
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::BAD_GATEWAY,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            reqwest::StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(
                !update_http_status_error(url, status).is_not_found(),
                "HTTP {status} must not be read as a missing asset"
            );
        }
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
    fn update_help_documents_the_two_linux_builds() {
        let out = update_run_command(&update_args(&["--help"]));
        assert!(out.stdout.contains("MAW_LIBC"), "stdout={}", out.stdout);
        assert!(out.stdout.contains("CANNOT resolve .local"), "stdout={}", out.stdout);
    }

    /// The libc choice must be explained to the operator, not just made (#812).
    #[test]
    fn update_libc_labels_and_notes_name_the_dot_local_tradeoff() {
        assert_eq!(update_libc_label(UpdateLibc::Gnu), "glibc");
        assert_eq!(update_libc_label(UpdateLibc::Musl), "musl");
        assert!(update_libc_note(UpdateLibc::Gnu).contains("resolves .local"));
        assert!(update_libc_note(UpdateLibc::Musl).contains("cannot resolve .local"));
        assert!(update_libc_note(UpdateLibc::Musl).contains("MAW_LIBC=gnu"));
    }

    #[test]
    fn update_proof_failure_points_at_the_musl_escape_hatch_for_gnu_only() {
        let annotated =
            update_annotate_proof_failure("new binary exited nonzero".to_owned(), "maw-rs-linux-x86_64-gnu");
        assert!(annotated.starts_with("new binary exited nonzero"), "annotated={annotated}");
        assert!(annotated.contains("MAW_LIBC=musl"), "annotated={annotated}");
        for asset in ["maw-rs-linux-x86_64-musl", "maw-rs-macos-arm64"] {
            assert_eq!(update_annotate_proof_failure("boom".to_owned(), asset), "boom");
        }
    }

    /// On this Linux host (glibc, `ldd --version` answers) detection must land
    /// on gnu; anywhere else it must still return one of the two shipped libcs.
    #[test]
    fn update_detect_host_libc_returns_a_shipped_libc() {
        let detected = update_detect_host_libc();
        assert!(matches!(detected, UpdateLibc::Gnu | UpdateLibc::Musl));
        let asset = update_asset_for_platform("linux", "x86_64", detected).expect("linux asset");
        assert!(asset.starts_with("maw-rs-linux-x86_64-"), "asset={asset}");
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

    /// Bare sha256 hex of `bytes`, via the same hasher production uses.
    fn update_hex_of(root: &std::path::Path, label: &str, bytes: &[u8]) -> String {
        let scratch = root.join(format!("hash-scratch-{label}"));
        std::fs::write(&scratch, bytes).expect("scratch");
        let digest = hash_file(&scratch).expect("hash");
        let _ = std::fs::remove_file(&scratch);
        digest.strip_prefix("sha256:").expect("prefix").to_owned()
    }

    #[cfg(unix)]
    fn update_write_script(path: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).expect("script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
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

        let new_hex = update_hex_of(&root, "new", b"new binary");
        let backup = update_safe_replace(&target, b"new binary", &new_hex)
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
        assert!(backup.exists(), "backup is kept for the version proof, not deleted here");

        // no pre-existing target: replace still installs, no backup
        let fresh = root.join("fresh");
        let fresh_hex = update_hex_of(&root, "fresh", b"fresh binary");
        assert!(update_safe_replace(&fresh, b"fresh binary", &fresh_hex).expect("replace").is_none());
        assert_eq!(std::fs::read(&fresh).expect("read"), b"fresh binary");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn update_safe_replace_refuses_sha_mismatch_and_removes_staged_dotfile() {
        let root = update_temp_root("replace-mismatch");
        let target = root.join("maw");
        std::fs::write(&target, b"old binary").expect("old");

        let wrong_hex = "a".repeat(64);
        let error = update_safe_replace(&target, b"tampered bytes", &wrong_hex).expect_err("mismatch");
        assert!(error.contains("sha256 mismatch"), "error={error}");
        assert!(error.contains("refusing to install"), "error={error}");
        assert_eq!(std::fs::read(&target).expect("read"), b"old binary", "target untouched");

        let leftovers: Vec<String> = std::fs::read_dir(&root)
            .expect("read dir")
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|entry_name| entry_name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "staged dotfile leaked: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn update_prove_and_finalize_deletes_backup_only_after_proof_passes() {
        let root = update_temp_root("proof-ok");
        let target = root.join("maw");
        update_write_script(&target, "#!/bin/sh\necho proof-marker-26\nexit 0\n");
        let backup = root.join("maw.bak-update-test");
        std::fs::write(&backup, b"old binary").expect("backup");

        let proof = update_prove_and_finalize(&target, Some(&backup)).expect("proof");
        assert!(proof.contains("proof-marker-26"), "proof={proof}");
        assert!(!backup.exists(), "backup deleted once the new binary proved itself");
        assert!(target.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn update_prove_and_finalize_restores_backup_when_proof_fails() {
        let root = update_temp_root("proof-fail");
        let target = root.join("maw");
        update_write_script(&target, "#!/bin/sh\nexit 1\n");
        let backup = root.join("maw.bak-update-test");
        std::fs::write(&backup, b"old binary").expect("backup");

        let error = update_prove_and_finalize(&target, Some(&backup)).expect_err("proof fails");
        assert!(error.contains("exited nonzero"), "error={error}");
        assert!(error.contains("restored the previous binary"), "error={error}");
        assert!(!backup.exists(), "backup renamed back over the target");
        assert_eq!(std::fs::read(&target).expect("target"), b"old binary", "old binary restored");

        // no backup existed (fresh install): the error says so honestly
        let fresh = root.join("fresh");
        update_write_script(&fresh, "#!/bin/sh\nexit 1\n");
        let error = update_prove_and_finalize(&fresh, None).expect_err("proof fails");
        assert!(error.contains("no previous binary existed to restore"), "error={error}");
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
