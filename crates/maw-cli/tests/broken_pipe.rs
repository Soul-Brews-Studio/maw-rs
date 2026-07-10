use std::{
    fmt::Write as _,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_root(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("maw-rs-{name}-{}-{nanos}", std::process::id()))
}

#[test]
fn audit_closed_stdout_pipe_exits_zero_without_panic() {
    let root = temp_root("broken-pipe-audit");
    let state = root.join("state");
    let maw_state = state.join("maw");
    std::fs::create_dir_all(root.join("home")).expect("home dir");
    std::fs::create_dir_all(&maw_state).expect("state dir");

    let mut audit = String::new();
    for index in 0..50_000 {
        let _ = writeln!(
            audit,
            r#"{{"ts":"2026-07-10T00:00:{:02}.000Z","cmd":"audit-spam","args":["{index}"],"result":"ok"}}"#,
            index % 60
        );
    }
    std::fs::write(maw_state.join("audit.jsonl"), audit).expect("audit log");

    let mut child = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["audit", "50000"])
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", &state)
        .env("MAW_XDG", "1")
        .env_remove("MAW_HOME")
        .env_remove("MAW_STATE_DIR")
        .env_remove("TMUX")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn maw-rs audit");

    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait maw-rs audit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr={stderr}");
    assert!(!stderr.contains("panicked"), "stderr={stderr}");

    std::fs::remove_dir_all(root).expect("cleanup");
}
