#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn notify_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn notify_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn notify_temp(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-notify-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    std::fs::create_dir_all(root.join("psi/inbox")).expect("inbox");
    notify_write(
        &root.join("xdg-config/maw/maw.config.json"),
        &format!(
            r#"{{"node":"bigboy-vps","oracle":"gm-bo","psiPath":"{}","agents":{{"nova":"local"}}}}"#,
            root.join("psi").display()
        ),
    );
    root
}

fn notify_install_fake_tmux(root: &Path) {
    notify_write(
        &root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  list-sessions)
    printf '%s\n' '13-nova'
    ;;
  list-windows)
    printf '%s\n' '13-nova|||0|||nova-oracle|||1|||/tmp'
    ;;
  send-keys|display-message|split-window|select-layout|kill-pane|notify-send|osascript)
    echo "unexpected mutating tmux/notifier command: $*" >&2
    exit 44
    ;;
  *)
    echo "unexpected tmux command: $*" >&2
    exit 9
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

fn notify_command(root: &Path) -> Command {
    let mut command = Command::new(notify_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-80,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_SENDER", "bigboy-vps:08-gm-bo")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn notify_native_local_writes_inbox_only_without_pane_injection() {
    let root = notify_temp("local");
    notify_install_fake_tmux(&root);
    let output = notify_command(&root)
        .args(["notify", "local:nova", "routine", "done"])
        .output()
        .expect("run notify");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("queued inbox nova"), "{stdout}");
    assert_eq!(dispatcher_status("notify"), DispatchKind::Native);
    let entries = std::fs::read_dir(root.join("psi/inbox"))
        .expect("inbox")
        .collect::<Result<Vec<_>, _>>()
        .expect("entries");
    assert_eq!(entries.len(), 1);
    let body = std::fs::read_to_string(entries[0].path()).expect("message");
    assert!(body.contains("from: bigboy-vps:08-gm-bo"), "{body}");
    assert!(body.contains("to: nova"), "{body}");
    assert!(body.contains("routine done"), "{body}");
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(log.contains("list-windows -a -F"), "{log}");
    assert!(
        !log.contains("send-keys"),
        "notify must not pane-inject: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn notify_native_force_warns_but_stays_inbox_only() {
    let root = notify_temp("force");
    notify_install_fake_tmux(&root);
    let output = notify_command(&root)
        .args([
            "notify",
            "--force",
            "--from",
            "relay:bot",
            "nova",
            "heads",
            "up",
        ])
        .output()
        .expect("run notify force");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("--force is not meaningful for notify"),
        "{stdout}"
    );
    let entries = std::fs::read_dir(root.join("psi/inbox"))
        .expect("inbox")
        .collect::<Result<Vec<_>, _>>()
        .expect("entries");
    let body = std::fs::read_to_string(entries[0].path()).expect("message");
    assert!(body.contains("from: relay:bot"), "{body}");
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("send-keys"),
        "notify must not pane-inject: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn notify_native_guard_rejects_separator_before_tmux_or_notifier() {
    let root = notify_temp("guard");
    notify_install_fake_tmux(&root);
    let output = notify_command(&root)
        .args(["notify", "--", "nova", "msg"])
        .output()
        .expect("run notify guard");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("-- separator is not supported"));
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("send-keys"),
        "guard must not pane-inject: {log}"
    );
    assert!(
        !log.contains("notify-send"),
        "guard must not spawn notifier: {log}"
    );
    assert!(
        !log.contains("osascript"),
        "guard must not spawn notifier: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}
