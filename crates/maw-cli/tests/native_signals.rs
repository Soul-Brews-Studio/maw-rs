#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use maw_cli::{dispatcher_status, DispatchKind};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-signals-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_SIGNALS_TODAY", "2026-06-25")
        .output()
        .expect("run maw-rs")
}

fn seed_signals(root: &Path) {
    let dir = root.join("ψ/memory/signals");
    fs::create_dir_all(&dir).expect("signals dir");
    fs::write(
        dir.join("2026-06-24_beta_disk.json"),
        serde_json::json!({
            "timestamp":"2026-06-24T03:00:00.000Z",
            "bud":"beta",
            "kind":"alert",
            "message":"disk high",
            "context":{"percent":91}
        })
        .to_string(),
    )
    .expect("beta signal");
    fs::write(
        dir.join("2026-06-23_alpha_pattern.json"),
        serde_json::json!({
            "timestamp":"2026-06-23T02:00:00.000Z",
            "bud":"alpha",
            "kind":"pattern",
            "message":"repeated context-switch detected"
        })
        .to_string(),
    )
    .expect("pattern signal");
    fs::write(
        dir.join("2026-06-20_alpha_birth.json"),
        serde_json::json!({
            "timestamp":"2026-06-20T01:00:00.000Z",
            "bud":"alpha",
            "kind":"info",
            "message":"bud born: alpha"
        })
        .to_string(),
    )
    .expect("birth signal");
    fs::write(
        dir.join("2026-06-01_old.json"),
        serde_json::json!({
            "timestamp":"2026-06-01T01:00:00.000Z",
            "bud":"old",
            "kind":"info",
            "message":"too old"
        })
        .to_string(),
    )
    .expect("old signal");
    fs::write(dir.join("broken.json"), "{bad json").expect("malformed signal");
    fs::write(dir.join("ignored.txt"), "not json").expect("ignored non-json");
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_signals_text_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("text");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    seed_signals(&cwd);

    let output = run(&["signals"], &cwd, &maw_home);

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-signals/signals.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_signals_json_root_and_days_match_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("json");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    let oracle_root = root.join("oracle");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&oracle_root).expect("oracle root");
    seed_signals(&oracle_root);

    let output = run(
        &[
            "signals",
            "--root",
            oracle_root.to_str().expect("utf8 path"),
            "--days",
            "7",
            "--json",
        ],
        &cwd,
        &maw_home,
    );

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-signals/signals.json")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_signals_empty_directory_matches_maw_js_message_without_ref_checkout() {
    let root = temp_dir("empty");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let output = run(&["signals", "--days", "3"], &cwd, &maw_home);

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "  \u{1b}[90mno signals in the last 3 days\u{1b}[0m\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_signals_root_guard_blocks_option_injection() {
    let root = temp_dir("guard");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let output = run(&["signals", "--root", "--help"], &cwd, &maw_home);

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("--root must be non-empty, unpadded, and not start with '-'"));
}

#[test]
fn native_signals_dispatcher_registered() {
    assert_eq!(dispatcher_status("signals"), DispatchKind::Native);
}
