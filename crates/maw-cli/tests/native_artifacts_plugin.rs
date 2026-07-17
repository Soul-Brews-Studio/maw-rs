#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn artifacts_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn artifacts_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn artifacts_seed(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-artifacts-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let cache = root.join("cache");
    let alpha = cache.join("artifacts/alpha/101");
    artifacts_write(
        &alpha.join("meta.json"),
        r#"{"team":"alpha","taskId":"101","subject":"Ship native artifacts","owner":"nova","status":"completed","createdAt":"2026-06-25T00:00:00Z","updatedAt":"2026-06-25T01:00:00Z","commitHash":"abc123"}"#,
    );
    artifacts_write(&alpha.join("spec.md"), "# Spec\n\nDo it.\n");
    artifacts_write(&alpha.join("result.md"), "Done.\n");
    artifacts_write(&alpha.join("attachments/log.txt"), "log\n");
    let beta = cache.join("artifacts/beta/202");
    artifacts_write(
        &beta.join("meta.json"),
        r#"{"team":"beta","taskId":"202","subject":"Pending beta task","status":"in_progress","createdAt":"2026-06-24T00:00:00Z","updatedAt":"2026-06-24T01:00:00Z"}"#,
    );
    artifacts_write(&beta.join("spec.md"), "# Beta\n");
    (root, home, config, cache)
}

fn artifacts_command(root: &Path, home: &Path, config: &Path, cache: &Path) -> Command {
    let mut command = Command::new(artifacts_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_CACHE_DIR", cache)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    command
}

#[test]
fn artifacts_native_list_json_golden_is_hermetic() {
    let (root, home, config, cache) = artifacts_seed("list");
    let output = artifacts_command(&root, &home, &config, &cache)
        .args(["artifacts", "--json"])
        .output()
        .expect("run artifacts list");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-artifacts/list.json")
    );
    assert_eq!(dispatcher_status("artifacts"), DispatchKind::Native);
    assert_eq!(dispatcher_status("artifact"), DispatchKind::Native);
    assert!(
        root.join("config").read_dir().is_err(),
        "artifacts must not write config"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn artifact_native_get_alias_and_guards_are_hermetic() {
    let (root, home, config, cache) = artifacts_seed("get");
    let output = artifacts_command(&root, &home, &config, &cache)
        .args(["artifact", "get", "alpha", "101", "--json"])
        .output()
        .expect("run artifact get");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["meta"]["taskId"], "101");
    assert_eq!(value["result"], "Done.\n");
    assert_eq!(value["attachments"], serde_json::json!(["log.txt"]));
    assert_eq!(
        value["dir"],
        cache.join("artifacts/alpha/101").display().to_string()
    );

    let missing = artifacts_command(&root, &home, &config, &cache)
        .args(["artifacts", "get", "alpha", "missing"])
        .output()
        .expect("missing");
    assert!(!missing.status.success());
    assert_eq!(
        String::from_utf8(missing.stderr).expect("stderr"),
        "artifact not found: alpha/missing\n"
    );

    let guarded = artifacts_command(&root, &home, &config, &cache)
        .args(["artifacts", "get", "-team", "101"])
        .output()
        .expect("guard");
    assert!(!guarded.status.success());
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr"),
        "artifacts: unknown argument -team\n"
    );
    let _ = std::fs::remove_dir_all(root);
}
