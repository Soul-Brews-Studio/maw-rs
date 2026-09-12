#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-ctq-{label}-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join("config/maw")).expect("config");
    fs::create_dir_all(root.join("state")).expect("state");
    root
}

fn chmod_exec(path: &Path) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).expect("chmod");
}

fn write_fake_marker(bin_dir: &Path, name: &str, marker: &str) {
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{marker} $*'\nexit 0\n")).expect("marker");
    chmod_exec(&path);
}

fn run_ctq(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("MAW_CONFIG_DIR", root.join("config/maw"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run ctq")
}

fn assert_no_delegation(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "stderr={stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "stderr={stderr}");
}

#[test]
fn cross_team_queue_runtime_fake_maw_proof() {
    assert_eq!(dispatcher_status("cross-team-queue"), DispatchKind::Native);
    let root = temp_dir("runtime-proof");
    let bin_dir = root.join("bin");
    write_fake_marker(&bin_dir, "maw", "DELEGATED-MAW");
    write_fake_marker(&bin_dir, "bun", "DELEGATED-BUN");

    let output = run_ctq(&root, &["cross-team-queue"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_no_delegation(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("fixtures/zerobun/cross-team-queue-empty.stdout")
    );

    let json = run_ctq(&root, &["cross-team-queue", "--json"]);
    assert!(
        json.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&json.stderr)
    );
    assert_no_delegation(&json);
    assert_eq!(json.stdout, output.stdout);
    let _ = fs::remove_dir_all(root);
}
