#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-spsw-{name}-{stamp}"));
    fs::create_dir_all(path.join("bin")).expect("temp bin");
    fs::create_dir_all(path.join("config")).expect("temp config");
    fs::create_dir_all(path.join("state")).expect("temp state");
    path
}

fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn write_fake_maw(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        r#"#!/bin/sh
printf 'DELEGATED-MAW %s\n' "$*"
exit 77
"#,
    )
    .expect("write fake maw");
    chmod_exec(&maw);
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("HOME", root)
        .env("MAW_CONFIG_DIR", root.join("config"))
        .env("MAW_STATE_DIR", root.join("state"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run maw-rs")
}

fn assert_no_delegation(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout delegated: {stdout:?}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr delegated: {stderr:?}"
    );
}

#[test]
fn spsw_dispatch_is_native() {
    assert_eq!(
        dispatcher_status("serve-peer-startup-warnings"),
        DispatchKind::Native
    );
}

#[test]
fn spsw_no_warning_golden_and_fake_maw_native_proof() {
    let root = temp_dir("no-warning");
    write_fake_maw(&root.join("bin"));
    fs::write(
        root.join("config/maw.config.json"),
        r#"{"node":"nova-node","oracle":"nova","port":3456,"federationToken":"secret-value-never-print"}
"#,
    )
    .expect("config");
    fs::write(
        root.join("state/peers.json"),
        r#"{"version":1,"peers":{}}
"#,
    )
    .expect("peers");

    let output = run(&root, &["serve-peer-startup-warnings"]);
    assert!(output.status.success(), "status={:?}", output.status);
    assert_no_delegation(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("fixtures/native-serve-peer-startup-warnings/no-warnings.stdout")
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-value"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-value"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn spsw_warning_stderr_matches_golden_without_real_maw() {
    let root = temp_dir("warning");
    write_fake_maw(&root.join("bin"));
    fs::write(
        root.join("config/maw.config.json"),
        r#"{"node":"nova-node","oracle":"nova","port":3456,"namedPeers":[{"name":"peer-a","url":"https://peer.example.test"}]}
"#,
    )
    .expect("config");
    fs::write(
        root.join("state/peers.json"),
        r#"{"version":1,"peers":{"peer-a":{"url":"https://peer.example.test","node":"peer-node","addedAt":"2026-06-24T09:00:00.000Z","lastSeen":null,"identity":{"oracle":"nova","node":"nova-node"}}}}
"#,
    )
    .expect("peers");

    let output = run(&root, &["serve-peer-startup-warnings"]);
    assert!(output.status.success(), "status={:?}", output.status);
    assert_no_delegation(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        include_str!("fixtures/native-serve-peer-startup-warnings/warnings.stderr")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn spsw_rejects_unexpected_arguments_before_delegation() {
    let root = temp_dir("reject");
    write_fake_maw(&root.join("bin"));
    let output = run(&root, &["serve-peer-startup-warnings", "--bad"]);
    assert!(!output.status.success());
    assert_no_delegation(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown flag --bad"));
    let _ = fs::remove_dir_all(root);
}
