use maw_cli::{dispatcher_status, DispatchKind};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-wake-cutover-{name}-{stamp}"));
    fs::create_dir_all(root.join("fakebin")).expect("fakebin");
    fs::create_dir_all(root.join("maw-home")).expect("maw home");
    root
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

fn seed_fake_bins(root: &Path, windows: &str) {
    write_executable(
        &root.join("fakebin/tmux"),
        &format!(
            r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$TMUX_WAKE_CALLS"
case "$1" in
  list-sessions)
    exit 0
    ;;
  list-windows)
    cat <<'WINDOWS'
{windows}WINDOWS
    exit 0
    ;;
  *)
    echo "unexpected mutating tmux $*" >&2
    exit 31
    ;;
esac
"#
        ),
    );
    write_executable(
        &root.join("fakebin/maw"),
        r#"#!/usr/bin/env bash
echo "DELEGATED-MAW $*" >> "$FAKE_MAW_LOG"
echo DELEGATED-MAW
exit 99
"#,
    );
}

fn run_wake(root: &Path, calls: &Path, args: &[&str]) -> std::process::Output {
    let path = format!(
        "{}:{}",
        root.join("fakebin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("TMUX_WAKE_CALLS", calls)
        .env("FAKE_MAW_LOG", root.join("fake-maw.log"))
        .env_remove("MAW_RS_WAKE_FALLBACK")
        .env_remove("MAW_PEER_KEY")
        .output()
        .expect("run maw-rs wake")
}

fn assert_no_delegate_or_broadcast(root: &Path, calls: &Path, stdout: &str, stderr: &str) {
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout delegated: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr delegated: {stderr}"
    );
    assert!(
        !root.join("fake-maw.log").exists(),
        "wake cutover spawned PATH maw fallback"
    );
    let tmux_calls = fs::read_to_string(calls).unwrap_or_default();
    assert!(
        tmux_calls.lines().all(|line| {
            line.starts_with("list-windows ")
                || line.starts_with("list-sessions ")
                || line.starts_with("display-message ")
        }),
        "wake cutover must only inspect fake tmux, got: {tmux_calls}"
    );
    assert!(
        !tmux_calls.contains("send-keys"),
        "wake broadcast via tmux: {tmux_calls}"
    );
    assert!(
        !tmux_calls.contains("new-session"),
        "wake spawned tmux session: {tmux_calls}"
    );
}

#[test]
fn wake_cutover_local_route_fails_closed_without_maw_js_or_broadcast() {
    let root = temp_dir("local");
    seed_fake_bins(&root, "47-mawjs|||0|||mawjs|||1|||/tmp\n");
    let calls = root.join("tmux.calls");
    let output = run_wake(
        &root,
        &calls,
        &["wake", "local:mawjs", "--from", "sender:node"],
    );

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        include_str!("fixtures/native-wake-cutover/local-fail-closed.stderr")
    );
    assert_eq!(dispatcher_status("wake"), DispatchKind::Native);
    assert_no_delegate_or_broadcast(&root, &calls, &stdout, &stderr);
}

#[test]
fn wake_cutover_unknown_remote_fails_closed_without_maw_js_or_cross_fleet_broadcast() {
    let root = temp_dir("unknown-remote");
    seed_fake_bins(&root, "");
    let calls = root.join("tmux.calls");
    let output = run_wake(
        &root,
        &calls,
        &["wake", "elsewhere:agent", "--from", "sender:node"],
    );

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        include_str!("fixtures/native-wake-cutover/unknown-remote-fail-closed.stderr")
    );
    assert_no_delegate_or_broadcast(&root, &calls, &stdout, &stderr);
}
