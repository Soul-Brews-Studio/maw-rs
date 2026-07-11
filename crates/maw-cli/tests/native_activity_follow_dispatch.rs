use std::{path::PathBuf, process::Command};

fn epic55_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn epic55_base() -> Command {
    let mut command = Command::new(epic55_bin());
    command.env("MAW_JS_REF_DIR", "/nonexistent");
    command
}

fn epic55_follow() -> (Command, PathBuf) {
    let root = std::env::temp_dir().join(format!("maw-rs-follow-plugin-{}", std::process::id()));
    let plugin = root.join("follow");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin).expect("plugin dir");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epic55/follow-plugin");
    std::fs::copy(fixture.join("plugin.json"), plugin.join("plugin.json")).expect("manifest");
    std::fs::copy(fixture.join("plugin.wasm"), plugin.join("plugin.wasm")).expect("wasm");
    let mut command = epic55_base();
    command.env("MAW_PLUGINS_DIR", &root);
    (command, root)
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
fn epic55_follow_plugin_preserves_usage_guard() {
    let (mut command, root) = epic55_follow();
    let output = command.args(["follow", "-pane"]).output().expect("follow");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("usage: maw follow"));
    let _ = std::fs::remove_dir_all(root);
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

    let (mut follow_command, root) = epic55_follow();
    let follow = follow_command
        .args(["follow", "-pane"])
        .output()
        .expect("follow");
    assert!(!follow.status.success());
    assert!(String::from_utf8(follow.stderr)
        .expect("stderr")
        .contains("usage: maw follow"));
    let _ = std::fs::remove_dir_all(root);
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
