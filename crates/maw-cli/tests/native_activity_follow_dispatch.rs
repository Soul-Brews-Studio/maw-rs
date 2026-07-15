// Dispatcher registration pins (and other pre-invoke coverage) run on the
// default test path. The wasm-host follow-plugin usage-guard test was removed
// in the repo split — the epic55/follow-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
use std::{path::PathBuf, process::Command};

fn epic55_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn epic55_base() -> Command {
    let mut command = Command::new(epic55_bin());
    command.env("MAW_JS_REF_DIR", "/nonexistent");
    command
}

#[test]
fn epic55_activity_matches_committed_golden_without_ref_checkout() {
    let output = epic55_base()
        .args(["activity", "s:main", "--json", "--window=2s", "--samples=2"])
        .env("MAW_RS_ACTIVITY_FAKE_CAPTURE", "ready\n---sample---\nready")
        .output()
        .expect("run activity");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/epic55/activity-idle-json.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_activity_guard_leading_dash_values_before_io() {
    let activity = epic55_base()
        .args(["activity", "-pane"])
        .output()
        .expect("activity");
    assert!(!activity.status.success());
    assert!(String::from_utf8(activity.stderr)
        .expect("stderr")
        .contains("usage: maw activity"));
}

#[test]
fn epic55_dispatch_registers_activity_follow_without_token_slice() {
    assert_eq!(
        maw_cli::dispatcher_status("activity"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("follow"),
        maw_cli::DispatchKind::NativeError
    );
    assert_eq!(
        maw_cli::dispatcher_status("token"),
        maw_cli::DispatchKind::Native
    );
}
