use std::collections::BTreeSet;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    discover_packages, discover_packages_with_profile, hash_file, reset_discover_cache, satisfies,
    scan_dirs, DiscoverPackagesOptions, PluginNameAndTier, PluginTier,
};
use serde_json::{json, Map, Value};

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-registry-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn semver_satisfies_matches_maw_js_registry_helper_shapes() {
    assert!(satisfies("1.2.3", "*"));
    assert!(satisfies("1.2.3-alpha.1+build", "1.2.3"));
    assert!(satisfies("1.9.0", "^1.2.3"));
    assert!(!satisfies("2.0.0", "^1.2.3"));
    assert!(satisfies("0.2.9", "^0.2.3"));
    assert!(!satisfies("0.3.0", "^0.2.3"));
    assert!(satisfies("1.2.9", "~1.2.3"));
    assert!(!satisfies("1.3.0", "~1.2.3"));
    assert!(satisfies("1.2.4", ">1.2.3"));
    assert!(satisfies("1.2.3", ">=1.2.3"));
    assert!(satisfies("1.2.3", "<=1.2.3"));
    assert!(!satisfies("1.2.3", "<1.2.3"));
    assert!(!satisfies("not-semver", "*"));
}

#[test]
fn scan_dirs_prefers_explicit_plugins_dir_then_maw_home_then_home() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let original_plugins = std::env::var_os("MAW_PLUGINS_DIR");
    let original_maw_home = std::env::var_os("MAW_HOME");
    let original_home = std::env::var_os("HOME");

    std::env::set_var("MAW_PLUGINS_DIR", "/tmp/maw-explicit-plugins");
    std::env::remove_var("MAW_HOME");
    std::env::set_var("HOME", "/tmp/maw-home-ignored");
    assert_eq!(
        scan_dirs(),
        vec![PathBuf::from("/tmp/maw-explicit-plugins")]
    );

    std::env::remove_var("MAW_PLUGINS_DIR");
    std::env::set_var("MAW_HOME", "/tmp/maw-home");
    assert_eq!(scan_dirs(), vec![PathBuf::from("/tmp/maw-home/plugins")]);

    std::env::remove_var("MAW_HOME");
    std::env::set_var("HOME", "/tmp/real-home");
    assert_eq!(
        scan_dirs(),
        vec![PathBuf::from("/tmp/real-home/.maw/plugins")]
    );

    restore_env("MAW_PLUGINS_DIR", original_plugins);
    restore_env("MAW_HOME", original_maw_home);
    restore_env("HOME", original_home);
}

#[test]
fn semver_satisfies_rejects_bad_ranges_and_zero_major_caret_edges() {
    assert!(!satisfies("1.2.3", "not-a-range"));
    assert!(!satisfies("1.2.3", "1.2.3.4"));
    assert!(!satisfies("0.0.4", "^0.0.3"));
    assert!(satisfies("0.0.3", "^0.0.3"));
}

