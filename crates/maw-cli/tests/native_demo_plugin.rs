use maw_cli::{dispatcher_status, DispatchKind};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("maw-rs-demo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("temp bin");
    fs::create_dir_all(root.join("repo")).expect("temp repo");
    let plugin = root.join("plugins/demo");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-demo/demo-plugin/plugin.json"),
    )
    .expect("plugin manifest");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-demo/demo-plugin/plugin.wasm"),
    )
    .expect("plugin wasm");
    root
}

fn command(root: &Path) -> Command {
    let mut command = Command::new(bin());
    command
        .current_dir(root.join("repo"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", root.join("plugins"))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        );
    command
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    String::from_utf8(output.stdout).expect("stdout")
}

fn install_fake_tmux(root: &Path) {
    fs::write(
        root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  display-message) printf '%s\n' '%0' ;;
  list-panes)
    count=$(grep -c '^split-window ' "$MAW_FAKE_TMUX_LOG" || true)
    printf '%s\n' '%0'
    [ "$count" -ge 1 ] && printf '%s\n' '%1'
    [ "$count" -ge 2 ] && printf '%s\n' '%2'
    exit 0
    ;;
  split-window) : ;;
  kill-pane) : ;;
  *) echo "unexpected tmux command: $*" >&2; exit 9 ;;
esac
"#,
    )
    .expect("fake tmux");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("bin/tmux");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

#[test]
fn demo_plugin_no_tmux_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("no-tmux");
    let output = command(&root)
        .args(["demo", "--fast"])
        .output()
        .expect("run demo");
    assert_eq!(
        assert_success(output),
        include_str!("fixtures/native-demo/no-tmux.stdout")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn demo_plugin_fast_showcase_uses_managed_tmux_argv_and_cleanup() {
    let root = temp_dir("tmux");
    install_fake_tmux(&root);
    let output = command(&root)
        .env("TMUX", "/tmp/tmux-103,1,0")
        .env("TMUX_PANE", "%0")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .args(["demo", "--fast"])
        .output()
        .expect("run demo");
    let stdout = assert_success(output);
    let log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        stdout.contains("agent-1 spawned (%1)"),
        "{stdout}\nlog:\n{log}"
    );
    assert!(
        stdout.contains("agent-2 spawned (%2)"),
        "{stdout}\nlog:\n{log}"
    );
    assert!(stdout.contains("COST REPORT — demo session"), "{stdout}");
    assert!(log.contains("display-message -p #{pane_id}"), "{log}");
    assert!(
        log.contains("split-window -t %0 -h -l 50% bash -lc"),
        "{log}"
    );
    assert!(
        log.contains("split-window -t %1 -v -l 50% bash -lc"),
        "{log}"
    );
    assert!(log.contains("kill-pane -t %2"), "{log}");
    assert!(log.contains("kill-pane -t %1"), "{log}");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn demo_native_dispatcher_registration_is_removed() {
    assert_eq!(dispatcher_status("demo"), DispatchKind::NativeError);
}
