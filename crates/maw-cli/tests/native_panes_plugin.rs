#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn panes_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn panes_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn panes_temp(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("maw-rs-native-panes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bin")).expect("temp");
    root
}

fn panes_install_fake_tmux(root: &Path) {
    panes_write(
        &root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  list-sessions)
    if [ "$2" != "-F" ]; then
      echo "unexpected list-sessions argv: $*" >&2
      exit 8
    fi
    printf '%s\n' 'alpha-main' 'alpha-side' 'beta'
    ;;
  list-panes)
    if [ "$2" = "-t" ]; then
      if [ "$3" != "alpha-main" ] || [ "$4" != "-F" ]; then
        echo "unexpected target list-panes argv: $*" >&2
        exit 9
      fi
      printf '%s\n' 'alpha-main:0.0|||120x40|||zsh|||lead|||111' 'alpha-main:1.0|||80x24|||codex|||worker|||222'
    elif [ "$2" = "-a" ]; then
      if [ "$3" != "-F" ]; then
        echo "unexpected all list-panes argv: $*" >&2
        exit 10
      fi
      printf '%s\n' 'alpha-main:0.0|||120x40|||zsh|||lead' 'beta:0.0|||90x30|||bash|||beta'
    elif [ "$2" = "-F" ]; then
      printf '%s\n' 'current:0.0|||100x30|||zsh|||current'
    else
      echo "unexpected list-panes argv: $*" >&2
      exit 11
    fi
    ;;
  *)
    echo "unexpected tmux command: $*" >&2
    exit 7
    ;;
esac
"#,
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

fn panes_command(root: &Path) -> Command {
    let mut command = Command::new(panes_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-98,1,0")
        .env("TMUX_PANE", "%7")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn panes_native_pid_target_resolves_session_and_formats_table() {
    let root = panes_temp("pid");
    panes_install_fake_tmux(&root);
    let output = panes_command(&root)
        .args(["panes", "alpha-main", "--pid"])
        .output()
        .expect("run panes");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "  \u{1b}[90mTARGET          SIZE    PID  COMMAND  TITLE\u{1b}[0m\n  alpha-main:0.0  120x40  111  zsh      \u{1b}[90mlead\u{1b}[0m\n  alpha-main:1.0  80x24   222  codex    \u{1b}[90mworker\u{1b}[0m\n"
    );
    assert_eq!(dispatcher_status("panes"), DispatchKind::Native);
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(log.contains("list-sessions -F #{session_name}"), "{log}");
    assert!(log.contains("list-panes -t alpha-main -F #{session_name}:#{window_index}.#{pane_index}|||#{pane_width}x#{pane_height}|||#{pane_current_command}|||#{pane_title}|||#{pane_pid}"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn panes_native_all_ignores_target_without_session_resolution() {
    let root = panes_temp("all");
    panes_install_fake_tmux(&root);
    let output = panes_command(&root)
        .args(["panes", "alpha-main", "--all"])
        .output()
        .expect("run panes all");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.starts_with("  \u{1b}[90m⚠ --all ignores target argument\u{1b}[0m\n"),
        "{stdout}"
    );
    assert!(stdout.contains("alpha-main:0.0"), "{stdout}");
    assert!(stdout.contains("beta:0.0"), "{stdout}");
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(log.contains("list-panes -a -F #{session_name}:#{window_index}.#{pane_index}|||#{pane_width}x#{pane_height}|||#{pane_current_command}|||#{pane_title}"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn panes_native_guard_rejects_leading_dash_before_tmux_listing() {
    let root = panes_temp("guard");
    panes_install_fake_tmux(&root);
    let output = panes_command(&root)
        .args(["panes", "-target"])
        .output()
        .expect("run panes guard");
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        "\"-target\" looks like a flag, not a target.\n  usage: maw panes [target] [--pid] [--all|-a]  (see: maw pane swap, maw tile)\n"
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("list-panes"),
        "guard must not list panes: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}
