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

/// Release asset name for the current platform; only two prebuilt targets exist.
fn update_asset_for_platform(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("maw-rs-macos-arm64"),
        ("linux", "x86_64") => Some("maw-rs-linux-x86_64-musl"),
        _ => None,
    }
}

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
        assert_eq!(update_asset_for_platform("macos", "aarch64"), Some("maw-rs-macos-arm64"));
        assert_eq!(update_asset_for_platform("linux", "x86_64"), Some("maw-rs-linux-x86_64-musl"));
        assert_eq!(update_asset_for_platform("windows", "x86_64"), None);
        assert_eq!(update_asset_for_platform("linux", "aarch64"), None);
        assert_eq!(
            update_download_url("v26.7.16", "maw-rs-macos-arm64"),
            "https://github.com/Soul-Brews-Studio/maw-rs/releases/download/v26.7.16/maw-rs-macos-arm64"
        );
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
