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
fn epic55_follow_matches_committed_golden_without_ref_checkout() {
    let output = epic55_base()
        .args(["follow", "s:main", "--grep", "hello"])
        .env(
            "MAW_RS_FOLLOW_FAKE_STREAM",
            "skip me\n---chunk---\nhello from pane\n",
        )
        .output()
        .expect("run follow");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/epic55/follow-fake.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_activity_follow_guard_leading_dash_values_before_io() {
    let activity = epic55_base()
        .args(["activity", "-pane"])
        .output()
        .expect("activity");
    assert!(!activity.status.success());
    assert!(String::from_utf8(activity.stderr)
        .expect("stderr")
        .contains("usage: maw activity"));

    let follow = epic55_base()
        .args(["follow", "-pane"])
        .output()
        .expect("follow");
    assert!(!follow.status.success());
    assert!(String::from_utf8(follow.stderr)
        .expect("stderr")
        .contains("usage: maw follow"));
}

#[test]
fn epic55_dispatch_registers_activity_follow_without_token_slice() {
    assert_eq!(
        maw_cli::dispatcher_status("activity"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("follow"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("token"),
        maw_cli::DispatchKind::Native
    );
}
