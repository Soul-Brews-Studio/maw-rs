use std::{
    fs,
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
    let path = std::env::temp_dir().join(format!("maw-rs-epic56-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], maw_home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn assert_stdout_golden(name: &str, args: &[&str], expected: &str) {
    let root = temp_dir(name);
    let output = run(args, &root);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic56_attach_view_split_committed_golden_without_js_ref() {
    assert_stdout_golden(
        "attach-local-plan",
        &["attach", "mawjs", "--alive", "50-mawjs", "--print"],
        include_str!("fixtures/epic56/attach-local-plan.stdout"),
    );
    assert_stdout_golden(
        "view-readonly-plan",
        &["view", "mawjs", "--alive", "50-mawjs"],
        include_str!("fixtures/epic56/view-readonly-plan.stdout"),
    );
    assert_stdout_golden(
        "split-dry-run",
        &[
            "split",
            "%isolated",
            "--vertical",
            "--pct",
            "25",
            "--cmd",
            "echo hi",
            "--dry-run",
        ],
        include_str!("fixtures/epic56/split-dry-run.stdout"),
    );
}

#[test]
fn epic56_stream_attach_ssh_committed_golden_without_js_ref() {
    assert_stdout_golden(
        "stream-unlink-dry-run",
        &["stream", "--unlink", "view:oracle", "--dry-run"],
        include_str!("fixtures/epic56/stream-unlink-dry-run.stdout"),
    );
    assert_stdout_golden(
        "attach-ssh-dry-run",
        &[
            "attach-ssh",
            "--node",
            "peer-one",
            "--session",
            "50-mawjs",
            "--ssh-alias",
            "peer-one",
            "--dry-run",
        ],
        include_str!("fixtures/epic56/attach-ssh-dry-run.stdout"),
    );
}
