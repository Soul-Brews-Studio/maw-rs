#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-assign-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn write_fake_gh(bin_dir: &Path) {
    let gh = bin_dir.join("gh");
    fs::write(
        &gh,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_ASSIGN_GH_LOG"
printf '{"title":"port assign native","body":"Implement assign.","labels":[{"name":"P1"}]}'
"#,
    )
    .expect("write fake gh");
    chmod_exec(&gh);
}

fn write_fake_maw(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        r#"#!/bin/sh
printf 'DELEGATED-MAW %s\n' "$*" >> "$MAW_ASSIGN_WAKE_LOG"
exit 37
"#,
    )
    .expect("write fake maw");
    chmod_exec(&maw);
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    fs::write(
        &tmux,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_ASSIGN_TMUX_LOG"
case "$1" in
  display-message)
    printf 'detected-oracle-task\n'
    ;;
  list-windows|capture-pane)
    printf ''
    ;;
  load-buffer)
    # Real tmux `load-buffer -` consumes stdin. Drain it with a shell builtin
    # (`cat` is not on the test's restricted PATH) so the parent's write_all
    # never races into a closed pipe (was a flaky EPIPE / status 127).
    while read -r _; do :; done
    ;;
  *)
    exit 0
    ;;
esac
"#,
    )
    .expect("write fake tmux");
    chmod_exec(&tmux);
}

fn seed_config(root: &Path) {
    let config = root.join("xdg-config").join("maw");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.json"),
        r#"{"node":"ci","oracle":"seed"}"#,
    )
    .expect("seed config");
}

fn run(root: &Path, args: &[&str], with_tmux: bool) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");

    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_ASSIGN_GH_LOG", root.join("gh.log"))
        .env("MAW_ASSIGN_WAKE_LOG", root.join("wake.log"))
        .env("MAW_ASSIGN_PROMPT_LOG", root.join("prompt.log"))
        .env("MAW_ASSIGN_TMUX_LOG", root.join("tmux.log"));
    if with_tmux {
        command.env("TMUX", root.join("tmux-socket"));
    }
    command.output().expect("run maw-rs")
}

#[test]
fn native_assign_explicit_oracle_matches_committed_golden_and_is_hermetic() {
    let root = temp_dir("explicit");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_gh(&bin_dir);
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);
    seed_config(&root);
    fs::create_dir_all(root.join("ghq/github.com/tonkmac/maw-rs")).expect("repo path");

    let output = run(
        &root,
        &[
            "assign",
            "https://github.com/tonkmac/maw-rs/issues/127",
            "--oracle",
            "nova",
        ],
        false,
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.starts_with("\u{1b}[36m⚡\u{1b}[0m fetching issue #127 from tonkmac/maw-rs...\n")
    );
    assert!(stdout.contains("woke 'nova-issue-127'"), "{stdout}");
    assert!(stdout.contains("ghq/github.com/tonkmac/maw-rs"), "{stdout}");
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("gh.log")).expect("gh log"),
        "issue view 127 --repo tonkmac/maw-rs --json title,body,labels\n"
    );
    assert!(
        !root.join("wake.log").exists(),
        "must not delegate to PATH maw"
    );
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(tmux_log.contains("has-session -t "));
    assert!(tmux_log.contains("new-window -t "));
    assert!(tmux_log.contains("paste-buffer -t "));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_assign_detects_oracle_from_isolated_tmux_and_rejects_injection() {
    let root = temp_dir("detect");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_gh(&bin_dir);
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);
    seed_config(&root);
    fs::create_dir_all(root.join("ghq/github.com/tonkmac/maw-rs")).expect("repo path");

    assert_eq!(dispatcher_status("assign"), DispatchKind::Native);

    let output = run(
        &root,
        &["assign", "https://github.com/tonkmac/maw-rs/issues/127"],
        true,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        tmux_log
            .starts_with("list-sessions -F #{session_name}\ndisplay-message -p #{window_name}\n"),
        "{tmux_log}"
    );
    assert!(tmux_log.contains("has-session -t "));
    assert!(tmux_log.contains("new-window -t "));
    assert!(
        !root.join("wake.log").exists(),
        "must not delegate to PATH maw"
    );

    let bad = run(
        &root,
        &[
            "assign",
            "https://github.com/tonkmac/maw-rs/issues/127",
            "--oracle",
            "-bad",
        ],
        false,
    );
    assert!(!bad.status.success());
    assert!(String::from_utf8(bad.stderr)
        .expect("stderr")
        .contains("--oracle requires a value")
        .not());
    fs::remove_dir_all(root).expect("cleanup");
}

trait BoolNot {
    fn not(self) -> bool;
}

impl BoolNot for bool {
    fn not(self) -> bool {
        !self
    }
}
