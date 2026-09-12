#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn talkto_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn talkto_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn talkto_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-talkto-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn talkto_install_fake_tmux(root: &Path) {
    talkto_write(
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
  list-panes)
    case "$*" in
      *'#{pane_id}'*) printf '%s\n' '%42' ;;
      *'#{pane_current_command}'*) printf '%s\n' 'claude' ;;
      *) echo "unexpected list-panes args: $*" >&2; exit 9 ;;
    esac
    ;;
  display-message)
    printf '%s\n' '0'
    ;;
  capture-pane)
    printf '%s\n' '$ '
    ;;
  load-buffer)
    # Real tmux `load-buffer -` consumes stdin, so the parent's write_all always
    # lands. This fake must too. With an empty arm the shell exits as soon as it
    # has logged the line; if the parent is descheduled for the ~1ms that takes,
    # its write hits a closed pipe -> EPIPE -> send_text fails -> talk-to prints
    # "thread #N updated" with exit 0, so the status assert passes and the
    # "+ sent -> %42" assert reddens with no code change (#875). Drain with a
    # shell builtin -- `cat` is not on the test's restricted PATH. Same scar and
    # same fix as native_assign_plugin.rs (bc8e27fb).
    while read -r _; do :; done
    ;;
  paste-buffer)
    ;;
  send-keys)
    ;;
  split-window|select-layout|kill-pane|notify-send|osascript|curl|ssh)
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

fn talkto_write_config(root: &Path, json: &str) {
    talkto_write(&root.join("xdg-config/maw/maw.config.json"), json);
    talkto_write(&root.join("home/.maw/config/maw.config.json"), json);
}

fn talkto_write_local_config(root: &Path) {
    talkto_write_config(
        root,
        r#"{"node":"bigboy-vps","oracle":"gm-bo","agents":{"nova":"local"}}"#,
    );
}

fn talkto_write_peer_config(root: &Path) {
    talkto_write_config(
        root,
        r#"{"node":"bigboy-vps","oracle":"gm-bo","namedPeers":[{"name":"remote","url":"http://remote.invalid"}]}"#,
    );
}

fn talkto_command(root: &Path) -> Command {
    let mut command = Command::new(talkto_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-83,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_SENDER", "bigboy-vps:08-gm-bo")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("CLAUDE_AGENT_NAME", "codex-4")
        .env("CLAUDE_SESSION_ID", "sess-talkto")
        .env("HOSTNAME", "test-host")
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn talkto_native_local_thread_notification_sends_to_guarded_pane() {
    let root = talkto_temp("local");
    talkto_install_fake_tmux(&root);
    talkto_write_local_config(&root);
    let output = talkto_command(&root)
        .args(["talk-to", "local:nova", "hello", "there"])
        .output()
        .expect("run talk-to");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("thread #1 + sent → %42"), "{stdout}");
    assert_eq!(dispatcher_status("talk-to"), DispatchKind::Native);
    assert_eq!(dispatcher_status("talkto"), DispatchKind::Native);
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(log.contains("list-windows -a -F"), "{log}");
    assert!(
        log.contains("list-panes -t 13-nova:0 -F #{pane_id}"),
        "{log}"
    );
    assert!(
        log.contains("list-panes -t %42 -F #{pane_current_command}"),
        "{log}"
    );
    assert!(log.contains("load-buffer -"), "{log}");
    assert!(log.contains("paste-buffer -t %42"), "{log}");
    assert!(log.contains("send-keys -t %42 Enter"), "{log}");
    assert!(log.contains("capture-pane -t %42 -e -p -J -S -80"), "{log}");
    let state_log =
        std::fs::read_to_string(root.join("home/.maw/maw-log.jsonl")).expect("state log");
    assert!(state_log.contains(r#""ch":"thread:1""#), "{state_log}");
    let thread_file = root.join("home/.maw/threads/1.json");
    let thread = std::fs::read_to_string(thread_file).expect("thread file");
    assert!(
        thread.contains(r#""title": "channel:local:nova""#),
        "{thread}"
    );
    assert!(thread.contains(r#""content": "hello there""#), "{thread}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn talkto_native_peer_uses_fake_transport_without_tmux_injection() {
    let root = talkto_temp("peer");
    talkto_install_fake_tmux(&root);
    talkto_write_peer_config(&root);
    let output = talkto_command(&root)
        .env("MAW_RS_TALKTO_FAKE_PEER_LOG", root.join("peer.jsonl"))
        .args(["talk-to", "remote:neo", "cross", "node", "--force"])
        .output()
        .expect("run peer talk-to");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("thread #1 + sent → remote:neo"), "{stdout}");
    let peer = std::fs::read_to_string(root.join("peer.jsonl")).expect("peer log");
    assert!(
        peer.contains(r#""peerUrl":"http://remote.invalid""#),
        "{peer}"
    );
    assert!(peer.contains(r#""target":"neo""#), "{peer}");
    assert!(peer.contains("cross node"), "{peer}");
    let thread =
        std::fs::read_to_string(root.join("home/.maw/threads/1.json")).expect("thread file");
    assert!(
        thread.contains(r#""title": "channel:remote:neo""#),
        "{thread}"
    );
    let tmux = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !tmux.contains("send-keys"),
        "peer path must not inject locally: {tmux}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn talkto_native_guard_rejects_separator_before_transport() {
    let root = talkto_temp("guard");
    talkto_install_fake_tmux(&root);
    talkto_write_local_config(&root);
    let output = talkto_command(&root)
        .env("MAW_RS_TALKTO_FAKE_PEER_LOG", root.join("peer.jsonl"))
        .args(["talk-to", "--", "nova", "msg"])
        .output()
        .expect("run guard");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("-- separator is not supported"));
    assert!(!root.join("home/.maw/threads/1.json").exists());
    let tmux = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !tmux.contains("send-keys"),
        "guard should fail before injection: {tmux}"
    );
    assert!(!root.join("peer.jsonl").exists());
    let _ = std::fs::remove_dir_all(root);
}
