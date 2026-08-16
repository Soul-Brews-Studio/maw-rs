// Pure planning layer for `maw update` — no network, no filesystem writes.
// Channel/tag/version logic lives here; effects stay in update.rs.

const UPDATE_REPO: &str = "Soul-Brews-Studio/maw-rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateChannel {
    Stable,
    Alpha,
}

impl UpdateChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Alpha => "alpha",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateTagInfo {
    tag: String,
    base: [i64; 3],
    channel: UpdateChannel,
    alpha_n: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateLocalVersion {
    base: [i64; 3],
    channel: UpdateChannel,
    alpha_n: i64,
    dev: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateCompare {
    RemoteNewer,
    RemoteOlder,
    Equal,
    LocalDev,
}

/// Which Linux C library the *host* runs — decides which Linux asset to fetch.
///
/// Two Linux binaries ship per release and they are NOT interchangeable:
/// the musl build is static and runs anywhere, but musl has no glibc NSS, so
/// it cannot load `mdns4_minimal` from `/etc/nsswitch.conf` and therefore
/// cannot resolve `*.local` names at all (#812). The gnu build needs a glibc
/// host but resolves `.local` through the system resolver. Prefer gnu when the
/// host is provably glibc; fall back to musl whenever the evidence is unclear,
/// because a static binary that runs beats a dynamic one that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateLibc {
    Gnu,
    Musl,
}

/// Channel inference from the running build version: hyphenated = alpha, else stable.
fn update_infer_channel(version: &str) -> UpdateChannel {
    if version.contains('-') {
        UpdateChannel::Alpha
    } else {
        UpdateChannel::Stable
    }
}

fn update_channel_from_name(name: &str) -> Result<UpdateChannel, String> {
    match name {
        "stable" => Ok(UpdateChannel::Stable),
        "alpha" => Ok(UpdateChannel::Alpha),
        other => Err(format!(
            "unknown channel \"{other}\" — expected stable or alpha"
        )),
    }
}

fn update_parse_base_number(text: &str) -> Option<i64> {
    if text.is_empty() || text.len() > 6 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn update_parse_base_triplet(text: &str) -> Option<[i64; 3]> {
    let mut parts = text.split('.');
    let year = update_parse_base_number(parts.next()?)?;
    let month = update_parse_base_number(parts.next()?)?;
    let day = update_parse_base_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some([year, month, day])
}

/// Classify a release tag into a channel by tag shape alone.
///
/// Never trusts the GitHub `prerelease` flag or `/releases/latest` (#536):
/// `vYY.M.D` = stable, `vYY.M.D-alpha.N` = alpha, anything else = out of scope.
fn update_classify_tag(tag: &str) -> Option<UpdateTagInfo> {
    let rest = tag.strip_prefix('v')?;
    let (base_text, suffix) = rest
        .split_once('-')
        .map_or((rest, None), |(base, suffix)| (base, Some(suffix)));
    let base = update_parse_base_triplet(base_text)?;
    let (channel, alpha_n) = match suffix {
        None => (UpdateChannel::Stable, 0),
        Some(suffix) => {
            let digits = suffix.strip_prefix("alpha.")?;
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            (UpdateChannel::Alpha, digits.parse().ok()?)
        }
    };
    Some(UpdateTagInfo {
        tag: tag.to_owned(),
        base,
        channel,
        alpha_n,
    })
}

/// Parse the running build version, flagging dev builds (git-describe
/// `-N-gSHA` suffixes, `-dirty`, bare short SHAs) as incomparable.
fn update_parse_local_version(version: &str) -> UpdateLocalVersion {
    let dev = UpdateLocalVersion {
        base: [0, 0, 0],
        channel: UpdateChannel::Stable,
        alpha_n: 0,
        dev: true,
    };
    let rest = version.strip_prefix('v').unwrap_or(version);
    let (base_text, suffix) = rest
        .split_once('-')
        .map_or((rest, None), |(base, suffix)| (base, Some(suffix)));
    let Some(base) = update_parse_base_triplet(base_text) else {
        return dev;
    };
    match suffix {
        None => UpdateLocalVersion {
            base,
            channel: UpdateChannel::Stable,
            alpha_n: 0,
            dev: false,
        },
        Some(suffix) => {
            let Some(after_alpha) = suffix.strip_prefix("alpha.") else {
                return dev;
            };
            if after_alpha.is_empty() {
                return dev;
            }
            if after_alpha.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Ok(alpha_n) = after_alpha.parse() {
                    return UpdateLocalVersion {
                        base,
                        channel: UpdateChannel::Alpha,
                        alpha_n,
                        dev: false,
                    };
                }
            }
            dev
        }
    }
}

fn update_order_key(base: [i64; 3], channel: UpdateChannel, alpha_n: i64) -> ([i64; 3], i64, i64) {
    match channel {
        UpdateChannel::Stable => (base, 1, 0),
        UpdateChannel::Alpha => (base, 0, alpha_n),
    }
}

/// Compare a remote release tag against the running build version.
///
/// `CalVer` ordering: calendar base first, then stable outranks alpha of the
/// same day, then the alpha `HMM` suffix. Dev builds are incomparable.
fn update_compare_to_local(local_version: &str, remote: &UpdateTagInfo) -> UpdateCompare {
    let local = update_parse_local_version(local_version);
    if local.dev {
        return UpdateCompare::LocalDev;
    }
    let local_key = update_order_key(local.base, local.channel, local.alpha_n);
    let remote_key = update_order_key(remote.base, remote.channel, remote.alpha_n);
    match remote_key.cmp(&local_key) {
        std::cmp::Ordering::Greater => UpdateCompare::RemoteNewer,
        std::cmp::Ordering::Less => UpdateCompare::RemoteOlder,
        std::cmp::Ordering::Equal => UpdateCompare::Equal,
    }
}

/// Newest release tag within a channel — max by (calendar base, alpha suffix).
fn update_pick_latest_tag(tags: &[String], channel: UpdateChannel) -> Option<UpdateTagInfo> {
    tags.iter()
        .filter_map(|tag| update_classify_tag(tag))
        .filter(|info| info.channel == channel)
        .max_by_key(|info| (info.base, info.alpha_n))
}

/// Release asset name for a platform. Linux ships two `x86_64` builds, so the
/// host libc (see [`UpdateLibc`]) picks between them; other platforms ignore it.
fn update_asset_for_platform(os: &str, arch: &str, libc: UpdateLibc) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("maw-rs-macos-arm64"),
        ("linux", "x86_64") => Some(match libc {
            UpdateLibc::Gnu => "maw-rs-linux-x86_64-gnu",
            UpdateLibc::Musl => "maw-rs-linux-x86_64-musl",
        }),
        _ => None,
    }
}

