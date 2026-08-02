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
    let path = std::env::temp_dir().join(format!("maw-rs-team-shutdown-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed(root: &Path) {
    fs::create_dir_all(root.join("home/.claude/teams/alpha/inboxes")).expect("inboxes");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/builder")).expect("builder");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/reviewer")).expect("reviewer");
    fs::create_dir_all(root.join("maw-home/teams/alpha/tasks")).expect("tasks");
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[{"name":"lead","agentType":"team-lead","tmuxPaneId":"%0"},{"name":"builder","tmuxPaneId":"%1"},{"name":"reviewer","tmuxPaneId":"%2"}],"createdAt":1}"#,
    ).expect("config");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/builder.json"),
        "[]",
    )
    .expect("builder inbox");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/reviewer.json"),
        "[]",
    )
    .expect("reviewer inbox");
    fs::write(
        root.join("home/.claude/teams/alpha/builder/builder_findings.md"),
        "builder finding\n",
    )
    .expect("findings");
    fs::write(
        root.join("home/.claude/teams/alpha/reviewer/reviewer_findings.md"),
        "reviewer finding\n",
    )
    .expect("findings");
    fs::write(
        root.join("maw-home/teams/alpha/tasks/1.json"),
        r#"{"id":1}"#,
    )
    .expect("task");
}

fn run(args: &[&str], root: &Path, panes: &str, log: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_SHUTDOWN_STAMP", "fixed")
        .env("MAW_RS_TEAM_FIXED_TIME", "2026-06-25T20:30:00+07:00")
        .env("MAW_RS_TEAM_TMUX_PANES", panes)
        .env("MAW_RS_TEAM_SHUTDOWN_FAKE_LOG", log)
        .output()
        .expect("run maw-rs")
}

fn panes() -> &'static str {
    "alpha|builder|codex|/repo/builder|%1\nalpha|reviewer|claude|/repo/reviewer|%2"
}

#[test]
fn team_shutdown_force_merge_archives_fake_wait_and_kills_only_valid_panes() {
    let root = temp_dir("force-merge");
    seed(&root);
    let log = root.join("fake/shutdown.log");
    let output = run(
        &["team", "shutdown", "alpha", "--force", "--merge"],
        &root,
        panes(),
        &log,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-team-shutdown/team-shutdown-force-merge.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(&log).expect("fake log"),
        "wait\tfake-no-real-30s\nkill\t%1\nkill\t%2\n"
    );
    let archive = root.join("ψ/memory/mailbox/teams/alpha/shutdown-archive-fixed");
    assert!(
        archive.join("tool-team/config.json").exists(),
        "config archived before cleanup"
    );
    assert!(
        archive.join("tasks/1.json").exists(),
        "tasks archived before cleanup"
    );
    assert!(
        root.join("ψ/memory/mailbox/builder/team-alpha-inbox.json")
            .exists(),
        "merge copied inbox"
    );
    assert!(
        root.join("ψ/memory/mailbox/builder/builder_findings.md")
            .exists(),
        "merge copied findings"
    );
    assert!(
        !root.join("home/.claude/teams/alpha").exists(),
        "cleanup removes tool team after archive"
    );
    assert!(
        !root.join("maw-home/teams/alpha").exists(),
        "cleanup removes tasks team after archive"
    );
}

#[test]
fn team_shutdown_no_force_keeps_state_when_members_survive() {
    let root = temp_dir("no-force");
    seed(&root);
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[{"name":"builder","tmuxPaneId":"%1"}],"createdAt":1}"#,
    )
    .expect("config");
    let log = root.join("fake/shutdown.log");
    let output = run(
        &["team", "shutdown", "alpha"],
        &root,
        "alpha|builder|codex|/repo/builder|%1",
        &log,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-team-shutdown/team-shutdown-no-force.stdout")
    );
    assert_eq!(
        fs::read_to_string(&log).expect("fake log"),
        "wait\tfake-no-real-30s\n",
        "fake clock should avoid real 30s and no kill without --force"
    );
    assert!(
        root.join("home/.claude/teams/alpha").exists(),
        "leftovers kept when members survive"
    );
    assert!(
        root.join("maw-home/teams/alpha").exists(),
        "leftovers kept when members survive"
    );
}

#[test]
fn team_shutdown_rejects_pane_mismatch_and_injection_before_force_kill() {
    let root = temp_dir("guards");
    seed(&root);
    let log = root.join("fake/shutdown.log");
    let mismatch = run(
        &["team", "shutdown", "alpha", "--force"],
        &root,
        "alpha|wrong-window|codex|/repo/builder|%1",
        &log,
    );
    assert!(!mismatch.status.success());
    assert!(
        String::from_utf8_lossy(&mismatch.stderr)
            .contains("refuse pane mismatch before force kill")
            || String::from_utf8_lossy(&mismatch.stderr)
                .contains("refuse missing pane before force kill")
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "wait\tfake-no-real-30s\n",
        "must not kill mismatched pane"
    );

    let bad = run(&["team", "shutdown", "-alpha"], &root, panes(), &log);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("leading dash rejected"));
}
