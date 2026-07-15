//! #522 defense 4 — loud missing-plugin dispatch fallthrough.
//!
//! A verb that resolves to no native handler and no installed plugin, but is a
//! KNOWN extracted verb (fleet-plugins table or plugins.lock pin), must print
//! an actionable message instead of the bare unknown-command exit; true typos
//! keep the unknown-command path. Also pins the fleet table to the real
//! `fleet-plugins/*/plugin.json` manifests and asserts every shipped manifest
//! still passes the ABI-derived SDK floor.

use maw_cli::{run_cli, KNOWN_FLEET_PLUGIN_VERBS};
use maw_plugin_manifest::{host_abi_version, satisfies};
use std::ffi::OsString;
use std::fs::{create_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    home: Option<OsString>,
    maw_home: Option<OsString>,
    maw_plugins_dir: Option<OsString>,
    maw_plugins_lock: Option<OsString>,
    maw_plugin_dev: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            maw_home: std::env::var_os("MAW_HOME"),
            maw_plugins_dir: std::env::var_os("MAW_PLUGINS_DIR"),
            maw_plugins_lock: std::env::var_os("MAW_PLUGINS_LOCK"),
            maw_plugin_dev: std::env::var_os("MAW_PLUGIN_DEV"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("HOME", self.home.take());
        restore_env("MAW_HOME", self.maw_home.take());
        restore_env("MAW_PLUGINS_DIR", self.maw_plugins_dir.take());
        restore_env("MAW_PLUGINS_LOCK", self.maw_plugins_lock.take());
        restore_env("MAW_PLUGIN_DEV", self.maw_plugin_dev.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-missing-verb-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Point every plugin surface at hermetic temp locations.
fn seed_hermetic_env(root: &Path) {
    let plugins = root.join("plugins");
    create_dir_all(&plugins).expect("plugins dir");
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("MAW_HOME", root.join("maw-home"));
    std::env::set_var("MAW_PLUGINS_DIR", &plugins);
    std::env::set_var("MAW_PLUGINS_LOCK", root.join("plugins.lock"));
    std::env::remove_var("MAW_PLUGIN_DEV");
}

fn fleet_plugins_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fleet-plugins")
}

#[test]
fn fleet_verb_table_matches_shipped_manifests_and_all_pass_the_sdk_floor() {
    let root = fleet_plugins_root();
    let mut manifest_rows = Vec::new();
    for entry in std::fs::read_dir(&root).expect("fleet-plugins dir").flatten() {
        let manifest_path = entry.path().join("plugin.json");
        if !manifest_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest_path).expect("manifest read");
        let manifest: serde_json::Value = serde_json::from_str(&text).expect("manifest json");
        let dir = entry.file_name().to_string_lossy().into_owned();
        let name = manifest["name"].as_str().expect("name").to_owned();
        let verb = manifest["cli"]["command"]
            .as_str()
            .unwrap_or(&name)
            .to_owned();
        let sdk = manifest["sdk"].as_str().expect("sdk").to_owned();
        // Every shipped fleet manifest must keep loading under the
        // ABI-derived floor (backward-compat guarantee of #522 defense 1).
        assert!(
            satisfies(&host_abi_version(), &sdk),
            "fleet-plugins/{dir} sdk range {sdk} refuses current host ABI {}",
            host_abi_version()
        );
        manifest_rows.push((verb, name, dir));
    }
    manifest_rows.sort();

    let mut table_rows: Vec<(String, String, String)> = KNOWN_FLEET_PLUGIN_VERBS
        .iter()
        .map(|(verb, name, dir)| ((*verb).to_owned(), (*name).to_owned(), (*dir).to_owned()))
        .collect();
    table_rows.sort();

    assert_eq!(
        table_rows, manifest_rows,
        "KNOWN_FLEET_PLUGIN_VERBS is out of sync with fleet-plugins/*/plugin.json"
    );
}

#[test]
fn known_fleet_verb_without_installed_plugin_prints_install_hint_not_unknown_command() {
    let _guard = env_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = EnvRestore::capture();
    let root = temp_dir("fleet-hint");
    seed_hermetic_env(&root);

    // "menubar" has no native handler; its plugin (maw-menubar) is not installed.
    let output = run_cli(&args(&["menubar"]));

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.contains("verb 'menubar' is provided by plugin 'maw-menubar'"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("maw plugin install Soul-Brews-Studio/maw-rs/fleet-plugins/maw-menubar"),
        "stderr: {}",
        output.stderr
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lock_pinned_verb_without_installed_plugin_prints_lock_derived_install_hint() {
    let _guard = env_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = EnvRestore::capture();
    let root = temp_dir("lock-hint");
    seed_hermetic_env(&root);
    let sha = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    write(
        root.join("plugins.lock"),
        format!(
            r#"{{"schema":1,"plugins":{{"foo-fleet":{{"version":"1.0.0","sha256":"{sha}","source":"github:o/r@v1"}}}}}}"#
        ),
    )
    .expect("lock");

    let output = run_cli(&args(&["foo-fleet"]));

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output
            .stderr
            .contains("verb 'foo-fleet' is provided by plugin 'foo-fleet', which is not installed"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains(&format!("maw plugin install o/r@v1 --sha256 {sha}")),
        "stderr: {}",
        output.stderr
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refused_plugin_for_known_verb_surfaces_the_refusal_not_unknown_command() {
    let _guard = env_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = EnvRestore::capture();
    let root = temp_dir("refusal");
    seed_hermetic_env(&root);
    let sha = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    write(
        root.join("plugins.lock"),
        format!(
            r#"{{"schema":1,"plugins":{{"tampered-demo":{{"version":"1.0.0","sha256":"{sha}","source":"github:o/r@v1"}}}}}}"#
        ),
    )
    .expect("lock");
    // Installed plugin dir whose committed artifact does NOT hash to its pin.
    let plugin_dir = root.join("plugins").join("tampered-demo");
    create_dir_all(&plugin_dir).expect("plugin dir");
    write(plugin_dir.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00tampered").expect("wasm");
    write(
        plugin_dir.join("plugin.json"),
        format!(
            r#"{{"name":"tampered-demo","version":"1.0.0","sdk":"*","target":"wasm","entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"./plugin.wasm","sha256":"{sha}"}},"cli":{{"command":"tampered-demo"}}}}"#
        ),
    )
    .expect("manifest");

    let output = run_cli(&args(&["tampered-demo"]));

    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("refused to load"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("artifact hash mismatch"),
        "stderr: {}",
        output.stderr
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn true_typos_keep_the_unknown_command_path() {
    let _guard = env_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = EnvRestore::capture();
    let root = temp_dir("typo");
    seed_hermetic_env(&root);

    let output = run_cli(&args(&["definitely-not-a-verb"]));

    assert_eq!(output.code, 2);
    assert_eq!(
        output.stderr,
        "maw-rs: unknown command 'definitely-not-a-verb'\nsee maw-rs --help\n"
    );

    let _ = std::fs::remove_dir_all(root);
}