/// An explicit `MAW_LIBC` override, if it names a libc we ship.
///
/// Same variable install.sh honors, so both installers can be forced the same
/// way. An unset or unrecognized value means "decide from evidence".
fn update_libc_from_override(value: Option<&str>) -> Option<UpdateLibc> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "gnu" | "glibc" => Some(UpdateLibc::Gnu),
        "musl" => Some(UpdateLibc::Musl),
        _ => None,
    }
}

/// Decide the host libc from gathered evidence. Pure: the caller runs `ldd`
/// and stats the loader path, this only judges what came back.
///
/// Mirrors `install.sh`'s `detect_linux_libc()` rule for rule — the installer
/// and `maw update` must never disagree about which asset a host gets.
///
///  1. `ldd --version` naming musl wins outright (musl's own ldd says "musl").
///  2. otherwise `ldd --version` naming GNU/GLIBC means glibc.
///  3. otherwise a glibc dynamic loader on disk means glibc.
///  4. otherwise musl — the safe default when nothing is provable.
fn update_classify_libc(ldd_version_output: Option<&str>, glibc_loader_present: bool) -> UpdateLibc {
    if let Some(text) = ldd_version_output {
        let lower = text.to_ascii_lowercase();
        if lower.contains("musl") {
            return UpdateLibc::Musl;
        }
        if lower.contains("gnu libc") || lower.contains("glibc") || lower.contains("gnu c library") {
            return UpdateLibc::Gnu;
        }
    }
    if glibc_loader_present {
        UpdateLibc::Gnu
    } else {
        UpdateLibc::Musl
    }
}

