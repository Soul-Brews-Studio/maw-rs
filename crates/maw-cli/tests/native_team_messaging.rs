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
    let path = std::env::temp_dir().join(format!("maw-rs-team-t2-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed_team(root: &Path) {
    let team_dir = root.join("home/.claude/teams/alpha");
    let registry_dir = root.join("maw-home/teams/alpha");
    fs::create_dir_all(&team_dir).expect("team dir");
    fs::create_dir_all(&registry_dir).expect("registry dir");
    fs::write(
        team_dir.join("config.json"),
        r#"{"name":"alpha","members":[{"name":"builder"},{"name":"reviewer"},{"name":"team-lead","agentType":"team-lead"}],"createdAt":1}"#,
    )
    .expect("config");
    fs::write(
        registry_dir.join("oracle-members.json"),
        r#"{"name":"alpha","members":[{"oracle":"oracle-one","role":"advisor","addedAt":"2026-06-25T00:00:00Z"}],"createdAt":"2026-06-25T00:00:00Z"}"#,
    )
    .expect("registry");
    fs::create_dir_all(root.join("psi")).expect("psi");
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_FIXED_TIME", "2026-06-25T08:00:00Z")
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
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        expected,
        "{name}"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn team_t2_comms_goldens_are_hermetic_and_use_inboxes() {
    let root = temp_dir("goldens");
    seed_team(&root);

    assert_stdout_golden(
        "send-single",
        &root,
        &["team", "send", "alpha", "builder", "Hello", "builder"],
        include_str!("fixtures/native-team-t2-comms/team-send-single.stdout"),
    );
    assert_stdout_golden(
        "send-broadcast",
        &root,
        &["team", "send", "alpha", "Hello", "all"],
        include_str!("fixtures/native-team-t2-comms/team-send-broadcast.stdout"),
    );
    assert_stdout_golden(
        "inbox-live",
        &root,
        &["team", "inbox", "alpha", "builder"],
        include_str!("fixtures/native-team-t2-comms/team-inbox-live.stdout"),
    );
    assert_stdout_golden(
        "send-vault",
        &root,
        &["team", "send", "ghost", "scout", "Hello", "offline"],
        include_str!("fixtures/native-team-t2-comms/team-send-vault.stdout"),
    );
    assert_stdout_golden(
        "inbox-vault",
        &root,
        &["team", "inbox", "ghost", "scout"],
        include_str!("fixtures/native-team-t2-comms/team-inbox-vault.stdout"),
    );

    let live = fs::read_to_string(root.join("home/.claude/teams/alpha/inboxes/builder.json"))
        .expect("live inbox");
    assert!(live.contains("Hello builder"));
    assert!(live.contains("Hello all"));
    let vault_entries = fs::read_dir(root.join("psi/memory/mailbox/scout"))
        .expect("vault mailbox")
        .count();
    assert_eq!(vault_entries, 1);
}

#[test]
fn team_t2_explicit_broadcast_command_has_golden() {
    let root = temp_dir("broadcast");
    seed_team(&root);
    assert_stdout_golden(
        "broadcast",
        &root,
        &["team", "broadcast", "alpha", "Explicit", "broadcast"],
        include_str!("fixtures/native-team-t2-comms/team-broadcast.stdout"),
    );
}

#[test]
fn team_t2_comms_rejects_injection_before_writing_inbox() {
    let root = temp_dir("guard");
    seed_team(&root);
    let bad_target = run(&["team", "send", "alpha", "-bad", "hello"], &root);
    assert!(!bad_target.status.success());
    assert!(String::from_utf8_lossy(&bad_target.stderr).contains("leading dash rejected"));

    let bad_message = run(&["team", "send", "alpha", "builder", "-oops"], &root);
    assert!(!bad_message.status.success());
    assert!(String::from_utf8_lossy(&bad_message.stderr).contains("unsafe team message"));
    assert!(!root
        .join("home/.claude/teams/alpha/inboxes/builder.json")
        .exists());
}
