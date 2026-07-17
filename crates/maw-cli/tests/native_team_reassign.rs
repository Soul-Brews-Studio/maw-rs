#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
    let root = std::env::temp_dir().join(format!("maw-rs-team-reassign-{name}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("git marker");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/inboxes")).expect("inboxes");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/builder")).expect("member dir");
    fs::create_dir_all(root.join("ψ/teams")).expect("teams");
    fs::create_dir_all(root.join("agents/builder")).expect("builder wt");
    fs::create_dir_all(root.join("agents/reviewer")).expect("reviewer wt");
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[{"name":"builder"},{"name":"reviewer"}],"createdAt":1}"#,
    )
    .expect("config");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/builder.json"),
        r#"[{"message":"inbox"}]"#,
    )
    .expect("inbox");
    fs::write(
        root.join("home/.claude/teams/alpha/builder/builder_findings.md"),
        "finding\n",
    )
    .expect("finding");
    seed_charter(&root, "alpha.yaml", BASE_CHARTER);
    fs::write(root.join("issue.json"), r#"{"title":"Fix quoted path","body":"Do not run '$(touch pwn)'\nUse data only.","labels":[{"name":"bug"}]}"#).expect("issue");
    root
}

const BASE_CHARTER: &str = r"name: alpha
session: alpha
description: Alpha team
members:
  - role: builder
    name: builder
    engine: codex
    cwd: agents/builder
  - role: reviewer
    name: reviewer
    engine: claude
    cwd: agents/reviewer
";

fn seed_charter(root: &Path, name: &str, body: &str) {
    fs::write(root.join("ψ/teams").join(name), body).expect("charter");
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("MAW_TEAM", "alpha")
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SELF_BIN", "/fake/maw")
        .env("MAW_RS_TEAM_REASSIGN_REPO", "tonkmac/maw-rs")
        .env("MAW_RS_TEAM_FAKE_GH_JSON", root.join("issue.json"))
        .env("MAW_RS_TEAM_REASSIGN_FAKE_LOG", root.join("ops.log"))
        .env("MAW_RS_TEAM_DOWN_FAKE_LOG", root.join("ops.log"))
        .env("MAW_RS_TEAM_FAKE_TMUX_LOG", root.join("tmux.jsonl"))
        .env("MAW_RS_TEAM_TMUX_PANES", "alpha|builder|codex|/repo/agents/builder|%1\nalpha|reviewer|claude|/repo/agents/reviewer|%2")
        .output()
        .expect("run maw-rs")
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_golden(root: &Path, args: &[&str], fixture: &str) {
    let output = run(root, args);
    assert!(output.status.success(), "stderr={}", stderr(&output));
    assert_eq!(stdout(&output), fixture);
}

#[test]
fn team_reassign_success_golden_and_ordering() {
    let root = temp_dir("success");
    assert_golden(
        &root,
        &["team", "reassign", "builder", "219"],
        include_str!("fixtures/native-team-reassign/success.stdout"),
    );
    assert_eq!(
        fs::read_to_string(root.join("ops.log")).expect("ops"),
        "fetch\ttonkmac/maw-rs#219\narchive\tbuilder\ndone\talpha:builder\nwake\tbuilder\n"
    );
    assert!(root
        .join("ψ/memory/mailbox/builder/team-alpha-archive/inbox.json")
        .exists());
    let tmux = fs::read_to_string(root.join("tmux.jsonl")).expect("tmux");
    assert!(tmux.contains(r#""args":["new-window","-c","#));
    assert!(tmux.contains(r#""-t","alpha:","-n","builder"]"#));
    assert!(tmux
        .contains(r#""send-keys","-t","alpha:builder","-l","--","'/fake/maw' 'wake' 'builder'"#));
    assert!(tmux.contains("[EXTERNAL CONTENT"));
    assert!(
        tmux.contains("Do not run '\\\\''$(touch pwn)'\\\\''"),
        "issue quote must be POSIX escaped: {tmux}"
    );
}

#[test]
fn team_reassign_error_goldens_are_hermetic() {
    let root = temp_dir("errors");
    let invalid = run(&root, &["team", "reassign", "builder", "0"]);
    assert!(!invalid.status.success());
    assert_eq!(
        stderr(&invalid),
        include_str!("fixtures/native-team-reassign/invalid-issue.stderr")
    );

    let missing = run(&root, &["team", "reassign", "ghost", "219"]);
    assert!(!missing.status.success());
    assert_eq!(
        stderr(&missing),
        include_str!("fixtures/native-team-reassign/not-found.stderr")
    );

    seed_charter(
        &root,
        "alpha.yaml",
        r"name: alpha
session: alpha
members:
  - role: builder
    name: builder
    engine: codex
    cwd: agents/builder
  - role: reviewer
    name: builder
    engine: claude
    cwd: agents/reviewer
",
    );
    let ambiguous = run(&root, &["team", "reassign", "builder", "219"]);
    assert!(!ambiguous.status.success());
    assert_eq!(
        stderr(&ambiguous),
        include_str!("fixtures/native-team-reassign/ambiguous.stderr")
    );
}

#[test]
fn team_reassign_skipped_and_fetch_fail_abort_before_done() {
    let root = temp_dir("abort");
    seed_charter(
        &root,
        "alpha.yaml",
        &BASE_CHARTER.replace(
            "    cwd: agents/builder",
            "    cwd: agents/builder\n    target: remote",
        ),
    );
    let skipped = run(&root, &["team", "reassign", "builder", "219"]);
    assert!(!skipped.status.success());
    assert_eq!(
        stderr(&skipped),
        include_str!("fixtures/native-team-reassign/skipped.stderr")
    );
    assert!(!root.join("ops.log").exists());

    seed_charter(&root, "alpha.yaml", BASE_CHARTER);
    let output = Command::new(bin())
        .args(["team", "reassign", "builder", "219"])
        .current_dir(&root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("MAW_TEAM", "alpha")
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_RS_TEAM_REASSIGN_REPO", "tonkmac/maw-rs")
        .env("MAW_RS_TEAM_FAKE_GH_FAIL", "1")
        .env("MAW_RS_TEAM_REASSIGN_FAKE_LOG", root.join("ops-fail.log"))
        .env("MAW_RS_TEAM_DOWN_FAKE_LOG", root.join("ops-fail.log"))
        .env("MAW_RS_TEAM_FAKE_TMUX_LOG", root.join("tmux-fail.jsonl"))
        .env(
            "MAW_RS_TEAM_TMUX_PANES",
            "alpha|builder|codex|/repo/agents/builder|%1",
        )
        .output()
        .expect("run fail");
    assert!(!output.status.success());
    assert_eq!(
        stderr(&output),
        include_str!("fixtures/native-team-reassign/fetch-fail.stderr")
    );
    assert_eq!(
        fs::read_to_string(root.join("ops-fail.log")).expect("ops"),
        "fetch\ttonkmac/maw-rs#219\n"
    );
    assert!(
        !root.join("tmux-fail.jsonl").exists(),
        "fetch failure must abort before wake"
    );
}

#[test]
fn team_reassign_injection_rejected_before_runner() {
    let root = temp_dir("inject");
    let output = run(&root, &["team", "reassign", "bad;name", "219"]);
    assert!(!output.status.success());
    assert_eq!(
        stderr(&output),
        include_str!("fixtures/native-team-reassign/injection.stderr")
    );
    assert!(!root.join("ops.log").exists());
    assert!(!root.join("tmux.jsonl").exists());
}
