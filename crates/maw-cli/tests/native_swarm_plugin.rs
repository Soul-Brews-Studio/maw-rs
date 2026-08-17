#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn swarm_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn swarm_temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-swarm-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn swarm_fake_command(home: &Path) -> Command {
    let mut command = Command::new(swarm_bin());
    command
        .env("HOME", home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SWARM_FAKE_TMUX", "1")
        .env("MAW_RS_SWARM_FAKE_NOW", "1000")
        .env("TMUX", "fake-tmux-socket")
        .env("TMUX_PANE", "%leader");
    command
}

#[test]
fn swarm_default_matches_committed_golden_without_ref_checkout() {
    let root = swarm_temp_dir("default");
    let output = swarm_fake_command(&root.join("home"))
        .arg("swarm")
        .output()
        .expect("run swarm");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-swarm/swarm-default.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");

    let config = fs::read_to_string(root.join("home/.claude/teams/swarm/config.json"))
        .expect("swarm config");
    let value: serde_json::Value = serde_json::from_str(&config).expect("json");
    assert_eq!(value["name"], "swarm");
    assert_eq!(value["description"], "Multi-AI swarm");
    assert_eq!(value["createdAt"], 1000);
    assert_eq!(value["members"].as_array().expect("members").len(), 3);
    assert_eq!(value["members"][0]["name"], "claude-1");
    assert_eq!(value["members"][0]["tmuxPaneId"], "%pane1");
}

#[test]
fn swarm_tiled_positional_matches_committed_golden_without_ref_checkout() {
    let root = swarm_temp_dir("tiled");
    let output = swarm_fake_command(&root.join("home"))
        .args(["swarm", "codex", "opencode", "--tiled"])
        .env_remove("TMUX_PANE")
        .output()
        .expect("run swarm tiled");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-swarm/swarm-tiled.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn swarm_help_matches_committed_golden_without_ref_checkout() {
    let output = Command::new(swarm_bin())
        .args(["swarm", "--help"])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("TMUX", "fake-tmux-socket")
        .output()
        .expect("run swarm help");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-swarm/swarm-help.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn swarm_tmux_family_guards_run_before_host_tmux() {
    let root = swarm_temp_dir("guards");
    let log = root.join("tmux.log");
    let missing_tmux = Command::new(swarm_bin())
        .args(["swarm"])
        .env("HOME", root.join("home"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SWARM_FAKE_TMUX", "1")
        .env("MAW_RS_SWARM_FAKE_LOG", &log)
        .env_remove("TMUX")
        .output()
        .expect("missing tmux");
    assert!(!missing_tmux.status.success());
    assert!(String::from_utf8(missing_tmux.stderr)
        .expect("stderr")
        .contains("swarm requires tmux"));
    assert!(
        !log.exists(),
        "fake tmux should not be called before TMUX guard"
    );

    let worktree_flag = Command::new(swarm_bin())
        .args(["swarm", "--wt"])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("TMUX", "fake-tmux-socket")
        .output()
        .expect("worktree flag");
    assert!(!worktree_flag.status.success());
    let stderr = String::from_utf8(worktree_flag.stderr).expect("stderr");
    assert!(stderr.contains("unknown flag for swarm: --wt"));
    assert!(stderr.contains("maw work <repo> --wt <slot> -e <engine>"));
    assert!(!stderr.contains("maw wake"));
    assert!(!stderr.contains("--split"));
}

#[test]
fn swarm_rejects_flag_like_values_and_too_many_agents() {
    let root = swarm_temp_dir("reject");
    let bad_count = swarm_fake_command(&root.join("home"))
        .args(["swarm", "--count", "-1"])
        .output()
        .expect("bad count");
    assert!(!bad_count.status.success());
    assert!(String::from_utf8(bad_count.stderr)
        .expect("stderr")
        .contains("positive integer"));

    let too_many = swarm_fake_command(&root.join("home"))
        .args([
            "swarm", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k",
        ])
        .output()
        .expect("too many");
    assert!(!too_many.status.success());
    assert!(String::from_utf8(too_many.stderr)
        .expect("stderr")
        .contains("max 10"));
}

#[test]
fn swarm_count_spawns_default_claude_agents_without_treating_count_as_agent() {
    let root = swarm_temp_dir("count");
    let output = swarm_fake_command(&root.join("home"))
        .args(["swarm", "--count", "2"])
        .output()
        .expect("count swarm");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("claude-1 (Claude Code)"));
    assert!(stdout.contains("claude-2 (Claude Code)"));
    assert!(!stdout.contains("2-1"));
    assert!(stdout.contains("swarm: 2 agents (main-vertical)"));
}

#[test]
fn swarm_dispatch_registers_part117_native() {
    assert_eq!(
        maw_cli::dispatcher_status("swarm"),
        maw_cli::DispatchKind::Native
    );
}