#[test]
fn discover_packages_returns_empty_for_missing_roots() {
    let root = make_temp_dir("missing");
    let report = discover_packages(&DiscoverPackagesOptions {
        scan_dirs: vec![root.join("missing-root")],
        runtime_version: "1.0.0".to_owned(),
        ..DiscoverPackagesOptions::default()
    });
    assert!(report.plugins.is_empty());
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn discover_packages_memoizes_until_reset() {
    let root = make_temp_dir("cache");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_entry_plugin(&plugins_dir, "registry-cache-a", Map::new());

    reset_discover_cache();
    let options = DiscoverPackagesOptions {
        scan_dirs: vec![plugins_dir.clone()],
        runtime_version: "1.0.0".to_owned(),
        use_cache: true,
        ..DiscoverPackagesOptions::default()
    };
    let first = discover_packages(&options);
    write_entry_plugin(&plugins_dir, "registry-cache-b", Map::new());
    let cached_after_mutation = discover_packages(&options);

    assert_eq!(names(&first), vec!["registry-cache-a"]);
    assert_eq!(names(&cached_after_mutation), vec!["registry-cache-a"]);

    reset_discover_cache();
    let fresh = discover_packages(&options);
    assert_eq!(
        sorted_names(&fresh),
        vec!["registry-cache-a", "registry-cache-b"]
    );

    remove_dir_all(root).expect("cleanup");
    reset_discover_cache();
}

#[test]
#[allow(clippy::too_many_lines)]
fn discover_packages_applies_registry_gates_overrides_and_sorting() {
    let root = make_temp_dir("gates");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");

    create_dir_all(plugins_dir.join("invalid-json")).expect("invalid dir");
    write(plugins_dir.join("invalid-json/plugin.json"), b"not json").expect("invalid json");
    create_dir_all(plugins_dir.join("no-plugin-json")).expect("non-plugin dir");

    write_entry_plugin(
        &plugins_dir,
        "registry-bad-sdk",
        map([("sdk", json!(">=999.0.0"))]),
    );
    write_artifact_plugin(
        &plugins_dir,
        "registry-unbuilt",
        "export default 'unbuilt';\n",
        None,
        Map::new(),
    );
    write_plugin(
        &plugins_dir,
        "registry-missing-artifact",
        map([(
            "artifact",
            json!({ "path": "dist/missing.js", "sha256": "sha256:expected" }),
        )]),
        &[],
    );
    write_artifact_plugin(
        &plugins_dir,
        "registry-hash-mismatch",
        "export default 'actual';\n",
        Some("sha256:expected"),
        Map::new(),
    );

    let artifact_text = "export default 'artifact';\n";
    let disabled_text = "export default 'disabled';\n";
    let artifact_ok = write_artifact_plugin(
        &plugins_dir,
        "registry-artifact-ok",
        artifact_text,
        Some(&sha256(artifact_text)),
        map([("weight", json!(80)), ("tier", json!("standard"))]),
    );
    let disabled_ok = write_artifact_plugin(
        &plugins_dir,
        "registry-disabled-ok",
        disabled_text,
        Some(&sha256(disabled_text)),
        map([("weight", json!(70))]),
    );
    write_entry_plugin(
        &plugins_dir,
        "registry-legacy-ok",
        map([("weight", json!(50))]),
    );

    let dev_source_root = root.join("dev-source");
    create_dir_all(&dev_source_root).expect("dev source");
    write_artifact_plugin(
        &dev_source_root,
        "registry-dev-artifact",
        "export default 'dev';\n",
        None,
        map([("weight", json!(60))]),
    );
    symlink_dir(
        &dev_source_root.join("registry-dev-artifact"),
        &plugins_dir.join("registry-dev-artifact"),
    );

    write(
        plugins_dir.join(".overrides.json"),
        br#"{"registry-artifact-ok":5,"registry-disabled-ok":1}"#,
    )
    .expect("overrides");

    let report = discover_packages(&DiscoverPackagesOptions {
        scan_dirs: vec![plugins_dir],
        disabled_plugins: vec!["registry-disabled-ok".to_owned()],
        runtime_version: "1.0.0".to_owned(),
        ..DiscoverPackagesOptions::default()
    });

    // registry-dev-artifact is a symlinked, unbuilt (no artifact.sha256)
    // plugin dir: without MAW_PLUGIN_DEV=1 the symlink no longer bypasses
    // hash verification, so it refuses like a regular unbuilt dir (#522).
    assert_eq!(
        names(&report),
        vec![
            "registry-disabled-ok",
            "registry-artifact-ok",
            "registry-legacy-ok",
        ]
    );
    assert!(
        report
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.name == "registry-disabled-ok")
            .expect("disabled")
            .disabled
    );
    assert_eq!(
        report
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.name == "registry-artifact-ok")
            .expect("artifact")
            .manifest
            .weight,
        Some(5)
    );
    assert_eq!(
        report
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.name == "registry-disabled-ok")
            .expect("disabled")
            .manifest
            .weight,
        Some(1)
    );
    assert!(
        !report
            .plugins
            .iter()
            .any(|plugin| plugin.manifest.name == "registry-dev-artifact"),
        "symlinked unbuilt plugin must refuse without MAW_PLUGIN_DEV=1"
    );
    assert_eq!(
        hash_file(&artifact_ok).expect("hash"),
        sha256(artifact_text)
    );
    assert_eq!(
        hash_file(&disabled_ok).expect("hash"),
        sha256(disabled_text)
    );

    let warning_text = report.warnings.join("\n");
    assert!(warning_text.contains("requires maw SDK"), "{warning_text}");
    assert!(warning_text.contains("plugin 'registry-unbuilt' is unbuilt"));
    assert!(warning_text.contains("plugin 'registry-missing-artifact' artifact missing"));
    assert!(warning_text.contains("plugin 'registry-hash-mismatch' artifact hash mismatch"));
    assert!(
        warning_text.contains("plugin 'registry-dev-artifact' is unbuilt"),
        "{warning_text}"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn discover_packages_applies_profile_filter_after_defaulting_missing_tiers_to_core() {
    let root = make_temp_dir("profile");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    let artifact_text = "export default 'profile artifact';\n";
    write_artifact_plugin(
        &plugins_dir,
        "registry-profile-artifact",
        artifact_text,
        Some(&sha256(artifact_text)),
        map([("tier", json!("standard")), ("weight", json!(1))]),
    );
    write_entry_plugin(
        &plugins_dir,
        "registry-profile-legacy",
        map([("weight", json!(2))]),
    );

    let mut seen_plugins = Vec::new();
    let report = discover_packages_with_profile(
        &DiscoverPackagesOptions {
            scan_dirs: vec![plugins_dir],
            runtime_version: "1.0.0".to_owned(),
            ..DiscoverPackagesOptions::default()
        },
        |plugins| {
            seen_plugins = plugins.to_vec();
            Some(BTreeSet::from(["registry-profile-legacy".to_owned()]))
        },
    );

    assert_eq!(
        seen_plugins,
        vec![
            PluginNameAndTier {
                name: "registry-profile-artifact".to_owned(),
                tier: PluginTier::Standard,
            },
            PluginNameAndTier {
                name: "registry-profile-legacy".to_owned(),
                tier: PluginTier::Core,
            },
        ]
    );
    assert_eq!(names(&report), vec!["registry-profile-legacy"]);

    remove_dir_all(root).expect("cleanup");
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn names(report: &maw_plugin_manifest::DiscoverPackagesReport) -> Vec<&str> {
    report
        .plugins
        .iter()
        .map(|plugin| plugin.manifest.name.as_str())
        .collect()
}

fn sorted_names(report: &maw_plugin_manifest::DiscoverPackagesReport) -> Vec<&str> {
    let mut names = names(report);
    names.sort_unstable();
    names
}

fn write_entry_plugin(root: &Path, name: &str, manifest: Map<String, Value>) -> PathBuf {
    write_plugin(
        root,
        name,
        extend_manifest(manifest, [("entry", json!("index.ts"))]),
        &[(
            "index.ts",
            format!(
                "export default async function {}() {{}}\n",
                name.replace('-', "_")
            ),
        )],
    )
}

fn write_artifact_plugin(
    root: &Path,
    name: &str,
    artifact_text: &str,
    sha: Option<&str>,
    manifest: Map<String, Value>,
) -> PathBuf {
    let dir = write_plugin(
        root,
        name,
        extend_manifest(
            manifest,
            [(
                "artifact",
                json!({ "path": "dist/index.js", "sha256": sha }),
            )],
        ),
        &[("dist/index.js", artifact_text.to_owned())],
    );
    dir.join("dist/index.js")
}

fn write_plugin(
    root: &Path,
    name: &str,
    manifest: Map<String, Value>,
    files: &[(&str, String)],
) -> PathBuf {
    let dir = root.join(name);
    create_dir_all(&dir).expect("plugin dir");
    for (relative_path, contents) in files {
        let path = dir.join(relative_path);
        create_dir_all(path.parent().expect("file parent")).expect("file parent");
        write(path, contents).expect("plugin file");
    }

    let mut full_manifest = map([
        ("name", json!(name)),
        ("version", json!("1.0.0")),
        ("sdk", json!("*")),
        ("target", json!("js")),
    ]);
    full_manifest.extend(manifest);
    write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(&Value::Object(full_manifest)).expect("json"),
    )
    .expect("manifest");
    dir
}

fn extend_manifest(
    mut manifest: Map<String, Value>,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) -> Map<String, Value> {
    manifest.extend(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value)),
    );
    manifest
}

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn sha256(text: &str) -> String {
    let dir = make_temp_dir("hash");
    let path = dir.join("artifact.js");
    write(&path, text).expect("hash input");
    let digest = hash_file(&path).expect("hash");
    remove_dir_all(dir).expect("hash cleanup");
    digest
}

#[cfg(unix)]
fn symlink_dir(source: &Path, link: &Path) {
    std::os::unix::fs::symlink(source, link).expect("symlink");
}

#[cfg(windows)]
fn symlink_dir(source: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(source, link).expect("symlink");
}
