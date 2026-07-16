//! #522 defense 3 — local-install pin parity + plugins.lock writes.
//!
//! `maw plugin install <dir>` must verify a `target=wasm` package exactly like
//! the git route (committed artifact hashes to `artifact.sha256`, satisfies
//! `--sha256`/plugins.lock) and successful installs must WRITE the resolved
//! pin into plugins.lock (created if absent, existing entries preserved).

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn maw_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-local-pin-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_wasm_plugin(dir: &Path, name: &str) -> String {
    fs::create_dir_all(dir).expect("plugin dir");
    fs::write(
        dir.join("plugin.wasm"),
        b"\0asm\x01\x00\x00\x00local-fixture",
    )
    .expect("wasm artifact");
    let sha256 = maw_plugin_manifest::hash_file(&dir.join("plugin.wasm")).expect("hash wasm");
    write_wasm_manifest(dir, name, &sha256, "1.0.0");
    sha256
}

fn write_wasm_manifest(dir: &Path, name: &str, sha256: &str, version: &str) {
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "{version}",
  "target": "wasm",
  "sdk": "*",
  "entry": {{ "kind": "wasm", "path": "plugin.wasm", "export": "handle" }},
  "wasm": "./plugin.wasm",
  "artifact": {{ "path": "./plugin.wasm", "sha256": "{sha256}" }},
  "cli": {{ "command": "{name}" }}
}}
"#
        ),
    )
    .expect("wasm manifest");
}

fn install_local(root: &Path, source: &Path, extra: &[&str]) -> Output {
    let install_root = root.join("plugins");
    let mut command = Command::new(maw_bin());
    command
        .args(["plugin", "install", source.to_str().expect("source utf8")])
        .args(["--root", install_root.to_str().expect("root utf8")])
        .args(extra)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_PLUGINS_LOCK", root.join("plugins.lock"))
        .env_remove("MAW_PLUGINS_DIR")
        .env_remove("MAW_PLUGIN_DEV");
    command.output().expect("maw plugin install")
}

fn read_lock(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join("plugins.lock")).expect("lock file");
    serde_json::from_str(&text).expect("lock json")
}

#[test]
fn local_wasm_install_verifies_pin_and_writes_plugins_lock() {
    let root = temp_dir("verify-write");
    let source = root.join("src-pkg");
    let sha256 = write_wasm_plugin(&source, "local-pin-demo");

    let output = install_local(&root, &source, &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("plugins/local-pin-demo/plugin.json").is_file());

    let lock = read_lock(&root);
    assert_eq!(lock["schema"], 1);
    let entry = &lock["plugins"]["local-pin-demo"];
    assert_eq!(entry["version"], "1.0.0");
    assert_eq!(entry["sha256"], sha256);
    let recorded_source = entry["source"].as_str().expect("source");
    assert!(
        recorded_source.starts_with("path:"),
        "source: {recorded_source}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_wasm_install_refuses_tampered_artifact() {
    let root = temp_dir("tamper");
    let source = root.join("src-pkg");
    write_wasm_plugin(&source, "tamper-demo");
    // Tamper the committed artifact after pinning.
    fs::write(source.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00TAMPERED").expect("tamper");

    let output = install_local(&root, &source, &[]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("artifact sha256 mismatch — refusing to install"),
        "stderr: {stderr}"
    );
    assert!(!root.join("plugins/tamper-demo").exists());
    assert!(
        !root.join("plugins.lock").exists(),
        "refused install must not write the lock"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_install_honors_explicit_sha256_flag() {
    let root = temp_dir("explicit-sha");
    let source = root.join("src-pkg");
    let sha256 = write_wasm_plugin(&source, "explicit-demo");

    let ok = install_local(&root, &source, &["--sha256", &sha256]);
    assert_eq!(
        ok.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let wrong = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    let refused = install_local(&root, &source, &["--sha256", wrong, "--force"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("sha256 mismatch"),
        "stderr: {}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn lock_pin_gates_reinstall_and_force_updates_the_pin() {
    let root = temp_dir("lock-gate");
    let source = root.join("src-pkg");
    write_wasm_plugin(&source, "gate-demo");
    assert_eq!(install_local(&root, &source, &[]).status.code(), Some(0));

    // New version of the same plugin: lock pin mismatch refuses without --force.
    fs::write(source.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00v2-bytes").expect("v2 wasm");
    let v2_sha = maw_plugin_manifest::hash_file(&source.join("plugin.wasm")).expect("hash");
    write_wasm_manifest(&source, "gate-demo", &v2_sha, "2.0.0");

    let refused = install_local(&root, &source, &[]);
    assert_eq!(refused.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("plugins.lock"), "stderr: {stderr}");
    assert!(stderr.contains("--force"), "stderr: {stderr}");

    let forced = install_local(&root, &source, &["--force"]);
    assert_eq!(
        forced.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(
        String::from_utf8_lossy(&forced.stderr).contains("pin replaced"),
        "stderr: {}",
        String::from_utf8_lossy(&forced.stderr)
    );
    let lock = read_lock(&root);
    assert_eq!(lock["plugins"]["gate-demo"]["version"], "2.0.0");
    assert_eq!(lock["plugins"]["gate-demo"]["sha256"], v2_sha);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unpinned_local_dev_dir_still_installs_without_lock_entry() {
    let root = temp_dir("dev-dir");
    let source = root.join("src-pkg");
    fs::create_dir_all(&source).expect("dir");
    fs::write(
        source.join("plugin.json"),
        r#"{"name":"dev-demo","version":"0.1.0","sdk":"*","target":"js","entry":"index.ts","cli":{"command":"dev-demo"}}"#,
    )
    .expect("manifest");
    fs::write(
        source.join("index.ts"),
        "export default async function main() {}\n",
    )
    .expect("entry");

    let output = install_local(&root, &source, &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("plugins/dev-demo/plugin.json").is_file());
    assert!(
        !root.join("plugins.lock").exists(),
        "unpinned dev install must not fabricate a lock pin"
    );

    let _ = fs::remove_dir_all(root);
}
