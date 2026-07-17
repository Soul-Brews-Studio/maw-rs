#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn forwarderror_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn forwarderror_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn forwarderror_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-forwarderror-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn forwarderror_install_fake_tmux(root: &Path) {
    forwarderror_write(
        &root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  capture-pane)
    printf '%s\n' 'line 1' 'error: boom'
    ;;
  list-sessions)
    printf '%s\n' '13-nova'
    ;;
  list-windows)
    printf '%s\n' '13-nova|||0|||nova-oracle|||1|||/tmp'
    ;;
  send-keys)
    ;;
  display-message|split-window|select-layout|kill-pane|notify-send|osascript|curl|ssh)
    echo "unexpected mutating transport/notifier command: $*" >&2
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

fn forwarderror_write_config(root: &Path, json: &str) {
    forwarderror_write(&root.join("xdg-config/maw/maw.config.json"), json);
    forwarderror_write(&root.join("home/.maw/config/maw.config.json"), json);
}

fn forwarderror_command(root: &Path) -> Command {
    let mut command = Command::new(forwarderror_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-85,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_SENDER", "bigboy-vps:08-gm-bo")
        .env("MAW_LAST_EXIT_CODE", "42")
        .env("MAW_RS_FORWARDERROR_NOW", "2026-06-08T02:03:04.000Z")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn forwarderror_native_local_captures_and_delivers_json() {
    let root = forwarderror_temp("local");
    forwarderror_install_fake_tmux(&root);
    forwarderror_write_config(
        &root,
        r#"{"node":"bigboy-vps","oracle":"gm-bo","agents":{"nova":"local"},"errorForward":{"target":"local:nova"}}"#,
    );
    let output = forwarderror_command(&root)
        .args(["forward-error", "--last", "12"])
        .output()
        .expect("run forward-error");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("forwarded last 12 line(s) to local:nova"),
        "{stdout}"
    );
    assert_eq!(dispatcher_status("forward-error"), DispatchKind::Native);
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(log.contains("capture-pane -p -S -12"), "{log}");
    assert!(log.contains("list-windows -a -F"), "{log}");
    assert!(
        log.contains("send-keys -t 13-nova:0 -l [bigboy-vps:gm-bo]"),
        "{log}"
    );
    assert!(log.contains(r#""error":"line 1"#), "{log}");
    assert!(log.contains("error: boom"), "{log}");
    assert!(log.contains(r#""exitCode":42"#), "{log}");
    assert!(
        log.contains(r#""timestamp":"2026-06-08T02:03:04.000Z"#),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn forwarderror_native_peer_uses_fake_transport() {
    let root = forwarderror_temp("peer");
    forwarderror_install_fake_tmux(&root);
    forwarderror_write_config(
        &root,
        r#"{"node":"bigboy-vps","oracle":"gm-bo","namedPeers":[{"name":"remote","url":"http://remote.invalid"}]}"#,
    );
    let output = forwarderror_command(&root)
        .env("MAW_RS_FORWARDERROR_FAKE_PEER_LOG", root.join("peer.jsonl"))
        .args(["forward-error", "--to", "remote:doctor", "--last=5"])
        .output()
        .expect("run peer forward-error");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("forwarded last 5 line(s) to remote:doctor"),
        "{stdout}"
    );
    let peer = std::fs::read_to_string(root.join("peer.jsonl")).expect("peer log");
    assert!(
        peer.contains(r#""peerUrl":"http://remote.invalid""#),
        "{peer}"
    );
    assert!(peer.contains(r#""target":"doctor""#), "{peer}");
    assert!(peer.contains("error: boom"), "{peer}");
    let tmux = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !tmux.contains("send-keys"),
        "peer path must not inject locally: {tmux}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn forwarderror_native_guard_rejects_before_capture_or_transport() {
    let root = forwarderror_temp("guard");
    forwarderror_install_fake_tmux(&root);
    forwarderror_write_config(
        &root,
        r#"{"node":"bigboy-vps","oracle":"gm-bo","agents":{"nova":"local"}}"#,
    );
    let output = forwarderror_command(&root)
        .env("MAW_RS_FORWARDERROR_FAKE_PEER_LOG", root.join("peer.jsonl"))
        .args(["forward-error", "--", "local:nova"])
        .output()
        .expect("run guard");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("-- separator is not supported"));
    let tmux = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !tmux.contains("capture-pane"),
        "guard should fail before capture: {tmux}"
    );
    assert!(!root.join("peer.jsonl").exists());
    let _ = std::fs::remove_dir_all(root);
}
