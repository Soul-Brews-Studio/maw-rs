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
    let path = std::env::temp_dir().join(format!("maw-rs-team-enter-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed_team(root: &Path) {
    let team_dir = root.join("home/.claude/teams/alpha");
    fs::create_dir_all(&team_dir).expect("team dir");
    fs::write(
        team_dir.join("config.json"),
        r#"{
  "name":"alpha",
  "members":[
    {"name":"builder","agentId":"builder@alpha","tmuxPaneId":"%11"},
    {"name":"reviewer","tmuxPaneId":"%12"},
    {"name":"lead","agentType":"team-lead","tmuxPaneId":"%13"},
    {"name":"offline"}
  ],
  "createdAt":1
}
"#,
    )
    .expect("config");
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
    run_with_fake(args, root, &[])
}

fn run_with_fake(args: &[&str], root: &Path, fake_env: &[(&str, &str)]) -> std::process::Output {
    let fake_bin = write_fake_maw(root);
    let fake_log = root.join("tmux.jsonl");
    let mut command = Command::new(bin());
    command
        .args(args)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_TEAM", "alpha")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_ENTER_FAKE_TMUX_LOG", &fake_log)
        .env("MAW_RS_TEAM_TMUX_PANES", "alpha|builder|codex|/tmp|%11\nalpha|reviewer|claude|/tmp|%12\nalpha|lead|codex|/tmp|%13")
        .env("PATH", format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap_or_default()));
    for (key, value) in fake_env {
        command.env(key, value);
    }
    command.output().expect("run maw-rs")
}

fn assert_stdout_golden(name: &str, root: &Path, args: &[&str], expected: &str) -> String {
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
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "{name} delegated via stdout"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "{name} delegated via stderr"
    );
    assert_eq!(stderr, "");
    fs::read_to_string(root.join("tmux.jsonl")).expect("tmux log")
}

fn assert_stderr_golden(root: &Path, fake_key: &str, expected: &str) -> String {
    let output = run_with_fake(
        &["team", "send-enter", "all", "hello", "team"],
        root,
        &[(fake_key, "%12")],
    );
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), expected);
    fs::read_to_string(root.join("tmux.jsonl")).expect("tmux log")
}

#[test]
fn team_enter_and_send_enter_are_native_and_argv_tmux() {
    let root = temp_dir("golden");
    seed_team(&root);

    let enter_log = assert_stdout_golden(
        "enter-builder",
        &root,
        &["team", "enter", "builder"],
        include_str!("fixtures/native-team-enter/team-enter-builder.stdout"),
    );
    assert!(
        enter_log.matches(r#"["-t","%11","Enter"]"#).count() == 1,
        "{enter_log}"
    );

    fs::remove_file(root.join("tmux.jsonl")).expect("reset log");
    let send_log = assert_stdout_golden(
        "send-enter-all",
        &root,
        &["team", "send-enter", "all", "hello", "team"],
        include_str!("fixtures/native-team-enter/team-send-enter-all.stdout"),
    );
    assert!(
        send_log.contains(
            r#"{"args":["-t","%11","-l","hello team"],"command":"send-keys","program":"tmux"}"#
        ),
        "{send_log}"
    );
    assert!(
        send_log.contains(
            r#"{"args":["-t","%12","-l","hello team"],"command":"send-keys","program":"tmux"}"#
        ),
        "{send_log}"
    );
    assert!(
        !send_log.contains("%13"),
        "team lead must not receive enter: {send_log}"
    );
}

#[test]
fn team_send_enter_buffers_501_bytes_via_stdin() {
    let root = temp_dir("buffer");
    seed_team(&root);
    let payload = "x".repeat(501);
    let output = run(&["team", "send-enter", "builder", payload.as_str()], &root);
    assert!(output.status.success());
    let log = fs::read_to_string(root.join("tmux.jsonl")).expect("tmux log");
    assert!(log.contains(r#""command":"load-buffer""#));
    assert!(log.contains(r#""stdinBytes":501"#));
    assert!(log.contains(r#""command":"paste-buffer""#));
    assert!(!log.contains(r#""-l""#));
}

#[test]
fn team_send_enter_preflights_all_before_payload_mutation() {
    let root = temp_dir("pending");
    seed_team(&root);
    let pending_log = assert_stderr_golden(
        &root,
        "MAW_RS_TEAM_ENTER_FAKE_PENDING_PANE",
        include_str!("fixtures/native-team-enter/team-send-enter-pending.stderr"),
    );
    assert!(!pending_log.contains(r#""command":"send-keys""#));
    assert!(!pending_log.contains("load-buffer"));
    assert!(!pending_log.contains("paste-buffer"));
    for pane in ["%11", "%12"] {
        assert!(pending_log.contains(&format!(r#"["-t","{pane}","-e","-p","-J","-S","-80"]"#)));
    }
}

#[test]
fn team_send_enter_reports_prior_confirmation_when_next_is_unconfirmed() {
    let root = temp_dir("partial");
    seed_team(&root);
    let partial_log = assert_stderr_golden(
        &root,
        "MAW_RS_TEAM_ENTER_FAKE_DIFFERENT_PANE",
        include_str!("fixtures/native-team-enter/team-send-enter-partial.stderr"),
    );
    assert!(partial_log.contains(r#"["-t","%11","-l","hello team"]"#));
    assert!(partial_log.contains(r#"["-t","%12","-l","hello team"]"#));
    assert_eq!(partial_log.matches(r#"["-t","%12","Enter"]"#).count(), 1);
}

#[test]
fn team_enter_rejects_injection_and_missing_pane_before_send() {
    let root = temp_dir("guards");
    seed_team(&root);

    let bad_member = run(&["team", "enter", "-bad"], &root);
    assert!(!bad_member.status.success());
    assert!(String::from_utf8_lossy(&bad_member.stderr).contains("leading dash rejected"));
    assert!(
        !root.join("tmux.jsonl").exists(),
        "should reject before tmux"
    );

    let bad_text = run(&["team", "send-enter", "builder", "-bad"], &root);
    assert!(!bad_text.status.success());
    assert!(String::from_utf8_lossy(&bad_text.stderr).contains("invalid team text"));

    let missing = run(&["team", "enter", "offline"], &root);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found or no pane ID"));
}
