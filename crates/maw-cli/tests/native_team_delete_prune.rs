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
    let path = std::env::temp_dir().join(format!("maw-rs-team-delete-prune-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed_delete(root: &Path) {
    fs::create_dir_all(root.join("home/.claude/teams/alpha/inboxes")).expect("team dir");
    fs::create_dir_all(root.join("maw-home/teams/alpha/tasks")).expect("tasks");
    fs::create_dir_all(root.join("ψ/memory/mailbox/teams/alpha")).expect("vault");
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[],"createdAt":1}"#,
    )
    .expect("config");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/worker.json"),
        "[]",
    )
    .expect("inbox");
    fs::write(
        root.join("maw-home/teams/alpha/tasks/1.json"),
        r#"{"id":1}"#,
    )
    .expect("task");
    fs::write(
        root.join("ψ/memory/mailbox/teams/alpha/manifest.json"),
        r#"{"name":"alpha"}"#,
    )
    .expect("manifest");
}

fn seed_prune(root: &Path) {
    for (name, members) in [
        ("empty", "[]"),
        ("memberful", "[{\"name\":\"worker\"}]"),
        ("active", "[]"),
    ] {
        fs::create_dir_all(root.join(format!("home/.claude/teams/{name}"))).expect("team dir");
        fs::write(
            root.join(format!("home/.claude/teams/{name}/config.json")),
            format!(r#"{{"name":"{name}","members":{members},"createdAt":1}}"#),
        )
        .expect("config");
    }
    fs::create_dir_all(root.join("home/.claude/teams/malformed")).expect("malformed dir");
    fs::write(
        root.join("home/.claude/teams/malformed/config.json"),
        "not-json",
    )
    .expect("malformed config");
    fs::create_dir_all(root.join("home/.claude/teams/bad/name")).expect("ignored nested bad");
}

fn seed_gc(root: &Path) {
    for (name, members, created_at) in [
        ("active", "[{\"name\":\"worker\",\"isActive\":true}]", "1"),
        ("empty", "[]", "1"),
        ("inactive-future", "[{\"name\":\"worker\"}]", "18446744073709551615"),
        ("inactive-old", "[{\"name\":\"worker\"}]", "1"),
        ("session-dead", "[{\"name\":\"worker\"}]", "18446744073709551615"),
    ] {
        fs::create_dir_all(root.join(format!("home/.claude/teams/{name}"))).expect("team dir");
        fs::write(
            root.join(format!("home/.claude/teams/{name}/config.json")),
            format!(r#"{{"name":"{name}","members":{members},"createdAt":{created_at}}}"#),
        )
        .expect("config");
    }
    fs::create_dir_all(root.join("home/.claude/teams/malformed")).expect("malformed dir");
    fs::write(root.join("home/.claude/teams/malformed/config.json"), "not-json")
        .expect("malformed config");
    fs::create_dir_all(root.join("maw-home/teams/inactive-old/tasks")).expect("tasks");
    fs::write(root.join("maw-home/teams/inactive-old/tasks/1.json"), r#"{"id":1}"#)
        .expect("task");
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_RS_TEAM_DELETE_STAMP", "fixed")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_TMUX_SESSIONS", "active\n42-active")
        .output()
        .expect("run maw-rs")
}

#[test]
fn team_delete_archives_before_bounded_remove_and_matches_golden() {
    let root = temp_dir("delete");
    seed_delete(&root);
    let output = run(&["team", "delete", "alpha"], &root);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-team-delete-prune/team-delete.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(
        !root.join("home/.claude/teams/alpha").exists(),
        "team dir must be removed after archive"
    );
    assert!(
        !root.join("maw-home/teams/alpha/tasks").exists(),
        "tasks dir must be removed after archive"
    );
    let archive = root.join("ψ/memory/mailbox/teams/alpha/delete-archive-fixed");
    assert!(
        archive.join("tool-team/config.json").exists(),
        "config archived before rm"
    );
    assert!(
        archive.join("tool-team/inboxes/worker.json").exists(),
        "inbox archived before rm"
    );
    assert!(
        archive.join("tasks/1.json").exists(),
        "tasks archived before rm"
    );
    assert!(
        archive.join("manifest.json").exists(),
        "manifest archived before rm"
    );
}

#[test]
fn team_prune_skips_active_memberful_malformed_and_archives() {
    let root = temp_dir("prune");
    seed_prune(&root);
    let output = run(&["team", "prune"], &root);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-team-delete-prune/team-prune.stdout")
    );
    assert!(
        !root.join("home/.claude/teams/empty").exists(),
        "empty inactive team pruned"
    );
    assert!(
        root.join("home/.claude/teams/memberful").exists(),
        "memberful team skipped"
    );
    assert!(
        root.join("home/.claude/teams/active").exists(),
        "active team skipped"
    );
    assert!(
        root.join("home/.claude/teams/malformed").exists(),
        "malformed team skipped"
    );
    assert!(root
        .join("ψ/memory/mailbox/teams/empty/delete-archive-fixed/tool-team/config.json")
        .exists());
}

#[test]
fn team_gc_dry_run_then_confirm_removes_only_stale_inactive_candidates() {
    let root = temp_dir("gc");
    seed_gc(&root);
    let dry = run(&["team", "gc"], &root);
    assert!(
        dry.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert_eq!(
        String::from_utf8(dry.stdout).expect("stdout"),
        "team gc dry-run: 3 candidate(s)\n  - empty [empty] members=0\n  - inactive-old [all-inactive] members=1\n  - session-dead [session-temp] members=1\n  rerun with --confirm to remove\n"
    );
    assert!(root.join("home/.claude/teams/empty").exists(), "dry-run must not remove");

    let confirmed = run(&["team", "gc", "--confirm"], &root);
    assert!(
        confirmed.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    assert_eq!(
        String::from_utf8(confirmed.stdout).expect("stdout"),
        "team gc: 3 removed\n  - empty [empty] members=0\n  - inactive-old [all-inactive] members=1\n  - session-dead [session-temp] members=1\n"
    );
    for name in ["empty", "inactive-old", "session-dead"] {
        assert!(!root.join(format!("home/.claude/teams/{name}")).exists(), "{name} removed");
    }
    for name in ["active", "inactive-future", "malformed"] {
        assert!(root.join(format!("home/.claude/teams/{name}")).exists(), "{name} kept");
    }
    assert!(
        !root.join("maw-home/teams/inactive-old/tasks").exists(),
        "gc removes archived team tasks"
    );
    assert!(root
        .join("ψ/memory/mailbox/teams/inactive-old/delete-archive-fixed/tasks/1.json")
        .exists());
}

#[test]
fn team_delete_prune_reject_injection_and_unbounded_remove_helper() {
    let root = temp_dir("guards");
    seed_delete(&root);
    let bad = run(&["team", "delete", "../alpha"], &root);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("path traversal rejected"));
    assert!(
        root.join("home/.claude/teams/alpha").exists(),
        "bad name must not remove"
    );

    let dash = run(&["team", "delete", "-alpha"], &root);
    assert!(!dash.status.success());
    assert!(String::from_utf8_lossy(&dash.stderr).contains("leading dash rejected"));
}