/// Glibc loader/libc paths probed when `ldd --version` gives no answer.
/// Kept beside the classifier so install.sh and `maw update` probe the same set.
const UPDATE_GLIBC_LOADER_PATHS: &[&str] = &[
    "/lib/x86_64-linux-gnu/libc.so.6",
    "/usr/lib/x86_64-linux-gnu/libc.so.6",
    "/lib64/ld-linux-x86-64.so.2",
    "/lib/ld-linux-x86-64.so.2",
    "/usr/lib64/libc.so.6",
];

fn update_download_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{UPDATE_REPO}/releases/download/{tag}/{asset}")
}

/// First whitespace-separated field of line 1, as install.sh reads sidecars.
fn update_parse_sha256_sidecar(text: &str) -> Option<String> {
    let first = text.lines().next()?.split_whitespace().next()?;
    let lower = first.to_ascii_lowercase();
    if lower.len() == 64 && lower.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(lower)
    } else {
        None
    }
}

/// Refuse to self-update a cargo dev build (`target*/debug|release` paths).
fn update_target_dir_guard(path: &std::path::Path) -> Result<(), String> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect();
    for (index, component) in components.iter().enumerate() {
        let next_is_profile = matches!(components.get(index + 1), Some(&"debug" | &"release"));
        if *component == "target" || (component.starts_with("target") && next_is_profile) {
            return Err(format!(
                "refusing to self-update: {} looks like a cargo dev build inside a target/ directory — update via git + cargo instead, or point MAW_RS_SELF_BIN at the installed binary",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod update_plan_tests {
    use super::*;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn update_channel_inference_from_build_versions() {
        assert_eq!(update_infer_channel("26.7.16"), UpdateChannel::Stable);
        assert_eq!(update_infer_channel("26.7.16-alpha.1159"), UpdateChannel::Alpha);
        // git-describe dev forms are hyphenated, so they follow the alpha channel
        assert_eq!(update_infer_channel("26.7.15-alpha.1755-20-g072a086f"), UpdateChannel::Alpha);
        assert_eq!(update_infer_channel("26.7.16-dirty"), UpdateChannel::Alpha);
        // bare short-SHA fallback has no hyphen: stable per the inference rule
        assert_eq!(update_infer_channel("072a086f"), UpdateChannel::Stable);
    }

    #[test]
    fn update_tag_classification_by_shape_not_prerelease_flag() {
        let stable = update_classify_tag("v26.7.16").expect("stable");
        assert_eq!(stable.channel, UpdateChannel::Stable);
        assert_eq!(stable.base, [26, 7, 16]);

        let alpha = update_classify_tag("v26.7.16-alpha.1159").expect("alpha");
        assert_eq!(alpha.channel, UpdateChannel::Alpha);
        assert_eq!(alpha.alpha_n, 1159);

        // beta, malformed, and non-release tags are out of scope entirely
        assert_eq!(update_classify_tag("v26.7.16-beta.900"), None);
        assert_eq!(update_classify_tag("26.7.16"), None);
        assert_eq!(update_classify_tag("v26.7"), None);
        assert_eq!(update_classify_tag("v26.7.16-alpha."), None);
        assert_eq!(update_classify_tag("v26.7.16-alpha.11x9"), None);
        assert_eq!(update_classify_tag("install.sh"), None);
    }

    #[test]
    fn update_compare_matrix_older_newer_equal_dev() {
        let newer_alpha = update_classify_tag("v26.7.17-alpha.900").expect("tag");
        let same_alpha = update_classify_tag("v26.7.16-alpha.1159").expect("tag");
        let older_alpha = update_classify_tag("v26.7.15-alpha.2000").expect("tag");
        let same_day_stable = update_classify_tag("v26.7.16").expect("tag");

        let local = "26.7.16-alpha.1159";
        assert_eq!(update_compare_to_local(local, &newer_alpha), UpdateCompare::RemoteNewer);
        assert_eq!(update_compare_to_local(local, &same_alpha), UpdateCompare::Equal);
        assert_eq!(update_compare_to_local(local, &older_alpha), UpdateCompare::RemoteOlder);
        // stable of the same day is the promotion of that day's alphas
        assert_eq!(update_compare_to_local(local, &same_day_stable), UpdateCompare::RemoteNewer);

        // later alpha HMM on the same day is newer
        let later_hmm = update_classify_tag("v26.7.16-alpha.1830").expect("tag");
        assert_eq!(update_compare_to_local(local, &later_hmm), UpdateCompare::RemoteNewer);

        // stable local: same-day alpha is older than the stable cut
        assert_eq!(update_compare_to_local("26.7.16", &same_alpha), UpdateCompare::RemoteOlder);
        assert_eq!(update_compare_to_local("26.7.16", &same_day_stable), UpdateCompare::Equal);

        // dev builds are incomparable regardless of the remote tag
        for dev in ["26.7.15-alpha.1755-20-g072a086f", "26.7.16-dirty", "26.7.16-alpha.1159-dirty", "072a086f", ""] {
            assert_eq!(update_compare_to_local(dev, &newer_alpha), UpdateCompare::LocalDev, "dev form {dev:?}");
        }
    }

    #[test]
    fn update_pick_latest_tag_filters_channel_and_maxes_calver() {
        let all = tags(&[
            "v26.7.16-alpha.1159",
            "v26.7.16",
            "v26.7.17-alpha.905",
            "v26.7.17-alpha.1830",
            "v26.7.15",
            "v26.7.17-beta.900",
            "garbage",
        ]);
        let alpha = update_pick_latest_tag(&all, UpdateChannel::Alpha).expect("alpha");
        assert_eq!(alpha.tag, "v26.7.17-alpha.1830");
        let stable = update_pick_latest_tag(&all, UpdateChannel::Stable).expect("stable");
        assert_eq!(stable.tag, "v26.7.16");
        assert_eq!(update_pick_latest_tag(&tags(&["v26.7.17-beta.900"]), UpdateChannel::Stable), None);
    }

    #[test]
    fn update_asset_selection_per_platform() {
        for libc in [UpdateLibc::Gnu, UpdateLibc::Musl] {
            assert_eq!(update_asset_for_platform("macos", "aarch64", libc), Some("maw-rs-macos-arm64"));
            assert_eq!(update_asset_for_platform("windows", "x86_64", libc), None);
            assert_eq!(update_asset_for_platform("linux", "aarch64", libc), None);
        }
        // the only place libc matters: two Linux x86_64 builds ship per release
        assert_eq!(
            update_asset_for_platform("linux", "x86_64", UpdateLibc::Gnu),
            Some("maw-rs-linux-x86_64-gnu")
        );
        assert_eq!(
            update_asset_for_platform("linux", "x86_64", UpdateLibc::Musl),
            Some("maw-rs-linux-x86_64-musl")
        );
        assert_eq!(
            update_download_url("v26.7.16", "maw-rs-macos-arm64"),
            "https://github.com/Soul-Brews-Studio/maw-rs/releases/download/v26.7.16/maw-rs-macos-arm64"
        );
    }

    #[test]
    fn update_libc_classification_prefers_gnu_only_on_proof() {
        // real `ldd --version` first lines, glibc and musl
        let glibc = "ldd (Ubuntu GLIBC 2.39-0ubuntu8.8) 2.39\nCopyright (C) 2024 Free Software Foundation, Inc.";
        let debian = "ldd (Debian GNU libc 2.36-9) 2.36";
        let musl = "musl libc (x86_64)\nVersion 1.2.5\nDynamic Program Loader";
        assert_eq!(update_classify_libc(Some(glibc), false), UpdateLibc::Gnu);
        assert_eq!(update_classify_libc(Some(debian), false), UpdateLibc::Gnu);
        assert_eq!(update_classify_libc(Some(musl), false), UpdateLibc::Musl);

        // musl wins even when a glibc loader is also on disk (gcompat / mixed host):
        // a musl ldd is proof the host is musl, and musl is the safe answer anyway
        assert_eq!(update_classify_libc(Some(musl), true), UpdateLibc::Musl);

        // no ldd at all: the loader probe decides, and absence means musl
        assert_eq!(update_classify_libc(None, true), UpdateLibc::Gnu);
        assert_eq!(update_classify_libc(None, false), UpdateLibc::Musl);

        // unrecognizable ldd output is ambiguous, never a reason to pick gnu
        assert_eq!(update_classify_libc(Some("ldd: unrecognized option"), false), UpdateLibc::Musl);
        assert_eq!(update_classify_libc(Some(""), false), UpdateLibc::Musl);
        assert_eq!(update_classify_libc(Some("ldd: unrecognized option"), true), UpdateLibc::Gnu);
    }

    #[test]
    fn update_libc_override_accepts_only_shipped_names() {
        assert_eq!(update_libc_from_override(Some("gnu")), Some(UpdateLibc::Gnu));
        assert_eq!(update_libc_from_override(Some("glibc")), Some(UpdateLibc::Gnu));
        assert_eq!(update_libc_from_override(Some(" MUSL \n")), Some(UpdateLibc::Musl));
        assert_eq!(update_libc_from_override(Some("uclibc")), None);
        assert_eq!(update_libc_from_override(Some("")), None);
        assert_eq!(update_libc_from_override(None), None);
    }

    /// How `ldd --version` behaves on the host under test.
    #[derive(Clone, Copy)]
    enum LddStub {
        /// Prints to stdout and exits 0 — glibc's behaviour.
        Stdout(&'static str),
        /// Prints to stderr and exits non-zero — musl's actual behaviour, and
        /// the reason install.sh captures with `2>&1 || true`.
        StderrFail(&'static str),
        /// No `ldd` on PATH at all, as in a minimal musl container.
        Absent,
    }

    /// Resolve a libc the way `maw update` does: an explicit override wins,
    /// otherwise judge the evidence.
    fn updater_libc(override_value: &str, ldd: LddStub) -> &'static str {
        // The updater runs `ldd` itself: text on either stream is evidence,
        // while a binary that will not spawn leaves it with nothing to judge.
        let evidence = match ldd {
            LddStub::Stdout(text) | LddStub::StderrFail(text) => Some(text),
            LddStub::Absent => None,
        };
        let over = (!override_value.is_empty()).then_some(override_value);
        let libc = update_libc_from_override(over)
            .unwrap_or_else(|| update_classify_libc(evidence, false));
        match libc {
            UpdateLibc::Gnu => "gnu",
            UpdateLibc::Musl => "musl",
        }
    }

    /// Resolve a libc the way install.sh does, by sourcing the real script and
    /// calling the real function with `ldd` stubbed out. Loader probing is
    /// pointed at a path that cannot exist so the on-disk branch is pinned to
    /// the `glibc_loader_present: false` the updater side is given.
    fn installer_libc(override_value: &str, ldd: LddStub) -> String {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../install.sh")
            .canonicalize()
            .expect("install.sh must sit at the repo root");
        // `command_not_found_handle`-free way to make `ldd` genuinely absent:
        // shadow it with a function that exits like a missing command.
        let stub = match ldd {
            LddStub::Stdout(_) => "ldd() { printf '%s\\n' \"$FAKE_LDD\"; }",
            LddStub::StderrFail(_) => "ldd() { printf '%s\\n' \"$FAKE_LDD\" >&2; return 1; }",
            LddStub::Absent => "ldd() { printf 'sh: ldd: not found\\n' >&2; return 127; }",
        };
        let text = match ldd {
            LddStub::Stdout(text) | LddStub::StderrFail(text) => text,
            LddStub::Absent => "",
        };
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(". \"$MAW_INSTALL_SH\"\n{stub}\ndetect_linux_libc"))
            .env("MAW_INSTALL_SH", &script)
            .env("MAW_INSTALL_TESTING", "1")
            .env("MAW_LIBC", override_value)
            .env("FAKE_LDD", text)
            .env("MAW_LIBC_LOADER_PATHS", "/nonexistent/maw-parity/libc.so.6")
            .output()
            .expect("sh must run install.sh");
        assert!(
            output.status.success(),
            "install.sh detect_linux_libc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    // #812: install.sh and `maw update` each implement the libc rule, the
    // commit claimed they "mirror rule for rule", and each side's own tests
    // passed while the pair disagreed — the updater trimmed and lowercased,
    // the installer matched exact literals, so MAW_LIBC="MUSL" sent the two
    // installers to different assets. Asserting the rule twice cannot catch
    // that; only driving both from one table can.
    #[test]
    fn install_sh_and_updater_resolve_every_libc_input_identically() {
        let glibc = LddStub::Stdout("ldd (Ubuntu GLIBC 2.39-0ubuntu8) 2.39");
        let cases: &[(&str, LddStub)] = &[
            // no override: the evidence decides
            ("", glibc),
            ("", LddStub::Stdout("musl libc (x86_64)")),
            ("", LddStub::Stdout("Musl libc (x86_64)")),
            ("", LddStub::Stdout("MUSL libc")),
            ("", LddStub::Stdout("GNU LIBC")),
            ("", LddStub::Stdout("gnu c library")),
            ("", LddStub::Stdout("ldd: unrecognized option")),
            ("", LddStub::Stdout("")),
            // override wins over contradicting evidence
            ("gnu", LddStub::Stdout("musl libc")),
            ("glibc", LddStub::Stdout("musl libc")),
            ("musl", glibc),
            // case and surrounding whitespace must not change the answer
            ("MUSL", glibc),
            (" MUSL \n", glibc),
            ("  gnu  ", LddStub::Stdout("musl libc")),
            ("GLIBC", LddStub::Stdout("musl libc")),
            ("\tMusl\t", glibc),
            // a .envrc edited on Windows leaves a trailing CR
            ("musl\r", glibc),
            ("gnu\r\n", LddStub::Stdout("musl libc")),
            // unrecognized overrides fall through to the evidence
            ("uclibc", glibc),
            ("uclibc", LddStub::Stdout("musl libc")),
            // interior whitespace is not a shipped name on either side
            ("mu sl", glibc),
            // musl's real ldd: writes to stderr and exits non-zero
            ("", LddStub::StderrFail("musl libc (x86_64)\nVersion 1.2.5")),
            ("", LddStub::StderrFail("ldd: unrecognized option")),
            ("gnu", LddStub::StderrFail("musl libc (x86_64)")),
            // no ldd on PATH at all, as in a minimal musl container
            ("", LddStub::Absent),
            ("gnu", LddStub::Absent),
            ("musl", LddStub::Absent),
        ];

        for (override_value, ldd) in cases {
            let installer = installer_libc(override_value, *ldd);
            let updater = updater_libc(override_value, *ldd);
            let shown = match ldd {
                LddStub::Stdout(text) => format!("stdout {text:?}"),
                LddStub::StderrFail(text) => format!("stderr+fail {text:?}"),
                LddStub::Absent => "absent".to_owned(),
            };
            assert_eq!(
                installer, updater,
                "libc parity broke for MAW_LIBC={override_value:?} ldd={shown}: install.sh said {installer}, maw update said {updater}"
            );
        }
    }

    #[test]
    fn update_glibc_loader_paths_are_absolute_and_nonempty() {
        assert!(!UPDATE_GLIBC_LOADER_PATHS.is_empty());
        for path in UPDATE_GLIBC_LOADER_PATHS {
            assert!(path.starts_with('/'), "loader probe path must be absolute: {path}");
        }
    }

    #[test]
    fn update_sha256_sidecar_parses_first_field_of_line_one() {
        let hex = "a".repeat(64);
        assert_eq!(update_parse_sha256_sidecar(&format!("{hex}  maw-rs-macos-arm64\n")), Some(hex.clone()));
        assert_eq!(update_parse_sha256_sidecar(&hex.to_ascii_uppercase()), Some(hex));
        assert_eq!(update_parse_sha256_sidecar("not-a-hash maw-rs-macos-arm64\n"), None);
        assert_eq!(update_parse_sha256_sidecar(""), None);
        assert_eq!(update_parse_sha256_sidecar(&"a".repeat(63)), None);
        assert_eq!(update_parse_sha256_sidecar(&format!("{} x", "z".repeat(64))), None);
    }

    #[test]
    fn update_target_dir_guard_refuses_cargo_dev_builds() {
        let refuse = |path: &str| update_target_dir_guard(std::path::Path::new(path)).is_err();
        assert!(refuse("/opt/Code/maw-rs/target/debug/maw"));
        assert!(refuse("/opt/Code/maw-rs/target/release/maw"));
        assert!(refuse("/opt/Code/maw-rs/target-gate/debug/maw"));
        assert!(!refuse("/usr/local/bin/maw"));
        assert!(!refuse("/Users/nat/.local/bin/maw"));
    }
}
