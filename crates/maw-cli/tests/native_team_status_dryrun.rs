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
    let path = std::env::temp_dir().join(format!("maw-rs-team-t3-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("ψ/teams")).expect("teams");
    fs::create_dir_all(root.join("maw-home/teams/alpha")).expect("registry dir");
    fs::write(
        root.join("maw-home/teams/alpha/oracle-members.json"),
        r#"{"name":"alpha","members":[{"oracle":"builder","role":"builder"},{"oracle":"reviewer","role":"reviewer"}]}"#,
    ).expect("registry");
    let charter = root.join("ψ/teams/alpha.yaml");
    fs::write(
        &charter,
        r"name: alpha
description: Alpha team
goal: Ship T3
members:
  - role: builder
    name: builder
    engine: codex
    cwd: agents/builder
  - role: reviewer
    name: reviewer
    engine: claude
    cwd: agents/reviewer
  - role: scout
    name: scout
    engine: omx
    cwd: agents/scout
",
    )
    .expect("charter");
    charter
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_TMUX_PANES", "alpha|builder|codex|/repo/agents/builder|%1\nalpha|reviewer|bash|/repo/agents/reviewer|%2\nother|scout|omx|/repo/agents/scout|%3")
        .output()
        .expect("run maw-rs")
}

fn assert_golden(name: &str, root: &Path, args: &[&str], expected: &str) {
    let output = run(args, root);
    assert!(
        output.status.success(),
        "{name} stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        expected,
        "{name}"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn team_t3_status_dryrun_liveness_goldens_are_hermetic() {
    let root = temp_dir("goldens");
    let charter = seed(&root);
    let charter_s = charter.to_string_lossy().into_owned();

    assert_golden(
        "up-status",
        &root,
        &["team", "up", "alpha", "--status", "--session", "alpha"],
        include_str!("fixtures/native-team-t3/team-up-status.stdout"),
    );
    assert_golden(
        "up-dry-run",
        &root,
        &["team", "up", "alpha", "--dry-run", "--session", "alpha"],
        include_str!("fixtures/native-team-t3/team-up-dry-run.stdout"),
    );
    assert_golden(
        "bring-dry-run",
        &root,
        &[
            "team",
            "bring",
            "alpha",
            "--dry-run",
            "--session",
            "alpha",
            "--split",
        ],
        include_str!("fixtures/native-team-t3/team-bring-dry-run.stdout"),
    );
    assert_golden(
        "apply-dry-run",
        &root,
        &["team", "apply", &charter_s, "--session", "alpha"],
        include_str!("fixtures/native-team-t3/team-apply-dry-run.stdout"),
    );
    assert_golden(
        "liveness",
        &root,
        &["team", "liveness", "alpha", "--session", "alpha"],
        include_str!("fixtures/native-team-t3/team-liveness.stdout"),
    );
}

#[test]
fn team_t3_rejects_injection_before_exec() {
    let root = temp_dir("guards");
    seed(&root);
    let bad = run(
        &["team", "up", "alpha", "--status", "--engine", "-bad"],
        &root,
    );
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("leading dash rejected"));
}
