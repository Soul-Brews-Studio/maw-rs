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
    let path = std::env::temp_dir().join(format!("maw-rs-team-task-ops-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_fake_maw(root: &Path) -> PathBuf {
    let bin = root.join("fake-bin");
    fs::create_dir_all(&bin).expect("fake bin");
    let maw = bin.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW >&2\nexit 73\n").expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(&maw).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&maw, perms).expect("chmod");
    }
    bin
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    let fake_bin = write_fake_maw(root);
    let home = root.join("home");
    let state = root.join("state");
    let config = root.join("config");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&state).expect("state");
    fs::create_dir_all(&config).expect("config");
    Command::new(bin())
        .args(args)
        .env("HOME", &home)
        .env("MAW_STATE_DIR", &state)
        .env("MAW_CONFIG_DIR", &config)
        .env("MAW_TEAM", "alpha")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_FIXED_TIME", "2026-06-27T01:00:00.000Z")
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("run maw-rs")
}

fn assert_stdout_golden(name: &str, root: &Path, args: &[&str], expected: &str) {
    let output = run(args, root);
    assert!(
        output.status.success(),
        "{name} stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stdout, expected, "{name}");
    assert!(!stdout.contains("DELEGATED-MAW"), "{name} delegated stdout");
    assert!(!stderr.contains("DELEGATED-MAW"), "{name} delegated stderr");
    assert_eq!(stderr, "");
}

fn task_path(root: &Path, team: &str, id: u64) -> PathBuf {
    root.join("state")
        .join("teams")
        .join(team)
        .join("tasks")
        .join(format!("{id}.json"))
}

fn counter_path(root: &Path, team: &str) -> PathBuf {
    root.join("state")
        .join("teams")
        .join(team)
        .join("tasks")
        .join("_counter.json")
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).expect("json file")).expect("json")
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

#[test]
fn team_task_ops_are_native_atomic_and_preserve_unknown_keys() {
    let root = temp_dir("golden");

    assert_stdout_golden(
        "add",
        &root,
        &[
            "team",
            "add",
            "First",
            "task",
            "--assign",
            "builder",
            "--description",
            "Detailed task",
        ],
        include_str!("fixtures/native-team-task-ops/team-add.stdout"),
    );
    assert_stdout_golden(
        "tasks-pending",
        &root,
        &["team", "tasks", "alpha"],
        include_str!("fixtures/native-team-task-ops/team-tasks-pending.stdout"),
    );

    let path = task_path(&root, "alpha", 1);
    let mut task = read_json(&path);
    task.as_object_mut()
        .expect("object")
        .insert("xUnknown".to_owned(), serde_json::json!("keep-me"));
    fs::write(&path, serde_json::to_string_pretty(&task).expect("encode")).expect("seed unknown");

    assert_stdout_golden(
        "assign",
        &root,
        &["team", "assign", "1", "reviewer"],
        include_str!("fixtures/native-team-task-ops/team-assign.stdout"),
    );
    assert_stdout_golden(
        "done",
        &root,
        &["team", "done", "1"],
        include_str!("fixtures/native-team-task-ops/team-done.stdout"),
    );
    assert_stdout_golden(
        "tasks-completed",
        &root,
        &["team", "tasks", "--team", "alpha"],
        include_str!("fixtures/native-team-task-ops/team-tasks-completed.stdout"),
    );

    let final_task = read_json(&path);
    assert_eq!(final_task["subject"], "First task");
    assert_eq!(final_task["description"], "Detailed task");
    assert_eq!(final_task["assignee"], "reviewer");
    assert_eq!(final_task["status"], "completed");
    assert_eq!(final_task["createdAt"], "2026-06-27T01:00:00.000Z");
    assert_eq!(final_task["updatedAt"], "2026-06-27T01:00:00.000Z");
    assert_eq!(final_task["xUnknown"], "keep-me");
    assert_eq!(read_json(&counter_path(&root, "alpha"))["next"], 2);

    #[cfg(unix)]
    {
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&counter_path(&root, "alpha")), 0o600);
    }
}

#[test]
fn team_task_ops_read_legacy_and_write_primary_without_dropping_unknown_keys() {
    let root = temp_dir("legacy");
    let legacy_dir = root.join("config/teams/legacy/tasks");
    fs::create_dir_all(&legacy_dir).expect("legacy dir");
    fs::write(
        legacy_dir.join("7.json"),
        r#"{
  "id": 7,
  "subject": "Legacy task",
  "status": "pending",
  "createdAt": "old",
  "updatedAt": "old",
  "xUnknown": "legacy-keep"
}
"#,
    )
    .expect("legacy task");

    assert_stdout_golden(
        "done-legacy",
        &root,
        &["team", "done", "7", "--team", "legacy"],
        include_str!("fixtures/native-team-task-ops/team-done-legacy.stdout"),
    );

    let primary = task_path(&root, "legacy", 7);
    let migrated = read_json(&primary);
    assert_eq!(migrated["status"], "completed");
    assert_eq!(migrated["xUnknown"], "legacy-keep");
}

#[test]
fn team_task_ops_reject_injection_before_writing() {
    let root = temp_dir("guards");
    let bad_subject = run(&["team", "add", "-bad"], &root);
    assert!(!bad_subject.status.success());
    assert!(String::from_utf8_lossy(&bad_subject.stderr).contains("leading dash rejected"));
    assert!(!root.join("state/teams/alpha/tasks").exists());

    let bad_member = run(&["team", "assign", "1", "../bad"], &root);
    assert!(!bad_member.status.success());
    assert!(String::from_utf8_lossy(&bad_member.stderr).contains("path traversal rejected"));
    assert!(!root.join("state/teams/alpha/tasks").exists());
}
