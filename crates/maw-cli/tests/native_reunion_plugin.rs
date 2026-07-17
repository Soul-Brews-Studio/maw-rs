#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn reunion_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn reunion_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-reunion-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn reunion_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn reunion_install_fake_tmux(root: &Path, cwd: &Path) {
    reunion_write(
        &root.join("bin/tmux"),
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  display-message)
    printf '%s\n' '{}'
    ;;
  list-sessions)
    printf '%s\n' '77-mawjs'
    ;;
  list-windows)
    printf '%s\n' '77-mawjs|||3|||Work|||1|||{}'
    ;;
  send-keys|split-window|select-layout|kill-pane|ssh|curl)
    echo "unexpected mutating command: $*" >&2
    exit 44
    ;;
  *)
    echo "unexpected tmux command: $*" >&2
    exit 9
    ;;
esac
"#,
            cwd.display(),
            cwd.display()
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(root.join("bin/tmux"))
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(root.join("bin/tmux"), permissions).expect("chmod");
    }
}

fn reunion_command(root: &Path) -> Command {
    let mut command = Command::new(reunion_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-112,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn reunion_native_syncs_new_memory_without_overwriting() {
    let root = reunion_temp("sync");
    let worktree = root.join("worktree");
    let main = root.join("main-oracle");
    reunion_install_fake_tmux(&root, &worktree);
    reunion_write(&worktree.join("ψ/memory/learnings/new.md"), "new");
    reunion_write(&worktree.join("ψ/memory/learnings/existing.md"), "worktree");
    reunion_write(&worktree.join("ψ/memory/traces/nested/trace.md"), "trace");
    reunion_write(&main.join("ψ/memory/learnings/existing.md"), "main");
    let common = main.join(".git");
    let output = reunion_command(&root)
        .args([
            "reunion",
            "--git-common-dir",
            common.to_str().expect("utf8"),
        ])
        .output()
        .expect("run reunion");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("reunion: synced 1 learnings, 1 traces"),
        "{stdout}"
    );
    assert_eq!(dispatcher_status("reunion"), DispatchKind::Native);
    assert_eq!(
        std::fs::read_to_string(main.join("ψ/memory/learnings/new.md")).expect("new"),
        "new"
    );
    assert_eq!(
        std::fs::read_to_string(main.join("ψ/memory/learnings/existing.md")).expect("existing"),
        "main"
    );
    assert_eq!(
        std::fs::read_to_string(main.join("ψ/memory/traces/nested/trace.md")).expect("trace"),
        "trace"
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        log.contains("display-message -p #{pane_current_path}"),
        "{log}"
    );
    assert!(
        !log.contains("send-keys"),
        "reunion must not inject panes: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reunion_native_named_window_resolves_case_insensitively_and_reports_empty() {
    let root = reunion_temp("window");
    let worktree = root.join("worktree");
    let main = root.join("main-oracle");
    reunion_install_fake_tmux(&root, &worktree);
    std::fs::create_dir_all(worktree.join("ψ/memory/learnings")).expect("worktree psi");
    std::fs::create_dir_all(main.join(".git")).expect("main git");
    let output = reunion_command(&root)
        .args([
            "reunion",
            "work",
            "--git-common-dir",
            main.join(".git").to_str().expect("utf8"),
        ])
        .output()
        .expect("run reunion window");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("nothing new to sync to main"), "{stdout}");
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(log.contains("list-windows -a -F"), "{log}");
    assert!(
        log.contains("display-message -t 77-mawjs:Work -p #{pane_current_path}"),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reunion_native_guard_rejects_separator_before_tmux_or_git() {
    let root = reunion_temp("guard");
    reunion_install_fake_tmux(&root, &root.join("worktree"));
    let output = reunion_command(&root)
        .args(["reunion", "--", "work"])
        .output()
        .expect("run reunion guard");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("-- separator is not supported"));
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("display-message") && !log.contains("send-keys"),
        "guard should fail before cwd/git/mutation: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}
