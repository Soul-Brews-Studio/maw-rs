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
    let path = std::env::temp_dir().join(format!("maw-rs-team-remove-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed(root: &Path) {
    fs::create_dir_all(root.join(".maw/teams")).expect("teams");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/inboxes")).expect("inboxes");
    fs::write(
        root.join(".maw/teams/alpha.yaml"),
        r"name: alpha
session: lead-session
members:
  - role: lead
    name: mawjs-oracle
    engine: codex
  - role: worker
    name: mawjs-worker
    engine: omx
",
    )
    .expect("charter");
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[{"name":"lead"},{"name":"worker"}],"createdAt":1}"#,
    )
    .expect("config");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/worker.json"),
        r#"[{"message":"remove me"}]"#,
    )
    .expect("inbox");
}

fn run(args: &[&str], root: &Path, log: Option<&Path>, panes: &str) -> std::process::Output {
    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_TEAM", "alpha")
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_TMUX_PANES", panes);
    if let Some(log) = log {
        command.env("MAW_RS_TEAM_REMOVE_FAKE_LOG", log);
    }
    command.output().expect("run maw-rs")
}

fn live_panes() -> &'static str {
    "lead-session|mawjs-oracle|claude|/repo/lead|%1\nlead-session|mawjs-worker|omx|/repo/worker|%2"
}

fn normalize(root: &Path, bytes: &[u8]) -> String {
    // On macOS, std::env::temp_dir() lives under /var/folders or /tmp, but the
    // command output canonicalizes through the /private symlink — so replace the
    // canonical path too, otherwise a stray `/private` prefix leaks into goldens.
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    String::from_utf8(bytes.to_vec())
        .expect("utf8")
        .replace(&canonical.display().to_string(), "<ROOT>")
        .replace(&root.display().to_string(), "<ROOT>")
}

#[test]
fn team_remove_dry_run_and_live_goldens_are_hermetic() {
    let root = temp_dir("goldens");
    seed(&root);
    let before = fs::read_to_string(root.join(".maw/teams/alpha.yaml")).expect("before");
    let dry_log = root.join("fake/dry.log");
    let dry = run(
        &["team", "remove", "worker", "--dry-run", "--keep-branch"],
        &root,
        Some(&dry_log),
        live_panes(),
    );
    assert!(
        dry.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert_eq!(
        normalize(&root, &dry.stdout),
        include_str!("fixtures/native-team-remove/team-remove-dry-run.stdout")
    );
    assert_eq!(
        fs::read_to_string(root.join(".maw/teams/alpha.yaml")).expect("after dry"),
        before
    );
    assert!(
        !dry_log.exists(),
        "dry-run must not archive or call cmdDone"
    );

    let live_log = root.join("fake/live.log");
    let live = run(
        &["team", "remove", "worker"],
        &root,
        Some(&live_log),
        live_panes(),
    );
    assert!(
        live.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert_eq!(
        normalize(&root, &live.stdout),
        include_str!("fixtures/native-team-remove/team-remove-live.stdout")
    );
    assert_eq!(
        fs::read_to_string(&live_log).expect("fake log"),
        "archive\tworker\ndone\tlead-session:mawjs-worker:keep_branch=false\n",
        "archive must be recorded before cmdDone",
    );
    let after = fs::read_to_string(root.join(".maw/teams/alpha.yaml")).expect("after live");
    assert!(!after.contains("role: worker"));
    assert!(after.contains("role: lead"));
    assert!(root
        .join("ψ/memory/mailbox/worker/team-alpha-remove-archive/charter.yaml")
        .exists());
    assert!(root
        .join("ψ/memory/mailbox/worker/team-alpha-remove-archive/inbox.json")
        .exists());
}

#[test]
fn team_remove_refuses_missing_ambiguous_last_and_injection_before_done() {
    let root = temp_dir("guards");
    seed(&root);
    let log = root.join("fake/guards.log");
    let missing = run(
        &["team", "remove", "worker"],
        &root,
        Some(&log),
        "lead-session|mawjs-oracle|claude|/repo/lead|%1",
    );
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr)
        .contains("refuse missing live target before teardown"));
    assert!(!log.exists(), "missing live target must reject before done");

    let ambiguous = run(
        &["team", "remove", "worker"],
        &root,
        Some(&log),
        "lead-session|mawjs-worker|omx|/repo/a|%1\nlead-session|mawjs-worker|codex|/repo/b|%2",
    );
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr)
        .contains("refuse ambiguous live target before teardown"));
    assert!(
        !log.exists(),
        "ambiguous live target must reject before done"
    );

    let bad_selector = run(
        &["team", "remove", "-worker"],
        &root,
        Some(&log),
        live_panes(),
    );
    assert!(!bad_selector.status.success());
    assert!(String::from_utf8_lossy(&bad_selector.stderr).contains("leading dash rejected"));
    assert!(!log.exists(), "bad selector must reject before done");

    fs::write(
        root.join(".maw/teams/alpha.yaml"),
        "name: alpha\nsession: lead-session\nmembers:\n  - role: lead\n    name: mawjs-oracle\n",
    )
    .expect("solo charter");
    let last = run(&["team", "remove", "lead"], &root, Some(&log), live_panes());
    assert!(!last.status.success());
    assert!(String::from_utf8_lossy(&last.stderr).contains("refusing to remove the last member"));
    assert!(!log.exists(), "last-member reject must not call done");
}

#[test]
fn team_remove_worktree_false_edits_charter_without_done() {
    let root = temp_dir("no-worktree");
    seed(&root);
    fs::write(
        root.join(".maw/teams/alpha.yaml"),
        r"name: alpha
session: lead-session
members:
  - role: lead
    name: mawjs-oracle
  - role: peer
    name: oss-oracle
    worktree: false
",
    )
    .expect("charter");
    let log = root.join("fake/no-worktree.log");
    let output = run(&["team", "remove", "peer"], &root, Some(&log), live_panes());
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !log.exists(),
        "worktree:false must not call done/archive fake log"
    );
    let after = fs::read_to_string(root.join(".maw/teams/alpha.yaml")).expect("after");
    assert!(!after.contains("role: peer"));
    assert!(after.contains("role: lead"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("no worktree for oss-oracle"));
}
