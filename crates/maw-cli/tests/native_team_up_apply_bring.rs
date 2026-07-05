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
    let path = std::env::temp_dir().join(format!("maw-rs-team-t5-prb-{name}-{stamp}"));
    fs::create_dir_all(path.join(".git")).expect("git marker");
    fs::create_dir_all(path.join("home")).expect("home");
    fs::create_dir_all(path.join("maw-home/teams/alpha")).expect("maw team");
    fs::create_dir_all(path.join("ψ/teams")).expect("psi teams");
    fs::create_dir_all(path.join("agents/builder")).expect("builder");
    fs::create_dir_all(path.join("agents/reviewer")).expect("reviewer");
    fs::create_dir_all(path.join("builder")).expect("bring builder");
    fs::create_dir_all(path.join("reviewer")).expect("bring reviewer");
    seed_charter(&path, "alpha.yaml");
    fs::write(
        path.join("maw-home/teams/alpha/oracle-members.json"),
        r#"{"name":"alpha","members":[{"oracle":"builder","role":"builder"},{"oracle":"reviewer","role":"reviewer"}]}"#,
    ).expect("oracle registry");
    path
}

fn seed_charter(root: &Path, file: &str) -> PathBuf {
    let path = root.join("ψ/teams").join(file);
    fs::write(
        &path,
        r"name: alpha
description: Alpha team
goal: Ship PR-B
members:
  - role: builder
    name: builder
    engine: codex
    cwd: agents/builder
  - role: reviewer
    name: reviewer
    engine: claude
    cwd: agents/reviewer
",
    )
    .expect("charter");
    path
}

fn run(args: &[&str], root: &Path, log: Option<&Path>) -> std::process::Output {
    run_with_panes(
        args,
        root,
        log,
        "alpha|reviewer|bash|/repo/agents/reviewer|%2\nalpha|live|codex|/repo/live|%3",
    )
}

fn run_with_panes(
    args: &[&str],
    root: &Path,
    log: Option<&Path>,
    panes: &str,
) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SELF_BIN", "/fake/maw")
        .env("MAW_RS_TEAM_TMUX_PANES", panes);
    if let Some(log) = log {
        cmd.env("MAW_RS_TEAM_FAKE_TMUX_LOG", log);
    }
    cmd.output().expect("run maw-rs")
}

fn normalize(root: &Path, bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .expect("utf8")
        .replace(&root.display().to_string(), "<ROOT>")
}

#[test]
fn team_t5b_dry_run_zero_tmux_mutation() {
    let root = temp_dir("dry");
    let log = root.join("tmux.jsonl");
    let output = run(
        &["team", "up", "alpha", "--dry-run", "--session", "alpha"],
        &root,
        Some(&log),
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!log.exists(), "dry-run must not call tmux");
}

#[test]
fn team_t5b_up_exec_uses_fixed_maw_send_keys_literal_and_resume_sequence() {
    let root = temp_dir("up");
    let log = root.join("tmux.jsonl");
    let output = run(
        &["team", "up", "alpha", "--session", "alpha"],
        &root,
        Some(&log),
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize(&root, output.stdout),
        include_str!("fixtures/native-team-t5-prb/team-up-exec.stdout")
    );
    let log_text = normalize(
        &root,
        fs::read_to_string(&log).expect("tmux log").into_bytes(),
    );
    assert!(log_text.contains(r#""args":["new-window","-t","alpha:","-n","builder"]"#));
    assert!(log_text.contains(r#""args":["send-keys","-t","%2","C-u"]"#));
    assert!(log_text.contains(r#""send-keys","-t","%2","-l","--","'/fake/maw' 'wake' 'reviewer'"#));
    assert!(log_text.contains(r#""send-keys","-t","%2","Enter"]"#));
}

#[test]
fn team_t5b_up_new_window_targets_exact_session_when_names_prefix_overlap() {
    let root = temp_dir("prefix");
    let log = root.join("tmux.jsonl");
    let output = run_with_panes(
        &["team", "up", "alpha", "--session", "188-maw-rs"],
        &root,
        Some(&log),
        "maw-rs|maw-rs-oracle|codex|/repo/maw-rs|%9\n188-maw-rs|reviewer|bash|/repo/agents/reviewer|%2",
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log_text = normalize(
        &root,
        fs::read_to_string(&log).expect("tmux log").into_bytes(),
    );
    assert!(log_text.contains(r#""args":["new-window","-t","188-maw-rs:","-n","builder"]"#));
    assert!(!log_text.contains(r#""args":["new-window","-t","188-maw-rs","-n","builder"]"#));
    assert!(log_text.contains(r#""send-keys","-t","188-maw-rs:builder""#));
}

#[test]
fn team_t5b_apply_only_mutates_with_apply_and_uses_fake_tmux() {
    let root = temp_dir("apply");
    let log = root.join("tmux.jsonl");
    let dry = run(
        &["team", "apply", "alpha", "--session", "alpha"],
        &root,
        Some(&log),
    );
    assert!(dry.status.success());
    assert!(!log.exists(), "apply dry-run default must not call tmux");
    let live = run(
        &["team", "apply", "alpha", "--session", "alpha", "--apply"],
        &root,
        Some(&log),
    );
    assert!(
        live.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert_eq!(
        normalize(&root, live.stdout),
        include_str!("fixtures/native-team-t5-prb/team-apply-exec.stdout")
    );
    assert!(fs::read_to_string(&log)
        .expect("tmux log")
        .contains("send-keys"));
}

#[test]
fn team_t5b_bring_exec_wakes_registry_members() {
    let root = temp_dir("bring");
    let log = root.join("tmux.jsonl");
    let output = run(
        &["team", "bring", "alpha", "--session", "alpha"],
        &root,
        Some(&log),
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize(&root, output.stdout),
        include_str!("fixtures/native-team-t5-prb/team-bring-exec.stdout")
    );
    let log_text = fs::read_to_string(&log).expect("tmux log");
    assert!(log_text.contains("builder"));
    assert!(log_text.contains("reviewer"));
}

#[test]
fn team_t5b_metachar_member_rejected_before_runner_and_quote_helper_covers_quotes() {
    let root = temp_dir("inject");
    let bad = root.join("ψ/teams/bad.yaml");
    fs::write(
        &bad,
        "name: bad\nmembers:\n  - role: \"bad'$(touch pwn)\"\n    cwd: agents/builder\n",
    )
    .expect("bad charter");
    let log = root.join("tmux.jsonl");
    let output = run(
        &["team", "up", "bad", "--session", "alpha"],
        &root,
        Some(&log),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("metacharacter rejected"));
    assert!(!log.exists(), "bad member must reject before tmux runner");
}
