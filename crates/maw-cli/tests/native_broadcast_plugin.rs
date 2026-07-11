use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn broadcast_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn broadcast_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn broadcast_chmod(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn broadcast_seed(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-broadcast-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    broadcast_write(&root.join("CLAUDE.md"), "test repo\n");
    broadcast_write(
        &config.join("fleet/01-alpha.json"),
        r#"{"name":"01-alpha","squadName":"alpha","windows":[{"name":"neo-oracle"}]}"#,
    );
    broadcast_write(
        &home.join(".claude/teams/tk/config.json"),
        r#"{"members":[{"name":"neo"},{"name":"team-lead","role":"lead"}]}"#,
    );
    broadcast_write(
        &root.join("ψ/memory/mailbox/teams/tk/manifest.json"),
        r#"{"members":["extra-oracle"],"charter":{"members":[{"role":"neo"}]}}"#,
    );
    broadcast_write(
        &bin.join("tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$BROADCAST_TMUX_LOG"
case "$1" in
  display-message)
    if [ "$2" = "-p" ]; then printf 'sender-window\n'; exit 0; fi
    case "$3" in
      01-alpha:0) printf 'codex\n' ;;
      01-alpha:1) printf 'bash\n' ;;
      *) exit 7 ;;
    esac
    ;;
  list-sessions)
    printf '01-alpha\n02-beta\n99-overview\n'
    ;;
  list-windows)
    printf '01-alpha|||0|||neo-oracle|||1|||/tmp\n01-alpha|||1|||shell|||0|||/tmp\n02-beta|||0|||beta-oracle|||0|||/tmp\n99-overview|||0|||watch|||0|||/tmp\n'
    ;;
  send-keys)
    exit 0
    ;;
  *) exit 64 ;;
esac
"#,
    );
    broadcast_chmod(&bin.join("tmux"));
    (root, home, config)
}

fn broadcast_command(root: &Path, home: &Path, config: &Path) -> Command {
    let plugins = root.join("plugins");
    let plugin = plugins.join("broadcast");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-broadcast/broadcast-plugin");
    std::fs::create_dir_all(&plugin).expect("plugin dir");
    std::fs::copy(fixture.join("plugin.json"), plugin.join("plugin.json")).expect("manifest");
    std::fs::copy(fixture.join("plugin.wasm"), plugin.join("plugin.wasm")).expect("wasm");
    let mut command = Command::new(broadcast_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugins)
        .env("BROADCAST_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn broadcast_plugin_session_golden_is_hermetic_without_js_ref() {
    let (root, home, config) = broadcast_seed("session");
    let output = broadcast_command(&root, &home, &config)
        .args(["broadcast", "hello", "fleet", "--session", "01-alpha"])
        .output()
        .expect("run broadcast");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-broadcast/session.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        log.contains("send-keys -t 01-alpha:0 -l [broadcast from sender-window] hello fleet"),
        "{log}"
    );
    assert!(!log.contains("send-keys -t 01-alpha:1"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn broadcast_plugin_registers_team_fleet_and_blocks_option_injection() {
    let (root, home, config) = broadcast_seed("scope");
    assert_eq!(dispatcher_status("broadcast"), DispatchKind::NativeError);
    let output = broadcast_command(&root, &home, &config)
        .args(["broadcast", "hi", "--team", "tk", "--fleet", "alpha"])
        .output()
        .expect("run scoped broadcast");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .expect("stdout")
        .contains("[scope: team=tk, fleet=alpha]"));

    let guarded = broadcast_command(&root, &home, &config)
        .args(["broadcast", "hi", "--session", "-Sbad"])
        .output()
        .expect("run guard");
    assert!(!guarded.status.success());
    assert_eq!(String::from_utf8(guarded.stderr).expect("stderr"), "--session requires a value\nusage: maw broadcast [--dry-run] [--session <name>] [--team <name>] [--fleet <name>] [--] <message>\n");
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(!log.contains("-Sbad"), "guarded target reached tmux: {log}");
    let _ = std::fs::remove_dir_all(root);
}
